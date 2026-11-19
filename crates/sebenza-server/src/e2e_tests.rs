//! Loopback end-to-end tests (TS-56, TS-57).
//!
//! The real router is bound to `127.0.0.1` on an ephemeral port and driven
//! over plain HTTP, wired the way `main` wires it: the system agent triages
//! every request it is told about and its results are applied by the
//! production [`TriageApplier`]. Only the edges are fakes: the agent CLI is
//! the stub (`services/system_agent/testdata`), the pane is a recording
//! [`PaneSink`], and worktrees come from a recording [`ConversionRunner`], so
//! nothing calls a model, git or tmux, or touches `~/.ai/sebenza`.

use crate::adapters::projects_registry::{ProjectEntry, ProjectsRegistry};
use crate::domain::inbox_events::{InboxEventKind, WorktreeKey};
use crate::server::AppState;
use crate::services::inbox_convert::{ConversionRunner, ConversionTarget, SEBENZA_INDEX_REL_PATH};
use crate::services::inbox_service::InboxService;
use crate::services::system_agent::apply::TriageApplier;
use crate::services::system_agent::{SystemAgentService, stub_agent_for_tests};
use common::adapters::inbox_store::InboxStore;
use common::services::resolution_delivery::{PaneSink, prepare_resolution};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The same value the route tests pin: the token is process-wide.
const TOKEN: &str = "route-test-token";
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Records every paste; never touches tmux.
#[derive(Default)]
struct FakePane(Mutex<Vec<(WorktreeKey, String)>>);
impl PaneSink for FakePane {
    fn send(&self, worktree: &WorktreeKey, text: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push((worktree.clone(), text.to_string()));
        Ok(())
    }
}

/// Records each launch prompt; never touches git or tmux. Workspace
/// detection is the real rule: the project has `.ai/sebenza/index.md`.
#[derive(Default)]
struct RecordingRunner(Mutex<Vec<(String, String)>>);
impl ConversionRunner for RecordingRunner {
    fn create_worktree(&self, t: &ConversionTarget) -> Result<String, String> {
        Ok(format!("/wt/{}", t.branch))
    }
    fn write_note(&self, _p: &str, _b: &str) -> Result<(), String> {
        Ok(())
    }
    fn exclude_note(&self, _p: &str) -> Result<(), String> {
        Ok(())
    }
    fn record_origin(&self, _p: &str, _d: &str) -> Result<(), String> {
        Ok(())
    }
    fn send_prompt(&self, t: &ConversionTarget, _p: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push((t.branch.clone(), t.prompt.clone()));
        Ok(())
    }
    fn has_sebenza_workspace(&self, t: &ConversionTarget) -> bool {
        Path::new(&t.project_path)
            .join(SEBENZA_INDEX_REL_PATH)
            .is_file()
    }
    fn now(&self) -> String {
        "t".into()
    }
}

/// A running daemon on a loopback port.
struct Daemon {
    state: AppState,
    pane: Arc<FakePane>,
    base: PathBuf,
    port: u16,
}

impl Daemon {
    /// Start the router with the system agent enabled on the stub in `mode`.
    async fn start(mode: &str) -> Daemon {
        crate::adapters::control_token::pin_control_token(TOKEN);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("sebenza-e2e-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("temp base");

        let inbox = Arc::new(InboxService::new(
            InboxStore::with_dir(base.join("inbox")),
            ProjectsRegistry::with_file(base.join("projects.json")),
        ));
        let agent_stream = Arc::new(crate::services::agent_stream::AgentStreamManager::new());
        let (config, options, _stub) = stub_agent_for_tests(&base, mode);
        let system_agent =
            SystemAgentService::new(config, inbox.clone(), agent_stream.clone(), options);
        // As `main`: results applied in production form, every request
        // triaged, delivery only through the pane sink.
        system_agent.set_sink(Arc::new(TriageApplier::new(inbox.clone())));
        inbox.set_request_observer(system_agent.observer());
        let pane = Arc::new(FakePane::default());
        inbox.set_pane_sink(pane.clone());

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
            chat_gate: Arc::new(crate::services::chat::gate::ChatGate::new(
                Default::default(),
                std::env::temp_dir().join(format!("sebenza-chat-test-{}", std::process::id())),
            )),
            completion: Arc::new(
                crate::services::track_completion::TrackCompletionService::new(
                    base.join("reviews"),
                ),
            ),
            frontend_dist: None,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let app = crate::server::build_router(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Daemon {
            state,
            pane,
            base,
            port,
        }
    }

    /// A tempdir project registered with the server and the inbox, with a
    /// Sebenza workspace when `workspace`.
    fn project(&self, name: &str, workspace: bool) -> String {
        let dir = self.base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        if workspace {
            std::fs::create_dir_all(dir.join(".ai/sebenza")).unwrap();
            std::fs::write(dir.join(SEBENZA_INDEX_REL_PATH), "# index").unwrap();
        }
        let app = self.state.manager.add_ephemeral(&dir.to_string_lossy());
        ProjectsRegistry::with_file(self.base.join("projects.json")).add(ProjectEntry {
            path: app.path.clone(),
            name: name.to_string(),
            added_at: 0,
        });
        app.path.clone()
    }

    /// One HTTP/1.1 exchange as the SPA makes it: same-origin, token-bearing.
    /// `bearer_only` drops the Origin, as agentctl does.
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
        bearer_only: bool,
    ) -> (u16, serde_json::Value) {
        let host = format!("127.0.0.1:{}", self.port);
        let origin = (!bearer_only).then(|| format!("http://{host}"));
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        self.call_as(method, path, Some(TOKEN), origin.as_deref(), &payload)
            .await
    }

    /// A request with `bearer` (or none) and a raw `payload`.
    async fn call_as(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        origin: Option<&str>,
        payload: &str,
    ) -> (u16, serde_json::Value) {
        let host = format!("127.0.0.1:{}", self.port);
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        if let Some(b) = bearer {
            req.push_str(&format!("Authorization: Bearer {b}\r\n"));
        }
        if let Some(o) = origin {
            req.push_str(&format!("Origin: {o}\r\n"));
        }
        if !payload.is_empty() {
            req.push_str("Content-Type: application/json\r\n");
        }
        req.push_str(&format!(
            "Content-Length: {}\r\n\r\n{payload}",
            payload.len()
        ));

        let mut stream = tokio::net::TcpStream::connect(&host)
            .await
            .expect("connect");
        stream.write_all(req.as_bytes()).await.expect("send");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.expect("read");
        let raw = String::from_utf8(raw).expect("utf8 response");
        let (head, rest) = raw.split_once("\r\n\r\n").expect("http response");
        let status: u16 = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .expect("status");
        let chunked = head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked");
        let text = if chunked {
            dechunk(rest)
        } else {
            rest.to_string()
        };
        let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
        (status, json)
    }

    async fn ok(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let (status, v) = self.call(method, path, body, false).await;
        assert_eq!(status, 200, "{method} {path}: {v}");
        v
    }
}

/// Decode a chunked body (enough for one JSON document).
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

fn kind_name(kind: &InboxEventKind) -> &'static str {
    match kind {
        InboxEventKind::Comment { .. } => "comment",
        InboxEventKind::RequestOpened { .. } => "request_opened",
        InboxEventKind::Proposal { .. } => "proposal",
        InboxEventKind::Advice { .. } => "advice",
        InboxEventKind::TriageFailed { .. } => "triage_failed",
        InboxEventKind::Rejected { .. } => "rejected",
        InboxEventKind::ResolutionConfirmed { .. } => "resolution_confirmed",
        InboxEventKind::Delivered { .. } => "delivered",
        InboxEventKind::DeliveryFailed { .. } => "delivery_failed",
        InboxEventKind::PriorityChanged { .. } => "priority_changed",
        _ => "other",
    }
}

// TS-56: a worktree request raised through the agentctl ingress is triaged
// by the stub into a proposal; the operator edits and confirms it over HTTP;
// the pane receives exactly the edited, sanitised text, once, and the
// request folds to resolved with the full event chain.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ts56_a_worktree_request_is_triaged_edited_confirmed_and_delivered() {
    let d = Daemon::start("ok").await;
    let project = d.project("acme-demo", true);
    let created = d
        .ok(
            "POST",
            "/api/inbox",
            Some(serde_json::json!({"title": "Importer"})),
        )
        .await;
    let id = created["id"].as_str().expect("id").to_string();

    // Converted into one worktree, so the ingress knows where it may come from.
    let target = ConversionTarget {
        project_path: project.clone(),
        branch: "feat-x".into(),
        prompt: "Build the importer".into(),
        ..Default::default()
    };
    d.state
        .inbox
        .convert(&id, &[target], &RecordingRunner::default())
        .expect("convert");

    // The worktree agent raises it, as `sebenza-agentctl request` does.
    let (status, v) = d
        .call(
            "POST",
            "/api/runtime/events",
            Some(serde_json::json!({
                "type": "inbox.request",
                "worktreeId": "wt-id-1",
                "branch": "feat-x",
                "draftId": id,
                "worktreePath": "/wt/feat-x",
                "title": "Need a decision",
                "body": "Which fixture loader should the tests use?",
                "caller": "worktree",
            })),
            true,
        )
        .await;
    assert_eq!(status, 200, "{v}");
    let rid = v["requestId"].as_str().expect("requestId").to_string();

    // Triage runs on its own; wait for the proposal to land.
    let request = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let list = d
                .ok("GET", &format!("/api/inbox/{id}/requests"), None)
                .await;
            let r = list["requests"][0].clone();
            if r["status"] == "proposed" {
                return r;
            }
            assert_ne!(r["flagged"], true, "triage failed: {r}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("triage proposed in time");
    assert_eq!(request["requestId"], rid.as_str());
    assert_eq!(
        request["proposal"],
        "Use the existing fixture loader in tests/helpers."
    );
    let hash = request["proposalHash"].as_str().expect("hash").to_string();
    assert!(
        d.pane.0.lock().unwrap().is_empty(),
        "nothing before confirm"
    );
    let item = d.ok("GET", &format!("/api/inbox/{id}"), None).await;
    assert_eq!(item["priority"], "P1", "triage set the agent priority");

    // A stale hash is refused and sends nothing.
    let (status, _) = d
        .call(
            "POST",
            &format!("/api/inbox/{id}/requests/{rid}/confirm"),
            Some(serde_json::json!({"body": "x", "contentHash": "deadbeef"})),
            false,
        )
        .await;
    assert_eq!(status, 409);

    // The operator edits — including terminal escapes and tmux syntax a
    // careless paste might carry — and confirms against the hash shown.
    let edited = "Use tests/helpers/loader.rs\x1b[31m and #{pane_id} C-c keep it small.";
    let v = d
        .ok(
            "POST",
            &format!("/api/inbox/{id}/requests/{rid}/confirm"),
            Some(serde_json::json!({"body": edited, "contentHash": hash})),
        )
        .await;
    assert_eq!(v["request"]["status"], "resolved", "{v}");
    assert_eq!(v["request"]["confirmedText"], edited);

    let pasted = d.pane.0.lock().unwrap().clone();
    assert_eq!(pasted.len(), 1, "delivered exactly once");
    assert_eq!(
        pasted[0].0,
        WorktreeKey {
            project: project.clone(),
            branch: "feat-x".into()
        }
    );
    assert_eq!(pasted[0].1, prepare_resolution(&rid, edited));
    assert!(pasted[0].1.contains("Use tests/helpers/loader.rs"));
    assert!(pasted[0].1.contains("keep it small."));
    for bad in ["\x1b", "#{", "C-c", "fixture loader in tests/helpers"] {
        assert!(!pasted[0].1.contains(bad), "{bad:?} reached the pane");
    }

    let list = d
        .ok("GET", &format!("/api/inbox/{id}/requests"), None)
        .await;
    assert_eq!(list["requests"][0]["status"], "resolved");
    let chain: Vec<&str> = d
        .state
        .inbox
        .events(&id)
        .expect("events")
        .iter()
        .map(|e| kind_name(&e.kind))
        .collect();
    assert_eq!(
        chain,
        [
            "request_opened",
            "priority_changed",
            "proposal",
            "resolution_confirmed",
            "delivered"
        ]
    );
}

/// Creates each worktree as a real directory next to the project, the way a
/// relative `worktreeRoot: ../worktrees` lays them out, and reports git's
/// canonical path for it.
struct SiblingWorktreeRunner(PathBuf);
impl ConversionRunner for SiblingWorktreeRunner {
    fn create_worktree(&self, t: &ConversionTarget) -> Result<String, String> {
        let dir = self.0.join("worktrees").join(&t.branch);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let real = dir.canonicalize().map_err(|e| e.to_string())?;
        Ok(real.to_string_lossy().to_string())
    }
    fn write_note(&self, _p: &str, _b: &str) -> Result<(), String> {
        Ok(())
    }
    fn exclude_note(&self, _p: &str) -> Result<(), String> {
        Ok(())
    }
    fn record_origin(&self, _p: &str, _d: &str) -> Result<(), String> {
        Ok(())
    }
    fn send_prompt(&self, _t: &ConversionTarget, _p: &str) -> Result<(), String> {
        Ok(())
    }
    fn now(&self) -> String {
        "2026-11-05T00:00:00Z".into()
    }
}

// Nervous-system bug TS-14: the whole round trip through the real router. A
// worktree agent with a non-canonical SEBENZA_WORKTREE_PATH (relative
// worktreeRoot) raises a request; it is listed, counted, and notified; the
// operator confirms and the reply reaches the pane; a follow-up operator
// comment reaches the pane too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ts14_an_agent_request_round_trips_to_the_operator_and_back() {
    let d = Daemon::start("ok").await;
    let project = d.project("acme-demo", true);
    let created = d
        .ok(
            "POST",
            "/api/inbox",
            Some(serde_json::json!({"title": "Task graph"})),
        )
        .await;
    let id = created["id"].as_str().expect("id").to_string();
    let target = ConversionTarget {
        project_path: project.clone(),
        branch: "feat-x".into(),
        prompt: "Build it".into(),
        ..Default::default()
    };
    d.state
        .inbox
        .convert(&id, &[target], &SiblingWorktreeRunner(d.base.clone()))
        .expect("convert");

    // What the agent's environment holds under `worktreeRoot: ../worktrees`.
    let claimed = format!("{project}/../worktrees/feat-x");
    let (status, v) = d
        .call(
            "POST",
            "/api/runtime/events",
            Some(serde_json::json!({
                "type": "inbox.request",
                "worktreeId": "wt-id-1",
                "branch": "feat-x",
                "draftId": id,
                "worktreePath": claimed,
                "title": "Need a decision",
                "body": "SYNTHETIC-PHI-MARKER-123 which loader?",
                "caller": "worktree",
            })),
            true,
        )
        .await;
    assert_eq!(status, 200, "non-canonical path refused: {v}");
    let rid = v["requestId"].as_str().expect("requestId").to_string();

    // The operator can see it: listed, counted, badged and notified.
    let drafts = d.ok("GET", "/api/inbox", None).await;
    assert_eq!(drafts["drafts"][0]["openRequestCount"], 1);
    let summary = d.ok("GET", "/api/inbox/summary", None).await;
    assert_eq!(summary["openRequests"], 1);
    let app = d.state.manager.get_by_path(&project).expect("project");
    let note = app
        .notifications
        .list()
        .into_iter()
        .find(|n| n.r#type == "inbox_request")
        .expect("operator notified");
    assert!(note.message.contains("Need a decision"));
    assert!(!note.message.contains("SYNTHETIC-PHI-MARKER-123"));

    // Triage proposes; the operator confirms; the reply reaches the agent.
    let request = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let list = d
                .ok("GET", &format!("/api/inbox/{id}/requests"), None)
                .await;
            let r = list["requests"][0].clone();
            if r["status"] == "proposed" {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("triage proposed in time");
    let hash = request["proposalHash"].as_str().expect("hash").to_string();
    let v = d
        .ok(
            "POST",
            &format!("/api/inbox/{id}/requests/{rid}/confirm"),
            Some(serde_json::json!({"contentHash": hash})),
        )
        .await;
    assert_eq!(v["request"]["status"], "resolved", "{v}");
    let key = WorktreeKey {
        project: project.clone(),
        branch: "feat-x".into(),
    };
    {
        let pasted = d.pane.0.lock().unwrap().clone();
        assert_eq!(pasted.len(), 1, "reply delivered once");
        assert_eq!(pasted[0].0, key);
        assert!(pasted[0].1.contains(&rid));
    }
    let summary = d.ok("GET", "/api/inbox/summary", None).await;
    assert_eq!(
        summary["openRequests"], 0,
        "resolved requests stop counting"
    );

    // A follow-up operator comment on the worktree thread reaches it too.
    let v = d
        .ok(
            "POST",
            &format!("/api/inbox/{id}/comments"),
            Some(serde_json::json!({
                "body": "Also add a regression test.",
                "worktree": {"project": project, "branch": "feat-x"},
            })),
        )
        .await;
    assert_eq!(v["comment"]["delivery"], "delivered", "{v}");
    let pasted = d.pane.0.lock().unwrap().clone();
    assert_eq!(pasted.len(), 2);
    assert_eq!(pasted[1].0, key);
    assert!(pasted[1].1.contains("Operator comment on inbox item"));
    assert!(pasted[1].1.contains("Also add a regression test."));
}

// Graph/notes TS-53: convert, let the agent write a plan, read the graph
// through the real router. Volatile fields are masked; the rest is pinned.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ts53_graph_after_conversion_through_the_router() {
    let d = Daemon::start("ok").await;
    let project = d.project("acme-demo", true);
    let created = d
        .ok(
            "POST",
            "/api/inbox",
            Some(serde_json::json!({"title": "Task graph"})),
        )
        .await;
    let id = created["id"].as_str().expect("id").to_string();
    let target = ConversionTarget {
        project_path: project.clone(),
        branch: "feat-x".into(),
        prompt: "Build it".into(),
        ..Default::default()
    };
    d.state
        .inbox
        .convert(&id, &[target], &SiblingWorktreeRunner(d.base.clone()))
        .expect("convert");
    let wt = d.base.join("worktrees/feat-x").canonicalize().unwrap();
    // The agent's plan, and the daemon's knowledge of the worktree.
    let ws = wt.join(".ai/sebenza");
    std::fs::create_dir_all(ws.join("tracks/t_20261105")).unwrap();
    std::fs::write(
        ws.join("tracks.json"),
        r#"{"tracks":[{"track_id":"t_20261105","description":"Graph","status":"doing"}]}"#,
    )
    .unwrap();
    std::fs::write(
        ws.join("tracks/t_20261105/plan.json"),
        r#"{"phases":[{"id":"phase-1","name":"P","status":"doing","tasks":[
            {"id":"a","name":"A","status":"done"},
            {"id":"b","name":"B","status":"doing","depends_on":["a"]}]}]}"#,
    )
    .unwrap();
    let app = d.state.manager.get_by_path(&project).expect("project");
    app.runtime
        .lock()
        .unwrap()
        .upsert_worktree(common::services::project_runtime::UpsertInput {
            worktree_id: "id-feat-x".into(),
            kind: Default::default(),
            branch: "feat-x".into(),
            label: None,
            base_branch: None,
            path: wt.to_string_lossy().to_string(),
            profile: None,
            agent_name: Some("claude".into()),
            agent_terminal_stale: false,
            runtime: "host".into(),
            source: common::domain::model::WorktreeSource::Ui,
            oneshot: None,
            tabs: vec![],
            active_tab_id: None,
        });

    let mut g = d.ok("GET", &format!("/api/inbox/{id}/graph"), None).await;
    for volatile in ["generatedAt", "etag"] {
        assert!(g[volatile].is_string(), "{volatile}");
        g[volatile] = serde_json::Value::Null;
    }
    g["worktrees"][0]["agent"] = serde_json::Value::Null;
    let expected = r#"{
        "draftId": "__ID__", "generatedAt": null, "etag": null, "truncated": false,
        "progress": {"done": 1, "total": 2}, "failed": 0, "unavailable": 0,
        "worktrees": [{
            "id": "w:0", "branch": "feat-x", "projectPath": "__PROJECT__", "state": "ok",
            "errorCode": null, "progress": {"done": 1, "total": 2}, "agent": null,
            "tracks": [{
                "id": "t:0/t_20261105", "trackId": "t_20261105", "description": "Graph",
                "status": "doing", "currentPhase": "phase-1", "designing": false,
                "errorCode": null, "progress": {"done": 1, "total": 2},
                "phases": [{
                    "id": "p:0/t_20261105/phase-1", "phaseId": "phase-1", "name": "P",
                    "status": "doing", "progress": {"done": 1, "total": 2},
                    "tasks": [
                        {"id": "k:0/t_20261105/a", "taskId": "a", "name": "A", "status": "done",
                         "dependsOn": [], "request": null},
                        {"id": "k:0/t_20261105/b", "taskId": "b", "name": "B", "status": "doing",
                         "dependsOn": ["a"], "request": null}
                    ]
                }]
            }]
        }],
        "edges": [
            {"from": "w:0", "to": "t:0/t_20261105", "kind": "contains", "dangling": false, "flag": null},
            {"from": "t:0/t_20261105", "to": "p:0/t_20261105/phase-1", "kind": "contains", "dangling": false, "flag": null},
            {"from": "p:0/t_20261105/phase-1", "to": "k:0/t_20261105/a", "kind": "contains", "dangling": false, "flag": null},
            {"from": "p:0/t_20261105/phase-1", "to": "k:0/t_20261105/b", "kind": "contains", "dangling": false, "flag": null},
            {"from": "k:0/t_20261105/b", "to": "k:0/t_20261105/a", "kind": "depends_on", "dangling": false, "flag": null}
        ]
    }"#
    .replace("__ID__", &id)
    .replace("__PROJECT__", &project);
    let expected: serde_json::Value = serde_json::from_str(&expected).unwrap();
    assert_eq!(g, expected);
}

// TS-57: an item with two targets gets a system instruction per target from
// the stub over HTTP, is converted with them, and each worktree's launch
// prompt is architect-first (both projects have `.ai/sebenza/index.md`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ts57_two_targets_convert_with_instructions_and_launch_architect_first() {
    let d = Daemon::start("fixture:convert_instructions_two").await;
    let acme = d.project("acme-demo", true);
    let beta = d.project("beta-svc", true);
    let created = d
        .ok(
            "POST",
            "/api/inbox",
            Some(serde_json::json!({"title": "Bulk importer"})),
        )
        .await;
    let id = created["id"].as_str().expect("id").to_string();
    d.ok(
        "POST",
        &format!("/api/inbox/{id}/comments"),
        Some(serde_json::json!({"body": "the parser must stream"})),
    )
    .await;

    let resp = d
        .ok(
            "POST",
            &format!("/api/inbox/{id}/convert/instructions"),
            Some(serde_json::json!({"targets": [
                {"project": acme, "branch": "feat-x", "prompt": "Build the parser"},
                {"project": beta, "branch": "feat-y", "prompt": "Build the upload UI"},
            ]})),
        )
        .await;
    assert_eq!(resp["status"], "succeeded", "{resp}");
    assert_eq!(resp["fallback"], false);
    let targets = resp["targets"].as_array().expect("targets");
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0]["key"], "acme-demo/feat-x");
    assert_eq!(targets[1]["key"], "beta-svc/feat-y");
    for t in targets {
        assert_eq!(t["sebenzaWorkspace"], true);
        assert!(t["systemInstruction"].is_string(), "{t}");
    }

    // Convert with what the dialog submits: each operator prompt plus the
    // instruction it was given.
    let prompts = ["Build the parser", "Build the upload UI"];
    let submit: Vec<ConversionTarget> = targets
        .iter()
        .zip(prompts)
        .map(|(t, prompt)| ConversionTarget {
            project_path: t["project"].as_str().unwrap().into(),
            branch: t["branch"].as_str().unwrap().into(),
            prompt: prompt.into(),
            system_instruction: t["systemInstruction"].as_str().map(str::to_string),
            ..Default::default()
        })
        .collect();
    let runner = RecordingRunner::default();
    d.state
        .inbox
        .convert(&id, &submit, &runner)
        .expect("convert");

    let launched = runner.0.into_inner().unwrap();
    assert_eq!(launched.len(), 2, "two launch prompts");
    let expect = [
        (
            "feat-x",
            "Build the parser",
            "Architect the streaming parser only",
        ),
        (
            "feat-y",
            "Build the upload UI",
            "Architect the upload UI only",
        ),
    ];
    for ((branch, text), (want_branch, operator, system)) in launched.iter().zip(expect) {
        assert_eq!(branch, want_branch);
        let architect = text.find("sebenza-architect").expect("architect-first");
        assert!(architect < text.find(operator).expect("operator prompt"));
        assert!(text.contains(system), "{text}");
    }

    let history = d
        .ok("GET", &format!("/api/inbox/{id}/conversions"), None)
        .await;
    let history = history["conversions"].as_array().expect("conversions");
    assert_eq!(history.len(), 2);
    for h in history {
        assert_eq!(h["architectFirst"], true, "{h}");
        assert!(h["systemInstruction"].is_string(), "{h}");
    }
}

// --- Track completion (TS-01, TS-63, TS-65, TS-66) -------------------------

/// A real repo served by the daemon, whose worktree `id-feat` on `feat`
/// carries the track `TRACK`.
fn completion_project(d: &Daemon) -> crate::services::review_test_repo::TestRepo {
    let r = crate::services::review_test_repo::TestRepo::new("e2e");
    let app = d.state.manager.add_ephemeral(&r.repo.to_string_lossy());
    app.runtime
        .lock()
        .unwrap()
        .upsert_worktree(common::services::project_runtime::UpsertInput {
            worktree_id: "id-feat".into(),
            kind: Default::default(),
            branch: "feat".into(),
            label: None,
            base_branch: Some("main".into()),
            path: r.worktree.to_string_lossy().to_string(),
            profile: None,
            agent_name: Some("claude".into()),
            agent_terminal_stale: false,
            runtime: "host".into(),
            source: common::domain::model::WorktreeSource::Ui,
            oneshot: None,
            tabs: vec![],
            active_tab_id: None,
        });
    r
}

fn completion_log(d: &Daemon) -> common::adapters::review_log::ReviewState {
    let app = d.state.manager.list().into_iter().next().expect("project");
    d.state.completion.log(&app.prefix).open().expect("log")
}

async fn complete(d: &Daemon, track: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    d.call(
        "POST",
        &format!("/api/tracks/{track}/complete"),
        Some(body),
        true,
    )
    .await
}

#[tokio::test]
async fn ts01_agentctl_complete_enqueues_one_job_and_projects_done() {
    use crate::services::review_test_repo::{OTHER_TRACK, TRACK};
    let d = Daemon::start("ok").await;
    let r = completion_project(&d);
    let (status, v) = complete(&d, TRACK, serde_json::json!({"worktreeId": "id-feat"})).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["appended"], true);
    assert_eq!(v["projected"], "applied");
    assert_eq!(v["job"]["headSha"], r.head());
    assert_eq!(v["job"]["baseSha"], r.main_head());
    assert_eq!(v["job"]["standardsRev"].as_str().map(str::len), Some(64));
    let log = completion_log(&d);
    assert_eq!(log.job_count(TRACK), 1);
    assert_eq!(log.completions[0].credential.as_deref(), Some("control"));
    assert_eq!(r.track_status(TRACK), "done");
    assert_eq!(r.track_status(OTHER_TRACK), "doing");
}

#[tokio::test]
async fn ts63_the_control_token_can_only_enqueue() {
    use crate::services::review_test_repo::TRACK;
    let d = Daemon::start("ok").await;
    let _r = completion_project(&d);
    let forged = serde_json::json!({"worktreeId": "id-feat", "status": "signed-off"});
    let (status, _) = complete(&d, TRACK, forged).await;
    assert_eq!(
        status, 422,
        "no status, sign-off or finding field is accepted"
    );
    assert_eq!(completion_log(&d).completions.len(), 0);
    let (status, v) = complete(&d, TRACK, serde_json::json!({"worktreeId": "id-feat"})).await;
    assert_eq!(status, 200, "{v}");
    let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, vec!["appended", "job", "ok", "projected"]);
    let log = completion_log(&d);
    assert_eq!(log.completions.len(), 1);
    assert_eq!(log.completions[0].credential.as_deref(), Some("control"));
}

#[tokio::test]
async fn ts65_completing_again_at_the_same_head_is_idempotent() {
    use crate::services::review_test_repo::TRACK;
    let d = Daemon::start("ok").await;
    let _r = completion_project(&d);
    let body = serde_json::json!({"worktreeId": "id-feat"});
    let (s1, first) = complete(&d, TRACK, body.clone()).await;
    let (s2, again) = complete(&d, TRACK, body).await;
    assert_eq!((s1, s2), (200, 200));
    assert_eq!(again["appended"], false);
    assert_eq!(again["projected"], "already_done");
    assert_eq!(again["job"], first["job"]);
    assert_eq!(completion_log(&d).job_count(TRACK), 1);
}

#[tokio::test]
async fn ts66_another_worktrees_track_is_refused_and_appends_nothing() {
    use crate::services::review_test_repo::OTHER_TRACK;
    let d = Daemon::start("ok").await;
    let _r = completion_project(&d);
    let (status, v) = complete(
        &d,
        OTHER_TRACK,
        serde_json::json!({"worktreeId": "id-feat"}),
    )
    .await;
    assert_eq!(
        (status, v["code"].as_str()),
        (403, Some("foreign_track")),
        "{v}"
    );
    let (status, v) = complete(
        &d,
        OTHER_TRACK,
        serde_json::json!({"worktreeId": "id-elsewhere"}),
    )
    .await;
    assert_eq!(
        (status, v["code"].as_str()),
        (404, Some("unknown_worktree")),
        "{v}"
    );
    assert_eq!(completion_log(&d).completions.len(), 0);
}

// --- Review runs (TS-22, TS-37, TS-56, TS-60, TS-74) -------------------------

/// A completed track with two rules at its base, served by the daemon.
async fn review_project(d: &Daemon) -> (crate::services::review_test_repo::TestRepo, String) {
    use crate::services::review_test_repo::TRACK;
    use crate::services::standards_review::run_store::tests::{RULES, STYLE};
    let r = completion_project(d);
    r.commit_on_main(STYLE, RULES);
    r.merge_main_into_feat();
    let (status, v) = approve_plan(d, &operator_token(d)).await;
    assert_eq!(status, 200, "{v}");
    let (status, v) = complete(d, TRACK, serde_json::json!({"worktreeId": "id-feat"})).await;
    assert_eq!(status, 200, "{v}");
    let prefix = d.state.manager.list()[0].prefix.clone();
    (r, prefix)
}

fn operator_token(d: &Daemon) -> String {
    d.state
        .operator
        .issue_session_at(std::time::SystemTime::now())
        .0
}

/// `POST /api/tracks/{TRACK}/plan/approve` with `bearer`.
async fn approve_plan(d: &Daemon, bearer: &str) -> (u16, serde_json::Value) {
    use crate::services::review_test_repo::TRACK;
    let path = format!("/api/tracks/{TRACK}/plan/approve");
    d.call_as("POST", &path, Some(bearer), None, "").await
}

fn plan_approvals(d: &Daemon, prefix: &str) -> Vec<common::domain::review_records::PlanApproved> {
    use common::adapters::review_log::ReviewEventKind;
    let events = d.state.completion.log(prefix).events().expect("log");
    events
        .into_iter()
        .filter_map(|e| match e.kind {
            ReviewEventKind::PlanApproved(a) => Some(a),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn ts45_ts64_only_the_operator_token_approves_a_plan() {
    use crate::services::review_test_repo::{PLAN, TASK, plan_json};
    let d = Daemon::start("ok").await;
    let r = completion_project(&d);
    let committed = plan_json(&[(TASK, Some(&["src.rs"]))]);
    r.commit_on_feat(PLAN, &committed);
    let prefix = d.state.manager.list()[0].prefix.clone();
    let (status, v) = approve_plan(&d, TOKEN).await;
    assert_eq!(
        (status, v["code"].as_str()),
        (403, Some("operator_required")),
        "{v}"
    );
    assert!(
        plan_approvals(&d, &prefix).is_empty(),
        "a refusal records nothing"
    );

    let (status, v) = approve_plan(&d, &operator_token(&d)).await;
    assert_eq!(status, 200, "{v}");
    let want = common::domain::solution::sha256_hex(&committed);
    assert_eq!(v["planSha256"], want.as_str());
    assert_eq!(v["headSha"], r.head().as_str());
    assert_eq!(
        v["tasks"],
        serde_json::json!([{"taskId": TASK, "files": ["src.rs"]}])
    );
    let approvals = plan_approvals(&d, &prefix);
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        approvals[0].plan_sha256, want,
        "the boundary hash is the approved plan's"
    );
    assert_eq!(approvals[0].credential, "operator_session");
}

#[tokio::test]
async fn ts08_a_one_file_track_is_reviewed_on_that_file_only() {
    use crate::services::review_test_repo::{PLAN, TASK, TRACK, plan_json};
    use crate::services::standards_review::run_store::tests::{RULES, STYLE, clean};
    let d = Daemon::start("ok").await;
    let r = completion_project(&d);
    r.commit_on_main(STYLE, RULES);
    r.commit_on_main("lib.rs", "// untouched\nfn lib() {}\n");
    r.merge_main_into_feat();
    r.commit_on_feat(PLAN, &plan_json(&[(TASK, Some(&["src.rs"]))]));
    r.commit_on_feat("src.rs", "fn main() {\n    run();\n}\n");
    assert_eq!(approve_plan(&d, &operator_token(&d)).await.0, 200);
    let (status, v) = complete(&d, TRACK, serde_json::json!({"worktreeId": "id-feat"})).await;
    assert_eq!(status, 200, "{v}");
    let prefix = d.state.manager.list()[0].prefix.clone();

    let run = lease(&d, &prefix);
    let (status, v) = claim(&d, Some(run.token.expose()), "first").await;
    assert_eq!(status, 200, "{v}");
    let files = v["files"].as_array().expect("files");
    assert_eq!(files.len(), 1, "{v}");
    assert_eq!(files[0]["path"], "src.rs");
    for hunk in files[0]["hunks"].as_array().expect("hunks") {
        assert_eq!(hunk["anchor"]["path"], "src.rs");
    }
    let stub = serde_json::json!({
        "dispositions": [clean("plain", "src.rs"), {"ruleId": "short", "path": "src.rs", "outcome": "finding"}],
        "findings": [{"ruleId": "short", "path": "src.rs", "line": 2, "description": "main does too much."}]
    });
    let (status, v) = submit(&d, &run.run_id, run.token.expose(), &stub).await;
    assert_eq!(status, 200, "{v}");
    let f = d
        .ok(
            "GET",
            &format!("/{prefix}/api/review/tracks/{TRACK}/findings"),
            None,
        )
        .await;
    let findings = f["findings"].as_array().expect("findings");
    assert_eq!(
        findings.len(),
        1,
        "no finding or edit outside the file: {f}"
    );
    assert_eq!(findings[0]["ruleId"], "short");
    assert_eq!(findings[0]["anchor"]["path"], "src.rs");
    assert_eq!(findings[0]["anchor"]["line"], 2);
    let lib = crate::services::review_test_repo::git(&r.worktree, &["show", "HEAD:lib.rs"]);
    assert_eq!(
        lib, "// untouched\nfn lib() {}",
        "the comment in the untouched file stays"
    );
}

#[tokio::test]
async fn ts12_an_untrailered_commit_makes_the_job_scope_unproven() {
    let d = Daemon::start("ok").await;
    let (r, prefix) = review_project(&d).await;
    let bare = r.commit_on_feat_with("notes.txt", "x\n", "hand-made");
    let body = serde_json::json!({"worktreeId": "id-feat"});
    assert_eq!(
        complete(&d, crate::services::review_test_repo::TRACK, body)
            .await
            .0,
        200
    );
    let run = lease(&d, &prefix);
    let (status, v) = claim(&d, Some(run.token.expose()), "first").await;
    assert_eq!(
        (status, v["code"].as_str()),
        (409, Some("scope_unproven")),
        "{v}"
    );
    let s = review_status(&d, &prefix).await;
    assert_eq!(s["job"]["state"], "scope-unproven", "{s}");
    assert_eq!(s["job"]["blockers"][0]["reason"], "missing-trailer");
    assert_eq!(s["job"]["blockers"][0]["commit"], bare.as_str());
}

fn lease(d: &Daemon, prefix: &str) -> crate::services::standards_review::run_store::LeasedRun {
    use crate::services::standards_review::run_store::now_secs;
    d.state
        .review_runs()
        .lease_next(prefix, now_secs())
        .expect("log")
        .expect("a queued job")
}

fn review_events(d: &Daemon, prefix: &str) -> usize {
    d.state.completion.log(prefix).events().expect("log").len()
}

async fn claim(d: &Daemon, bearer: Option<&str>, kind: &str) -> (u16, serde_json::Value) {
    let body = serde_json::json!({ "kind": kind }).to_string();
    d.call_as("POST", "/api/review-runs/claim", bearer, None, &body)
        .await
}

async fn submit(
    d: &Daemon,
    run_id: &str,
    bearer: &str,
    body: &serde_json::Value,
) -> (u16, serde_json::Value) {
    let path = format!("/api/review-runs/{run_id}/submit");
    d.call_as("POST", &path, Some(bearer), None, &body.to_string())
        .await
}

/// Every (rule, file) pair of the review project dispositioned clean.
fn clean_matrix() -> serde_json::Value {
    use crate::services::standards_review::run_store::tests::clean;
    serde_json::json!({ "dispositions": [
        clean("plain", "src.rs"),
        clean("short", "src.rs"),
    ]})
}

async fn review_status(d: &Daemon, prefix: &str) -> serde_json::Value {
    use crate::services::review_test_repo::TRACK;
    d.ok("GET", &format!("/{prefix}/api/review/tracks/{TRACK}"), None)
        .await
}

#[tokio::test]
async fn ts22_forged_verdict_fields_are_rejected_and_the_verdict_stays_daemon_derived() {
    let d = Daemon::start("ok").await;
    let (_r, prefix) = review_project(&d).await;
    let run = lease(&d, &prefix);
    let (status, v) = claim(&d, Some(run.token.expose()), "first").await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["runId"], run.run_id.as_str());
    assert_eq!(v["manifest"].as_array().map(Vec::len), Some(2));
    assert_eq!(v["files"][0]["path"], "src.rs");
    let mut forged = clean_matrix();
    forged["verdict"] = serde_json::json!({"passing": true});
    let (status, v) = submit(&d, &run.run_id, run.token.expose(), &forged).await;
    assert_eq!(
        (status, v["code"].as_str()),
        (422, Some("submission_rejected")),
        "{v}"
    );
    assert!(
        v["error"]
            .as_str()
            .unwrap_or("")
            .contains("unknown field `verdict`"),
        "{v}"
    );
    let s = review_status(&d, &prefix).await;
    assert_eq!(
        s["job"]["verdict"],
        serde_json::Value::Null,
        "no verdict from a forged field"
    );
    assert_eq!(s["job"]["attempts"]["first"], 1);

    let run = lease(&d, &prefix);
    let (status, v) = submit(&d, &run.run_id, run.token.expose(), &clean_matrix()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["verdict"]["passing"], false);
    let s = review_status(&d, &prefix).await;
    assert_eq!(s["job"]["state"], "verdict");
    assert_eq!(
        s["job"]["verdict"]["reasons"],
        serde_json::json!(["existing-checks-missing"])
    );
}

#[tokio::test]
async fn ts74_run_routes_refuse_other_credentials_and_replayed_tokens() {
    use crate::services::standards_review::run_token::RunToken;
    let d = Daemon::start("ok").await;
    let (_r, prefix) = review_project(&d).await;
    let run = lease(&d, &prefix);
    let before = review_events(&d, &prefix);
    let (operator, _) = d
        .state
        .operator
        .issue_session_at(std::time::SystemTime::now());
    let forged = RunToken::mint();
    for (bearer, code) in [
        (Some(TOKEN), "wrong_credential"),
        (Some(operator.as_str()), "wrong_credential"),
        (None, "run_token_required"),
        (Some(forged.expose()), "bad_run_token"),
    ] {
        let (status, v) = claim(&d, bearer, "first").await;
        assert_eq!((status, v["code"].as_str()), (401, Some(code)), "{v}");
        let (status, v) = submit(&d, &run.run_id, bearer.unwrap_or("none"), &clean_matrix()).await;
        assert_eq!(status, 401, "{v}");
    }
    let (status, v) = claim(&d, Some(run.token.expose()), "second").await;
    assert_eq!(
        (status, v["code"].as_str()),
        (403, Some("token_wrong_kind")),
        "{v}"
    );
    let (status, v) = submit(&d, "run_other", run.token.expose(), &clean_matrix()).await;
    assert_eq!(
        (status, v["code"].as_str()),
        (403, Some("token_wrong_run")),
        "{v}"
    );
    assert_eq!(
        review_events(&d, &prefix),
        before,
        "refusals change nothing"
    );

    let (status, v) = submit(&d, &run.run_id, run.token.expose(), &clean_matrix()).await;
    assert_eq!(status, 200, "{v}");
    let after = review_events(&d, &prefix);
    let (status, v) = submit(&d, &run.run_id, run.token.expose(), &clean_matrix()).await;
    assert_eq!(
        (status, v["code"].as_str()),
        (401, Some("run_token_used")),
        "{v}"
    );
    assert_eq!(review_events(&d, &prefix), after);
}

#[tokio::test]
async fn ts37_three_expired_first_run_leases_make_the_review_failed() {
    use crate::services::completion_reconciler::{project_worktrees, reap_expired_leases_at};
    let d = Daemon::start("ok").await;
    let (_r, prefix) = review_project(&d).await;
    let app = d.state.manager.list()[0].clone();
    for attempt in 1..=3 {
        let run = lease(&d, &prefix);
        assert_eq!(run.attempt, attempt);
        let reaped = reap_expired_leases_at(
            &d.state.completion,
            &project_worktrees(&app),
            run.lease_until,
        );
        assert_eq!(reaped, 1);
    }
    let s = review_status(&d, &prefix).await;
    assert_eq!(s["job"]["state"], "review-failed", "{s}");
    assert_eq!(
        s["job"]["attempts"],
        serde_json::json!({"first": 3, "second": 0})
    );
    let events = d.state.completion.log(&prefix).events().expect("log");
    let expiries = events
        .iter()
        .filter(|e| serde_json::to_value(&e.kind).unwrap()["cause"] == "lease-expired")
        .count();
    assert_eq!(expiries, 3);
}

#[tokio::test]
async fn ts60_attempts_count_per_run_kind_through_the_api() {
    use crate::services::standards_review::run_store::now_secs;
    use common::domain::review_records::RunKind;
    let d = Daemon::start("ok").await;
    let (_r, prefix) = review_project(&d).await;
    let runs = d.state.review_runs();
    let empty = serde_json::json!({"dispositions": []});
    let mut job_id = String::new();
    for _ in 0..2 {
        let first = lease(&d, &prefix);
        job_id = first.job_id.clone();
        let (status, _) = submit(&d, &first.run_id, first.token.expose(), &empty).await;
        assert_eq!(status, 422);
        let second = runs
            .lease_run(&prefix, &job_id, RunKind::Second, now_secs())
            .expect("log")
            .expect("leased");
        let (status, v) = claim(&d, Some(second.token.expose()), "second").await;
        assert_eq!(status, 200, "{v}");
        assert_eq!(
            v["priorFindings"],
            serde_json::json!([]),
            "a second run sees no prior output"
        );
        let (status, _) = submit(&d, &second.run_id, second.token.expose(), &empty).await;
        assert_eq!(status, 422);
    }
    let s = review_status(&d, &prefix).await;
    assert_eq!(
        s["job"]["attempts"],
        serde_json::json!({"first": 2, "second": 2})
    );
    assert_ne!(
        s["job"]["state"], "review-failed",
        "two of each kind is not three of one"
    );
    let second = runs
        .lease_run(&prefix, &job_id, RunKind::Second, now_secs())
        .expect("log")
        .expect("leased");
    let (status, _) = submit(&d, &second.run_id, second.token.expose(), &empty).await;
    assert_eq!(status, 422);
    assert_eq!(
        review_status(&d, &prefix).await["job"]["state"],
        "review-failed"
    );
}

#[tokio::test]
async fn ts56_track_review_runs_leave_project_reviews_alone() {
    let d = Daemon::start("ok").await;
    let (_r, prefix) = review_project(&d).await;
    let run = lease(&d, &prefix);
    let (status, _) = submit(&d, &run.run_id, run.token.expose(), &clean_matrix()).await;
    assert_eq!(status, 200);
    assert!(d.state.reviews.job(&run.run_id).is_none());
    assert!(d.state.reviews.job(&run.job_id).is_none());
    let (status, _) = d
        .call(
            "GET",
            &format!("/api/runtime/review/{}", run.run_id),
            None,
            true,
        )
        .await;
    assert_eq!(
        status, 404,
        "project review polling knows nothing of track runs"
    );
    d.state.reviews.fail_running("shutdown");
    assert_eq!(review_status(&d, &prefix).await["job"]["state"], "verdict");
}

#[tokio::test]
async fn the_findings_route_lists_the_tracks_findings_with_status() {
    use crate::services::review_test_repo::TRACK;
    let d = Daemon::start("ok").await;
    let (_r, prefix) = review_project(&d).await;
    let run = lease(&d, &prefix);
    let body = serde_json::json!({
        "dispositions": [
            crate::services::standards_review::run_store::tests::clean("plain", "src.rs"),
            {"ruleId": "short", "path": "src.rs", "outcome": "finding"}
        ],
        "findings": [{"ruleId": "short", "path": "src.rs", "line": 1, "description": "main is too long."}]
    });
    let (status, v) = submit(&d, &run.run_id, run.token.expose(), &body).await;
    assert_eq!(
        (status, v["findingsOpened"].as_u64()),
        (200, Some(1)),
        "{v}"
    );
    let f = d
        .ok(
            "GET",
            &format!("/{prefix}/api/review/tracks/{TRACK}/findings"),
            None,
        )
        .await;
    assert_eq!(f["findings"][0]["status"], "open");
    assert_eq!(f["findings"][0]["anchor"]["path"], "src.rs");
    let (status, _) = d
        .call(
            "GET",
            &format!("/{prefix}/api/review/tracks/bad..id"),
            None,
            false,
        )
        .await;
    assert_eq!(status, 400);
}
