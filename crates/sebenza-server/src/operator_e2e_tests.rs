//! Loopback end-to-end tests for the operator credential and gate
//! (TS-18 route half, TS-23, TS-38, TS-48, TS-50, TS-51, TS-58, TS-62).
//!
//! The real router runs on an ephemeral loopback port with a capturing audit
//! sink. Nothing touches `~/.ai`, `~/.config` or a model.

use crate::adapters::projects_registry::ProjectsRegistry;
use crate::domain::inbox_events::InboxEventKind;
use crate::server::AppState;
use crate::services::inbox_service::{AuditRecord, AuditSink, InboxService};
use common::adapters::inbox_store::{EventAuthor, InboxStore};
use common::domain::inbox_events::{Thread, content_hash};
use common::domain::solution::{ReviewLevel, RoundRef};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) const TOKEN: &str = "route-test-token";
static SEQ: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
pub(crate) struct Captured(Mutex<Vec<AuditRecord>>);
impl AuditSink for Captured {
    fn record(&self, r: &AuditRecord) {
        self.0.lock().unwrap().push(r.clone());
    }
}
impl Captured {
    pub(crate) fn all(&self) -> Vec<AuditRecord> {
        self.0.lock().unwrap().clone()
    }
    pub(crate) fn with(&self, action: &str) -> Vec<AuditRecord> {
        self.all()
            .into_iter()
            .filter(|r| r.action == action)
            .collect()
    }
}

pub(crate) struct Daemon {
    pub(crate) state: AppState,
    pub(crate) store: InboxStore,
    pub(crate) audit: Arc<Captured>,
    pub(crate) base: PathBuf,
    pub(crate) port: u16,
}

pub(crate) struct Call<'a> {
    pub(crate) method: &'a str,
    pub(crate) path: String,
    pub(crate) bearer: Option<String>,
    pub(crate) origin: Option<String>,
    pub(crate) sec_fetch_site: Option<&'a str>,
    pub(crate) host: Option<String>,
    pub(crate) extra: Vec<(&'a str, String)>,
    pub(crate) body: Option<serde_json::Value>,
}

impl Daemon {
    pub(crate) async fn start() -> Daemon {
        crate::adapters::control_token::pin_control_token(TOKEN);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("sebenza-op-e2e-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let audit = Arc::new(Captured::default());
        let inbox = Arc::new(
            InboxService::new(
                InboxStore::with_dir(base.join("inbox")),
                ProjectsRegistry::with_file(base.join("projects.json")),
            )
            .with_audit_sink(audit.clone()),
        );
        let agent_stream = Arc::new(crate::services::agent_stream::AgentStreamManager::new());
        let system_agent = crate::services::system_agent::SystemAgentService::new(
            Default::default(),
            inbox.clone(),
            agent_stream.clone(),
            Default::default(),
        );
        let factory = Arc::new(crate::operator_routes::FactorySettings::new(
            base.join("sebenza.yaml"),
        ));
        let inbox_for_solution = inbox.clone();
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
            operator: Arc::new(crate::operator_auth::OperatorAuth::new()),
            factory: factory.clone(),
            solution: Arc::new(crate::services::solution::SolutionService::new(
                inbox_for_solution.clone(),
                Arc::new(crate::services::role_runner::test_runner(&base)),
                factory.clone(),
            )),
            reviews: Arc::new(crate::services::project_review::ProjectReviewService::new(
                inbox_for_solution.clone(),
                Arc::new(crate::services::role_runner::test_runner(&base)),
                factory.clone(),
                base.join("snapshots"),
            )),
            frontend_dist: None,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = crate::server::build_router(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Daemon {
            state,
            store: InboxStore::with_dir(base.join("inbox")),
            audit,
            base,
            port,
        }
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// A request as the SPA sends it, with `bearer`.
    pub(crate) fn spa<'a>(&self, method: &'a str, path: &str, bearer: &str) -> Call<'a> {
        Call {
            method,
            path: path.to_string(),
            bearer: Some(bearer.to_string()),
            origin: Some(format!("http://{}", self.host())),
            sec_fetch_site: Some("same-origin"),
            host: None,
            extra: vec![],
            body: None,
        }
    }

    pub(crate) async fn send(&self, c: Call<'_>) -> (u16, serde_json::Value) {
        let host = c.host.clone().unwrap_or_else(|| self.host());
        let payload = c.body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!(
            "{} {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n",
            c.method, c.path
        );
        if let Some(b) = &c.bearer {
            req.push_str(&format!("Authorization: Bearer {b}\r\n"));
        }
        if let Some(o) = &c.origin {
            req.push_str(&format!("Origin: {o}\r\n"));
        }
        if let Some(s) = c.sec_fetch_site {
            req.push_str(&format!("Sec-Fetch-Site: {s}\r\n"));
        }
        for (k, v) in &c.extra {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if !payload.is_empty() {
            req.push_str("Content-Type: application/json\r\n");
        }
        req.push_str(&format!(
            "Content-Length: {}\r\n\r\n{payload}",
            payload.len()
        ));
        let mut stream = tokio::net::TcpStream::connect(self.host()).await.unwrap();
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let raw = String::from_utf8(raw).unwrap();
        let (head, rest) = raw.split_once("\r\n\r\n").unwrap();
        let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        let text = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            dechunk(rest)
        } else {
            rest.to_string()
        };
        let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
        (status, json)
    }

    pub(crate) async fn operator_token(&self) -> String {
        let mut c = self.spa("POST", "/api/operator/session", "");
        c.bearer = None;
        let (code, v) = self.send(c).await;
        assert_eq!(code, 200, "{v}");
        v["token"].as_str().unwrap().to_string()
    }

    fn problem_item(&self) -> (String, String) {
        let id = self.state.inbox.create("Solution").unwrap().id;
        self.store
            .append_event(
                &id,
                EventAuthor::system_agent(),
                None,
                InboxEventKind::ProblemRaised {
                    request_id: "01PROBLEM".into(),
                    problem_id: "P-1".into(),
                    round: RoundRef {
                        level: ReviewLevel::Solution,
                        scope: id.clone(),
                        cycle: 1,
                        round: 3,
                    },
                    title: "IdP?".into(),
                    detail: "Which IdP?".into(),
                    warnings: vec![],
                },
            )
            .unwrap();
        (id, "01PROBLEM".to_string())
    }

    pub(crate) fn comments(&self, id: &str) -> Vec<(Option<String>, String)> {
        self.store
            .read_events(id)
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.kind {
                InboxEventKind::Comment {
                    thread: Thread::Overall,
                    body,
                    ..
                } => Some((e.caller, body)),
                _ => None,
            })
            .collect()
    }
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&tail[..n]);
        rest = &tail[n + 2..];
    }
    out
}

fn dependent_config() -> serde_json::Value {
    serde_json::json!({"solution": {"designer": {"agent": "codex"}, "reviewer": {"agent": "codex"}}})
}

fn good_config() -> serde_json::Value {
    serde_json::json!({
        "solution": {"designer": {"agent": "claude", "model": "claude-opus-5-5"},
                     "reviewer": {"agent": "codex"}},
        "project": {"reviewer": {"agent": "claude", "model": "claude-fable-5-2"}}
    })
}

// TS-50: the operator token is issued only to a same-origin dashboard request.
#[tokio::test]
async fn ts50_the_operator_token_is_issued_same_origin_only() {
    let d = Daemon::start().await;
    let base = || {
        let mut c = d.spa("POST", "/api/operator/session", "");
        c.bearer = None;
        c
    };
    let mut no_fetch_site = base();
    no_fetch_site.sec_fetch_site = None;
    let mut cross_site = base();
    cross_site.sec_fetch_site = Some("cross-site");
    let mut evil_origin = base();
    evil_origin.origin = Some("http://evil.test".into());
    let mut no_origin = base();
    no_origin.origin = None;
    for (name, c) in [
        ("no Sec-Fetch-Site", no_fetch_site),
        ("cross-site", cross_site),
        ("foreign Origin", evil_origin),
        ("no Origin", no_origin),
    ] {
        let (code, v) = d.send(c).await;
        assert_eq!(code, 403, "{name}: {v}");
        assert_eq!(v["code"], "same_origin_required", "{name}");
        assert!(v.get("token").is_none());
    }
    let mut rebound = base();
    rebound.host = Some(format!("evil.test:{}", d.port));
    rebound.origin = Some(format!("http://evil.test:{}", d.port));
    assert_eq!(
        d.send(rebound).await.0,
        403,
        "a non-loopback Host is refused"
    );

    let token = d.operator_token().await;
    assert!(token.starts_with("op_"));
    assert_eq!(d.audit.with("operator.session.refused").len(), 4);
}

// TS-23 + TS-58: the shared control token is refused with 403
// operator_required on every operator route, changes nothing, and each
// refusal is audited with the control credential.
#[tokio::test]
async fn ts23_ts58_the_shared_token_opens_no_operator_route() {
    let d = Daemon::start().await;
    let (id, rid) = d.problem_item();
    let routes: Vec<(&str, String, Option<serde_json::Value>)> = vec![
        ("GET", "/api/operator/logins".into(), None),
        ("POST", "/api/operator/logins/01X/confirm".into(), None),
        ("POST", "/api/operator/logins/01X/deny".into(), None),
        ("PUT", "/api/factory/config".into(), Some(good_config())),
        (
            "PUT",
            format!("/api/inbox/{id}/factory/agents"),
            Some(serde_json::json!({"designer": {"agent": "codex"}})),
        ),
        (
            "POST",
            format!("/api/inbox/{id}/requests/{rid}/confirm"),
            Some(serde_json::json!({"body": "Use OIDC", "contentHash": content_hash("Use OIDC")})),
        ),
    ];
    let n = routes.len();
    for (method, path, body) in routes {
        let mut c = d.spa(method, &path, TOKEN);
        c.body = body;
        let (code, v) = d.send(c).await;
        assert_eq!(code, 403, "{method} {path}: {v}");
        assert_eq!(v["code"], "operator_required", "{method} {path}");
    }
    assert!(!d.base.join("sebenza.yaml").exists(), "no config written");
    let req = d.state.inbox.request(&id, &rid).unwrap();
    assert_eq!(
        req.status,
        common::domain::inbox_events::RequestStatus::Open
    );
    let common::domain::model::InboxDraftView::Parsed(draft) = d.store.get(&id).unwrap() else {
        panic!()
    };
    assert_eq!(draft.frontmatter.factory, None, "no overrides written");
    let denied: Vec<_> = d
        .audit
        .all()
        .into_iter()
        .filter(|r| r.refusal == Some("operator_required"))
        .collect();
    assert_eq!(denied.len(), n, "every refusal is audited");
    assert!(
        denied
            .iter()
            .all(|r| r.caller.as_deref() == Some("control"))
    );
    assert!(
        d.comments(&id).is_empty(),
        "a refusal posts no action comment"
    );

    // A bad token is a 401, also audited.
    let (code, _) = d.send(d.spa("GET", "/api/operator/logins", "nope")).await;
    assert_eq!(code, 401);
}

// TS-18 (route half): an unindependent reviewer is a 422 not_independent on
// save, globally and per item, and nothing is written.
#[tokio::test]
async fn ts18_an_unindependent_reviewer_is_refused_on_save() {
    let d = Daemon::start().await;
    let op = d.operator_token().await;
    let mut c = d.spa("PUT", "/api/factory/config", &op);
    c.body = Some(dependent_config());
    let (code, v) = d.send(c).await;
    assert_eq!(code, 422, "{v}");
    assert_eq!(v["code"], "not_independent");
    assert!(v["error"].as_str().unwrap().contains("not independent"));
    assert!(!d.base.join("sebenza.yaml").exists());

    let mut c = d.spa("PUT", "/api/factory/config", &op);
    c.body = Some(good_config());
    let (code, v) = d.send(c).await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["effective"]["solutionReviewer"]["agent"], "codex");

    // Per item: making the designer codex clashes with the codex reviewer.
    let id = d.state.inbox.create("S").unwrap().id;
    let mut c = d.spa("PUT", &format!("/api/inbox/{id}/factory/agents"), &op);
    c.body = Some(serde_json::json!({"designer": {"agent": "codex"}}));
    let (code, v) = d.send(c).await;
    assert_eq!(code, 422, "{v}");
    assert_eq!(v["code"], "not_independent");
    // Haiku never reviews, whatever the designer.
    let mut c = d.spa("PUT", &format!("/api/inbox/{id}/factory/agents"), &op);
    c.body = Some(
        serde_json::json!({"reviewer": {"solution": {"agent": "claude", "model": "claude-haiku-5-5"}}}),
    );
    assert_eq!(d.send(c).await.0, 422);
    assert!(d.comments(&id).is_empty());

    let (code, v) = d
        .send(d.spa("GET", &format!("/api/inbox/{id}/factory/agents"), TOKEN))
        .await;
    assert_eq!(code, 200);
    assert_eq!(v["overrides"], serde_json::json!({}));
    assert_eq!(v["effective"]["designer"]["model"], "claude-opus-5-5");
}

// TS-62 + TS-38: every gated action posts an Overall-thread comment with
// action, credential class and hash; the caller comes from the credential,
// not the spoofed header.
#[tokio::test]
async fn ts62_ts38_gated_actions_are_audited_as_the_credential() {
    let d = Daemon::start().await;
    let op = d.operator_token().await;
    let id = d.state.inbox.create("S").unwrap().id;
    let mut c = d.spa("PUT", &format!("/api/inbox/{id}/factory/agents"), &op);
    c.extra.push(("x-sebenza-caller", "worktree".into()));
    c.body = Some(serde_json::json!({"coordinator": {"agent": "grok"}}));
    let (code, v) = d.send(c).await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["effective"]["coordinator"]["agent"], "grok");

    let comments = d.comments(&id);
    assert_eq!(comments.len(), 1);
    let (caller, body) = &comments[0];
    assert_eq!(caller.as_deref(), Some("operator_session"));
    assert!(body.contains("`factory.agents.set`"), "{body}");
    assert!(body.contains("credential `operator_session`"), "{body}");
    assert!(body.contains("sha256 `"), "{body}");
    let rec = &d.audit.with("factory.agents.set")[0];
    assert_eq!(rec.caller.as_deref(), Some("operator_session"));
    assert_eq!(rec.thread, Some("overall"));
    assert!(rec.hash_after.as_ref().is_some_and(|h| h.len() == 64));

    // Confirming a solution problem: the gated comment carries the hash of
    // the confirmed text; the spoofed header is ignored.
    let (pid, rid) = d.problem_item();
    let text = "Use the existing OIDC provider.";
    let mut c = d.spa(
        "POST",
        &format!("/api/inbox/{pid}/requests/{rid}/confirm"),
        &op,
    );
    c.extra.push(("x-sebenza-caller", "worktree".into()));
    c.body = Some(serde_json::json!({"body": text, "contentHash": content_hash(text)}));
    let (code, v) = d.send(c).await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["request"]["status"], "confirmed");
    let comments = d.comments(&pid);
    assert_eq!(comments.len(), 1);
    assert!(comments[0].1.contains("`inbox.problem.confirmed`"));
    assert!(comments[0].1.contains(&content_hash(text)));
    let confirm = d.audit.with("inbox.resolution.confirmed");
    assert_eq!(confirm[0].caller.as_deref(), Some("operator_session"));

    // Global config: audited with the credential and the config hash.
    let mut c = d.spa("PUT", "/api/factory/config", &op);
    c.body = Some(good_config());
    assert_eq!(d.send(c).await.0, 200);
    let rec = &d.audit.with("factory.config.saved")[0];
    assert_eq!(rec.caller.as_deref(), Some("operator_session"));
    assert!(rec.hash_after.is_some());
}

// TS-51 (server half) + TS-48 (log half): a CLI login works only once the
// operator confirms it in the UI, hands the token over once to the verifier
// holder, and the token never reaches an audit line.
#[tokio::test]
async fn ts51_a_cli_login_needs_the_operators_confirmation() {
    let d = Daemon::start().await;
    let verifier = "cli-secret-verifier";
    let start = |d: &Daemon| {
        let mut c = d.spa("POST", "/api/operator/login", TOKEN);
        c.origin = None;
        c.sec_fetch_site = None;
        c.body = Some(serde_json::json!({
            "client": "sebenza-cli",
            "verifierHash": crate::operator_auth::verifier_hash(verifier),
        }));
        c
    };
    let (code, v) = d.send(start(&d)).await;
    assert_eq!(code, 200, "{v}");
    let login_id = v["loginId"].as_str().unwrap().to_string();
    let code_shown = v["code"].as_str().unwrap().to_string();

    let poll = |d: &Daemon, verifier: &str| {
        let mut c = d.spa("GET", &format!("/api/operator/login/{login_id}"), "");
        c.bearer = None;
        c.origin = None;
        c.extra
            .push(("x-sebenza-login-verifier", verifier.to_string()));
        c
    };
    assert_eq!(d.send(poll(&d, verifier)).await.1["status"], "pending");
    assert_eq!(d.send(poll(&d, "wrong")).await.0, 403);

    // The shared token cannot confirm; the dashboard's operator token can.
    let (code, _) = d
        .send(d.spa(
            "POST",
            &format!("/api/operator/logins/{login_id}/confirm"),
            TOKEN,
        ))
        .await;
    assert_eq!(code, 403);
    let op = d.operator_token().await;
    let (_, list) = d.send(d.spa("GET", "/api/operator/logins", &op)).await;
    assert_eq!(list["logins"][0]["code"], code_shown.as_str());
    let (code, _) = d
        .send(d.spa(
            "POST",
            &format!("/api/operator/logins/{login_id}/confirm"),
            &op,
        ))
        .await;
    assert_eq!(code, 200);

    let (_, v) = d.send(poll(&d, verifier)).await;
    assert_eq!(v["status"], "confirmed");
    let cli_token = v["token"].as_str().unwrap().to_string();
    assert!(v["expiresAt"].as_u64().unwrap() > 0);
    assert_eq!(d.send(poll(&d, verifier)).await.0, 404, "handed over once");

    // The CLI token opens an operator route and is audited as operator_cli.
    let mut c = d.spa("PUT", "/api/factory/config", &cli_token);
    c.origin = None;
    c.sec_fetch_site = None;
    c.body = Some(good_config());
    assert_eq!(d.send(c).await.0, 200);
    assert_eq!(
        d.audit.with("factory.config.saved")[0].caller.as_deref(),
        Some("operator_cli")
    );

    // A denied login yields no token.
    let (_, v) = d.send(start(&d)).await;
    let denied = v["loginId"].as_str().unwrap().to_string();
    d.send(d.spa("POST", &format!("/api/operator/logins/{denied}/deny"), &op))
        .await;
    let mut c = d.spa("GET", &format!("/api/operator/login/{denied}"), "");
    c.bearer = None;
    c.extra
        .push(("x-sebenza-login-verifier", verifier.to_string()));
    let (_, v) = d.send(c).await;
    assert_eq!(v["status"], "denied");
    assert!(v.get("token").is_none());

    // TS-48: neither token appears in any audit record.
    let dump = format!("{:?}", d.audit.all());
    assert!(!dump.contains(&op) && !dump.contains(&cli_token));
}
