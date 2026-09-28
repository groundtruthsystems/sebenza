//! The `/api/inbox` hub routes.
//!
//! Global, not project-prefixed: a draft may exist before it belongs to any
//! project. That breadth is exactly why every mutating route here runs through
//! [`guard_inbox_request`] — a control token *and* a same-origin check, neither
//! substituting for the other.

use crate::domain::model::{DraftStatus, FileRevision, InboxDraft, InboxDraftView};
use crate::domain::policies::{
    InboxGuard, InboxGuardDenial, guard_inbox_request, is_safe_project_path, origin_is_acceptable,
};
use crate::inbox_runner::ServerConversionRunner;
use crate::services::inbox_convert::{ConversionTarget, validate_targets};
use crate::services::inbox_jobs::JobSnapshot;
use crate::services::inbox_service::{
    DraftSummary, InboxService, InboxServiceError, ListQuery, ProjectLink,
};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Json;
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
    svc.get(&id)?;

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
            tracing::info!(
                branch = %outcome.branch,
                project = %outcome.project_path,
                outcome = %outcome.outcome,
                "inbox conversion target finished"
            );
            jobs.push_outcome(&job, outcome.clone());
        }) {
            Ok(_) => jobs.finish(&job),
            Err(e) => jobs.fail(&job, e.to_string()),
        }
    });

    Ok(Json(serde_json::json!({ "jobId": job_id })))
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
