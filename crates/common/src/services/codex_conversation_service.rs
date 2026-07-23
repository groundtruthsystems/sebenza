//! Codex worktree conversation read path — the history-read subset of
//! `backend-legacy/src/services/worktree-conversation-service.ts`. Reads the
//! latest on-disk Codex rollout for the worktree cwd (no app-server needed for
//! history). Send/interrupt/live-streaming (which DO need the Codex app-server)
//! are NOT ported here.

use crate::adapters::codex_session_log::latest_codex_session;
use crate::domain::model::WorktreeSnapshot;
use crate::services::agents_ui::{build_worktree_summary, AgentsUiConversationResponse, ConversationState};

/// Read the latest Codex conversation for a worktree.
pub fn read_worktree_conversation(worktree: &WorktreeSnapshot) -> AgentsUiConversationResponse {
    let session = latest_codex_session(&worktree.path);
    let (conversation_id, messages) = match session {
        Some((id, messages)) => (id, messages),
        None => (format!("codex-pending:{}", worktree.path), Vec::new()),
    };

    AgentsUiConversationResponse {
        worktree: build_worktree_summary(worktree),
        conversation: ConversationState {
            provider: "codexAppServer".to_string(),
            conversation_id,
            cwd: worktree.path.clone(),
            running: false,
            active_turn_id: None,
            messages,
        },
    }
}
