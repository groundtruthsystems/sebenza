//! The in-scope change of a track (FR-08): what its first-parent commits
//! base..head changed, minus what merges from the target brought in.
//!
//! Because the base is the merge-base with the target, the tree diff
//! base→head already leaves out every change a target merge brought in: those
//! changes are in the base. The commit list still marks such merges, and
//! merges of any other branch, for the scope proof (phase 4). A target merge
//! whose tree differs from git's automatic merge of its parents carries
//! edits of its own, so it is marked apart and scoped like a change.
//!
//! Hunks come from `git diff -M -U0` between the pinned SHAs, so renames
//! keep their history. A hunk's anchor is (path, line, side, SHA): a
//! deletion-only hunk anchors on the base side at the merge-base SHA, at its
//! old path; anything that adds lines anchors on the head side.

use super::git_pinned::{Git, GitError, RunLimits};
use super::objects::{blob_sizes, read_blobs};
use super::reasons::{Blocker, Reason};
use super::standards_docs::{WORKSPACE, is_track_bookkeeping};
use serde::Serialize;

/// The pinned commits a review job reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotPins {
    pub head_sha: String,
    pub base_sha: String,
    pub target_sha: String,
}

/// How a first-parent commit entered the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommitKind {
    /// A single-parent commit.
    Change,
    /// A merge whose second parent is reachable from the target and whose
    /// tree is exactly git's automatic merge of its parents.
    TargetMerge,
    /// A target merge that also carries changes of its own (an edit or a
    /// conflict resolution); scoped like a change commit.
    EditedTargetMerge,
    /// A merge of anything else.
    ForeignMerge,
}

/// One commit on the first-parent chain base..head, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub kind: CommitKind,
}

/// Which tree an anchor's line is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Base,
    Head,
}

/// Where a finding about a hunk points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Anchor {
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub sha: String,
}

/// One `-U0` hunk: the old and new line ranges and its anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub anchor: Anchor,
}

/// The kind of change to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
}

/// One changed file and its hunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffFile {
    pub status: FileStatus,
    /// The path at the base; `None` for an added file.
    pub old_path: Option<String>,
    /// The path at head; `None` for a deleted file.
    pub new_path: Option<String>,
    pub new_mode: String,
    /// The blob or commit at head; `None` for a deleted file.
    pub new_oid: Option<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

impl DiffFile {
    /// The head path, or the base path of a deleted file.
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }
}

/// The caps that keep a job's inputs bounded (TS-51).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLimits {
    /// The largest blob read or handed to a reviewer.
    pub max_blob_bytes: u64,
    /// The largest scope diff, in bytes of patch text.
    pub max_diff_bytes: usize,
    /// The most hunks one job may carry.
    pub max_hunks: usize,
}

impl Default for ReviewLimits {
    fn default() -> Self {
        ReviewLimits {
            max_blob_bytes: super::standards_docs::MAX_STANDARDS_BLOB,
            max_diff_bytes: 4 * 1024 * 1024,
            max_hunks: 2_000,
        }
    }
}

/// The scope diff and why it may be unusable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeDiff {
    /// Changed files; their hunks are empty when a cap was exceeded.
    pub files: Vec<DiffFile>,
    pub blockers: Vec<Blocker>,
}

/// The track's edits to `.ai/sebenza/` outside track bookkeeping (FR-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StandardsChange {
    pub paths: Vec<String>,
}

/// The first-parent commits base..head, oldest first.
pub fn first_parent_commits(git: &Git, pins: &SnapshotPins) -> Result<Vec<ScopeCommit>, GitError> {
    let range = format!("{}..{}", pins.base_sha, pins.head_sha);
    let out = git.text(&[
        "rev-list",
        "--first-parent",
        "--parents",
        "--reverse",
        &range,
    ])?;
    out.lines()
        .map(|line| scope_commit(git, line, &pins.target_sha))
        .collect()
}

/// A `rev-list --parents` line: the commit, then its parents.
fn scope_commit(git: &Git, line: &str, target: &str) -> Result<ScopeCommit, GitError> {
    let mut shas = line.split_whitespace().map(str::to_string);
    let sha = shas.next().unwrap_or_default();
    let parents: Vec<String> = shas.collect();
    let kind = if parents.len() < 2 {
        CommitKind::Change
    } else if all_reachable(git, &parents[1..], target)? {
        target_merge_kind(git, &sha, parents.len())?
    } else {
        CommitKind::ForeignMerge
    };
    Ok(ScopeCommit { sha, parents, kind })
}

/// Whether the target merge `sha` is git's own merge. An octopus merge
/// cannot be re-merged, so it is never taken as clean.
fn target_merge_kind(git: &Git, sha: &str, parents: usize) -> Result<CommitKind, GitError> {
    if parents == 2 && remerge_paths(git, sha)?.is_empty() {
        Ok(CommitKind::TargetMerge)
    } else {
        Ok(CommitKind::EditedTargetMerge)
    }
}

/// `git diff-tree --remerge-diff` of merge `sha` as `-z --name-status`: what
/// its tree changes against a fresh automatic merge of its two parents.
pub fn remerge_paths(git: &Git, sha: &str) -> Result<Vec<u8>, GitError> {
    git.run(&[
        "diff-tree",
        "--remerge-diff",
        "-r",
        "-z",
        "-M",
        "--name-status",
        "--no-commit-id",
        sha,
    ])
}

fn all_reachable(git: &Git, commits: &[String], target: &str) -> Result<bool, GitError> {
    for c in commits {
        match git.run(&["merge-base", "--is-ancestor", c, target]) {
            Ok(_) => {}
            // Exit 1 with no message is git's "not an ancestor".
            Err(GitError::Failed(msg)) if msg.is_empty() => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// The scope diff base→head under `limits`.
pub fn scope_diff(
    git: &Git,
    pins: &SnapshotPins,
    limits: &ReviewLimits,
) -> Result<ScopeDiff, GitError> {
    let (base, head) = (pins.base_sha.as_str(), pins.head_sha.as_str());
    let raw = git.run(&["diff", "--raw", "-z", "--no-abbrev", "-M", base, head])?;
    let mut files = parse_raw(&raw);
    let mut blockers = oversized_blobs(git, &files, limits.max_blob_bytes)?;
    match patch_blocks(git, pins, limits.max_diff_bytes) {
        Err(GitError::OutputTooLarge) => blockers.push(Blocker::whole(Reason::DiffTooLarge)),
        Err(e) => return Err(e),
        Ok(blocks) => {
            if let Some(b) = attach_hunks(&mut files, blocks, pins, limits.max_hunks)? {
                blockers.push(b);
            }
        }
    }
    blockers.extend(unreviewable_binaries(git, &files, limits.max_blob_bytes)?);
    blockers.extend(type_changes(&files));
    blockers.sort();
    Ok(ScopeDiff { files, blockers })
}

/// Parse `diff --raw -z`: `:<old mode> <new mode> <old oid> <new oid>
/// <status>`, then one path, or two for a rename or copy.
fn parse_raw(raw: &[u8]) -> Vec<DiffFile> {
    let fields: Vec<String> = raw
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).to_string())
        .collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let Some(meta) = fields[i].strip_prefix(':') else {
            i += 1;
            continue;
        };
        let meta: Vec<&str> = meta.split(' ').collect();
        let letter = meta.get(4).and_then(|s| s.chars().next()).unwrap_or('M');
        let paths = if matches!(letter, 'R' | 'C') { 2 } else { 1 };
        let old = fields.get(i + 1).cloned().unwrap_or_default();
        let new = fields.get(i + paths).cloned().unwrap_or_default();
        files.push(raw_file(&meta, letter, old, new));
        i += 1 + paths;
    }
    files
}

fn raw_file(meta: &[&str], letter: char, old: String, new: String) -> DiffFile {
    let status = match letter {
        'A' => FileStatus::Added,
        'D' => FileStatus::Deleted,
        'R' => FileStatus::Renamed,
        'C' => FileStatus::Copied,
        'T' => FileStatus::TypeChanged,
        _ => FileStatus::Modified,
    };
    let new_oid = meta
        .get(3)
        .filter(|o| o.bytes().any(|b| b != b'0'))
        .map(|o| o.to_string());
    DiffFile {
        status,
        old_path: (status != FileStatus::Added).then_some(old),
        new_path: (status != FileStatus::Deleted).then_some(new),
        new_mode: meta.get(1).map(|m| m.to_string()).unwrap_or_default(),
        new_oid,
        binary: false,
        hunks: Vec::new(),
    }
}

/// A blocker per regular file at head over the blob cap.
fn oversized_blobs(git: &Git, files: &[DiffFile], max: u64) -> Result<Vec<Blocker>, GitError> {
    let regular: Vec<&DiffFile> = files
        .iter()
        .filter(|f| matches!(f.new_mode.as_str(), "100644" | "100755"))
        .collect();
    let oids: Vec<String> = regular.iter().filter_map(|f| f.new_oid.clone()).collect();
    let sizes = blob_sizes(git, &oids)?;
    Ok(regular
        .into_iter()
        .filter(|f| {
            f.new_oid
                .as_ref()
                .and_then(|o| sizes.get(o))
                .is_some_and(|s| *s > max)
        })
        .map(|f| Blocker::at(Reason::BlobTooLarge, f.path()))
        .collect())
}

/// Extensions of text a reviewer must read, whatever git's binary
/// heuristic or a `-diff` attribute says.
const REVIEWABLE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "c", "h", "cc", "cpp",
    "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "sql", "html", "css", "scss", "vue",
    "svelte", "md", "json", "yaml", "yml", "toml", "xml", "txt",
];

/// A blocker per in-scope file git calls binary that a reviewer could have
/// read: a source extension, or a blob that is valid UTF-8. Fails closed,
/// because `$GIT_DIR/info/attributes` can mark any path `-diff`.
fn unreviewable_binaries(
    git: &Git,
    files: &[DiffFile],
    max_blob: u64,
) -> Result<Vec<Blocker>, GitError> {
    let suspects: Vec<&DiffFile> = files.iter().filter(|f| binary_in_scope(f)).collect();
    let (by_name, by_content): (Vec<&DiffFile>, Vec<&DiffFile>) = suspects
        .into_iter()
        .partition(|f| has_reviewable_extension(f.path()));
    let mut blocked: Vec<&DiffFile> = by_name;
    blocked.extend(utf8_blobs(git, by_content, max_blob)?);
    Ok(blocked
        .into_iter()
        .map(|f| Blocker::at(Reason::UnreviewableBinary, f.path()))
        .collect())
}

fn binary_in_scope(f: &DiffFile) -> bool {
    f.binary
        && f.status != FileStatus::Deleted
        && matches!(f.new_mode.as_str(), "100644" | "100755")
        && !f.path().starts_with(&format!("{WORKSPACE}/"))
}

fn has_reviewable_extension(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| REVIEWABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Those of `files` whose head blob, within the blob cap, decodes as UTF-8.
/// A blob over the cap is already an oversized blocker.
fn utf8_blobs<'a>(
    git: &Git,
    files: Vec<&'a DiffFile>,
    max_blob: u64,
) -> Result<Vec<&'a DiffFile>, GitError> {
    let oids: Vec<String> = files.iter().filter_map(|f| f.new_oid.clone()).collect();
    let sizes = blob_sizes(git, &oids)?;
    let small: Vec<String> = oids
        .into_iter()
        .filter(|o| sizes.get(o).is_some_and(|s| *s <= max_blob))
        .collect();
    let total = small.iter().filter_map(|o| sizes.get(o)).sum::<u64>();
    let blobs = read_blobs(git, &small, usize::try_from(total).unwrap_or(usize::MAX))?;
    Ok(files
        .into_iter()
        .filter(|f| {
            let blob = f.new_oid.as_ref().and_then(|o| blobs.get(o));
            blob.is_some_and(|b| std::str::from_utf8(b).is_ok())
        })
        .collect())
}

/// One file's section of the patch.
#[derive(Debug, Default)]
struct PatchBlock {
    /// Its `diff --git` line.
    header: String,
    binary: bool,
    /// (old start, old lines, new start, new lines) per hunk.
    ranges: Vec<(u32, u32, u32, u32)>,
}

fn patch_blocks(
    git: &Git,
    pins: &SnapshotPins,
    max_bytes: usize,
) -> Result<Vec<PatchBlock>, GitError> {
    // Global and HEAD-tree attributes are ignored; `$GIT_DIR/info/attributes`
    // cannot be, so `unreviewable_binaries` is the real guard.
    let args = [
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "attr.tree=4b825dc642cb6eb9a060e54bf8d69288fbee4904",
        "diff",
        "-U0",
        "-M",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        &pins.base_sha,
        &pins.head_sha,
    ];
    let limits = RunLimits {
        max_output: Some(max_bytes),
        ..RunLimits::default()
    };
    let patch = git.run_limited(&args, limits)?;
    Ok(parse_patch(&String::from_utf8_lossy(&patch)))
}

fn parse_patch(patch: &str) -> Vec<PatchBlock> {
    let mut blocks: Vec<PatchBlock> = Vec::new();
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            blocks.push(PatchBlock {
                header: line.to_string(),
                ..PatchBlock::default()
            });
        } else if let Some(block) = blocks.last_mut() {
            if line.starts_with("Binary files ") || line == "GIT binary patch" {
                block.binary = true;
            } else if let Some(range) = parse_hunk_header(line) {
                block.ranges.push(range);
            }
        }
    }
    blocks
}

/// `@@ -<old>[,<n>] +<new>[,<n>] @@`.
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let (ranges, _) = line.strip_prefix("@@ -")?.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let ((os, ol), (ns, nl)) = (parse_range(old)?, parse_range(new)?);
    Some((os, ol, ns, nl))
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// Give each file its patch block's hunks, or none when there are more
/// than `max_hunks` in all.
fn attach_hunks(
    files: &mut [DiffFile],
    blocks: Vec<PatchBlock>,
    pins: &SnapshotPins,
    max_hunks: usize,
) -> Result<Option<Blocker>, GitError> {
    let blocks = pair_type_changes(files, blocks);
    if blocks.len() != files.len() {
        return Err(GitError::Failed("raw and patch diffs disagree".to_string()));
    }
    let total: usize = blocks.iter().map(|b| b.ranges.len()).sum();
    for (file, block) in files.iter_mut().zip(blocks) {
        file.binary = block.binary;
        if total > max_hunks {
            continue;
        }
        file.hunks = block
            .ranges
            .into_iter()
            .map(|(os, ol, ns, nl)| Hunk {
                old_start: os,
                old_lines: ol,
                new_start: ns,
                new_lines: nl,
                anchor: anchor_for(file, os, ns, nl, pins),
            })
            .collect();
    }
    Ok((total > max_hunks).then(|| Blocker::whole(Reason::TooManyHunks)))
}

/// A type change is one raw entry but two patch blocks under one header:
/// the old file deleted, then the new one added. Fold each pair into one.
fn pair_type_changes(files: &[DiffFile], blocks: Vec<PatchBlock>) -> Vec<PatchBlock> {
    let mut blocks = blocks.into_iter().peekable();
    let mut paired = Vec::with_capacity(files.len());
    for file in files {
        let Some(mut block) = blocks.next() else {
            break;
        };
        let twin =
            blocks.next_if(|b| file.status == FileStatus::TypeChanged && b.header == block.header);
        if let Some(twin) = twin {
            block.binary |= twin.binary;
            block.ranges.extend(twin.ranges);
        }
        paired.push(block);
    }
    paired.extend(blocks);
    paired
}

/// A blocker per file that became, or stopped being, a symlink or a
/// submodule: neither side of such a change is reviewable source.
fn type_changes(files: &[DiffFile]) -> Vec<Blocker> {
    files
        .iter()
        .filter(|f| f.status == FileStatus::TypeChanged)
        .map(|f| Blocker::at(Reason::TypeChange, f.path()))
        .collect()
}

/// The anchor of a hunk with the given ranges in `file`.
pub fn anchor_for(
    file: &DiffFile,
    old_start: u32,
    new_start: u32,
    new_lines: u32,
    pins: &SnapshotPins,
) -> Anchor {
    if new_lines == 0 {
        return Anchor {
            path: file.old_path.as_deref().unwrap_or(file.path()).to_string(),
            line: old_start,
            side: Side::Base,
            sha: pins.base_sha.clone(),
        };
    }
    Anchor {
        path: file.path().to_string(),
        line: new_start,
        side: Side::Head,
        sha: pins.head_sha.clone(),
    }
}

/// The standards documents `files` touch, if any.
pub fn standards_change(files: &[DiffFile]) -> Option<StandardsChange> {
    let prefix = format!("{WORKSPACE}/");
    let mut paths: Vec<String> = files
        .iter()
        .flat_map(|f| [f.old_path.clone(), f.new_path.clone()])
        .flatten()
        .filter(|p| p.starts_with(&prefix) && !is_track_bookkeeping(p))
        .collect();
    paths.sort();
    paths.dedup();
    (!paths.is_empty()).then_some(StandardsChange { paths })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::TestRepo;
    use crate::services::standards_review::reasons::BlockCode;

    fn pins_of(r: &TestRepo) -> SnapshotPins {
        let git = Git::open(&r.repo).unwrap();
        let head = r.head();
        let target = r.main_head();
        let base = git.text(&["merge-base", &head, &target]).unwrap();
        SnapshotPins {
            head_sha: head,
            base_sha: base,
            target_sha: target,
        }
    }

    fn diff(r: &TestRepo, limits: &ReviewLimits) -> ScopeDiff {
        scope_diff(&Git::open(&r.repo).unwrap(), &pins_of(r), limits).unwrap()
    }

    fn file<'a>(d: &'a ScopeDiff, path: &str) -> &'a DiffFile {
        d.files.iter().find(|f| f.path() == path).unwrap()
    }

    fn pins() -> SnapshotPins {
        SnapshotPins {
            head_sha: "h".repeat(40),
            base_sha: "b".repeat(40),
            target_sha: "t".repeat(40),
        }
    }

    fn renamed() -> DiffFile {
        DiffFile {
            status: FileStatus::Renamed,
            old_path: Some("old.rs".into()),
            new_path: Some("new.rs".into()),
            new_mode: "100644".into(),
            new_oid: None,
            binary: false,
            hunks: vec![],
        }
    }

    #[test]
    fn ts31_a_deletion_only_hunk_anchors_on_the_base_side() {
        let p = pins();
        assert_eq!(
            anchor_for(&renamed(), 7, 6, 0, &p),
            Anchor {
                path: "old.rs".into(),
                line: 7,
                side: Side::Base,
                sha: p.base_sha.clone(),
            }
        );
    }

    #[test]
    fn additions_and_changes_anchor_on_the_head_side() {
        let p = pins();
        let head = |line| Anchor {
            path: "new.rs".into(),
            line,
            side: Side::Head,
            sha: p.head_sha.clone(),
        };
        assert_eq!(anchor_for(&renamed(), 3, 4, 2, &p), head(4));
        assert_eq!(anchor_for(&renamed(), 0, 1, 1, &p), head(1));
    }

    #[test]
    fn ts31_a_real_deletion_anchors_at_the_merge_base() {
        let r = TestRepo::new("scope-ts31");
        r.commit_on_main("lib.rs", "one\ntwo\nthree\n");
        r.merge_main_into_feat();
        let base_before = pins_of(&r).base_sha;
        r.commit_on_feat("lib.rs", "one\nthree\n");
        let p = pins_of(&r);
        assert_eq!(p.base_sha, base_before);
        let d = diff(&r, &ReviewLimits::default());
        let hunks = &file(&d, "lib.rs").hunks;
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].anchor,
            Anchor {
                path: "lib.rs".into(),
                line: 2,
                side: Side::Base,
                sha: p.base_sha,
            }
        );
    }

    #[test]
    fn hunks_carry_ranges_and_renames_keep_their_old_path() {
        let r = TestRepo::new("scope-rename");
        let body: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        r.commit_on_main("old.rs", &body);
        r.merge_main_into_feat();
        crate::services::review_test_repo::git(&r.worktree, &["mv", "old.rs", "new.rs"]);
        r.commit_on_feat("new.rs", &body.replace("line 10\n", "line ten\nline 10b\n"));
        let d = diff(&r, &ReviewLimits::default());
        let f = file(&d, "new.rs");
        assert_eq!(f.status, FileStatus::Renamed);
        assert_eq!(f.old_path.as_deref(), Some("old.rs"));
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (10, 1, 10, 2)
        );
        assert_eq!(h.anchor.side, Side::Head);
    }

    #[test]
    fn added_deleted_and_binary_files_are_listed() {
        let r = TestRepo::new("scope-kinds");
        r.commit_on_main("gone.rs", "a\nb\n");
        r.merge_main_into_feat();
        r.remove_on_feat("gone.rs");
        r.commit_on_feat("img.bin", "\0\x01\x02");
        let d = diff(&r, &ReviewLimits::default());
        let gone = file(&d, "gone.rs");
        assert_eq!(gone.status, FileStatus::Deleted);
        assert_eq!(gone.new_path, None);
        assert_eq!(gone.hunks[0].anchor.side, Side::Base);
        assert!(file(&d, "img.bin").binary);
        assert_eq!(file(&d, "src.rs").status, FileStatus::Added);
    }

    fn binary_blockers(d: &ScopeDiff) -> Vec<&Blocker> {
        d.blockers
            .iter()
            .filter(|b| b.reason == Reason::UnreviewableBinary)
            .collect()
    }

    #[test]
    fn a_nul_byte_cannot_hide_a_source_file_from_review() {
        let r = TestRepo::new("scope-nul");
        r.commit_bytes_on_feat("src/hidden.rs", b"fn evil() {}\0\n");
        let d = diff(&r, &ReviewLimits::default());
        assert!(file(&d, "src/hidden.rs").binary);
        assert_eq!(
            binary_blockers(&d),
            vec![&Blocker::at(Reason::UnreviewableBinary, "src/hidden.rs")]
        );
        assert_eq!(d.blockers[0].code, BlockCode::Unreviewable);
    }

    #[test]
    fn a_diff_attribute_in_the_git_dir_cannot_hide_a_source_file() {
        let r = TestRepo::new("scope-attr");
        let info = r.repo.join(".git/info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("attributes"), "*.rs -diff\n").unwrap();
        r.commit_on_feat("src/a.rs", "fn a() {}\n");
        let d = diff(&r, &ReviewLimits::default());
        let blocked: Vec<_> = binary_blockers(&d).iter().map(|b| b.path.clone()).collect();
        assert!(
            blocked.contains(&Some("src/a.rs".to_string())),
            "{blocked:?}"
        );
    }

    #[test]
    fn a_real_binary_asset_is_not_blocked() {
        let r = TestRepo::new("scope-png");
        r.commit_bytes_on_feat("img/logo.png", &[0x89, b'P', b'N', b'G', 0xff, 0x00, 0xfe]);
        let d = diff(&r, &ReviewLimits::default());
        assert!(file(&d, "img/logo.png").binary);
        assert!(binary_blockers(&d).is_empty(), "{:?}", d.blockers);
    }

    #[test]
    fn a_file_turned_into_a_symlink_is_a_blocker_not_an_error() {
        let r = TestRepo::new("scope-typechange");
        r.commit_on_main("lib.rs", "fn lib() {}\n");
        r.merge_main_into_feat();
        r.symlink_on_feat("lib.rs", "/etc/passwd");
        r.commit_on_feat("src/after.rs", "fn after() {}\n");
        let d = diff(&r, &ReviewLimits::default());
        assert_eq!(file(&d, "lib.rs").status, FileStatus::TypeChanged);
        assert_eq!(d.blockers, vec![Blocker::at(Reason::TypeChange, "lib.rs")]);
        assert!(
            !file(&d, "src/after.rs").hunks.is_empty(),
            "later files keep their hunks"
        );
    }

    #[test]
    fn unusual_file_names_keep_their_exact_paths() {
        let r = TestRepo::new("scope-names");
        r.commit_on_feat("dir with space/\u{fc}n\u{ef} \"q\".rs", "x\n");
        let d = diff(&r, &ReviewLimits::default());
        let f = file(&d, "dir with space/\u{fc}n\u{ef} \"q\".rs");
        assert_eq!(f.hunks[0].anchor.path, f.path());
    }

    #[test]
    fn changes_merged_from_the_target_are_out_of_scope() {
        let r = TestRepo::new("scope-merge");
        r.commit_on_main("upstream.rs", "fn up() {}\n");
        r.commit_on_feat("mine.rs", "fn mine() {}\n");
        r.merge_main_into_feat();
        let d = diff(&r, &ReviewLimits::default());
        let paths: Vec<_> = d.files.iter().map(|f| f.path()).collect();
        assert!(!paths.contains(&"upstream.rs"), "{paths:?}");
        assert!(paths.contains(&"mine.rs"));
    }

    #[test]
    fn first_parent_commits_mark_target_and_foreign_merges() {
        let r = TestRepo::new("scope-commits");
        r.commit_on_main("upstream.rs", "fn up() {}\n");
        r.commit_on_feat("mine.rs", "fn mine() {}\n");
        let target_merge = r.merge_main_into_feat();
        let g = |args: &[&str]| crate::services::review_test_repo::git(&r.worktree, args);
        g(&["checkout", "-q", "-b", "side", "HEAD~1"]);
        r.commit_on_feat("side.rs", "fn side() {}\n");
        g(&["checkout", "-q", "feat"]);
        g(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "merge",
            "-q",
            "--no-ff",
            "--no-edit",
            "side",
        ]);
        let p = pins_of(&r);
        let commits = first_parent_commits(&Git::open(&r.repo).unwrap(), &p).unwrap();
        let kinds: Vec<_> = commits.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CommitKind::Change,
                CommitKind::Change,
                CommitKind::TargetMerge,
                CommitKind::ForeignMerge
            ]
        );
        assert_eq!(commits[2].sha, target_merge);
        assert_eq!(commits[3].sha, p.head_sha);
        assert_eq!(commits[3].parents.len(), 2);
    }

    #[test]
    fn ts17_a_standards_edit_is_detected_and_bookkeeping_is_not() {
        let r = TestRepo::new("scope-ts17");
        r.commit_on_feat(".ai/sebenza/tracks/t/plan.json", "{}\n");
        let d = diff(&r, &ReviewLimits::default());
        assert_eq!(standards_change(&d.files), None);
        r.commit_on_feat(".ai/sebenza/workflow.md", "# Weaker\n");
        let d = diff(&r, &ReviewLimits::default());
        assert_eq!(
            standards_change(&d.files),
            Some(StandardsChange {
                paths: vec![".ai/sebenza/workflow.md".into()]
            })
        );
    }

    #[test]
    fn ts51_a_diff_over_its_cap_is_blocked_without_hunks() {
        let r = TestRepo::new("scope-ts51-diff");
        r.commit_on_feat("big.rs", &"let x = 1;\n".repeat(500));
        let limits = ReviewLimits {
            max_diff_bytes: 1024,
            ..ReviewLimits::default()
        };
        let d = diff(&r, &limits);
        assert_eq!(d.blockers, vec![Blocker::whole(Reason::DiffTooLarge)]);
        assert!(d.files.iter().all(|f| f.hunks.is_empty()));
    }

    #[test]
    fn ts51_too_many_hunks_are_blocked() {
        let r = TestRepo::new("scope-ts51-hunks");
        r.commit_on_feat("a.rs", "a\n");
        r.commit_on_feat("b.rs", "b\n");
        let limits = ReviewLimits {
            max_hunks: 2,
            ..ReviewLimits::default()
        };
        let d = diff(&r, &limits);
        assert_eq!(d.blockers, vec![Blocker::whole(Reason::TooManyHunks)]);
        assert!(d.files.iter().all(|f| f.hunks.is_empty()));
    }

    #[test]
    fn ts51_an_in_scope_blob_over_its_cap_is_blocked() {
        let r = TestRepo::new("scope-ts51-blob");
        r.commit_on_feat("huge.txt", &"y".repeat(2048));
        let limits = ReviewLimits {
            max_blob_bytes: 1024,
            max_diff_bytes: 1 << 20,
            ..ReviewLimits::default()
        };
        let d = diff(&r, &limits);
        assert_eq!(
            d.blockers,
            vec![Blocker::at(Reason::BlobTooLarge, "huge.txt")]
        );
    }
}
