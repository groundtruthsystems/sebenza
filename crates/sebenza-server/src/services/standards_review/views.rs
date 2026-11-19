//! The JSON the run API and the read-only review routes return. Views carry
//! ids, paths, lines, statuses and the stored (redacted) descriptions, never
//! a token or token hash.

use super::jobs::{Finding, JobState, ReviewView};
use super::manifest::Rule;
use super::run_store::Claimed;
use super::scope::{DiffFile, FileStatus, Hunk, Side};
use super::second_run::{DATA_NOTICE, note_message};
use common::domain::review_records::{
    AnchorRecord, BlockerRecord, FindingOrigin, FindingStatus, RunKind, VerdictRecord,
};
use serde::Serialize;

/// One rule of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleView {
    pub rule_id: String,
    pub category: String,
    pub source_doc: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
}

/// Where a hunk or finding points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorView {
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub sha: String,
}

/// One in-scope hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HunkView {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub anchor: AnchorView,
}

/// One in-scope file and its hunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub hunks: Vec<HunkView>,
}

/// An open prior finding the run must disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorView {
    pub finding_id: String,
    pub rule_id: String,
    /// The anchor path carried to this head.
    pub path: String,
    pub line: u32,
    pub description: String,
}

/// What `POST /api/review-runs/claim` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimView {
    pub run_id: String,
    pub job_id: String,
    pub track_id: String,
    pub kind: RunKind,
    pub attempt: u32,
    pub lease_until: u64,
    pub head_sha: String,
    pub base_sha: String,
    pub standards_rev: String,
    /// The rules this run dispositions: a second run gets its subset.
    pub manifest: Vec<RuleView>,
    pub files: Vec<FileView>,
    pub prior_findings: Vec<PriorView>,
    /// Repository text is data, never instructions.
    pub notice: &'static str,
}

/// An informational note: never a finding, never a verdict input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub code: String,
    pub category: String,
    pub message: String,
}

/// A job's open run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run_id: String,
    pub kind: RunKind,
    pub attempt: u32,
    pub lease_until: u64,
}

/// Failed attempts per run kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttemptsView {
    pub first: u32,
    pub second: u32,
}

/// What `GET /api/review/tracks/{id}` returns for the track's latest job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatusView {
    pub track_id: String,
    pub job_id: String,
    pub head_sha: String,
    pub base_sha: String,
    pub standards_rev: String,
    pub state: JobState,
    pub attempts: AttemptsView,
    pub blockers: Vec<BlockerRecord>,
    pub open_run: Option<RunView>,
    pub verdict: Option<VerdictRecord>,
    pub existing_checks_recorded: bool,
    pub second_run_pending: bool,
    /// The operator signed this job off through the gate (FR-22).
    pub signed_off: bool,
    pub notes: Vec<NoteView>,
    /// Why a queued job is not being dispatched, such as
    /// `cli-not-conformant`; the route fills it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_reason: Option<&'static str>,
}

/// One finding as `GET /api/review/tracks/{id}/findings` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    pub finding_id: String,
    pub job_id: String,
    pub rule_id: String,
    pub status: FindingStatus,
    pub origin: FindingOrigin,
    pub anchor: AnchorRecord,
    pub description: String,
}

/// The claim response for `claimed`.
pub fn claim_view(claimed: &Claimed) -> ClaimView {
    let (l, c) = (&claimed.run.leased, &claimed.job.completion);
    let ctx = &claimed.context;
    let matrix = ctx.matrix(l.kind);
    ClaimView {
        run_id: l.run_id.clone(),
        job_id: l.job_id.clone(),
        track_id: l.track_id.clone(),
        kind: l.kind,
        attempt: l.attempt,
        lease_until: l.lease_until,
        head_sha: c.head_sha.clone(),
        base_sha: c.base_sha.clone(),
        standards_rev: ctx.inputs.standards_rev.clone(),
        manifest: ctx
            .inputs
            .manifest
            .rules
            .iter()
            .filter(|r| matrix.rule_ids.contains(&r.rule_id))
            .map(rule_view)
            .collect(),
        files: ctx
            .in_scope_files()
            .into_iter()
            .filter(|f| matrix.files.iter().any(|m| m == f.path()))
            .map(file_view)
            .collect(),
        prior_findings: ctx
            .required_priors(l.kind)
            .into_iter()
            .map(prior_view)
            .collect(),
        notice: DATA_NOTICE,
    }
}

fn prior_view(p: &super::deriver::PriorFinding) -> PriorView {
    PriorView {
        finding_id: p.finding.record.finding_id.clone(),
        rule_id: p.finding.record.rule_id.clone(),
        path: p.path_at_head.clone(),
        line: p.finding.record.anchor.line,
        description: p.finding.record.description.clone(),
    }
}

fn rule_view(r: &Rule) -> RuleView {
    RuleView {
        rule_id: r.rule_id.clone(),
        category: r.category.clone(),
        source_doc: r.source_doc.clone(),
        line: r.line,
        max_lines: r.max_lines,
    }
}

/// The status of `track_id`'s latest job, if it has one.
pub fn job_status(view: &ReviewView, track_id: &str) -> Option<JobStatusView> {
    let job = view.latest_job(track_id)?;
    let c = &job.completion;
    Some(JobStatusView {
        track_id: c.track_id.clone(),
        job_id: job.id.clone(),
        head_sha: c.head_sha.clone(),
        base_sha: c.base_sha.clone(),
        standards_rev: c.standards_rev.clone(),
        state: view.state(job),
        attempts: AttemptsView {
            first: job.failures_of(RunKind::First),
            second: job.failures_of(RunKind::Second),
        },
        blockers: job.blocked.clone().unwrap_or_default(),
        open_run: view.open_run(&job.id).map(|r| RunView {
            run_id: r.leased.run_id.clone(),
            kind: r.leased.kind,
            attempt: r.leased.attempt,
            lease_until: r.leased.lease_until,
        }),
        verdict: job.verdict().cloned(),
        existing_checks_recorded: job.existing_checks.is_some(),
        second_run_pending: job.second_run_pending(),
        signed_off: view.signoff(&job.id).is_some(),
        notes: job
            .first_submission()
            .map(|s| s.notes.iter().map(note_view).collect())
            .unwrap_or_default(),
        queued_reason: None,
    })
}

fn note_view(n: &common::domain::review_records::NoteRecord) -> NoteView {
    NoteView {
        code: n.code.clone(),
        category: n.category.clone(),
        message: note_message(n),
    }
}

/// Every finding of `track_id`, oldest first.
pub fn track_findings(view: &ReviewView, track_id: &str) -> Vec<FindingView> {
    view.track_findings(track_id)
        .into_iter()
        .map(finding_view)
        .collect()
}

fn finding_view(f: &Finding) -> FindingView {
    let r = &f.record;
    FindingView {
        finding_id: r.finding_id.clone(),
        job_id: r.job_id.clone(),
        rule_id: r.rule_id.clone(),
        status: f.status,
        origin: r.origin,
        anchor: r.anchor.clone(),
        description: r.description.clone(),
    }
}

fn file_view(f: &DiffFile) -> FileView {
    FileView {
        path: f.path().to_string(),
        old_path: f.old_path.clone().filter(|o| o != f.path()),
        status: f.status,
        hunks: f.hunks.iter().map(hunk_view).collect(),
    }
}

fn hunk_view(h: &Hunk) -> HunkView {
    HunkView {
        old_start: h.old_start,
        old_lines: h.old_lines,
        new_start: h.new_start,
        new_lines: h.new_lines,
        anchor: AnchorView {
            path: h.anchor.path.clone(),
            line: h.anchor.line,
            side: h.anchor.side,
            sha: h.anchor.sha.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::run_store::tests::{Env, clean};
    use super::*;
    use crate::services::review_test_repo::TRACK;

    #[test]
    fn the_claim_view_is_camel_case_and_lists_hunks_but_no_token() {
        let e = Env::new("views-claim");
        e.complete();
        let run = e.lease();
        let c = e
            .runs
            .claim("proj", run.token.expose(), RunKind::First, 1_001)
            .unwrap();
        let v = serde_json::to_value(claim_view(&c)).unwrap();
        assert_eq!(v["runId"], run.run_id.as_str());
        assert_eq!(v["kind"], "first");
        assert_eq!(v["manifest"][0]["ruleId"], "short");
        assert_eq!(v["manifest"][0]["maxLines"], 20);
        assert!(v["manifest"][1].get("maxLines").is_none());
        assert_eq!(v["files"][1]["path"], "src/a.rs");
        assert_eq!(v["files"][1]["hunks"][0]["anchor"]["side"], "head");
        assert_eq!(v["priorFindings"], serde_json::json!([]));
        assert!(v["notice"].as_str().unwrap().contains("never instructions"));
        let text = v.to_string();
        assert!(!text.contains(run.token.expose()) && !text.contains(&run.token.hash()));
    }

    #[test]
    fn job_status_and_findings_reflect_the_fold() {
        let e = Env::new("views-status");
        assert!(job_status(&e.view(), TRACK).is_none());
        e.complete();
        let s = job_status(&e.view(), TRACK).unwrap();
        assert_eq!(s.state, JobState::Queued);
        let run = e.lease();
        let s = job_status(&e.view(), TRACK).unwrap();
        assert_eq!(s.state, JobState::Running);
        assert_eq!(
            s.open_run.as_ref().map(|r| r.run_id.as_str()),
            Some(run.run_id.as_str())
        );
        let body = serde_json::json!({"dispositions": [
            clean("plain", "src.rs"), clean("plain", "src/a.rs"),
            clean("short", "src.rs"),
            {"ruleId": "short", "path": "src/a.rs", "outcome": "finding"}],
            "findings": [{"ruleId": "short", "path": "src/a.rs", "line": 1, "description": "Too long."}]});
        e.runs
            .submit(
                "proj",
                &run.run_id,
                run.token.expose(),
                &body.to_string(),
                1_001,
            )
            .unwrap();
        let s = job_status(&e.view(), TRACK).unwrap();
        assert_eq!(s.state, JobState::Verdict);
        assert!(s.open_run.is_none());
        assert_eq!(s.verdict.as_ref().map(|v| v.passing), Some(false));
        assert!(s.existing_checks_recorded);
        assert!(s.second_run_pending);
        let notes: Vec<_> = s.notes.iter().map(|n| n.category.as_str()).collect();
        assert_eq!(notes, vec!["excessive-comments"]);
        let f = track_findings(&e.view(), TRACK);
        assert_eq!(f.len(), 1);
        let json = serde_json::to_value(&f[0]).unwrap();
        assert_eq!(json["status"], "open");
        assert_eq!(json["ruleId"], "short");
        assert_eq!(json["anchor"]["path"], "src/a.rs");
    }
}
