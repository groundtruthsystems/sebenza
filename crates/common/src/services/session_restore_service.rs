use crate::adapters::fs::write_open_sessions_state;
use crate::adapters::git::{GitGateway, GitWorktreeEntry, canonical_path};
use crate::adapters::tmux::{
    TmuxGateway, TmuxWindowSummary, build_project_session_name, build_worktree_window_name,
};
use crate::domain::model::{OPEN_SESSIONS_STATE_VERSION, OpenSessionsState};
use std::path::Path;

/// The branch of a live worktree entry, falling back to the directory basename
/// (mirrors the CLI/list convention).
fn entry_branch(entry: &GitWorktreeEntry) -> String {
    entry.branch.clone().unwrap_or_else(|| {
        Path::new(&entry.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    })
}

/// Branches with an open tmux window in the project session, sorted. Pure —
/// testable without git/tmux.
///
/// The repo root is included: the main checkout is an openable terminal session
/// like any other, so a restore after reboot should bring it back too. Bare
/// entries are still excluded — they have no working tree to open.
pub fn compute_open_branches(
    worktrees: &[GitWorktreeEntry],
    windows: &[TmuxWindowSummary],
    session_name: &str,
) -> Vec<String> {
    let open_window_names: std::collections::HashSet<&str> = windows
        .iter()
        .filter(|w| w.session_name == session_name)
        .map(|w| w.window_name.as_str())
        .collect();

    let mut branches: Vec<String> = worktrees
        .iter()
        .filter(|e| !e.bare)
        .map(entry_branch)
        .filter(|branch| open_window_names.contains(build_worktree_window_name(branch).as_str()))
        .collect();
    branches.sort();
    branches
}

pub fn build_open_sessions_state(branches: Vec<String>, saved_at: String) -> OpenSessionsState {
    OpenSessionsState {
        schema_version: OPEN_SESSIONS_STATE_VERSION,
        saved_at,
        branches,
    }
}

/// Persist the currently-open worktree sessions. Returns the branches written,
/// or `None` when nothing was written. An empty open set never overwrites the
/// snapshot (on reboot the server starts before sessions are re-opened, so
/// writing an empty list would clobber the data `restore` needs).
pub fn save_open_sessions_snapshot(
    git: &GitGateway,
    tmux: &TmuxGateway,
    project_root: &str,
    saved_at: String,
) -> Option<Vec<String>> {
    let project_root = canonical_path(project_root);
    let session_name = build_project_session_name(&project_root);
    let windows = tmux.list_windows().unwrap_or_default();
    let worktrees = git.list_live_worktrees(&project_root);
    let branches = compute_open_branches(&worktrees, &windows, &session_name);
    if branches.is_empty() {
        return None;
    }
    let git_dir = git.resolve_worktree_git_dir(&project_root).ok()?;
    write_open_sessions_state(
        &git_dir,
        &build_open_sessions_state(branches.clone(), saved_at),
    )
    .ok()?;
    Some(branches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, branch: Option<&str>, bare: bool) -> GitWorktreeEntry {
        GitWorktreeEntry {
            path: path.to_string(),
            branch: branch.map(str::to_string),
            head: None,
            detached: false,
            bare,
        }
    }

    fn window(session: &str, name: &str) -> TmuxWindowSummary {
        TmuxWindowSummary {
            session_name: session.to_string(),
            window_name: name.to_string(),
            pane_count: 1,
        }
    }

    #[test]
    fn open_branches_only_include_entries_with_a_live_window_in_this_session() {
        let session = build_project_session_name("/repo");
        let worktrees = vec![
            entry("/repo", Some("main"), false),
            entry("/repo/wt-a", Some("feat-a"), false),
            entry("/repo/wt-b", Some("feat-b"), false),
            entry("/repo/bare", None, true), // bare — excluded
        ];
        let windows = vec![
            window(&session, &build_worktree_window_name("feat-a")),
            // A window in another tmux session must not count.
            window("other-session", &build_worktree_window_name("feat-b")),
        ];
        let open = compute_open_branches(&worktrees, &windows, &session);
        assert_eq!(open, vec!["feat-a".to_string()]);
    }

    #[test]
    fn the_main_checkout_is_restored_when_its_window_is_open() {
        // The repo root used to be filtered out here, so a reboot lost the main
        // session even though it is an ordinary terminal session.
        let session = build_project_session_name("/repo");
        let worktrees = vec![
            entry("/repo", Some("main"), false),
            entry("/repo/wt-a", Some("feat-a"), false),
        ];
        let windows = vec![
            window(&session, &build_worktree_window_name("main")),
            window(&session, &build_worktree_window_name("feat-a")),
        ];
        let open = compute_open_branches(&worktrees, &windows, &session);
        assert_eq!(open, vec!["feat-a".to_string(), "main".to_string()]);
    }

    #[test]
    fn bare_entries_are_still_excluded() {
        let session = build_project_session_name("/repo");
        let worktrees = vec![entry("/repo/bare", Some("main"), true)];
        let windows = vec![window(&session, &build_worktree_window_name("main"))];
        assert!(compute_open_branches(&worktrees, &windows, &session).is_empty());
    }
}
