//! `POST /api/tracks/{id}/complete` (FR-01): `sebenza-agentctl complete`.
//!
//! The control token completes a track; it grants nothing else. The caller
//! names its worktree by the id the daemon wrote into that worktree's
//! `control.env`, and the daemon resolves the path, branch and target from
//! its own record of that worktree, never from the request. Only a track
//! registered in that worktree can be completed. Every completion and refusal
//! is audited with the credential class, ids and SHAs only.

use crate::server::{ApiError, AppState};
use crate::services::completion_reconciler::project_worktrees;
use crate::services::track_completion::{Completion, CompletionError, CompletionTarget};
use crate::services::tracks_projection::Projected;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
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
    let credential = CredentialClass::Control;
    let s = state.clone();
    let worktree_id = body.worktree_id.clone();
    let target = tokio::task::spawn_blocking(move || resolve_target(&s, &worktree_id))
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?;
    let Some(target) = target else {
        audit_refusal(&track_id, None, "unknown_worktree");
        return Err(ApiError::coded(
            404,
            "unknown_worktree",
            "This worktree is not one the daemon manages.".to_string(),
        ));
    };
    let s = state.clone();
    let (t, id) = (target.clone(), track_id.clone());
    let result = tokio::task::spawn_blocking(move || s.completion.complete(&t, &id, credential))
        .await
        .map_err(|_| ApiError::new(500, "task panicked".to_string()))?;
    match result {
        Ok(done) => {
            audit_completion(&target, &done, credential);
            Ok(Json(completion_json(&done)))
        }
        Err(e) => {
            let err = completion_error(e);
            audit_refusal(&track_id, Some(&target), err.code.unwrap_or("error"));
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
