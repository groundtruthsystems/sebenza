//! Agent launch-command builders — pure port of
//! `backend-legacy/src/services/agent-service.ts`. Produces the shell command a
//! tmux pane runs to source the runtime env then exec the agent (claude/codex or
//! a custom template). Docker variants are deferred until the docker adapter lands.

use crate::services::agent_registry::{AgentDefinition, AgentImplementation};

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

fn built_in_invocation(
    agent: &str,
    inv: &AgentInvocation,
) -> String {
    let prompt_suffix = inv
        .prompt
        .map(|p| format!(" -- {}", quote_shell(p)))
        .unwrap_or_default();

    if agent == "codex" {
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
        return format!("codex{hooks}{yolo}{prompt_suffix}");
    }

    // claude
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
    if let Some(sys) = inv.system_prompt {
        return format!("claude{yolo} --append-system-prompt {}{prompt_suffix}", quote_shell(sys));
    }
    format!("claude{yolo}{prompt_suffix}")
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
        AgentImplementation::Builtin(agent) => built_in_invocation(agent, inv),
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
    use crate::services::agent_registry::{AgentCapabilities, AgentDefinition};

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
            },
            implementation: AgentImplementation::Builtin(id.to_string()),
        }
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
}
