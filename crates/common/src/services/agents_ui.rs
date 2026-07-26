use crate::domain::model::{PrEntry, ServiceRuntimeState, WorktreeSnapshot};
use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentsUiMessage {
    pub id: String,
    pub turn_id: String,
    pub order: usize,
    pub role: String, // "user" | "assistant"
    pub text: String,
    pub status: String, // "completed" | "inProgress" | "failed"
    pub created_at: Option<String>,
    pub kind: String, // "text" | "thinking" | "toolUse" | "toolResult"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
}

impl AgentsUiMessage {
    /// A minimal message; callers set the discriminating fields they need.
    pub fn new(id: String, turn_id: String, role: &str, kind: &str, text: String) -> Self {
        AgentsUiMessage {
            id,
            turn_id,
            order: 0,
            role: role.to_string(),
            text,
            status: "completed".to_string(),
            created_at: None,
            kind: kind.to_string(),
            phase: None,
            tool_name: None,
            tool_call_id: None,
            command: None,
            cwd: None,
            exit_code: None,
            duration_ms: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationState {
    pub provider: String,
    pub conversation_id: String,
    pub cwd: String,
    pub running: bool,
    pub active_turn_id: Option<String>,
    pub messages: Vec<AgentsUiMessage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSummary {
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    pub path: String,
    pub archived: bool,
    pub profile: Option<String>,
    pub agent_name: Option<String>,
    pub agent_label: Option<String>,
    pub agent_terminal_stale: bool,
    pub mux: bool,
    pub status: String,
    pub dirty: bool,
    pub unpushed: bool,
    pub services: Vec<ServiceRuntimeState>,
    pub prs: Vec<PrEntry>,
    pub creating: bool,
    pub creation_phase: Option<String>,
    pub conversation: Option<serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsUiConversationResponse {
    pub worktree: WorktreeSummary,
    pub conversation: ConversationState,
}

/// Project a worktree snapshot into the agents-UI summary shape.
pub fn build_worktree_summary(worktree: &WorktreeSnapshot) -> WorktreeSummary {
    WorktreeSummary {
        branch: worktree.branch.clone(),
        base_branch: worktree.base_branch.clone(),
        path: worktree.path.clone(),
        archived: worktree.archived,
        profile: worktree.profile.clone(),
        agent_name: worktree.agent_name.clone(),
        agent_label: worktree.agent_label.clone(),
        agent_terminal_stale: worktree.agent_terminal_stale,
        mux: worktree.mux,
        status: worktree.status.clone(),
        dirty: worktree.dirty,
        unpushed: worktree.unpushed,
        services: worktree.services.clone(),
        prs: worktree.prs.clone(),
        creating: worktree.status == "creating",
        // creationPhase (in-flight creations) + the meta conversation ref are deferred.
        creation_phase: None,
        conversation: None,
    }
}
