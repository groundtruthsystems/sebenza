//! The in-scope change of a track (FR-08): what its first-parent commits
//! base..head changed, minus what merges from the target brought in.
//!
//! Because the base is the merge-base with the target, the tree diff
//! base→head already leaves out every change a target merge brought in: those
//! changes are in the base. The commit list still marks such merges, and
//! merges of any other branch, for the scope proof (phase 4).
//!
//! Hunks come from `git diff -M -U0` between the pinned SHAs, so renames
//! keep their history. A hunk's anchor is (path, line, side, SHA): a
//! deletion-only hunk anchors on the base side at the merge-base SHA, at its
//! old path; anything that adds lines anchors on the head side.

use super::git_pinned::{Git, GitError, RunLimits};
use super::objects::blob_sizes;
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
    /// A merge whose second parent is reachable from the target.
    TargetMerge,
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
    todo!("phase-2-task-2")
}

/// The scope diff base→head under `limits`.
pub fn scope_diff(
    git: &Git,
    pins: &SnapshotPins,
    limits: &ReviewLimits,
) -> Result<ScopeDiff, GitError> {
    todo!("phase-2-task-2")
}

/// The anchor of a hunk with the given ranges in `file`.
pub fn anchor_for(
    file: &DiffFile,
    old_start: u32,
    new_start: u32,
    new_lines: u32,
    pins: &SnapshotPins,
) -> Anchor {
    todo!("phase-2-task-2")
}

/// The standards documents `files` touch, if any.
pub fn standards_change(files: &[DiffFile]) -> Option<StandardsChange> {
    todo!("phase-2-task-2")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::TestRepo;

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
