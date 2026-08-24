use crate::domain::config::{AutoNameConfig, AutoNameProvider};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEFAULT_CLAUDE_MODEL: &str = "claude-haiku-4-5-20251001";
// grok's own default. `grok models` lists grok-4.6 (default) and grok-4.5; there is no
// smaller/faster tier to pick for a naming call, so `--effort low` does that work instead.
const DEFAULT_GROK_MODEL: &str = "grok-4.6";

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// The CLI argv for a one-shot generation with the given prompts.
pub fn build_llm_args(
    config: &AutoNameConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Vec<String> {
    match config.provider {
        AutoNameProvider::Claude => vec![
            "claude".into(),
            "-p".into(),
            "--system-prompt".into(),
            system_prompt.into(),
            "--output-format".into(),
            "text".into(),
            "--no-session-persistence".into(),
            "--model".into(),
            config
                .model
                .clone()
                .unwrap_or_else(|| DEFAULT_CLAUDE_MODEL.to_string()),
            "--effort".into(),
            "low".into(),
            user_prompt.into(),
        ],
        // grok's headless mode. Mirrors claude's: `-p` carries the prompt (as the flag's
        // VALUE - grok does not read it from stdin), `--output-format plain` gives prose
        // rather than JSON, and `--rules` appends the system prompt. `--rules` and not
        // `--system-prompt-override`, which would replace grok's own system prompt.
        AutoNameProvider::Grok => vec![
            "grok".into(),
            "-p".into(),
            user_prompt.into(),
            "--output-format".into(),
            "plain".into(),
            "--rules".into(),
            system_prompt.into(),
            "--model".into(),
            config
                .model
                .clone()
                .unwrap_or_else(|| DEFAULT_GROK_MODEL.to_string()),
            "--effort".into(),
            "low".into(),
        ],
        // opencode's one-shot mode. `--format json` emits raw JSON events rather than
        // prose, so auto-naming reads plain output instead; `--agent`/`--model` carry the
        // model choice. Note there is NO system-prompt flag - opencode keeps system
        // instructions in its own agent/config files - so the system prompt is prepended
        // to the user prompt here. That is acceptable for a one-shot naming call, unlike
        // an interactive session where it would silently alter the transcript.
        AutoNameProvider::Opencode => {
            let mut args: Vec<String> = vec!["opencode".into(), "run".into()];
            if let Some(model) = &config.model {
                args.push("--model".into());
                args.push(model.clone());
            }
            args.push(format!("{system_prompt}\n\n{user_prompt}"));
            args
        }
        AutoNameProvider::Codex => {
            let mut args = vec![
                "codex".into(),
                "-c".into(),
                format!(
                    "developer_instructions=\"{}\"",
                    escape_toml_string(system_prompt)
                ),
                "exec".into(),
                "--ephemeral".into(),
            ];
            if let Some(model) = &config.model {
                args.push("-m".into());
                args.push(model.clone());
            }
            args.push(user_prompt.into());
            args
        }
    }
}

pub enum RunLlmResult {
    Ok {
        stdout: String,
    },
    Timeout,
    SpawnError,
    ExitNonzero {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
}

/// Spawn the LLM CLI, draining its pipes on background threads and killing it if
/// it outruns `timeout`. Blocking — call from `spawn_blocking`.
pub fn run_short_llm_task(
    config: &AutoNameConfig,
    system_prompt: &str,
    user_prompt: &str,
    timeout: Duration,
) -> RunLlmResult {
    let args = build_llm_args(config, system_prompt, user_prompt);
    let mut child = match Command::new(&args[0])
        .args(&args[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return RunLlmResult::SpawnError,
    };

    // Drain pipes on their own threads so a full buffer can't deadlock the child.
    let stdout_rx = drain(child.stdout.take());
    let stderr_rx = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_rx.recv().unwrap_or_default();
                let stderr = stderr_rx.recv().unwrap_or_default();
                let code = status.code().unwrap_or(-1);
                return if code == 0 {
                    RunLlmResult::Ok { stdout }
                } else {
                    RunLlmResult::ExitNonzero {
                        exit_code: code,
                        stdout,
                        stderr,
                    }
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    return RunLlmResult::Timeout;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return RunLlmResult::SpawnError,
        }
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = pipe.read_to_string(&mut buf);
            let _ = tx.send(buf);
        });
    } else {
        let _ = tx.send(String::new());
    }
    rx
}

pub fn llm_provider_label(config: &AutoNameConfig) -> &'static str {
    match config.provider {
        AutoNameProvider::Claude => "claude",
        AutoNameProvider::Grok => "grok",
        AutoNameProvider::Codex => "codex",
        AutoNameProvider::Opencode => "opencode",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: AutoNameProvider, model: Option<&str>) -> AutoNameConfig {
        AutoNameConfig {
            provider,
            model: model.map(str::to_string),
            system_prompt: None,
        }
    }

    #[test]
    fn claude_args_use_default_model_and_effort() {
        let args = build_llm_args(&config(AutoNameProvider::Claude, None), "sys", "user");
        assert_eq!(args[0], "claude");
        assert!(args.contains(&"--system-prompt".to_string()));
        assert!(args.contains(&DEFAULT_CLAUDE_MODEL.to_string()));
        assert!(args.contains(&"low".to_string()));
        assert_eq!(args.last().unwrap(), "user");
    }

    #[test]
    fn grok_auto_name_argv_passes_the_prompt_as_a_value_and_appends_the_system_prompt() {
        let args = build_llm_args(
            &config(AutoNameProvider::Grok, None),
            "be terse",
            "name this branch",
        );
        assert_eq!(args[0], "grok");
        // `grok -p` REQUIRES a value; unlike claude it does not read the prompt from stdin.
        let p = args.iter().position(|a| a == "-p").expect("-p present");
        assert_eq!(args[p + 1], "name this branch");
        // --rules appends; --system-prompt-override would replace grok's own system prompt.
        let r = args.iter().position(|a| a == "--rules").expect("--rules");
        assert_eq!(args[r + 1], "be terse");
        assert!(!args.iter().any(|a| a == "--system-prompt-override"));
        assert!(args.contains(&DEFAULT_GROK_MODEL.to_string()));
        assert_eq!(
            llm_provider_label(&config(AutoNameProvider::Grok, None)),
            "grok"
        );
    }

    #[test]
    fn opencode_args_use_run_and_fold_the_system_prompt_into_the_message() {
        let args = build_llm_args(
            &config(AutoNameProvider::Opencode, Some("google/gemini")),
            "sys",
            "name this",
        );
        assert_eq!(args[0], "opencode");
        assert_eq!(args[1], "run");
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"google/gemini".to_string()));
        // opencode has no system-prompt flag, so the two are folded into one message.
        // Acceptable for a one-shot naming call; it would NOT be for an interactive
        // session, where it would silently alter the transcript.
        let last = args.last().unwrap();
        assert!(
            last.contains("sys") && last.contains("name this"),
            "got {last}"
        );
        assert_eq!(
            llm_provider_label(&config(AutoNameProvider::Opencode, None)),
            "opencode"
        );
    }

    #[test]
    fn codex_args_embed_developer_instructions_and_model() {
        let args = build_llm_args(
            &config(AutoNameProvider::Codex, Some("gpt-x")),
            "sy\"s",
            "u",
        );
        assert_eq!(args[0], "codex");
        assert!(
            args.iter()
                .any(|a| a.contains("developer_instructions=\"sy\\\"s\""))
        );
        assert!(args.contains(&"-m".to_string()));
        assert!(args.contains(&"gpt-x".to_string()));
        assert!(args.contains(&"--ephemeral".to_string()));
    }
}
