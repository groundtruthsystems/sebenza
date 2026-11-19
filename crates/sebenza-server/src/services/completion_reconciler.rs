//! The completion reconciler (FR-04), run at startup and every five minutes.
//!
//! The review log is the source of truth and `tracks.json` its projection.
//! For each project the reconciler:
//! - appends a `source=reconciled` completion, pinned at the branch head now,
//!   for every track marked done with no completion event (a legacy or hand
//!   edit), at most once per track;
//! - re-projects `done` where a completion's worktree shows anything else
//!   (a crash before projection, or a skill overwrote it);
//! - reaps expired run leases: each becomes one failed attempt.

use crate::server::AppState;
use crate::services::project_manager::ProjectApp;
use crate::services::standards_review::run_store::{now_secs, reap_expired};
use crate::services::track_completion::{CompletionTarget, TrackCompletionService};
use crate::services::tracks_projection::{Projected, project_done, track_statuses};
use common::adapters::review_log::ReviewState;
use std::path::Path;
use std::time::Duration;

/// How often the reconciler runs after its startup pass.
pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// One project's worktrees, as the daemon records them.
#[derive(Debug, Clone)]
pub struct ProjectWorktrees {
    pub project_key: String,
    pub worktrees: Vec<CompletionTarget>,
}

/// What one pass changed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileReport {
    pub appended: usize,
    pub reprojected: usize,
    pub reaped: usize,
    pub errors: usize,
}

/// Reconcile one project.
pub fn reconcile_project(
    svc: &TrackCompletionService,
    project: &ProjectWorktrees,
) -> ReconcileReport {
    let mut report = ReconcileReport::default();
    for wt in &project.worktrees {
        append_missing(svc, wt, &mut report);
    }
    match svc.log(&project.project_key).open() {
        Ok(state) => reproject(&state, project, &mut report),
        Err(e) => {
            tracing::warn!(project = %project.project_key, "review log unreadable: {e}");
            report.errors += 1;
        }
    }
    report.reaped = reap_expired_leases(svc, project);
    report
}

/// Fail every run whose lease has expired, freeing its job for the next
/// attempt. Returns how many attempts were recorded.
pub fn reap_expired_leases(svc: &TrackCompletionService, project: &ProjectWorktrees) -> usize {
    reap_expired_leases_at(svc, project, now_secs())
}

/// [`reap_expired_leases`] as of `now` (unix seconds).
pub fn reap_expired_leases_at(
    svc: &TrackCompletionService,
    project: &ProjectWorktrees,
    now: u64,
) -> usize {
    todo!()
}

fn append_missing(
    svc: &TrackCompletionService,
    wt: &CompletionTarget,
    report: &mut ReconcileReport,
) {
    let Ok(statuses) = track_statuses(&wt.tracks_path()) else {
        return;
    };
    let Ok(state) = svc.log(&wt.project_key).open() else {
        report.errors += 1;
        return;
    };
    for (track, status) in statuses {
        if status != "done" || state.completion_for(&track).is_some() {
            continue;
        }
        match svc.record_reconciled(wt, &track) {
            Ok(Some(_)) => report.appended += 1,
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(track = %track, "reconcile: cannot record completion: {e}");
                report.errors += 1;
            }
        }
    }
}

/// Re-apply `done` for each track's latest completion, in the worktree that
/// recorded it, while that worktree is still on the same branch.
fn reproject(state: &ReviewState, project: &ProjectWorktrees, report: &mut ReconcileReport) {
    for event in state.latest_completions() {
        let Some(wt) = project.worktrees.iter().find(|w| {
            w.branch == event.branch && w.worktree_path == Path::new(&event.worktree_path)
        }) else {
            continue;
        };
        match project_done(&wt.tracks_path(), &event.track_id) {
            Ok(Projected::Applied) => report.reprojected += 1,
            Ok(Projected::AlreadyDone) => {}
            Err(e) => {
                tracing::warn!(track = %event.track_id, "reconcile: cannot re-project: {e}");
                report.errors += 1;
            }
        }
    }
}

/// The daemon's record of `app`'s worktrees, each targeting its base branch
/// or the project's main branch.
pub fn project_worktrees(app: &ProjectApp) -> ProjectWorktrees {
    let main_branch = app.config().workspace.main_branch.clone();
    let worktrees = app
        .runtime
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list_worktrees()
        .into_iter()
        .map(|w| CompletionTarget {
            project_key: app.prefix.clone(),
            worktree_path: w.path.clone().into(),
            target_branch: w.base_branch.clone().unwrap_or_else(|| main_branch.clone()),
            branch: w.branch,
        })
        .collect();
    ProjectWorktrees {
        project_key: app.prefix.clone(),
        worktrees,
    }
}

/// One pass over every loaded project. Blocking (git and file IO).
pub fn reconcile_all(state: &AppState) -> ReconcileReport {
    let mut total = ReconcileReport::default();
    for app in state.manager.list() {
        if app
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .list_worktrees()
            .is_empty()
        {
            app.reconciliation.reconcile(&app.path, &app.runtime, true);
        }
        let r = reconcile_project(&state.completion, &project_worktrees(&app));
        total.appended += r.appended;
        total.reprojected += r.reprojected;
        total.reaped += r.reaped;
        total.errors += r.errors;
    }
    total
}

/// Run [`reconcile_all`] now and then every [`RECONCILE_INTERVAL`].
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(RECONCILE_INTERVAL);
        loop {
            tick.tick().await;
            let s = state.clone();
            match tokio::task::spawn_blocking(move || reconcile_all(&s)).await {
                Ok(r) if r != ReconcileReport::default() => tracing::info!(
                    appended = r.appended,
                    reprojected = r.reprojected,
                    reaped = r.reaped,
                    errors = r.errors,
                    "completion reconciler pass"
                ),
                Ok(_) => {}
                Err(e) => tracing::warn!("completion reconciler panicked: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{OTHER_TRACK, TRACK, TestRepo, tracks_json, write};
    use crate::services::track_completion::hooks;
    use common::adapters::review_log::CompletionSource;
    use common::domain::solution::CredentialClass;
    use std::sync::Arc;

    fn project(r: &TestRepo) -> ProjectWorktrees {
        ProjectWorktrees {
            project_key: "proj".into(),
            worktrees: vec![CompletionTarget {
                project_key: "proj".into(),
                worktree_path: r.worktree.clone(),
                branch: "feat".into(),
                target_branch: "main".into(),
            }],
        }
    }

    fn service(r: &TestRepo) -> TrackCompletionService {
        TrackCompletionService::new(r.base.join("reviews"))
    }

    fn mark_done_by_hand(r: &TestRepo) {
        write(
            &r.worktree,
            ".ai/sebenza/tracks.json",
            &tracks_json("done", "feat"),
        );
    }

    #[test]
    fn ts68_done_set_by_hand_gets_one_reconciled_event_across_passes() {
        let r = TestRepo::new("rec-hand");
        mark_done_by_hand(&r);
        let svc = service(&r);
        let first = reconcile_project(&svc, &project(&r));
        let second = reconcile_project(&svc, &project(&r));
        assert_eq!((first.appended, second.appended), (1, 0));
        let state = svc.log("proj").open().unwrap();
        assert_eq!(state.job_count(TRACK), 1);
        let e = state.completion_for(TRACK).unwrap();
        assert_eq!(e.source, CompletionSource::Reconciled);
        assert_eq!(e.head_sha, r.head());
        assert_eq!(
            state.job_count(OTHER_TRACK),
            0,
            "not done, so not completed"
        );
    }

    #[test]
    fn ts05_racing_passes_and_a_live_completion_leave_one_job() {
        let r = TestRepo::new("rec-race");
        mark_done_by_hand(&r);
        let svc = Arc::new(service(&r));
        let p = project(&r);
        let mut handles = Vec::new();
        for i in 0..3 {
            let (svc, p) = (svc.clone(), p.clone());
            handles.push(std::thread::spawn(move || {
                if i == 1 {
                    let _ = svc.complete(&p.worktrees[0], TRACK, CredentialClass::Control);
                } else {
                    reconcile_project(&svc, &p);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(svc.log("proj").open().unwrap().job_count(TRACK), 1);
    }

    #[test]
    fn ts06_the_log_wins_over_an_edited_projection() {
        let r = TestRepo::new("rec-edit");
        let svc = service(&r);
        svc.complete(&project(&r).worktrees[0], TRACK, CredentialClass::Control)
            .unwrap();
        write(
            &r.worktree,
            ".ai/sebenza/tracks.json",
            &tracks_json("doing", "feat"),
        );
        let report = reconcile_project(&svc, &project(&r));
        assert_eq!((report.appended, report.reprojected), (0, 1));
        assert_eq!(r.track_status(TRACK), "done");
    }

    #[test]
    fn ts70_a_missing_projection_is_applied_touching_only_that_track() {
        let r = TestRepo::new("rec-missing");
        let svc = service(&r);
        hooks::fail_after_log_fsync();
        assert!(
            svc.complete(&project(&r).worktrees[0], TRACK, CredentialClass::Control)
                .is_err()
        );
        let before = std::fs::read_to_string(r.tracks_path()).unwrap();
        let restarted = service(&r);
        let report = reconcile_project(&restarted, &project(&r));
        assert_eq!((report.appended, report.reprojected), (0, 1));
        let after = std::fs::read_to_string(r.tracks_path()).unwrap();
        assert_eq!(r.track_status(TRACK), "done");
        assert_eq!(r.track_status(OTHER_TRACK), "doing");
        let changed: Vec<_> = before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .map(|(_, b)| b.trim().split(':').next().unwrap_or("").to_string())
            .collect();
        assert_eq!(changed, vec!["\"status\"", "\"updated_at\""]);
        assert_eq!(restarted.log("proj").open().unwrap().job_count(TRACK), 1);
    }

    #[test]
    fn a_worktree_on_another_branch_is_not_re_projected() {
        let r = TestRepo::new("rec-branch");
        let svc = service(&r);
        svc.complete(&project(&r).worktrees[0], TRACK, CredentialClass::Control)
            .unwrap();
        write(
            &r.worktree,
            ".ai/sebenza/tracks.json",
            &tracks_json("doing", "feat"),
        );
        let mut moved = project(&r);
        moved.worktrees[0].branch = "renamed".into();
        assert_eq!(reconcile_project(&svc, &moved).reprojected, 0);
        assert_eq!(r.track_status(TRACK), "doing");
    }

    #[test]
    fn expired_leases_are_reaped_once_as_failed_attempts() {
        use crate::services::standards_review::free_text::ContentScanner;
        use crate::services::standards_review::run_store::{ReviewRuns, RunSettings};
        let r = TestRepo::new("rec-reap");
        let svc = service(&r);
        assert_eq!(reap_expired_leases(&svc, &project(&r)), 0);
        svc.complete(&project(&r).worktrees[0], TRACK, CredentialClass::Control)
            .unwrap();
        let runs = ReviewRuns::new(
            svc.root().to_path_buf(),
            RunSettings::default(),
            Arc::new(ContentScanner::default()),
        );
        let run = runs.lease_next("proj", 10).unwrap().expect("queued");
        let p = project(&r);
        assert_eq!(reap_expired_leases_at(&svc, &p, run.lease_until - 1), 0);
        assert_eq!(reap_expired_leases_at(&svc, &p, run.lease_until), 1);
        assert_eq!(reap_expired_leases_at(&svc, &p, run.lease_until), 0);
    }
}
