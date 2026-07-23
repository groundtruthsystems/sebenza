//! On-disk agent session discovery — port of
//! `backend-legacy/src/adapters/session-discovery.ts`. Finds the session ids a
//! built-in agent has written for a given cwd, newest first, and can poll for a
//! freshly-created one (used when forking a tab and the id can't be pinned).

use crate::adapters::{claude_cli, codex_session_log};

/// Built-in agents whose on-disk session history we can discover.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiscoverableAgentKind {
    Claude,
    Codex,
}

/// Session ids for `cwd`, newest first.
pub fn list_session_ids(agent: DiscoverableAgentKind, cwd: &str) -> Vec<String> {
    match agent {
        DiscoverableAgentKind::Claude => claude_cli::list_session_ids(cwd),
        DiscoverableAgentKind::Codex => codex_session_log::list_session_ids(cwd),
    }
}

/// Poll for a session id present in `cwd` but absent from `before`, returning the
/// newest such id. Used to learn a freshly-forked session's id when it cannot be
/// pinned (Codex). Returns `None` if nothing new appears within the retry budget.
pub fn capture_new_session_id(
    agent: DiscoverableAgentKind,
    cwd: &str,
    before: &[String],
) -> Option<String> {
    let before: std::collections::HashSet<&str> = before.iter().map(String::as_str).collect();
    for _ in 0..20 {
        let after = list_session_ids(agent, cwd);
        if let Some(fresh) = after.into_iter().find(|id| !before.contains(id.as_str())) {
            return Some(fresh);
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    None
}
