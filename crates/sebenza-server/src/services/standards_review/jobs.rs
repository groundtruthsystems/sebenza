//! Review jobs, runs and findings as a fold over the review log.
//!
//! A job is a track completion (keyed by its [`JobKey`]). Runs are leased
//! against a job, one kind at a time; an attempt is a run that ends without
//! an accepted submission. The fold is idempotent: a lease is applied once
//! per (job, kind, attempt), and an attempt failure, a block or a
//! submission only ends the run whose lease id it names, once.
//!
//! [`JobKey`]: common::adapters::review_log::JobKey

use super::findings::{Actor, check_transition};
use common::adapters::review_log::{ReviewEvent, ReviewEventKind, TrackCompleted};
use common::domain::review_records::{
    AttemptCause, AttemptFailed, BlockerRecord, FindingRecord, FindingStatus, JobBlocked, RunKind,
    RunLeased, RunSubmitted, VerdictRecord,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// Failed attempts of one run kind that make a job `review-failed`.
pub const MAX_ATTEMPTS: u32 = 3;

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Queued,
    Running,
    /// A submission was accepted and a verdict derived.
    Verdict,
    /// The inputs are blocked; the verdict is non-passing (AC-08).
    StandardsUnavailable,
    /// Three failed attempts of one run kind.
    ReviewFailed,
}

/// One review job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub completion: TrackCompleted,
    /// Failed attempts per run kind.
    pub failures: BTreeMap<RunKind, u32>,
    pub blocked: Option<Vec<BlockerRecord>>,
    pub submissions: Vec<RunSubmitted>,
    /// Failed existing-check ids once recorded; `None` until then.
    pub existing_checks: Option<Vec<String>>,
}

impl Job {
    /// Failed attempts of `kind`.
    pub fn failures_of(&self, kind: RunKind) -> u32 {
        self.failures.get(&kind).copied().unwrap_or(0)
    }

    /// The latest derived verdict.
    pub fn verdict(&self) -> Option<&VerdictRecord> {
        self.submissions.last().map(|s| &s.verdict)
    }

    /// The manifest rule ids recorded by the job's first accepted submission.
    pub fn manifest_rule_ids(&self) -> Option<&[String]> {
        self.submissions
            .first()
            .map(|s| s.manifest_rule_ids.as_slice())
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    Failed(AttemptCause),
    Blocked,
    Submitted,
}

/// One leased run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub leased: RunLeased,
    pub end: Option<RunEnd>,
}

impl Run {
    /// Not ended yet. An expired lease stays open until it is reaped.
    pub fn is_open(&self) -> bool {
        self.end.is_none()
    }

    /// Open, with its lease still live at `now`.
    pub fn is_live(&self, now: u64) -> bool {
        self.is_open() && now < self.leased.lease_until
    }
}

/// A finding and its current status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub record: FindingRecord,
    pub status: FindingStatus,
}

/// The fold of a project's review log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewView {
    jobs: Vec<Job>,
    runs: Vec<Run>,
    findings: Vec<Finding>,
}

impl ReviewView {
    /// Fold `events` in log order.
    pub fn fold(events: &[ReviewEvent]) -> ReviewView {
        todo!()
    }

    /// Every job, in completion order.
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    /// The job with id `job_id`.
    pub fn job(&self, job_id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == job_id)
    }

    /// The latest job of `track_id`; earlier ones are superseded.
    pub fn latest_job(&self, track_id: &str) -> Option<&Job> {
        self.jobs
            .iter()
            .rev()
            .find(|j| j.completion.track_id == track_id)
    }

    /// Every run, in lease order.
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// The run with id `run_id`.
    pub fn run(&self, run_id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.leased.run_id == run_id)
    }

    /// The run whose token hashes to `token_hash`.
    pub fn run_by_token_hash(&self, token_hash: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.leased.token_hash == token_hash)
    }

    /// The open run of `job_id`, if any.
    pub fn open_run(&self, job_id: &str) -> Option<&Run> {
        self.runs
            .iter()
            .find(|r| r.leased.job_id == job_id && r.is_open())
    }

    /// Open runs whose lease ended at or before `now`.
    pub fn expired_runs(&self, now: u64) -> Vec<&Run> {
        self.runs
            .iter()
            .filter(|r| r.is_open() && !r.is_live(now))
            .collect()
    }

    /// The state of `job`.
    pub fn state(&self, job: &Job) -> JobState {
        todo!()
    }

    /// Every finding of `track_id`, in the order they were raised.
    pub fn track_findings(&self, track_id: &str) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.record.track_id == track_id)
            .collect()
    }

    /// The finding with id `finding_id`.
    pub fn finding(&self, finding_id: &str) -> Option<&Finding> {
        self.findings
            .iter()
            .find(|f| f.record.finding_id == finding_id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use common::adapters::review_log::CompletionSource;
    use common::domain::review_records::{
        AnchorRecord, AnchorSide, ExistingChecksRecorded, FindingOrigin, TransitionRecord,
    };

    pub(crate) fn completion(track: &str, head: &str) -> TrackCompleted {
        TrackCompleted {
            track_id: track.into(),
            head_sha: head.into(),
            base_sha: "b".repeat(40),
            standards_rev: "r".repeat(64),
            source: CompletionSource::Daemon,
            credential: Some("control".into()),
            worktree_path: "/wt".into(),
            branch: "feat".into(),
        }
    }

    fn ev(kind: ReviewEventKind) -> ReviewEvent {
        ReviewEvent::new(kind)
    }

    fn job_id(c: &TrackCompleted) -> String {
        c.job_key().job_id()
    }

    fn lease(job: &str, kind: RunKind, attempt: u32) -> RunLeased {
        RunLeased {
            job_id: job.into(),
            track_id: "t1".into(),
            run_id: format!("run-{}-{attempt}", kind.as_str()),
            kind,
            attempt,
            lease_id: format!("lease-{}-{attempt}", kind.as_str()),
            token_hash: format!("hash-{}-{attempt}", kind.as_str()),
            lease_until: 100,
        }
    }

    fn failed(l: &RunLeased, cause: AttemptCause) -> ReviewEventKind {
        ReviewEventKind::AttemptFailed(AttemptFailed {
            job_id: l.job_id.clone(),
            run_id: l.run_id.clone(),
            kind: l.kind,
            attempt: l.attempt,
            lease_id: l.lease_id.clone(),
            cause,
            reason: None,
        })
    }

    pub(crate) fn finding(id: &str, job: &str, rule: &str, path: &str) -> FindingRecord {
        FindingRecord {
            finding_id: id.into(),
            track_id: "t1".into(),
            job_id: job.into(),
            rule_id: rule.into(),
            anchor: AnchorRecord {
                path: path.into(),
                line: 3,
                side: AnchorSide::Head,
                sha: "h1".into(),
            },
            text_hash: "x".repeat(64),
            description: "Too long.".into(),
            origin: FindingOrigin::Agent,
        }
    }

    fn submitted(
        l: &RunLeased,
        opened: Vec<FindingRecord>,
        t: Vec<TransitionRecord>,
    ) -> RunSubmitted {
        RunSubmitted {
            job_id: l.job_id.clone(),
            run_id: l.run_id.clone(),
            kind: l.kind,
            lease_id: l.lease_id.clone(),
            manifest_rule_ids: vec!["r1".into()],
            pairs: vec![],
            priors: vec![],
            opened,
            transitions: t,
            verdict: VerdictRecord {
                passing: false,
                reasons: vec!["open-findings".into()],
            },
        }
    }

    #[test]
    fn a_completion_is_a_queued_job_and_a_lease_makes_it_running() {
        let c = completion("t1", "h1");
        let id = job_id(&c);
        let l = lease(&id, RunKind::First, 1);
        let mut events = vec![ev(ReviewEventKind::TrackCompleted(c.clone()))];
        let view = ReviewView::fold(&events);
        assert_eq!(view.state(view.job(&id).unwrap()), JobState::Queued);
        events.push(ev(ReviewEventKind::RunLeased(l.clone())));
        let view = ReviewView::fold(&events);
        assert_eq!(view.state(view.job(&id).unwrap()), JobState::Running);
        assert_eq!(view.run_by_token_hash(&l.token_hash).unwrap().leased, l);
        assert!(view.run(&l.run_id).unwrap().is_live(99));
        assert!(!view.run(&l.run_id).unwrap().is_live(100));
        assert_eq!(view.expired_runs(100).len(), 1);
    }

    #[test]
    fn ts37_three_failed_attempts_of_one_kind_fail_the_job() {
        let c = completion("t1", "h1");
        let id = job_id(&c);
        let mut events = vec![ev(ReviewEventKind::TrackCompleted(c))];
        for attempt in 1..=3 {
            let l = lease(&id, RunKind::First, attempt);
            events.push(ev(ReviewEventKind::RunLeased(l.clone())));
            events.push(ev(failed(&l, AttemptCause::LeaseExpired)));
            let view = ReviewView::fold(&events);
            let job = view.job(&id).unwrap();
            assert_eq!(job.failures_of(RunKind::First), attempt);
            let want = if attempt < 3 {
                JobState::Queued
            } else {
                JobState::ReviewFailed
            };
            assert_eq!(view.state(job), want);
        }
    }

    #[test]
    fn ts60_attempts_count_per_run_kind() {
        let c = completion("t1", "h1");
        let id = job_id(&c);
        let mut events = vec![ev(ReviewEventKind::TrackCompleted(c))];
        for (kind, attempt) in [
            (RunKind::First, 1),
            (RunKind::Second, 1),
            (RunKind::First, 2),
            (RunKind::Second, 2),
        ] {
            let l = lease(&id, kind, attempt);
            events.push(ev(ReviewEventKind::RunLeased(l.clone())));
            events.push(ev(failed(&l, AttemptCause::Rejected)));
        }
        let view = ReviewView::fold(&events);
        let job = view.job(&id).unwrap();
        assert_eq!(job.failures_of(RunKind::First), 2);
        assert_eq!(job.failures_of(RunKind::Second), 2);
        assert_ne!(
            view.state(job),
            JobState::ReviewFailed,
            "two of each kind is not three of one"
        );
        let l = lease(&id, RunKind::Second, 3);
        events.push(ev(ReviewEventKind::RunLeased(l.clone())));
        events.push(ev(failed(&l, AttemptCause::Rejected)));
        let view = ReviewView::fold(&events);
        assert_eq!(view.state(view.job(&id).unwrap()), JobState::ReviewFailed);
    }

    #[test]
    fn lease_and_attempt_events_are_idempotent() {
        let c = completion("t1", "h1");
        let id = job_id(&c);
        let l = lease(&id, RunKind::First, 1);
        let mut twin = l.clone();
        twin.run_id = "run-twin".into();
        twin.lease_id = "lease-twin".into();
        let events = vec![
            ev(ReviewEventKind::TrackCompleted(c)),
            ev(ReviewEventKind::RunLeased(l.clone())),
            ev(ReviewEventKind::RunLeased(twin.clone())),
            ev(failed(&l, AttemptCause::LeaseExpired)),
            ev(failed(&l, AttemptCause::LeaseExpired)),
            ev(failed(&twin, AttemptCause::LeaseExpired)),
        ];
        let view = ReviewView::fold(&events);
        assert_eq!(view.runs().len(), 1, "one lease per (job, kind, attempt)");
        assert_eq!(view.job(&id).unwrap().failures_of(RunKind::First), 1);
    }

    #[test]
    fn a_lease_for_an_unknown_job_is_ignored() {
        let events = vec![ev(ReviewEventKind::RunLeased(lease(
            "job_x",
            RunKind::First,
            1,
        )))];
        assert!(ReviewView::fold(&events).runs().is_empty());
    }

    #[test]
    fn a_block_ends_the_run_without_an_attempt() {
        let c = completion("t1", "h1");
        let id = job_id(&c);
        let l = lease(&id, RunKind::First, 1);
        let blocker = BlockerRecord {
            code: "standards-unavailable".into(),
            reason: "symlink".into(),
            path: Some(".ai/sebenza/x.md".into()),
            line: None,
        };
        let events = vec![
            ev(ReviewEventKind::TrackCompleted(c)),
            ev(ReviewEventKind::RunLeased(l.clone())),
            ev(ReviewEventKind::JobBlocked(JobBlocked {
                job_id: id.clone(),
                run_id: l.run_id.clone(),
                lease_id: l.lease_id.clone(),
                blockers: vec![blocker.clone()],
            })),
        ];
        let view = ReviewView::fold(&events);
        let job = view.job(&id).unwrap();
        assert_eq!(view.state(job), JobState::StandardsUnavailable);
        assert_eq!(job.blocked, Some(vec![blocker]));
        assert_eq!(job.failures_of(RunKind::First), 0);
        assert_eq!(view.run(&l.run_id).unwrap().end, Some(RunEnd::Blocked));
    }

    #[test]
    fn a_submission_opens_findings_applies_transitions_and_records_the_verdict() {
        let c1 = completion("t1", "h1");
        let c2 = completion("t1", "h2");
        let (j1, j2) = (job_id(&c1), job_id(&c2));
        let l1 = lease(&j1, RunKind::First, 1);
        let mut l2 = lease(&j2, RunKind::First, 1);
        l2.run_id = "run-2".into();
        l2.lease_id = "lease-2".into();
        l2.token_hash = "hash-2".into();
        let resolve = TransitionRecord {
            finding_id: "f1".into(),
            from: FindingStatus::Open,
            to: FindingStatus::Resolved,
            cause: "verified-clean".into(),
        };
        let reopen = TransitionRecord {
            finding_id: "f1".into(),
            from: FindingStatus::Resolved,
            to: FindingStatus::Open,
            cause: "forged".into(),
        };
        let events = vec![
            ev(ReviewEventKind::TrackCompleted(c1)),
            ev(ReviewEventKind::RunLeased(l1.clone())),
            ev(ReviewEventKind::RunSubmitted(submitted(
                &l1,
                vec![finding("f1", &j1, "r1", "a.rs")],
                vec![],
            ))),
            ev(ReviewEventKind::TrackCompleted(c2)),
            ev(ReviewEventKind::RunLeased(l2.clone())),
            ev(ReviewEventKind::RunSubmitted(submitted(
                &l2,
                vec![],
                vec![resolve, reopen],
            ))),
            ev(ReviewEventKind::ExistingChecksRecorded(
                ExistingChecksRecorded {
                    job_id: j2.clone(),
                    failed: vec![],
                },
            )),
        ];
        let view = ReviewView::fold(&events);
        assert_eq!(
            view.finding("f1").unwrap().status,
            FindingStatus::Resolved,
            "a refused transition is skipped"
        );
        let job = view.job(&j2).unwrap();
        assert_eq!(view.state(job), JobState::Verdict);
        assert_eq!(job.verdict().unwrap().reasons, vec!["open-findings"]);
        assert_eq!(job.existing_checks, Some(vec![]));
        assert_eq!(view.latest_job("t1").unwrap().id, j2);
        assert_eq!(view.run("run-2").unwrap().end, Some(RunEnd::Submitted));
        assert_eq!(
            view.job(&j1).unwrap().manifest_rule_ids(),
            Some(&["r1".to_string()][..])
        );
    }
}
