//! The project's standards at the merge-base (FR-07, D-02): `index.md` and
//! the documents it links to, read from git objects only.
//!
//! An index link is resolved against `.ai/sebenza/`. It must be relative, use
//! no `..`, `.` (beyond one leading `./`), empty segment or backslash, and
//! stay under `.ai/sebenza/`. A link to a directory takes every blob under
//! it. Each named entry must be a regular blob (not a symlink or submodule),
//! UTF-8 text without NUL bytes, and within the blob cap. Anything else, and
//! any listed path that names nothing, is a `standards-unavailable` blocker;
//! a missing path also enters the revision as (path, `missing`). The track
//! registry and track directories are bookkeeping and never standards.

use super::git_pinned::{Git, GitError};
use super::objects::{blob_sizes, read_blobs};
use super::reasons::{Blocker, Reason};
use super::standards_rev::{INDEX_PATH, StandardsEntry, missing};

/// The workspace every standards document lives under.
pub const WORKSPACE: &str = ".ai/sebenza";
/// The default cap on one blob the resolver reads (256 KiB).
pub const MAX_STANDARDS_BLOB: u64 = 256 * 1024;

/// One standards document's text, as stored at the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardsDoc {
    pub path: String,
    pub blob: String,
    pub text: String,
}

/// Everything the standards resolution found at one base.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Standards {
    /// The revision inputs, sorted and deduplicated.
    pub entries: Vec<StandardsEntry>,
    /// The readable documents, `index.md` first.
    pub docs: Vec<StandardsDoc>,
    pub blockers: Vec<Blocker>,
}

/// A `git ls-tree` row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeEntry {
    mode: String,
    oid: String,
    path: String,
}

/// Resolve the standards at `base`, reading no blob over `max_blob` bytes.
pub fn resolve_standards(git: &Git, base: &str, max_blob: u64) -> Result<Standards, GitError> {
    let mut acc = Standards::default();
    let index_tree = ls_tree(git, base, &[INDEX_PATH.to_string()])?;
    let index_blobs = admit(&mut acc, INDEX_PATH, index_tree);
    let Some(index) = load(git, &index_blobs, max_blob, &mut acc)?.pop() else {
        return Ok(finish(acc));
    };
    let listed = listed_repo_paths(&index.text, &mut acc);
    let tree = ls_tree(git, base, &listed)?;
    let mut blobs = Vec::new();
    for path in &listed {
        blobs.extend(admit(&mut acc, path, under(&tree, path)));
    }
    let mut docs = load(git, &blobs, max_blob, &mut acc)?;
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    docs.dedup_by(|a, b| a.path == b.path);
    docs.retain(|d| d.path != INDEX_PATH);
    acc.docs = std::iter::once(index).chain(docs).collect();
    Ok(finish(acc))
}

fn finish(mut acc: Standards) -> Standards {
    acc.entries.sort();
    acc.entries.dedup();
    acc.blockers.sort();
    acc.blockers.dedup();
    acc
}

/// The workspace paths `index_md` lists; each refused link becomes a
/// blocker and a (link, `missing`) revision entry.
fn listed_repo_paths(index_md: &str, acc: &mut Standards) -> Vec<String> {
    let mut paths = Vec::new();
    for target in link_targets(index_md) {
        match classify_link(&target) {
            Ok(path) if is_track_bookkeeping(&path) => {}
            Ok(path) => paths.push(path),
            Err(reason) => {
                acc.entries.push(missing(&target));
                acc.blockers.push(Blocker::at(reason, &target));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Record what `path` names in the revision, and return its regular blobs.
fn admit(acc: &mut Standards, path: &str, found: Vec<TreeEntry>) -> Vec<TreeEntry> {
    if found.is_empty() {
        acc.entries.push(missing(path));
        acc.blockers.push(Blocker::at(Reason::Missing, path));
    }
    let mut regular = Vec::new();
    for e in found {
        acc.entries.push(StandardsEntry {
            path: e.path.clone(),
            blob: Some(e.oid.clone()),
        });
        match e.mode.as_str() {
            "100644" | "100755" => regular.push(e),
            "120000" => acc.blockers.push(Blocker::at(Reason::Symlink, &e.path)),
            "160000" => acc.blockers.push(Blocker::at(Reason::Submodule, &e.path)),
            _ => acc.blockers.push(Blocker::at(Reason::Binary, &e.path)),
        }
    }
    regular
}

/// Read `blobs` as text; an oversized, binary or non-UTF-8 blob becomes a
/// blocker instead of a document, and an oversized one is never read.
fn load(
    git: &Git,
    blobs: &[TreeEntry],
    max_blob: u64,
    acc: &mut Standards,
) -> Result<Vec<StandardsDoc>, GitError> {
    let oids: Vec<String> = blobs.iter().map(|b| b.oid.clone()).collect();
    let sizes = blob_sizes(git, &oids)?;
    let (small, big): (Vec<&TreeEntry>, Vec<&TreeEntry>) = blobs
        .iter()
        .partition(|b| sizes.get(&b.oid).is_some_and(|s| *s <= max_blob));
    for b in big {
        acc.blockers.push(Blocker::at(Reason::Oversized, &b.path));
    }
    let small_oids: Vec<String> = small.iter().map(|b| b.oid.clone()).collect();
    let total = small_oids.iter().filter_map(|o| sizes.get(o)).sum::<u64>();
    let contents = read_blobs(git, &small_oids, total as usize)?;
    Ok(small
        .into_iter()
        .filter_map(|b| as_doc(b, contents.get(&b.oid), acc))
        .collect())
}

fn as_doc(b: &TreeEntry, bytes: Option<&Vec<u8>>, acc: &mut Standards) -> Option<StandardsDoc> {
    let bytes = bytes.map(Vec::as_slice).unwrap_or_default();
    let text = match std::str::from_utf8(bytes) {
        _ if bytes.contains(&0) => Err(Reason::Binary),
        Ok(text) => Ok(text.to_string()),
        Err(_) => Err(Reason::NotUtf8),
    };
    text.map_err(|reason| acc.blockers.push(Blocker::at(reason, &b.path)))
        .ok()
        .map(|text| StandardsDoc {
            path: b.path.clone(),
            blob: b.oid.clone(),
            text,
        })
}

/// Every entry at or under `paths` in `rev`, recursively.
fn ls_tree(git: &Git, rev: &str, paths: &[String]) -> Result<Vec<TreeEntry>, GitError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", rev, "--"];
    args.extend(paths.iter().map(String::as_str));
    let out = git.run(&args)?;
    Ok(out
        .split(|b| *b == 0)
        .filter_map(|rec| parse_ls_tree_record(&String::from_utf8_lossy(rec)))
        .collect())
}

/// `<mode> <type> <oid>\t<path>`.
fn parse_ls_tree_record(rec: &str) -> Option<TreeEntry> {
    let (meta, path) = rec.split_once('\t')?;
    let mut f = meta.split_whitespace();
    let (mode, _kind, oid) = (f.next()?, f.next()?, f.next()?);
    Some(TreeEntry {
        mode: mode.to_string(),
        oid: oid.to_string(),
        path: path.to_string(),
    })
}

fn under(tree: &[TreeEntry], path: &str) -> Vec<TreeEntry> {
    let dir = format!("{path}/");
    tree.iter()
        .filter(|e| e.path == path || e.path.starts_with(&dir))
        .cloned()
        .collect()
}

/// The raw link targets of `index_md`, minus URLs, fragments-only links and
/// mail links, each without its `#fragment` or title.
pub fn link_targets(index_md: &str) -> Vec<String> {
    index_md
        .split("](")
        .skip(1)
        .filter_map(link_target)
        .filter(|t| !t.contains("://") && !t.starts_with("mailto:"))
        .collect()
}

/// The target at the start of `rest`, the text after a link's `](`.
fn link_target(rest: &str) -> Option<String> {
    let raw = match rest.strip_prefix('<') {
        Some(bracketed) => bracketed.split('>').next()?,
        None => rest.split(')').next()?.split_whitespace().next()?,
    };
    let target = raw.split('#').next().unwrap_or_default().trim();
    (!target.is_empty()).then(|| target.to_string())
}

/// The repo path an index link names, or why it is refused.
pub fn classify_link(target: &str) -> Result<String, Reason> {
    if is_absolute(target) {
        return Err(Reason::AbsolutePath);
    }
    let rel = target.strip_prefix("./").unwrap_or(target);
    let rel = rel.strip_suffix('/').unwrap_or(rel);
    let segments: Vec<&str> = rel.split('/').collect();
    if segments.contains(&"..") {
        return Err(traversal_reason(&segments));
    }
    let odd_segment = segments.iter().any(|s| s.is_empty() || *s == ".");
    if odd_segment || rel.contains('\\') || rel.chars().any(char::is_control) {
        return Err(Reason::NonNormalisedPath);
    }
    Ok(format!("{WORKSPACE}/{rel}"))
}

/// A `..` that climbs above `.ai/sebenza/` leaves the workspace; one that
/// stays inside is still refused, as an unnormalised traversal.
fn traversal_reason(segments: &[&str]) -> Reason {
    let mut depth = 0i32;
    for s in segments {
        depth += if *s == ".." { -1 } else { 1 };
        if depth < 0 {
            return Reason::OutsideWorkspace;
        }
    }
    Reason::PathTraversal
}

fn is_absolute(target: &str) -> bool {
    let b = target.as_bytes();
    let drive = b.len() > 1 && b[0].is_ascii_alphabetic() && b[1] == b':';
    target.starts_with('/') || target.starts_with('~') || drive
}

/// Whether `path` is the track registry or under the tracks directory.
pub fn is_track_bookkeeping(path: &str) -> bool {
    let rel = path.strip_prefix(&format!("{WORKSPACE}/")).unwrap_or("");
    rel == "tracks.json" || rel == "tracks" || rel.starts_with("tracks/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::TestRepo;

    const CAP: u64 = MAX_STANDARDS_BLOB;

    fn resolved(r: &TestRepo) -> Standards {
        let git = Git::open(&r.repo).unwrap();
        resolve_standards(&git, &r.main_head(), CAP).unwrap()
    }

    fn reasons(s: &Standards) -> Vec<(Reason, String)> {
        s.blockers
            .iter()
            .map(|b| (b.reason, b.path.clone().unwrap_or_default()))
            .collect()
    }

    fn with_index(r: &TestRepo, links: &str) {
        r.commit_on_main(".ai/sebenza/index.md", &format!("# Project\n{links}"));
    }

    #[test]
    fn index_links_are_extracted_without_fragments_titles_or_urls() {
        let md = "- [W](./workflow.md)\n- [S](./SECURITY.md#threats)\n\
                  - [T](./tech.md \"Tech\")\n- [X](https://example.com)\n\
                  - [A](#top) [M](mailto:a@b)\n- [G](<./code style/>)\n";
        assert_eq!(
            link_targets(md),
            vec![
                "./workflow.md",
                "./SECURITY.md",
                "./tech.md",
                "./code style/"
            ]
        );
    }

    #[test]
    fn links_resolve_under_the_workspace() {
        assert_eq!(
            classify_link("./workflow.md"),
            Ok(".ai/sebenza/workflow.md".into())
        );
        assert_eq!(
            classify_link("code_styleguides/"),
            Ok(".ai/sebenza/code_styleguides".into())
        );
    }

    #[test]
    fn absolute_escaping_and_unnormalised_links_are_refused() {
        for (link, reason) in [
            ("/etc/passwd", Reason::AbsolutePath),
            ("~/notes.md", Reason::AbsolutePath),
            ("C:/x.md", Reason::AbsolutePath),
            ("../../etc/passwd", Reason::OutsideWorkspace),
            ("../sebenza/workflow.md", Reason::OutsideWorkspace),
            ("./code_styleguides/../workflow.md", Reason::PathTraversal),
            ("./a//b.md", Reason::NonNormalisedPath),
            ("./a/./b.md", Reason::NonNormalisedPath),
            ("a\\b.md", Reason::NonNormalisedPath),
            ("a\u{7}.md", Reason::NonNormalisedPath),
            ("", Reason::NonNormalisedPath),
        ] {
            assert_eq!(classify_link(link), Err(reason), "{link}");
        }
    }

    #[test]
    fn the_fixture_standards_load_as_text() {
        let r = TestRepo::new("docs-ok");
        let s = resolved(&r);
        assert!(s.blockers.is_empty(), "{:?}", s.blockers);
        let paths: Vec<_> = s.docs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                ".ai/sebenza/index.md",
                ".ai/sebenza/code_styleguides/general.md",
                ".ai/sebenza/workflow.md",
            ]
        );
        assert_eq!(s.docs[2].text, "# Workflow\n");
    }

    #[test]
    fn unlisted_workspace_documents_are_not_loaded() {
        let r = TestRepo::new("docs-d02");
        r.commit_on_main(
            ".ai/sebenza/agent-preferences.md",
            "- [rule:x category=y] z\n",
        );
        let s = resolved(&r);
        assert!(
            s.docs
                .iter()
                .all(|d| !d.path.ends_with("agent-preferences.md"))
        );
    }

    #[test]
    fn ts09_an_absolute_or_escaping_entry_is_unavailable() {
        let r = TestRepo::new("docs-ts09");
        with_index(
            &r,
            "- [W](./workflow.md)\n- [P](/etc/passwd)\n- [E](../../src.rs)\n",
        );
        let s = resolved(&r);
        assert_eq!(
            reasons(&s),
            vec![
                (Reason::AbsolutePath, "/etc/passwd".into()),
                (Reason::OutsideWorkspace, "../../src.rs".into()),
            ]
        );
        assert!(s.docs.iter().all(|d| d.path.starts_with(".ai/sebenza/")));
    }

    #[test]
    fn ts10_a_symlinked_doc_is_refused_and_its_target_never_read() {
        let r = TestRepo::new("docs-ts10-link");
        r.commit_on_main("outside.md", "OUTSIDE-SENTINEL\n");
        r.symlink_on_main(".ai/sebenza/workflow.md", "../../outside.md");
        let s = resolved(&r);
        assert_eq!(
            reasons(&s),
            vec![(Reason::Symlink, ".ai/sebenza/workflow.md".into())]
        );
        assert!(s.docs.iter().all(|d| !d.text.contains("OUTSIDE-SENTINEL")));
    }

    #[test]
    fn ts10_a_symlinked_index_is_refused() {
        let r = TestRepo::new("docs-ts10-index");
        r.commit_on_main("outside.md", "- [W](./workflow.md)\n");
        r.symlink_on_main(".ai/sebenza/index.md", "../../outside.md");
        let s = resolved(&r);
        assert_eq!(
            reasons(&s),
            vec![(Reason::Symlink, ".ai/sebenza/index.md".into())]
        );
        assert!(s.docs.is_empty());
    }

    #[test]
    fn ts10_a_submodule_in_a_listed_directory_is_refused() {
        let r = TestRepo::new("docs-ts10-sub");
        r.gitlink_on_main(".ai/sebenza/code_styleguides/vendored");
        let s = resolved(&r);
        assert_eq!(
            reasons(&s),
            vec![(
                Reason::Submodule,
                ".ai/sebenza/code_styleguides/vendored".into()
            )]
        );
    }

    #[test]
    fn ts11_a_missing_doc_is_unavailable_with_a_sentinel() {
        let r = TestRepo::new("docs-ts11-missing");
        with_index(
            &r,
            "- [W](./workflow.md)\n- [Gone](./product-guidelines.md)\n",
        );
        let s = resolved(&r);
        let path = ".ai/sebenza/product-guidelines.md";
        assert_eq!(reasons(&s), vec![(Reason::Missing, path.into())]);
        assert!(s.entries.contains(&StandardsEntry {
            path: path.into(),
            blob: None
        }));
    }

    #[test]
    fn ts11_a_missing_index_is_unavailable() {
        let r = TestRepo::new("docs-ts11-noindex");
        crate::services::review_test_repo::git(&r.repo, &["rm", "-q", INDEX_PATH]);
        crate::services::review_test_repo::git(&r.repo, &["commit", "-qm", "rm"]);
        let s = resolved(&r);
        assert_eq!(reasons(&s), vec![(Reason::Missing, INDEX_PATH.into())]);
    }

    #[test]
    fn ts11_an_oversized_doc_is_unavailable_and_not_read() {
        let r = TestRepo::new("docs-ts11-big");
        r.commit_on_main(".ai/sebenza/workflow.md", &"x".repeat(256 * 1024 + 1));
        let s = resolved(&r);
        assert_eq!(
            reasons(&s),
            vec![(Reason::Oversized, ".ai/sebenza/workflow.md".into())]
        );
        assert!(s.docs.iter().all(|d| !d.path.ends_with("workflow.md")));
    }

    #[test]
    fn ts11_binary_and_non_utf8_docs_are_unavailable() {
        let r = TestRepo::new("docs-ts11-bin");
        r.commit_bytes_on_main(".ai/sebenza/workflow.md", b"PNG\0\x01\x02");
        r.commit_bytes_on_main(".ai/sebenza/code_styleguides/general.md", b"caf\xe9\n");
        let s = resolved(&r);
        assert_eq!(
            reasons(&s),
            vec![
                (
                    Reason::NotUtf8,
                    ".ai/sebenza/code_styleguides/general.md".into()
                ),
                (Reason::Binary, ".ai/sebenza/workflow.md".into()),
            ]
        );
    }

    #[test]
    fn a_doc_exactly_at_the_cap_is_read() {
        let r = TestRepo::new("docs-at-cap");
        r.commit_on_main(".ai/sebenza/workflow.md", &"x".repeat(256 * 1024));
        assert!(resolved(&r).blockers.is_empty());
    }
}
