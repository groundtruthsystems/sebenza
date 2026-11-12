//! Operator-only routes and the gate in front of them (FR-10, FR-11, FT-01).
//!
//! [`require_operator`] is the single gate. It accepts only a live operator
//! token ([`crate::operator_auth`]); the shared control token gets 403
//! `operator_required`, and that refusal is audited (TS-58). Every action it
//! lets through derives its audit caller from the credential class, never from
//! `x-sebenza-caller` (FT-02).
//!
//! Routes here:
//! - `POST /api/operator/session` — the SPA's token, same-origin only;
//! - `POST /api/operator/login`, `GET /api/operator/login/{id}` — CLI login;
//! - `GET /api/operator/logins`, `POST /api/operator/logins/{id}/{confirm|deny}`;
//! - `GET|PUT /api/factory/config` — global role profiles;
//! - `GET|PUT /api/inbox/{id}/factory/agents` — per-item profiles.

use crate::inbox_routes::{blocking, check, check_host, inbox};
use crate::operator_auth::{LoginError, LoginPoll};
use crate::server::{ApiError, AppState};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use common::domain::factory::{FactoryConfig, FactoryConfigError, validate_profiles};
use common::domain::model::InboxDraftView;
use common::domain::policies::origin_is_acceptable;
use common::domain::solution::{CredentialClass, FactoryOverrides, sha256_hex};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

/// Where the global `factory:` block lives (`~/.ai/sebenza.yaml` in
/// production), with a lock so two saves cannot interleave.
pub struct FactorySettings {
    pub path: PathBuf,
    lock: Mutex<()>,
}

impl FactorySettings {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn load(&self) -> Result<FactoryConfig, ApiError> {
        common::config::load_factory_config_from(&self.path).map_err(factory_error)
    }

    fn save(&self, config: &FactoryConfig) -> Result<(), ApiError> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        common::config::save_factory_config_to(&self.path, config).map_err(factory_error)
    }
}

fn factory_error(e: FactoryConfigError) -> ApiError {
    match e {
        FactoryConfigError::NotIndependent { .. } => {
            ApiError::coded(422, "not_independent", e.to_string())
        }
        _ => ApiError::coded(400, "invalid_factory_config", e.to_string()),
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_string)
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    header(headers, "authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_string))
}

/// The operator gate. `route` names the action for the denial audit line.
pub fn require_operator(
    state: &AppState,
    headers: &HeaderMap,
    route: &'static str,
    item: Option<&str>,
) -> Result<CredentialClass, ApiError> {
    let host = header(headers, "host").unwrap_or_default();
    check_host(&host)?;
    if !origin_is_acceptable(
        header(headers, "origin").as_deref(),
        header(headers, "referer").as_deref(),
        &format!("http://{host}"),
    ) {
        return Err(ApiError::new(
            403,
            "Cross-origin request refused".to_string(),
        ));
    }
    let token = bearer(headers).unwrap_or_default();
    if let Some(class) = state.operator.classify(&token) {
        return Ok(class);
    }
    let control =
        crate::adapters::control_token::load_control_token().map_err(|e| ApiError::new(500, e))?;
    if !token.is_empty() && token == control {
        inbox(state).audit_operator(
            route,
            item,
            Some(CredentialClass::Control),
            None,
            Some("operator_required"),
        );
        return Err(ApiError::coded(
            403,
            "operator_required",
            "This action needs the operator: sign in from the dashboard, or run `sebenza-cli login`."
                .to_string(),
        ));
    }
    inbox(state).audit_operator(route, item, None, None, Some("bad_token"));
    Err(ApiError::new(401, "Unauthorized".to_string()))
}

/// `POST /api/operator/session` — the SPA's operator token.
///
/// Issued only to a same-origin browser request: `Sec-Fetch-Site:
/// same-origin`, an `Origin` naming this daemon, and an allowed `Host`
/// (TS-50). A same-uid process can forge these headers; that residual is the
/// documented known limitation.
pub async fn operator_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let host = header(&headers, "host").unwrap_or_default();
    check_host(&host)?;
    let same_site = header(&headers, "sec-fetch-site").as_deref() == Some("same-origin");
    let origin = header(&headers, "origin");
    let origin_ok = origin.is_some()
        && origin_is_acceptable(origin.as_deref(), None, &format!("http://{host}"));
    if !same_site || !origin_ok {
        inbox(&state).audit_operator(
            "operator.session.refused",
            None,
            None,
            None,
            Some("not_same_origin"),
        );
        return Err(ApiError::coded(
            403,
            "same_origin_required",
            "The operator token is only issued to the dashboard itself.".to_string(),
        ));
    }
    let (token, expires_at) = state.operator.issue_session();
    Ok(Json(
        serde_json::json!({ "token": token, "expiresAt": expires_at }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginBody {
    #[serde(default)]
    pub client: Option<String>,
    /// SHA-256 hex of a secret the CLI keeps; its poll must present the secret.
    pub verifier_hash: String,
}

/// `POST /api/operator/login` — `sebenza-cli login` asks for an operator
/// token. Needs the control token; the operator must then confirm the code
/// in the dashboard.
pub async fn start_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "POST")?;
    let hash = body.verifier_hash.trim().to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::new(
            400,
            "verifierHash must be a SHA-256 hex digest".to_string(),
        ));
    }
    let client = body.client.unwrap_or_else(|| "sebenza-cli".to_string());
    let (login_id, code) = state
        .operator
        .start_login_at(&client, &hash, SystemTime::now());
    inbox(&state).audit_operator("operator.login.started", None, None, None, None);
    Ok(Json(serde_json::json!({
        "loginId": login_id,
        "code": code,
        "expiresIn": crate::operator_auth::LOGIN_TTL.as_secs(),
    })))
}

/// `GET /api/operator/login/{id}` — the CLI polls with its verifier in
/// `x-sebenza-login-verifier`. The token is returned once, on confirm.
pub async fn poll_login(
    State(state): State<AppState>,
    Path(login_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check_host(&header(&headers, "host").unwrap_or_default())?;
    let verifier = header(&headers, "x-sebenza-login-verifier").unwrap_or_default();
    let poll = state
        .operator
        .poll_login_at(&login_id, &verifier, SystemTime::now())
        .map_err(|e| match e {
            LoginError::NotFound => ApiError::new(404, e.to_string()),
            LoginError::BadVerifier => ApiError::new(403, e.to_string()),
            LoginError::NotPending => ApiError::new(409, e.to_string()),
        })?;
    Ok(Json(match poll {
        LoginPoll::Pending => serde_json::json!({ "status": "pending" }),
        LoginPoll::Denied => serde_json::json!({ "status": "denied" }),
        LoginPoll::Expired => serde_json::json!({ "status": "expired" }),
        LoginPoll::Confirmed { token, expires_at } => {
            serde_json::json!({ "status": "confirmed", "token": token, "expiresAt": expires_at })
        }
    }))
}

/// `GET /api/operator/logins` — CLI logins waiting for the operator.
pub async fn list_logins(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&state, &headers, "operator.logins.list", None)?;
    Ok(Json(
        serde_json::json!({ "logins": state.operator.pending_logins_at(SystemTime::now()) }),
    ))
}

/// `POST /api/operator/logins/{id}/{decision}` — `confirm` or `deny`.
pub async fn decide_login(
    State(state): State<AppState>,
    Path((login_id, decision)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let action = match decision.as_str() {
        "confirm" => "operator.login.confirmed",
        "deny" => "operator.login.denied",
        _ => return Err(ApiError::new(404, "unknown decision".to_string())),
    };
    let class = require_operator(&state, &headers, action, None)?;
    state
        .operator
        .decide_login_at(&login_id, decision == "confirm", SystemTime::now())
        .map_err(|e| match e {
            LoginError::NotFound => ApiError::new(404, e.to_string()),
            _ => ApiError::new(409, e.to_string()),
        })?;
    inbox(&state).audit_operator(action, None, Some(class), None, None);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/factory/config` — the global block and what it resolves to.
pub async fn get_factory_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "GET")?;
    let config = state.factory.load()?;
    let effective = config.resolve(None);
    Ok(Json(
        serde_json::json!({ "config": config, "effective": effective }),
    ))
}

/// `PUT /api/factory/config` — operator only; an unindependent reviewer is a
/// 422 `not_independent` and nothing is written.
pub async fn put_factory_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<FactoryConfig>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "factory.config.saved", None)?;
    state.factory.save(&config)?;
    let hash = sha256_hex(&serde_json::to_string(&config).unwrap_or_default());
    inbox(&state).audit_operator("factory.config.saved", None, Some(class), Some(&hash), None);
    Ok(Json(
        serde_json::json!({ "config": config, "effective": config.resolve(None) }),
    ))
}

fn overrides_of(state: &AppState, id: &str) -> Result<Option<FactoryOverrides>, ApiError> {
    let (view, _) = inbox(state).get(id)?;
    match view {
        InboxDraftView::Parsed(d) => Ok(d.frontmatter.factory),
        InboxDraftView::Raw { error, .. } => Err(ApiError::new(
            409,
            format!("draft {id} is not parseable ({error})"),
        )),
    }
}

/// `GET /api/inbox/{id}/factory/agents` — the item's overrides and the
/// profiles in effect for it.
pub async fn get_item_agents(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check(&headers, "GET")?;
    let overrides = overrides_of(&state, &id)?;
    let effective = state.factory.load()?.resolve(overrides.as_ref());
    Ok(Json(serde_json::json!({
        "overrides": overrides.unwrap_or_default(),
        "effective": effective,
    })))
}

/// `PUT /api/inbox/{id}/factory/agents` — operator only. The profiles this
/// would put in effect are checked first (422 `not_independent`); success
/// posts the audit comment on the item.
pub async fn put_item_agents(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(overrides): Json<FactoryOverrides>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let class = require_operator(&state, &headers, "factory.agents.set", Some(&id))?;
    let global = state.factory.load()?;
    let effective = global.resolve(Some(&overrides));
    validate_profiles(&effective).map_err(factory_error)?;
    let svc = inbox(&state);
    let hash = sha256_hex(&serde_json::to_string(&overrides).unwrap_or_default());
    let saved = overrides.clone();
    blocking(move || {
        svc.set_factory_overrides(&id, saved)?;
        svc.record_gated_action(&id, "factory.agents.set", class, Some(&hash))
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "overrides": overrides, "effective": effective }),
    ))
}
