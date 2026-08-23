//! Dispatch worktree sandboxes to the LXC or Apple Container adapters.

use crate::adapters::apple_container::{self, LaunchAppleOpts};
use crate::adapters::docker;
use crate::adapters::lxc::{self, LaunchLxcOpts};
use crate::domain::config::{MountSpec, RuntimeKind, is_sandboxed};
use crate::services::agent_service::{
    AgentInvocation, build_agent_pane_command, build_apple_agent_pane_command,
    build_apple_shell_command, build_lxc_agent_pane_command, build_lxc_shell_command,
    build_managed_shell_command,
};
use std::collections::HashMap;
use std::process::Command;

pub struct SandboxLaunchSpec<'a> {
    pub runtime: RuntimeKind,
    pub branch: &'a str,
    pub worktree_path: &'a str,
    pub repo_root: &'a str,
    pub image: &'a str,
    pub env_passthrough: &'a [String],
    pub mounts: &'a [MountSpec],
    pub service_port_envs: &'a [String],
    pub runtime_env: &'a HashMap<String, String>,
}

pub fn host_user_ids() -> (u32, u32) {
    let parse = |flag| {
        Command::new("id")
            .arg(flag)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    };
    (parse("-u"), parse("-g"))
}

/// Start or reuse a sandbox. Host (and the removed docker runtime) return `None`.
pub fn launch(spec: &SandboxLaunchSpec<'_>) -> Result<Option<String>, String> {
    match spec.runtime {
        RuntimeKind::Host | RuntimeKind::Docker => Ok(None),
        RuntimeKind::Lxc => Ok(Some(lxc::launch_instance(&LaunchLxcOpts {
            branch: spec.branch.to_string(),
            wt_dir: spec.worktree_path.to_string(),
            main_repo_dir: spec.repo_root.to_string(),
            image: spec.image.to_string(),
            env_passthrough: spec.env_passthrough.to_vec(),
            mounts: spec.mounts.to_vec(),
            service_port_envs: spec.service_port_envs.to_vec(),
            runtime_env: spec.runtime_env.clone(),
        })?)),
        RuntimeKind::Apple => Ok(Some(apple_container::launch_instance(&LaunchAppleOpts {
            branch: spec.branch.to_string(),
            wt_dir: spec.worktree_path.to_string(),
            main_repo_dir: spec.repo_root.to_string(),
            image: spec.image.to_string(),
            env_passthrough: spec.env_passthrough.to_vec(),
            mounts: spec.mounts.to_vec(),
            service_port_envs: spec.service_port_envs.to_vec(),
            runtime_env: spec.runtime_env.clone(),
        })?)),
    }
}

/// Destroy instances for a persisted `WorktreeMeta.runtime` string.
pub fn remove(runtime: &str, branch: &str) {
    match runtime {
        "lxc" => lxc::remove_instance(branch),
        "apple" => apple_container::remove_instance(branch),
        "docker" => docker::remove_container(branch),
        _ => {}
    }
}

/// Best-effort cleanup when create fails after a sandbox may already exist.
pub fn remove_any(branch: &str) {
    lxc::remove_instance(branch);
    apple_container::remove_instance(branch);
    docker::remove_container(branch);
}

/// tmux pane commands for this runtime. `instance` is required for LXC/Apple.
pub fn pane_commands(
    runtime: RuntimeKind,
    instance: Option<&str>,
    worktree_path: &str,
    runtime_env_path: &str,
    inv: &AgentInvocation<'_>,
    host_uid: u32,
    host_gid: u32,
    host_home: &str,
) -> Result<(String, String), String> {
    match runtime {
        RuntimeKind::Host => Ok((
            build_agent_pane_command(runtime_env_path, inv),
            build_managed_shell_command(runtime_env_path),
        )),
        RuntimeKind::Docker => Err(
            "The docker runtime has been removed. Use runtime: lxc on Linux or runtime: apple on macOS."
                .to_string(),
        ),
        RuntimeKind::Lxc => {
            let name = instance.ok_or_else(|| "LXC sandbox instance name is missing".to_string())?;
            Ok((
                build_lxc_agent_pane_command(
                    name,
                    worktree_path,
                    runtime_env_path,
                    host_uid,
                    host_gid,
                    host_home,
                    inv,
                ),
                build_lxc_shell_command(
                    name,
                    worktree_path,
                    runtime_env_path,
                    host_uid,
                    host_gid,
                    host_home,
                ),
            ))
        }
        RuntimeKind::Apple => {
            let name =
                instance.ok_or_else(|| "Apple sandbox instance name is missing".to_string())?;
            Ok((
                build_apple_agent_pane_command(
                    name,
                    worktree_path,
                    runtime_env_path,
                    host_uid,
                    host_gid,
                    inv,
                ),
                build_apple_shell_command(name, worktree_path, runtime_env_path, host_uid, host_gid),
            ))
        }
    }
}

pub fn tabs_unsupported_message(runtime: RuntimeKind) -> Option<&'static str> {
    is_sandboxed(runtime).then_some("Tabs are not supported for sandboxed worktrees")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_registry::{
        AgentCapabilities, AgentDefinition, AgentImplementation, BuiltinAgentId,
    };
    use crate::services::agent_service::AgentLaunchMode;

    fn inv<'a>(agent: &'a AgentDefinition) -> AgentInvocation<'a> {
        AgentInvocation {
            agent,
            yolo: false,
            system_prompt: None,
            prompt: None,
            launch_mode: AgentLaunchMode::Fresh,
            worktree_path: "/home/u/wt",
            repo_root: "/home/u/repo",
            branch: "feat",
            profile_name: "sandbox",
            resume_conversation_id: None,
            fork_from_session_id: None,
            pin_session_id: None,
        }
    }

    fn claude() -> AgentDefinition {
        AgentDefinition {
            id: "claude".into(),
            label: "claude".into(),
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
            implementation: AgentImplementation::Builtin(BuiltinAgentId::Claude),
        }
    }

    #[test]
    fn host_panes_do_not_enter_a_sandbox() {
        let agent = claude();
        let i = inv(&agent);
        let (agent_cmd, shell_cmd) = pane_commands(
            RuntimeKind::Host,
            None,
            "/home/u/wt",
            "/home/u/wt/.git/.ai/sebenza/runtime.env",
            &i,
            1000,
            1000,
            "/home/u",
        )
        .unwrap();
        assert!(!agent_cmd.contains("lxc-attach"), "{agent_cmd}");
        assert!(!agent_cmd.contains("container machine"), "{agent_cmd}");
        assert!(agent_cmd.contains("claude"), "{agent_cmd}");
        assert!(shell_cmd.contains("bash -lc"), "{shell_cmd}");
    }

    #[test]
    fn lxc_panes_attach_to_the_instance() {
        let agent = claude();
        let i = inv(&agent);
        let (agent_cmd, shell_cmd) = pane_commands(
            RuntimeKind::Lxc,
            Some("sebenza-feat-1"),
            "/home/u/wt",
            "/home/u/wt/.git/.ai/sebenza/runtime.env",
            &i,
            1000,
            1000,
            "/home/u",
        )
        .unwrap();
        assert!(
            agent_cmd.contains("lxc-attach -n 'sebenza-feat-1'"),
            "{agent_cmd}"
        );
        assert!(shell_cmd.contains("lxc-attach"), "{shell_cmd}");
        assert!(
            pane_commands(
                RuntimeKind::Lxc,
                None,
                "/wt",
                "/env",
                &i,
                1000,
                1000,
                "/home/u",
            )
            .is_err()
        );
    }

    #[test]
    fn apple_panes_run_in_the_machine() {
        let agent = claude();
        let i = inv(&agent);
        let (agent_cmd, _) = pane_commands(
            RuntimeKind::Apple,
            Some("sebenza-feat-1"),
            "/Users/u/wt",
            "/Users/u/wt/.git/.ai/sebenza/runtime.env",
            &i,
            501,
            20,
            "/Users/u",
        )
        .unwrap();
        assert!(
            agent_cmd.contains("container machine run -n 'sebenza-feat-1'"),
            "{agent_cmd}"
        );
    }

    #[test]
    fn docker_panes_are_refused() {
        let agent = claude();
        let i = inv(&agent);
        let err = pane_commands(
            RuntimeKind::Docker,
            Some("c"),
            "/wt",
            "/env",
            &i,
            1000,
            1000,
            "/home/u",
        )
        .unwrap_err();
        assert!(err.to_lowercase().contains("removed"), "{err}");
    }

    #[test]
    fn tabs_are_blocked_only_for_live_sandboxes() {
        assert!(tabs_unsupported_message(RuntimeKind::Host).is_none());
        assert!(tabs_unsupported_message(RuntimeKind::Docker).is_none());
        assert!(tabs_unsupported_message(RuntimeKind::Lxc).is_some());
        assert!(tabs_unsupported_message(RuntimeKind::Apple).is_some());
    }
}
