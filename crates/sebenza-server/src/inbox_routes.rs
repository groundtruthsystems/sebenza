//! The `/api/inbox` hub routes.
//!
//! Global, not project-prefixed: a draft may exist before it belongs to any
//! project. That breadth is exactly why every mutating route here runs through
//! [`guard_inbox_request`] — a control token *and* a same-origin check, neither
//! substituting for the other.

use crate::adapters::inbox_store::EventAuthor;
use crate::domain::inbox_events::{Thread, WorktreeKey, request_id_of};
use crate::domain::model::{
    DraftStatus, FileRevision, InboxDraft, InboxDraftView, Priority, PrioritySource,
};
use crate::domain::policies::{
    InboxGuard, InboxGuardDenial, allowed_hosts_from_env, guard_inbox_request, host_is_allowed,
    is_safe_project_path, origin_is_acceptable, sanitize_caller_marker,
};
use crate::inbox_runner::ServerConversionRunner;
use crate::services::inbox_convert::{
    Advisory, ConversionTarget, sandbox_advisory, scan_for_secrets, validate_targets,
};
use crate::services::inbox_jobs::{JobEvent, JobSnapshot, JobSubscription};
use crate::services::inbox_service::{
    CommentGroups, DraftSummary, InboxService, InboxServiceError, ListQuery, ProjectLink,
    parse_worktree_ingress,
};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::Json;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::server::ApiError;

/// Wire shape of a project link. `resolved` is false when the stored path no
/// longer matches a registered project — the draft is still perfectly usable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLinkWire {
    pub path: String,
    pub name: Option<String>,
    pub resolved: bool,
}

impl From<ProjectLink> for ProjectLinkWire {
    fn from(link: ProjectLink) -> Self {
        match link {
            ProjectLink::Resolved { path, name } => Self {
                path,
                name: Some(name),
                resolved: true,
            },
            ProjectLink::Unresolved { path } => Self {
                path,
                name: None,
                resolved: false,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSummaryWire {
    pub id: String,
    pub title: String,
    pub status: DraftStatus,
    pub updated_at: String,
    pub project: Option<ProjectLinkWire>,
    pub priority: Priority,
    pub priority_source: PrioritySource,
    pub is_raw: bool,
}

impl From<DraftSummary> for DraftSummaryWire {
    fn from(s: DraftSummary) -> Self {
        Self {
            id: s.id,
            title: s.title,
            status: s.status,
            updated_at: s.updated_at,
            project: s.project.map(Into::into),
            priority: s.priority,
            priority_source: s.priority_source,
            is_raw: s.is_raw,
        }
    }
}

/// A full draft. `bodyHash` is what a later save must echo back; it is the only
/// thing that gates a write, so the frontmatter may move underneath an open
/// editor without provoking a conflict.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftWire {
    pub id: String,
    pub title: String,
    pub status: DraftStatus,
    pub created_at: String,
    pub updated_at: String,
    pub body: String,
    pub body_hash: String,
    pub project: Option<ProjectLinkWire>,
    pub priority: Priority,
    /// `operator` while an override stands; the agent may not move it then.
    pub priority_source: PrioritySource,
    /// Every wave this draft has produced, so a second conversion can start
    /// from the last one rather than a blank form.
    pub conversions: Vec<serde_yaml::Value>,
    /// Present instead of the fields above when the file did not parse.
    pub raw: Option<RawWire>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawWire {
    pub text: String,
    pub error: String,
}

fn to_wire(view: InboxDraftView, link: Option<ProjectLink>) -> DraftWire {
    match view {
        InboxDraftView::Parsed(d) => DraftWire {
            body_hash: FileRevision::of_body(&d.body).body_hash,
            id: d.id,
            title: d.frontmatter.title,
            status: d.frontmatter.status,
            created_at: d.frontmatter.created_at,
            updated_at: d.frontmatter.updated_at,
            body: d.body,
            project: link.map(Into::into),
            priority: d.frontmatter.priority,
            priority_source: d.frontmatter.priority_source,
            conversions: d.frontmatter.conversions,
            raw: None,
        },
        InboxDraftView::Raw {
            id,
            raw_text,
            error,
        } => DraftWire {
            id,
            title: String::new(),
            status: DraftStatus::Draft,
            created_at: String::new(),
            updated_at: String::new(),
            body: String::new(),
            body_hash: String::new(),
            project: None,
            priority: Priority::default(),
            priority_source: PrioritySource::default(),
            conversions: Vec::new(),
            raw: Some(RawWire {
                text: raw_text,
                error,
            }),
        },
    }
}

fn draft_to_wire(draft: InboxDraft, link: Option<ProjectLink>) -> DraftWire {
    to_wire(InboxDraftView::Parsed(draft), link)
}

impl From<InboxServiceError> for ApiError {
    fn from(err: InboxServiceError) -> Self {
        use crate::adapters::inbox_store::InboxStoreError;
        match err {
            InboxServiceError::ConfirmationRequired(_) => ApiError::new(409, err.to_string()),
            InboxServiceError::Store(InboxStoreError::NotFound(_)) => {
                ApiError::new(404, err.to_string())
            }
            InboxServiceError::Store(InboxStoreError::InvalidId(_)) => {
                ApiError::new(400, err.to_string())
            }
            InboxServiceError::Store(InboxStoreError::Conflict { .. }) => {
                ApiError::new(409, err.to_string())
            }
            InboxServiceError::Store(InboxStoreError::Unparsed { .. }) => {
                ApiError::new(422, err.to_string())
            }
            InboxServiceError::Invalid(_) => ApiError::new(400, err.to_string()),
            InboxServiceError::TooLarge { .. } => ApiError::new(413, err.to_string()),
            InboxServiceError::RateLimited | InboxServiceError::TooManyOpenRequests(_) => {
                ApiError::new(429, err.to_string())
            }
            InboxServiceError::ForeignWorktree(_) => ApiError::new(403, err.to_string()),
            other => ApiError::new(500, other.to_string()),
        }
    }
}

/// Run the guard for this request, or turn its refusal into a 401/403.
///
/// `self_origin` is derived from the request's own `Host` header rather than a
/// configured value, so `localhost:5111` and `127.0.0.1:5111` both work without
/// the server having to know which one the browser used.
fn check(headers: &HeaderMap, method: &str) -> Result<(), ApiError> {
    let header = |name: axum::http::HeaderName| -> Option<String> {
        headers.get(name)?.to_str().ok().map(str::to_string)
    };
    let bearer = header(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.strip_prefix("Bearer ").map(str::to_string));
    let origin = header(axum::http::header::ORIGIN);
    let referer = header(axum::http::header::REFERER);
    let host = header(axum::http::header::HOST).unwrap_or_default();
    check_host(&host)?;
    let self_origin = format!("http://{host}");

    let expected =
        crate::adapters::control_token::load_control_token().map_err(|e| ApiError::new(500, e))?;

    match guard_inbox_request(
        method,
        bearer.as_deref(),
        origin.as_deref(),
        referer.as_deref(),
        &expected,
        &self_origin,
    ) {
        InboxGuard::Allow => Ok(()),
        InboxGuard::Deny(InboxGuardDenial::BadToken) => {
            Err(ApiError::new(401, "Unauthorized".to_string()))
        }
        InboxGuard::Deny(InboxGuardDenial::CrossOrigin) => Err(ApiError::new(
            403,
            "Cross-origin request refused".to_string(),
        )),
    }
}

/// Refuse a `Host` that is not this loopback daemon (T-10). The origin check
/// trusts `Host` to say what "same origin" means, so on its own it waves
/// through a DNS-rebinding page whose `Origin` and `Host` agree.
fn check_host(host: &str) -> Result<(), ApiError> {
    if host_is_allowed(Some(host), &allowed_hosts_from_env()) {
        Ok(())
    } else {
        Err(ApiError::new(403, "Host not allowed".to_string()))
    }
}

fn inbox(state: &AppState) -> Arc<InboxService> {
    state.inbox.clone()
}

use crate::server::AppState;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    pub search: Option<String>,
    pub include_dropped: Option<bool>,
}

/// `GET /api/inbox` — every draft, newest first.
pub async fn list_drafts(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let query = ListQuery {
        search: params.search,
        include_dropped: params.include_dropped.unwrap_or(false),
    };
    let drafts: Vec<DraftSummaryWire> = inbox(&state)
        .list(&query)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(serde_json::json!({ "drafts": drafts })))
}

/// `GET /api/inbox/{id}` — one draft, with its project link resolved.
pub async fn get_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DraftWire>, ApiError> {
    let (view, link) = inbox(&state).get(&id)?;
    Ok(Json(to_wire(view, link)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBody {
    pub title: String,
}

/// `POST /api/inbox` — a new, empty draft.
pub async fn create_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> Result<Json<DraftWire>, ApiError> {
    check(&headers, "POST")?;
    let draft = inbox(&state).create(&body.title)?;
    Ok(Json(draft_to_wire(draft, None)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBodyBody {
    pub expected_hash: String,
    pub body: String,
}

/// `PUT /api/inbox/{id}/body` — replace the body, gated on `expectedHash`.
/// A mismatch is a 409 and writes nothing.
pub async fn save_draft_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SaveBodyBody>,
) -> Result<Json<DraftWire>, ApiError> {
    check(&headers, "PUT")?;
    let svc = inbox(&state);
    let draft = svc.save_body(&id, &body.expected_hash, &body.body)?;
    let (_, link) = svc.get(&id)?;
    Ok(Json(draft_to_wire(draft, link)))
}

/// Frontmatter edits the editor owns. `projectPath: null` unlinks; omitting the
/// field leaves the link alone.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchBody {
    pub title: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub project_path: Option<Option<String>>,
    pub status: Option<DraftStatus>,
}

/// Distinguish `"projectPath": null` (unlink) from an absent key (no change).
fn double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(de)?))
}

/// `PATCH /api/inbox/{id}` — rename, (un)link a project, or drop.
pub async fn patch_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PatchBody>,
) -> Result<Json<DraftWire>, ApiError> {
    check(&headers, "PATCH")?;
    let svc = inbox(&state);

    if let Some(title) = body.title {
        svc.rename(&id, &title)?;
    }
    if let Some(project) = body.project_path {
        match project {
            Some(path) => {
                if !is_safe_project_path(&path) {
                    return Err(ApiError::new(400, format!("unsafe project path {path:?}")));
                }
                svc.link_project(&id, &path)?;
            }
            None => {
                svc.unlink_project(&id)?;
            }
        }
    }
    if let Some(status) = body.status {
        match status {
            DraftStatus::Dropped => {
                svc.drop_draft(&id)?;
            }
            // Promoted belongs to the conversion job, not to a client request.
            other => {
                return Err(ApiError::new(
                    400,
                    format!("status {other:?} is not settable here"),
                ));
            }
        }
    }

    let (view, link) = svc.get(&id)?;
    Ok(Json(to_wire(view, link)))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteParams {
    pub confirmed: Option<bool>,
}

/// `DELETE /api/inbox/{id}` — remove the file. A promoted draft needs
/// `?confirmed=true`, otherwise this is a 409 and nothing is written.
pub async fn delete_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<DeleteParams>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "DELETE")?;
    inbox(&state).delete(&id, params.confirmed.unwrap_or(false))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/inbox/session` — hands the SPA the control token it must send on
/// every mutating inbox call.
///
/// Safe over GET because this server mounts no CORS layer: a cross-origin page
/// can issue the request but the browser will not let it read the response. The
/// origin check is belt-and-braces, so the route fails closed if a permissive
/// CORS layer is ever added.
pub async fn inbox_session(headers: HeaderMap) -> Result<Json<serde_json::Value>, ApiError> {
    let header = |name: axum::http::HeaderName| -> Option<String> {
        headers.get(name)?.to_str().ok().map(str::to_string)
    };
    let host = header(axum::http::header::HOST).unwrap_or_default();
    check_host(&host)?;
    let self_origin = format!("http://{host}");
    if !origin_is_acceptable(
        header(axum::http::header::ORIGIN).as_deref(),
        header(axum::http::header::REFERER).as_deref(),
        &self_origin,
    ) {
        return Err(ApiError::new(
            403,
            "Cross-origin request refused".to_string(),
        ));
    }
    let token =
        crate::adapters::control_token::load_control_token().map_err(|e| ApiError::new(500, e))?;
    Ok(Json(serde_json::json!({ "token": token })))
}

// --- Conversion -----------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertBody {
    pub targets: Vec<ConversionTarget>,
}

/// `POST /api/inbox/{id}/convert` — start a fan-out, return its job id.
///
/// Returns immediately: `git worktree add` plus a tmux launch is multiple
/// seconds per target, far too long to hold a request open. Validation still
/// happens synchronously, so a bad request is a 400 here rather than a job that
/// fails a moment later.
pub async fn convert_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ConvertBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let svc = inbox(&state);

    // Validate before promising a job id. `convert` re-validates inside the
    // job, but a caller deserves the errors now, not by polling for them.
    let known: Vec<String> = state
        .manager
        .list()
        .iter()
        .map(|a| a.path.clone())
        .collect();
    let errors = validate_targets(&body.targets, &known);
    if !errors.is_empty() {
        let detail = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::new(400, detail));
    }
    // A draft that does not parse has no body to copy into a worktree.
    let (view, _) = svc.get(&id)?;

    // Advisory only — never a gate. A heuristic that can block work is a
    // heuristic that gets switched off.
    let mut advisories: Vec<Advisory> = Vec::new();
    if let InboxDraftView::Parsed(ref d) = view {
        advisories.extend(scan_for_secrets(&d.body));
    }
    for target in &body.targets {
        let sandboxed = state
            .manager
            .list()
            .iter()
            .find(|a| a.path == target.project_path)
            .map(|a| {
                let config = a.config();
                // The profile a converted worktree will actually launch under.
                let profile = common::services::config_view::get_default_profile_name(&config);
                config
                    .profiles
                    .get(&profile)
                    .map(|p| common::domain::config::is_sandboxed(p.runtime))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if let Some(warning) = sandbox_advisory(&target.branch, sandboxed) {
            advisories.push(warning);
        }
    }

    let job_id = state.inbox_jobs.start(&id, body.targets.len());
    let runner = ServerConversionRunner::new(state.clone());
    let jobs = state.inbox_jobs.clone();
    let targets = body.targets.clone();
    let draft_id = id.clone();
    let job = job_id.clone();

    // The fan-out is blocking (git, tmux), so it owns a blocking thread rather
    // than stalling the async runtime.
    tokio::task::spawn_blocking(move || {
        let span = tracing::info_span!("inbox_convert", job_id = %job, draft_id = %draft_id, targets = targets.len());
        let _enter = span.enter();
        match svc.convert_streaming(&draft_id, &targets, &runner, |outcome| {
            // The audit line: which draft produced which worktree, where, with
            // which agent, and when. Structured so it can be grepped after the
            // fact, which is the whole point of recording it.
            tracing::info!(
                audit = "inbox.convert.target",
                draft_id = %draft_id,
                branch = %outcome.branch,
                project = %outcome.project_path,
                agent = outcome.agent_id.as_deref().unwrap_or("default"),
                outcome = %outcome.outcome,
                at = %outcome.at,
                error = outcome.error.as_deref().unwrap_or(""),
                "inbox conversion target finished"
            );
            jobs.push_outcome(&job, outcome.clone());
        }) {
            Ok(_) => jobs.finish(&job),
            Err(e) => jobs.fail(&job, e.to_string()),
        }
    });

    Ok(Json(
        serde_json::json!({ "jobId": job_id, "advisories": advisories }),
    ))
}

/// `GET /api/inbox/jobs/{id}` — a job's state.
///
/// Exists for `sebenza-cli`, which is not a WebSocket client. An unknown id is
/// a 404, and since job ids are ULIDs that is the whole access check.
pub async fn get_conversion_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobSnapshot>, ApiError> {
    state
        .inbox_jobs
        .snapshot(&id)
        .map(Json)
        .ok_or_else(|| ApiError::new(404, "Unknown conversion job".to_string()))
}

/// `GET /api/inbox/{id}/conversions` — a draft's durable conversion history.
///
/// The job manager is in-memory and dies with the process, so after a restart
/// this is the only thing that still knows what a wave produced. It reads the
/// draft's own frontmatter, which is why each outcome is flushed as it happens.
pub async fn get_draft_conversions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let history = inbox(&state).conversion_history(&id)?;
    Ok(Json(serde_json::json!({ "conversions": history })))
}

/// `GET /api/inbox/jobs/{id}/stream` — live per-target progress.
///
/// The server's first non-project-prefixed WebSocket, because a conversion is
/// cross-project. An unknown id closes immediately: job ids are ULIDs, so that
/// is the access check, and this channel carries project paths and prompt text.
pub async fn ws_conversion_job(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let Some(subscription) = state.inbox_jobs.subscribe(&id) else {
        return (StatusCode::NOT_FOUND, "Unknown conversion job").into_response();
    };
    ws.on_upgrade(move |socket| conversion_job_socket(socket, subscription))
}

async fn conversion_job_socket(mut socket: WebSocket, subscription: JobSubscription) {
    // Replay first: a client attaching after the first target finished would
    // otherwise never learn of it.
    let snapshot = subscription.snapshot;
    let already_finished = snapshot.finished;
    if let Ok(text) = serde_json::to_string(&serde_json::json!({
        "type": "snapshot",
        "snapshot": snapshot,
    })) && socket.send(Message::Text(text.into())).await.is_err()
    {
        return;
    }
    if already_finished {
        return;
    }

    let mut receiver = subscription.receiver;
    loop {
        match receiver.recv().await {
            Ok(event) => {
                let Ok(text) = serde_json::to_string(&event) else {
                    continue;
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
                if matches!(event, JobEvent::Done { .. } | JobEvent::Failed { .. }) {
                    return;
                }
            }
            // Lagged: the snapshot route is authoritative, so drop rather than
            // stream a gap the client would silently treat as complete.
            Err(_) => return,
        }
    }
}

// --- Priority, comments and requests ---------------------------------------

/// The unauthenticated `X-Sebenza-Caller` marker (T-01), normalised.
fn caller_marker(headers: &HeaderMap) -> Option<String> {
    sanitize_caller_marker(
        headers
            .get("x-sebenza-caller")
            .and_then(|v| v.to_str().ok()),
    )
}

/// `PATCH /api/inbox/{id}/priority` — `{"priority": "P0"}` sets the operator
/// override; `{"priority": null}` clears it and hands control back to the
/// agent. The key is required, so an empty body cannot clear by accident.
pub async fn patch_priority(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<DraftWire>, ApiError> {
    check(&headers, "PATCH")?;
    let priority = match body.get("priority") {
        None => {
            return Err(ApiError::new(
                400,
                "priority is required; send null to clear the override".to_string(),
            ));
        }
        Some(serde_json::Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value::<Priority>(v.clone())
                .map_err(|_| ApiError::new(400, format!("unknown priority {v}")))?,
        ),
    };
    let svc = inbox(&state);
    svc.set_priority(&id, priority, caller_marker(&headers))?;
    let (view, link) = svc.get(&id)?;
    Ok(Json(to_wire(view, link)))
}

/// `GET /api/inbox/{id}/comments` — the overall thread plus one per worktree.
pub async fn list_comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CommentGroups>, ApiError> {
    Ok(Json(inbox(&state).list_comments(&id)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostCommentBody {
    pub body: String,
    /// A worktree thread; absent or null posts to the overall thread.
    #[serde(default)]
    pub worktree: Option<WorktreeKey>,
}

/// `POST /api/inbox/{id}/comments` — an operator comment.
pub async fn post_comment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PostCommentBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let thread = body
        .worktree
        .map(Thread::Worktree)
        .unwrap_or(Thread::Overall);
    // The token holder is the operator as far as the server can tell; the
    // marker records what the caller claimed to be (T-01).
    let author = EventAuthor {
        caller: caller_marker(&headers),
        ..EventAuthor::operator()
    };
    let svc = inbox(&state);
    let event = svc.add_comment(&id, author, thread, &body.body)?;
    Ok(Json(
        serde_json::json!({ "comment": svc.comment_view(&event) }),
    ))
}

/// `GET /api/inbox/{id}/requests` — every request, folded from the log.
pub async fn list_requests(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let requests = inbox(&state).list_requests(&id)?;
    Ok(Json(serde_json::json!({ "requests": requests })))
}

// --- Decisions, triage retry, agent jobs and redaction ------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmBody {
    /// The edited or operator-authored text; absent confirms the proposal.
    #[serde(default)]
    pub body: Option<String>,
    /// `content_hash` of the text the operator was shown (T-12).
    pub content_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectBody {
    pub reason: String,
}

fn not_yet() -> ApiError {
    ApiError::new(501, "not implemented".to_string())
}

/// `POST /api/inbox/{id}/requests/{rid}/confirm` — confirm and deliver.
pub async fn confirm_request(
    State(state): State<AppState>,
    Path((id, rid)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<ConfirmBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let _ = (state, id, rid, body);
    Err(not_yet())
}

/// `POST /api/inbox/{id}/requests/{rid}/reject` — back to open, with a reason.
pub async fn reject_request(
    State(state): State<AppState>,
    Path((id, rid)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<RejectBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let _ = (state, id, rid, body);
    Err(not_yet())
}

/// `POST /api/inbox/{id}/requests/{rid}/redeliver` — retry a failed delivery.
pub async fn redeliver_request(
    State(state): State<AppState>,
    Path((id, rid)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let _ = (state, id, rid);
    Err(not_yet())
}

/// `POST /api/inbox/{id}/requests/{rid}/retry-triage` — re-run triage.
pub async fn retry_triage(
    State(state): State<AppState>,
    Path((id, rid)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let _ = (state, id, rid);
    Err(not_yet())
}

/// `GET /api/inbox/{id}/agent/jobs/{jobId}` — one system-agent job.
pub async fn get_agent_job(
    State(state): State<AppState>,
    Path((id, job_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<crate::services::system_agent::JobRecord>, ApiError> {
    let _ = (state, id, job_id, headers);
    Err(not_yet())
}

/// `GET /api/inbox/{id}/agent/stream` — WebSocket of `inbox.job` events.
pub async fn ws_agent_jobs(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let _ = (ws, id, headers, state);
    not_yet().into_response()
}

/// `POST /api/inbox/{id}/comments/{eventId}/redact` — tombstone a body.
pub async fn redact_comment(
    State(state): State<AppState>,
    Path((id, event_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let _ = (state, id, event_id);
    Err(not_yet())
}

/// Handle a `/api/runtime/events` body if it is inbox ingress
/// (`sebenza-agentctl request|comment`). `None` when it is an ordinary
/// runtime event. The bearer check has already happened.
pub async fn inbox_runtime_event(
    state: &AppState,
    raw: &serde_json::Value,
) -> Option<Result<Json<serde_json::Value>, ApiError>> {
    let ingress = match parse_worktree_ingress(raw)? {
        Ok(ingress) => ingress,
        Err(msg) => return Some(Err(ApiError::new(400, msg))),
    };
    let svc = inbox(state);
    let outcome = tokio::task::spawn_blocking(move || svc.ingest(&ingress)).await;
    Some(match outcome {
        Err(_) => Err(ApiError::new(500, "task panicked".to_string())),
        Ok(Err(e)) => Err(e.into()),
        Ok(Ok(event)) => Ok(Json(serde_json::json!({
            "ok": true,
            "eventId": event.event_id,
            "requestId": request_id_of(&event.kind),
        }))),
    })
}

#[cfg(test)]
mod tests {
    //! Route-level tests: each handler is driven directly with a tempdir
    //! store, a pinned control token, and a capturing audit sink, so nothing
    //! touches `~/.ai/sebenza` or the operator's real token.

    use super::*;
    use crate::adapters::inbox_store::{FrontmatterAuthor, FrontmatterPatch, InboxStore};
    use crate::adapters::projects_registry::ProjectsRegistry;
    use crate::domain::inbox_events::{AuthorKind, InboxEventKind};
    use crate::services::inbox_convert::ConversionOutcome;
    use crate::services::inbox_limits::{InboxLimits, RateLimit};
    use crate::services::inbox_service::{AuditRecord, AuditSink};
    use axum::http::HeaderValue;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TOKEN: &str = "route-test-token";
    const HOST: &str = "127.0.0.1:5111";
    static SEQ: AtomicUsize = AtomicUsize::new(0);

    #[derive(Default)]
    struct Captured(Mutex<Vec<AuditRecord>>);
    impl AuditSink for Captured {
        fn record(&self, r: &AuditRecord) {
            self.0.lock().unwrap().push(r.clone());
        }
    }

    /// Records every paste; never touches tmux.
    #[derive(Default)]
    struct FakePane(Mutex<Vec<(crate::domain::inbox_events::WorktreeKey, String)>>);
    impl common::services::resolution_delivery::PaneSink for FakePane {
        fn send(
            &self,
            worktree: &crate::domain::inbox_events::WorktreeKey,
            text: &str,
        ) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push((worktree.clone(), text.to_string()));
            Ok(())
        }
    }

    struct Fixture {
        state: AppState,
        store: InboxStore,
        audit: Arc<Captured>,
        pane: Arc<FakePane>,
    }

    fn fixture_with(limits: InboxLimits) -> Fixture {
        crate::adapters::control_token::pin_control_token(TOKEN);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("sebenza-inbox-routes-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("temp base");
        let audit = Arc::new(Captured::default());
        let inbox = InboxService::new(
            InboxStore::with_dir(base.join("inbox")),
            ProjectsRegistry::with_file(base.join("projects.json")),
        )
        .with_limits(limits)
        .with_audit_sink(audit.clone());
        let pane = Arc::new(FakePane::default());
        inbox.set_pane_sink(pane.clone());
        let inbox = Arc::new(inbox);
        let agent_stream = Arc::new(crate::services::agent_stream::AgentStreamManager::new());
        // Disabled: route tests never spawn an agent.
        let system_agent = crate::services::system_agent::SystemAgentService::new(
            Default::default(),
            inbox.clone(),
            agent_stream.clone(),
            Default::default(),
        );
        let state = AppState {
            manager: Arc::new(crate::services::project_manager::ProjectManager::new(
                ProjectsRegistry::with_file(base.join("server-projects.json")),
                "http://127.0.0.1:5111".into(),
            )),
            terminal: Arc::new(crate::adapters::terminal::TerminalManager::new(0)),
            agent_stream,
            project_inits: Arc::new(
                crate::services::project_init_service::ProjectInitTracker::new(),
            ),
            inbox,
            inbox_jobs: Arc::new(crate::services::inbox_jobs::ConversionJobManager::new()),
            system_agent,
            frontend_dist: None,
        };
        Fixture {
            state,
            store: InboxStore::with_dir(base.join("inbox")),
            audit,
            pane,
        }
    }

    fn fixture() -> Fixture {
        fixture_with(InboxLimits::default())
    }

    /// A same-origin, token-bearing request, as the SPA sends it.
    fn good_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            "authorization",
            HeaderValue::from_static("Bearer route-test-token"),
        );
        h.insert("host", HeaderValue::from_static(HOST));
        h.insert("origin", HeaderValue::from_static("http://127.0.0.1:5111"));
        h
    }

    fn without(mut h: HeaderMap, name: &str) -> HeaderMap {
        h.remove(name);
        h
    }

    fn with(mut h: HeaderMap, name: &'static str, value: &'static str) -> HeaderMap {
        h.insert(name, HeaderValue::from_static(value));
        h
    }

    fn new_draft(f: &Fixture) -> String {
        f.state.inbox.create("Idea").expect("create").id
    }

    fn convert_into(f: &Fixture, id: &str, project: &str, branch: &str, path: &str) {
        let target = crate::services::inbox_convert::ConversionTarget {
            project_path: project.into(),
            branch: branch.into(),
            base_branch: None,
            agent_id: None,
            prompt: "go".into(),
        };
        let mut all = match f.store.get(id).expect("get") {
            InboxDraftView::Parsed(d) => d.frontmatter.conversions,
            _ => panic!("unparsed"),
        };
        all.push(
            serde_yaml::to_value(ConversionOutcome::created(&target, path.into(), "t".into()))
                .unwrap(),
        );
        f.store
            .merge_frontmatter(
                id,
                FrontmatterAuthor::Job,
                FrontmatterPatch {
                    conversions: Some(all),
                    ..Default::default()
                },
            )
            .expect("record conversion");
    }

    fn converted(f: &Fixture) -> String {
        let id = new_draft(f);
        convert_into(f, &id, "/code/acme-demo", "feat-x", "/wt/acme-demo/feat-x");
        id
    }

    async fn patch(
        f: &Fixture,
        id: &str,
        h: HeaderMap,
        body: serde_json::Value,
    ) -> Result<DraftWire, ApiError> {
        patch_priority(State(f.state.clone()), Path(id.to_string()), h, Json(body))
            .await
            .map(|Json(w)| w)
    }

    async fn comment(
        f: &Fixture,
        id: &str,
        h: HeaderMap,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ApiError> {
        let body: PostCommentBody = serde_json::from_value(body).expect("comment body");
        post_comment(State(f.state.clone()), Path(id.to_string()), h, Json(body))
            .await
            .map(|Json(v)| v)
    }

    /// POST a raw body to `/api/runtime/events`, as agentctl does.
    async fn runtime(
        f: &Fixture,
        bearer: Option<&str>,
        raw: Vec<u8>,
    ) -> Result<serde_json::Value, ApiError> {
        let mut h = HeaderMap::new();
        if let Some(t) = bearer {
            h.insert(
                "authorization",
                HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
            );
        }
        crate::server::runtime_event(State(f.state.clone()), h, axum::body::Bytes::from(raw))
            .await
            .map(|Json(v)| v)
    }

    fn request_payload(id: &str, path: &str, branch: &str, body: &str) -> Vec<u8> {
        serde_json::json!({
            "type": "inbox.request",
            "worktreeId": "wt-id-1",
            "branch": branch,
            "draftId": id,
            "worktreePath": path,
            "title": "Need a decision",
            "body": body,
            "caller": "worktree",
        })
        .to_string()
        .into_bytes()
    }

    fn status(r: Result<impl Sized, ApiError>) -> u16 {
        match r {
            Ok(_) => 200,
            Err(e) => e.status.as_u16(),
        }
    }

    // --- TS-06: priority ------------------------------------------------------

    #[tokio::test]
    async fn priority_override_then_clear() {
        let f = fixture();
        let id = new_draft(&f);
        let w = patch(
            &f,
            &id,
            good_headers(),
            serde_json::json!({"priority": "P0"}),
        )
        .await
        .expect("set");
        assert_eq!(w.priority, Priority::P0);
        assert_eq!(w.priority_source, PrioritySource::Operator);

        let w = patch(
            &f,
            &id,
            good_headers(),
            serde_json::json!({"priority": null}),
        )
        .await
        .expect("clear");
        assert_eq!(w.priority, Priority::P0);
        assert_eq!(
            w.priority_source,
            PrioritySource::Agent,
            "control is back with the agent"
        );

        let (view, _) = f.state.inbox.get(&id).expect("get");
        let InboxDraftView::Parsed(d) = view else {
            panic!("parsed")
        };
        assert_eq!(d.frontmatter.priority_source, PrioritySource::Agent);

        let actions: Vec<_> = f.audit.0.lock().unwrap().iter().map(|r| r.action).collect();
        assert_eq!(
            actions,
            ["inbox.priority.changed", "inbox.priority.override_cleared"]
        );
    }

    #[tokio::test]
    async fn a_priority_patch_without_the_key_or_with_a_bad_value_is_a_400() {
        let f = fixture();
        let id = new_draft(&f);
        assert_eq!(
            status(patch(&f, &id, good_headers(), serde_json::json!({})).await),
            400
        );
        assert_eq!(
            status(
                patch(
                    &f,
                    &id,
                    good_headers(),
                    serde_json::json!({"priority": "P9"})
                )
                .await
            ),
            400
        );
    }

    #[tokio::test]
    async fn the_list_and_the_item_carry_priority_and_sort_by_it() {
        let f = fixture();
        let first = new_draft(&f);
        let _second = new_draft(&f);
        patch(
            &f,
            &first,
            good_headers(),
            serde_json::json!({"priority": "P0"}),
        )
        .await
        .expect("set");

        let Json(list) = list_drafts(State(f.state.clone()), Query(ListParams::default()))
            .await
            .expect("list");
        let drafts = list["drafts"].as_array().expect("drafts");
        assert_eq!(drafts[0]["id"], first.as_str(), "P0 sorts first");
        assert_eq!(drafts[0]["priority"], "P0");
        assert_eq!(drafts[0]["prioritySource"], "operator");
        assert_eq!(drafts[1]["priority"], "P2");

        let Json(item) = get_draft(State(f.state.clone()), Path(first.clone()))
            .await
            .expect("get");
        assert_eq!(item.priority, Priority::P0);
    }

    // --- TS-32: the token in a worktree's hands (accepted T-01) --------------

    #[tokio::test]
    async fn a_worktree_caller_with_the_token_can_set_priority_and_is_marked() {
        let f = fixture();
        let id = new_draft(&f);
        // What a worktree agent could do with the token it holds: no Origin,
        // its own marker. Accepted residual risk, so this succeeds.
        let h = with(
            without(good_headers(), "origin"),
            "x-sebenza-caller",
            "worktree",
        );
        patch(&f, &id, h, serde_json::json!({"priority": "P0"}))
            .await
            .expect("accepted: succeeds");

        let records = f.audit.0.lock().unwrap().clone();
        assert_eq!(records[0].actor, AuthorKind::Operator);
        assert_eq!(records[0].caller.as_deref(), Some("worktree"));
        let events = f.state.inbox.events(&id).expect("events");
        assert_eq!(events.last().unwrap().caller.as_deref(), Some("worktree"));
    }

    // --- TS-41: guard on every new mutating route ----------------------------

    #[tokio::test]
    async fn new_routes_refuse_a_missing_token_a_foreign_origin_and_a_bad_host() {
        let f = fixture();
        let id = new_draft(&f);
        let cases = [
            (without(good_headers(), "authorization"), 401),
            (with(good_headers(), "authorization", "Bearer wrong"), 401),
            (with(good_headers(), "origin", "http://evil.test"), 403),
            // DNS rebinding: Host and Origin agree, but neither is us.
            (
                with(
                    with(good_headers(), "host", "evil.test:5111"),
                    "origin",
                    "http://evil.test:5111",
                ),
                403,
            ),
            (without(good_headers(), "host"), 403),
        ];
        for (h, want) in cases {
            assert_eq!(
                status(patch(&f, &id, h.clone(), serde_json::json!({"priority": "P0"})).await),
                want,
                "PATCH priority {h:?}"
            );
            assert_eq!(
                status(comment(&f, &id, h.clone(), serde_json::json!({"body": "hi"})).await),
                want,
                "POST comments {h:?}"
            );
        }
        assert!(
            f.state.inbox.events(&id).expect("events").is_empty(),
            "nothing written"
        );
    }

    #[tokio::test]
    async fn the_token_route_refuses_a_rebinding_host() {
        let _f = fixture();
        let h = with(
            with(HeaderMap::new(), "host", "evil.test:5111"),
            "origin",
            "http://evil.test:5111",
        );
        assert_eq!(status(inbox_session(h).await), 403);
        let ok = with(HeaderMap::new(), "host", HOST);
        assert_eq!(status(inbox_session(ok).await), 200);
    }

    // --- comments -------------------------------------------------------------

    #[tokio::test]
    async fn an_operator_comment_round_trips_into_its_group() {
        let f = fixture();
        let id = converted(&f);
        comment(
            &f,
            &id,
            good_headers(),
            serde_json::json!({"body": "overall"}),
        )
        .await
        .expect("overall");
        let v = comment(
            &f,
            &id,
            good_headers(),
            serde_json::json!({"body": "here", "worktree": {"project": "/code/acme-demo", "branch": "feat-x"}}),
        )
        .await
        .expect("worktree");
        assert_eq!(v["comment"]["body"], "here");

        let Json(groups) = list_comments(State(f.state.clone()), Path(id.clone()))
            .await
            .expect("groups");
        assert_eq!(groups.overall.len(), 1);
        assert_eq!(groups.worktrees[0].comments[0].body, "here");

        let stray = comment(
            &f,
            &id,
            good_headers(),
            serde_json::json!({"body": "x", "worktree": {"project": "/nope", "branch": "b"}}),
        )
        .await;
        assert_eq!(status(stray), 400);
    }

    #[tokio::test]
    async fn an_oversized_comment_is_a_413() {
        let f = fixture();
        let id = new_draft(&f);
        let huge = "x".repeat(crate::services::inbox_limits::MAX_BODY_BYTES + 1);
        assert_eq!(
            status(comment(&f, &id, good_headers(), serde_json::json!({"body": huge})).await),
            413
        );
    }

    #[tokio::test]
    async fn an_unknown_draft_is_a_404() {
        let f = fixture();
        let missing = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        assert_eq!(
            status(list_comments(State(f.state.clone()), Path(missing.into())).await),
            404
        );
        assert_eq!(
            status(list_requests(State(f.state.clone()), Path(missing.into())).await),
            404
        );
    }

    // --- TS-11 / TS-12 / TS-37 / TS-13: agentctl ingress ----------------------

    #[tokio::test]
    async fn an_agentctl_request_lands_open_in_its_worktree_group() {
        let f = fixture();
        let id = converted(&f);
        let v = runtime(
            &f,
            Some(TOKEN),
            request_payload(&id, "/wt/acme-demo/feat-x", "feat-x", "Which db?"),
        )
        .await
        .expect("accepted");
        assert_eq!(v["ok"], true);
        assert!(v["requestId"].is_string());

        let Json(list) = list_requests(State(f.state.clone()), Path(id.clone()))
            .await
            .expect("requests");
        let r = &list["requests"][0];
        assert_eq!(r["status"], "open");
        assert_eq!(r["worktree"]["project"], "/code/acme-demo");
        assert_eq!(r["worktree"]["branch"], "feat-x");

        let events = f.state.inbox.events(&id).expect("events");
        assert_eq!(events[0].author, AuthorKind::WorktreeAgent);
        assert!(matches!(
            events[0].kind,
            InboxEventKind::RequestOpened { .. }
        ));
    }

    #[tokio::test]
    async fn an_agentctl_comment_lands_in_its_worktree_group() {
        let f = fixture();
        let id = converted(&f);
        let raw = serde_json::json!({
            "type": "inbox.comment", "worktreeId": "wt-id-1", "branch": "feat-x",
            "draftId": id, "worktreePath": "/wt/acme-demo/feat-x", "body": "progress",
        });
        runtime(&f, Some(TOKEN), raw.to_string().into_bytes())
            .await
            .expect("accepted");
        let groups = f.state.inbox.list_comments(&id).expect("groups");
        assert_eq!(groups.worktrees[0].comments[0].body, "progress");
    }

    #[tokio::test]
    async fn ingress_without_the_token_is_a_401() {
        let f = fixture();
        let id = converted(&f);
        let r = runtime(
            &f,
            None,
            request_payload(&id, "/wt/acme-demo/feat-x", "feat-x", "b"),
        )
        .await;
        assert_eq!(status(r), 401);
        assert!(f.state.inbox.events(&id).expect("events").is_empty());
    }

    #[tokio::test]
    async fn a_request_from_a_worktree_not_in_conversions_is_refused() {
        let f = fixture();
        let id = converted(&f);
        let r = runtime(
            &f,
            Some(TOKEN),
            request_payload(&id, "/wt/rogue", "feat-x", "b"),
        )
        .await;
        assert_eq!(status(r), 403);
        assert!(f.state.inbox.events(&id).expect("events").is_empty());
    }

    #[tokio::test]
    async fn a_forged_origin_naming_another_item_is_refused() {
        let f = fixture();
        let _mine = converted(&f);
        let theirs = new_draft(&f);
        convert_into(&f, &theirs, "/code/beta", "feat-y", "/wt/beta/feat-y");
        let r = runtime(
            &f,
            Some(TOKEN),
            request_payload(&theirs, "/wt/acme-demo/feat-x", "feat-x", "b"),
        )
        .await;
        assert_eq!(status(r), 403);
        assert!(f.state.inbox.events(&theirs).expect("events").is_empty());
    }

    #[tokio::test]
    async fn a_flood_is_capped_by_size_rate_and_depth() {
        let f = fixture_with(InboxLimits {
            requests: RateLimit {
                max: 2,
                window: std::time::Duration::from_secs(60),
            },
            ..InboxLimits::default()
        });
        let id = converted(&f);
        let ok = || request_payload(&id, "/wt/acme-demo/feat-x", "feat-x", "b");

        let huge = "x".repeat(crate::services::inbox_limits::MAX_INGRESS_BYTES + 1);
        let r = runtime(
            &f,
            Some(TOKEN),
            request_payload(&id, "/wt/acme-demo/feat-x", "feat-x", &huge),
        )
        .await;
        assert_eq!(status(r), 413, "raw payload over the ingress cap");

        let body_over = "x".repeat(crate::services::inbox_limits::MAX_BODY_BYTES + 1);
        let r = runtime(
            &f,
            Some(TOKEN),
            request_payload(&id, "/wt/acme-demo/feat-x", "feat-x", &body_over),
        )
        .await;
        assert_eq!(status(r), 413, "body over the field cap");

        assert_eq!(status(runtime(&f, Some(TOKEN), ok()).await), 200);
        assert_eq!(status(runtime(&f, Some(TOKEN), ok()).await), 200);
        assert_eq!(status(runtime(&f, Some(TOKEN), ok()).await), 429);
        assert_eq!(f.state.inbox.list_requests(&id).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_malformed_inbox_event_is_a_400() {
        let f = fixture();
        let raw = serde_json::json!({"type": "inbox.request", "branch": "b"});
        assert_eq!(
            status(runtime(&f, Some(TOKEN), raw.to_string().into_bytes()).await),
            400
        );
    }

    // --- Decisions, retry, jobs and redaction (phase 4) -----------------------

    /// A converted item with one request from feat-x, proposed by triage.
    fn proposed(f: &Fixture, text: &str) -> (String, String, String) {
        let id = converted(f);
        let e = f
            .state
            .inbox
            .open_request(
                &id,
                crate::adapters::inbox_store::EventAuthor::worktree_agent(),
                crate::domain::inbox_events::WorktreeKey {
                    project: "/code/acme-demo".into(),
                    branch: "feat-x".into(),
                },
                "Need a decision",
                "Which loader?",
            )
            .expect("request");
        let rid = request_id_of(&e.kind).unwrap().to_string();
        f.state
            .inbox
            .record_proposal(&id, &rid, text, "r")
            .expect("proposal")
            .expect("open");
        let hash = f
            .state
            .inbox
            .request(&id, &rid)
            .unwrap()
            .proposal_hash
            .unwrap();
        (id, rid, hash)
    }

    fn ids(id: &str, rid: &str) -> Path<(String, String)> {
        Path((id.to_string(), rid.to_string()))
    }

    async fn confirm(
        f: &Fixture,
        id: &str,
        rid: &str,
        h: HeaderMap,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ApiError> {
        let body: ConfirmBody = serde_json::from_value(body).expect("confirm body");
        confirm_request(State(f.state.clone()), ids(id, rid), h, Json(body))
            .await
            .map(|Json(v)| v)
    }

    async fn reject(
        f: &Fixture,
        id: &str,
        rid: &str,
        h: HeaderMap,
        reason: &str,
    ) -> Result<serde_json::Value, ApiError> {
        reject_request(
            State(f.state.clone()),
            ids(id, rid),
            h,
            Json(RejectBody {
                reason: reason.into(),
            }),
        )
        .await
        .map(|Json(v)| v)
    }

    // TS-25 over HTTP: confirm delivers once and returns the resolved request.
    #[tokio::test]
    async fn confirm_delivers_and_returns_the_request() {
        let f = fixture();
        let (id, rid, hash) = proposed(&f, "Use the loader.");
        let v = confirm(
            &f,
            &id,
            &rid,
            good_headers(),
            serde_json::json!({"contentHash": hash}),
        )
        .await
        .expect("confirm");
        assert_eq!(v["request"]["status"], "resolved");
        assert_eq!(v["request"]["attempts"], 1);
        assert_eq!(f.pane.0.lock().unwrap().len(), 1);
    }

    // TS-42 / TS-31 over HTTP: a stale or tampered hash is a 409; nothing sent.
    #[tokio::test]
    async fn confirm_with_the_wrong_hash_is_a_409() {
        let f = fixture();
        let (id, rid, _) = proposed(&f, "Use the loader.");
        let r = confirm(
            &f,
            &id,
            &rid,
            good_headers(),
            serde_json::json!({"contentHash": "deadbeef"}),
        )
        .await;
        assert_eq!(status(r), 409);
        assert!(f.pane.0.lock().unwrap().is_empty());
    }

    // TS-61 over HTTP: no proposal; the operator authors the resolution.
    #[tokio::test]
    async fn an_authored_resolution_is_confirmed_over_http() {
        let f = fixture();
        let (id, rid, hash) = proposed(&f, "Use the loader.");
        reject(&f, &id, &rid, good_headers(), "no")
            .await
            .expect("reject");
        let text = "Use tests/helpers/loader.rs";
        let v = confirm(
            &f,
            &id,
            &rid,
            good_headers(),
            serde_json::json!({
                "body": text,
                "contentHash": crate::domain::inbox_events::content_hash(text),
            }),
        )
        .await
        .expect("confirm");
        assert_eq!(v["request"]["status"], "resolved");
        assert_eq!(v["request"]["confirmedText"], text);
        let _ = hash;
    }

    // TS-32 (confirm half): a worktree caller with the token is accepted
    // (T-01) and audited as operator with its marker.
    #[tokio::test]
    async fn a_worktree_caller_with_the_token_can_confirm_and_is_marked() {
        let f = fixture();
        let (id, rid, hash) = proposed(&f, "Use the loader.");
        let h = with(good_headers(), "x-sebenza-caller", "worktree");
        confirm(&f, &id, &rid, h, serde_json::json!({"contentHash": hash}))
            .await
            .expect("accepted residual T-01");
        let audit = f.audit.0.lock().unwrap();
        let rec = audit
            .iter()
            .find(|r| r.action == "inbox.resolution.confirmed")
            .expect("audited");
        assert_eq!(rec.actor, AuthorKind::Operator);
        assert_eq!(rec.caller.as_deref(), Some("worktree"));
    }

    // TS-29 over HTTP.
    #[tokio::test]
    async fn reject_reopens_with_the_reason() {
        let f = fixture();
        let (id, rid, _) = proposed(&f, "Use the loader.");
        let v = reject(&f, &id, &rid, good_headers(), "Wrong loader")
            .await
            .expect("reject");
        assert_eq!(v["request"]["status"], "open");
        assert_eq!(v["request"]["lastReason"], "Wrong loader");
        assert_eq!(status(reject(&f, &id, &rid, good_headers(), "").await), 400);
        assert!(f.pane.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn redeliver_needs_a_failed_delivery_and_unknown_requests_are_404() {
        let f = fixture();
        let (id, rid, _) = proposed(&f, "Use the loader.");
        let r = redeliver_request(State(f.state.clone()), ids(&id, &rid), good_headers()).await;
        assert_eq!(status(r), 409);
        let r = redeliver_request(State(f.state.clone()), ids(&id, "ghost"), good_headers()).await;
        assert_eq!(status(r), 404);
        assert!(f.pane.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn retry_triage_with_the_agent_disabled_is_a_503() {
        let f = fixture();
        let (id, rid, _) = proposed(&f, "x");
        f.state.inbox.reject_request(&id, &rid, "no", None).unwrap();
        let r = retry_triage(State(f.state.clone()), ids(&id, &rid), good_headers()).await;
        assert_eq!(status(r), 503);
        let r = retry_triage(State(f.state.clone()), ids(&id, "ghost"), good_headers()).await;
        assert_eq!(status(r), 503, "the kill switch is checked first");
    }

    #[tokio::test]
    async fn an_unknown_agent_job_is_a_404_and_a_bad_host_a_403() {
        let f = fixture();
        let id = new_draft(&f);
        let r = get_agent_job(State(f.state.clone()), ids(&id, "01NOJOB"), good_headers()).await;
        assert_eq!(status(r), 404);
        let h = with(good_headers(), "host", "evil.test:5111");
        let r = get_agent_job(State(f.state.clone()), ids(&id, "01NOJOB"), h).await;
        assert_eq!(status(r), 403);
    }

    // TS-62 over HTTP: redaction masks the comment in the API.
    #[tokio::test]
    async fn redact_masks_the_comment() {
        let f = fixture();
        let id = new_draft(&f);
        let v = comment(
            &f,
            &id,
            good_headers(),
            serde_json::json!({"body": "oops sk-TEST-0000000000000000000000"}),
        )
        .await
        .expect("comment");
        assert_eq!(v["comment"]["warnings"][0], "OpenAI-style key");
        let event_id = v["comment"]["eventId"].as_str().unwrap().to_string();
        let r = redact_comment(State(f.state.clone()), ids(&id, &event_id), good_headers())
            .await
            .map(|Json(v)| v)
            .expect("redact");
        assert_eq!(r["targetEventId"], event_id);
        let groups = f.state.inbox.list_comments(&id).unwrap();
        assert_eq!(
            groups.overall[0].body,
            crate::domain::inbox_events::REDACTED_BODY
        );
        let r = redact_comment(State(f.state.clone()), ids(&id, "01GHOST"), good_headers()).await;
        assert_eq!(status(r), 404);
    }

    // TS-41: every new mutating route refuses a missing token, a foreign
    // origin and a bad host, and writes nothing.
    #[tokio::test]
    async fn decision_routes_refuse_a_missing_token_a_foreign_origin_and_a_bad_host() {
        let f = fixture();
        let (id, rid, hash) = proposed(&f, "Use the loader.");
        let before = f.store.read_events(&id).unwrap().len();
        let cases = [
            (without(good_headers(), "authorization"), 401),
            (with(good_headers(), "origin", "http://evil.test"), 403),
            (
                with(
                    with(good_headers(), "host", "evil.test:5111"),
                    "origin",
                    "http://evil.test:5111",
                ),
                403,
            ),
        ];
        for (h, want) in cases {
            let body = serde_json::json!({"contentHash": hash});
            assert_eq!(status(confirm(&f, &id, &rid, h.clone(), body).await), want);
            assert_eq!(status(reject(&f, &id, &rid, h.clone(), "no").await), want);
            let r = redeliver_request(State(f.state.clone()), ids(&id, &rid), h.clone()).await;
            assert_eq!(status(r), want);
            let r = retry_triage(State(f.state.clone()), ids(&id, &rid), h.clone()).await;
            assert_eq!(status(r), want);
            let r = redact_comment(State(f.state.clone()), ids(&id, &rid), h.clone()).await;
            assert_eq!(status(r), want);
        }
        assert_eq!(
            f.store.read_events(&id).unwrap().len(),
            before,
            "nothing written"
        );
        assert!(f.pane.0.lock().unwrap().is_empty());
    }

    /// TS-31: enumerate every inbox route the router mounts. Only the
    /// confirm and redeliver handlers call into delivery, and they are
    /// mounted only at the confirm and redeliver paths; nothing else in the
    /// server calls confirm or redeliver.
    #[test]
    fn only_confirm_and_redeliver_routes_reach_delivery() {
        let server = include_str!("server.rs");
        let mut routes: Vec<(String, Vec<String>)> = Vec::new();
        for chunk in server.split(".route(").skip(1) {
            let Some(start) = chunk.find('"') else {
                continue;
            };
            let path: String = chunk[start + 1..]
                .chars()
                .take_while(|c| *c != '"')
                .collect();
            if !path.starts_with("/api/inbox") {
                continue;
            }
            let handlers = chunk
                .match_indices("crate::inbox_routes::")
                .map(|(i, m)| {
                    chunk[i + m.len()..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                })
                .collect();
            routes.push((path, handlers));
        }
        assert!(routes.len() >= 16, "found {} inbox routes", routes.len());

        let src = include_str!("inbox_routes.rs");
        let src = &src[..src.find("#[cfg(test)]").unwrap()];
        let delivering: Vec<String> = src
            .split("pub async fn ")
            .skip(1)
            .filter(|body| body.contains(".confirm_resolution(") || body.contains(".redeliver("))
            .map(|body| {
                body.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect()
            })
            .collect();
        assert_eq!(delivering, ["confirm_request", "redeliver_request"]);
        for (path, handlers) in &routes {
            for h in handlers.iter().filter(|h| delivering.contains(h)) {
                assert!(
                    path.ends_with("/confirm") || path.ends_with("/redeliver"),
                    "{h} mounted at {path}"
                );
            }
        }
        for (name, other) in [
            ("server.rs", server),
            ("inbox_runner.rs", include_str!("inbox_runner.rs")),
            ("main.rs", include_str!("main.rs")),
            (
                "system_agent/mod.rs",
                include_str!("services/system_agent/mod.rs"),
            ),
            (
                "system_agent/apply.rs",
                include_str!("services/system_agent/apply.rs"),
            ),
            (
                "pane_delivery.rs",
                include_str!("services/pane_delivery.rs"),
            ),
        ] {
            for call in [".confirm_resolution(", ".redeliver("] {
                assert!(!other.contains(call), "{name} calls {call}");
            }
        }
    }
}
