use crate::adapters::claude_cli::{ClaudeCliSession, latest_session};
use crate::domain::model::WorktreeSnapshot;
use crate::services::agents_ui::{
    AgentsUiConversationResponse, AgentsUiMessage, ConversationState, build_worktree_summary,
};

fn build_state(
    worktree: &WorktreeSnapshot,
    session: Option<&ClaudeCliSession>,
) -> ConversationState {
    let conversation_id = session
        .map(|s| s.session_id.clone())
        .unwrap_or_else(|| format!("claude-pending:{}", worktree.path));
    let messages = session
        .map(|s| {
            s.messages
                .iter()
                .enumerate()
                .map(|(order, m)| {
                    let mut msg = AgentsUiMessage::new(
                        m.id.clone(),
                        m.turn_id.clone(),
                        &m.role,
                        &m.kind,
                        m.text.clone(),
                    );
                    msg.order = order;
                    msg.created_at = m.created_at.clone();
                    msg.tool_name = m.tool_name.clone();
                    msg.tool_call_id = m.tool_call_id.clone();
                    msg
                })
                .collect()
        })
        .unwrap_or_default();

    ConversationState {
        provider: "claudeCode".to_string(),
        conversation_id,
        cwd: worktree.path.clone(),
        running: false,
        active_turn_id: None,
        messages,
    }
}

/// Read the latest Claude conversation for a worktree.
pub fn read_worktree_conversation(worktree: &WorktreeSnapshot) -> AgentsUiConversationResponse {
    let session = latest_session(&worktree.path);
    AgentsUiConversationResponse {
        worktree: build_worktree_summary(worktree),
        conversation: build_state(worktree, session.as_ref()),
    }
}
