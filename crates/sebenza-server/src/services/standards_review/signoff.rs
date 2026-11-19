//! The sign-off gate (FR-22): the operator signs off a track only while its
//! latest job's verdict passes and the refs still say what was reviewed.
//!
//! Passing means an accepted submission (so a complete matrix), no open or
//! needs-operator-decision finding, no pending second run, and an
//! existing-checks report with no failures; a missing report fails closed.
//! Fresh means the track ref still equals the verdict's head SHA and its
//! merge-base with the target still equals the verdict's base SHA.
//!
//! It is a compare-and-swap. The fold and the refs are validated first; the
//! refs are then pinned again, and one [`ReviewLog::transact`] re-folds the
//! log under its lock, re-checks the verdict and compares the re-pinned SHAs
//! before it appends [`SignedOff`]. A ref that moved after validation, or a
//! newer completion, aborts it with nothing appended. Git is never called
//! under the log's lock (NFR-03).

use super::deriver::verdict_of;
use super::git_pinned::Git;
use super::jobs::{Job, JobState, ReviewView};
use super::resolver::{ResolveError, pin};
use super::run_store::track_statuses;
use super::scope::SnapshotPins;
use common::adapters::review_log::{ReviewEventKind, ReviewLog};
use common::domain::review_records::{SignedOff, VerdictRecord};
use std::collections::BTreeSet;

/// Verdict reason: the job's runs ended without a reviewable outcome
/// (blocked or failed after a verdict was derived).
pub const REVIEW_INCOMPLETE: &str = "review-incomplete";

/// The refs a sign-off compares: the track branch and the target it merges
/// into, both from the daemon's own record of the worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignoffRefs {
    pub branch: String,
    pub target_branch: String,
}

/// Why a sign-off was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum SignoffError {
    #[error("the track has no review verdict yet")]
    NoVerdict,
    #[error("the verdict is not passing: {}", .0.join(", "))]
    NotPassing(Vec<String>),
    #[error("the verdict is stale: the track is now at {head_sha} over base {base_sha}")]
    Stale { head_sha: String, base_sha: String },
    #[error("git: {0}")]
    Git(#[from] ResolveError),
    #[error("review log: {0}")]
    Log(#[from] std::io::Error),
}

/// What a sign-off produced; `appended` is false when the job was already
/// signed off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signoff {
    pub record: SignedOff,
    pub appended: bool,
}

/// Sign off `track_id` for a caller holding `credential`.
pub fn sign_off(
    log: &ReviewLog,
    git: &Git,
    refs: &SignoffRefs,
    track_id: &str,
    credential: &str,
) -> Result<Signoff, SignoffError> {
    let view = ReviewView::fold(&log.events()?);
    if let Some(done) = existing(&view, track_id) {
        return Ok(done);
    }
    let validated = passing_job(&view, track_id)?;
    check_fresh(validated, &current_pins(git, refs)?)?;
    #[cfg(test)]
    hooks::run_after_validation();
    let repinned = current_pins(git, refs)?;
    log.transact(|events| {
        let view = ReviewView::fold(events);
        commit(&view, track_id, &validated.id, &repinned, credential)
    })
}

/// The swap: under the log's lock, the job validated must still be the
/// latest, still passing, and still at the re-pinned SHAs.
fn commit(
    view: &ReviewView,
    track_id: &str,
    validated_id: &str,
    pins: &SnapshotPins,
    credential: &str,
) -> Result<(Option<ReviewEventKind>, Signoff), SignoffError> {
    if let Some(done) = existing(view, track_id) {
        return Ok((None, done));
    }
    let job = passing_job(view, track_id)?;
    check_fresh(job, pins)?;
    if job.id != validated_id {
        return Err(stale(pins));
    }
    let record = signed_off(view, job, credential);
    let event = ReviewEventKind::SignedOff(record.clone());
    let appended = true;
    Ok((Some(event), Signoff { record, appended }))
}

/// The latest job of `track_id`, when its verdict passes now.
pub fn passing_job<'v>(view: &'v ReviewView, track_id: &str) -> Result<&'v Job, SignoffError> {
    let job = view.latest_job(track_id).ok_or(SignoffError::NoVerdict)?;
    let stored = job.verdict().ok_or(SignoffError::NoVerdict)?;
    let reasons = gate_reasons(view, job, stored);
    if reasons.is_empty() {
        Ok(job)
    } else {
        Err(SignoffError::NotPassing(reasons))
    }
}

/// Why `job` cannot be signed off: its verdict re-derived from the fold
/// now, the stored verdict's own reasons, and an unfinished review.
fn gate_reasons(view: &ReviewView, job: &Job, stored: &VerdictRecord) -> Vec<String> {
    let statuses = track_statuses(view, job, None);
    let fresh = verdict_of(
        &statuses,
        job.second_run_pending(),
        job.existing_checks.as_deref(),
    );
    let mut reasons = fresh.reasons;
    if !stored.passing {
        reasons.extend(stored.reasons.iter().cloned());
    }
    if view.state(job) != JobState::Verdict {
        reasons.push(REVIEW_INCOMPLETE.to_string());
    }
    let mut seen = BTreeSet::new();
    reasons.retain(|r| seen.insert(r.clone()));
    reasons
}

/// The latest job's sign-off, when it already has one.
fn existing(view: &ReviewView, track_id: &str) -> Option<Signoff> {
    let job = view.latest_job(track_id)?;
    view.signoff(&job.id).map(|record| Signoff {
        record: record.clone(),
        appended: false,
    })
}

fn check_fresh(job: &Job, pins: &SnapshotPins) -> Result<(), SignoffError> {
    let c = &job.completion;
    if pins.head_sha == c.head_sha && pins.base_sha == c.base_sha {
        Ok(())
    } else {
        Err(stale(pins))
    }
}

fn stale(pins: &SnapshotPins) -> SignoffError {
    SignoffError::Stale {
        head_sha: pins.head_sha.clone(),
        base_sha: pins.base_sha.clone(),
    }
}

fn signed_off(view: &ReviewView, job: &Job, credential: &str) -> SignedOff {
    let c = &job.completion;
    SignedOff {
        track_id: c.track_id.clone(),
        job_id: job.id.clone(),
        head_sha: c.head_sha.clone(),
        base_sha: c.base_sha.clone(),
        credential: credential.to_string(),
        finding_ids: view
            .track_findings(&c.track_id)
            .iter()
            .map(|f| f.record.finding_id.clone())
            .collect(),
    }
}

/// The track ref and its merge-base with the target, pinned now.
pub fn current_pins(git: &Git, refs: &SignoffRefs) -> Result<SnapshotPins, SignoffError> {
    Ok(pin(git, &refs.branch, &refs.target_branch)?)
}

/// Test-only fault injection between validation and the commit.
#[cfg(test)]
pub mod hooks {
    use std::cell::RefCell;

    type Hook = Box<dyn FnOnce()>;

    thread_local! {
        static AFTER_VALIDATION: RefCell<Option<Hook>> = RefCell::new(None);
    }

    /// Run `f` once, on this thread, after the next sign-off validates.
    pub fn after_validation(f: impl FnOnce() + 'static) {
        AFTER_VALIDATION.with(|h| *h.borrow_mut() = Some(Box::new(f)));
    }

    pub(super) fn run_after_validation() {
        if let Some(f) = AFTER_VALIDATION.with(|h| h.borrow_mut().take()) {
            f();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{TASK, TRACK, git, trailers};
    use crate::services::standards_review::run_store::tests::{
        Env, all_clean, second_body, target,
    };
    use crate::services::track_completion::TrackCompletionService;
    use common::adapters::review_log::ReviewEventKind;
    use common::domain::solution::CredentialClass;

    const FILES: [&str; 2] = ["src.rs", "src/a.rs"];

    fn refs() -> SignoffRefs {
        SignoffRefs {
            branch: "feat".into(),
            target_branch: "main".into(),
        }
    }

    /// A completed track whose first and second runs are both clean.
    fn passing(name: &str) -> Env {
        let e = Env::new(name);
        e.complete();
        let run = e.lease();
        e.submit(&run, &all_clean(&FILES)).unwrap();
        let second = e.second();
        let ok = e.submit(&second, &second_body(&FILES, &[])).unwrap();
        assert!(ok.verdict.passing, "{:?}", ok.verdict);
        e
    }

    fn sign(e: &Env) -> Result<Signoff, SignoffError> {
        let git = Git::open(&e.r.worktree).unwrap();
        sign_off(&e.runs.log("proj"), &git, &refs(), TRACK, "operator_cli")
    }

    fn signoffs(e: &Env) -> Vec<SignedOff> {
        let events = e.runs.log("proj").events().unwrap();
        events
            .into_iter()
            .filter_map(|ev| match ev.kind {
                ReviewEventKind::SignedOff(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ts02_a_track_without_a_verdict_cannot_be_signed_off() {
        let e = Env::new("signoff-ts02");
        assert!(matches!(sign(&e), Err(SignoffError::NoVerdict)));
        e.complete();
        assert!(matches!(sign(&e), Err(SignoffError::NoVerdict)));
        assert!(signoffs(&e).is_empty());
    }

    #[test]
    fn ts38_a_passing_fresh_verdict_is_signed_off_with_its_shas() {
        let e = passing("signoff-ts38");
        let done = sign(&e).unwrap();
        assert!(done.appended);
        let job = e.view().latest_job(TRACK).cloned().unwrap();
        assert_eq!(done.record.job_id, job.id);
        assert_eq!(done.record.head_sha, e.r.head());
        assert_eq!(done.record.base_sha, job.completion.base_sha);
        assert_eq!(done.record.credential, "operator_cli");
        assert_eq!(signoffs(&e), vec![done.record.clone()]);
        assert_eq!(e.view().signoff(&job.id), Some(&done.record));
        let again = sign(&e).unwrap();
        assert!(!again.appended, "a repeat appends nothing");
        assert_eq!(signoffs(&e).len(), 1);
    }

    #[test]
    fn non_passing_verdicts_are_refused_with_their_reasons() {
        let e = Env::new("signoff-pending");
        e.complete();
        let run = e.lease();
        e.submit(&run, &all_clean(&FILES)).unwrap();
        match sign(&e) {
            Err(SignoffError::NotPassing(r)) => assert_eq!(r, vec!["second-run-pending"]),
            other => panic!("{other:?}"),
        }
        let second = e.second();
        e.submit(&second, &second_body(&FILES, &[])).unwrap();
        e.record_checks(&["coverage"]);
        match sign(&e) {
            Err(SignoffError::NotPassing(r)) => {
                assert_eq!(r, vec!["existing-checks-failed:coverage"])
            }
            other => panic!("{other:?}"),
        }
        assert!(signoffs(&e).is_empty());
    }

    #[test]
    fn an_open_finding_is_not_passing() {
        let e = Env::new("signoff-open");
        e.complete();
        let run = e.lease();
        e.submit(&run, &all_clean(&FILES)).unwrap();
        let second = e.second();
        let flagged = second_body(&FILES, &[("src.rs", "Restates the call.")]);
        e.submit(&second, &flagged).unwrap();
        match sign(&e) {
            Err(SignoffError::NotPassing(r)) => assert!(
                r.iter()
                    .any(|r| r == "undecided-findings" || r == "open-findings"),
                "{r:?}"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ts39_a_track_ref_moved_after_the_verdict_is_stale() {
        let e = passing("signoff-ts39");
        let moved = e.r.commit_on_feat("src/b.rs", "fn b() {}\n");
        match sign(&e) {
            Err(SignoffError::Stale { head_sha, .. }) => assert_eq!(head_sha, moved),
            other => panic!("{other:?}"),
        }
        assert!(signoffs(&e).is_empty());
    }

    #[test]
    fn ts39_a_moved_merge_base_is_stale() {
        let e = passing("signoff-ts39-base");
        let base = e
            .view()
            .latest_job(TRACK)
            .unwrap()
            .completion
            .base_sha
            .clone();
        let root = git(&e.r.repo, &["rev-list", "--max-parents=0", "main"]);
        git(&e.r.repo, &["update-ref", "refs/heads/main", &root]);
        match sign(&e) {
            Err(SignoffError::Stale { base_sha, .. }) => {
                assert_ne!(base_sha, base);
                assert_eq!(base_sha, root);
            }
            other => panic!("{other:?}"),
        }
        assert!(signoffs(&e).is_empty());
    }

    #[test]
    fn ts39_a_tag_named_like_the_target_cannot_freshen_a_stale_verdict() {
        let e = passing("signoff-tag-shadow");
        let job = e.view().latest_job(TRACK).cloned().unwrap();
        let root = git(&e.r.repo, &["rev-list", "--max-parents=0", "main"]);
        git(&e.r.repo, &["update-ref", "refs/heads/main", &root]);
        git(&e.r.repo, &["tag", "main", &job.completion.base_sha]);
        git(&e.r.repo, &["tag", "feat", &job.completion.head_sha]);
        match sign(&e) {
            Err(SignoffError::Stale { base_sha, .. }) => assert_eq!(base_sha, root),
            other => panic!("{other:?}"),
        }
        assert!(signoffs(&e).is_empty());
    }

    #[test]
    fn ts39_a_branch_reset_to_the_reviewed_head_is_fresh_after_re_completion() {
        let e = passing("signoff-reset");
        let reviewed = e.r.head();
        e.r.commit_on_feat("src/b.rs", "fn b() {}\n");
        e.complete_unapproved();
        git(&e.r.worktree, &["reset", "-q", "--hard", &reviewed]);
        e.completion
            .complete(&target(&e.r), TRACK, CredentialClass::Control)
            .unwrap();
        let done = sign(&e).unwrap();
        assert_eq!(done.record.head_sha, reviewed);
    }

    #[test]
    fn ts40_a_ref_moved_between_validation_and_commit_aborts_the_swap() {
        let e = passing("signoff-ts40");
        let worktree = e.r.worktree.clone();
        hooks::after_validation(move || {
            std::fs::write(worktree.join("src/c.rs"), "fn c() {}\n").unwrap();
            git(&worktree, &["add", "-A"]);
            git(&worktree, &["commit", "-q", "-m", "racing commit"]);
        });
        let before = e.runs.log("proj").events().unwrap().len();
        assert!(matches!(sign(&e), Err(SignoffError::Stale { .. })));
        assert_eq!(e.runs.log("proj").events().unwrap().len(), before);
        assert!(signoffs(&e).is_empty(), "no sign-off record");
    }

    #[test]
    fn a_newer_completion_mid_swap_aborts_it() {
        let e = passing("signoff-superseded");
        let (r_target, worktree) = (target(&e.r), e.r.worktree.clone());
        let completion = TrackCompletionService::new(e.r.base.join("reviews"));
        hooks::after_validation(move || {
            std::fs::write(worktree.join("src/d.rs"), "fn d() {}\n").unwrap();
            git(&worktree, &["add", "-A"]);
            git(
                &worktree,
                &["commit", "-q", "-m", "d", "-m", &trailers(TRACK, TASK)],
            );
            completion
                .complete(&r_target, TRACK, CredentialClass::Control)
                .unwrap();
        });
        assert!(sign(&e).is_err());
        assert!(signoffs(&e).is_empty());
    }

    #[test]
    fn ts52_the_signoff_record_is_metadata_only() {
        let e = passing("signoff-ts52");
        let done = sign(&e).unwrap();
        let events = e.runs.log("proj").events().unwrap();
        let line = serde_json::to_value(events.last().unwrap()).unwrap();
        let mut keys: Vec<&str> = line
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "base_sha",
                "credential",
                "event_id",
                "finding_ids",
                "head_sha",
                "job_id",
                "kind",
                "schema_version",
                "track_id",
                "ts",
            ]
        );
        assert_eq!(line["kind"], "signed-off");
        assert!(!line["ts"].as_str().unwrap().is_empty());
        assert_eq!(done.record.finding_ids, Vec::<String>::new());
    }
}
