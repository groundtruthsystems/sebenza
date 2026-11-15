//! The zero-tool gate for handover chat (track sebenza_polishing, FR-17,
//! FR-18, NFR-01).
//!
//! Chat runs `claude` with no tools at all. That is structural, built by one
//! constructor ([`chat_run_input`]), and then *proven*: a capability probe
//! runs the same argv with a turn that asks for a tool and passes only when
//! the CLI reports zero tools and no tool call appears. The probe is the only
//! thing that sets `enabled`. It runs at start and whenever `claude --version`
//! changes; anything it cannot prove fails closed. A runtime guard watches
//! every chat turn and, on any tool activity, aborts the turn, discards its
//! output and turns chat off until the next passing probe. There is no
//! fallback that runs chat with tools.

use crate::services::agent_stream::{
    AgentStreamManager, DraftMessage, StartRunInput, StreamEvent, StreamProvider, ToolPolicy,
    claude_args,
};
use common::domain::config::ChatConfig;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Every built-in tool name, denied explicitly on top of `--tools ""`, so an
/// older CLI that ignored the empty set would still have nothing to run.
pub const BUILTIN_DENY: &[&str] = &[
    "Agent",
    "AskUserQuestion",
    "Bash",
    "BashOutput",
    "Edit",
    "ExitPlanMode",
    "Glob",
    "Grep",
    "KillShell",
    "LS",
    "Monitor",
    "MultiEdit",
    "NotebookEdit",
    "Read",
    "Skill",
    "SlashCommand",
    "Task",
    "TodoWrite",
    "ToolSearch",
    "WebFetch",
    "WebSearch",
    "Workflow",
    "Write",
];

/// The probe's turn: it asks for a tool, so a CLI that has one would use it.
pub const PROBE_PROMPT: &str =
    "Use a tool to list the files in the current directory, then reply with the word done.";

/// How long a cached `claude --version` is trusted before it is re-read.
const VERSION_TTL: Duration = Duration::from_secs(60);
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);

/// What a chat run needs; everything else is fixed by [`chat_run_input`].
pub struct ChatRun {
    pub binary: String,
    pub cwd: PathBuf,
    pub conversation_id: String,
    pub prompt: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub timeout: Duration,
}

/// The only way a chat (or probe) `StartRunInput` is built. Claude, isolated,
/// an allowlisted environment with no Sebenza token, an empty working
/// directory, no resume, a timeout, a non-permissive permission mode, no
/// hooks, no MCP servers, an empty allow list, every built-in denied and the
/// built-in set emptied with `--tools ""`.
pub fn chat_run_input(run: ChatRun) -> StartRunInput {
    StartRunInput {
        provider: StreamProvider::Claude,
        conversation_id: run.conversation_id,
        cwd: run.cwd.to_string_lossy().to_string(),
        prompt: run.prompt,
        env: chat_env(std::env::vars()),
        permission_mode: Some("dontAsk".to_string()),
        resume_session_id: None,
        system_prompt: Some(run.system_prompt),
        binary: Some(run.binary),
        model: run.model,
        tools: Some(ToolPolicy {
            allowed: Vec::new(),
            disallowed: BUILTIN_DENY.iter().map(|s| s.to_string()).collect(),
            strict_mcp: true,
            settings: Some(serde_json::json!({ "disableAllHooks": true }).to_string()),
            max_turns: None,
            builtin: Some(String::new()),
        }),
        isolated: true,
        timeout: Some(run.timeout),
    }
}

/// The child environment: the system agent's allowlist (what the CLI needs to
/// find itself and its credentials), never anything `SEBENZA_*`.
pub fn chat_env<I>(parent: I) -> HashMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    crate::services::system_agent::isolation::child_env(parent)
}

/// A message that shows tool activity: a tool call or a tool result.
pub fn is_tool_activity(m: &DraftMessage) -> bool {
    m.kind == "toolUse" || m.kind == "toolResult" || m.tool_name.is_some()
}

/// Judge a probe run's raw stream-json output. Passes only when the `init`
/// line lists zero tools (and zero MCP servers), no tool call or tool result
/// appears anywhere, and the run ended with a non-error result.
pub fn judge_probe(stdout: &str) -> Result<(), String> {
    let mut init_seen = false;
    let mut result: Option<Result<(), String>> = None;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let sub = v.get("subtype").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "system" if sub == "init" => {
                init_seen = true;
                match v.get("tools").and_then(|t| t.as_array()) {
                    Some(tools) if tools.is_empty() => {}
                    Some(tools) => {
                        return Err(format!("the CLI reports {} available tool(s)", tools.len()));
                    }
                    None => return Err("the CLI did not report its tools".to_string()),
                }
                if v.get("mcp_servers")
                    .and_then(|m| m.as_array())
                    .is_some_and(|m| !m.is_empty())
                {
                    return Err("the CLI reports MCP servers".to_string());
                }
            }
            "assistant" | "user" => {
                let content = v
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                if content.iter().any(|c| {
                    matches!(
                        c.get("type").and_then(|t| t.as_str()),
                        Some("tool_use" | "server_tool_use" | "tool_result")
                    )
                }) {
                    return Err("a tool call appeared in the probe run".to_string());
                }
            }
            "stream_event" => {
                if matches!(
                    v.pointer("/event/content_block/type")
                        .and_then(|t| t.as_str()),
                    Some("tool_use" | "server_tool_use")
                ) {
                    return Err("a tool call appeared in the probe run".to_string());
                }
            }
            "result" => {
                let is_error = v.get("is_error").and_then(|e| e.as_bool()) == Some(true);
                result = Some(if is_error {
                    Err(format!(
                        "the probe run failed: {}",
                        v.get("result").and_then(|r| r.as_str()).unwrap_or("error")
                    ))
                } else {
                    Ok(())
                });
            }
            _ => {}
        }
    }
    if !init_seen {
        return Err("the CLI did not report its tools".to_string());
    }
    result.unwrap_or_else(|| Err("the probe run did not finish".to_string()))
}

/// What the gate knows, for the chat routes and the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GateStatus {
    pub enabled: bool,
    pub cli_version: Option<String>,
    pub binary: Option<String>,
    pub probed_at: Option<String>,
    /// Why chat is off, when it is.
    pub reason: Option<String>,
}

struct Inner {
    status: GateStatus,
    version_checked: Option<Instant>,
}

/// Why a chat turn did not produce an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnError {
    /// Chat is off (probe failed, stale, or a guard trip): 503.
    Unavailable(String),
    /// The guard saw tool activity; the turn was aborted, output discarded.
    ToolActivity,
    Failed(String),
}

pub struct ChatGate {
    cfg: ChatConfig,
    scratch_root: PathBuf,
    inner: RwLock<Inner>,
    /// One probe at a time; turns that find it held get 503.
    probing: tokio::sync::Mutex<()>,
}

impl ChatGate {
    /// Off until a probe passes (TS-49).
    pub fn new(cfg: ChatConfig, scratch_root: PathBuf) -> Self {
        Self {
            cfg,
            scratch_root,
            inner: RwLock::new(Inner {
                status: GateStatus {
                    enabled: false,
                    cli_version: None,
                    binary: None,
                    probed_at: None,
                    reason: Some("the capability probe has not run".to_string()),
                },
                version_checked: None,
            }),
            probing: tokio::sync::Mutex::new(()),
        }
    }

    pub fn config(&self) -> &ChatConfig {
        &self.cfg
    }

    pub fn status(&self) -> GateStatus {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }

    pub fn enabled(&self) -> bool {
        self.status().enabled
    }

    /// Turn chat off (guard trip, probe error). Never turns it on.
    pub fn disable(&self, reason: &str) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if inner.status.enabled {
            tracing::warn!(reason, "chat: disabled");
        }
        inner.status.enabled = false;
        inner.status.reason = Some(reason.to_string());
    }

    /// The `claude` to run: the configured path, else `claude` resolved on
    /// `PATH` to an absolute path (pinned for the probe and every turn).
    pub fn resolve_binary(&self) -> Option<String> {
        if let Some(b) = &self.cfg.binary {
            return Some(b.clone());
        }
        let path = std::env::var("PATH").ok()?;
        std::env::split_paths(&path)
            .map(|d| d.join("claude"))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().to_string())
    }

    /// `<binary> --version`, or `None` if it cannot be read in time.
    pub async fn read_version(binary: &str) -> Option<String> {
        let out = tokio::time::timeout(
            VERSION_TIMEOUT,
            tokio::process::Command::new(binary)
                .arg("--version")
                .env_clear()
                .envs(chat_env(std::env::vars()))
                .kill_on_drop(true)
                .output(),
        )
        .await
        .ok()?
        .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|v| !v.is_empty())
    }

    /// Run the capability probe. The only writer of `enabled = true`.
    pub async fn probe(&self) -> bool {
        let _probing = self.probing.lock().await;
        self.probe_locked().await
    }

    async fn probe_locked(&self) -> bool {
        let Some(binary) = self.resolve_binary() else {
            self.record(None, None, Err("claude is not installed".to_string()));
            return false;
        };
        let Some(version) = Self::read_version(&binary).await else {
            self.record(
                Some(binary),
                None,
                Err("claude --version failed".to_string()),
            );
            return false;
        };
        let verdict = self.run_probe(&binary).await;
        let pass = verdict.is_ok();
        self.record(Some(binary), Some(version), verdict);
        pass
    }

    fn record(&self, binary: Option<String>, version: Option<String>, verdict: Result<(), String>) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.version_checked = Some(Instant::now());
        inner.status.binary = binary;
        inner.status.cli_version = version.clone();
        inner.status.probed_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        match verdict {
            Ok(()) => {
                inner.status.enabled = true;
                inner.status.reason = None;
                tracing::info!(version = ?version, "chat: zero-tool probe passed");
            }
            Err(reason) => {
                inner.status.enabled = false;
                tracing::warn!(version = ?version, reason, "chat: zero-tool probe failed; chat is off");
                inner.status.reason = Some(reason);
            }
        }
    }

    /// Spawn the probe with exactly the chat argv, prompt on stdin.
    async fn run_probe(&self, binary: &str) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        let cwd = fresh_scratch(&self.scratch_root, "probe").map_err(|e| e.to_string())?;
        let input = chat_run_input(ChatRun {
            binary: binary.to_string(),
            cwd: cwd.clone(),
            conversation_id: "chat-probe".to_string(),
            prompt: PROBE_PROMPT.to_string(),
            system_prompt: "You are a capability probe.".to_string(),
            model: self.cfg.model.clone(),
            timeout: PROBE_TIMEOUT,
        });
        let mut child = tokio::process::Command::new(binary)
            .args(claude_args(&input))
            .current_dir(&cwd)
            .env_clear()
            .envs(&input.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("could not start claude: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.prompt.as_bytes()).await;
        }
        let out = tokio::time::timeout(PROBE_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| "the probe timed out".to_string())?
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&cwd);
        judge_probe(&String::from_utf8_lossy(&out.stdout))
    }

    /// Before a turn: re-read `claude --version` when the cached one is older
    /// than a minute, and re-probe when it changed or was never probed. Chat is
    /// off (503) while a probe runs and whenever this cannot prove it current.
    pub async fn ensure_current(&self) -> bool {
        let (due, known, binary) = {
            let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
            (
                inner
                    .version_checked
                    .is_none_or(|t| t.elapsed() >= VERSION_TTL),
                inner.status.cli_version.clone(),
                inner.status.binary.clone(),
            )
        };
        if !due {
            return self.enabled();
        }
        let Ok(_probing) = self.probing.try_lock() else {
            return false;
        };
        let binary = binary.or_else(|| self.resolve_binary());
        let now = match &binary {
            Some(b) => Self::read_version(b).await,
            None => None,
        };
        if now.is_some() && now == known {
            self.inner
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .version_checked = Some(Instant::now());
            return self.enabled();
        }
        tracing::info!(before = ?known, now = ?now, "chat: claude version changed; re-probing");
        self.probe_locked().await
    }

    /// Run one chat turn under the runtime guard. Any tool call or tool result
    /// aborts the turn, discards whatever it wrote and turns chat off.
    pub async fn run_guarded_turn(
        &self,
        streams: &AgentStreamManager,
        input: StartRunInput,
    ) -> Result<String, TurnError> {
        if !self.enabled() {
            return Err(TurnError::Unavailable(
                self.status()
                    .reason
                    .unwrap_or_else(|| "chat unavailable".into()),
            ));
        }
        let conversation = input.conversation_id.clone();
        streams.start_run(input).map_err(TurnError::Failed)?;
        let Some(sub) = streams.subscribe(&conversation) else {
            return Err(TurnError::Failed("the chat run did not start".into()));
        };
        let mut texts: Vec<(String, String)> = Vec::new();
        let mut error: Option<String> = None;
        let mut on_event = |ev: StreamEvent| -> Option<Result<(), TurnError>> {
            match ev {
                StreamEvent::Upsert { message, .. } if is_tool_activity(&message) => {
                    Some(Err(TurnError::ToolActivity))
                }
                StreamEvent::Upsert { message, .. }
                    if message.role == "assistant" && message.kind == "text" =>
                {
                    match texts.iter_mut().find(|(id, _)| id == &message.id) {
                        Some(t) => t.1 = message.text,
                        None => texts.push((message.id, message.text)),
                    }
                    None
                }
                StreamEvent::Error { message } => {
                    error = Some(message);
                    None
                }
                StreamEvent::Status { running: false, .. } => Some(Ok(())),
                _ => None,
            }
        };
        let mut verdict = None;
        for ev in sub.replay {
            if let Some(v) = on_event(ev) {
                verdict = Some(v);
                break;
            }
        }
        let mut rx = sub.receiver;
        while verdict.is_none() {
            match rx.recv().await {
                Ok(ev) => verdict = on_event(ev),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => verdict = Some(Ok(())),
            }
        }
        if let Some(Err(TurnError::ToolActivity)) = verdict {
            streams.interrupt(&conversation);
            self.disable("tool activity in a chat turn");
            tracing::error!(
                conversation = %conversation,
                "chat: runtime guard tripped on tool activity; turn aborted, output discarded"
            );
            return Err(TurnError::ToolActivity);
        }
        if let Some(e) = error {
            return Err(TurnError::Failed(e));
        }
        let answer = texts
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if answer.trim().is_empty() {
            return Err(TurnError::Failed("the agent gave no answer".into()));
        }
        Ok(answer)
    }

    /// A fresh, empty working directory for one turn.
    pub fn scratch(&self, name: &str) -> std::io::Result<PathBuf> {
        fresh_scratch(&self.scratch_root, name)
    }
}

fn fresh_scratch(root: &Path, name: &str) -> std::io::Result<PathBuf> {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dir = root.join(format!("{safe}-{}", common::util::id::random_ulid()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn temp(name: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "sebenza-chatgate-{name}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A fake `claude`: `--version` prints `<dir>/version`; anything else
    /// appends a line to `<dir>/calls`, records its argv, and prints
    /// `<dir>/out.jsonl`.
    fn fake_claude(dir: &Path, version: &str, out: &[&str]) -> String {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("version"), version).unwrap();
        std::fs::write(dir.join("out.jsonl"), out.join("\n") + "\n").unwrap();
        let script = dir.join("claude");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nD='{}'\nif [ \"$1\" = \"--version\" ]; then cat \"$D/version\"; exit 0; fi\necho run >> \"$D/calls\"\necho \"$@\" > \"$D/argv\"\nenv > \"$D/env\"\ncat > /dev/null\ncat \"$D/out.jsonl\"\n",
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().to_string()
    }

    const INIT_NONE: &str =
        r#"{"type":"system","subtype":"init","session_id":"s","tools":[],"mcp_servers":[]}"#;
    const INIT_BASH: &str =
        r#"{"type":"system","subtype":"init","session_id":"s","tools":["Bash"],"mcp_servers":[]}"#;
    const TEXT: &str = r#"{"type":"assistant","message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"I have no tools. done"}]},"session_id":"s"}"#;
    const TOOL_USE: &str = r#"{"type":"assistant","message":{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]},"session_id":"s"}"#;
    const RESULT_OK: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"s"}"#;
    const RESULT_AUTH: &str = r#"{"type":"result","subtype":"error","is_error":true,"result":"Invalid API key · Please run /login","session_id":"s"}"#;

    fn gate(dir: &Path, binary: &str) -> ChatGate {
        ChatGate::new(
            ChatConfig {
                binary: Some(binary.to_string()),
                ..ChatConfig::default()
            },
            dir.join("scratch"),
        )
    }

    // TS-21 / NFR-01: the one chat constructor.
    #[test]
    fn the_chat_run_has_zero_tools_by_construction() {
        // SAFETY: test-only; no other thread reads this variable.
        unsafe { std::env::set_var("SEBENZA_CONTROL_TOKEN", "secret-token") };
        let input = chat_run_input(ChatRun {
            binary: "/usr/bin/claude".into(),
            cwd: PathBuf::from("/tmp/empty"),
            conversation_id: "chat:01ID:acme".into(),
            prompt: "q".into(),
            system_prompt: "role".into(),
            model: None,
            timeout: Duration::from_secs(120),
        });
        assert!(input.provider == StreamProvider::Claude);
        assert!(input.isolated);
        assert_eq!(input.timeout, Some(Duration::from_secs(120)));
        assert!(input.resume_session_id.is_none());
        assert_eq!(input.permission_mode.as_deref(), Some("dontAsk"));
        assert!(
            input.env.keys().all(|k| !k.starts_with("SEBENZA_")),
            "no Sebenza token"
        );
        let args = claude_args(&input);
        let after = |flag: &str| {
            let i = args
                .iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("{flag}"));
            args[i + 1].clone()
        };
        assert_eq!(after("--tools"), "", "every built-in disabled");
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
        assert!(!args.iter().any(|a| a == "--mcp-config"));
        assert!(
            !args.iter().any(|a| a == "--allowedTools"),
            "empty allow list"
        );
        for t in ["Bash", "Read", "Write", "Edit", "WebFetch", "Task"] {
            assert!(
                after("--disallowedTools").split(',').any(|d| d == t),
                "{t} denied"
            );
        }
        assert!(after("--settings").contains("\"disableAllHooks\":true"));
        assert!(
            !args
                .iter()
                .any(|a| a == "bypassPermissions" || a == "acceptEdits")
        );
        assert!(!args.iter().any(|a| a == "-r"));
    }

    // TS-50: the other callers' argv is unchanged (no `--tools`).
    #[test]
    fn other_callers_do_not_get_the_builtin_flag() {
        let policies = [
            crate::services::role_runner::tool_policy("claude"),
            crate::services::system_agent::isolation::read_only_tools(),
        ];
        for p in policies {
            assert!(p.builtin.is_none());
            let input = StartRunInput {
                tools: Some(p),
                ..chat_run_input(ChatRun {
                    binary: "c".into(),
                    cwd: PathBuf::from("/tmp"),
                    conversation_id: "x".into(),
                    prompt: "p".into(),
                    system_prompt: "s".into(),
                    model: None,
                    timeout: Duration::from_secs(1),
                })
            };
            assert!(!claude_args(&input).iter().any(|a| a == "--tools"));
        }
    }

    // TS-19 (judge): pass only with zero tools, no tool call and a clean result.
    #[test]
    fn the_probe_verdict() {
        let j = |lines: &[&str]| judge_probe(&lines.join("\n"));
        assert_eq!(j(&[INIT_NONE, TEXT, RESULT_OK]), Ok(()));
        assert!(
            j(&[INIT_BASH, TEXT, RESULT_OK]).is_err(),
            "a tool is available"
        );
        assert!(j(&[INIT_NONE, TOOL_USE, RESULT_OK]).is_err(), "a tool call");
        assert!(
            j(&[INIT_NONE, r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","name":"Bash"}}}"#, RESULT_OK]).is_err(),
            "a streamed tool call"
        );
        assert!(j(&[TEXT, RESULT_OK]).is_err(), "no init line");
        assert!(j(&["not json", INIT_NONE]).is_err(), "no result");
        assert!(j(&[INIT_NONE, RESULT_AUTH]).is_err(), "an error result");
        assert!(
            j(&[
                r#"{"type":"system","subtype":"init","tools":[],"mcp_servers":[{"name":"x"}]}"#,
                RESULT_OK
            ])
            .is_err(),
            "an MCP server"
        );
    }

    // TS-49: chat starts off.
    #[test]
    fn chat_is_off_until_a_probe_passes() {
        let g = ChatGate::new(ChatConfig::default(), temp("boot"));
        assert!(!g.enabled());
        assert!(g.status().reason.is_some());
    }

    // TS-19 (gate): the probe enables chat only on a pass, and fails closed.
    #[tokio::test]
    async fn the_probe_alone_enables_chat_and_fails_closed() {
        let dir = temp("probe");
        let ok = fake_claude(&dir, "2.1.0", &[INIT_NONE, TEXT, RESULT_OK]);
        let g = gate(&dir, &ok);
        assert!(g.probe().await);
        assert!(g.enabled());
        assert_eq!(g.status().cli_version.as_deref(), Some("2.1.0"));
        let argv = std::fs::read_to_string(dir.join("argv")).unwrap();
        assert!(
            argv.contains("--tools  --strict-mcp-config") || argv.contains("--tools"),
            "{argv}"
        );
        let env = std::fs::read_to_string(dir.join("env")).unwrap();
        assert!(
            !env.contains("SEBENZA_"),
            "no Sebenza variable reaches the CLI"
        );

        for bad in [
            vec![INIT_BASH, TEXT, RESULT_OK],
            vec![INIT_NONE, TOOL_USE, RESULT_OK],
            vec!["garbage"],
        ] {
            let d = temp("probe-bad");
            let g = gate(&d, &fake_claude(&d, "2.1.0", &bad));
            assert!(!g.probe().await);
            assert!(!g.enabled());
        }
        let g = gate(&dir, "/nonexistent/claude");
        assert!(!g.probe().await, "a missing CLI fails closed");
    }

    // TS-20: a version change re-probes; an unauthenticated CLI fails closed.
    #[tokio::test]
    async fn a_version_change_re_probes() {
        let dir = temp("version");
        let bin = fake_claude(&dir, "2.1.0", &[INIT_NONE, TEXT, RESULT_OK]);
        let g = gate(&dir, &bin);
        assert!(g.probe().await);
        let calls = || {
            std::fs::read_to_string(dir.join("calls"))
                .unwrap_or_default()
                .lines()
                .count()
        };
        assert_eq!(calls(), 1);
        // Within the TTL nothing is re-read.
        assert!(g.ensure_current().await);
        assert_eq!(calls(), 1);
        // Expire the cache, change the version, and make the CLI unauthenticated.
        g.inner.write().unwrap().version_checked = None;
        std::fs::write(dir.join("version"), "2.2.0").unwrap();
        std::fs::write(
            dir.join("out.jsonl"),
            format!("{INIT_NONE}\n{RESULT_AUTH}\n"),
        )
        .unwrap();
        assert!(!g.ensure_current().await, "re-probed and failed closed");
        assert_eq!(calls(), 2);
        assert_eq!(g.status().cli_version.as_deref(), Some("2.2.0"));
        assert!(g.status().reason.unwrap().contains("Invalid API key"));
    }

    fn turn_input(g: &ChatGate, bin: &str) -> StartRunInput {
        chat_run_input(ChatRun {
            binary: bin.to_string(),
            cwd: g.scratch("turn").unwrap(),
            conversation_id: format!("chat:test:{}", common::util::id::random_ulid()),
            prompt: "What does the handover change?".into(),
            system_prompt: "role".into(),
            model: None,
            timeout: Duration::from_secs(20),
        })
    }

    // TS-14: an injected tool call aborts the turn, discards the output and
    // turns chat off; the next turn is refused (503 at the route).
    #[tokio::test]
    async fn the_guard_aborts_on_tool_activity() {
        let dir = temp("guard");
        let bin = fake_claude(&dir, "2.1.0", &[INIT_NONE, TEXT, RESULT_OK]);
        let g = gate(&dir, &bin);
        assert!(g.probe().await);
        let streams = AgentStreamManager::new();
        assert_eq!(
            g.run_guarded_turn(&streams, turn_input(&g, &bin)).await,
            Ok("I have no tools. done".to_string())
        );
        std::fs::write(
            dir.join("out.jsonl"),
            format!("{INIT_NONE}\n{TEXT}\n{TOOL_USE}\n{RESULT_OK}\n"),
        )
        .unwrap();
        assert_eq!(
            g.run_guarded_turn(&streams, turn_input(&g, &bin)).await,
            Err(TurnError::ToolActivity)
        );
        assert!(!g.enabled());
        assert!(matches!(
            g.run_guarded_turn(&streams, turn_input(&g, &bin)).await,
            Err(TurnError::Unavailable(_))
        ));
    }

    // TS-12 (gate side): with chat off, a turn is refused without running.
    #[tokio::test]
    async fn a_disabled_gate_refuses_turns_without_running() {
        let dir = temp("off");
        let bin = fake_claude(&dir, "2.1.0", &[INIT_NONE, TEXT, RESULT_OK]);
        let g = gate(&dir, &bin);
        let streams = AgentStreamManager::new();
        assert!(matches!(
            g.run_guarded_turn(&streams, turn_input(&g, &bin)).await,
            Err(TurnError::Unavailable(_))
        ));
        assert!(!dir.join("calls").exists(), "the CLI never ran");
    }
}
