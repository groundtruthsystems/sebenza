//! The run store (FR-11, FR-12, FR-14): leases, run tokens, claims,
//! submissions and expiry, all as review-log events.
//!
//! Every state change is one [`ReviewLog::transact`] whose decision re-folds
//! the log and re-checks the run under the log's lock, so two workers, a
//! replayed token or a racing reaper cannot both win. Git is read before
//! that lock, from the job's pinned SHAs only. A presented token is checked
//! by hash: it must name an open run with a live lease, of the claimed kind
//! or the submitted run id. A token refusal changes nothing; a rejected
//! submission ends the run as a failed attempt.

use super::deriver::{Derivation, DeriveInputs, MeasuredFinding, derive};
use super::free_text::TextScanner;
use super::jobs::{Job, JobState, ReviewView, Run};
use super::reasons::Blocker;
use super::resolver::ResolveError;
use super::run_context::{RunContext, resolve_context};
use super::run_token::{RunToken, hash_token};
use super::scope::ReviewLimits;
use super::submission::{Rejection, ValidSubmission, validate};
use common::adapters::review_log::{ReviewEventKind, ReviewLog};
use common::domain::review_records::{
    AttemptCause, AttemptFailed, BlockerRecord, ExistingChecksRecorded, JobBlocked, RunKind,
    RunLeased, RunSubmitted, VerdictRecord,
};
use std::path::PathBuf;
use std::sync::Arc;

/// How long a lease, and its token, lives.
pub const LEASE_SECS: u64 = 30 * 60;

/// Tunables of the run store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSettings {
    pub lease_secs: u64,
    pub limits: ReviewLimits,
}

impl Default for RunSettings {
    fn default() -> Self {
        RunSettings {
            lease_secs: LEASE_SECS,
            limits: ReviewLimits::default(),
        }
    }
}

/// A run just leased, with the only copy of its token.
#[derive(Debug, Clone)]
pub struct LeasedRun {
    pub project_key: String,
    pub run_id: String,
    pub job_id: String,
    pub track_id: String,
    pub kind: RunKind,
    pub attempt: u32,
    pub lease_until: u64,
    pub token: RunToken,
    /// The worktree whose head the run reviews.
    pub worktree_path: String,
}

/// Why a claim or submit was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("no run holds this token")]
    UnknownToken,
    #[error("the token belongs to another run")]
    WrongRun,
    #[error("the token belongs to another run kind")]
    WrongKind,
    #[error("the run's lease has expired")]
    Expired,
    #[error("the token was already used or its run has ended")]
    Used,
    #[error("the job's inputs are unavailable")]
    Blocked(Vec<BlockerRecord>),
    #[error("submission rejected ({}): {}", .0.code, .0.problems.join("; "))]
    Rejected(Rejection),
    #[error("could not resolve the job's inputs: {0}")]
    Inputs(#[from] ResolveError),
    #[error("review log: {0}")]
    Log(#[from] std::io::Error),
}

/// What an accepted submission produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub run_id: String,
    pub job_id: String,
    pub findings_opened: usize,
    pub verdict: VerdictRecord,
}

/// What a claim hands the run: its job's resolved context.
#[derive(Debug, Clone)]
pub struct Claimed {
    pub run: Run,
    pub job: Job,
    pub context: RunContext,
}

/// Leases, claims and submissions over each project's review log.
#[derive(Clone)]
pub struct ReviewRuns {
    root: PathBuf,
    settings: RunSettings,
    scanner: Arc<dyn TextScanner>,
}

impl ReviewRuns {
    /// A store over the review logs under `root`.
    pub fn new(root: PathBuf, settings: RunSettings, scanner: Arc<dyn TextScanner>) -> Self {
        ReviewRuns {
            root,
            settings,
            scanner,
        }
    }

    /// The review log of `project_key`.
    pub fn log(&self, project_key: &str) -> ReviewLog {
        ReviewLog::for_project(&self.root, project_key)
    }

    /// The current fold of `project_key`'s log.
    pub fn view(&self, project_key: &str) -> std::io::Result<ReviewView> {
        Ok(ReviewView::fold(&self.log(project_key).events()?))
    }

    /// Whether `project_key`'s log holds a run with this token.
    pub fn holds_token(&self, project_key: &str, token: &str) -> bool {
        self.view(project_key)
            .is_ok_and(|v| v.run_by_token_hash(&hash_token(token)).is_some())
    }

    /// Lease a first run of the next queued job, if any: the latest job of a
    /// track, with no open run and no outcome yet.
    pub fn lease_next(&self, project_key: &str, now: u64) -> std::io::Result<Option<LeasedRun>> {
        todo!()
    }

    /// Lease a run of `kind` for `job_id` when the job has no open run and
    /// is neither failed nor blocked. Phase 5 schedules second runs here.
    pub fn lease_run(
        &self,
        project_key: &str,
        job_id: &str,
        kind: RunKind,
        now: u64,
    ) -> std::io::Result<Option<LeasedRun>> {
        todo!()
    }

    /// Check `token` for a claim of `kind` and resolve the run's inputs. A
    /// blocked input ends the run without an attempt and makes the job
    /// `standards-unavailable`.
    pub fn claim(
        &self,
        project_key: &str,
        token: &str,
        kind: RunKind,
        now: u64,
    ) -> Result<Claimed, RunError> {
        todo!()
    }

    /// Check `token` for `run_id`, validate `body` and derive. An accepted
    /// submission consumes the token; a rejected one fails the attempt.
    pub fn submit(
        &self,
        project_key: &str,
        run_id: &str,
        token: &str,
        body: &str,
        now: u64,
    ) -> Result<Accepted, RunError> {
        todo!()
    }

    /// Record the existing sebenza-review check results of `job_id`.
    /// Returns false when the job is unknown.
    pub fn record_existing_checks(
        &self,
        project_key: &str,
        job_id: &str,
        failed: Vec<String>,
    ) -> std::io::Result<bool> {
        todo!()
    }
}

/// Fail every open run of `log` whose lease ended by `now`. Returns how many
/// attempts were recorded; a run already failed is never failed twice.
pub fn reap_expired(log: &ReviewLog, now: u64) -> std::io::Result<usize> {
    todo!()
}

/// Seconds since the epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::services::review_test_repo::{TRACK, TestRepo, git};
    use crate::services::standards_review::free_text::ContentScanner;
    use crate::services::track_completion::{CompletionTarget, TrackCompletionService};
    use common::domain::review_records::FindingStatus;
    use common::domain::solution::CredentialClass;
    use serde_json::{Value, json};

    pub(crate) const STYLE: &str = ".ai/sebenza/code_styleguides/general.md";
    pub(crate) const RULES: &str = "# General\n- [rule:short category=long-methods max_lines=20] Short.\n- [rule:plain category=restating-comments] Plain.\n";
    const NOW: u64 = 1_000;

    pub(crate) struct Env {
        pub r: TestRepo,
        pub runs: ReviewRuns,
        pub completion: TrackCompletionService,
    }

    pub(crate) fn target(r: &TestRepo) -> CompletionTarget {
        CompletionTarget {
            project_key: "proj".into(),
            worktree_path: r.worktree.clone(),
            branch: "feat".into(),
            target_branch: "main".into(),
        }
    }

    impl Env {
        /// A track with the two rules at its base and `src/a.rs` added.
        pub(crate) fn new(name: &str) -> Env {
            let r = TestRepo::new(name);
            r.commit_on_main(STYLE, RULES);
            r.merge_main_into_feat();
            r.commit_on_feat("src/a.rs", "fn a() {}\n");
            Env::over(r)
        }

        pub(crate) fn over(r: TestRepo) -> Env {
            let root = r.base.join("reviews");
            Env {
                runs: ReviewRuns::new(
                    root.clone(),
                    RunSettings::default(),
                    Arc::new(ContentScanner::default()),
                ),
                completion: TrackCompletionService::new(root),
                r,
            }
        }

        pub(crate) fn complete(&self) {
            self.completion
                .complete(&target(&self.r), TRACK, CredentialClass::Control)
                .unwrap();
        }

        pub(crate) fn lease(&self) -> LeasedRun {
            self.runs
                .lease_next("proj", NOW)
                .unwrap()
                .expect("a queued job")
        }

        pub(crate) fn view(&self) -> ReviewView {
            self.runs.view("proj").unwrap()
        }

        fn event_count(&self) -> usize {
            self.runs.log("proj").events().unwrap().len()
        }

        fn submit(&self, run: &LeasedRun, body: &Value) -> Result<Accepted, RunError> {
            self.runs.submit(
                "proj",
                &run.run_id,
                run.token.expose(),
                &body.to_string(),
                NOW + 1,
            )
        }

        fn job_state(&self) -> JobState {
            let v = self.view();
            v.state(v.latest_job(TRACK).unwrap())
        }
    }

    pub(crate) fn clean(rule: &str, path: &str) -> Value {
        json!({"ruleId": rule, "path": path, "outcome": "clean", "span": {"startLine": 1, "endLine": 1}})
    }

    fn all_clean(paths: &[&str]) -> Value {
        let pairs: Vec<Value> = ["plain", "short"]
            .iter()
            .flat_map(|rule| paths.iter().map(move |p| clean(rule, p)))
            .collect();
        json!({ "dispositions": pairs })
    }

    fn with_finding(paths: &[&str], path: &str, text: &str) -> Value {
        let mut v = all_clean(paths);
        let pairs = v["dispositions"].as_array_mut().unwrap();
        let at = pairs
            .iter()
            .position(|p| p["ruleId"] == "short" && p["path"] == path)
            .unwrap();
        pairs[at] = json!({"ruleId": "short", "path": path, "outcome": "finding"});
        v["findings"] = json!([{"ruleId": "short", "path": path, "line": 1, "description": text}]);
        v
    }

    #[test]
    fn a_completed_job_is_leased_once_with_only_its_token_hash_logged() {
        let e = Env::new("store-lease");
        e.complete();
        let run = e.lease();
        assert_eq!(
            (run.kind, run.attempt, run.track_id.as_str()),
            (RunKind::First, 1, TRACK)
        );
        assert_eq!(run.lease_until, NOW + LEASE_SECS);
        assert!(
            e.runs.lease_next("proj", NOW).unwrap().is_none(),
            "one open run per job"
        );
        let text = std::fs::read_to_string(e.runs.log("proj").path()).unwrap();
        assert!(!text.contains(run.token.expose()));
        assert!(text.contains(&run.token.hash()));
        assert!(e.runs.holds_token("proj", run.token.expose()));
        assert_eq!(e.job_state(), JobState::Running);
    }

    #[test]
    fn a_claim_returns_the_manifest_in_scope_files_and_no_priors() {
        let e = Env::new("store-claim");
        e.complete();
        let run = e.lease();
        let c = e
            .runs
            .claim("proj", run.token.expose(), RunKind::First, NOW + 1)
            .unwrap();
        assert_eq!(c.run.leased.run_id, run.run_id);
        let rules: Vec<_> = c.context.rule_ids().into_iter().collect();
        assert_eq!(rules, vec!["plain", "short"]);
        let files: Vec<_> = c
            .context
            .in_scope_files()
            .iter()
            .map(|f| f.path().to_string())
            .collect();
        assert_eq!(files, vec!["src.rs", "src/a.rs"]);
        assert!(c.context.required_priors(RunKind::First).is_empty());
    }

    #[test]
    fn ts74_claims_with_a_wrong_kind_unknown_or_expired_token_are_refused() {
        let e = Env::new("store-claim-refused");
        e.complete();
        let run = e.lease();
        let before = e.event_count();
        let t = run.token.expose();
        assert!(matches!(
            e.runs.claim("proj", t, RunKind::Second, NOW),
            Err(RunError::WrongKind)
        ));
        assert!(matches!(
            e.runs.claim(
                "proj",
                &RunToken::mint().expose().to_string(),
                RunKind::First,
                NOW
            ),
            Err(RunError::UnknownToken)
        ));
        assert!(matches!(
            e.runs.claim("proj", t, RunKind::First, run.lease_until),
            Err(RunError::Expired)
        ));
        assert_eq!(e.event_count(), before, "a refusal changes nothing");
    }

    #[test]
    fn a_blocked_input_makes_the_job_standards_unavailable_without_an_attempt() {
        let r = TestRepo::new("store-blocked");
        r.commit_on_main(".ai/sebenza/index.md", "- [E](../../src.rs)\n");
        r.merge_main_into_feat();
        let e = Env::over(r);
        e.complete();
        let run = e.lease();
        match e
            .runs
            .claim("proj", run.token.expose(), RunKind::First, NOW + 1)
        {
            Err(RunError::Blocked(b)) => assert_eq!(b[0].code, "standards-unavailable"),
            other => panic!("{other:?}"),
        }
        assert_eq!(e.job_state(), JobState::StandardsUnavailable);
        let v = e.view();
        assert_eq!(v.latest_job(TRACK).unwrap().failures_of(RunKind::First), 0);
        assert!(
            e.runs.lease_next("proj", NOW + 2).unwrap().is_none(),
            "no run for a blocked job"
        );
        assert!(matches!(
            e.runs
                .claim("proj", run.token.expose(), RunKind::First, NOW + 3),
            Err(RunError::Used)
        ));
    }

    #[test]
    fn ts18_a_complete_submission_is_accepted_and_consumes_the_token() {
        let e = Env::new("store-ts18");
        e.complete();
        let run = e.lease();
        let ok = e.submit(&run, &all_clean(&["src.rs", "src/a.rs"])).unwrap();
        assert_eq!(ok.run_id, run.run_id);
        assert_eq!(
            ok.verdict.reasons,
            vec!["existing-checks-missing"],
            "fails closed without checks"
        );
        assert_eq!(e.job_state(), JobState::Verdict);
        let before = e.event_count();
        let again = e.submit(&run, &all_clean(&["src.rs", "src/a.rs"]));
        assert!(
            matches!(again, Err(RunError::Used)),
            "TS-74: a used token is refused"
        );
        assert_eq!(e.event_count(), before);
    }

    #[test]
    fn recorded_green_checks_let_a_clean_job_pass() {
        let e = Env::new("store-pass");
        e.complete();
        let run = e.lease();
        assert!(
            e.runs
                .record_existing_checks("proj", &run.job_id, vec![])
                .unwrap()
        );
        assert!(
            !e.runs
                .record_existing_checks("proj", "job_unknown", vec![])
                .unwrap()
        );
        let ok = e.submit(&run, &all_clean(&["src.rs", "src/a.rs"])).unwrap();
        assert_eq!(
            ok.verdict,
            VerdictRecord {
                passing: true,
                reasons: vec![]
            }
        );
    }

    #[test]
    fn ts19_ts21_a_rejected_submission_fails_the_attempt_and_frees_the_job() {
        let e = Env::new("store-ts19");
        e.complete();
        let run = e.lease();
        match e.submit(&run, &all_clean(&["src.rs"])) {
            Err(RunError::Rejected(r)) => {
                assert_eq!(r.code, "incomplete-matrix");
                assert!(
                    r.problems.iter().any(|p| p.contains("src/a.rs")),
                    "{:?}",
                    r.problems
                );
            }
            other => panic!("{other:?}"),
        }
        let v = e.view();
        assert_eq!(v.latest_job(TRACK).unwrap().failures_of(RunKind::First), 1);
        assert!(matches!(
            e.submit(&run, &all_clean(&["src.rs", "src/a.rs"])),
            Err(RunError::Used)
        ));
        let next = e.lease();
        assert_eq!(next.attempt, 2);
        match e.submit(&next, &json!({"dispositions": []})) {
            Err(RunError::Rejected(r)) => assert_eq!(r.code, "empty-matrix"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ts74_a_token_for_another_run_is_refused_without_a_state_change() {
        let e = Env::new("store-ts74");
        e.complete();
        let run = e.lease();
        let before = e.event_count();
        let other = e
            .runs
            .submit("proj", "run_other", run.token.expose(), "{}", NOW + 1);
        assert!(matches!(other, Err(RunError::WrongRun)));
        let forged = e.runs.submit(
            "proj",
            &run.run_id,
            RunToken::mint().expose(),
            "{}",
            NOW + 1,
        );
        assert!(matches!(forged, Err(RunError::UnknownToken)));
        let late = e.runs.submit(
            "proj",
            &run.run_id,
            run.token.expose(),
            "{}",
            run.lease_until,
        );
        assert!(matches!(late, Err(RunError::Expired)));
        assert_eq!(e.event_count(), before);
    }

    #[test]
    fn ts37_three_expired_leases_fail_the_review() {
        let e = Env::new("store-ts37");
        e.complete();
        let log = e.runs.log("proj");
        let mut now = NOW;
        for attempt in 1..=3 {
            let run = e.runs.lease_next("proj", now).unwrap().expect("queued");
            assert_eq!(run.attempt, attempt);
            now = run.lease_until;
            assert_eq!(reap_expired(&log, now).unwrap(), 1);
            assert_eq!(reap_expired(&log, now).unwrap(), 0, "reaping is idempotent");
        }
        assert_eq!(e.job_state(), JobState::ReviewFailed);
        assert!(e.runs.lease_next("proj", now).unwrap().is_none());
    }

    #[test]
    fn ts50_a_secret_is_redacted_in_the_log_and_phi_rejects_with_ids_only_logged() {
        let e = Env::new("store-ts50");
        e.complete();
        let run = e.lease();
        let key = format!("ghp_{}", "a1b2c3d4e5f6g7h8i9j0");
        let body = with_finding(
            &["src.rs", "src/a.rs"],
            "src/a.rs",
            &format!("Key {key} is hard-coded."),
        );
        let logs = capture_logs(|| e.submit(&run, &body).unwrap());
        let text = std::fs::read_to_string(e.runs.log("proj").path()).unwrap();
        assert!(!text.contains(&key), "the log never holds the secret");
        assert!(text.contains("[redacted: GitHub token]"));
        assert!(
            !logs.contains(&key)
                && !logs.contains(run.token.expose())
                && !logs.contains("hard-coded"),
            "{logs}"
        );

        let e = Env::new("store-ts50-phi");
        e.complete();
        let run = e.lease();
        let body = with_finding(
            &["src.rs", "src/a.rs"],
            "src/a.rs",
            "Fixture holds SSN 123-45-6789.",
        );
        let logs = capture_logs(|| match e.submit(&run, &body) {
            Err(RunError::Rejected(r)) => assert_eq!(r.code, "likely-phi"),
            other => panic!("{other:?}"),
        });
        let text = std::fs::read_to_string(e.runs.log("proj").path()).unwrap();
        assert!(!text.contains("123-45-6789") && !logs.contains("123-45-6789"));
    }

    /// Re-complete the track at its new head and lease the new job's run.
    fn next_job(e: &Env) -> LeasedRun {
        e.complete();
        e.lease()
    }

    /// Job 1: `src/a.rs` gets a long-function finding.
    fn with_open_finding(name: &str) -> (Env, String) {
        let e = Env::new(name);
        e.complete();
        let run = e.lease();
        let body = with_finding(
            &["src.rs", "src/a.rs"],
            "src/a.rs",
            "Function a is too long.",
        );
        e.submit(&run, &body).unwrap();
        let v = e.view();
        let f = v.track_findings(TRACK);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].status, FindingStatus::Open);
        (e, f[0].record.finding_id.clone())
    }

    fn status_of(e: &Env, id: &str) -> FindingStatus {
        e.view().finding(id).unwrap().status
    }

    #[test]
    fn ts32_a_renamed_file_keeps_its_finding_id() {
        let (e, id) = with_open_finding("store-ts32");
        git(&e.r.worktree, &["mv", "src/a.rs", "src/b.rs"]);
        git(&e.r.worktree, &["commit", "-q", "-m", "rename"]);
        let run = next_job(&e);
        let c = e
            .runs
            .claim("proj", run.token.expose(), RunKind::First, NOW + 1)
            .unwrap();
        let priors = c.context.required_priors(RunKind::First);
        assert_eq!(priors.len(), 1);
        assert_eq!(priors[0].finding.record.finding_id, id);
        assert_eq!(priors[0].path_at_head, "src/b.rs");
        let mut body = with_finding(
            &["src.rs", "src/b.rs"],
            "src/b.rs",
            "Function a is too long.",
        );
        body["priorFindings"] = json!([{"findingId": id, "outcome": "finding"}]);
        let ok = e.submit(&run, &body).unwrap();
        assert_eq!(ok.findings_opened, 0, "one finding, not two");
        assert_eq!(e.view().track_findings(TRACK).len(), 1);
        assert_eq!(status_of(&e, &id), FindingStatus::Open);
    }

    #[test]
    fn a_prior_dispositioned_clean_is_resolved_and_a_regression_is_new() {
        let (e, id) = with_open_finding("store-resolve");
        e.r.commit_on_feat("src/a.rs", "fn a() { short(); }\n");
        let run = next_job(&e);
        let mut body = all_clean(&["src.rs", "src/a.rs"]);
        body["priorFindings"] =
            json!([{"findingId": id, "outcome": "clean", "span": {"startLine": 1, "endLine": 1}}]);
        e.submit(&run, &body).unwrap();
        assert_eq!(status_of(&e, &id), FindingStatus::Resolved);
        e.r.commit_on_feat("src/a.rs", "fn a() { long(); }\n");
        let run = next_job(&e);
        let body = with_finding(
            &["src.rs", "src/a.rs"],
            "src/a.rs",
            "Function a is too long.",
        );
        assert_eq!(e.submit(&run, &body).unwrap().findings_opened, 1);
        assert_eq!(e.view().track_findings(TRACK).len(), 2);
    }

    #[test]
    fn ts34_a_finding_whose_hunk_left_the_scope_is_resolved_out_of_scope() {
        let (e, id) = with_open_finding("store-ts34");
        e.r.remove_on_feat("src/a.rs");
        let run = next_job(&e);
        e.submit(&run, &all_clean(&["src.rs"])).unwrap();
        assert_eq!(status_of(&e, &id), FindingStatus::ResolvedOutOfScope);
    }

    #[test]
    fn ts35_a_rule_removed_on_the_target_withdraws_its_findings() {
        let (e, id) = with_open_finding("store-ts35");
        e.r.commit_on_main(
            STYLE,
            "# General\n- [rule:plain category=restating-comments] Plain.\n",
        );
        e.r.merge_main_into_feat();
        let run = next_job(&e);
        let body = json!({"dispositions": [clean("plain", "src.rs"), clean("plain", "src/a.rs")]});
        e.submit(&run, &body).unwrap();
        assert_eq!(status_of(&e, &id), FindingStatus::RuleWithdrawn);
    }

    #[test]
    fn ts36_a_rule_removed_only_on_the_track_leaves_the_finding_open() {
        let (e, id) = with_open_finding("store-ts36");
        e.r.commit_on_feat(
            STYLE,
            "# General\n- [rule:plain category=restating-comments] Plain.\n",
        );
        let run = next_job(&e);
        let c = e
            .runs
            .claim("proj", run.token.expose(), RunKind::First, NOW + 1)
            .unwrap();
        assert_eq!(
            c.context.required_priors(RunKind::First).len(),
            1,
            "the base still has the rule"
        );
        let mut body = all_clean(&["src.rs", "src/a.rs"]);
        body["priorFindings"] = json!([{"findingId": id, "outcome": "finding"}]);
        let ok = e.submit(&run, &body).unwrap();
        assert_eq!(status_of(&e, &id), FindingStatus::Open);
        assert!(!ok.verdict.passing);
        let v = e.view();
        let sc: Vec<_> = v
            .track_findings(TRACK)
            .into_iter()
            .filter(|f| f.record.rule_id == "standards-change")
            .collect();
        assert_eq!(sc.len(), 1, "the track's standards edit is a finding");
    }

    /// Run `f` with a capturing subscriber; returns everything it logged.
    pub(crate) fn capture_logs<T>(f: impl FnOnce() -> T) -> String {
        use std::sync::Mutex;
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf::default();
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || sink.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let bytes = buf.0.lock().unwrap().clone();
        String::from_utf8_lossy(&bytes).to_string()
    }
}
