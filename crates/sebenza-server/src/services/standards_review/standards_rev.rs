//! `standards_rev`: which revision of a project's standards a review job
//! reads. It is the SHA-256 over the path-sorted (path, blob SHA) pairs of
//! `.ai/sebenza/index.md` and the documents it lists, at the merge-base, read
//! from git objects only. A listed document that is absent contributes
//! (path, `missing`), so a job over it can be told apart.
//!
//! [`resolve_entries`] delegates to [`resolve_standards`], which also applies
//! the path, mode, size and type checks that make a job
//! `standards-unavailable`; those blockers do not change the revision.

use super::git_pinned::Git;
use super::standards_docs::{
    MAX_STANDARDS_BLOB, classify_link, is_track_bookkeeping, link_targets, resolve_standards,
};
use common::domain::solution::sha256_hex;
use std::path::Path;

/// The handshake document every listed standard hangs off.
pub const INDEX_PATH: &str = ".ai/sebenza/index.md";
/// The blob value of a listed document that does not exist at the base.
pub const MISSING: &str = "missing";

/// One input to the revision: a repo-relative path and its object SHA.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct StandardsEntry {
    pub path: String,
    /// `None` when the path names nothing readable at the base.
    pub blob: Option<String>,
}

/// The (path, `missing`) sentinel for a path that names nothing.
pub fn missing(path: &str) -> StandardsEntry {
    StandardsEntry {
        path: path.to_string(),
        blob: None,
    }
}

/// The standards revision at `base_sha`.
pub fn standards_rev(repo: &Path, base_sha: &str) -> Result<String, String> {
    Ok(rev_of(&resolve_entries(repo, base_sha)?))
}

/// The hash of `entries`, independent of their order.
pub fn rev_of(entries: &[StandardsEntry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut text = String::new();
    for e in &sorted {
        text.push_str(&e.path);
        text.push('\0');
        text.push_str(e.blob.as_deref().unwrap_or(MISSING));
        text.push('\n');
    }
    sha256_hex(&text)
}

/// `index.md` and every object its listed documents name at `base_sha`.
pub fn resolve_entries(repo: &Path, base_sha: &str) -> Result<Vec<StandardsEntry>, String> {
    let git = Git::open(repo).map_err(|e| e.to_string())?;
    let standards =
        resolve_standards(&git, base_sha, MAX_STANDARDS_BLOB).map_err(|e| e.to_string())?;
    Ok(standards.entries)
}

/// The repo paths `index.md` links to, resolved against `.ai/sebenza/`,
/// minus track bookkeeping. A refused link keeps its raw form.
pub fn listed_paths(index_md: &str) -> Vec<String> {
    let mut paths: Vec<String> = link_targets(index_md)
        .into_iter()
        .map(|t| classify_link(&t).unwrap_or(t))
        .filter(|p| !is_track_bookkeeping(p))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::TestRepo;

    #[test]
    fn listed_paths_resolve_against_the_workspace_and_skip_track_bookkeeping() {
        let md = "- [W](./workflow.md)\n- [G](./code_styleguides/)\n- [T](./tracks.json)\n\
                  - [D](./tracks/)\n- [X](https://example.com)\n- [E](../../etc/passwd)\n\
                  - [S](./SECURITY.md#threats)\n";
        assert_eq!(
            listed_paths(md),
            vec![
                "../../etc/passwd".to_string(),
                ".ai/sebenza/SECURITY.md".to_string(),
                ".ai/sebenza/code_styleguides".to_string(),
                ".ai/sebenza/workflow.md".to_string(),
            ]
        );
    }

    #[test]
    fn the_rev_covers_index_and_listed_docs_at_the_base() {
        let r = TestRepo::new("rev");
        let base = r.main_head();
        let entries = resolve_entries(&r.repo, &base).unwrap();
        let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                ".ai/sebenza/code_styleguides/general.md",
                ".ai/sebenza/index.md",
                ".ai/sebenza/workflow.md",
            ]
        );
        assert!(
            entries
                .iter()
                .all(|e| e.blob.as_ref().is_some_and(|b| b.len() == 40))
        );
        assert_eq!(standards_rev(&r.repo, &base).unwrap(), rev_of(&entries));
    }

    #[test]
    fn a_listed_doc_change_moves_the_rev_and_bookkeeping_does_not() {
        let r = TestRepo::new("rev-moves");
        let before = standards_rev(&r.repo, &r.main_head()).unwrap();
        let after_tracks = r.commit_on_main(".ai/sebenza/tracks.json", "{\"tracks\":[1]}\n");
        assert_eq!(standards_rev(&r.repo, &after_tracks).unwrap(), before);
        let after_doc = r.commit_on_main(".ai/sebenza/workflow.md", "# Workflow v2\n");
        assert_ne!(standards_rev(&r.repo, &after_doc).unwrap(), before);
    }

    #[test]
    fn a_missing_listed_doc_contributes_a_sentinel() {
        let r = TestRepo::new("rev-missing");
        let base = r.commit_on_main(
            ".ai/sebenza/index.md",
            "- [W](./workflow.md)\n- [Gone](./product-guidelines.md)\n",
        );
        let entries = resolve_entries(&r.repo, &base).unwrap();
        assert!(entries.contains(&StandardsEntry {
            path: ".ai/sebenza/product-guidelines.md".into(),
            blob: None,
        }));
    }

    #[test]
    fn the_working_tree_is_never_read() {
        let r = TestRepo::new("rev-objects");
        let base = r.main_head();
        let before = standards_rev(&r.repo, &base).unwrap();
        std::fs::write(r.repo.join(".ai/sebenza/workflow.md"), "uncommitted").unwrap();
        assert_eq!(standards_rev(&r.repo, &base).unwrap(), before);
    }

    #[test]
    fn rev_of_ignores_order_and_distinguishes_missing() {
        let a = StandardsEntry {
            path: "a".into(),
            blob: Some("1".into()),
        };
        let b = missing("b");
        assert_eq!(
            rev_of(&[a.clone(), b.clone()]),
            rev_of(&[b.clone(), a.clone()])
        );
        assert_ne!(rev_of(std::slice::from_ref(&a)), rev_of(&[a, b]));
    }
}
