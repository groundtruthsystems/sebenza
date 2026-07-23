// Reusable adapters live in the `common` crate; re-export them under
// `crate::adapters::*` so the server code keeps its existing paths. Only the
// PTY/WebSocket terminal manager is server-local.
#[allow(unused_imports)]
pub use common::adapters::{
    agent_runtime, claude_cli, codex_session_log, control_token, docker, fs, git, hooks,
    instance_registry, projects_registry, session_discovery, tmux,
};

pub mod terminal;
