//! Solution-level routes (application-architecture.md, Interfaces). Reads
//! need the inbox token; every action that runs an agent, edits a document or
//! decides a problem is operator-only ([`require_operator`]).

use crate::inbox_routes::check;
use crate::operator_routes::require_operator;
use crate::server::{ApiError, AppState};
use crate::services::solution::SolutionError;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

impl From<SolutionError> for ApiError {
    fn from(e: SolutionError) -> Self {
        use crate::services::role_runner::RoleJobError;
        match e {
            SolutionError::NotSolution(_) => ApiError::coded(409, "not_solution", e.to_string()),
            SolutionError::WrongPhase { .. } => ApiError::coded(409, "wrong_phase", e.to_string()),
            SolutionError::Busy => ApiError::coded(409, "busy", e.to_string()),
            SolutionError::RoundLimit { .. } => ApiError::coded(409, "round_limit", e.to_string()),
            SolutionError::NotIndependent(_) => {
                ApiError::coded(422, "not_independent", e.to_string())
            }
            SolutionError::ScanWarning(_) => ApiError::coded(422, "scan_warning", e.to_string()),
            SolutionError::TooLarge => ApiError::coded(413, "too_large", e.to_string()),
            SolutionError::NotFound(_) => ApiError::new(404, e.to_string()),
            SolutionError::ApprovalRequired(_) => {
                ApiError::coded(409, "approval_required", e.to_string())
            }
            SolutionError::AlreadyConverted(_) => {
                ApiError::coded(409, "already_converted", e.to_string())
            }
            SolutionError::Agent(RoleJobError::Refused(_)) => {
                ApiError::coded(422, "agent_refused", e.to_string())
            }
            SolutionError::Agent(_) => ApiError::coded(502, "agent_failed", e.to_string()),
            SolutionError::Inbox(i) => i.into(),
        }
    }
}

/// `GET /api/inbox/{id}/solution`
pub async fn get_solution(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "GET")?;
    Ok(Json(state.solution.view(&id)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnBody {
    pub message: String,
    /// Send even though the scan flagged the message (warn-and-confirm).
    #[serde(default)]
    pub confirm_warnings: bool,
}

/// `POST /api/inbox/{id}/coordinator/turn` — operator only; 202 with the
/// turn id. The reply lands in the transcript.
pub async fn coordinator_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<TurnBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let class = require_operator(&state, &headers, "coordinator.turn", Some(&id))?;
    let turn_id =
        state
            .solution
            .coordinator_turn(&id, &body.message, body.confirm_warnings, class)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "turnId": turn_id })),
    ))
}

/// `GET /api/inbox/{id}/coordinator/transcript`
pub async fn coordinator_transcript(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "GET")?;
    let svc = &state.solution;
    let entries =
        svc.inbox().store().read_transcript(&id).map_err(|e| {
            ApiError::from(crate::services::inbox_service::InboxServiceError::from(e))
        })?;
    Ok(Json(
        serde_json::json!({ "entries": entries, "job": svc.job(&id) }),
    ))
}

/// `POST /api/inbox/{id}/coordinator/proposals/{turn}/{index}/apply`
pub async fn apply_proposal(
    State(state): State<AppState>,
    Path((id, turn, index)): Path<(String, String, usize)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "coordinator.proposal.applied", Some(&id))?;
    Ok(Json(
        state.solution.apply_proposal(&id, &turn, index, class)?,
    ))
}

/// `POST /api/inbox/{id}/solution/start-design` — operator only (UC-03).
pub async fn start_design(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let class = require_operator(&state, &headers, "solution.design.started", Some(&id))?;
    state.solution.start_design(&id, class)?;
    Ok((StatusCode::ACCEPTED, Json(state.solution.view(&id)?)))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReviewBody {
    #[serde(default)]
    pub new_cycle: bool,
    // Any other field (a client-chosen `round`, say) is ignored: rounds come
    // from the log, never the client (TS-44).
}

/// `POST /api/inbox/{id}/solution/review` — operator only (UC-04).
pub async fn review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<ReviewBody>>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let class = require_operator(&state, &headers, "solution.review", Some(&id))?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    state.solution.review(&id, body.new_cycle, class)?;
    Ok((StatusCode::ACCEPTED, Json(state.solution.view(&id)?)))
}

/// `GET /api/inbox/{id}/solution/docs/{*name}`
pub async fn get_doc(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "GET")?;
    let doc = state
        .solution
        .inbox()
        .store()
        .read_solution_doc(&id, &name)
        .map_err(|e| ApiError::from(crate::services::inbox_service::InboxServiceError::from(e)))?
        .ok_or_else(|| ApiError::new(404, format!("no document {name}")))?;
    Ok(Json(
        serde_json::json!({ "name": name, "content": doc.0, "hash": doc.1 }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocBody {
    pub content: String,
    /// The SHA-256 last read; empty creates a new document.
    #[serde(default)]
    pub expected_hash: String,
}

/// `PUT /api/inbox/{id}/solution/docs/{*name}` — operator only; a stale hash
/// is 409 `doc_conflict` and nothing is written.
pub async fn put_doc(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<DocBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "solution.doc.edited", Some(&id))?;
    let hash = state
        .solution
        .edit_doc(&id, &name, &body.content, &body.expected_hash, class)?;
    Ok(Json(serde_json::json!({ "name": name, "hash": hash })))
}

#[derive(Debug, Deserialize, Default)]
pub struct ResolveBody {
    #[serde(default)]
    pub note: String,
}

/// `POST /api/inbox/{id}/solution/problems/{pid}/resolve` — operator only.
pub async fn resolve_problem(
    State(state): State<AppState>,
    Path((id, pid)): Path<(String, String)>,
    headers: HeaderMap,
    body: Option<Json<ResolveBody>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "solution.problem.resolved", Some(&id))?;
    let note = body.map(|Json(b)| b.note).unwrap_or_default();
    Ok(Json(serde_json::json!({
        "request": state.solution.resolve_problem(&id, &pid, &note, class)?
    })))
}

/// `POST /api/inbox/{id}/solution/approve-design` — operator only.
pub async fn approve_design(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "solution.design.approved", Some(&id))?;
    let svc = state.solution.clone();
    Ok(Json(
        tokio::task::spawn_blocking(move || svc.approve_design(&id, class))
            .await
            .map_err(|_| ApiError::new(500, "task panicked".into()))??,
    ))
}

/// `POST /api/inbox/{id}/handover/draft` — operator only.
pub async fn draft_handovers(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let class = require_operator(&state, &headers, "solution.handover.drafted", Some(&id))?;
    state.solution.draft_handovers(&id, class)?;
    Ok((StatusCode::ACCEPTED, Json(state.solution.view(&id)?)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverEditBody {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub expected_hash: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
}

/// `PUT /api/inbox/{id}/handover/{project}` — operator only.
pub async fn edit_handover(
    State(state): State<AppState>,
    Path((id, project)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<HandoverEditBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "solution.handover.edited", Some(&id))?;
    Ok(Json(state.solution.edit_handover(
        &id,
        &project,
        body.content.as_deref(),
        &body.expected_hash,
        body.branch.as_deref(),
        body.base.as_deref(),
        class,
    )?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveBody {
    /// The instruction hash the operator was shown.
    pub instruction_hash: String,
}

/// `POST /api/inbox/{id}/handover/{project}/approve` — operator only;
/// creates that project's one worktree.
pub async fn approve_handover(
    State(state): State<AppState>,
    Path((id, project)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<ApproveBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "solution.handover.approved", Some(&id))?;
    let svc = state.solution.clone();
    Ok(Json(
        tokio::task::spawn_blocking(move || {
            svc.approve_handover(&id, &project, &body.instruction_hash, class)
        })
        .await
        .map_err(|_| ApiError::new(500, "task panicked".into()))??,
    ))
}

/// `POST /api/inbox/{id}/handover/{project}/defer` — operator only.
pub async fn defer_handover(
    State(state): State<AppState>,
    Path((id, project)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "solution.handover.deferred", Some(&id))?;
    state.solution.defer_handover(&id, &project, class)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/factory/agents` — agents for the pickers: installed, and how
/// far their read-only enforcement is proven.
pub async fn factory_agents(headers: HeaderMap) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "GET")?;
    use crate::services::role_runner::conformance::TABLE;
    let path = std::env::var_os("PATH");
    let agents: Vec<serde_json::Value> = TABLE
        .iter()
        .map(|(agent, enforcement, _)| {
            let installed = path
                .as_ref()
                .is_some_and(|p| std::env::split_paths(p).any(|d| d.join(agent).is_file()));
            serde_json::json!({
                "agent": agent,
                "installed": installed,
                "enforcement": enforcement,
                "canReview": *enforcement == crate::services::role_runner::Enforcement::Verified,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "agents": agents })))
}
