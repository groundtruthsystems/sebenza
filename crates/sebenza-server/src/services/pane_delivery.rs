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
use crate::server::{resolve_terminal_target, submit_delay_for_branch};
use crate::services::project_manager::ProjectManager;
use common::services::resolution_delivery::PaneSink;
use std::sync::Arc;

#[cfg(doc)]
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
        let app = self
            .manager
            .list()
            .into_iter()
            .find(|app| app.path == worktree.project)
            .ok_or_else(|| format!("no registered project at {}", worktree.project))?;
        // The main checkout's pane is a login shell: pasting there would run
        // the text as commands.
        if app.lifecycle().is_main_branch(&worktree.branch) {
            return Err(format!(
                "{} is the main checkout, which has no agent pane",
                worktree.branch
            ));
        }
        // Resolve first: it reconciles a stale runtime view of the session.
        let resolved = resolve_terminal_target(&app, &worktree.branch)?;
        let runtime = app
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_worktree_by_branch(&worktree.branch)
            .ok_or_else(|| format!("Worktree not found: {}", worktree.branch))?;
        if runtime.worktree_id != resolved.worktree_id || runtime.branch != worktree.branch {
            return Err(format!(
                "the pane found for {} belongs to another worktree",
                worktree.branch
            ));
        }
        if runtime.agent_name.is_none() {
            return Err(format!("{} has no agent pane", worktree.branch));
        }
        let delay = submit_delay_for_branch(&app, &worktree.branch);
        // Pane 0 is the agent's, as for the conversion's initial prompt.
        self.terminal
            .send_prompt(&resolved.attach_target, text, 0, None, delay)
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
