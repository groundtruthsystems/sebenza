//! Per-agent conversation routing.
//!
//! Resolves which agent's conversation adapter serves a worktree. The interrupt and
//! agents-streaming paths in `sebenza-server` previously called
//! `claude_conversation_service` unconditionally, so for a Codex worktree they resolved a
//! `claude-pending:<path>` id that never matches the id the run was registered under —
//! making interrupt and live streaming silently inoperable for Codex.
//!
//! Centralising the dispatch here keeps every caller honest. The string match is the
//! interim shape; it becomes a `BuiltinAgentId` enum match when the agent registry gains
//! one, at which point adding an agent is a compile error rather than a silent fallthrough.

use crate::domain::model::WorktreeSnapshot;
use crate::services::agents_ui::AgentsUiConversationResponse;

/// Read the conversation for `worktree` using the adapter matching its own agent.
/// `None` when the worktree has no agent, or its agent has no conversation adapter.
pub fn read_worktree_conversation(
    worktree: &WorktreeSnapshot,
) -> Option<AgentsUiConversationResponse> {
    match worktree.agent_name.as_deref() {
        Some("claude") => {
            Some(crate::services::claude_conversation_service::read_worktree_conversation(worktree))
        }
        Some("codex") => {
            Some(crate::services::codex_conversation_service::read_worktree_conversation(worktree))
        }
        // opencode cannot discover its own session; it uses the id the agent reported at
        // creation, carried on the snapshot as `reported_session_id`.
        Some("opencode") => Some(
            crate::services::opencode_conversation_service::read_worktree_conversation(
                worktree,
                worktree.reported_session_id.as_deref(),
            ),
        ),
        _ => None,
    }
}

/// Ids of the agents that have a conversation adapter, for building accurate
/// "not supported for this agent" messages instead of hardcoding a list in each caller.
pub fn conversation_capable_agent_ids() -> &'static [&'static str] {
    &["claude", "codex", "opencode"]
}

/// The conversation id for `worktree`, resolved through its own agent's adapter.
pub fn resolve_conversation_id(worktree: &WorktreeSnapshot) -> Option<String> {
    read_worktree_conversation(worktree).map(|r| r.conversation.conversation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{WorktreeSnapshot, WorktreeSource};

    /// A worktree snapshot with no sessions on disk, so each adapter yields its
    /// `<agent>-pending:<path>` sentinel — enough to prove which adapter ran.
    fn snapshot(agent: Option<&str>) -> WorktreeSnapshot {
        WorktreeSnapshot {
            kind: crate::domain::model::WorktreeKind::Linked,
            branch: "feature".to_string(),
            label: None,
            base_branch: None,
            path: "/wt".to_string(),
            dir: "/wt".to_string(),
            archived: false,
            profile: None,
            agent_name: agent.map(str::to_string),
            agent_label: None,
            agent_terminal_stale: false,
            mux: true,
            dirty: false,
            unpushed: false,
            pane_count: 1,
            status: "idle".to_string(),
            feedback_state: crate::domain::model::AgentFeedbackState::None,
            elapsed: String::new(),
            services: Vec::new(),
            prs: Vec::new(),
            creation: None,
            source: WorktreeSource::Ui,
            oneshot: None,
            tabs: Vec::new(),
            active_tab_id: None,
            reported_session_id: None,
        }
    }

    /// Route by `provider`, not by the conversation id: `claude_cli::latest_session`
    /// falls back to the newest session across ALL project dirs when the cwd has none,
    /// so a claude id is not deterministic in a test environment. `provider` is.
    #[test]
    fn codex_worktree_resolves_through_the_codex_adapter() {
        let conv = read_worktree_conversation(&snapshot(Some("codex")))
            .expect("codex worktree must resolve a conversation")
            .conversation;
        assert_eq!(
            conv.provider, "codexAppServer",
            "a Codex worktree must not be routed through the Claude adapter"
        );
        assert_eq!(conv.conversation_id, "codex-pending:/wt");
    }

    #[test]
    fn claude_worktree_resolves_through_the_claude_adapter() {
        let conv = read_worktree_conversation(&snapshot(Some("claude")))
            .expect("claude worktree must resolve a conversation")
            .conversation;
        assert_eq!(conv.provider, "claudeCode");
    }

    #[test]
    fn opencode_routes_to_its_own_adapter_and_pends_without_a_reported_id() {
        let conv = read_worktree_conversation(&snapshot(Some("opencode")))
            .expect("opencode has a conversation adapter")
            .conversation;
        assert_eq!(conv.provider, "opencode");
        // No recorded session id yet -> a pending placeholder. Critically NOT another
        // worktree's transcript, which is the failure mode claude_cli's all-projects
        // fallback exhibits.
        assert_eq!(conv.conversation_id, "opencode-pending:/wt");
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn agent_without_a_conversation_adapter_resolves_to_none() {
        assert!(resolve_conversation_id(&snapshot(Some("some-custom-agent"))).is_none());
        assert!(resolve_conversation_id(&snapshot(None)).is_none());
    }
}
