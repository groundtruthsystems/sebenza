//! Plan approval (FR-20): the operator approves a track's plan.json as
//! committed at the branch head, and the daemon snapshots each task's edit
//! boundary from it into the review log. The implementing agent cannot amend
//! the snapshot; a changed boundary needs a new approval.
//!
//! The snapshot carries two hashes. `plan_sha256` is the hash of the plan
//! blob itself. `plan_content_sha256` hashes the plan with its progress
//! fields removed, because sebenza-implement commits task and phase status
//! after every task; any other plan edit after approval changes it and makes
//! the boundary stale.

use super::boundary::valid_glob;
use super::git_pinned::{Git, GitError, branch_ref};
use super::objects::{blob_sizes, read_blobs};
use super::standards_docs::WORKSPACE;
use common::adapters::review_log::{ReviewEventKind, ReviewLog};
use common::domain::review_records::{PlanApproved, TaskBoundary};
use common::domain::solution::sha256_hex;
use serde_json::Value;

/// The largest plan.json read.
pub const MAX_PLAN_BYTES: u64 = 1024 * 1024;

/// The plan keys sebenza-implement and the inbox write as work progresses.
pub const PROGRESS_FIELDS: [&str; 5] = [
    "status",
    "blocked_reason",
    "commit_sha",
    "checkpoint_sha",
    "request",
];

/// Why an approval was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("git: {0}")]
    Git(#[from] GitError),
    #[error("plan.json is not committed at {0}")]
    NoPlan(String),
    #[error("plan.json is invalid: {0}")]
    InvalidPlan(String),
    #[error("review log: {0}")]
    Log(#[from] std::io::Error),
}

/// `.ai/sebenza/tracks/<track>/plan.json`.
pub fn plan_path(track_id: &str) -> String {
    format!("{WORKSPACE}/tracks/{track_id}/plan.json")
}

/// Approve `track_id`'s plan as committed at the head of `branch`, and
/// append the boundary snapshot to `log`.
pub fn approve_plan(
    log: &ReviewLog,
    git: &Git,
    track_id: &str,
    branch: &str,
    credential: &str,
) -> Result<PlanApproved, ApprovalError> {
    let head = git.commit_sha(&branch_ref(branch))?;
    let bytes =
        plan_at(git, &head, track_id)?.ok_or_else(|| ApprovalError::NoPlan(head.clone()))?;
    let mut approved = snapshot(track_id, &head, &bytes)?;
    approved.credential = credential.to_string();
    let event = ReviewEventKind::PlanApproved(approved.clone());
    log.transact(|_| Ok::<_, std::io::Error>((Some(event), ())))?;
    Ok(approved)
}

/// The plan.json blob of `track_id` at commit `sha`, when it is a regular
/// file there within [`MAX_PLAN_BYTES`].
pub fn plan_at(git: &Git, sha: &str, track_id: &str) -> Result<Option<Vec<u8>>, GitError> {
    let entry = git.text(&["ls-tree", sha, "--", &plan_path(track_id)])?;
    let fields: Vec<&str> = entry.split_whitespace().collect();
    let [mode, "blob", oid, ..] = fields.as_slice() else {
        return Ok(None);
    };
    let oid = oid.to_string();
    let small = blob_sizes(git, std::slice::from_ref(&oid))?
        .get(&oid)
        .is_some_and(|size| *size <= MAX_PLAN_BYTES);
    if !matches!(*mode, "100644" | "100755") || !small {
        return Ok(None);
    }
    Ok(read_blobs(git, std::slice::from_ref(&oid), MAX_PLAN_BYTES as usize)?.remove(&oid))
}

/// The approval record of `bytes`, the plan of `track_id` at `head_sha`.
pub fn snapshot(
    track_id: &str,
    head_sha: &str,
    bytes: &[u8],
) -> Result<PlanApproved, ApprovalError> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8"))?;
    let plan: Value = serde_json::from_str(text).map_err(|e| invalid(&e.to_string()))?;
    if plan["track_id"] != track_id {
        return Err(invalid("its track_id names another track"));
    }
    Ok(PlanApproved {
        track_id: track_id.to_string(),
        head_sha: head_sha.to_string(),
        plan_sha256: sha256_hex(text),
        plan_content_sha256: content_sha256(&plan),
        boundaries: boundaries(&plan)?,
        credential: String::new(),
    })
}

/// The hash of `plan` without its [`PROGRESS_FIELDS`].
pub fn content_sha256(plan: &Value) -> String {
    let mut content = plan.clone();
    strip_progress(&mut content);
    sha256_hex(&content.to_string())
}

/// The content hash of a plan.json blob, `None` when it is not JSON.
pub fn content_sha256_of(bytes: &[u8]) -> Option<String> {
    let plan: Value = serde_json::from_slice(bytes).ok()?;
    Some(content_sha256(&plan))
}

fn strip_progress(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for key in PROGRESS_FIELDS {
                map.remove(key);
            }
            map.values_mut().for_each(strip_progress);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_progress),
        _ => {}
    }
}

/// Every task's boundary, in plan order.
fn boundaries(plan: &Value) -> Result<Vec<TaskBoundary>, ApprovalError> {
    let tasks = plan["phases"]
        .as_array()
        .ok_or_else(|| invalid("no phases"))?
        .iter()
        .flat_map(|phase| phase["tasks"].as_array().cloned().unwrap_or_default());
    tasks.map(|task| task_boundary(&task)).collect()
}

fn task_boundary(task: &Value) -> Result<TaskBoundary, ApprovalError> {
    let task_id = task["id"]
        .as_str()
        .ok_or_else(|| invalid("a task has no id"))?;
    let files = match &task["files"] {
        Value::Null => Vec::new(),
        Value::Array(globs) => globs
            .iter()
            .map(|g| g.as_str().filter(|g| valid_glob(g)).map(String::from))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| invalid(&format!("task {task_id} has an invalid files glob")))?,
        _ => return Err(invalid(&format!("task {task_id} files is not an array"))),
    };
    Ok(TaskBoundary {
        task_id: task_id.to_string(),
        files,
    })
}

fn invalid(why: &str) -> ApprovalError {
    ApprovalError::InvalidPlan(why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{PLAN, TASK, TRACK, TestRepo, plan_json, write};

    fn boundary(id: &str, files: &[&str]) -> TaskBoundary {
        TaskBoundary {
            task_id: id.into(),
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn a_snapshot_records_each_task_boundary_and_the_blob_hash() {
        let text = plan_json(&[("a", Some(&["src/"])), ("b", None)]);
        let s = snapshot(TRACK, "h", text.as_bytes()).unwrap();
        assert_eq!(s.plan_sha256, sha256_hex(&text));
        assert_eq!(
            s.boundaries,
            vec![boundary("a", &["src/"]), boundary("b", &[])]
        );
        assert_eq!(s.head_sha, "h");
    }

    #[test]
    fn progress_edits_keep_the_content_hash_and_boundary_edits_change_it() {
        let text = plan_json(&[("a", Some(&["src/"]))]);
        let base = content_sha256_of(text.as_bytes()).unwrap();
        let progressed = text
            .replace("\"backlog\"", "\"done\", \"commit_sha\": \"abc1234\"")
            .replace("\"doing\"", "\"done\", \"checkpoint_sha\": \"abc1234\"");
        assert_eq!(content_sha256_of(progressed.as_bytes()).unwrap(), base);
        let widened = plan_json(&[("a", Some(&["src/", "docs/"]))]);
        assert_ne!(content_sha256_of(widened.as_bytes()).unwrap(), base);
        let added = plan_json(&[("a", Some(&["src/"])), ("b", Some(&["**"]))]);
        assert_ne!(content_sha256_of(added.as_bytes()).unwrap(), base);
    }

    #[test]
    fn invalid_plans_are_refused() {
        let escaping = plan_json(&[("a", Some(&["../x"]))]);
        assert!(matches!(
            snapshot(TRACK, "h", escaping.as_bytes()),
            Err(ApprovalError::InvalidPlan(_))
        ));
        let other = plan_json(&[("a", None)]).replace(TRACK, "o_20261101");
        assert!(matches!(
            snapshot(TRACK, "h", other.as_bytes()),
            Err(ApprovalError::InvalidPlan(_))
        ));
        assert!(snapshot(TRACK, "h", b"{").is_err());
    }

    #[test]
    fn ts45_approval_snapshots_the_committed_plan_into_the_log() {
        let r = TestRepo::new("approve-ts45");
        let committed = plan_json(&[(TASK, Some(&["src.rs"]))]);
        r.commit_on_feat(PLAN, &committed);
        write(&r.worktree, PLAN, &plan_json(&[(TASK, Some(&["**"]))]));
        let log = ReviewLog::for_project(&r.base.join("reviews"), "proj");
        let git = Git::open(&r.worktree).unwrap();
        let a = approve_plan(&log, &git, TRACK, "feat", "operator").unwrap();
        assert_eq!(a.head_sha, r.head());
        assert_eq!(
            a.plan_sha256,
            sha256_hex(&committed),
            "the committed blob, not the working tree"
        );
        assert_eq!(a.boundaries, vec![boundary(TASK, &["src.rs"])]);
        assert_eq!(a.credential, "operator");
        let events = log.events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, ReviewEventKind::PlanApproved(a));
    }

    #[test]
    fn a_track_without_a_committed_plan_cannot_be_approved() {
        let r = TestRepo::new("approve-noplan");
        let log = ReviewLog::for_project(&r.base.join("reviews"), "proj");
        let git = Git::open(&r.worktree).unwrap();
        let err = approve_plan(&log, &git, "z_20261118", "feat", "operator").unwrap_err();
        assert!(matches!(err, ApprovalError::NoPlan(_)), "{err}");
        assert!(log.events().unwrap().is_empty());
    }
}
