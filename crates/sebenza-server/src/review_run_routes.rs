//! The standards-review run API (FR-11..FR-13) and the read-only review
//! routes behind `sebenza-cli review status|findings` (FR-27).
//!
//! - `POST /api/review-runs/claim` and `POST /api/review-runs/{id}/submit`
//!   take only a run token as the bearer. The control token and operator
//!   tokens are refused outright, never looked up. The token's own run
//!   names the project, so the request carries no project.
//! - `GET /{prefix}/api/review/tracks/{id}` and `.../findings` read the
//!   fold, like the project's other read routes.
//!
//! Every outcome is audited with run, job and track ids and a code; never a
//! token, a description or submitted text.

use crate::server::{ApiError, AppState};
use crate::services::standards_review::jobs::SCOPE_UNPROVEN;
use crate::services::standards_review::run_store::{Accepted, Claimed, RunError, now_secs};
use crate::services::standards_review::views::{claim_view, job_status, track_findings};
use crate::services::track_completion::valid_track_id;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use common::domain::review_records::RunKind;
use serde::Deserialize;

/// The claim body: which kind of run the token is for.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimBody {
    pub kind: RunKind,
}

/// `POST /api/review-runs/claim` — run token only.
pub async fn claim_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ClaimBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = run_token(&state, &headers)?;
    blocking(move || {
        let project = project_of(&state, &token)?;
        let claimed = state
            .review_runs()
            .claim(&project, &token, body.kind, now_secs())
            .map_err(|e| refused("claim", e))?;
        audit_claim(&claimed);
        Ok(serde_json::to_value(claim_view(&claimed)).unwrap_or_default())
    })
    .await
}

/// `POST /api/review-runs/{id}/submit` — run token only. The body is parsed
/// here, not by an extractor, so a schema rejection is a counted attempt
/// that names the offending field.
pub async fn submit_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = run_token(&state, &headers)?;
    blocking(move || {
        let project = project_of(&state, &token)?;
        let accepted = state
            .review_runs()
            .submit(&project, &run_id, &token, &body, now_secs())
            .map_err(|e| refused("submit", e))?;
        audit_submit(&accepted);
        Ok(submit_json(&accepted))
    })
    .await
}

/// `GET /{prefix}/api/review/tracks/{id}` — the track's latest review job.
pub async fn review_status(
    State(state): State<AppState>,
    Path((prefix, track_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let view = read_view(&state, &prefix, &track_id).await?;
    let job = job_status(&view, &track_id);
    Ok(Json(serde_json::json!({ "trackId": track_id, "job": job })))
}

/// `GET /{prefix}/api/review/tracks/{id}/findings` — every finding of the
/// track with its status.
pub async fn review_findings(
    State(state): State<AppState>,
    Path((prefix, track_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let view = read_view(&state, &prefix, &track_id).await?;
    let findings = track_findings(&view, &track_id);
    Ok(Json(
        serde_json::json!({ "trackId": track_id, "findings": findings }),
    ))
}

async fn read_view(
    state: &AppState,
    prefix: &str,
    track_id: &str,
) -> Result<crate::services::standards_review::jobs::ReviewView, ApiError> {
    if !valid_track_id(track_id) {
        return Err(ApiError::coded(400, "bad_track", "invalid track id".into()));
    }
    if state.manager.get(prefix).is_none() {
        return Err(ApiError::new(404, "Project not found".to_string()));
    }
    let (runs, prefix) = (state.review_runs(), prefix.to_string());
    tokio::task::spawn_blocking(move || runs.view(&prefix))
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .map_err(|e| ApiError::coded(500, "review_log", e.to_string()))
}

async fn blocking(
    f: impl FnOnce() -> Result<serde_json::Value, ApiError> + Send + 'static,
) -> Result<Json<serde_json::Value>, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .map(Json)
}

/// The bearer, which must not be the control token or an operator token.
fn run_token(state: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            ApiError::coded(401, "run_token_required", "A run token is required.".into())
        })?;
    let control = crate::adapters::control_token::load_control_token().ok();
    if control.as_deref() == Some(token) || state.operator.classify(token).is_some() {
        audit_wrong_credential();
        return Err(ApiError::coded(
            401,
            "wrong_credential",
            "Review runs accept only a run token.".into(),
        ));
    }
    Ok(token.to_string())
}

/// The project whose log holds the token's run.
fn project_of(state: &AppState, token: &str) -> Result<String, ApiError> {
    let runs = state.review_runs();
    state
        .manager
        .list()
        .into_iter()
        .map(|app| app.prefix.clone())
        .find(|p| runs.holds_token(p, token))
        .ok_or_else(|| refused("lookup", RunError::UnknownToken))
}

fn refused(action: &str, e: RunError) -> ApiError {
    let msg = e.to_string();
    let err = match e {
        RunError::UnknownToken => ApiError::coded(401, "bad_run_token", msg),
        RunError::Expired => ApiError::coded(401, "lease_expired", msg),
        RunError::Used => ApiError::coded(401, "run_token_used", msg),
        RunError::WrongRun => ApiError::coded(403, "token_wrong_run", msg),
        RunError::WrongKind => ApiError::coded(403, "token_wrong_kind", msg),
        RunError::Blocked(b) if b.iter().any(|b| b.code == SCOPE_UNPROVEN) => {
            ApiError::coded(409, "scope_unproven", msg)
        }
        RunError::Blocked(_) => ApiError::coded(409, "standards_unavailable", msg),
        RunError::Rejected(_) => ApiError::coded(422, "submission_rejected", msg),
        RunError::Inputs(_) => ApiError::coded(503, "inputs_unavailable", msg),
        RunError::Log(_) => ApiError::coded(500, "review_log", msg),
    };
    tracing::info!(
        audit = "review.run.refused",
        action,
        code = err.code.unwrap_or("error"),
        "review run audit"
    );
    err
}

fn submit_json(a: &Accepted) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "runId": a.run_id,
        "jobId": a.job_id,
        "findingsOpened": a.findings_opened,
        "verdict": a.verdict,
    })
}

fn audit_claim(c: &Claimed) {
    tracing::info!(
        audit = "review.run.claimed",
        run = %c.run.leased.run_id,
        job = %c.job.id,
        track = %c.job.completion.track_id,
        kind = c.run.leased.kind.as_str(),
        "review run audit"
    );
}

fn audit_submit(a: &Accepted) {
    tracing::info!(
        audit = "review.run.submitted",
        run = %a.run_id,
        job = %a.job_id,
        findings_opened = a.findings_opened,
        passing = a.verdict.passing,
        "review run audit"
    );
}

fn audit_wrong_credential() {
    tracing::info!(
        audit = "review.run.refused",
        code = "wrong_credential",
        "review run audit"
    );
}
