//! The worker pool: [`DEFAULT_WORKERS`] tokio tasks that lease queued jobs
//! and hand each run, with its token, to a [`RunLauncher`].
//!
//! Workers share nothing but the review log: a lease is one log transaction,
//! so two workers can never lease the same job. A launcher that fails leaves
//! the lease to expire, which the reconciler reaps as a failed attempt.
//! Phase 5 provides the sandboxed launcher; until a launcher is wired the
//! daemon starts no workers, so no lease is taken that nothing would serve.

use super::run_store::{LeasedRun, ReviewRuns, now_secs};
use crate::server::AppState;
use std::sync::Arc;
use std::time::Duration;

/// Workers the daemon runs.
pub const DEFAULT_WORKERS: usize = 2;
/// How often an idle worker looks for a queued job.
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Starts the review agent for a leased run.
pub trait RunLauncher: Send + Sync {
    /// Launch `run`; its token reaches the agent only through here.
    fn launch(&self, run: LeasedRun) -> Result<(), String>;
}

/// Lease at most one run per project in `projects` and launch each.
/// Returns how many runs were launched.
pub fn work_once(
    runs: &ReviewRuns,
    projects: &[String],
    launcher: &dyn RunLauncher,
    now: u64,
) -> usize {
    let mut launched = 0;
    for project in projects {
        match runs.lease_next(project, now) {
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
    use super::super::run_store::tests::Env;
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);
    impl RunLauncher for Recording {
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
}
