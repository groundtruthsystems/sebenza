//! The worker pool: [`DEFAULT_WORKERS`] tokio tasks that lease queued jobs'
//! first runs, then due second runs, and hand each run, with its token, to
//! a [`RunLauncher`].
//!
//! Workers share nothing but the review log: a lease is one log transaction,
//! so two workers can never lease the same job. A launcher that fails leaves
//! the lease to expire, which the reconciler reaps as a failed attempt.
//! A launcher whose CLI is not marked read-only-and-egress-verified is
//! never dispatched to: nothing is leased and jobs stay queued with reason
//! [`CLI_NOT_CONFORMANT`]. The daemon starts workers only when a conformant
//! launcher is available; otherwise [`block_dispatch`] records why.

use super::run_store::{LeasedRun, ReviewRuns, now_secs};
use crate::server::AppState;
use crate::services::role_runner::conformance::review_dispatchable;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Workers the daemon runs.
pub const DEFAULT_WORKERS: usize = 2;
/// How often an idle worker looks for a queued job.
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Queued reason: the review CLI is not read-only-and-egress-verified.
pub const CLI_NOT_CONFORMANT: &str = "cli-not-conformant";
/// Queued reason: no review CLI is installed.
pub const CLI_NOT_INSTALLED: &str = "cli-not-installed";

static DISPATCH_BLOCKED: OnceLock<&'static str> = OnceLock::new();

/// Record, once per daemon, why no review run is dispatched.
pub fn block_dispatch(reason: &'static str) {
    if DISPATCH_BLOCKED.set(reason).is_ok() {
        tracing::warn!(
            reason,
            "standards review: no conformant launcher; jobs stay queued"
        );
    }
}

/// Why this daemon dispatches no review run, if it does not.
pub fn dispatch_blocked() -> Option<&'static str> {
    DISPATCH_BLOCKED.get().copied()
}

/// Starts the review agent for a leased run.
pub trait RunLauncher: Send + Sync {
    /// The CLI this launcher runs, as named in the conformance table.
    fn agent(&self) -> &str;
    /// Launch `run`; its token reaches the agent only through here.
    fn launch(&self, run: LeasedRun) -> Result<(), String>;
}

/// Why `launcher` may not be dispatched to, if it may not.
pub fn dispatch_refusal(launcher: &dyn RunLauncher) -> Option<&'static str> {
    (!review_dispatchable(launcher.agent())).then_some(CLI_NOT_CONFORMANT)
}

/// Lease at most one run per project in `projects`, a queued first run
/// before a due second run, and launch each. Returns how many runs were
/// launched; none when the launcher is not conformant.
pub fn work_once(
    runs: &ReviewRuns,
    projects: &[String],
    launcher: &dyn RunLauncher,
    now: u64,
) -> usize {
    if dispatch_refusal(launcher).is_some() {
        return 0;
    }
    let mut launched = 0;
    for project in projects {
        let leased = runs.lease_next(project, now).and_then(|first| {
            first.map_or_else(|| runs.lease_next_second(project, now), |r| Ok(Some(r)))
        });
        match leased {
            Ok(Some(run)) => {
                let (run_id, job_id) = (run.run_id.clone(), run.job_id.clone());
                match launcher.launch(run) {
                    Ok(()) => launched += 1,
                    Err(e) => {
                        tracing::warn!(run = %run_id, job = %job_id, "review run launch failed: {e}")
                    }
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(project = %project, "review log unreadable: {e}"),
        }
    }
    launched
}

/// A queued job's first run, else a due second run.
fn lease_any(runs: &ReviewRuns, project: &str, now: u64) -> std::io::Result<Option<LeasedRun>> {
    match runs.lease_next(project, now)? {
        Some(run) => Ok(Some(run)),
        None => runs.lease_next_second(project, now),
    }
}

/// Spawn `workers` tasks that call [`work_once`] every [`POLL_INTERVAL`].
pub fn spawn_workers(state: AppState, launcher: Arc<dyn RunLauncher>, workers: usize) {
    for _ in 0..workers {
        let (state, launcher) = (state.clone(), launcher.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(POLL_INTERVAL);
            loop {
                tick.tick().await;
                let (state, launcher) = (state.clone(), launcher.clone());
                let _ = tokio::task::spawn_blocking(move || {
                    let projects: Vec<String> = state
                        .manager
                        .list()
                        .iter()
                        .map(|a| a.prefix.clone())
                        .collect();
                    work_once(
                        &state.review_runs(),
                        &projects,
                        launcher.as_ref(),
                        now_secs(),
                    )
                })
                .await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::jobs::JobState;
    use super::super::run_store::tests::{Env, all_clean};
    use super::*;
    use crate::services::review_test_repo::TRACK;
    use common::domain::review_records::RunKind;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);
    impl RunLauncher for Recording {
        fn agent(&self) -> &str {
            "claude"
        }
        fn launch(&self, run: LeasedRun) -> Result<(), String> {
            self.0.lock().unwrap().push(run.run_id);
            Ok(())
        }
    }

    #[test]
    fn two_workers_racing_lease_one_run_per_job() {
        let e = Env::new("workers-race");
        e.complete();
        let runs = Arc::new(e.runs.clone());
        let launcher = Arc::new(Recording::default());
        let handles: Vec<_> = (0..DEFAULT_WORKERS * 4)
            .map(|_| {
                let (runs, launcher) = (runs.clone(), launcher.clone());
                std::thread::spawn(move || {
                    work_once(&runs, &["proj".into()], launcher.as_ref(), 1_000)
                })
            })
            .collect();
        let launched: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(launched, 1);
        assert_eq!(launcher.0.lock().unwrap().len(), 1);
        assert_eq!(e.view().runs().len(), 1);
        assert_eq!(
            work_once(
                &runs,
                &["proj".into(), "other".into()],
                launcher.as_ref(),
                1_000
            ),
            0
        );
    }

    /// A launcher for `agent` that keeps every run it is handed.
    struct Keeping(&'static str, Mutex<Vec<LeasedRun>>);
    impl RunLauncher for Keeping {
        fn agent(&self) -> &str {
            self.0
        }
        fn launch(&self, run: LeasedRun) -> Result<(), String> {
            self.1.lock().unwrap().push(run);
            Ok(())
        }
    }

    #[test]
    fn ts53_a_cli_not_marked_egress_verified_is_never_dispatched() {
        let e = Env::new("workers-ts53");
        e.complete();
        for agent in ["grok", "codex", "aider"] {
            let launcher = Keeping(agent, Mutex::default());
            assert_eq!(
                dispatch_refusal(&launcher),
                Some(CLI_NOT_CONFORMANT),
                "{agent}"
            );
            assert_eq!(work_once(&e.runs, &["proj".into()], &launcher, 1_000), 0);
            assert!(launcher.1.lock().unwrap().is_empty());
        }
        let v = e.view();
        assert!(v.runs().is_empty(), "no lease was taken");
        assert_eq!(v.state(v.latest_job(TRACK).unwrap()), JobState::Queued);
        assert_eq!(dispatch_refusal(&Keeping("claude", Mutex::default())), None);
    }

    #[test]
    fn ts58_workers_dispatch_the_second_run_once_the_first_is_accepted() {
        let e = Env::new("workers-second");
        e.complete();
        let launcher = Keeping("claude", Mutex::default());
        let projects = ["proj".to_string()];
        assert_eq!(work_once(&e.runs, &projects, &launcher, 1_000), 1);
        assert_eq!(
            work_once(&e.runs, &projects, &launcher, 1_000),
            0,
            "first run open"
        );
        let first = launcher.1.lock().unwrap()[0].clone();
        assert_eq!(first.kind, RunKind::First);
        e.submit(&first, &all_clean(&["src.rs", "src/a.rs"]))
            .unwrap();
        assert_eq!(work_once(&e.runs, &projects, &launcher, 1_000), 1);
        let second = launcher.1.lock().unwrap()[1].clone();
        assert_eq!(second.kind, RunKind::Second);
        assert_eq!(second.job_id, first.job_id);
        assert_ne!(second.token.expose(), first.token.expose());
    }
}
