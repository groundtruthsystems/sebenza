use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const WORKTREE_META_SCHEMA_VERSION: i32 = 1;
pub const WORKTREE_ARCHIVE_STATE_VERSION: i32 = 1;
pub const OPEN_SESSIONS_STATE_VERSION: i32 = 1;
pub const ROOT_TAB_ID: &str = "root";

/// `WorktreeMeta::agent` value for the repository's own checkout, which runs no
/// agent.
///
/// A sentinel rather than making the field `Option<String>`: that would change the
/// on-disk shape of *every* existing `meta.json` to `"agent": null`, which an older
/// binary cannot deserialize — it would then read the worktree as unmanaged and
/// overwrite its meta. The sentinel never reaches the UI, because reconciliation
/// forces `agent_name: None` for the main entry.
pub const MAIN_REPO_AGENT_SENTINEL: &str = "none";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeConversationProvider {
    #[serde(rename = "codexAppServer")]
    CodexAppServer,
    #[serde(rename = "claudeCode")]
    ClaudeCode,
    #[serde(rename = "opencode")]
    Opencode,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "provider")]
pub enum WorktreeConversationMeta {
    #[serde(rename = "codexAppServer")]
    Codex {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        cwd: String,
        #[serde(rename = "lastSeenAt")]
        last_seen_at: String,
        #[serde(rename = "threadId")]
        thread_id: String,
    },
    #[serde(rename = "claudeCode")]
    Claude {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        cwd: String,
        #[serde(rename = "lastSeenAt")]
        last_seen_at: String,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// opencode records the id Sebenza was told at launch, because it cannot be
    /// discovered afterwards: `project_id` is per-repository and `opencode session list`
    /// has no directory column, so neither identifies this worktree's session.
    #[serde(rename = "opencode")]
    Opencode {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        cwd: String,
        #[serde(rename = "lastSeenAt")]
        last_seen_at: String,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
}

impl WorktreeConversationMeta {
    pub fn conversation_session_id(&self) -> &str {
        match self {
            Self::Codex { thread_id, .. } => thread_id,
            Self::Claude { session_id, .. } => session_id,
            Self::Opencode { session_id, .. } => session_id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeTabKind {
    Root,
    /// A continuation of the root conversation, so it is bound to the worktree's
    /// own agent and needs session-id discovery to exist.
    Fork,
    /// A plain managed shell — no agent.
    Shell,
    /// A *fresh* session of any configured agent, built-in or custom. Unlike a
    /// fork it starts a new conversation, so it carries no session lineage and
    /// can run an agent other than the worktree's own.
    Agent,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeTab {
    pub tab_id: String,
    pub kind: WorktreeTabKind,
    pub label: String,
    pub seq: Option<i32>,
    pub session_id: Option<String>,
    // `paneId` is `.optional()` in the contract (omitted when absent), not nullable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    /// The agent whose session runs in this tab's pane. `None` on shell tabs and
    /// on tabs written before per-tab agents existed; readers must fall back to
    /// `WorktreeMeta::agent` via `tab_logic::tab_agent_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeSource {
    Ui,
    Oneshot,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OneshotMeta {
    pub auto_close_on_done: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeMeta {
    pub schema_version: i32,
    pub worktree_id: String,
    pub branch: String,
    pub label: Option<String>,
    pub base_branch: Option<String>,
    pub created_at: String,
    pub profile: String,
    pub agent: String,
    pub runtime: String, // "host" | "docker"
    pub startup_env_values: HashMap<String, String>,
    pub allocated_ports: HashMap<String, u16>,
    pub source: Option<WorktreeSource>,
    pub oneshot: Option<OneshotMeta>,
    pub conversation: Option<WorktreeConversationMeta>,
    pub agent_terminal_stale: Option<bool>,
    pub tabs: Option<Vec<WorktreeTab>>,
    pub active_tab_id: Option<String>,
    pub fork_counter: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedWorktreeEntry {
    pub path: String,
    pub archived_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeArchiveState {
    pub schema_version: i32,
    pub entries: Vec<ArchivedWorktreeEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenSessionsState {
    pub schema_version: i32,
    pub saved_at: String,
    pub branches: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStoragePaths {
    pub git_dir: String,
    pub sebenza_dir: String,
    pub meta_path: String,
    pub runtime_env_path: String,
    pub control_env_path: String,
    pub prs_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycle {
    Closed,
    Starting,
    Running,
    Idle,
    /// Blocked mid-turn on a permission decision the agent is showing in its own UI.
    ///
    /// Distinct from `Idle` on purpose: both mean "this worktree wants you", but they call
    /// for different actions — `Idle` wants your next prompt, this wants you to approve or
    /// reject something already proposed. With many parallel worktrees, that difference is
    /// the difference between a legible queue and a wall of identical "waiting" badges.
    ///
    /// Sebenza cannot answer the prompt for you: opencode's `permission.ask` hook does not
    /// fire (verified on 1.18.9), so this state is observational only.
    AwaitingPermission,
    Stopped,
    Error,
}

/// Whether a worktree is waiting on an explicit human response, independent of
/// [`AgentLifecycle`].
///
/// Provider-neutral on purpose. It is not merely a restatement of
/// `AwaitingPermission`: an agent that asks a free-text question is still `Running`,
/// so the lifecycle alone cannot express "this one needs a human". Keeping the answer
/// in one field means the dashboard reads a single signal rather than unioning two
/// orthogonal concepts.
///
/// Set only from observable runtime events. Never inferred from idleness, CI/PR state,
/// errors, or unread notifications — a false "needs you" badge is worse than none.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentFeedbackState {
    /// Nothing is waiting on the user.
    #[default]
    None,
    /// Blocked on a permission decision the agent is showing in its own UI.
    PermissionRequest,
    /// The agent asked a free-text question awaiting a human answer.
    ///
    /// Reserved. No built-in adapter can observe such an event today, so nothing sets
    /// this yet; consumers must still render it correctly if it ever appears.
    UserQuestion,
}

impl AgentFeedbackState {
    /// The stable wire/log spelling, matching what serde emits.
    ///
    /// Shared by the snapshot mapping and the transition log so the two can never
    /// disagree about what a state is called.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentFeedbackState::None => "none",
            AgentFeedbackState::PermissionRequest => "permission_request",
            AgentFeedbackState::UserQuestion => "user_question",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeRuntimeState {
    pub exists: bool,
    pub branch: String,
    pub dirty: bool,
    pub ahead_count: i32,
    pub current_commit: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeState {
    pub exists: bool,
    pub session_name: Option<String>,
    pub window_name: String,
    pub pane_count: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeState {
    pub runtime: String, // "host" | "docker"
    pub lifecycle: AgentLifecycle,
    /// Written in the same `apply_event` arm as `lifecycle`, never from a separate path,
    /// so the two cannot drift apart.
    #[serde(default)]
    pub feedback_state: AgentFeedbackState,
    pub last_started_at: Option<String>,
    pub last_event_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRuntimeState {
    pub name: String,
    pub port: Option<u16>,
    pub running: bool,
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrComment {
    pub r#type: String, // "comment" | "inline"
    pub author: String,
    pub body: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub line: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_hunk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_reply: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CiCheck {
    pub name: String,
    pub status: String, // "pending" | "success" | "failed" | "skipped"
    pub url: Option<String>,
    pub run_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrEntry {
    pub repo: String,
    pub number: i32,
    pub state: String, // "open" | "closed" | "merged"
    pub url: String,
    pub updated_at: String,
    pub ci_status: String, // "none" | "pending" | "success" | "failed"
    pub ci_checks: Vec<CiCheck>,
    pub comments: Vec<PrComment>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCreationPhase {
    CreatingWorktree,
    PreparingRuntime,
    RunningPostCreateHook,
    StartingSession,
    Reconciling,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreatingWorktreeState {
    pub branch: String,
    pub base_branch: Option<String>,
    pub path: String,
    pub profile: Option<String>,
    pub agent_name: Option<String>,
    pub phase: WorktreeCreationPhase,
    pub source: WorktreeSource,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreationSnapshot {
    pub phase: WorktreeCreationPhase,
}

/// Whether an entry is the repository's own checkout or a linked worktree.
///
/// The main checkout is a real, openable session but is not a worktree anyone may
/// merge, archive or remove — this discriminator is what the guards key off.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeKind {
    Main,
    #[default]
    Linked,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ManagedWorktreeRuntimeState {
    pub worktree_id: String,
    pub kind: WorktreeKind,
    pub branch: String,
    pub label: Option<String>,
    pub base_branch: Option<String>,
    pub path: String,
    pub profile: Option<String>,
    pub agent_name: Option<String>,
    pub source: WorktreeSource,
    pub oneshot: Option<OneshotMeta>,
    pub agent_terminal_stale: bool,
    pub tabs: Vec<WorktreeTab>,
    pub active_tab_id: Option<String>,
    pub git: GitWorktreeRuntimeState,
    pub session: SessionRuntimeState,
    pub agent: AgentRuntimeState,
    pub services: Vec<ServiceRuntimeState>,
    pub prs: Vec<PrEntry>,
    /// Session id reported by the agent at creation, for agents whose id cannot be
    /// discovered from disk afterwards (opencode). `None` for claude/codex, whose ids are
    /// recovered by scanning their session logs.
    pub reported_session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshot {
    pub branch: String,
    pub kind: WorktreeKind,
    pub label: Option<String>,
    // `baseBranch` is `.optional()` in the contract (omitted when absent), not nullable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    pub path: String,
    pub dir: String,
    pub archived: bool,
    pub profile: Option<String>,
    pub agent_name: Option<String>,
    pub agent_label: Option<String>,
    pub agent_terminal_stale: bool,
    pub mux: bool,
    pub dirty: bool,
    pub unpushed: bool,
    pub pane_count: i32,
    pub status: String,
    pub elapsed: String,
    pub services: Vec<ServiceRuntimeState>,
    pub prs: Vec<PrEntry>,
    pub creation: Option<WorktreeCreationSnapshot>,
    pub source: WorktreeSource,
    pub oneshot: Option<OneshotMeta>,
    pub tabs: Vec<WorktreeTab>,
    pub active_tab_id: Option<String>,
    /// Session id the agent reported at creation, for agents whose id cannot be recovered
    /// from disk (opencode). Not serialized to the frontend: it is an internal correlation
    /// key, and adding it to the wire contract would expand the surface for no UI benefit.
    #[serde(skip)]
    pub reported_session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub project: ProjectInfo,
    pub worktrees: Vec<WorktreeSnapshot>,
    pub notifications: Vec<NotificationView>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub name: String,
    pub main_branch: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotificationView {
    pub id: i64,
    pub branch: String,
    pub r#type: String, // "agent_stopped" | "pr_opened" | "runtime_error" | "worktree_auto_removed"
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub timestamp: i64,
}
