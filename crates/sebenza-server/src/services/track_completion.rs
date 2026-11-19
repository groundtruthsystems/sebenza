//! The track completion transition (FR-01..FR-03): the only writer of a
//! track's completion.
//!
//! Completing pins the job key — head, merge-base and `standards_rev` — then
//! appends one fsynced `track-completed` event to the project's review log.
//! That fsync is the commit point. Only then is `status: "done"` projected
//! into the worktree's `tracks.json`; a crash in between is repaired by the
//! reconciler, because the log wins. A repeat with the same job key appends
//! nothing. No lock is held across a git call.

use crate::services::git_pinned::{merge_base, rev_parse};
use crate::services::standards_rev::standards_rev;
use crate::services::tracks_projection::{Projected, ProjectionError, project_done};
use common::adapters::review_log::{CompletionSource, ReviewLog, TrackCompleted};
use common::domain::solution::CredentialClass;
use std::path::{Path, PathBuf};

/// The worktree a completion is about, as the daemon records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionTarget {
    /// Names the project's review log.
    pub project_key: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    /// The branch the track will merge into; the base is pinned against it.
    pub target_branch: String,
}

impl CompletionTarget {
    /// The worktree's `.ai/sebenza/tracks.json`.
    pub fn tracks_path(&self) -> PathBuf {
        tracks_path(&self.worktree_path)
    }
}

/// `<worktree>/.ai/sebenza/tracks.json`.
pub fn tracks_path(worktree: &Path) -> PathBuf {
    worktree.join(".ai").join("sebenza").join("tracks.json")
}

/// The pinned inputs of a review job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pins {
    pub head_sha: String,
    pub base_sha: String,
    pub standards_rev: String,
}

/// The result of a completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The event that holds this job: the new one, or the one already there.
    pub event: TrackCompleted,
    /// False when the job key was already in the log.
    pub appended: bool,
    pub projected: Projected,
}

/// Why a completion was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum CompletionError {
    #[error("invalid track id")]
    BadTrack,
    #[error("track {0} does not belong to this worktree")]
    ForeignTrack(String),
    #[error("could not pin the review inputs: {0}")]
    Unpinnable(String),
    #[error("review log: {0}")]
    Log(#[from] std::io::Error),
    #[error("tracks.json projection: {0}")]
    Projection(#[from] ProjectionError),
    #[cfg(test)]
    #[error("injected fault")]
    Injected,
}

/// Owns completion for every project; each has its own log under `root`.
pub struct TrackCompletionService {
    root: PathBuf,
}

impl TrackCompletionService {
    /// A service over the review logs under `root` (`~/.ai/sebenza/reviews`).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The review log of `project_key`.
    pub fn log(&self, project_key: &str) -> ReviewLog {
        ReviewLog::for_project(&self.root, project_key)
    }

    /// Complete `track_id` in `target`'s worktree for a caller holding
    /// `credential`. Idempotent on the job key.
    pub fn complete(
        &self,
        target: &CompletionTarget,
        track_id: &str,
        credential: CredentialClass,
    ) -> Result<Completion, CompletionError> {
        check_own_track(target, track_id)?;
        let event = completion_event(target, track_id, &pin(target)?, CompletionSource::Daemon);
        let key = event.job_key();
        let (event, appended) = self
            .log(&target.project_key)
            .append_completion_unless(with_credential(event, credential), |s| {
                s.job(&key).cloned()
            })?;
        #[cfg(test)]
        hooks::after_log_fsync()?;
        let projected = project_done(&target.tracks_path(), track_id)?;
        Ok(Completion {
            event,
            appended,
            projected,
        })
    }

    /// Record a completion the log lacks for a track already marked done,
    /// pinned at the branch head now. Appends nothing when the track already
    /// has any completion. Returns the event when one was appended.
    pub fn record_reconciled(
        &self,
        target: &CompletionTarget,
        track_id: &str,
    ) -> Result<Option<TrackCompleted>, CompletionError> {
        let event = completion_event(
            target,
            track_id,
            &pin(target)?,
            CompletionSource::Reconciled,
        );
        let (event, appended) = self
            .log(&target.project_key)
            .append_completion_unless(event, |s| s.completion_for(track_id).cloned())?;
        Ok(appended.then_some(event))
    }
}

fn with_credential(mut event: TrackCompleted, credential: CredentialClass) -> TrackCompleted {
    event.credential = Some(credential.as_str().to_string());
    event
}

fn completion_event(
    target: &CompletionTarget,
    track_id: &str,
    pins: &Pins,
    source: CompletionSource,
) -> TrackCompleted {
    TrackCompleted {
        track_id: track_id.to_string(),
        head_sha: pins.head_sha.clone(),
        base_sha: pins.base_sha.clone(),
        standards_rev: pins.standards_rev.clone(),
        source,
        credential: None,
        worktree_path: target.worktree_path.to_string_lossy().to_string(),
        branch: target.branch.clone(),
    }
}

/// Pin the branch head, its merge-base with the target and the standards
/// revision at that base.
pub fn pin(target: &CompletionTarget) -> Result<Pins, CompletionError> {
    let dir = &target.worktree_path;
    let head = rev_parse(dir, &format!("refs/heads/{}", target.branch))
        .map_err(CompletionError::Unpinnable)?;
    let target_sha = rev_parse(dir, &target.target_branch).map_err(CompletionError::Unpinnable)?;
    let base = merge_base(dir, &head, &target_sha).map_err(CompletionError::Unpinnable)?;
    let rev = standards_rev(dir, &base).map_err(CompletionError::Unpinnable)?;
    Ok(Pins {
        head_sha: head,
        base_sha: base,
        standards_rev: rev,
    })
}

/// A track id names one directory under `.ai/sebenza/tracks/`.
pub fn valid_track_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The track must be registered in this worktree's `tracks.json`, have its
/// directory here, and not record a different origin branch.
pub fn check_own_track(target: &CompletionTarget, track_id: &str) -> Result<(), CompletionError> {
    if !valid_track_id(track_id) {
        return Err(CompletionError::BadTrack);
    }
    let foreign = || CompletionError::ForeignTrack(track_id.to_string());
    let text = std::fs::read_to_string(target.tracks_path()).map_err(|_| foreign())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| foreign())?;
    let track = v["tracks"]
        .as_array()
        .and_then(|ts| ts.iter().find(|t| t["track_id"] == track_id))
        .ok_or_else(foreign)?;
    let origin = track["origin"]["branch"].as_str();
    let dir = target
        .worktree_path
        .join(".ai/sebenza/tracks")
        .join(track_id);
    if origin.is_some_and(|b| b != target.branch) || !dir.is_dir() {
        return Err(foreign());
    }
    Ok(())
}

/// Test-only fault injection between the log fsync and the projection.
#[cfg(test)]
pub mod hooks {
    use std::cell::Cell;

    thread_local! {
        static FAIL_AFTER_FSYNC: Cell<bool> = const { Cell::new(false) };
    }

    /// Make the next completion on this thread stop after its log append,
    /// as if the daemon died before projecting.
    pub fn fail_after_log_fsync() {
        FAIL_AFTER_FSYNC.with(|f| f.set(true));
    }

    pub(super) fn after_log_fsync() -> Result<(), super::CompletionError> {
        if FAIL_AFTER_FSYNC.with(|f| f.replace(false)) {
            return Err(super::CompletionError::Injected);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{OTHER_TRACK, TRACK, TestRepo};
    use std::io::Write;

    fn target(r: &TestRepo) -> CompletionTarget {
        CompletionTarget {
            project_key: "proj".into(),
            worktree_path: r.worktree.clone(),
            branch: "feat".into(),
            target_branch: "main".into(),
        }
    }

    fn service(r: &TestRepo) -> TrackCompletionService {
        TrackCompletionService::new(r.base.join("reviews"))
    }

    #[test]
    fn ts01_completing_appends_one_event_with_the_job_key_then_projects_done() {
        let r = TestRepo::new("complete");
        let svc = service(&r);
        let done = svc
            .complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap();
        assert!(done.appended);
        assert_eq!(done.projected, Projected::Applied);
        let e = &done.event;
        assert_eq!(e.head_sha, r.head());
        assert_eq!(e.base_sha, r.main_head());
        assert_eq!(
            e.standards_rev,
            standards_rev(&r.repo, &r.main_head()).unwrap()
        );
        assert_eq!(e.source, CompletionSource::Daemon);
        assert_eq!(e.credential.as_deref(), Some("control"));
        assert_eq!(svc.log("proj").open().unwrap().job_count(TRACK), 1);
        assert_eq!(r.track_status(TRACK), "done");
        assert_eq!(r.track_status(OTHER_TRACK), "doing");
    }

    #[test]
    fn ts65_a_repeat_at_the_same_head_is_idempotent() {
        let r = TestRepo::new("repeat");
        let svc = service(&r);
        let first = svc
            .complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap();
        let again = svc
            .complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap();
        assert!(!again.appended);
        assert_eq!(again.projected, Projected::AlreadyDone);
        assert_eq!(again.event, first.event);
        assert_eq!(svc.log("proj").open().unwrap().job_count(TRACK), 1);
    }

    #[test]
    fn a_new_head_enqueues_a_new_job() {
        let r = TestRepo::new("new-head");
        let svc = service(&r);
        svc.complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap();
        crate::services::review_test_repo::write(&r.worktree, "more.rs", "// more\n");
        crate::services::review_test_repo::git(&r.worktree, &["add", "-A"]);
        crate::services::review_test_repo::git(&r.worktree, &["commit", "-qm", "more"]);
        let again = svc
            .complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap();
        assert!(again.appended);
        assert_eq!(svc.log("proj").open().unwrap().job_count(TRACK), 2);
    }

    #[test]
    fn ts66_a_track_of_another_worktree_is_refused_without_an_event() {
        let r = TestRepo::new("foreign");
        let svc = service(&r);
        for track in [OTHER_TRACK, "absent_20260101"] {
            let err = svc
                .complete(&target(&r), track, CredentialClass::Control)
                .unwrap_err();
            assert!(
                matches!(err, CompletionError::ForeignTrack(_)),
                "{track}: {err}"
            );
        }
        let mut wrong_branch = target(&r);
        wrong_branch.branch = "other".into();
        assert!(matches!(
            svc.complete(&wrong_branch, TRACK, CredentialClass::Control),
            Err(CompletionError::ForeignTrack(_))
        ));
        assert!(matches!(
            svc.complete(&target(&r), "../escape", CredentialClass::Control),
            Err(CompletionError::BadTrack)
        ));
        assert!(!svc.log("proj").path().exists());
    }

    #[test]
    fn ts03_a_crash_before_the_fsync_leaves_the_track_uncompleted_after_restart() {
        let r = TestRepo::new("crash-before");
        let log = service(&r).log("proj");
        std::fs::create_dir_all(log.path().parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(log.path()).unwrap();
        f.write_all(br#"{"schema_version":1,"event_id":"x","ts":"t","kind":"track-completed","track_id":"t_2026"#)
            .unwrap();
        drop(f);
        let restarted = service(&r);
        let state = restarted.log("proj").open().unwrap();
        assert_eq!(state.job_count(TRACK), 0);
        assert_eq!(r.track_status(TRACK), "doing");
    }

    #[test]
    fn ts04_a_crash_after_the_fsync_keeps_the_completion_for_the_reconciler() {
        let r = TestRepo::new("crash-after");
        let svc = service(&r);
        hooks::fail_after_log_fsync();
        let err = svc
            .complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap_err();
        assert!(matches!(err, CompletionError::Injected));
        let restarted = service(&r);
        assert_eq!(restarted.log("proj").open().unwrap().job_count(TRACK), 1);
        assert_eq!(r.track_status(TRACK), "doing", "not projected yet");
    }

    #[test]
    fn a_reconciled_completion_is_appended_once_per_track() {
        let r = TestRepo::new("reconciled");
        let svc = service(&r);
        let first = svc.record_reconciled(&target(&r), TRACK).unwrap().unwrap();
        assert_eq!(first.source, CompletionSource::Reconciled);
        assert_eq!(first.credential, None);
        assert_eq!(first.head_sha, r.head());
        assert!(svc.record_reconciled(&target(&r), TRACK).unwrap().is_none());
        let live = svc
            .complete(&target(&r), TRACK, CredentialClass::Control)
            .unwrap();
        assert!(!live.appended, "same job key as the reconciled event");
        assert_eq!(svc.log("proj").open().unwrap().job_count(TRACK), 1);
    }

    #[test]
    fn track_ids_are_plain_directory_names() {
        assert!(valid_track_id("standards_review_20261118"));
        assert!(!valid_track_id(""));
        assert!(!valid_track_id("a/b"));
        assert!(!valid_track_id(".."));
        assert!(!valid_track_id(&"a".repeat(129)));
    }
}
