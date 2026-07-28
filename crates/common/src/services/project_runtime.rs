use crate::adapters::tmux::build_worktree_window_name;
use crate::domain::model::{
    AgentLifecycle, AgentRuntimeState, GitWorktreeRuntimeState, ManagedWorktreeRuntimeState,
    OneshotMeta, PrEntry, ServiceRuntimeState, SessionRuntimeState, WorktreeKind, WorktreeSource,
    WorktreeTab,
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

        match event {
            RuntimeEvent::AgentStopped { .. } => {
                state.agent.lifecycle = AgentLifecycle::Stopped;
            }
            RuntimeEvent::AgentStatusChanged { lifecycle, .. } => {
                state.agent.lifecycle = match lifecycle.as_str() {
                    "starting" => AgentLifecycle::Starting,
                    "running" => AgentLifecycle::Running,
                    "idle" => AgentLifecycle::Idle,
                    _ => AgentLifecycle::Stopped,
                };
                if state.agent.last_started_at.is_none() && lifecycle == "running" {
                    state.agent.last_started_at = Some(now);
                }
                state.agent.last_error = None;
            }
            RuntimeEvent::RuntimeError { message, .. } => {
                state.agent.lifecycle = AgentLifecycle::Error;
                state.agent.last_error = Some(message.clone());
            }
            RuntimeEvent::PrOpened { .. } => {}
            // Record the id so the opencode conversation service can export it. This is
            // the only route by which Sebenza learns an opencode session id.
            RuntimeEvent::ConversationStarted { session_id, .. } => {
                state.reported_session_id = Some(session_id.clone());
            }
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
            last_started_at: None,
            last_event_at: None,
            last_error: None,
        },
        services: Vec::new(),
        prs: Vec::new(),
        reported_session_id: None,
    }
}
