use crate::adapters::tmux::build_worktree_window_name;
use crate::domain::model::{
    AgentFeedbackState, AgentLifecycle, AgentRuntimeState, GitWorktreeRuntimeState,
    ManagedWorktreeRuntimeState, OneshotMeta, PrEntry, ServiceRuntimeState, SessionRuntimeState,
    WorktreeKind, WorktreeSource, WorktreeTab,
};
use std::collections::HashMap;

/// Input for `upsert_worktree`. Fields that are `Option` follow the TS semantics:
/// `None` means "leave existing value untouched" on update.
pub struct UpsertInput {
    pub worktree_id: String,
    pub kind: WorktreeKind,
    pub branch: String,
    pub label: Option<String>,
    pub base_branch: Option<String>,
    pub path: String,
    pub profile: Option<String>,
    pub agent_name: Option<String>,
    pub agent_terminal_stale: bool,
    pub runtime: String,
    pub source: WorktreeSource,
    pub oneshot: Option<OneshotMeta>,
    pub tabs: Vec<WorktreeTab>,
    pub active_tab_id: Option<String>,
}

/// In-memory store of managed worktree runtime state, refreshed by reconciliation.
/// The `agent` sub-state is event-driven and never overwritten by reconcile.
#[derive(Default)]
pub struct ProjectRuntime {
    worktrees: HashMap<String, ManagedWorktreeRuntimeState>,
    worktree_ids_by_branch: HashMap<String, String>,
}

impl ProjectRuntime {
    pub fn new() -> Self {
        ProjectRuntime::default()
    }

    pub fn upsert_worktree(&mut self, input: UpsertInput) {
        if let Some(existing) = self.worktrees.get_mut(&input.worktree_id) {
            if existing.branch != input.branch {
                self.worktree_ids_by_branch.remove(&existing.branch);
            }
            self.worktree_ids_by_branch
                .insert(input.branch.clone(), input.worktree_id.clone());

            existing.path = input.path;
            existing.kind = input.kind;
            existing.branch = input.branch.clone();
            existing.label = input.label;
            existing.base_branch = input.base_branch;
            if let Some(profile) = input.profile {
                existing.profile = Some(profile);
            }
            if let Some(agent) = input.agent_name {
                existing.agent_name = Some(agent);
            }
            existing.agent_terminal_stale = input.agent_terminal_stale;
            existing.agent.runtime = input.runtime;
            existing.source = input.source;
            existing.oneshot = input.oneshot;
            existing.tabs = input.tabs;
            existing.active_tab_id = input.active_tab_id;
            existing.git.exists = true;
            existing.git.branch = input.branch.clone();
            existing.session.window_name = build_worktree_window_name(&input.branch);
            return;
        }

        let created = make_default_state(input);
        self.worktree_ids_by_branch
            .insert(created.branch.clone(), created.worktree_id.clone());
        self.worktrees.insert(created.worktree_id.clone(), created);
    }

    pub fn set_git_state(&mut self, worktree_id: &str, patch: GitWorktreeRuntimeState) {
        if let Some(state) = self.worktrees.get_mut(worktree_id) {
            state.git = GitWorktreeRuntimeState {
                branch: state.branch.clone(),
                ..patch
            };
        }
    }

    pub fn set_session_state(
        &mut self,
        worktree_id: &str,
        exists: bool,
        session_name: Option<String>,
        pane_count: i32,
    ) {
        if let Some(state) = self.worktrees.get_mut(worktree_id) {
            state.session = SessionRuntimeState {
                exists,
                session_name,
                window_name: build_worktree_window_name(&state.branch),
                pane_count,
            };
        }
    }

    pub fn set_services(&mut self, worktree_id: &str, services: Vec<ServiceRuntimeState>) {
        if let Some(state) = self.worktrees.get_mut(worktree_id) {
            state.services = services;
        }
    }

    pub fn set_prs(&mut self, worktree_id: &str, prs: Vec<PrEntry>) {
        if let Some(state) = self.worktrees.get_mut(worktree_id) {
            state.prs = prs;
        }
    }

    /// Overwrite a worktree's oneshot arm state (used by the oneshot watcher to
    /// mirror a disarm into the snapshot without waiting for a reconcile).
    pub fn set_oneshot(&mut self, worktree_id: &str, oneshot: Option<OneshotMeta>) {
        if let Some(state) = self.worktrees.get_mut(worktree_id) {
            state.oneshot = oneshot;
        }
    }

    pub fn remove_worktree(&mut self, worktree_id: &str) {
        if let Some(state) = self.worktrees.remove(worktree_id) {
            self.worktree_ids_by_branch.remove(&state.branch);
        }
    }

    /// Apply an agent runtime event (event-driven agent lifecycle; never touched
    /// by reconcile). `Err(())` if the worktree id is unknown (caller reconciles
    /// and retries).
    pub fn apply_event(&mut self, event: &crate::domain::events::RuntimeEvent) -> Result<(), ()> {
        use crate::domain::events::RuntimeEvent;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let worktree_id = event.worktree_id().to_string();
        let branch = event.branch().to_string();

        // Re-key by branch if the event reports a renamed branch.
        if let Some(state) = self.worktrees.get(&worktree_id)
            && state.branch != branch
        {
            self.worktree_ids_by_branch.remove(&state.branch);
            self.worktree_ids_by_branch
                .insert(branch.clone(), worktree_id.clone());
        }

        let Some(state) = self.worktrees.get_mut(&worktree_id) else {
            return Err(());
        };
        if state.branch != branch {
            state.branch = branch.clone();
            state.git.branch = branch.clone();
            state.session.window_name = build_worktree_window_name(&branch);
        }
        state.agent.last_event_at = Some(now.clone());
        let event_at = now.clone();
        let feedback_before = state.agent.feedback_state;

        match event {
            RuntimeEvent::AgentStopped { .. } => {
                state.agent.lifecycle = AgentLifecycle::Stopped;
                // A dead session cannot be waiting on anyone. Left set, it would advertise
                // "needs your input" forever with no way to clear it.
                state.agent.feedback_state = AgentFeedbackState::None;
            }
            RuntimeEvent::AgentStatusChanged { lifecycle, .. } => {
                // Both fields are decided in this one match so they cannot drift apart.
                // `awaiting_permission` previously fell through to `Stopped`, which made a
                // worktree blocked on a permission prompt look finished.
                let (next_lifecycle, next_feedback) = match lifecycle.as_str() {
                    "starting" => (AgentLifecycle::Starting, AgentFeedbackState::None),
                    "running" => (AgentLifecycle::Running, AgentFeedbackState::None),
                    "idle" => (AgentLifecycle::Idle, AgentFeedbackState::None),
                    "awaiting_permission" => (
                        AgentLifecycle::AwaitingPermission,
                        AgentFeedbackState::PermissionRequest,
                    ),
                    _ => (AgentLifecycle::Stopped, AgentFeedbackState::None),
                };
                state.agent.lifecycle = next_lifecycle;
                // Any later lifecycle report clears a pending request. This is the only
                // signal available: the event carries no correlation id, so "the agent is
                // making progress again" is the proxy for "the human answered". It holds
                // because a blocked agent does not report progress while blocked.
                state.agent.feedback_state = next_feedback;
                if state.agent.last_started_at.is_none() && lifecycle == "running" {
                    state.agent.last_started_at = Some(now);
                }
                state.agent.last_error = None;
            }
            RuntimeEvent::RuntimeError { message, .. } => {
                state.agent.lifecycle = AgentLifecycle::Error;
                state.agent.feedback_state = AgentFeedbackState::None;
                state.agent.last_error = Some(message.clone());
            }
            RuntimeEvent::PrOpened { .. } => {}
            // Record the id so the opencode conversation service can export it. This is
            // the only route by which Sebenza learns an opencode session id.
            RuntimeEvent::ConversationStarted { session_id, .. } => {
                state.reported_session_id = Some(session_id.clone());
            }
        }

        // Logged because this state drives whether the dashboard tells the user a worktree
        // is waiting on them; a wrong or stuck badge is otherwise untraceable after the
        // fact, since the state is in-memory and leaves no other record.
        if let Some(record) = feedback_transition_record(
            &worktree_id,
            event.kind(),
            feedback_before,
            state.agent.feedback_state,
            &event_at,
        ) {
            tracing::info!("{record}");
        }

        Ok(())
    }

    /// The runtime state for a branch, cloned, or `None` if not tracked.
    pub fn get_worktree_by_branch(&self, branch: &str) -> Option<ManagedWorktreeRuntimeState> {
        self.worktree_ids_by_branch
            .get(branch)
            .and_then(|id| self.worktrees.get(id))
            .cloned()
    }

    /// All worktrees, sorted by branch name (matches the TS `listWorktrees`).
    pub fn list_worktrees(&self) -> Vec<ManagedWorktreeRuntimeState> {
        let mut states: Vec<ManagedWorktreeRuntimeState> =
            self.worktrees.values().cloned().collect();
        states.sort_by(|a, b| a.branch.cmp(&b.branch));
        states
    }
}

fn make_default_state(input: UpsertInput) -> ManagedWorktreeRuntimeState {
    let window_name = build_worktree_window_name(&input.branch);
    ManagedWorktreeRuntimeState {
        worktree_id: input.worktree_id,
        kind: input.kind,
        branch: input.branch.clone(),
        label: input.label,
        base_branch: input.base_branch,
        path: input.path,
        profile: input.profile,
        agent_name: input.agent_name,
        source: input.source,
        oneshot: input.oneshot,
        tabs: input.tabs,
        active_tab_id: input.active_tab_id,
        agent_terminal_stale: input.agent_terminal_stale,
        git: GitWorktreeRuntimeState {
            exists: true,
            branch: input.branch.clone(),
            dirty: false,
            ahead_count: 0,
            current_commit: None,
        },
        session: SessionRuntimeState {
            exists: false,
            session_name: None,
            window_name,
            pane_count: 0,
        },
        agent: AgentRuntimeState {
            runtime: input.runtime,
            lifecycle: AgentLifecycle::Closed,
            feedback_state: AgentFeedbackState::None,
            last_started_at: None,
            last_event_at: None,
            last_error: None,
        },
        services: Vec::new(),
        prs: Vec::new(),
        reported_session_id: None,
    }
}

/// The log record for a feedback-state change, or `None` if nothing changed.
///
/// Returning `Option` puts "only log real transitions" in one testable place rather
/// than leaving it to each caller.
///
/// Content-free by construction: every parameter is either an id, a fixed event
/// discriminant, a state value, or a timestamp. There is no parameter through which a
/// prompt, tool argument, terminal line, branch name, or token could reach the log, so
/// the guarantee holds by signature rather than by reviewer vigilance.
fn feedback_transition_record(
    worktree_id: &str,
    event_kind: &'static str,
    from: AgentFeedbackState,
    to: AgentFeedbackState,
    at: &str,
) -> Option<String> {
    if from == to {
        return None;
    }
    Some(format!(
        "[runtime-feedback] {worktree_id}: {} -> {} via {event_kind} at {at}",
        from.as_str(),
        to.as_str(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::RuntimeEvent;

    const WT: &str = "wt1";
    const BRANCH: &str = "feature";

    fn runtime_with_worktree() -> ProjectRuntime {
        let mut rt = ProjectRuntime::new();
        rt.upsert_worktree(UpsertInput {
            worktree_id: WT.to_string(),
            kind: WorktreeKind::Linked,
            branch: BRANCH.to_string(),
            label: None,
            base_branch: None,
            path: "/repo/feature".to_string(),
            profile: None,
            agent_name: None,
            agent_terminal_stale: false,
            runtime: "host".to_string(),
            source: WorktreeSource::Ui,
            oneshot: None,
            tabs: Vec::new(),
            active_tab_id: None,
        });
        rt
    }

    fn status_changed(lifecycle: &str) -> RuntimeEvent {
        RuntimeEvent::AgentStatusChanged {
            worktree_id: WT.to_string(),
            branch: BRANCH.to_string(),
            lifecycle: lifecycle.to_string(),
        }
    }

    fn agent(rt: &ProjectRuntime) -> AgentRuntimeState {
        rt.get_worktree_by_branch(BRANCH)
            .expect("worktree should be tracked")
            .agent
    }

    /// Drives the runtime into a pending permission request, so the reset tests start
    /// from a state that actually has something to clear.
    fn runtime_awaiting_permission() -> ProjectRuntime {
        let mut rt = runtime_with_worktree();
        rt.apply_event(&status_changed("awaiting_permission"))
            .expect("worktree is tracked");
        assert_eq!(
            agent(&rt).feedback_state,
            AgentFeedbackState::PermissionRequest,
            "precondition: the worktree must be awaiting permission"
        );
        rt
    }

    #[test]
    fn awaiting_permission_sets_lifecycle_and_feedback_together() {
        let mut rt = runtime_with_worktree();

        rt.apply_event(&status_changed("awaiting_permission"))
            .expect("worktree is tracked");

        let agent = agent(&rt);
        // Both fields come from the same match arm. Asserting them together is the point:
        // `awaiting_permission` previously fell through to `Stopped`, which made a blocked
        // worktree look finished.
        assert_eq!(agent.lifecycle, AgentLifecycle::AwaitingPermission);
        assert_eq!(agent.feedback_state, AgentFeedbackState::PermissionRequest);
    }

    #[test]
    fn active_lifecycles_report_no_pending_feedback() {
        for lifecycle in ["starting", "running", "idle"] {
            let mut rt = runtime_with_worktree();
            rt.apply_event(&status_changed(lifecycle))
                .expect("worktree is tracked");
            assert_eq!(
                agent(&rt).feedback_state,
                AgentFeedbackState::None,
                "{lifecycle} must not report pending feedback"
            );
        }
    }

    #[test]
    fn a_later_lifecycle_event_clears_a_pending_request() {
        // The only signal available today: no correlation id exists on the event, and a
        // blocked agent does not report progress while it is blocked.
        for lifecycle in ["running", "idle", "starting"] {
            let mut rt = runtime_awaiting_permission();
            rt.apply_event(&status_changed(lifecycle))
                .expect("worktree is tracked");
            assert_eq!(
                agent(&rt).feedback_state,
                AgentFeedbackState::None,
                "a {lifecycle} event must clear the pending request"
            );
        }
    }

    #[test]
    fn agent_stopped_clears_a_pending_request() {
        let mut rt = runtime_awaiting_permission();

        rt.apply_event(&RuntimeEvent::AgentStopped {
            worktree_id: WT.to_string(),
            branch: BRANCH.to_string(),
        })
        .expect("worktree is tracked");

        let agent = agent(&rt);
        assert_eq!(agent.lifecycle, AgentLifecycle::Stopped);
        // Without this reset a dead session advertises "needs your input" forever, and the
        // user cannot clear it from the dashboard because the ticker is read-only.
        assert_eq!(agent.feedback_state, AgentFeedbackState::None);
    }

    #[test]
    fn runtime_error_clears_a_pending_request() {
        let mut rt = runtime_awaiting_permission();

        rt.apply_event(&RuntimeEvent::RuntimeError {
            worktree_id: WT.to_string(),
            branch: BRANCH.to_string(),
            message: "agent crashed".to_string(),
        })
        .expect("worktree is tracked");

        let agent = agent(&rt);
        assert_eq!(agent.lifecycle, AgentLifecycle::Error);
        assert_eq!(agent.feedback_state, AgentFeedbackState::None);
        assert_eq!(agent.last_error.as_deref(), Some("agent crashed"));
    }

    #[test]
    fn reconcile_never_touches_feedback_state() {
        let mut rt = runtime_awaiting_permission();

        // Everything reconciliation is allowed to write.
        rt.set_git_state(
            WT,
            GitWorktreeRuntimeState {
                exists: true,
                branch: BRANCH.to_string(),
                dirty: true,
                ahead_count: 3,
                current_commit: Some("abc1234".to_string()),
            },
        );
        rt.set_session_state(WT, true, Some("sebenza-feature".to_string()), 2);
        rt.set_services(WT, Vec::new());
        rt.set_prs(WT, Vec::new());
        rt.upsert_worktree(UpsertInput {
            worktree_id: WT.to_string(),
            kind: WorktreeKind::Linked,
            branch: BRANCH.to_string(),
            label: Some("relabelled".to_string()),
            base_branch: None,
            path: "/repo/feature".to_string(),
            profile: None,
            agent_name: None,
            agent_terminal_stale: false,
            runtime: "host".to_string(),
            source: WorktreeSource::Ui,
            oneshot: None,
            tabs: Vec::new(),
            active_tab_id: None,
        });

        let agent = agent(&rt);
        assert_eq!(
            agent.feedback_state,
            AgentFeedbackState::PermissionRequest,
            "feedback state is event-driven; reconcile must leave it alone"
        );
        assert_eq!(agent.lifecycle, AgentLifecycle::AwaitingPermission);
    }

    #[test]
    fn no_event_ever_produces_user_question() {
        // `UserQuestion` is reserved: no adapter can observe a free-text question today, so
        // producing one would be a claim Sebenza cannot back up. This guards the whole event
        // surface rather than one arm, so a future arm cannot quietly start setting it.
        let events = vec![
            status_changed("starting"),
            status_changed("running"),
            status_changed("idle"),
            status_changed("awaiting_permission"),
            status_changed("stopped"),
            RuntimeEvent::AgentStopped {
                worktree_id: WT.to_string(),
                branch: BRANCH.to_string(),
            },
            RuntimeEvent::RuntimeError {
                worktree_id: WT.to_string(),
                branch: BRANCH.to_string(),
                message: "boom".to_string(),
            },
            RuntimeEvent::PrOpened {
                worktree_id: WT.to_string(),
                branch: BRANCH.to_string(),
                url: None,
            },
            RuntimeEvent::ConversationStarted {
                worktree_id: WT.to_string(),
                branch: BRANCH.to_string(),
                session_id: "ses_abc".to_string(),
            },
        ];

        for event in events {
            let mut rt = runtime_with_worktree();
            rt.apply_event(&event).expect("worktree is tracked");
            assert_ne!(
                agent(&rt).feedback_state,
                AgentFeedbackState::UserQuestion,
                "no event may produce UserQuestion, but {event:?} did"
            );
        }
    }

    const AT: &str = "2026-07-29T23:00:00.000Z";

    #[test]
    fn an_unchanged_feedback_state_produces_no_record() {
        // One record per real transition. Logging every event would bury the handful of
        // lines that matter under the 5s poll's worth of unchanged status reports.
        for state in [
            AgentFeedbackState::None,
            AgentFeedbackState::PermissionRequest,
            AgentFeedbackState::UserQuestion,
        ] {
            assert!(
                feedback_transition_record(WT, "agent_status_changed", state, state, AT).is_none(),
                "{state:?} -> {state:?} is not a transition"
            );
        }
    }

    #[test]
    fn a_feedback_transition_record_names_the_transition() {
        let record = feedback_transition_record(
            WT,
            "agent_status_changed",
            AgentFeedbackState::None,
            AgentFeedbackState::PermissionRequest,
            AT,
        )
        .expect("a change must be recorded");

        for expected in [WT, "agent_status_changed", "none", "permission_request", AT] {
            assert!(
                record.contains(expected),
                "record {record:?} must mention {expected}"
            );
        }
    }

    #[test]
    fn a_transition_record_carries_no_event_payload() {
        // Built from a fixed event discriminant plus the two state values, so a crafted
        // branch name or error message has no route into the log. Asserted rather than
        // assumed so a future signature change has to break a test to regress it.
        let event = RuntimeEvent::RuntimeError {
            worktree_id: WT.to_string(),
            branch: "feature/patient-12345".to_string(),
            message: "tool output with a secret".to_string(),
        };

        let record = feedback_transition_record(
            WT,
            event.kind(),
            AgentFeedbackState::PermissionRequest,
            AgentFeedbackState::None,
            AT,
        )
        .expect("a change must be recorded");

        for forbidden in ["patient-12345", "secret", "tool output", "feature/"] {
            assert!(
                !record.contains(forbidden),
                "record {record:?} leaked {forbidden}"
            );
        }
    }

    #[test]
    fn unrelated_events_leave_a_pending_request_alone() {
        // A PR opening or a session id being reported says nothing about whether the agent
        // is still waiting on the user, so neither may clear the request.
        for event in [
            RuntimeEvent::PrOpened {
                worktree_id: WT.to_string(),
                branch: BRANCH.to_string(),
                url: Some("https://example.test/pr/1".to_string()),
            },
            RuntimeEvent::ConversationStarted {
                worktree_id: WT.to_string(),
                branch: BRANCH.to_string(),
                session_id: "ses_abc".to_string(),
            },
        ] {
            let mut rt = runtime_awaiting_permission();
            rt.apply_event(&event).expect("worktree is tracked");
            assert_eq!(
                agent(&rt).feedback_state,
                AgentFeedbackState::PermissionRequest,
                "{event:?} must not clear a pending request"
            );
        }
    }
}
