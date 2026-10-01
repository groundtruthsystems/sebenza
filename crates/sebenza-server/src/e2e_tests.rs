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
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n"
        );
        if !bearer_only {
            req.push_str(&format!("Origin: http://{host}\r\n"));
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
