use crate::adapters::fs::{read_worktree_archive_state, write_worktree_archive_state};
use crate::adapters::git::GitGateway;
use crate::adapters::git::checked_out_branch_names;
use crate::adapters::terminal::{TerminalAttachTarget, TerminalEvent, TerminalManager};
use crate::config::project_root;
use crate::config::{
    persist_local_custom_agent, persist_local_github_config, remove_local_custom_agent,
};
use crate::domain::config::CustomAgentConfig;
use crate::domain::model::{OneshotMeta, WorktreeSnapshot, WorktreeSource};
use crate::domain::policies::{available_branch_names, base_branch_names, is_valid_branch_name};
use crate::services::agent_registry::{
    AgentImplementation, get_agent_definition, get_agent_details, is_builtin_agent_id,
    list_agent_details, normalize_custom_agent_id, validate_custom_agent_input,
};
use crate::services::archive_service::{
    build_archived_worktree_path_set, prune_archived_worktree_state,
};
use crate::services::auto_pull_service::{
    PullMainResult, force_pull_main_branch, pull_main_branch,
};
use crate::services::config_view::{AppConfig, build_app_config};
use crate::services::lifecycle_service::{CreateMode, CreateWorktreesInput, LifecycleError};
use crate::services::project_manager::{ProjectApp, ProjectManager};
use crate::services::snapshot::build_project_snapshot;
use crate::services::worktree_service::build_create_worktree_targets;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tower_http::services::{ServeDir, ServeFile};

/// Global server state. Per-project state lives in `ProjectApp` (resolved by URL
/// prefix via `project()`); the terminal manager and SPA assets are server-wide.
#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<ProjectManager>,
    pub terminal: Arc<TerminalManager>,
    pub agent_stream: Arc<crate::services::agent_stream::AgentStreamManager>,
    pub project_inits: Arc<crate::services::project_init_service::ProjectInitTracker>,
    pub frontend_dist: Option<PathBuf>,
}

impl AppState {
    /// Resolve the project served under `prefix`, or 404. Records activity so the
    /// PR monitor knows the project is being viewed.
    fn project(&self, prefix: &str) -> Result<Arc<ProjectApp>, ApiError> {
        let app = self
            .manager
            .get(prefix)
            .ok_or_else(|| ApiError::new(404, "Project not found".to_string()))?;
        app.touch();
        Ok(app)
    }
}

/// Spawn the background loops: PR/CI sync for viewed projects (10s) and
/// per-project auto-pull of the main branch (config interval). Both iterate the
/// live project set each tick, so newly-added projects are picked up.
pub fn spawn_background_loops(state: AppState) {
    // PR/CI monitor — only syncs projects a client is currently viewing.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                tick.tick().await;
                for app in state.manager.list() {
                    if !app.active_recently(std::time::Duration::from_secs(60)) {
                        continue;
                    }
                    let app = app.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let linked = app.config().integrations.github.linked_repos.clone();
                        crate::services::pr_service::sync_pr_status(&app.git, &app.path, &linked);
                    })
                    .await;
                }
            }
        });
    }

    // Session-snapshot monitor — persists each project's open sessions (30s) so
    // `sebenza-cli restore` can re-open them after a restart.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tick.tick().await;
                for app in state.manager.list() {
                    let app = app.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let saved_at =
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        crate::services::session_restore_service::save_open_sessions_snapshot(
                            &app.git, &app.tmux, &app.path, saved_at,
                        );
                    })
                    .await;
                }
            }
        });
    }

    // Oneshot watcher — fires end-of-run actions for armed oneshot worktrees (3s).
    // Per-project idle-timer state is kept across ticks in this task's scope.
    {
        let state = state.clone();
        tokio::spawn(async move {
            use crate::services::oneshot_watcher_service::{POLL_INTERVAL_MS, WatchStates};
            use std::collections::HashMap;
            use std::sync::{Arc, Mutex};
            let mut per_project: HashMap<String, Arc<Mutex<WatchStates>>> = HashMap::new();
            let mut tick =
                tokio::time::interval(std::time::Duration::from_millis(POLL_INTERVAL_MS));
            loop {
                tick.tick().await;
                for app in state.manager.list() {
                    let states = per_project
                        .entry(app.prefix.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(WatchStates::default())))
                        .clone();
                    let app = app.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        crate::services::oneshot_watcher_service::run_oneshot_watch(
                            &states,
                            &app.runtime,
                            &app.lifecycle(),
                            now_ms,
                        );
                    })
                    .await;
                }
            }
        });
    }

    // Auto-pull monitor — fast-forwards each enabled project's main branch.
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            for app in state.manager.list() {
                let config = app.config();
                let ap = &config.workspace.auto_pull;
                if !ap.enabled {
                    continue;
                }
                let interval = std::time::Duration::from_secs(ap.interval_seconds.max(30));
                if !app.pull_due(interval) {
                    continue;
                }
                app.mark_pulled();
                let app = app.clone();
                let main_branch = config.workspace.main_branch.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let result = crate::services::auto_pull_service::pull_main_branch(
                        &app.git,
                        &app.path,
                        &main_branch,
                    );
                    if result.status == "updated" {
                        tracing::info!("[auto-pull] updated {} on {}", main_branch, app.prefix);
                    }
                })
                .await;
            }
        }
    });
}

/// A JSON error body `{ "error": "..." }` with an HTTP status (mirrors
/// `ErrorResponseSchema`).
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: u16, message: String) -> Self {
        ApiError {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            message,
        }
    }
}

impl From<LifecycleError> for ApiError {
    fn from(err: LifecycleError) -> Self {
        ApiError::new(err.status, err.message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

/// Guard that marks one or more branches busy for the lifetime of a mutating
/// request and clears them on drop. Returns 409 if any is already busy.
struct BusyGuard {
    busy: Arc<Mutex<HashSet<String>>>,
    branches: Vec<String>,
}

impl BusyGuard {
    fn acquire(app: &ProjectApp, branch: &str) -> Result<BusyGuard, ApiError> {
        Self::acquire_many(app, &[branch])
    }

    /// Acquire several branches atomically. Names are sorted and deduped first so
    /// two concurrent multi-branch requests can never deadlock by taking them in
    /// opposite orders; on partial failure nothing is left marked busy.
    fn acquire_many(app: &ProjectApp, branches: &[&str]) -> Result<BusyGuard, ApiError> {
        let mut wanted: Vec<String> = branches.iter().map(|b| b.to_string()).collect();
        wanted.sort();
        wanted.dedup();

        let mut set = app.busy.lock().unwrap();
        if let Some(taken) = wanted.iter().find(|branch| set.contains(*branch)) {
            return Err(ApiError::new(
                409,
                format!("Worktree {taken} is busy with another operation"),
            ));
        }
        for branch in &wanted {
            set.insert(branch.clone());
        }
        Ok(BusyGuard {
            busy: app.busy.clone(),
            branches: wanted,
        })
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let mut set = self.busy.lock().unwrap();
        for branch in &self.branches {
            set.remove(branch);
        }
    }
}

#[derive(Serialize)]
struct AvailableBranch {
    name: String,
}

#[derive(Serialize)]
struct BranchListResponse {
    branches: Vec<AvailableBranch>,
}

#[derive(Serialize)]
struct WorktreeListResponse {
    worktrees: Vec<WorktreeSnapshot>,
}

#[derive(Deserialize)]
struct BranchQuery {
    #[serde(rename = "includeRemote")]
    include_remote: Option<String>,
}

pub fn build_router(state: AppState) -> Router {
    let frontend_dist = state.frontend_dist.clone();

    let router = Router::new()
        // Hub routes (no project prefix).
        .route("/api/projects", get(list_projects).post(add_project))
        .route("/api/projects/init", get(list_project_inits))
        .route("/api/projects/migrate", post(migrate_projects))
        .route("/api/projects/{prefix}", delete(remove_project))
        .route("/api/instances", get(fetch_instances))
        .route("/api/active-worktrees", get(get_active_worktrees))
        .route("/api/registry", get(fetch_registry))
        .route("/api/registry/file", get(fetch_registry_file))
        .route("/api/runtime/events", post(runtime_event))
        // Per-project routes, scoped under `/<prefix>`.
        .route("/{prefix}/api/config", get(get_config))
        .route("/{prefix}/api/branches", get(get_branches))
        .route("/{prefix}/api/base-branches", get(get_base_branches))
        .route("/{prefix}/api/project", get(get_project))
        .route(
            "/{prefix}/api/project/auto-name",
            get(fetch_auto_name_config),
        )
        .route(
            "/{prefix}/api/worktrees",
            get(get_worktrees).post(create_worktree),
        )
        .route("/{prefix}/api/worktrees/{name}", delete(remove_worktree))
        .route("/{prefix}/api/worktrees/{name}/merge", post(merge_worktree))
        .route(
            "/{prefix}/api/worktrees/{name}/label",
            put(set_worktree_label),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/archive",
            put(set_worktree_archived),
        )
        .route("/{prefix}/api/worktrees/{name}/close", post(close_worktree))
        .route("/{prefix}/api/worktrees/{name}/open", post(open_worktree))
        .route(
            "/{prefix}/api/worktrees/{name}/launch",
            post(launch_worktree),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/send",
            post(send_worktree_prompt),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/diff",
            get(fetch_worktree_diff),
        )
        .route("/{prefix}/api/worktrees/{name}/tracks", get(fetch_tracks))
        .route(
            "/{prefix}/api/worktrees/{name}/track-file",
            get(fetch_track_file),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/tabs",
            post(create_worktree_tab),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/shell",
            post(create_worktree_shell_tab),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/agent-tabs",
            post(create_worktree_agent_tab),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/tabs/{tabId}/select",
            post(select_worktree_tab),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/tabs/{tabId}",
            delete(delete_worktree_tab),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/agent-terminal/refresh",
            post(refresh_agent_terminal),
        )
        .route(
            "/{prefix}/api/worktrees/{name}/sync-prs",
            post(sync_worktree_prs),
        )
        .route("/{prefix}/api/pull-main", post(pull_main))
        .route("/{prefix}/api/ci-logs/{runId}", get(fetch_ci_logs))
        .route(
            "/{prefix}/api/agents/worktrees/{name}/attach",
            post(agents_attach),
        )
        .route(
            "/{prefix}/api/agents/worktrees/{name}/history",
            get(agents_history),
        )
        .route(
            "/{prefix}/api/agents/worktrees/{name}/messages",
            post(agents_send),
        )
        .route(
            "/{prefix}/api/agents/worktrees/{name}/interrupt",
            post(agents_interrupt),
        )
        .route(
            "/{prefix}/ws/agents/worktrees/{name}",
            get(ws_agents_stream),
        )
        .route(
            "/{prefix}/api/notifications/{id}/dismiss",
            post(dismiss_notification),
        )
        .route("/{prefix}/api/agents", get(list_agents).post(create_agent))
        .route("/{prefix}/api/agents/validate", post(validate_agent))
        .route(
            "/{prefix}/api/agents/{id}",
            put(update_agent).delete(delete_agent),
        )
        .route(
            "/{prefix}/api/github/auto-remove-on-merge",
            put(set_auto_remove_on_merge),
        )
        .route("/{prefix}/ws/{worktree}", get(ws_terminal))
        .with_state(state);

    // SPA static serving, falling back to index.html for client-side routes.
    // Default: the frontend bundle embedded in the binary. A `SEBENZA_FRONTEND_DIST`
    // override (via `frontend_dist`) serves from disk instead — handy for iterating
    // on a freshly-built dist without recompiling the server.
    match frontend_dist {
        Some(dist) => {
            let index = dist.join("index.html");
            let serve = ServeDir::new(dist).not_found_service(ServeFile::new(index));
            router.fallback_service(serve)
        }
        None => router.fallback(serve_embedded_frontend),
    }
}

/// The React SPA (`frontend/dist`) embedded into the binary at build time.
#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
struct FrontendAssets;

/// Serve an embedded frontend asset, falling back to `index.html` (200) for any
/// unmatched path so client-side routes work.
async fn serve_embedded_frontend(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(asset) = FrontendAssets::get(path) {
        let mime = asset.metadata.mimetype().to_string();
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, cache.to_string()),
            ],
            asset.data,
        )
            .into_response();
    }
    match FrontendAssets::get("index.html") {
        Some(index) => (
            [
                (header::CONTENT_TYPE, "text/html".to_string()),
                (header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            index.data,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "frontend not built").into_response(),
    }
}

async fn get_config(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<AppConfig>, ApiError> {
    let app = state.project(&prefix)?;
    Ok(Json(build_app_config(&app.config(), &app.path)))
}

async fn fetch_auto_name_config(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    Ok(Json(
        serde_json::json!({ "autoName": app.config().auto_name }),
    ))
}

// --- Agents CRUD + GitHub settings ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertAgentBody {
    label: String,
    start_command: String,
    resume_command: Option<String>,
}

impl UpsertAgentBody {
    /// Validate/trim into a `CustomAgentConfig` (label + start command required).
    fn into_config(self) -> Result<CustomAgentConfig, ApiError> {
        let label = self.label.trim().to_string();
        let start_command = self.start_command.trim().to_string();
        if label.is_empty() || start_command.is_empty() {
            return Err(ApiError::new(
                400,
                "label and startCommand are required".to_string(),
            ));
        }
        let resume_command = self
            .resume_command
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty());
        Ok(CustomAgentConfig {
            label,
            start_command,
            resume_command,
        })
    }
}

async fn list_agents(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    Ok(Json(
        serde_json::json!({ "agents": list_agent_details(&app.config()) }),
    ))
}

async fn validate_agent(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
    Json(body): Json<UpsertAgentBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.project(&prefix)?;
    let result = validate_custom_agent_input(
        &body.label,
        &body.start_command,
        body.resume_command.as_deref(),
    );
    Ok(Json(serde_json::json!(result)))
}

/// Persist a custom agent to the local overlay and swap it into the live config.
fn upsert_agent(
    app: &ProjectApp,
    agent_id: &str,
    agent: CustomAgentConfig,
) -> Result<(), ApiError> {
    persist_local_custom_agent(&app.path, agent_id, &agent)
        .map_err(|e| ApiError::new(500, e.to_string()))?;
    let mut config = (*app.config()).clone();
    config.agents.insert(agent_id.to_string(), agent);
    app.set_config(config);
    Ok(())
}

async fn create_agent(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
    Json(body): Json<UpsertAgentBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let agent = body.into_config()?;
    let agent_id = normalize_custom_agent_id(&agent.label);
    // `none` is the sentinel the main checkout's meta uses for "no agent"; a real
    // agent with that id would collide with it.
    if agent_id == crate::domain::model::MAIN_REPO_AGENT_SENTINEL {
        return Err(ApiError::new(
            400,
            format!("\"{agent_id}\" is a reserved agent id"),
        ));
    }
    if is_builtin_agent_id(&agent_id) || app.config().agents.contains_key(&agent_id) {
        return Err(ApiError::new(
            409,
            format!("Agent already exists: {agent_id}"),
        ));
    }
    upsert_agent(&app, &agent_id, agent)?;
    let details = get_agent_details(&app.config(), &agent_id).ok_or_else(|| {
        ApiError::new(
            500,
            format!("Created agent could not be loaded: {agent_id}"),
        )
    })?;
    Ok(Json(serde_json::json!({ "agent": details })))
}

async fn update_agent(
    State(state): State<AppState>,
    Path((prefix, agent_id)): Path<(String, String)>,
    Json(body): Json<UpsertAgentBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    if is_builtin_agent_id(&agent_id) {
        return Err(ApiError::new(
            400,
            format!("Built-in agent cannot be edited: {agent_id}"),
        ));
    }
    if !app.config().agents.contains_key(&agent_id) {
        return Err(ApiError::new(404, format!("Unknown agent: {agent_id}")));
    }
    let agent = body.into_config()?;
    upsert_agent(&app, &agent_id, agent)?;
    let details = get_agent_details(&app.config(), &agent_id).ok_or_else(|| {
        ApiError::new(
            500,
            format!("Updated agent could not be loaded: {agent_id}"),
        )
    })?;
    Ok(Json(serde_json::json!({ "agent": details })))
}

async fn delete_agent(
    State(state): State<AppState>,
    Path((prefix, agent_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    if is_builtin_agent_id(&agent_id) {
        return Err(ApiError::new(
            400,
            format!("Built-in agent cannot be deleted: {agent_id}"),
        ));
    }
    if !app.config().agents.contains_key(&agent_id) {
        return Err(ApiError::new(404, format!("Unknown agent: {agent_id}")));
    }
    remove_local_custom_agent(&app.path, &agent_id)
        .map_err(|e| ApiError::new(500, e.to_string()))?;
    let mut config = (*app.config()).clone();
    config.agents.remove(&agent_id);
    app.set_config(config);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ToggleEnabledBody {
    enabled: bool,
}

async fn set_auto_remove_on_merge(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
    Json(body): Json<ToggleEnabledBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    persist_local_github_config(&app.path, Some(body.enabled))
        .map_err(|e| ApiError::new(500, e.to_string()))?;
    let mut config = (*app.config()).clone();
    config.integrations.github.auto_remove_on_merge = body.enabled;
    app.set_config(config);
    Ok(Json(
        serde_json::json!({ "ok": true, "enabled": body.enabled }),
    ))
}

/// The set of branches checked out in any (non-bare) worktree — including stale
/// registrations, so `listWorktrees` (not live) is used, matching the TS backend.
fn checked_out_branches(app: &ProjectApp) -> BTreeSet<String> {
    checked_out_branch_names(&app.git.list_worktrees(&app.path))
}

async fn get_branches(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
    Query(query): Query<BranchQuery>,
) -> Result<Json<BranchListResponse>, ApiError> {
    let app = state.project(&prefix)?;
    let include_remote = query.include_remote.as_deref() == Some("true");
    let branches = available_branch_names(
        &app.git.list_local_branches(&app.path),
        &app.git.list_remote_branches(&app.path),
        &checked_out_branches(&app),
        include_remote,
    )
    .into_iter()
    .map(|name| AvailableBranch { name })
    .collect();

    Ok(Json(BranchListResponse { branches }))
}

async fn get_base_branches(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<BranchListResponse>, ApiError> {
    let app = state.project(&prefix)?;
    let branches = base_branch_names(&app.git.list_local_branches(&app.path))
        .into_iter()
        .map(|name| AvailableBranch { name })
        .collect();

    Ok(Json(BranchListResponse { branches }))
}

/// Reconcile (throttled) then build the project snapshot from in-memory state.
fn reconcile_and_snapshot(app: &ProjectApp) -> crate::domain::model::ProjectSnapshot {
    app.reconciliation.reconcile(&app.path, &app.runtime, false);
    let worktrees = app.runtime.lock().unwrap().list_worktrees();
    let archived = current_archived_paths(app, &worktrees);
    build_project_snapshot(
        &app.config(),
        &worktrees,
        Utc::now(),
        &archived,
        app.notifications.list(),
    )
}

/// Read the project archive state, prune entries whose worktree no longer exists
/// (persisting the pruned state), and return the normalized archived path set.
fn current_archived_paths(
    app: &ProjectApp,
    worktrees: &[crate::domain::model::ManagedWorktreeRuntimeState],
) -> HashSet<String> {
    let Ok(project_git_dir) = app.git.resolve_worktree_git_dir(&app.path) else {
        return HashSet::new();
    };
    let existing = read_worktree_archive_state(&project_git_dir);
    let live_paths: Vec<String> = worktrees.iter().map(|w| w.path.clone()).collect();
    let pruned = prune_archived_worktree_state(&existing, &live_paths);
    if pruned.entries.len() != existing.entries.len() {
        let _ = write_worktree_archive_state(&project_git_dir, &pruned);
    }
    build_archived_worktree_path_set(&pruned)
}

async fn get_project(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<crate::domain::model::ProjectSnapshot>, ApiError> {
    let app = state.project(&prefix)?;
    let snapshot = tokio::task::spawn_blocking(move || reconcile_and_snapshot(&app))
        .await
        .expect("reconcile task panicked");
    Ok(Json(snapshot))
}

async fn get_worktrees(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<WorktreeListResponse>, ApiError> {
    let app = state.project(&prefix)?;
    let snapshot = tokio::task::spawn_blocking(move || reconcile_and_snapshot(&app))
        .await
        .expect("reconcile task panicked");
    Ok(Json(WorktreeListResponse {
        worktrees: snapshot.worktrees,
    }))
}

// --- Worktree write endpoints (create / remove / merge / label / archive) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorktreeBody {
    mode: Option<String>,
    branch: Option<String>,
    base_branch: Option<String>,
    profile: Option<String>,
    agent: Option<String>,
    agents: Option<Vec<String>>,
    prompt: Option<String>,
    env_overrides: Option<std::collections::HashMap<String, String>>,
    source: Option<String>,
    oneshot: Option<OneshotBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OneshotBody {
    auto_close_on_done: Option<bool>,
}

async fn create_worktree(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
    Json(body): Json<CreateWorktreeBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let app = state.project(&prefix)?;
    let mode = match body.mode.as_deref() {
        Some("existing") => CreateMode::Existing,
        _ => CreateMode::New,
    };
    let base_branch = body
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty());
    if let Some(base) = base_branch {
        if !is_valid_branch_name(base) {
            return Err(ApiError::new(400, "Invalid base branch name".to_string()));
        }
        if mode == CreateMode::Existing {
            return Err(ApiError::new(
                400,
                "Base branch is only supported for new branches".to_string(),
            ));
        }
    }

    let selected_agents: Vec<String> = match &body.agents {
        Some(agents) if !agents.is_empty() => agents.clone(),
        _ => vec![
            body.agent
                .clone()
                .unwrap_or_else(|| app.config().workspace.default_agent.clone()),
        ],
    };

    // Lock every target branch for the duration of the create.
    let mut guards: Vec<BusyGuard> = Vec::new();
    if let Some(branch) = body
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        let targets = build_create_worktree_targets(branch, &selected_agents);
        for target in &targets {
            guards.push(BusyGuard::acquire(&app, &target.branch)?);
        }
        if let Some(base) = base_branch
            && targets.iter().any(|t| t.branch == base)
        {
            return Err(ApiError::new(
                400,
                "Base branch must differ from branch name".to_string(),
            ));
        }
    }

    let input = CreateWorktreesInput {
        mode: Some(mode),
        branch: body.branch.clone(),
        base_branch: base_branch.map(str::to_string),
        prompt: body.prompt.clone(),
        profile: body.profile.clone(),
        agent: body.agent.clone(),
        agents: body.agents.clone(),
        env_overrides: body.env_overrides.clone(),
        source: match body.source.as_deref() {
            Some("oneshot") => Some(WorktreeSource::Oneshot),
            _ => Some(WorktreeSource::Ui),
        },
        oneshot: body.oneshot.as_ref().map(|o| OneshotMeta {
            auto_close_on_done: o.auto_close_on_done.unwrap_or(false),
        }),
    };

    let lifecycle = app.lifecycle();
    let result = run_blocking(move || lifecycle.create_worktrees(&input)).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "primaryBranch": result.primary_branch,
            "branches": result.branches,
        })),
    ))
}

#[derive(Deserialize)]
struct SetLabelBody {
    label: Option<String>,
}

#[derive(Deserialize)]
struct SetArchivedBody {
    archived: bool,
}

/// Run a blocking lifecycle op off the async runtime.
async fn run_blocking<T, F>(f: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T, LifecycleError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result.map_err(ApiError::from),
        Err(_) => Err(ApiError::new(500, "operation task panicked".to_string())),
    }
}

async fn remove_worktree(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    run_blocking(move || lifecycle.remove_worktree(&branch)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn merge_worktree(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    // Merging checks out the main branch in the repo root, so the main branch is
    // just as much a subject of this operation as the source worktree — hold both
    // or a concurrent open/close of main could interleave with the checkout.
    let main_branch = app.config().workspace.main_branch.clone();
    let _guard = BusyGuard::acquire_many(&app, &[&name, &main_branch])?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    run_blocking(move || lifecycle.merge_worktree(&branch)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn set_worktree_label(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<SetLabelBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let label =
        run_blocking(move || lifecycle.set_worktree_label(&branch, body.label.as_deref())).await?;
    Ok(Json(serde_json::json!({ "ok": true, "label": label })))
}

async fn set_worktree_archived(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<SetArchivedBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let archived = body.archived;
    run_blocking(move || lifecycle.set_worktree_archived(&branch, archived)).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "archived": archived }),
    ))
}

async fn close_worktree(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    run_blocking(move || lifecycle.close_worktree(&branch)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchWorktreeBody {
    launcher_id: String,
}

async fn launch_worktree(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<LaunchWorktreeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let launcher_id = body.launcher_id;
    run_blocking(move || lifecycle.launch_worktree(&branch, &launcher_id)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct OpenWorktreeBody {
    prompt: Option<String>,
    oneshot: Option<OneshotBody>,
}

async fn open_worktree(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<OpenWorktreeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let prompt = body.prompt.and_then(|p| {
        let t = p.trim().to_string();
        (!t.is_empty()).then_some(t)
    });
    let oneshot = body.oneshot.as_ref().map(|o| OneshotMeta {
        auto_close_on_done: o.auto_close_on_done.unwrap_or(false),
    });
    run_blocking(move || lifecycle.open_worktree(&branch, prompt.as_deref(), oneshot)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn create_worktree_tab(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let tab = run_blocking(move || lifecycle.create_worktree_tab(&branch)).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "tab": serde_json::to_value(&tab).unwrap() })),
    ))
}

async fn create_worktree_shell_tab(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let tab = run_blocking(move || lifecycle.create_worktree_shell_tab(&branch)).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "tab": serde_json::to_value(&tab).unwrap() })),
    ))
}

#[derive(Deserialize)]
struct CreateAgentTabBody {
    /// Agent id to start a fresh session of. Built-in or custom.
    agent: String,
}

/// A separate route from `/tabs` on purpose: `/tabs` forks the root conversation
/// and takes no body, so overloading it would mean two different semantics behind
/// one path and a compat problem for existing clients (the CLI posts `{}`).
async fn create_worktree_agent_tab(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<CreateAgentTabBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let app = state.project(&prefix)?;
    let agent = body.agent.trim().to_string();
    if agent.is_empty() {
        return Err(ApiError::new(400, "Agent id cannot be empty".to_string()));
    }
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    let tab = run_blocking(move || lifecycle.create_worktree_agent_tab(&branch, &agent)).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "tab": serde_json::to_value(&tab).unwrap() })),
    ))
}

async fn select_worktree_tab(
    State(state): State<AppState>,
    Path((prefix, name, tab_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    run_blocking(move || lifecycle.select_worktree_tab(&branch, &tab_id)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_worktree_tab(
    State(state): State<AppState>,
    Path((prefix, name, tab_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let lifecycle = app.lifecycle();
    let branch = name.clone();
    run_blocking(move || lifecycle.delete_worktree_tab(&branch, &tab_id)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SendPromptBody {
    text: String,
    preamble: Option<String>,
}

/// Terminal submit delay for the branch's agent: Codex needs 200ms, others 0.
fn submit_delay_for_branch(app: &ProjectApp, branch: &str) -> u64 {
    let Some(agent_name) = app
        .runtime
        .lock()
        .unwrap()
        .get_worktree_by_branch(branch)
        .and_then(|w| w.agent_name)
    else {
        return 0;
    };
    match get_agent_definition(&app.config(), &agent_name).map(|a| a.implementation) {
        // Codex needs a beat between opening its composer and submitting.
        Some(AgentImplementation::Builtin(
            common::services::agent_registry::BuiltinAgentId::Codex,
        )) => 200,
        _ => 0,
    }
}

async fn send_worktree_prompt(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<SendPromptBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.text.is_empty() {
        return Err(ApiError::new(400, "text must not be empty".to_string()));
    }
    let app = state.project(&prefix)?;
    // The main repo's visible pane is a plain login shell, not an agent prompt, so
    // "sending a prompt" there would type the text plus Enter straight into a
    // shell — arbitrary command execution. Guarded here because this handler does
    // not go through LifecycleService.
    if app.lifecycle().is_main_branch(&name) {
        return Err(ApiError::new(
            409,
            "Cannot send a prompt to the main repository — its session is a terminal, not an agent"
                .to_string(),
        ));
    }
    let _guard = BusyGuard::acquire(&app, &name)?;

    let (target, delay) = {
        let app = app.clone();
        let branch = name.clone();
        tokio::task::spawn_blocking(move || {
            resolve_terminal_target(&app, &branch).map(|resolved| {
                (
                    resolved.attach_target,
                    submit_delay_for_branch(&app, &branch),
                )
            })
        })
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .map_err(|e| ApiError::new(409, e))?
    };

    let terminal = state.terminal.clone();
    let text = body.text.clone();
    let preamble = body.preamble.clone();
    let sent = tokio::task::spawn_blocking(move || {
        terminal.send_prompt(&target, &text, 0, preamble.as_deref(), delay)
    })
    .await
    .map_err(|_| ApiError::new(500, "task panicked".to_string()))?;

    sent.map_err(|e| ApiError::new(503, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

const MAX_DIFF_BYTES: usize = 200 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeDiffResponse {
    uncommitted: String,
    uncommitted_truncated: bool,
    git_status: String,
    unpushed_commits: Vec<crate::adapters::git::UnpushedCommit>,
}

fn worktree_diff(app: &ProjectApp, branch: &str) -> Result<WorktreeDiffResponse, LifecycleError> {
    app.reconciliation.reconcile(&app.path, &app.runtime, false);
    let path = app
        .runtime
        .lock()
        .unwrap()
        .get_worktree_by_branch(branch)
        .map(|w| w.path)
        .ok_or_else(|| LifecycleError {
            message: format!("Worktree not found: {branch}"),
            status: 404,
        })?;

    let uncommitted = app.git.read_diff(&path);
    let git_status = app.git.read_status(&path).unwrap_or_default();
    let unpushed_commits = app.git.list_unpushed_commits(&path);

    let truncated = uncommitted.len() > MAX_DIFF_BYTES;
    let uncommitted = if truncated {
        let end = (0..=MAX_DIFF_BYTES)
            .rev()
            .find(|&i| uncommitted.is_char_boundary(i))
            .unwrap_or(0);
        uncommitted[..end].to_string()
    } else {
        uncommitted
    };

    Ok(WorktreeDiffResponse {
        uncommitted,
        uncommitted_truncated: truncated,
        git_status,
        unpushed_commits,
    })
}

async fn fetch_worktree_diff(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<WorktreeDiffResponse>, ApiError> {
    let app = state.project(&prefix)?;
    let response = run_blocking(move || worktree_diff(&app, &name)).await?;
    Ok(Json(response))
}

/// Resolve a worktree's absolute filesystem path by branch (reconciling first).
/// 404 when the worktree isn't found.
fn worktree_path(app: &ProjectApp, branch: &str) -> Result<String, LifecycleError> {
    app.reconciliation.reconcile(&app.path, &app.runtime, false);
    app.runtime
        .lock()
        .unwrap()
        .get_worktree_by_branch(branch)
        .map(|w| w.path)
        .ok_or_else(|| LifecycleError {
            message: format!("Worktree not found: {branch}"),
            status: 404,
        })
}

/// Sebenza track registry (`.ai/sebenza/tracks.json`) for a worktree.
/// Returns `null` when the worktree has no Sebenza workspace.
async fn fetch_tracks(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<Option<serde_json::Value>>, ApiError> {
    let app = state.project(&prefix)?;
    let value = run_blocking(move || {
        let path = worktree_path(&app, &name)?;
        Ok(crate::adapters::fs::read_tracks(&path))
    })
    .await?;
    Ok(Json(value))
}

#[derive(Deserialize)]
struct TrackFileQuery {
    path: String,
}

/// A single file under a worktree's `.ai/sebenza` dir (plan.json / spec.md / design.md),
/// returned as `{ path, content }`. 400 on path traversal, 404 when absent.
async fn fetch_track_file(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Query(query): Query<TrackFileQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let rel = query.path;
    let response = run_blocking(move || {
        let path = worktree_path(&app, &name)?;
        track_file_json(crate::adapters::fs::read_track_file(&path, &rel), rel)
    })
    .await?;
    Ok(Json(response))
}

/// Shape a track-file read into `{ path, content }` or the matching HTTP error.
/// Shared by the per-worktree and registry-scoped endpoints.
fn track_file_json(
    read: Result<String, crate::adapters::fs::TrackFileError>,
    rel: String,
) -> Result<serde_json::Value, LifecycleError> {
    use crate::adapters::fs::TrackFileError;
    match read {
        Ok(content) => Ok(serde_json::json!({ "path": rel, "content": content })),
        Err(TrackFileError::Traversal) => Err(LifecycleError {
            message: "Invalid track file path".to_string(),
            status: 400,
        }),
        Err(TrackFileError::NotFound) => Err(LifecycleError {
            message: format!("Track file not found: {rel}"),
            status: 404,
        }),
    }
}

#[derive(Deserialize)]
struct PullMainBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    repo: Option<String>,
}

fn pull_main_impl(
    app: &ProjectApp,
    force: bool,
    repo: Option<String>,
) -> Result<PullMainResult, LifecycleError> {
    let config = app.config();
    let project_root = match repo.as_deref().filter(|r| !r.is_empty()) {
        None => app.path.clone(),
        Some(alias) => {
            let linked = config
                .integrations
                .github
                .linked_repos
                .iter()
                .find(|lr| lr.alias == alias)
                .ok_or_else(|| LifecycleError {
                    message: format!("Unknown linked repo: {alias}"),
                    status: 404,
                })?;
            let dir = linked.dir.as_ref().ok_or_else(|| LifecycleError {
                message: format!("Linked repo \"{alias}\" has no dir configured"),
                status: 400,
            })?;
            let resolved = std::path::Path::new(&app.path).join(dir);
            app.git
                .resolve_repo_root(&resolved.to_string_lossy())
                .ok_or_else(|| LifecycleError {
                    message: format!("Linked repo \"{alias}\" dir is not a git repository"),
                    status: 400,
                })?
        }
    };

    let main_branch = &config.workspace.main_branch;
    Ok(if force {
        force_pull_main_branch(&app.git, &project_root, main_branch)
    } else {
        pull_main_branch(&app.git, &project_root, main_branch)
    })
}

async fn pull_main(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
    Json(body): Json<PullMainBody>,
) -> Result<Json<PullMainResult>, ApiError> {
    let app = state.project(&prefix)?;
    let response = run_blocking(move || pull_main_impl(&app, body.force, body.repo)).await?;
    Ok(Json(response))
}

async fn refresh_agent_terminal(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let _guard = BusyGuard::acquire(&app, &name)?;
    let branch = name.clone();
    run_blocking(move || {
        // Resolve the latest on-disk conversation id to resume from.
        let snapshot = reconcile_and_snapshot(&app);
        let worktree = snapshot
            .worktrees
            .into_iter()
            .find(|w| w.branch == branch)
            .ok_or(LifecycleError {
                message: format!("Worktree not found: {branch}"),
                status: 404,
            })?;
        let conversation_id = common::services::conversation_router::resolve_conversation_id(
            &worktree,
        )
        .ok_or_else(|| LifecycleError {
            message: format!(
                "Refreshing the agent terminal is only available for these agents: {}",
                common::services::conversation_router::conversation_capable_agent_ids().join(", ")
            ),
            status: 409,
        })?;
        if conversation_id.contains("-pending:") {
            return Err(LifecycleError {
                message: "No conversation is available to refresh".to_string(),
                status: 409,
            });
        }
        app.lifecycle()
            .refresh_agent_terminal(&branch, &conversation_id)
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Sync PR status for the project, then return the (updated) worktree snapshot.
async fn sync_worktree_prs(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<WorktreeSnapshot>, ApiError> {
    let app = state.project(&prefix)?;
    let snapshot = tokio::task::spawn_blocking(move || {
        let linked = app.config().integrations.github.linked_repos.clone();
        crate::services::pr_service::sync_pr_status(&app.git, &app.path, &linked);
        reconcile_and_snapshot(&app)
    })
    .await
    .map_err(|_| ApiError::new(500, "sync task panicked".to_string()))?;

    snapshot
        .worktrees
        .into_iter()
        .find(|w| w.branch == name)
        .map(Json)
        .ok_or_else(|| ApiError::new(404, format!("Worktree not found: {name}")))
}

async fn fetch_ci_logs(
    State(state): State<AppState>,
    Path((prefix, run_id)): Path<(String, i64)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let path = app.path.clone();
    let logs = tokio::task::spawn_blocking(move || {
        crate::services::pr_service::fetch_ci_logs(run_id, &path)
    })
    .await
    .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
    .map_err(|e| ApiError::new(502, e))?;
    Ok(Json(serde_json::json!({ "logs": logs })))
}

// --- Agents chat (conversation read path) ---

/// Reconcile, find the worktree by branch, and read its conversation from the
/// latest on-disk agent session (Claude JSONL / Codex rollout). Attach and
/// history share this read path (send/interrupt/live-streaming deferred).
async fn agents_conversation(
    state: &AppState,
    prefix: &str,
    name: &str,
) -> Result<Json<crate::services::agents_ui::AgentsUiConversationResponse>, ApiError> {
    let app = state.project(prefix)?;
    let branch = name.to_string();
    let response = tokio::task::spawn_blocking(move || {
        let snapshot = reconcile_and_snapshot(&app);
        let worktree = snapshot
            .worktrees
            .into_iter()
            .find(|w| w.branch == branch)
            .ok_or((404u16, format!("Worktree not found: {branch}")))?;
        common::services::conversation_router::read_worktree_conversation(&worktree).ok_or_else(
            || {
                (
                    409u16,
                    format!(
                        "Worktree chat is only available for these agents: {}",
                        common::services::conversation_router::conversation_capable_agent_ids()
                            .join(", ")
                    ),
                )
            },
        )
    })
    .await
    .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
    .map_err(|(status, message)| ApiError::new(status, message))?;
    Ok(Json(response))
}

async fn agents_attach(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<crate::services::agents_ui::AgentsUiConversationResponse>, ApiError> {
    agents_conversation(&state, &prefix, &name).await
}

async fn agents_history(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<crate::services::agents_ui::AgentsUiConversationResponse>, ApiError> {
    agents_conversation(&state, &prefix, &name).await
}

#[derive(Deserialize)]
struct AgentsSendBody {
    text: String,
}

/// Build the streaming run input for a worktree (blocking: reconcile + fs).
/// Supports Claude (token deltas) and Codex (completed items).
fn prepare_agent_send(
    app: &ProjectApp,
    branch: &str,
    text: &str,
) -> Result<crate::services::agent_stream::StartRunInput, (u16, String)> {
    use crate::adapters::fs::{build_runtime_env_map, load_dotenv_local, read_worktree_meta};
    use crate::services::agent_stream::{StartRunInput, StreamProvider};

    let snapshot = reconcile_and_snapshot(app);
    let worktree = snapshot
        .worktrees
        .into_iter()
        .find(|w| w.branch == branch)
        .ok_or((404u16, format!("Worktree not found: {branch}")))?;
    let provider = worktree
        .agent_name
        .as_deref()
        .and_then(|id| get_agent_definition(&app.config(), id))
        .and_then(|def| match def.implementation {
            AgentImplementation::Builtin(id) => StreamProvider::for_builtin(id),
            AgentImplementation::Custom(_) => None,
        })
        .ok_or_else(|| {
            (
                409u16,
                format!(
                    "Streaming chat is only available for these agents: {}",
                    common::services::conversation_router::conversation_capable_agent_ids()
                        .join(", ")
                ),
            )
        })?;
    if !worktree.mux {
        return Err((
            409,
            "Open this worktree in the main dashboard before sending messages here".to_string(),
        ));
    }

    let git_dir = app
        .git
        .resolve_worktree_git_dir(&worktree.path)
        .map_err(|e| (422u16, e))?;
    let meta =
        read_worktree_meta(&git_dir).ok_or((409u16, "Worktree metadata is missing".to_string()))?;

    let config = app.config();
    let profile = config
        .profiles
        .get(&meta.profile)
        .ok_or((400u16, format!("Unknown profile: {}", meta.profile)))?;
    if meta.runtime != "host" {
        return Err((
            409,
            "Streaming chat is only available for host-runtime worktrees".to_string(),
        ));
    }

    // Resolve the current conversation id (real session id, or a pending placeholder)
    // through the worktree's own agent adapter — the same path interrupt and streaming
    // use, so all three cannot disagree about which session a worktree owns.
    let conversation_id = common::services::conversation_router::resolve_conversation_id(&worktree)
        .ok_or((
            409u16,
            "No conversation adapter for this worktree's agent".to_string(),
        ))?;
    let resume_session_id =
        (!conversation_id.contains("-pending:")).then(|| conversation_id.clone());

    let mut extra = std::collections::HashMap::new();
    extra.insert("SEBENZA_WORKTREE_PATH".to_string(), worktree.path.clone());
    let env = build_runtime_env_map(&meta, &extra, &load_dotenv_local(&worktree.path));

    Ok(StartRunInput {
        provider,
        conversation_id,
        cwd: worktree.path,
        prompt: text.to_string(),
        env,
        permission_mode: (profile.yolo == Some(true)).then(|| "bypassPermissions".to_string()),
        resume_session_id,
        system_prompt: profile.system_prompt.clone(),
    })
}

async fn agents_send(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
    Json(body): Json<AgentsSendBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.text.trim().is_empty() {
        return Err(ApiError::new(400, "text must not be empty".to_string()));
    }
    let app = state.project(&prefix)?;
    let branch = name.clone();
    let text = body.text.clone();
    let input = tokio::task::spawn_blocking(move || prepare_agent_send(&app, &branch, &text))
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .map_err(|(status, message)| ApiError::new(status, message))?;

    let conversation_id = input.conversation_id.clone();
    let turn_id = state
        .agent_stream
        .start_run(input)
        .map_err(|e| ApiError::new(409, e))?;

    Ok(Json(serde_json::json!({
        "conversationId": conversation_id,
        "turnId": turn_id,
        "running": true,
        "streaming": true,
    })))
}

async fn agents_interrupt(
    State(state): State<AppState>,
    Path((prefix, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    let branch = name.clone();
    // Resolve the conversation id for the worktree (blocking).
    let conversation_id = {
        let app2 = app.clone();
        tokio::task::spawn_blocking(move || {
            let snapshot = reconcile_and_snapshot(&app2);
            snapshot
                .worktrees
                .into_iter()
                .find(|w| w.branch == branch)
                // Route through the worktree's OWN agent adapter. Calling the Claude
                // service unconditionally resolved `claude-pending:<path>` for a Codex
                // worktree, which never matches the id the run was registered under, so
                // interrupt always 409'd for Codex.
                .map(|w| common::services::conversation_router::resolve_conversation_id(&w))
        })
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .ok_or_else(|| ApiError::new(404, format!("Worktree not found: {name}")))?
        .ok_or_else(|| {
            ApiError::new(
                409,
                format!(
                    "Interrupt is only available for these agents: {}",
                    common::services::conversation_router::conversation_capable_agent_ids()
                        .join(", ")
                ),
            )
        })?
    };

    match state.agent_stream.interrupt(&conversation_id) {
        Some(turn_id) => Ok(Json(serde_json::json!({
            "conversationId": conversation_id,
            "turnId": turn_id,
            "interrupted": true,
            "streaming": true,
        }))),
        None => Err(ApiError::new(
            409,
            "No active agent response to interrupt".to_string(),
        )),
    }
}

// --- Agents streaming WebSocket (`/:prefix/ws/agents/worktrees/:name`) ---

/// Per-subscriber ordering: assigns a monotonic `revision` to every event and a
/// stable `order` per item id (continuing after the history message count).
struct AgentsStreamSession {
    conversation_id: String,
    revision: u64,
    next_order: u64,
    item_orders: std::collections::HashMap<String, u64>,
}

impl AgentsStreamSession {
    fn new(conversation_id: String, next_order: u64) -> Self {
        AgentsStreamSession {
            conversation_id,
            revision: 0,
            next_order,
            item_orders: std::collections::HashMap::new(),
        }
    }

    fn next_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    fn reserve_order(&mut self, item_id: &str) -> u64 {
        if let Some(order) = self.item_orders.get(item_id) {
            return *order;
        }
        let order = self.next_order;
        self.next_order += 1;
        self.item_orders.insert(item_id.to_string(), order);
        order
    }

    /// Convert a live stream event into a wire JSON frame.
    fn frame_for(
        &mut self,
        event: crate::services::agent_stream::StreamEvent,
    ) -> serde_json::Value {
        use crate::services::agent_stream::StreamEvent;
        match event {
            StreamEvent::Status {
                running,
                active_turn_id,
            } => serde_json::json!({
                "type": "conversationStatus",
                "revision": self.next_revision(),
                "conversationId": self.conversation_id,
                "running": running,
                "activeTurnId": active_turn_id,
            }),
            StreamEvent::Delta {
                turn_id,
                item_id,
                delta,
            } => {
                let order = self.reserve_order(&item_id);
                serde_json::json!({
                    "type": "messageDelta",
                    "revision": self.next_revision(),
                    "conversationId": self.conversation_id,
                    "turnId": turn_id,
                    "itemId": item_id,
                    "order": order,
                    "delta": delta,
                })
            }
            StreamEvent::Upsert { message, order_key } => {
                let order = self.reserve_order(&order_key);
                self.item_orders.insert(message.id.clone(), order);
                serde_json::json!({
                    "type": "messageUpsert",
                    "revision": self.next_revision(),
                    "conversationId": self.conversation_id,
                    "message": message_to_json(&message, order),
                })
            }
            StreamEvent::Error { message } => serde_json::json!({
                "type": "error",
                "message": message,
            }),
        }
    }
}

fn message_to_json(
    m: &crate::services::agent_stream::DraftMessage,
    order: u64,
) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "id": m.id,
        "turnId": m.turn_id,
        "order": order,
        "role": m.role,
        "text": m.text,
        "status": m.status,
        "createdAt": m.created_at,
        "kind": m.kind,
    });
    if let Some(name) = &m.tool_name {
        obj["toolName"] = serde_json::json!(name);
    }
    if let Some(id) = &m.tool_call_id {
        obj["toolCallId"] = serde_json::json!(id);
    }
    obj
}

async fn ws_agents_stream(
    ws: WebSocketUpgrade,
    Path((prefix, name)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    let Ok(app) = state.project(&prefix) else {
        return (StatusCode::NOT_FOUND, "Project not found").into_response();
    };
    let agent_stream = state.agent_stream.clone();
    ws.on_upgrade(move |socket| agents_stream_socket(socket, app, name, agent_stream))
}

async fn agents_stream_socket(
    socket: WebSocket,
    app: Arc<ProjectApp>,
    branch: String,
    agent_stream: Arc<crate::services::agent_stream::AgentStreamManager>,
) {
    // Resolve the conversation id + history length (for the starting order).
    let resolved = {
        let app = app.clone();
        let branch = branch.clone();
        tokio::task::spawn_blocking(move || {
            let snapshot = reconcile_and_snapshot(&app);
            snapshot
                .worktrees
                .into_iter()
                .find(|w| w.branch == branch)
                .and_then(|w| {
                    // Route through the worktree's own agent adapter — see `agents_interrupt`.
                    let conv =
                        common::services::conversation_router::read_worktree_conversation(&w)?
                            .conversation;
                    Some((conv.conversation_id, conv.messages.len() as u64))
                })
        })
        .await
        .ok()
        .flatten()
    };

    let (mut sink, mut stream) = socket.split();
    let Some((conversation_id, next_order)) = resolved else {
        let _ = sink
            .send(Message::Text(
                serde_json::json!({"type":"error","message":"Worktree not found"})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };

    let mut session = AgentsStreamSession::new(conversation_id.clone(), next_order);

    match agent_stream.subscribe(&conversation_id) {
        Some(subscription) => {
            for event in subscription.replay {
                let frame = session.frame_for(event);
                if sink
                    .send(Message::Text(frame.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            let mut receiver = subscription.receiver;
            loop {
                tokio::select! {
                    incoming = stream.next() => match incoming {
                        None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                        _ => {}
                    },
                    event = receiver.recv() => match event {
                        Ok(event) => {
                            let frame = session.frame_for(event);
                            if sink.send(Message::Text(frame.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        }
        None => {
            // No active run — idle until the client disconnects.
            while let Some(Ok(message)) = stream.next().await {
                if matches!(message, Message::Close(_)) {
                    break;
                }
            }
        }
    }
}

// --- Hub endpoints (project registry, no prefix) ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSummary {
    prefix: String,
    name: String,
    path: String,
    active: bool,
}

fn to_summary(app: &ProjectApp) -> ProjectSummary {
    ProjectSummary {
        prefix: app.prefix.clone(),
        name: app.name(),
        path: app.path.clone(),
        active: app.active.load(Ordering::Relaxed),
    }
}

/// The `sebenza` plugin's user-scoped registry (`~/.ai/sebenza/registry.json`)
/// with each registered project's `tracks.json` resolved — the cross-project
/// portfolio. Broken entries come back flagged rather than omitted, so this
/// never fails because one project moved.
async fn fetch_registry() -> Result<Json<serde_json::Value>, ApiError> {
    let portfolio =
        run_blocking(|| Ok(common::services::portfolio_service::load_portfolio())).await?;
    serde_json::to_value(portfolio)
        .map(Json)
        .map_err(|e| ApiError::new(500, e.to_string()))
}

#[derive(Deserialize)]
struct RegistryFileQuery {
    /// Absolute project root, matched exactly against a registry entry.
    project: String,
    /// Path relative to that project's Sebenza workspace.
    path: String,
}

/// A track artifact (plan.json / spec.md / design.md) belonging to a registered
/// project, for portfolio drill-down.
async fn fetch_registry_file(
    Query(query): Query<RegistryFileQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let RegistryFileQuery { project, path: rel } = query;
    let response = run_blocking(move || {
        let read = common::services::portfolio_service::read_registry_track_file(&project, &rel);
        track_file_json(read, rel)
    })
    .await?;
    Ok(Json(response))
}

/// Every loaded project and its worktree snapshots, for the cross-project ticker.
///
/// Hub-level (no project prefix) because it deliberately spans projects. Reconciles each
/// project the same way `get_worktrees` does, so a worktree that appeared or vanished
/// since the last poll is reflected here too.
///
/// Only projects this process has loaded contribute; projects initialize lazily, so a
/// registered-but-untouched project is absent rather than empty.
async fn get_active_worktrees(
    State(state): State<AppState>,
) -> Json<crate::services::active_worktrees::ActiveWorktreesResponse> {
    let apps = state.manager.list();
    let projects = tokio::task::spawn_blocking(move || {
        apps.into_iter()
            .map(|app| {
                let snapshot = reconcile_and_snapshot(&app);
                (app.prefix.clone(), app.name(), snapshot.worktrees)
            })
            .collect::<Vec<_>>()
    })
    .await
    .expect("cross-project snapshot task panicked");

    Json(crate::services::active_worktrees::build_active_worktrees(
        projects,
    ))
}

async fn list_projects(State(state): State<AppState>) -> Json<serde_json::Value> {
    let projects: Vec<ProjectSummary> =
        state.manager.list().iter().map(|a| to_summary(a)).collect();
    Json(serde_json::json!({ "projects": projects }))
}

/// A repo is a Sebenza project once it has a config file.
fn has_project_config(root: &str) -> bool {
    let dir = std::path::Path::new(root).join(".ai");
    dir.join("sebenza.yaml").exists() || dir.join("sebenza.local.yaml").exists()
}

#[derive(Deserialize)]
struct AddProjectBody {
    path: String,
}

async fn add_project(
    State(state): State<AppState>,
    Json(body): Json<AddProjectBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let input = body.path.trim();
    if input.is_empty() {
        return Err(ApiError::new(
            400,
            "Request body must be { path: string }".to_string(),
        ));
    }
    if GitGateway::new().resolve_repo_root(input).is_none() {
        return Err(ApiError::new(400, format!("Not a git repository: {input}")));
    }
    let root = project_root(input);

    // Already served → register now (idempotent), no setup job.
    if let Some(existing) = state.manager.get_by_path(input) {
        return Ok(Json(serde_json::json!({
            "initializing": false,
            "path": existing.path,
            "project": to_summary(&existing),
        })));
    }

    // A setup is already in flight for this repo (double-click / second tab):
    // send the caller to the poller. Checked before `has_project_config` because
    // setup writes `.ai/sebenza.yaml` mid-run — without this a second request would
    // see the half-written config and register before analysis finishes.
    if state.project_inits.is_active(&root) {
        return Ok(Json(serde_json::json!({
            "initializing": true,
            "path": root,
            "project": serde_json::Value::Null,
        })));
    }

    // Already a Sebenza project → register immediately.
    if has_project_config(&root) {
        let app = state.manager.add(input);
        return Ok(Json(serde_json::json!({
            "initializing": false,
            "path": app.path,
            "project": to_summary(&app),
        })));
    }

    // No config → scaffold + analyze + register asynchronously; the client polls
    // `/api/projects/init` for progress and the resulting prefix.
    let root_owned = root.clone();
    tokio::task::spawn_blocking(move || {
        use crate::services::init_authoring;
        let agent = init_authoring::authoring_agent();
        let analyzer_available =
            crate::util::shell::which("claude") || crate::util::shell::which("codex");
        // A monotonic-ish unique suffix for the codex summary temp file.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| root_owned.len().to_string());
        crate::services::project_init_service::run_project_init(
            &state.project_inits,
            &root_owned,
            analyzer_available,
            || {
                init_authoring::scaffold_config(&init_authoring::detect_init_project_context(
                    &root_owned,
                    agent,
                ))
            },
            || {
                init_authoring::analyze_config(
                    &init_authoring::detect_init_project_context(&root_owned, agent),
                    &unique,
                )
            },
            || {
                let app = state.manager.add(&root_owned);
                (app.prefix.clone(), app.config().name.clone())
            },
        );
    });

    Ok(Json(serde_json::json!({
        "initializing": true,
        "path": root,
        "project": serde_json::Value::Null,
    })))
}

/// Poll in-flight (and recently-finished) on-add project setups.
async fn list_project_inits(State(state): State<AppState>) -> Json<serde_json::Value> {
    let inits: Vec<serde_json::Value> = state
        .project_inits
        .list()
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "path": s.path,
                "phase": s.phase.as_str(),
                "prefix": s.prefix,
                "name": s.name,
                "error": s.error,
            })
        })
        .collect();
    Json(serde_json::json!({ "inits": inits }))
}

async fn remove_project(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if state.manager.remove(&prefix) {
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err(ApiError::new(404, "Project not found".to_string()))
    }
}

/// Agent → backend control channel. Applies a runtime event to whichever
/// project owns the worktree id, recording a notification. Bearer-token authed.
async fn runtime_event(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let expected =
        crate::adapters::control_token::load_control_token().map_err(|e| ApiError::new(500, e))?;
    if token != Some(expected.as_str()) {
        return Err(ApiError::new(401, "Unauthorized".to_string()));
    }

    let raw: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::new(400, "Invalid JSON".to_string()))?;
    let event = crate::domain::events::parse_runtime_event(&raw)
        .ok_or_else(|| ApiError::new(400, "Invalid runtime event body".to_string()))?;

    let notification = tokio::task::spawn_blocking(move || apply_runtime_event(&state, &event))
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .ok_or_else(|| ApiError::new(404, "Unknown worktree id".to_string()))?;

    let mut out = serde_json::json!({ "ok": true });
    if let Some(notification) = notification {
        out["notification"] = serde_json::to_value(notification).unwrap_or(serde_json::Value::Null);
    }
    Ok(Json(out))
}

/// Apply the event to the project owning the worktree id (reconciling once if the
/// id isn't yet known). Returns `Some(notification?)` on success, `None` if no
/// project owns it. Blocking.
fn apply_runtime_event(
    state: &AppState,
    event: &crate::domain::events::RuntimeEvent,
) -> Option<Option<crate::domain::model::NotificationView>> {
    for app in state.manager.list() {
        if app.runtime.lock().unwrap().apply_event(event).is_ok() {
            return Some(app.notifications.record_event(event));
        }
    }
    // Unknown id everywhere — reconcile each project once, then retry.
    for app in state.manager.list() {
        app.reconciliation.reconcile(&app.path, &app.runtime, true);
        if app.runtime.lock().unwrap().apply_event(event).is_ok() {
            return Some(app.notifications.record_event(event));
        }
    }
    None
}

async fn dismiss_notification(
    State(state): State<AppState>,
    Path((prefix, id)): Path<(String, i64)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let app = state.project(&prefix)?;
    if app.notifications.dismiss(id) {
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err(ApiError::new(404, "Notification not found".to_string()))
    }
}

/// Other live Sebenza servers on this machine (the migration sensor). The
/// dashboard uses this to prompt consolidating leftover single-project servers.
async fn fetch_instances(State(_state): State<AppState>) -> Json<serde_json::Value> {
    let instances: Vec<serde_json::Value> = crate::adapters::instance_registry::list_live()
        .into_iter()
        .map(|e| serde_json::json!({ "port": e.port, "projectDir": e.project_dir }))
        .collect();
    Json(serde_json::json!({ "instances": instances }))
}

#[derive(Deserialize)]
struct MigrateProjectsBody {
    paths: Vec<String>,
}

/// Fold repos served by leftover single-project instances into this server. Each
/// path is validated (git repo + has `.ai/sebenza.yaml`) and added/persisted;
/// per-path failures are reported, not fatal.
async fn migrate_projects(
    State(state): State<AppState>,
    Json(body): Json<MigrateProjectsBody>,
) -> Json<serde_json::Value> {
    let mut migrated: Vec<ProjectSummary> = Vec::new();
    let mut failed: Vec<serde_json::Value> = Vec::new();
    for path in body.paths {
        let path = path.trim().to_string();
        if path.is_empty() {
            continue;
        }
        if GitGateway::new().resolve_repo_root(&path).is_none() {
            failed.push(serde_json::json!({ "path": path, "error": format!("Not a git repository: {path}") }));
            continue;
        }
        if !has_project_config(&project_root(&path)) {
            failed.push(serde_json::json!({ "path": path, "error": format!("No .ai/sebenza.yaml in {path}") }));
            continue;
        }
        let app = state.manager.add(&path);
        migrated.push(to_summary(&app));
    }
    Json(serde_json::json!({ "migrated": migrated, "failed": failed }))
}

// --- Terminal WebSocket (`/:prefix/ws/:worktree`) ---

/// Inbound terminal control messages (JSON, discriminated on `type`). Parsing is
/// lenient: an unrecognized or malformed frame yields an `error` reply.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum WsInbound {
    #[serde(rename = "input")]
    Input { data: String },
    #[serde(rename = "sendKeys")]
    SendKeys {
        #[serde(rename = "hexBytes")]
        hex_bytes: Vec<String>,
    },
    #[serde(rename = "selectPane")]
    SelectPane { pane: i64 },
    #[serde(rename = "resize")]
    Resize {
        cols: u16,
        rows: u16,
        #[serde(rename = "initialPane")]
        initial_pane: Option<i64>,
    },
}

/// Outbound terminal frames. Hot-path `output`/`scrollback` use a single-char
/// prefix; the rest are JSON.
enum OutFrame {
    Output(String),
    Scrollback(String),
    Exit(i32),
    Error(String),
}

impl OutFrame {
    fn into_message(self) -> Message {
        match self {
            OutFrame::Output(data) => Message::Text(format!("o{data}").into()),
            OutFrame::Scrollback(data) => Message::Text(format!("s{data}").into()),
            OutFrame::Exit(exit_code) => Message::Text(
                serde_json::json!({ "type": "exit", "exitCode": exit_code })
                    .to_string()
                    .into(),
            ),
            OutFrame::Error(message) => Message::Text(
                serde_json::json!({ "type": "error", "message": message })
                    .to_string()
                    .into(),
            ),
        }
    }
}

struct ResolvedTerminal {
    worktree_id: String,
    attach_target: TerminalAttachTarget,
}

/// Resolve the tmux window a branch's terminal attaches to. Reconciles once (to
/// pick up a freshly-created session) if the runtime has no live session yet.
/// Blocking — run via `spawn_blocking`.
fn resolve_terminal_target(app: &ProjectApp, branch: &str) -> Result<ResolvedTerminal, String> {
    let mut runtime_state = app.runtime.lock().unwrap().get_worktree_by_branch(branch);
    let needs_reconcile = runtime_state
        .as_ref()
        .map(|s| !s.session.exists || s.session.session_name.is_none())
        .unwrap_or(true);
    if needs_reconcile {
        app.reconciliation.reconcile(&app.path, &app.runtime, false);
        runtime_state = app.runtime.lock().unwrap().get_worktree_by_branch(branch);
    }

    let runtime_state = runtime_state.ok_or_else(|| format!("Worktree not found: {branch}"))?;
    let Some(session_name) = runtime_state.session.session_name.clone() else {
        return Err(format!("No open tmux window found for worktree: {branch}"));
    };
    if !runtime_state.session.exists {
        return Err(format!("No open tmux window found for worktree: {branch}"));
    }

    Ok(ResolvedTerminal {
        worktree_id: runtime_state.worktree_id,
        attach_target: TerminalAttachTarget {
            owner_session_name: session_name,
            window_name: runtime_state.session.window_name,
        },
    })
}

async fn ws_terminal(
    ws: WebSocketUpgrade,
    Path((prefix, worktree)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    let Ok(app) = state.project(&prefix) else {
        return (StatusCode::NOT_FOUND, "Project not found").into_response();
    };
    let terminal = state.terminal.clone();
    ws.on_upgrade(move |socket| terminal_socket(socket, worktree, app, terminal))
}

/// Attach the PTY, replay scrollback, and spawn the pump forwarding PTY output to
/// the socket. Returns the `attach_id` and the pump task handle.
async fn attach_flow(
    app: &Arc<ProjectApp>,
    terminal: &Arc<TerminalManager>,
    branch: &str,
    cols: u16,
    rows: u16,
    initial_pane: Option<i64>,
    out_tx: UnboundedSender<OutFrame>,
) -> Result<(String, tokio::task::JoinHandle<()>), String> {
    let resolved = {
        let app = app.clone();
        let branch = branch.to_string();
        tokio::task::spawn_blocking(move || resolve_terminal_target(&app, &branch))
            .await
            .map_err(|e| e.to_string())??
    };

    let attach_id = terminal.new_attach_id(&resolved.worktree_id);

    let mut pty_rx = {
        let terminal = terminal.clone();
        let attach_id = attach_id.clone();
        let target = resolved.attach_target.clone();
        tokio::task::spawn_blocking(move || {
            terminal.attach(&attach_id, &target, cols, rows, initial_pane)
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let scrollback = {
        let terminal = terminal.clone();
        let attach_id = attach_id.clone();
        tokio::task::spawn_blocking(move || terminal.get_scrollback(&attach_id))
            .await
            .unwrap_or_default()
    };
    if !scrollback.is_empty() {
        let _ = out_tx.send(OutFrame::Scrollback(scrollback));
    }

    let pump = tokio::spawn(async move {
        while let Some(event) = pty_rx.recv().await {
            match event {
                TerminalEvent::Data(data) => {
                    if out_tx.send(OutFrame::Output(data)).is_err() {
                        break;
                    }
                }
                TerminalEvent::Exit(code) => {
                    let _ = out_tx.send(OutFrame::Exit(code));
                    break;
                }
            }
        }
    });

    Ok((attach_id, pump))
}

async fn terminal_socket(
    socket: WebSocket,
    branch: String,
    app: Arc<ProjectApp>,
    terminal: Arc<TerminalManager>,
) {
    // Mark the project active (viewed) while a socket is open.
    app.active.store(true, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = unbounded_channel::<OutFrame>();

    // One task owns the sink; both the PTY pump and inbound handler feed it.
    let writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if sink.send(frame.into_message()).await.is_err() {
                break;
            }
        }
    });

    let mut attach_id: Option<String> = None;
    let mut pump: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(Ok(message)) = stream.next().await {
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(inbound) = serde_json::from_str::<WsInbound>(&text) else {
            let _ = out_tx.send(OutFrame::Error("malformed message".to_string()));
            continue;
        };

        match inbound {
            WsInbound::Input { data } => match &attach_id {
                Some(id) => {
                    let terminal = terminal.clone();
                    let id = id.clone();
                    let _ = tokio::task::spawn_blocking(move || terminal.write(&id, &data)).await;
                }
                None => {
                    let _ = out_tx.send(OutFrame::Error("Terminal not attached".to_string()));
                }
            },
            WsInbound::SendKeys { hex_bytes } => match &attach_id {
                Some(id) => {
                    let terminal = terminal.clone();
                    let id = id.clone();
                    let _ =
                        tokio::task::spawn_blocking(move || terminal.send_keys(&id, &hex_bytes))
                            .await;
                }
                None => {
                    let _ = out_tx.send(OutFrame::Error("Terminal not attached".to_string()));
                }
            },
            WsInbound::SelectPane { pane } => match &attach_id {
                Some(id) => {
                    let terminal = terminal.clone();
                    let id = id.clone();
                    let _ =
                        tokio::task::spawn_blocking(move || terminal.select_pane(&id, pane)).await;
                }
                None => {
                    let _ = out_tx.send(OutFrame::Error("Terminal not attached".to_string()));
                }
            },
            WsInbound::Resize {
                cols,
                rows,
                initial_pane,
            } => {
                if attach_id.is_none() {
                    // First resize = client reporting real dimensions. Attach now.
                    match attach_flow(
                        &app,
                        &terminal,
                        &branch,
                        cols,
                        rows,
                        initial_pane,
                        out_tx.clone(),
                    )
                    .await
                    {
                        Ok((id, task)) => {
                            attach_id = Some(id);
                            pump = Some(task);
                        }
                        Err(err) => {
                            let _ = out_tx.send(OutFrame::Error(err));
                            break;
                        }
                    }
                } else if let Some(id) = &attach_id {
                    let terminal = terminal.clone();
                    let id = id.clone();
                    let _ =
                        tokio::task::spawn_blocking(move || terminal.resize(&id, cols, rows)).await;
                }
            }
        }
    }

    if let Some(id) = attach_id {
        let terminal = terminal.clone();
        let _ = tokio::task::spawn_blocking(move || terminal.detach(&id)).await;
    }
    if let Some(task) = pump {
        task.abort();
    }
    drop(out_tx);
    writer.abort();
    app.active.store(false, Ordering::Relaxed);
}
