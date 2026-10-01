//! The tmux [`PaneSink`]: how a confirmed resolution reaches its origin
//! worktree's agent pane (FR-27, T-11).
//!
//! It is handed to [`InboxService::set_pane_sink`] at startup and called only
//! through `ResolutionDelivery`, which `InboxService` reaches only from
//! confirm and redeliver. It resolves the pane from the request's own
//! worktree key (project path + branch) rather than from anything the caller
//! sends, and refuses a main checkout or a worktree with no agent.

use crate::adapters::terminal::TerminalManager;
use crate::domain::inbox_events::WorktreeKey;
use crate::services::project_manager::ProjectManager;
use common::services::resolution_delivery::PaneSink;
use std::sync::Arc;

#[allow(unused_imports)]
use crate::services::inbox_service::InboxService;

/// Pastes into the agent pane of a registered project's worktree.
pub struct TmuxPaneSink {
    manager: Arc<ProjectManager>,
    terminal: Arc<TerminalManager>,
}

impl TmuxPaneSink {
    pub fn new(manager: Arc<ProjectManager>, terminal: Arc<TerminalManager>) -> Self {
        Self { manager, terminal }
    }
}

impl PaneSink for TmuxPaneSink {
    fn send(&self, worktree: &WorktreeKey, text: &str) -> Result<(), String> {
        let _ = (&self.manager, &self.terminal, worktree, text);
        todo!("phase-4-task-4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::projects_registry::ProjectsRegistry;

    fn sink() -> TmuxPaneSink {
        let base = std::env::temp_dir().join(format!("sebenza-pane-sink-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        TmuxPaneSink::new(
            Arc::new(ProjectManager::new(
                ProjectsRegistry::with_file(base.join("projects.json")),
                "http://127.0.0.1:5111".into(),
            )),
            Arc::new(TerminalManager::new(0)),
        )
    }

    // TS-27 / T-11: a worktree that is not a registered project's has no
    // pane to paste into, so nothing is sent.
    #[test]
    fn an_unregistered_project_is_refused() {
        let err = sink()
            .send(
                &WorktreeKey {
                    project: "/nowhere/acme-demo".into(),
                    branch: "feat-x".into(),
                },
                "text",
            )
            .expect_err("no pane");
        assert!(err.contains("no registered project"), "{err}");
    }
}
