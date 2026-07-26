use crate::adapters::fs::read_worktree_meta;
use crate::domain::model::AgentLifecycle;
use crate::services::lifecycle_service::LifecycleService;
use crate::services::project_runtime::ProjectRuntime;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const POLL_INTERVAL_MS: u64 = 3_000;
/// Idle/stopped/error can be transient (agent between tool calls). Wait this
/// long with a terminal status before firing close.
pub const IDLE_GRACE_MS: u64 = 15_000;

/// Per-branch idle-timer state, keyed by branch. `in_flight` guards against a
/// second firing while the close is running.
#[derive(Default)]
pub struct WatchStates {
    states: HashMap<String, WatchState>,
}

#[derive(Default, Clone, Copy)]
struct WatchState {
    idle_since_ms: Option<u64>,
    in_flight: bool,
}

/// The watcher's decision for one worktree on one tick — pure given the inputs.
#[derive(Debug, PartialEq, Eq)]
pub enum OneshotDecision {
    /// Not settled yet (or not terminal) — keep the (possibly updated) timer.
    Wait { idle_since_ms: Option<u64> },
    /// The run has settled — fire end-of-run actions.
    Fire { reason: String },
}

/// Decide what to do for an armed worktree given the agent lifecycle, whether it
/// has a PR, and the current idle-timer state. Grace logic:
/// `stopped`/`error` fire immediately; `idle`/`closed` need the grace window;
/// anything else resets the timer.
pub fn decide(
    lifecycle: AgentLifecycle,
    has_pr: bool,
    idle_since_ms: Option<u64>,
    now_ms: u64,
    idle_grace_ms: u64,
) -> OneshotDecision {
    let is_terminal = matches!(lifecycle, AgentLifecycle::Stopped | AgentLifecycle::Error);
    let needs_grace = matches!(lifecycle, AgentLifecycle::Idle | AgentLifecycle::Closed);
    if !is_terminal && !needs_grace {
        return OneshotDecision::Wait { idle_since_ms: None };
    }
    let idle_since = idle_since_ms.unwrap_or(now_ms);
    let stable = is_terminal || now_ms.saturating_sub(idle_since) >= idle_grace_ms;
    if !stable {
        return OneshotDecision::Wait { idle_since_ms: Some(idle_since) };
    }
    let reason = if is_terminal {
        format!("agent {}", lifecycle_label(lifecycle))
    } else if lifecycle == AgentLifecycle::Closed {
        "agent closed without resuming".to_string()
    } else if has_pr {
        "agent idle after opening PR".to_string()
    } else {
        "agent idle without opening a PR".to_string()
    };
    OneshotDecision::Fire { reason }
}

fn lifecycle_label(lifecycle: AgentLifecycle) -> &'static str {
    match lifecycle {
        AgentLifecycle::Stopped => "stopped",
        AgentLifecycle::Error => "error",
        AgentLifecycle::Idle => "idle",
        AgentLifecycle::Closed => "closed",
        AgentLifecycle::Running => "running",
        AgentLifecycle::Starting => "starting",
    }
}

/// One watch pass over every oneshot worktree. Blocking (reads meta, may close a
/// session) — call from `spawn_blocking`. Intentionally NOT gated on dashboard
/// activity: a CLI-only oneshot run produces no browser hits but must still act.
pub fn run_oneshot_watch(
    states: &Arc<Mutex<WatchStates>>,
    runtime: &Arc<Mutex<ProjectRuntime>>,
    lifecycle: &LifecycleService,
    now_ms: u64,
) {
    let worktrees = runtime.lock().unwrap().list_worktrees();
    for wt in worktrees {
        if wt.source != crate::domain::model::WorktreeSource::Oneshot {
            continue;
        }
        process_worktree(states, runtime, lifecycle, &wt.branch, &wt.path, wt.agent.lifecycle, !wt.prs.is_empty(), now_ms);
    }
}

#[allow(clippy::too_many_arguments)]
fn process_worktree(
    states: &Arc<Mutex<WatchStates>>,
    runtime: &Arc<Mutex<ProjectRuntime>>,
    lifecycle: &LifecycleService,
    branch: &str,
    path: &str,
    agent_lifecycle: AgentLifecycle,
    has_pr: bool,
    now_ms: u64,
) {
    // Read persisted meta: disarmed (or never armed) → drop tracked state & skip.
    let git_dir_meta = resolve_meta(lifecycle, path);
    let Some(meta) = git_dir_meta else {
        states.lock().unwrap().states.remove(branch);
        return;
    };
    if meta.oneshot.is_none() {
        states.lock().unwrap().states.remove(branch);
        return;
    }

    // Snapshot / update the timer under the lock, bail if already firing.
    let prior = {
        let mut guard = states.lock().unwrap();
        let state = guard.states.entry(branch.to_string()).or_default();
        if state.in_flight {
            return;
        }
        state.idle_since_ms
    };

    let decision = decide(agent_lifecycle, has_pr, prior, now_ms, IDLE_GRACE_MS);
    let reason = match decision {
        OneshotDecision::Wait { idle_since_ms } => {
            states.lock().unwrap().states.get_mut(branch).unwrap().idle_since_ms = idle_since_ms;
            return;
        }
        OneshotDecision::Fire { reason } => reason,
    };

    states.lock().unwrap().states.get_mut(branch).unwrap().in_flight = true;
    tracing::info!("[oneshot-watcher] {branch}: {reason} — firing end-of-run actions");

    let auto_close = meta.oneshot.as_ref().map(|o| o.auto_close_on_done).unwrap_or(false);
    if auto_close {
        // Re-read meta immediately before closing: a user interaction during this
        // window must abort the close.
        if resolve_meta(lifecycle, path).and_then(|m| m.oneshot).is_none() {
            tracing::info!("[oneshot-watcher] {branch}: disarmed before close — skipping");
            states.lock().unwrap().states.remove(branch);
            return;
        }
        match lifecycle.close_worktree(branch) {
            Ok(()) => tracing::info!("[oneshot-watcher] {branch}: closed session"),
            Err(e) => tracing::error!("[oneshot-watcher] {branch}: close failed — {}", e.message),
        }
    }

    // Disarm so the watcher doesn't re-trigger, then mirror to the in-memory
    // runtime so snapshots reflect the disarm without waiting for a reconcile.
    lifecycle.disarm_oneshot(branch);
    let worktree_id = runtime.lock().unwrap().get_worktree_by_branch(branch).map(|w| w.worktree_id);
    if let Some(id) = worktree_id {
        runtime.lock().unwrap().set_oneshot(&id, None);
    }
    states.lock().unwrap().states.remove(branch);
}

/// Read a worktree's persisted meta from its git dir, resolving the git dir via
/// the git gateway (returns `None` if the worktree/git dir can't be resolved).
fn resolve_meta(
    lifecycle: &LifecycleService,
    path: &str,
) -> Option<crate::domain::model::WorktreeMeta> {
    let git_dir = lifecycle.git().resolve_worktree_git_dir(path).ok()?;
    read_worktree_meta(&git_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_terminal_running_resets_timer() {
        assert_eq!(
            decide(AgentLifecycle::Running, false, Some(1_000), 5_000, 15_000),
            OneshotDecision::Wait { idle_since_ms: None }
        );
    }

    #[test]
    fn terminal_stopped_fires_immediately() {
        assert_eq!(
            decide(AgentLifecycle::Stopped, false, None, 0, 15_000),
            OneshotDecision::Fire { reason: "agent stopped".to_string() }
        );
    }

    #[test]
    fn idle_starts_grace_then_fires_after_window() {
        // First observation starts the timer.
        assert_eq!(
            decide(AgentLifecycle::Idle, true, None, 100, 15_000),
            OneshotDecision::Wait { idle_since_ms: Some(100) }
        );
        // Still within grace.
        assert_eq!(
            decide(AgentLifecycle::Idle, true, Some(100), 10_000, 15_000),
            OneshotDecision::Wait { idle_since_ms: Some(100) }
        );
        // Grace elapsed → fires with the PR-aware reason.
        assert_eq!(
            decide(AgentLifecycle::Idle, true, Some(100), 20_000, 15_000),
            OneshotDecision::Fire { reason: "agent idle after opening PR".to_string() }
        );
    }

    #[test]
    fn closed_uses_grace_and_dedicated_reason() {
        assert_eq!(
            decide(AgentLifecycle::Closed, false, Some(0), 20_000, 15_000),
            OneshotDecision::Fire { reason: "agent closed without resuming".to_string() }
        );
    }
}
