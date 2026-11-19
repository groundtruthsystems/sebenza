//! `POST /api/tracks/{id}/complete` (FR-01): `sebenza-agentctl complete`,
//! and `POST /api/tracks/{id}/plan/approve` (FR-20): `sebenza-cli track
//! approve-plan`.
//!
//! The control token completes a track; it grants nothing else. The caller
//! names its worktree by the id the daemon wrote into that worktree's
//! `control.env`, and the daemon resolves the path, branch and target from
//! its own record of that worktree, never from the request. Only a track
//! registered in that worktree can be completed. Every completion and refusal
//! is audited with the credential class, ids and SHAs only.
//!
//! Plan approval needs the operator token; the control token gets 403
//! `operator_required`. The daemon finds the one managed worktree that
//! registers the track, reads its plan.json at the branch head from git
//! objects, and records the per-task boundary snapshot in the review log.

use crate::server::{ApiError, AppState};
use crate::services::completion_reconciler::project_worktrees;
use crate::services::standards_review::git_pinned::Git;
use crate::services::standards_review::plan_approval::{ApprovalError, approve_plan};
use crate::services::track_completion::check_own_track;
use crate::services::track_completion::{Completion, CompletionError, CompletionTarget};
use crate::services::tracks_projection::Projected;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use common::domain::review_records::PlanApproved;
use common::domain::solution::CredentialClass;
use serde::Deserialize;

/// The request body: which worktree is asking.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteTrackBody {
    pub worktree_id: String,
}

/// `POST /api/tracks/{id}/complete` — control token only.
pub async fn complete_track(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CompleteTrackBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::solution_routes::control_only(&headers)?;
    tokio::task::spawn_blocking(move || {
        complete_blocking(
            &state,
            &track_id,
            &body.worktree_id,
            CredentialClass::Control,
        )
    })
    .await
    .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
    .map(Json)
}

/// Resolve the caller's worktree, complete the track there, and audit the
/// outcome either way.
fn complete_blocking(
    state: &AppState,
    track_id: &str,
    worktree_id: &str,
    credential: CredentialClass,
) -> Result<serde_json::Value, ApiError> {
    let Some(target) = resolve_target(state, worktree_id) else {
        audit_refusal(track_id, None, "unknown_worktree");
        return Err(ApiError::coded(
            404,
            "unknown_worktree",
            "This worktree is not one the daemon manages.".to_string(),
        ));
    };
    match state.completion.complete(&target, track_id, credential) {
        Ok(done) => {
            audit_completion(&target, &done, credential);
            Ok(completion_json(&done))
        }
        Err(e) => {
            let err = completion_error(e);
            audit_refusal(track_id, Some(&target), err.code.unwrap_or("error"));
            Err(err)
        }
    }
}

/// The daemon's record of the worktree with id `worktree_id`, reconciling
/// each project once when it is not yet known.
fn resolve_target(state: &AppState, worktree_id: &str) -> Option<CompletionTarget> {
    let find = || {
        state.manager.list().into_iter().find_map(|app| {
            let known = app
                .runtime
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .list_worktrees()
                .into_iter()
                .find(|w| w.worktree_id == worktree_id)?;
            project_worktrees(&app)
                .worktrees
                .into_iter()
                .find(|t| t.worktree_path == std::path::Path::new(&known.path))
        })
    };
    find().or_else(|| {
        for app in state.manager.list() {
            app.reconciliation.reconcile(&app.path, &app.runtime, true);
        }
        find()
    })
}

fn completion_json(done: &Completion) -> serde_json::Value {
    let e = &done.event;
    serde_json::json!({
        "ok": true,
        "appended": done.appended,
        "projected": match done.projected {
            Projected::Applied => "applied",
            Projected::AlreadyDone => "already_done",
        },
        "job": {
            "trackId": e.track_id,
            "headSha": e.head_sha,
            "baseSha": e.base_sha,
            "standardsRev": e.standards_rev,
        },
    })
}

fn completion_error(e: CompletionError) -> ApiError {
    let msg = e.to_string();
    match e {
        CompletionError::BadTrack => ApiError::coded(400, "bad_track", msg),
        CompletionError::ForeignTrack(_) => ApiError::coded(403, "foreign_track", msg),
        CompletionError::Unpinnable(_) => ApiError::coded(422, "unpinnable", msg),
        CompletionError::Log(_) | CompletionError::Projection(_) => {
            ApiError::coded(500, "completion_failed", msg)
        }
        #[cfg(test)]
        CompletionError::Injected => ApiError::coded(500, "completion_failed", msg),
    }
}

fn audit_completion(target: &CompletionTarget, done: &Completion, credential: CredentialClass) {
    tracing::info!(
        audit = "track.completed",
        credential = credential.as_str(),
        project = %target.project_key,
        branch = %target.branch,
        track = %done.event.track_id,
        head = %done.event.head_sha,
        base = %done.event.base_sha,
        standards_rev = %done.event.standards_rev,
        appended = done.appended,
        "track audit"
    );
}

fn audit_refusal(track_id: &str, target: Option<&CompletionTarget>, refusal: &str) {
    tracing::info!(
        audit = "track.complete.refused",
        credential = CredentialClass::Control.as_str(),
        project = %target.map(|t| t.project_key.as_str()).unwrap_or(""),
        branch = %target.map(|t| t.branch.as_str()).unwrap_or(""),
        track = %track_id.chars().take(128).collect::<String>(),
        refusal,
        "track audit"
    );
}

/// `POST /api/tracks/{id}/plan/approve` — operator token only.
pub async fn approve_track_plan(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = crate::operator_routes::require_operator(
        &state,
        &headers,
        "track.plan.approve",
        Some(&track_id),
    )?;
    tokio::task::spawn_blocking(move || approve_blocking(&state, &track_id, class))
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?
        .map(Json)
}

fn approve_blocking(
    state: &AppState,
    track_id: &str,
    class: CredentialClass,
) -> Result<serde_json::Value, ApiError> {
    let target = track_worktree(state, track_id)?;
    let git = Git::open(&target.worktree_path)
        .map_err(|e| ApiError::coded(500, "plan_approval_failed", e.to_string()))?;
    let log = state.completion.log(&target.project_key);
    let approved = approve_plan(&log, &git, track_id, &target.branch, class.as_str())
        .map_err(approval_error)?;
    audit_approval(&target, &approved, class);
    Ok(approval_json(&approved))
}

/// The one managed worktree whose `tracks.json` registers `track_id`.
fn track_worktree(state: &AppState, track_id: &str) -> Result<CompletionTarget, ApiError> {
    let mut owners: Vec<CompletionTarget> = state
        .manager
        .list()
        .iter()
        .flat_map(|app| project_worktrees(app).worktrees)
        .filter(|t| check_own_track(t, track_id).is_ok())
        .collect();
    match owners.len() {
        1 => Ok(owners.remove(0)),
        0 => Err(ApiError::coded(
            404,
            "unknown_track",
            "No managed worktree registers this track.".to_string(),
        )),
        _ => Err(ApiError::coded(
            409,
            "ambiguous_track",
            "More than one managed worktree registers this track.".to_string(),
        )),
    }
}

fn approval_error(e: ApprovalError) -> ApiError {
    let msg = e.to_string();
    match e {
        ApprovalError::NoPlan(_) => ApiError::coded(404, "plan_not_committed", msg),
        ApprovalError::InvalidPlan(_) => ApiError::coded(422, "invalid_plan", msg),
        ApprovalError::Git(_) | ApprovalError::Log(_) => {
            ApiError::coded(500, "plan_approval_failed", msg)
        }
    }
}

fn approval_json(a: &PlanApproved) -> serde_json::Value {
    let tasks: Vec<serde_json::Value> = a
        .boundaries
        .iter()
        .map(|b| serde_json::json!({ "taskId": b.task_id, "files": b.files }))
        .collect();
    serde_json::json!({
        "ok": true,
        "trackId": a.track_id,
        "headSha": a.head_sha,
        "planSha256": a.plan_sha256,
        "planContentSha256": a.plan_content_sha256,
        "tasks": tasks,
    })
}

fn audit_approval(target: &CompletionTarget, a: &PlanApproved, class: CredentialClass) {
    tracing::info!(
        audit = "track.plan.approved",
        credential = class.as_str(),
        project = %target.project_key,
        branch = %target.branch,
        track = %a.track_id,
        head = %a.head_sha,
        plan_sha256 = %a.plan_sha256,
        tasks = a.boundaries.len(),
        "track audit"
    );
}
