//! Agent launch-command builders. Produces the shell command a
//! tmux pane runs to source the runtime env then exec the agent (claude/codex or
//! a custom template). Docker variants are deferred until the docker adapter lands.

use crate::services::agent_registry::{AgentDefinition, AgentImplementation, BuiltinAgentId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AgentLaunchMode {
    Fresh,
    Resume,
    Fork,
}

fn quote_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn runtime_bootstrap(runtime_env_path: &str) -> String {
    format!("set -a; . {}; set +a", quote_shell(runtime_env_path))
}

const DOCKER_PATH_FALLBACK: &str = "/root/.local/bin:/usr/local/bin:/root/.bun/bin:/root/.cargo/bin";

fn docker_runtime_bootstrap(runtime_env_path: &str) -> String {
    format!("{}; export PATH=\"$PATH:{DOCKER_PATH_FALLBACK}\"", runtime_bootstrap(runtime_env_path))
}

fn docker_exec_command(container: &str, worktree_path: &str, command: &str) -> String {
    format!(
        "docker exec -it -w {} {} /bin/sh -c {}",
        quote_shell(worktree_path),
        quote_shell(container),
        quote_shell(command)
    )
}

/// Agent pane command for a docker-runtime worktree: `docker exec` into the
/// container, source the runtime env (+ PATH fallback), then run the agent.
pub fn build_docker_agent_pane_command(
    container: &str,
    worktree_path: &str,
    runtime_env_path: &str,
    inv: &AgentInvocation,
) -> String {
    let inner = format!("{}; {}", docker_runtime_bootstrap(runtime_env_path), agent_invocation(inv));
    docker_exec_command(container, worktree_path, &inner)
}

/// Shell pane command for a docker-runtime worktree.
pub fn build_docker_shell_command(container: &str, worktree_path: &str, runtime_env_path: &str) -> String {
    let inner = format!(
        "{}; if [ -x '/bin/bash' ]; then exec '/bin/bash' -i; elif [ -x /bin/sh ]; then exec /bin/sh -i; else echo 'sebenza: no shell found in container' >&2; exit 127; fi",
        docker_runtime_bootstrap(runtime_env_path)
    );
    docker_exec_command(container, worktree_path, &inner)
}

/// Parameters shared by the invocation builders.
pub struct AgentInvocation<'a> {
    pub agent: &'a AgentDefinition,
    pub yolo: bool,
    pub system_prompt: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub launch_mode: AgentLaunchMode,
    pub worktree_path: &'a str,
    pub repo_root: &'a str,
    pub branch: &'a str,
    pub profile_name: &'a str,
    pub resume_conversation_id: Option<&'a str>,
    pub fork_from_session_id: Option<&'a str>,
    pub pin_session_id: Option<&'a str>,
}

/// Claude's launch argv. Kept byte-identical to the pre-refactor behaviour — see the
/// `GOLDEN` table in this module's tests.
fn claude_invocation(inv: &AgentInvocation, prompt_suffix: &str) -> String {
    let yolo = if inv.yolo { " --dangerously-skip-permissions" } else { "" };
    if inv.launch_mode == AgentLaunchMode::Fork
        && let Some(fork) = inv.fork_from_session_id
    {
        let pin = inv
            .pin_session_id
            .map(|id| format!(" --session-id {}", quote_shell(id)))
            .unwrap_or_default();
        return format!(
            "claude{yolo} --resume {} --fork-session{pin}{prompt_suffix}",
            quote_shell(fork)
        );
    }
    if inv.launch_mode == AgentLaunchMode::Resume {
        let target = inv
            .resume_conversation_id
            .map(|id| format!(" --resume {}", quote_shell(id)))
            .unwrap_or_else(|| " --continue".to_string());
        return format!("claude{yolo}{target}{prompt_suffix}");
    }
    // A fresh launch may pin its own session id, so a tab whose pane is rebuilt
    // later can `--resume` it instead of losing its conversation.
    let pin = inv
        .pin_session_id
        .map(|id| format!(" --session-id {}", quote_shell(id)))
        .unwrap_or_default();
    if let Some(sys) = inv.system_prompt {
        return format!(
            "claude{yolo}{pin} --append-system-prompt {}{prompt_suffix}",
            quote_shell(sys)
        );
    }
    format!("claude{yolo}{pin}{prompt_suffix}")
}

/// Codex's launch argv. Unlike Claude, Codex needs `--enable hooks` explicitly and
/// ignores `pin_session_id` (it assigns its own session id).
fn codex_invocation(inv: &AgentInvocation, prompt_suffix: &str) -> String {
    let hooks = " --enable hooks";
    let yolo = if inv.yolo { " --yolo" } else { "" };
    if inv.launch_mode == AgentLaunchMode::Fork
        && let Some(fork) = inv.fork_from_session_id
    {
        return format!("codex{hooks}{yolo} fork {}{prompt_suffix}", quote_shell(fork));
    }
    if inv.launch_mode == AgentLaunchMode::Resume {
        let target = inv
            .resume_conversation_id
            .map(|id| format!(" {}", quote_shell(id)))
            .unwrap_or_else(|| " --last".to_string());
        return format!("codex{hooks}{yolo} resume{target}{prompt_suffix}");
    }
    if let Some(sys) = inv.system_prompt {
        return format!(
            "codex{hooks}{yolo} -c {}{prompt_suffix}",
            quote_shell(&format!("developer_instructions={sys}"))
        );
    }
    format!("codex{hooks}{yolo}{prompt_suffix}")
}

/// opencode's launch argv (verified against opencode 1.18.7).
///
/// Differences from claude/codex worth knowing:
/// - the initial prompt goes in `--prompt`, not a `--` positional (that form is
///   `opencode run`, which is the one-shot mode, not the interactive TUI Sebenza launches);
/// - bypass is `--auto`, a plain flag;
/// - **there is no system-prompt flag.** opencode carries system instructions via its
///   agent/config files, so a per-launch `system_prompt` cannot be passed and is dropped.
///   Recorded as a limitation rather than silently approximated.
fn opencode_invocation(inv: &AgentInvocation) -> String {
    let yolo = if inv.yolo { " --auto" } else { "" };
    let prompt = inv
        .prompt
        .map(|p| format!(" --prompt {}", quote_shell(p)))
        .unwrap_or_default();

    if inv.launch_mode == AgentLaunchMode::Fork
        && let Some(fork) = inv.fork_from_session_id
    {
        // --fork requires --session or --continue; it branches rather than resuming.
        return format!("opencode{yolo} --session {} --fork{prompt}", quote_shell(fork));
    }
    if inv.launch_mode == AgentLaunchMode::Resume {
        let target = inv
            .resume_conversation_id
            .map(|id| format!(" --session {}", quote_shell(id)))
            .unwrap_or_else(|| " --continue".to_string());
        return format!("opencode{yolo}{target}{prompt}");
    }
    format!("opencode{yolo}{prompt}")
}

/// Dispatch to the selected built-in agent. Exhaustive on `BuiltinAgentId`, so adding an
/// agent is a compile error here rather than a silent fallthrough to Claude.
fn built_in_invocation(agent: BuiltinAgentId, inv: &AgentInvocation) -> String {
    let prompt_suffix = inv
        .prompt
        .map(|p| format!(" -- {}", quote_shell(p)))
        .unwrap_or_default();

    match agent {
        BuiltinAgentId::Claude => claude_invocation(inv, &prompt_suffix),
        BuiltinAgentId::Codex => codex_invocation(inv, &prompt_suffix),
        // opencode takes its prompt via --prompt, so it builds its own rather than
        // using the shared `-- <prompt>` suffix.
        BuiltinAgentId::Opencode => opencode_invocation(inv),
    }
}

const CUSTOM_VARS: [(&str, &str); 6] = [
    ("${PROMPT}", "SEBENZA_AGENT_PROMPT"),
    ("${SYSTEM_PROMPT}", "SEBENZA_AGENT_SYSTEM_PROMPT"),
    ("${WORKTREE_PATH}", "SEBENZA_AGENT_WORKTREE_PATH"),
    ("${REPO_PATH}", "SEBENZA_AGENT_REPO_PATH"),
    ("${BRANCH}", "SEBENZA_AGENT_BRANCH"),
    ("${PROFILE}", "SEBENZA_AGENT_PROFILE"),
];

fn render_custom_template(template: &str) -> String {
    let mut out = template.to_string();
    for (placeholder, var) in CUSTOM_VARS {
        out = out.replace(placeholder, &format!("${var}"));
    }
    out
}

fn custom_invocation(config: &crate::domain::config::CustomAgentConfig, inv: &AgentInvocation) -> String {
    let template = if inv.launch_mode == AgentLaunchMode::Resume {
        config.resume_command.as_deref().unwrap_or(&config.start_command)
    } else {
        &config.start_command
    };
    let exports: Vec<String> = [
        ("SEBENZA_AGENT_PROMPT", inv.prompt.unwrap_or("")),
        ("SEBENZA_AGENT_SYSTEM_PROMPT", inv.system_prompt.unwrap_or("")),
        ("SEBENZA_AGENT_WORKTREE_PATH", inv.worktree_path),
        ("SEBENZA_AGENT_REPO_PATH", inv.repo_root),
        ("SEBENZA_AGENT_BRANCH", inv.branch),
        ("SEBENZA_AGENT_PROFILE", inv.profile_name),
    ]
    .iter()
    .map(|(k, v)| format!("export {k}={}", quote_shell(v)))
    .collect();
    format!("{}; {}", exports.join("; "), render_custom_template(template))
}

fn agent_invocation(inv: &AgentInvocation) -> String {
    match &inv.agent.implementation {
        AgentImplementation::Builtin(agent) => built_in_invocation(*agent, inv),
        AgentImplementation::Custom(config) => custom_invocation(config, inv),
    }
}

/// The agent pane command: source the runtime env, then exec the agent.
pub fn build_agent_pane_command(runtime_env_path: &str, inv: &AgentInvocation) -> String {
    format!("{}; {}", runtime_bootstrap(runtime_env_path), agent_invocation(inv))
}

/// The managed shell pane command: source the runtime env, then exec an
/// interactive login shell.
pub fn build_managed_shell_command(runtime_env_path: &str) -> String {
    let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let inner = format!(
        "{}; exec {} -i",
        runtime_bootstrap(runtime_env_path),
        quote_shell(&shell_path)
    );
    format!("bash -lc {}", quote_shell(&inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_registry::{AgentCapabilities, AgentDefinition, BuiltinAgentId};

    fn builtin(id: &str) -> AgentDefinition {
        AgentDefinition {
            id: id.to_string(),
            label: id.to_string(),
            kind: "builtin",
            capabilities: AgentCapabilities {
                terminal: true,
                in_app_chat: true,
                conversation_history: true,
                interrupt: true,
                resume: true,
                fork: true,
                pinnable_session_id: true,
                permission_interception: false,
            },
            implementation: AgentImplementation::Builtin(
                BuiltinAgentId::from_wire(id).expect("test helper takes a builtin id"),
            ),
        }
    }

    /// Golden argv for the built-in agents, captured from the pre-refactor
    /// `built_in_invocation`. The `BuiltinAgentId` refactor MUST NOT change any of these
    /// strings — this table is the proof that it is behaviour-preserving.
    const GOLDEN: &[(BuiltinAgentId, &str, &str)] = &[
        (BuiltinAgentId::Claude, "fresh", "claude"),
        (BuiltinAgentId::Claude, "fresh+yolo", "claude --dangerously-skip-permissions"),
        (BuiltinAgentId::Claude, "fresh+sys+prompt", "claude --append-system-prompt 'be x' -- 'do y'"),
        (BuiltinAgentId::Claude, "resume+last", "claude --continue"),
        (BuiltinAgentId::Claude, "resume+id", "claude --resume 'sid'"),
        (BuiltinAgentId::Claude, "fork", "claude --resume 'fid' --fork-session --session-id 'pin'"),
        (BuiltinAgentId::Codex, "fresh", "codex --enable hooks"),
        (BuiltinAgentId::Codex, "fresh+yolo", "codex --enable hooks --yolo"),
        (BuiltinAgentId::Codex, "fresh+sys+prompt", "codex --enable hooks -c 'developer_instructions=be x' -- 'do y'"),
        (BuiltinAgentId::Codex, "resume+last", "codex --enable hooks resume --last"),
        (BuiltinAgentId::Codex, "resume+id", "codex --enable hooks resume 'sid'"),
        (BuiltinAgentId::Codex, "fork", "codex --enable hooks fork 'fid'"),
        (BuiltinAgentId::Opencode, "fresh", "opencode"),
        (BuiltinAgentId::Opencode, "fresh+yolo", "opencode --auto"),
        (BuiltinAgentId::Opencode, "fresh+sys+prompt", "opencode --prompt 'do y'"),
        (BuiltinAgentId::Opencode, "resume+last", "opencode --continue"),
        (BuiltinAgentId::Opencode, "resume+id", "opencode --session 'sid'"),
        (BuiltinAgentId::Opencode, "fork", "opencode --session 'fid' --fork"),
    ];

    fn apply_case<'a>(i: &mut AgentInvocation<'a>, case: &str) {
        match case {
            "fresh" => {}
            "fresh+yolo" => i.yolo = true,
            "fresh+sys+prompt" => {
                i.system_prompt = Some("be x");
                i.prompt = Some("do y");
            }
            "resume+last" => i.launch_mode = AgentLaunchMode::Resume,
            "resume+id" => {
                i.launch_mode = AgentLaunchMode::Resume;
                i.resume_conversation_id = Some("sid");
            }
            "fork" => {
                i.launch_mode = AgentLaunchMode::Fork;
                i.fork_from_session_id = Some("fid");
                i.pin_session_id = Some("pin");
            }
            other => panic!("unknown golden case {other}"),
        }
    }

    #[test]
    fn builtin_argv_is_unchanged_by_the_enum_refactor() {
        for (id, case, expected) in GOLDEN {
            let agent = builtin(id.as_str());
            let mut i = inv(&agent);
            apply_case(&mut i, case);
            assert_eq!(
                &agent_invocation(&i),
                expected,
                "argv changed for {}/{case}: this refactor must preserve behaviour",
                id.as_str()
            );
        }
    }

    #[test]
    fn builtin_agent_id_round_trips_through_its_wire_string() {
        for id in BuiltinAgentId::ALL {
            assert_eq!(BuiltinAgentId::from_wire(id.as_str()), Some(*id));
        }
        assert_eq!(BuiltinAgentId::from_wire("goose"), None, "goose is not a builtin in this track");
        assert_eq!(BuiltinAgentId::from_wire("some-custom"), None);
        assert_eq!(BuiltinAgentId::from_wire("Claude"), None, "wire ids are case-sensitive");
    }

    #[test]
    fn all_lists_exactly_the_builtin_agents() {
        let ids: Vec<&str> = BuiltinAgentId::ALL.iter().map(|a| a.as_str()).collect();
        assert_eq!(ids, vec!["claude", "codex", "opencode"]);
    }

    fn inv<'a>(agent: &'a AgentDefinition) -> AgentInvocation<'a> {
        AgentInvocation {
            agent,
            yolo: false,
            system_prompt: None,
            prompt: None,
            launch_mode: AgentLaunchMode::Fresh,
            worktree_path: "/wt",
            repo_root: "/repo",
            branch: "feature",
            profile_name: "default",
            resume_conversation_id: None,
            fork_from_session_id: None,
            pin_session_id: None,
        }
    }

    #[test]
    fn claude_fresh_with_prompt_and_system() {
        let a = builtin("claude");
        let mut i = inv(&a);
        i.yolo = true;
        i.system_prompt = Some("be nice");
        i.prompt = Some("do it");
        let cmd = build_agent_pane_command("/wt/.git/.ai/sebenza/runtime.env", &i);
        assert!(cmd.contains("set -a; . '/wt/.git/.ai/sebenza/runtime.env'; set +a; "));
        assert!(cmd.contains(
            "claude --dangerously-skip-permissions --append-system-prompt 'be nice' -- 'do it'"
        ));
    }

    #[test]
    fn codex_resume_uses_last_when_no_id() {
        let a = builtin("codex");
        let mut i = inv(&a);
        i.launch_mode = AgentLaunchMode::Resume;
        let cmd = agent_invocation(&i);
        assert_eq!(cmd, "codex --enable hooks resume --last");
    }

    #[test]
    fn managed_shell_sources_env() {
        let cmd = build_managed_shell_command("/x/runtime.env");
        assert!(cmd.starts_with("bash -lc "));
        // The bootstrap's own single quotes are shell-escaped by the outer quoting.
        assert!(cmd.contains("set -a; . '\\''/x/runtime.env'\\''; set +a"));
        assert!(cmd.contains("exec "));
    }

    #[test]
    fn claude_fresh_pins_the_session_id_when_asked() {
        // Lets a fresh agent tab be resumed after its pane is rebuilt, instead of
        // silently losing the conversation.
        let agent = builtin("claude");
        let mut invocation = inv(&agent);
        invocation.pin_session_id = Some("11111111-2222-3333-4444-555555555555");
        let cmd = agent_invocation(&invocation);
        assert!(
            cmd.contains("--session-id '11111111-2222-3333-4444-555555555555'"),
            "expected a pinned session id, got: {cmd}"
        );
    }

    #[test]
    fn claude_fresh_pins_alongside_a_system_prompt() {
        let agent = builtin("claude");
        let mut invocation = inv(&agent);
        invocation.pin_session_id = Some("abc");
        invocation.system_prompt = Some("be terse");
        let cmd = agent_invocation(&invocation);
        assert!(cmd.contains("--session-id 'abc'"), "got: {cmd}");
        assert!(cmd.contains("--append-system-prompt 'be terse'"), "got: {cmd}");
    }

    #[test]
    fn claude_fresh_omits_the_session_flag_when_unpinned() {
        let agent = builtin("claude");
        let cmd = agent_invocation(&inv(&agent));
        assert!(!cmd.contains("--session-id"), "got: {cmd}");
    }

    #[test]
    fn a_fresh_custom_agent_runs_its_start_command() {
        // The path a Goose/OpenCode provider tab takes: no session discovery, no
        // resume — just the configured start command with the env exports.
        let agent = AgentDefinition {
            id: "goose".to_string(),
            label: "Goose".to_string(),
            kind: "custom",
            capabilities: AgentCapabilities {
                terminal: true,
                in_app_chat: false,
                conversation_history: false,
                interrupt: false,
                resume: false,
                fork: false,
            },
            implementation: AgentImplementation::Custom(crate::domain::config::CustomAgentConfig {
                label: "Goose".to_string(),
                start_command: "goose session start ${PROMPT}".to_string(),
                resume_command: None,
            }),
        };
        let cmd = agent_invocation(&inv(&agent));
        assert!(cmd.contains("goose session start"), "got: {cmd}");
        // `${PROMPT}` is rewritten to the exported env var, not interpolated.
        assert!(cmd.contains("SEBENZA_AGENT_PROMPT"), "got: {cmd}");
    }
}
