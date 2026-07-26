use crate::domain::model::{ArchivedWorktreeEntry, WorktreeArchiveState, WORKTREE_ARCHIVE_STATE_VERSION};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Lexically normalize a path to an absolute form (mirrors Node `resolve`): make
/// absolute against the cwd, then collapse `.`/`..` without touching the fs (the
/// path may already be deleted when we archive-clear on removal).
pub fn normalize_archive_path(path: &str) -> String {
    let p = Path::new(path);
    let abs: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };

    let mut out = PathBuf::new();
    for component in abs.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().to_string()
}

fn create_archive_state(mut entries: Vec<ArchivedWorktreeEntry>) -> WorktreeArchiveState {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    WorktreeArchiveState {
        schema_version: WORKTREE_ARCHIVE_STATE_VERSION,
        entries,
    }
}

pub fn build_archived_worktree_path_set(state: &WorktreeArchiveState) -> HashSet<String> {
    state
        .entries
        .iter()
        .map(|entry| normalize_archive_path(&entry.path))
        .collect()
}

/// Set (or clear) the archived flag for a path, stamping `archived_at` with the
/// caller-supplied ISO timestamp.
pub fn set_archived_worktree_state(
    state: &WorktreeArchiveState,
    path: &str,
    archived: bool,
    archived_at: &str,
) -> WorktreeArchiveState {
    let normalized = normalize_archive_path(path);
    let mut entries: Vec<ArchivedWorktreeEntry> = state
        .entries
        .iter()
        .filter(|entry| normalize_archive_path(&entry.path) != normalized)
        .cloned()
        .collect();
    if archived {
        entries.push(ArchivedWorktreeEntry {
            path: normalized,
            archived_at: archived_at.to_string(),
        });
    }
    create_archive_state(entries)
}

/// Drop entries whose path is no longer among the live worktree paths.
pub fn prune_archived_worktree_state(
    state: &WorktreeArchiveState,
    paths: &[String],
) -> WorktreeArchiveState {
    let valid: HashSet<String> = paths.iter().map(|p| normalize_archive_path(p)).collect();
    let entries = state
        .entries
        .iter()
        .filter(|entry| valid.contains(&normalize_archive_path(&entry.path)))
        .cloned()
        .collect();
    create_archive_state(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> WorktreeArchiveState {
        WorktreeArchiveState {
            schema_version: WORKTREE_ARCHIVE_STATE_VERSION,
            entries: Vec::new(),
        }
    }

    #[test]
    fn set_then_clear_is_idempotent() {
        let s = set_archived_worktree_state(&empty(), "/repo/a", true, "2026-01-01T00:00:00Z");
        assert_eq!(s.entries.len(), 1);
        assert_eq!(s.entries[0].path, "/repo/a");
        // Re-archiving the same path does not duplicate it.
        let s = set_archived_worktree_state(&s, "/repo/a", true, "2026-01-02T00:00:00Z");
        assert_eq!(s.entries.len(), 1);
        assert_eq!(s.entries[0].archived_at, "2026-01-02T00:00:00Z");
        // Clearing removes it.
        let s = set_archived_worktree_state(&s, "/repo/a", false, "t");
        assert!(s.entries.is_empty());
    }

    #[test]
    fn prune_drops_stale_paths_and_sorts() {
        let mut s = set_archived_worktree_state(&empty(), "/repo/b", true, "t");
        s = set_archived_worktree_state(&s, "/repo/a", true, "t");
        let pruned = prune_archived_worktree_state(&s, &["/repo/a".to_string()]);
        assert_eq!(pruned.entries.len(), 1);
        assert_eq!(pruned.entries[0].path, "/repo/a");
    }

    #[test]
    fn normalize_collapses_dot_segments() {
        assert_eq!(normalize_archive_path("/repo/./x/../y"), "/repo/y");
    }
}
