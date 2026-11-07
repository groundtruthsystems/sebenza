//! Path normalisation helpers.
//!
//! A relative `worktreeRoot` (the default is `../worktrees`) joined onto the
//! project root yields paths like `/code/repo/../worktrees/feat-x`. Git and the
//! filesystem treat that as `/code/worktrees/feat-x`, but string comparisons do
//! not, so anything that records or compares worktree paths normalises first.

use std::path::{Component, Path, PathBuf};

/// Resolve `.` and `..` segments without touching the filesystem.
///
/// Usable before the path exists (a worktree about to be created). Symlinks
/// are not resolved; use [`comparable_path`] when the path may exist.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` as the directory it names, for comparing two spellings of a path.
///
/// An existing path is canonicalised, so `..` segments and symlinks resolve to
/// the real directory. A path that does not exist falls back to
/// [`normalize_lexically`]. A trailing `/` never matters.
pub fn comparable_path(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| normalize_lexically(Path::new(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_segments_pop_their_parent() {
        assert_eq!(
            normalize_lexically(Path::new("/code/repo/../worktrees/feat-x")),
            PathBuf::from("/code/worktrees/feat-x")
        );
    }

    #[test]
    fn current_dir_segments_and_trailing_slashes_are_dropped() {
        assert_eq!(
            normalize_lexically(Path::new("/code/./worktrees/feat-x/")),
            PathBuf::from("/code/worktrees/feat-x")
        );
    }

    #[test]
    fn parent_of_root_stays_at_root() {
        assert_eq!(normalize_lexically(Path::new("/../x")), PathBuf::from("/x"));
    }

    #[test]
    fn a_missing_path_compares_lexically() {
        assert_eq!(
            comparable_path("/definitely/not/here/../there/"),
            PathBuf::from("/definitely/not/there")
        );
    }

    #[test]
    fn an_existing_path_is_canonicalised() {
        let dir = std::env::temp_dir().canonicalize().expect("temp dir");
        let spelled = format!("{}/./", dir.display());
        assert_eq!(comparable_path(&spelled), dir);
    }
}
