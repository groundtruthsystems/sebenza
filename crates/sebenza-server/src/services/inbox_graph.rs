//! The server's side of the inbox item graph: which worktrees the daemon
//! manages (confinement) and what their agents are doing, both read from the
//! in-memory project runtime. No reconcile and no tmux call per request, so a
//! 15 s poll stays cheap.

use crate::services::project_manager::ProjectManager;
use common::domain::task_graph::AgentState;
use common::services::task_graph::{AgentStateLookup, WorktreeRegistry};
use std::path::Path;
use std::sync::Arc;

/// [`WorktreeRegistry`] and [`AgentStateLookup`] over the project manager.
pub struct ServerGraphPorts {
    manager: Arc<ProjectManager>,
}

impl ServerGraphPorts {
    pub fn new(manager: Arc<ProjectManager>) -> Self {
        Self { manager }
    }
}

impl WorktreeRegistry for ServerGraphPorts {
    /// True when `canonical` is one of the managed worktrees of the project
    /// the conversion recorded. An unknown project vouches for nothing.
    fn is_registered(&self, project_path: &str, canonical: &Path) -> bool {
        let Some(app) = self.manager.get_by_path(project_path) else {
            return false;
        };
        let worktrees = app.runtime.lock().unwrap().list_worktrees();
        worktrees
            .iter()
            .any(|w| std::fs::canonicalize(&w.path).is_ok_and(|p| p == canonical))
    }
}

impl AgentStateLookup for ServerGraphPorts {
    fn agent_state(&self, project_path: &str, branch: &str) -> Option<AgentState> {
        let app = self.manager.get_by_path(project_path)?;
        let w = app.runtime.lock().unwrap().get_worktree_by_branch(branch)?;
        let feedback_state = serde_json::to_value(w.agent.feedback_state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "none".to_string());
        Some(AgentState {
            status: common::services::snapshot::lifecycle_status(w.agent.lifecycle),
            feedback_state,
            agent: w.agent_name,
        })
    }
}
