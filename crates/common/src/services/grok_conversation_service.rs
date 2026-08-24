//! Read the current grok conversation for a worktree.
//!
//! Unlike opencode this needs no subprocess: grok's transcript is plain JSONL on disk at
//! `<grok-home>/sessions/<encoded-cwd>/<session-id>/updates.jsonl`, so it is read directly,
//! the way the claude and codex adapters read theirs.
//!
//! The session id comes from Sebenza's own record of what it launched - the uuid pinned with
//! `-s` at launch, cross-checked by the id grok reports from its `SessionStart` hook. It is
//! never guessed from "the newest session", because that is exactly how
//! `claude_cli::latest_session`'s all-projects fallback can surface another worktree's
//! conversation as this one's.

use crate::adapters::grok_session_log;
use crate::domain::model::WorktreeSnapshot;
use crate::services::agents_ui::{
    AgentsUiConversationResponse, ConversationState, build_worktree_summary,
};

/// Read the conversation for `worktree`, given the session id Sebenza recorded at launch.
///
/// Returns a `pending` placeholder when there is no recorded session yet, when the
/// transcript cannot be read, or when the session belongs to a different directory - so a
/// mis-correlated id is reported as "no conversation" rather than as the wrong transcript.
pub fn read_worktree_conversation(
    worktree: &WorktreeSnapshot,
    session_id: Option<&str>,
) -> AgentsUiConversationResponse {
    let pending = || ConversationState {
        provider: "grok".to_string(),
        conversation_id: format!("grok-pending:{}", worktree.path),
        cwd: worktree.path.clone(),
        running: false,
        active_turn_id: None,
        messages: Vec::new(),
    };

    let conversation = session_id
        .filter(|id| {
            // Integrity check: the session's own group directory must be this worktree.
            // `None` means the cwd could not be determined, which is not evidence of a
            // mismatch, so it is allowed through - the parser still filters every line on
            // `params.sessionId`.
            grok_session_log::session_cwd(id).is_none_or(|cwd| cwd == worktree.path)
        })
        .and_then(grok_session_log::read_session)
        .map(|session| ConversationState {
            provider: "grok".to_string(),
            conversation_id: session.id,
            cwd: worktree.path.clone(),
            running: false,
            active_turn_id: None,
            messages: session.messages,
        })
        .unwrap_or_else(pending);

    AgentsUiConversationResponse {
        worktree: build_worktree_summary(worktree),
        conversation,
    }
}
