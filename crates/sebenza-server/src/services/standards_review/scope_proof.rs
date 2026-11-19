//! The scope proof (FR-09, FR-21): whether a job's commits are provably its
//! track's own, and which in-scope files they changed outside their task's
//! approved edit boundary.
//!
//! Scope is unproven, and the job non-passing, when the base is missing or
//! not under the head, the head is off the track's branch, the plan was never
//! approved, a merge brought in anything but the target, or a change commit
//! lacks a `Sebenza-Track: <track>` and `Sebenza-Task: <approved task>`
//! trailer pair. Target merges need no trailers; their changes are already
//! outside the scope diff.
//!
//! Each change commit's paths are checked against the union of its tasks'
//! boundaries, ignoring track bookkeeping. An in-scope file a commit changed
//! out of bounds yields one boundary finding anchored at the file's first
//! hunk. When the plan's content changed after approval, the boundary is
//! stale and every in-scope file is out of bounds until the operator
//! approves again.

use super::boundary::in_boundary;
use super::deriver::BoundaryFinding;
use super::git_pinned::{Git, GitError, RunLimits};
use super::plan_approval::{content_sha256_of, plan_at};
use super::reasons::{Blocker, Reason};
use super::resolver::ReviewInputs;
use super::scope::{Anchor, CommitKind, DiffFile, ScopeCommit, Side, SnapshotPins};
use super::standards_docs::is_track_bookkeeping;
use common::domain::review_records::{AnchorRecord, AnchorSide, PlanApproved};
use std::collections::{BTreeMap, BTreeSet};

/// The trailer naming a commit's track.
pub const TRACK_TRAILER: &str = "Sebenza-Track";
/// The trailer naming a commit's task.
pub const TASK_TRAILER: &str = "Sebenza-Task";

/// What the scope proof decided for one job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeProof {
    /// Non-empty when scope is unproven.
    pub blockers: Vec<Blocker>,
    pub out_of_scope: Vec<BoundaryFinding>,
}

/// The `Sebenza-*` trailers of one commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trailers {
    pub tracks: Vec<String>,
    pub tasks: Vec<String>,
}

/// Blockers about `pins` themselves: a base that is missing or not under
/// the head, or a head not reachable from `branch`.
pub fn prove_pins(git: &Git, pins: &SnapshotPins, branch: &str) -> Result<Vec<Blocker>, GitError> {
    todo!()
}

/// Prove the commits of `inputs` belong to `track_id` under `approval`,
/// then find the in-scope files changed outside their boundaries.
pub fn prove_scope(
    git: &Git,
    track_id: &str,
    inputs: &ReviewInputs,
    approval: Option<&PlanApproved>,
) -> Result<ScopeProof, GitError> {
    todo!()
}

fn blocked(mut blockers: Vec<Blocker>) -> ScopeProof {
    blockers.sort();
    ScopeProof {
        blockers,
        out_of_scope: Vec::new(),
    }
}

fn is_commit(git: &Git, rev: &str) -> Result<bool, GitError> {
    exits_zero(git.run(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{rev}^{{commit}}"),
    ]))
}

fn is_ancestor(git: &Git, ancestor: &str, of: &str) -> Result<bool, GitError> {
    exits_zero(git.run(&["merge-base", "--is-ancestor", ancestor, of]))
}

/// Git's silent exit 1 is a "no"; anything else that failed is an error.
fn exits_zero(result: Result<Vec<u8>, GitError>) -> Result<bool, GitError> {
    match result {
        Ok(_) => Ok(true),
        Err(GitError::Failed(msg)) if msg.is_empty() => Ok(false),
        Err(e) => Err(e),
    }
}

/// The `Sebenza-*` trailers of each of `commits`, by SHA.
pub fn commit_trailers(
    git: &Git,
    commits: &[&ScopeCommit],
) -> Result<BTreeMap<String, Trailers>, GitError> {
    if commits.is_empty() {
        return Ok(BTreeMap::new());
    }
    let shas: Vec<&str> = commits.iter().map(|c| c.sha.as_str()).collect();
    let limits = RunLimits {
        input: Some(format!("{}\n", shas.join("\n")).into_bytes()),
        ..RunLimits::default()
    };
    let format = "--format=%H%x1f%(trailers:only,unfold)%x1e";
    let out = git.run_limited(&["log", "--no-walk=unsorted", "--stdin", format], limits)?;
    Ok(parse_trailer_log(&String::from_utf8_lossy(&out)))
}

/// Records of `<sha> 0x1f <trailer lines> 0x1e`.
fn parse_trailer_log(out: &str) -> BTreeMap<String, Trailers> {
    out.split('\u{1e}')
        .filter_map(|record| record.trim_start().split_once('\u{1f}'))
        .map(|(sha, block)| (sha.to_string(), parse_trailers(block)))
        .collect()
}

fn parse_trailers(block: &str) -> Trailers {
    let mut t = Trailers::default();
    for (key, value) in block.lines().filter_map(|l| l.split_once(':')) {
        let (key, value) = (key.trim(), value.trim().to_string());
        if key.eq_ignore_ascii_case(TRACK_TRAILER) {
            t.tracks.push(value);
        } else if key.eq_ignore_ascii_case(TASK_TRAILER) {
            t.tasks.push(value);
        }
    }
    t
}

fn merge_blockers(commits: &[ScopeCommit]) -> Vec<Blocker> {
    commits
        .iter()
        .filter(|c| c.kind == CommitKind::ForeignMerge)
        .map(|c| Blocker::at_commit(Reason::ForeignMerge, &c.sha))
        .collect()
}

fn trailer_blockers(
    track_id: &str,
    approval: &PlanApproved,
    changes: &[&ScopeCommit],
    trailers: &BTreeMap<String, Trailers>,
) -> Vec<Blocker> {
    let tasks: BTreeSet<&str> = approval
        .boundaries
        .iter()
        .map(|b| b.task_id.as_str())
        .collect();
    let none = Trailers::default();
    changes
        .iter()
        .filter_map(|c| {
            let t = trailers.get(&c.sha).unwrap_or(&none);
            let reason = trailer_problem(track_id, &tasks, t)?;
            Some(Blocker::at_commit(reason, &c.sha))
        })
        .collect()
}

fn trailer_problem(track_id: &str, tasks: &BTreeSet<&str>, t: &Trailers) -> Option<Reason> {
    if t.tracks.is_empty() || t.tasks.is_empty() {
        Some(Reason::MissingTrailer)
    } else if t.tracks.iter().any(|track| track != track_id) {
        Some(Reason::ForeignTrailer)
    } else if t.tasks.iter().any(|task| !tasks.contains(task.as_str())) {
        Some(Reason::UnknownTask)
    } else {
        None
    }
}

/// Whether the plan at `head` differs in content from the approved one.
fn is_stale(
    git: &Git,
    head: &str,
    track_id: &str,
    approval: &PlanApproved,
) -> Result<bool, GitError> {
    let content = plan_at(git, head, track_id)?.and_then(|b| content_sha256_of(&b));
    Ok(content.as_deref() != Some(approval.plan_content_sha256.as_str()))
}

/// Paths some change commit edited outside its tasks' boundaries, with the
/// tasks that did so.
fn edits_outside(
    git: &Git,
    approval: &PlanApproved,
    changes: &[&ScopeCommit],
    trailers: &BTreeMap<String, Trailers>,
) -> Result<BTreeMap<String, BTreeSet<String>>, GitError> {
    let mut outside: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for c in changes {
        let tasks = trailers
            .get(&c.sha)
            .map(|t| t.tasks.clone())
            .unwrap_or_default();
        let globs = boundary_of(approval, &tasks);
        for path in changed_paths(git, c)? {
            if !is_track_bookkeeping(&path) && !in_boundary(&globs, &path) {
                outside
                    .entry(path)
                    .or_default()
                    .extend(tasks.iter().cloned());
            }
        }
    }
    Ok(outside)
}

/// The union of `tasks`' globs.
fn boundary_of(approval: &PlanApproved, tasks: &[String]) -> Vec<String> {
    approval
        .boundaries
        .iter()
        .filter(|b| tasks.contains(&b.task_id))
        .flat_map(|b| b.files.iter().cloned())
        .collect()
}

/// Every path, old and new, that commit `c` changed against its parent.
fn changed_paths(git: &Git, c: &ScopeCommit) -> Result<Vec<String>, GitError> {
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "-M",
        "--name-status",
        "--no-commit-id",
    ];
    match c.parents.first() {
        Some(parent) => args.push(parent),
        None => args.push("--root"),
    }
    args.push(&c.sha);
    let out = git.run(&args)?;
    Ok(parse_name_status(&String::from_utf8_lossy(&out)))
}

/// `-z --name-status`: a status field, then one path, or two for a rename
/// or copy.
fn parse_name_status(out: &str) -> Vec<String> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut paths = Vec::new();
    while let Some(status) = fields.next() {
        let count = if status.starts_with(['R', 'C']) { 2 } else { 1 };
        paths.extend(fields.by_ref().take(count).map(String::from));
    }
    paths
}

fn boundary_findings(
    files: &[DiffFile],
    outside: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<BoundaryFinding> {
    files
        .iter()
        .filter_map(|f| {
            let paths = [f.old_path.as_ref(), f.new_path.as_ref()];
            let tasks = paths.into_iter().flatten().find_map(|p| outside.get(p))?;
            let tasks: Vec<&str> = tasks.iter().map(String::as_str).collect();
            let why = format!(
                "{} was changed by task {} outside its approved edit boundary.",
                f.path(),
                tasks.join(", ")
            );
            boundary_finding(f, why)
        })
        .collect()
}

fn stale_findings(files: &[DiffFile]) -> Vec<BoundaryFinding> {
    files
        .iter()
        .filter(|f| !is_track_bookkeeping(f.path()))
        .filter_map(|f| {
            let why = format!(
                "{} was changed while the edit boundary is stale: plan.json changed after approval, so the operator must approve the plan again.",
                f.path()
            );
            boundary_finding(f, why)
        })
        .collect()
}

/// A finding about `f` at its first hunk; `None` when it has no hunk.
fn boundary_finding(f: &DiffFile, description: String) -> Option<BoundaryFinding> {
    let hunk = f.hunks.first()?;
    Some(BoundaryFinding {
        path: f.path().to_string(),
        anchor: anchor_record(&hunk.anchor),
        description,
    })
}

fn anchor_record(a: &Anchor) -> AnchorRecord {
    AnchorRecord {
        path: a.path.clone(),
        line: a.line,
        side: match a.side {
            Side::Base => AnchorSide::Base,
            Side::Head => AnchorSide::Head,
        },
        sha: a.sha.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{
        OTHER_TRACK, PLAN, TASK, TRACK, TestRepo, git, plan_json, trailers,
    };
    use crate::services::standards_review::plan_approval::snapshot;
    use crate::services::standards_review::resolver::{pin, resolve};
    use crate::services::standards_review::scope::ReviewLimits;

    fn approval_of(r: &TestRepo) -> PlanApproved {
        let g = Git::open(&r.repo).unwrap();
        let bytes = plan_at(&g, &r.head(), TRACK).unwrap().unwrap();
        snapshot(TRACK, &r.head(), &bytes).unwrap()
    }

    fn proof(r: &TestRepo, approval: Option<&PlanApproved>) -> (ReviewInputs, ScopeProof) {
        let g = Git::open(&r.repo).unwrap();
        let pins = pin(&g, "feat", "main").unwrap();
        let inputs = resolve(&g, &pins, &ReviewLimits::default()).unwrap();
        let proof = prove_scope(&g, TRACK, &inputs, approval).unwrap();
        (inputs, proof)
    }

    fn reasons(p: &ScopeProof) -> Vec<(Reason, Option<String>)> {
        p.blockers
            .iter()
            .map(|b| (b.reason, b.commit.clone()))
            .collect()
    }

    /// A repo whose plan gives `task-1` only `src.rs`, approved as committed.
    fn bounded(name: &str) -> (TestRepo, PlanApproved) {
        let r = TestRepo::new(name);
        r.commit_on_feat(
            PLAN,
            &plan_json(&[(TASK, Some(&["src.rs"])), ("task-2", None)]),
        );
        let a = approval_of(&r);
        (r, a)
    }

    #[test]
    fn trailered_commits_under_an_approved_plan_prove_scope() {
        let r = TestRepo::new("proof-ok");
        r.commit_on_feat("src.rs", "fn main() { run(); }\n");
        let (_, p) = proof(&r, Some(&approval_of(&r)));
        assert_eq!(p, ScopeProof::default());
    }

    #[test]
    fn without_an_approval_scope_is_unproven() {
        let r = TestRepo::new("proof-unapproved");
        let (_, p) = proof(&r, None);
        assert_eq!(reasons(&p), vec![(Reason::PlanNotApproved, None)]);
        assert_eq!(
            serde_json::to_value(&p.blockers[0]).unwrap()["code"],
            "scope-unproven"
        );
    }

    #[test]
    fn ts12_an_untrailered_commit_makes_scope_unproven_naming_it() {
        let r = TestRepo::new("proof-ts12");
        let a = approval_of(&r);
        let bare = r.commit_on_feat_with("other.rs", "x\n", "unrelated work");
        let (_, p) = proof(&r, Some(&a));
        assert_eq!(reasons(&p), vec![(Reason::MissingTrailer, Some(bare))]);
        let half = r.commit_on_feat_with(
            "other.rs",
            "y\n",
            &format!("half\n\nSebenza-Track: {TRACK}"),
        );
        let (_, p) = proof(&r, Some(&a));
        assert!(reasons(&p).contains(&(Reason::MissingTrailer, Some(half))));
    }

    #[test]
    fn ts13_another_tracks_trailer_or_an_unknown_task_makes_scope_unproven() {
        let r = TestRepo::new("proof-ts13");
        let a = approval_of(&r);
        let msg = format!("theirs\n\n{}", trailers(OTHER_TRACK, TASK));
        let foreign = r.commit_on_feat_with("other.rs", "x\n", &msg);
        let (_, p) = proof(&r, Some(&a));
        assert_eq!(reasons(&p), vec![(Reason::ForeignTrailer, Some(foreign))]);

        let r = TestRepo::new("proof-ts13-task");
        let a = approval_of(&r);
        let msg = format!("mine\n\n{}", trailers(TRACK, "task-9"));
        let unknown = r.commit_on_feat_with("other.rs", "x\n", &msg);
        let (_, p) = proof(&r, Some(&a));
        assert_eq!(reasons(&p), vec![(Reason::UnknownTask, Some(unknown))]);
    }

    #[test]
    fn ts14_a_target_merge_is_accepted_and_a_foreign_merge_is_not() {
        let r = TestRepo::new("proof-ts14");
        r.commit_on_main("main.rs", "fn m() {}\n");
        r.merge_main_into_feat();
        let (inputs, p) = proof(&r, Some(&approval_of(&r)));
        assert_eq!(p, ScopeProof::default());
        assert!(
            inputs.files.iter().all(|f| f.path() != "main.rs"),
            "target changes are excluded"
        );

        git(&r.repo, &["branch", "side", "main~1"]);
        git(&r.repo, &["checkout", "-q", "side"]);
        r.commit_on_main("side.rs", "fn s() {}\n");
        git(&r.repo, &["checkout", "-q", "main"]);
        git(
            &r.worktree,
            &["merge", "-q", "--no-ff", "--no-edit", "side"],
        );
        let merge = r.head();
        let (_, p) = proof(&r, Some(&approval_of(&r)));
        assert_eq!(reasons(&p), vec![(Reason::ForeignMerge, Some(merge))]);
    }

    #[test]
    fn ts15_a_missing_base_or_a_head_off_the_branch_is_unproven() {
        let r = TestRepo::new("proof-ts15");
        let g = Git::open(&r.repo).unwrap();
        let good = pin(&g, "feat", "main").unwrap();
        assert_eq!(prove_pins(&g, &good, "feat").unwrap(), vec![]);
        let missing = SnapshotPins {
            base_sha: "1".repeat(40),
            ..good.clone()
        };
        assert_eq!(
            prove_pins(&g, &missing, "feat").unwrap(),
            vec![Blocker::at_commit(Reason::MissingBase, &"1".repeat(40))]
        );
        let head = good.head_sha.clone();
        git(&r.worktree, &["reset", "-q", "--hard", "HEAD~1"]);
        assert_eq!(
            prove_pins(&g, &good, "feat").unwrap(),
            vec![Blocker::at_commit(Reason::HeadOffBranch, &head)]
        );
        assert_eq!(
            prove_pins(&g, &good, "no-such-branch").unwrap(),
            vec![Blocker::at_commit(Reason::HeadOffBranch, &head)]
        );
    }

    #[test]
    fn ts44_a_comment_deleted_outside_the_boundary_is_out_of_scope() {
        let (r, a) = bounded("proof-ts44");
        r.commit_on_main("lib.rs", "// keep me\nfn lib() {}\n");
        r.merge_main_into_feat();
        let msg = format!("tidy\n\n{}", trailers(TRACK, TASK));
        r.commit_on_feat_with("lib.rs", "fn lib() {}\n", &msg);
        r.commit_on_feat("src.rs", "fn main() { go(); }\n");
        let (inputs, p) = proof(&r, Some(&a));
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert_eq!(p.out_of_scope.len(), 1, "{:?}", p.out_of_scope);
        let f = &p.out_of_scope[0];
        assert_eq!(f.path, "lib.rs");
        assert_eq!(
            f.anchor.side,
            AnchorSide::Base,
            "a deletion anchors on the base"
        );
        assert_eq!(f.anchor.sha, inputs.pins.base_sha);
        assert_eq!(f.anchor.line, 1);
        assert!(f.description.contains(TASK));
    }

    #[test]
    fn a_task_without_files_has_an_empty_boundary() {
        let (r, a) = bounded("proof-nofiles");
        let msg = format!("t2\n\n{}", trailers(TRACK, "task-2"));
        r.commit_on_feat_with("src.rs", "fn main() { two(); }\n", &msg);
        let (_, p) = proof(&r, Some(&a));
        assert_eq!(p.out_of_scope.len(), 1);
        assert_eq!(p.out_of_scope[0].path, "src.rs");
        assert!(p.out_of_scope[0].description.contains("task-2"));
    }

    #[test]
    fn ts59_a_plan_edited_after_approval_makes_every_file_out_of_scope() {
        let (r, a) = bounded("proof-ts59");
        r.commit_on_feat("src.rs", "fn main() { go(); }\n");
        let progressed = plan_json(&[(TASK, Some(&["src.rs"])), ("task-2", None)]).replacen(
            "\"backlog\"",
            "\"done\"",
            1,
        );
        r.commit_on_feat(PLAN, &progressed);
        let (_, p) = proof(&r, Some(&a));
        assert!(p.out_of_scope.is_empty(), "progress is not a plan edit");

        r.commit_on_feat(PLAN, &plan_json(&[(TASK, Some(&["**"])), ("task-2", None)]));
        let (_, p) = proof(&r, Some(&a));
        assert_eq!(p.out_of_scope.len(), 1, "{:?}", p.out_of_scope);
        assert_eq!(p.out_of_scope[0].path, "src.rs");
        assert!(p.out_of_scope[0].description.contains("stale"));
        let (_, p) = proof(&r, Some(&approval_of(&r)));
        assert!(
            p.out_of_scope.is_empty(),
            "re-approval clears the stale boundary"
        );
    }

    #[test]
    fn trailer_records_parse_case_insensitively_and_ignore_other_keys() {
        let out = "abc\u{1f}sebenza-track: t1\nSebenza-Task: a\nSebenza-Task: b\nCo-Authored-By: x <y>\n\u{1e}\ndef\u{1f}\u{1e}\n";
        let parsed = parse_trailer_log(out);
        assert_eq!(parsed["abc"].tracks, vec!["t1"]);
        assert_eq!(parsed["abc"].tasks, vec!["a", "b"]);
        assert_eq!(parsed["def"], Trailers::default());
    }

    #[test]
    fn name_status_yields_both_paths_of_a_rename() {
        let out = "M\0a.rs\0R100\0old.rs\0new.rs\0D\0gone.rs\0";
        assert_eq!(
            parse_name_status(out),
            vec!["a.rs", "old.rs", "new.rs", "gone.rs"]
        );
    }
}
