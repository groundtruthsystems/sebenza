//! `standards_rev`: which revision of a project's standards a review job
//! reads. It is the SHA-256 over the path-sorted (path, blob SHA) pairs of
//! `.ai/sebenza/index.md` and the documents it lists, at the merge-base, read
//! from git objects only. A listed document that is absent contributes
//! (path, `missing`), so a job over it can be told apart.
//!
//! This is the minimal resolver phase 1 needs for the job key. The phase-2
//! resolver extends [`resolve_entries`] with the path, mode, size and type
//! checks that make a job `standards-unavailable`.
#![allow(unused_imports, unused_variables)] // red: stubs until phase-1-task-2

use crate::services::git_pinned::git;
use common::domain::solution::sha256_hex;
use std::path::Path;

/// The handshake document every listed standard hangs off.
pub const INDEX_PATH: &str = ".ai/sebenza/index.md";
/// The blob value of a listed document that does not exist at the base.
pub const MISSING: &str = "missing";
const WORKSPACE: &str = ".ai/sebenza";

/// One input to the revision: a repo-relative path and its blob SHA.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StandardsEntry {
    pub path: String,
    /// `None` when the path names nothing readable at the base.
    pub blob: Option<String>,
}

/// The standards revision at `base_sha`.
pub fn standards_rev(repo: &Path, base_sha: &str) -> Result<String, String> {
    Ok(rev_of(&resolve_entries(repo, base_sha)?))
}

/// The hash of `entries`, independent of their order.
pub fn rev_of(entries: &[StandardsEntry]) -> String {
    todo!("phase-1-task-2")
}

/// `index.md` and every blob its listed documents name at `base_sha`.
pub fn resolve_entries(repo: &Path, base_sha: &str) -> Result<Vec<StandardsEntry>, String> {
    todo!("phase-1-task-2")
}

fn listed_entries(repo: &Path, base: &str, path: &str) -> Result<Vec<StandardsEntry>, String> {
    if !path.starts_with(&format!("{WORKSPACE}/")) {
        return Ok(vec![missing(path)]);
    }
    let found = blobs_at(repo, base, path)?;
    Ok(if found.is_empty() {
        vec![missing(path)]
    } else {
        found
    })
}

/// The repo paths `index.md` links to, resolved against `.ai/sebenza/`. The
/// track registry and track directories are bookkeeping, not standards. A
/// link that escapes the workspace keeps its raw form and resolves to
/// [`MISSING`].
pub fn listed_paths(index_md: &str) -> Vec<String> {
    let mut paths: Vec<String> = link_targets(index_md)
        .into_iter()
        .filter(|t| !t.contains("://") && !t.starts_with('#') && !t.starts_with("mailto:"))
        .map(|t| normalise(&t).unwrap_or(t))
        .filter(|p| !is_track_bookkeeping(p))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn is_track_bookkeeping(path: &str) -> bool {
    let rel = path.strip_prefix(&format!("{WORKSPACE}/")).unwrap_or("");
    rel == "tracks.json" || rel == "tracks" || rel.starts_with("tracks/")
}

/// Markdown `[text](target)` targets, without any `#fragment`.
fn link_targets(md: &str) -> Vec<String> {
    md.split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .map(|t| t.split('#').next().unwrap_or("").trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// `target` relative to `.ai/sebenza/` as a repo path, or `None` when it is
/// absolute or climbs out of the workspace.
fn normalise(target: &str) -> Option<String> {
    if target.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = WORKSPACE.split('/').collect();
    for seg in target.trim_end_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." if parts.len() > 2 => {
                parts.pop();
            }
            ".." => return None,
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Every regular blob at or under `path` in `rev`.
fn blobs_at(repo: &Path, rev: &str, path: &str) -> Result<Vec<StandardsEntry>, String> {
    let out = git(repo, &["ls-tree", "-r", "--full-tree", rev, "--", path])?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(parse_ls_tree_line)
        .collect())
}

/// `<mode> blob <sha>\t<path>` for a regular file; symlinks, submodules and
/// trees are not standards.
fn parse_ls_tree_line(line: &str) -> Option<StandardsEntry> {
    let (meta, path) = line.split_once('\t')?;
    let mut f = meta.split_whitespace();
    let (mode, kind, sha) = (f.next()?, f.next()?, f.next()?);
    (kind == "blob" && (mode == "100644" || mode == "100755")).then(|| StandardsEntry {
        path: path.to_string(),
        blob: Some(sha.to_string()),
    })
}

fn blob_of(e: &StandardsEntry) -> String {
    e.blob.clone().unwrap_or_default()
}

fn missing(path: &str) -> StandardsEntry {
    StandardsEntry {
        path: path.to_string(),
        blob: None,
    }
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
