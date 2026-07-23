//! Periodically persists which worktree sessions are open, so `sebenza-cli restore`
//! can re-open them after a server restart/reboot — port of
//! `backend-legacy/src/services/session-restore-service.ts`. The pure
//! `compute_open_branches` is unit-tested without git/tmux.

use crate::adapters::fs::write_open_sessions_state;
use crate::adapters::git::{GitGateway, GitWorktreeEntry};
use crate::adapters::tmux::{
    build_project_session_name, build_worktree_window_name, TmuxGateway, TmuxWindowSummary,
};
use crate::domain::model::{OpenSessionsState, OPEN_SESSIONS_STATE_VERSION};
use std::path::Path;

/// Absolute, symlink-resolved path (falls back to the input if it doesn't
/// exist) — matches the worktree-root comparison used elsewhere.
fn canonical_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

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

/// Branches of worktrees that currently have an open tmux window in the project
/// session, sorted. Pure — testable without git/tmux.
pub fn compute_open_branches(
    worktrees: &[GitWorktreeEntry],
    windows: &[TmuxWindowSummary],
    session_name: &str,
    project_dir: &str,
) -> Vec<String> {
    let resolved_project_dir = canonical_path(project_dir);
    let open_window_names: std::collections::HashSet<&str> = windows
        .iter()
        .filter(|w| w.session_name == session_name)
        .map(|w| w.window_name.as_str())
        .collect();

    let mut branches: Vec<String> = worktrees
        .iter()
        .filter(|e| !e.bare && canonical_path(&e.path) != resolved_project_dir)
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
    let branches = compute_open_branches(&worktrees, &windows, &session_name, &project_root);
    if branches.is_empty() {
        return None;
    }
    let git_dir = git.resolve_worktree_git_dir(&project_root).ok()?;
    write_open_sessions_state(&git_dir, &build_open_sessions_state(branches.clone(), saved_at)).ok()?;
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
    fn open_branches_only_include_worktrees_with_a_live_window() {
        let session = build_project_session_name("/repo");
        let worktrees = vec![
            entry("/repo", Some("main"), false), // the project root itself — excluded
            entry("/repo/wt-a", Some("feat-a"), false),
            entry("/repo/wt-b", Some("feat-b"), false),
            entry("/repo/bare", None, true), // bare — excluded
        ];
        let windows = vec![
            window(&session, &build_worktree_window_name("feat-a")),
            window("other-session", &build_worktree_window_name("feat-b")),
        ];
        let open = compute_open_branches(&worktrees, &windows, &session, "/repo");
        assert_eq!(open, vec!["feat-a".to_string()]);
    }
}
