//! Loopback end-to-end tests for the coordinator chat and the solution
//! pipeline (TS-04, TS-05, TS-07..TS-17, TS-20, TS-44, TS-45, TS-47, TS-11).
//!
//! The real router runs with every agent CLI replaced by the checked-in stub
//! (`services/role_runner/testdata/stub-role.sh`). A test steers the stub with
//! `STUB-…` lines in the item's notes, which reach every role's prompt:
//! `STUB-REPLY-<role>[-<round>]: <file>` picks a reply, `STUB-LOG: <dir>`
//! records each launch, and `STUB-MODE: hang` makes it hang.

use crate::adapters::projects_registry::{ProjectEntry, ProjectsRegistry};
use crate::domain::inbox_events::InboxEventKind;
use crate::operator_e2e_tests::{Daemon, TOKEN};
use common::domain::solution::{ReviewLevel, SolutionPhase};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Item {
    id: String,
    op: String,
    log: PathBuf,
    dir: PathBuf,
}

const ANALYST: &str = r##"{"role":"system-analyst","schema_version":1,"documents":[{"name":"requirements.md","content":"# Requirements\n- FR-01 invoices"}]}"##;

fn architect(projects: &[&str]) -> String {
    let rows: String = projects
        .iter()
        .map(|p| format!("| {p} | builds it |\\n"))
        .collect();
    format!(
        r##"{{"role":"solutions-architect","schema_version":1,"documents":[{{"name":"solution-design.md","content":"# Design\n\n## Affected projects\n\n| Project | Responsibility |\n|---|---|\n{rows}\n## Risks\n- none"}}]}}"##
    )
}

fn reviewer(verdict: &str, problems: &[(&str, &str)]) -> String {
    let p: Vec<String> = problems
        .iter()
        .map(|(id, t)| format!(r#"{{"problem_id":"{id}","title":"{t}","detail":"fix {t}"}}"#))
        .collect();
    format!(
        r#"{{"role":"solution-reviewer","schema_version":1,"verdict":"{verdict}","problems":[{}],"report":"round done"}}"#,
        p.join(",")
    )
}

const COORD: &str = r#"{"role":"solution-coordinator","schema_version":1,"reply":"Shall I start the design? (start design)","report":"Agreed: billing builds invoices."}"#;

impl Item {
    /// A solution item whose stub replies come from `replies` (role[-round] -> json).
    async fn new(d: &Daemon, replies: &[(&str, String)], mode: &str) -> Item {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = d.base.join(format!("item-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log");
        let mut lines = vec![
            format!("STUB-LOG: {}", log.display()),
            format!("STUB-MODE: {mode}"),
        ];
        for (key, json) in replies {
            let file = dir.join(format!("{key}.json"));
            std::fs::write(&file, json).unwrap();
            lines.push(format!("STUB-REPLY-{key}: {}", file.display()));
        }
        let op = d.operator_token().await;
        let mut c = d.spa("POST", "/api/inbox", TOKEN);
        c.body = Some(serde_json::json!({"title": "Billing", "kind": "solution"}));
        let (code, v) = d.send(c).await;
        assert_eq!(code, 200, "{v}");
        assert_eq!(v["kind"], "solution");
        let id = v["id"].as_str().unwrap().to_string();
        // The brief: one note carrying the stub's steering lines.
        let (notes, lh) = d.state.inbox.notes(&id).unwrap();
        let _ = notes;
        d.state
            .inbox
            .add_note(
                &id,
                &None,
                &lh,
                "general",
                "Brief",
                &format!("Invoices for acme.\n{}", lines.join("\n")),
                None,
            )
            .unwrap();
        Item { id, op, log, dir }
    }

    async fn post(
        &self,
        d: &Daemon,
        path: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let mut c = d.spa("POST", &format!("/api/inbox/{}/{path}", self.id), &self.op);
        c.body = Some(body);
        d.send(c).await
    }

    async fn settle(&self, d: &Daemon) -> serde_json::Value {
        d.state.solution.settle(&self.id).await;
        let (code, v) = d
            .send(d.spa("GET", &format!("/api/inbox/{}/solution", self.id), TOKEN))
            .await;
        assert_eq!(code, 200, "{v}");
        v
    }

    fn order(&self) -> Vec<String> {
        std::fs::read_to_string(self.log.join("order"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .collect()
    }

    /// Every argv the stub recorded, joined.
    fn argvs(&self) -> Vec<String> {
        std::fs::read_dir(&self.log)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("argv."))
                    .map(|e| std::fs::read_to_string(e.path()).unwrap())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn transcript(&self, d: &Daemon) -> Vec<serde_json::Value> {
        let (_, v) = d
            .send(d.spa(
                "GET",
                &format!("/api/inbox/{}/coordinator/transcript", self.id),
                TOKEN,
            ))
            .await;
        v["entries"].as_array().cloned().unwrap_or_default()
    }

    fn events(&self, d: &Daemon) -> Vec<InboxEventKind> {
        d.store
            .read_events(&self.id)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }
}

async fn configure(d: &Daemon, solution_reviewer: serde_json::Value) {
    let op = d.operator_token().await;
    let mut c = d.spa("PUT", "/api/factory/config", &op);
    c.body = Some(serde_json::json!({
        "solution": {"designer": {"agent": "claude", "model": "claude-opus-5-5"}, "reviewer": solution_reviewer},
        "project": {"reviewer": {"agent": "claude", "model": "claude-fable-5-2"}}
    }));
    let (code, v) = d.send(c).await;
    assert_eq!(code, 200, "{v}");
}

fn register(d: &Daemon, name: &str) {
    ProjectsRegistry::with_file(d.base.join("projects.json")).add(ProjectEntry {
        path: format!("/code/{name}"),
        name: name.to_string(),
        added_at: 0,
    });
}

async fn daemon() -> Daemon {
    let d = Daemon::start().await;
    configure(&d, serde_json::json!({"agent": "codex"})).await;
    register(&d, "acme-billing");
    d
}

// TS-04 + TS-08 + TS-47 + TS-45: a turn streams into a persisted transcript;
// a coordinator "start" in prose starts nothing; oversize and unconfirmed
// secret-looking turns are refused before anything runs.
#[tokio::test]
async fn ts04_ts08_coordinator_turns_persist_and_never_start_design() {
    let d = daemon().await;
    let it = Item::new(&d, &[("solution-coordinator", COORD.into())], "reply").await;
    let (code, v) = it
        .post(
            &d,
            "coordinator/turn",
            serde_json::json!({"message": "We need invoices."}),
        )
        .await;
    assert_eq!(code, 202, "{v}");
    let turn = v["turnId"].as_str().unwrap().to_string();
    let state = it.settle(&d).await;
    let t = it.transcript(&d).await;
    assert_eq!(t.len(), 2);
    assert_eq!(
        (t[0]["kind"].as_str(), t[0]["text"].as_str()),
        (Some("operator"), Some("We need invoices."))
    );
    assert_eq!(t[1]["kind"], "coordinator");
    assert_eq!(t[1]["turnId"], turn.as_str());
    assert!(t[1]["text"].as_str().unwrap().contains("start design"));
    // TS-04: persisted — a fresh store reads the same transcript.
    let fresh = common::adapters::inbox_store::InboxStore::with_dir(d.base.join("inbox"));
    assert_eq!(fresh.read_transcript(&it.id).unwrap().len(), 2);
    assert!(
        it.events(&d)
            .iter()
            .any(|k| matches!(k, InboxEventKind::CoordinatorTurn { .. }))
    );
    // TS-08: no analyst ran and the phase did not move.
    assert_eq!(state["state"]["phase"], "requirements");
    assert_eq!(it.order(), vec!["solution-coordinator".to_string()]);

    // TS-47: an oversized turn is rejected and nothing is recorded.
    let (code, v) = it
        .post(
            &d,
            "coordinator/turn",
            serde_json::json!({"message": "x".repeat(16 * 1024 + 1)}),
        )
        .await;
    assert_eq!((code, v["code"].as_str()), (413, Some("too_large")));
    // TS-45 (input): a secret-looking message needs confirmation.
    let secret = "deploy with AKIAABCDEFGHIJKLMNOP";
    let (code, v) = it
        .post(
            &d,
            "coordinator/turn",
            serde_json::json!({"message": secret}),
        )
        .await;
    assert_eq!((code, v["code"].as_str()), (422, Some("scan_warning")));
    assert_eq!(
        it.transcript(&d).await.len(),
        2,
        "refused turns leave no trace"
    );
    let (code, _) = it
        .post(
            &d,
            "coordinator/turn",
            serde_json::json!({"message": secret, "confirmWarnings": true}),
        )
        .await;
    assert_eq!(code, 202);
    it.settle(&d).await;
    let t = it.transcript(&d).await;
    assert_eq!(t[2]["warnings"][0], "AWS access key");

    // Operator-only: the shared token cannot chat.
    let mut c = d.spa(
        "POST",
        &format!("/api/inbox/{}/coordinator/turn", it.id),
        TOKEN,
    );
    c.body = Some(serde_json::json!({"message": "hi"}));
    assert_eq!(d.send(c).await.1["code"], "operator_required");
}

// TS-05: proposals of three note types are applied by the operator, badged
// `coordinator` (NoteUpserted), applied once, and notes reach prompts fenced.
#[tokio::test]
async fn ts05_note_proposals_are_applied_and_badged() {
    let d = daemon().await;
    let reply = r#"{"role":"solution-coordinator","schema_version":1,"reply":"Here are three notes.","note_proposals":[
        {"note_id":null,"type":"general","title":"Overview","body":"Invoices for acme."},
        {"note_id":null,"type":"constraints","title":"Constraints","body":"- loopback only"},
        {"note_id":null,"type":"acceptance_criteria","title":"Acceptance Criteria","body":"1. An invoice is emailed."}]}"#;
    let it = Item::new(&d, &[("solution-coordinator", reply.into())], "reply").await;
    // Run the coordinator on codex so its prompt (argv) can be inspected.
    let mut c = d.spa(
        "PUT",
        &format!("/api/inbox/{}/factory/agents", it.id),
        &it.op,
    );
    c.body = Some(serde_json::json!({"coordinator": {"agent": "codex"}}));
    assert_eq!(d.send(c).await.0, 200);
    let (_, v) = it
        .post(
            &d,
            "coordinator/turn",
            serde_json::json!({"message": "Draft the notes."}),
        )
        .await;
    let turn = v["turnId"].as_str().unwrap().to_string();
    it.settle(&d).await;
    let prompt = it.argvs().join("\n");
    assert!(
        prompt.contains("<untrusted-data name=\"note-"),
        "notes are fenced as data"
    );

    for i in 0..3 {
        let (code, v) = it
            .post(
                &d,
                &format!("coordinator/proposals/{turn}/{i}/apply"),
                serde_json::json!({}),
            )
            .await;
        assert_eq!(code, 200, "{v}");
    }
    let (code, _) = it
        .post(
            &d,
            &format!("coordinator/proposals/{turn}/0/apply"),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(code, 409, "applied once");
    let (notes, _) = d.state.inbox.notes(&it.id).unwrap();
    let types: Vec<&str> = notes.iter().map(|n| n.note_type.as_str()).collect();
    assert!(types.contains(&"constraints") && types.contains(&"acceptance_criteria"));
    let state = d.state.solution.fold(&it.id).unwrap();
    assert_eq!(state.coordinator_notes.len(), 3);
    for (nid, hash) in &state.coordinator_notes {
        let note = notes.iter().find(|n| &n.id == nid).unwrap();
        assert_eq!(&note.hash(), hash, "the badge binds to the applied text");
    }
    assert!(
        d.comments(&it.id)
            .iter()
            .any(|(c, b)| c.as_deref() == Some("operator_session")
                && b.contains("coordinator.proposal.applied"))
    );
}

// TS-07: after an agent switch the next coordinator receives the transcript digest.
#[tokio::test]
async fn ts07_an_agent_switch_keeps_the_conversation() {
    let d = daemon().await;
    let it = Item::new(&d, &[("solution-coordinator", COORD.into())], "reply").await;
    it.post(
        &d,
        "coordinator/turn",
        serde_json::json!({"message": "Remember: euros only."}),
    )
    .await;
    it.settle(&d).await;
    let mut c = d.spa(
        "PUT",
        &format!("/api/inbox/{}/factory/agents", it.id),
        &it.op,
    );
    c.body = Some(serde_json::json!({"coordinator": {"agent": "codex"}}));
    assert_eq!(d.send(c).await.0, 200);
    it.post(
        &d,
        "coordinator/turn",
        serde_json::json!({"message": "Next?"}),
    )
    .await;
    it.settle(&d).await;
    let codex = it
        .argvs()
        .into_iter()
        .find(|a| a.starts_with("exec"))
        .expect("codex ran");
    assert!(codex.contains("transcript-digest"));
    assert!(
        codex.contains("Remember: euros only."),
        "the digest carries the earlier turn"
    );
    let t = it.transcript(&d).await;
    assert_eq!(t.last().unwrap()["agent"], "codex");
}

// TS-09 + TS-10 + TS-12 + TS-17: the operator's start runs analyst then
// architect then round 1; an unregistered project becomes a problem; "round
// 1 of 3" shows; a revision precedes round 2; the per-item reviewer wins.
#[tokio::test]
async fn ts09_ts10_ts12_ts17_design_review_and_revision() {
    let d = daemon().await;
    let it = Item::new(
        &d,
        &[
            ("system-analyst", ANALYST.into()),
            (
                "solutions-architect",
                architect(&["acme-billing", "ghost-service"]),
            ),
            // The reviewer agrees, but the unregistered project still blocks.
            ("solution-reviewer-1", reviewer("agreed", &[])),
            ("solution-reviewer-2", reviewer("agreed", &[])),
            ("solution-coordinator", COORD.into()),
        ],
        "reply",
    )
    .await;
    // TS-17: a per-item solution reviewer overrides the global one.
    let mut c = d.spa(
        "PUT",
        &format!("/api/inbox/{}/factory/agents", it.id),
        &it.op,
    );
    c.body = Some(
        serde_json::json!({"reviewer": {"solution": {"agent": "claude", "model": "claude-fable-5-2"}}}),
    );
    assert_eq!(d.send(c).await.0, 200);

    let (code, v) = it
        .post(&d, "solution/start-design", serde_json::json!({}))
        .await;
    assert_eq!(code, 202, "{v}");
    let v = it.settle(&d).await;
    assert_eq!(
        it.order()[..3],
        [
            "system-analyst",
            "solutions-architect",
            "solution-reviewer 1"
        ]
    );
    let docs = v["documents"].as_object().unwrap();
    assert!(docs.contains_key("requirements.md") && docs.contains_key("solution-design.md"));
    // TS-10: the unregistered project is a problem; the round asks for changes.
    assert_eq!(v["state"]["phase"], "revising");
    assert_eq!(
        (v["state"]["round"].as_u64(), v["roundOf"].as_u64()),
        (Some(1), Some(3))
    );
    let rounds = v["state"]["rounds"].as_array().unwrap();
    assert_eq!(rounds[0]["verdict"], "changes");
    assert_eq!(
        rounds[0]["problemIds"],
        serde_json::json!(["P-UNREGISTERED-GHOST-SERVICE"])
    );
    assert_eq!(
        rounds[0]["reviewer"]["model"], "claude-fable-5-2",
        "the item override ran"
    );
    // Starting again is refused.
    assert_eq!(
        it.post(&d, "solution/start-design", serde_json::json!({}))
            .await
            .0,
        409
    );

    // TS-12: the next review revises first, then reviews round 2.
    std::fs::write(
        it.dir.join("solutions-architect.json"),
        architect(&["acme-billing"]),
    )
    .unwrap();
    let (code, v) = it.post(&d, "solution/review", serde_json::json!({})).await;
    assert_eq!(code, 202, "{v}");
    let v = it.settle(&d).await;
    assert_eq!(
        it.order()[3..5],
        [
            "solutions-architect".to_string(),
            "solution-reviewer 2".to_string()
        ]
    );
    assert_eq!(v["state"]["phase"], "agreed");
    assert_eq!(v["state"]["round"], 2);
}

// TS-13 + TS-14 + TS-44 + TS-20(escalation): three rounds of changes escalate
// one request per problem; a 4th round is 409 round_limit whatever round the
// client names; only newCycle resets the count.
#[tokio::test]
async fn ts13_ts14_ts44_three_rounds_then_one_request_per_problem() {
    let d = daemon().await;
    let changes = reviewer("changes", &[("P-1", "IdP"), ("P-2", "Currency")]);
    let it = Item::new(
        &d,
        &[
            ("system-analyst", ANALYST.into()),
            ("solutions-architect", architect(&["acme-billing"])),
            ("solution-reviewer", changes),
            ("solution-coordinator", COORD.into()),
        ],
        "reply",
    )
    .await;
    it.post(&d, "solution/start-design", serde_json::json!({}))
        .await;
    it.settle(&d).await;
    for _ in 0..2 {
        let (code, v) = it
            .post(&d, "solution/review", serde_json::json!({"round": 1}))
            .await;
        assert_eq!(code, 202, "{v}");
        it.settle(&d).await;
    }
    let v = it.settle(&d).await;
    assert_eq!(v["state"]["phase"], "escalated");
    assert_eq!(v["state"]["round"], 3);
    // TS-14: exactly one request per problem, each a solution problem.
    let (_, reqs) = d
        .send(d.spa("GET", &format!("/api/inbox/{}/requests", it.id), TOKEN))
        .await;
    let reqs = reqs["requests"].as_array().unwrap();
    assert_eq!(reqs.len(), 2);
    assert!(
        reqs.iter()
            .all(|r| r["origin"] == "solution_problem" && r.get("worktree").is_none())
    );
    // TS-20 (escalation half): the coordinator reported, in transcript and comments.
    assert!(
        it.transcript(&d)
            .await
            .iter()
            .any(|e| e["kind"] == "report")
    );
    assert!(
        d.comments(&it.id)
            .iter()
            .any(|(_, b)| b.starts_with("**Coordinator report**"))
    );

    // TS-13 / TS-44: a 4th round is refused; the fold is unchanged.
    let before = d.state.solution.fold(&it.id).unwrap();
    for body in [
        serde_json::json!({}),
        serde_json::json!({"round": 1}),
        serde_json::json!({"round": 0, "cycle": 9}),
    ] {
        let (code, v) = it.post(&d, "solution/review", body).await;
        assert_eq!(
            (code, v["code"].as_str()),
            (409, Some("round_limit")),
            "{v}"
        );
    }
    assert_eq!(d.state.solution.fold(&it.id).unwrap(), before);
    let events = d.store.read_events(&it.id).unwrap();
    assert!(common::domain::solution::next_round(&events, ReviewLevel::Solution, &it.id).is_err());

    // Only the operator's newCycle resets the count.
    let mut c = d.spa(
        "POST",
        &format!("/api/inbox/{}/solution/review", it.id),
        TOKEN,
    );
    c.body = Some(serde_json::json!({"newCycle": true}));
    assert_eq!(d.send(c).await.1["code"], "operator_required");
    let (code, v) = it
        .post(&d, "solution/review", serde_json::json!({"newCycle": true}))
        .await;
    assert_eq!(code, 202, "{v}");
    let v = it.settle(&d).await;
    assert_eq!(v["state"]["cycle"], 2);
    assert_eq!(v["state"]["round"], 1, "round 1 of the new cycle ran");
}

// TS-15 + TS-16: one problem resolves through a confirmed coordinator
// proposal, another through a hand edit plus resolve; each only its own.
#[tokio::test]
async fn ts15_ts16_problems_resolve_one_at_a_time() {
    let d = daemon().await;
    let it = Item::new(
        &d,
        &[
            ("system-analyst", ANALYST.into()),
            ("solutions-architect", architect(&["acme-billing"])),
            (
                "solution-reviewer",
                reviewer(
                    "changes",
                    &[("P-1", "IdP"), ("P-2", "Currency"), ("P-3", "Tax")],
                ),
            ),
            ("solution-coordinator", COORD.into()),
        ],
        "reply",
    )
    .await;
    it.post(&d, "solution/start-design", serde_json::json!({}))
        .await;
    it.settle(&d).await;
    for _ in 0..2 {
        it.post(&d, "solution/review", serde_json::json!({})).await;
        it.settle(&d).await;
    }
    let state = d.state.solution.fold(&it.id).unwrap();
    assert_eq!(state.problems.len(), 3);
    let p1 = state
        .problems
        .iter()
        .find(|p| p.problem_id == "c1-P-1")
        .unwrap()
        .clone();
    let p2 = state
        .problems
        .iter()
        .find(|p| p.problem_id == "c1-P-2")
        .unwrap()
        .clone();

    // TS-15: the coordinator proposes a resolution for P-1; the operator confirms it.
    let proposal = r#"{"role":"solution-coordinator","schema_version":1,"reply":"Proposed.","problem_resolutions":[{"problem_id":"c1-P-1","text":"Use the existing OIDC provider."}]}"#;
    std::fs::write(it.dir.join("solution-coordinator.json"), proposal).unwrap();
    it.post(
        &d,
        "coordinator/turn",
        serde_json::json!({"message": "Resolve P-1"}),
    )
    .await;
    it.settle(&d).await;
    let req = d.state.inbox.request(&it.id, &p1.request_id).unwrap();
    assert_eq!(
        req.proposal.as_deref(),
        Some("Use the existing OIDC provider.")
    );
    let (code, v) = it
        .post(
            &d,
            &format!("requests/{}/confirm", p1.request_id),
            serde_json::json!({"contentHash": req.proposal_hash}),
        )
        .await;
    assert_eq!(code, 200, "{v}");
    let s = d.state.solution.fold(&it.id).unwrap();
    assert_eq!(s.open_problems(), 2, "only P-1 resolved");

    // TS-16: hand edit the design, then resolve P-2 by hand.
    let (_, doc) = d
        .send(d.spa(
            "GET",
            &format!("/api/inbox/{}/solution/docs/solution-design.md", it.id),
            TOKEN,
        ))
        .await;
    let mut c = d.spa(
        "PUT",
        &format!("/api/inbox/{}/solution/docs/solution-design.md", it.id),
        &it.op,
    );
    c.body = Some(
        serde_json::json!({"content": "# Design v2\nAll amounts in EUR.", "expectedHash": doc["hash"]}),
    );
    let (code, v) = d.send(c).await;
    assert_eq!(code, 200, "{v}");
    let new_hash = v["hash"].as_str().unwrap().to_string();
    let mut stale = d.spa(
        "PUT",
        &format!("/api/inbox/{}/solution/docs/solution-design.md", it.id),
        &it.op,
    );
    stale.body = Some(serde_json::json!({"content": "lost?", "expectedHash": doc["hash"]}));
    assert_eq!(d.send(stale).await.1["code"], "doc_conflict");
    let (code, v) = it
        .post(
            &d,
            &format!("solution/problems/{}/resolve", p2.problem_id),
            serde_json::json!({"note": "Edited: EUR only."}),
        )
        .await;
    assert_eq!(code, 200, "{v}");
    let s = d.state.solution.fold(&it.id).unwrap();
    assert_eq!(s.open_problems(), 1, "only P-2 more");
    let (_, now) = d
        .send(d.spa(
            "GET",
            &format!("/api/inbox/{}/solution/docs/solution-design.md", it.id),
            TOKEN,
        ))
        .await;
    assert_eq!(now["hash"], new_hash.as_str(), "the edit hash is current");
}

// TS-20: a passed review: report in transcript and comments, phase agreed,
// and convert is refused (409 approval_required).
#[tokio::test]
async fn ts20_an_agreed_review_reports_and_convert_is_refused() {
    let d = daemon().await;
    let it = Item::new(
        &d,
        &[
            ("system-analyst", ANALYST.into()),
            ("solutions-architect", architect(&["acme-billing"])),
            ("solution-reviewer", reviewer("agreed", &[])),
            ("solution-coordinator", COORD.into()),
        ],
        "reply",
    )
    .await;
    it.post(&d, "solution/start-design", serde_json::json!({}))
        .await;
    let v = it.settle(&d).await;
    assert_eq!(v["state"]["phase"], "agreed");
    let report = it
        .transcript(&d)
        .await
        .into_iter()
        .find(|e| e["kind"] == "report")
        .expect("report");
    assert_eq!(report["text"], "Agreed: billing builds invoices.");
    assert!(
        d.comments(&it.id)
            .iter()
            .any(|(_, b)| b.contains("Agreed: billing builds invoices."))
    );
    let mut c = d.spa("POST", &format!("/api/inbox/{}/convert", it.id), TOKEN);
    c.body = Some(
        serde_json::json!({"targets": [{"projectPath": "/code/acme-billing", "branch": "b", "prompt": "go"}]}),
    );
    let (code, v) = d.send(c).await;
    assert_eq!(
        (code, v["code"].as_str()),
        (409, Some("approval_required")),
        "{v}"
    );
}

// TS-18 (run half): an unindependent reviewer stops the start before any job.
#[tokio::test]
async fn ts18_an_unindependent_reviewer_stops_the_run() {
    let d = daemon().await;
    let it = Item::new(&d, &[("system-analyst", ANALYST.into())], "reply").await;
    // Write an override straight into the draft, as an older binary or a hand
    // edit could, bypassing the save-time check.
    d.state
        .inbox
        .set_factory_overrides(
            &it.id,
            common::domain::solution::FactoryOverrides {
                reviewer: common::domain::solution::ReviewerProfiles {
                    solution: Some(common::domain::solution::AgentProfile::new(
                        "claude",
                        Some("claude-opus-5-5"),
                    )),
                    project: None,
                },
                ..Default::default()
            },
        )
        .unwrap();
    let (code, v) = it
        .post(&d, "solution/start-design", serde_json::json!({}))
        .await;
    assert_eq!(
        (code, v["code"].as_str()),
        (422, Some("not_independent")),
        "{v}"
    );
    assert!(it.order().is_empty(), "no job ran");
    assert_eq!(
        d.state.solution.fold(&it.id).unwrap().phase,
        SolutionPhase::Requirements
    );
}

// TS-11 (pipeline half): a hung reviewer times out, records no round, and
// the operator can retry.
#[tokio::test]
async fn ts11_a_timed_out_review_consumes_no_round() {
    let d = daemon().await;
    let it = Item::new(
        &d,
        &[
            ("system-analyst", ANALYST.into()),
            ("solutions-architect", architect(&["acme-billing"])),
        ],
        // Every role hangs once a reply file is missing for it: use hang for all.
        "reply",
    )
    .await;
    // Make only the reviewer hang: its reply is a hang marker in a mode line.
    let notes = d.state.inbox.notes(&it.id).unwrap().0;
    let brief = notes.iter().find(|n| n.title == "Brief").unwrap();
    d.state
        .inbox
        .update_note(
            &it.id,
            &None,
            &brief.id,
            &brief.hash(),
            common::adapters::inbox_store::NotePatch {
                body: Some(brief.body.replace(
                    "STUB-MODE: reply",
                    "STUB-MODE: reply\nSTUB-HANG-ROLE: solution-reviewer",
                )),
                ..Default::default()
            },
        )
        .unwrap();
    it.post(&d, "solution/start-design", serde_json::json!({}))
        .await;
    let v = it.settle(&d).await;
    assert_eq!(v["job"]["errorCode"], "timeout", "{v}");
    assert_eq!(
        v["state"]["rounds"],
        serde_json::json!([]),
        "no round consumed"
    );
    assert_eq!(v["state"]["phase"], "revising", "the operator can retry");
    let events = d.store.read_events(&it.id).unwrap();
    assert_eq!(
        common::domain::solution::rounds_used(&events, ReviewLevel::Solution, &it.id),
        0
    );
}

// Everything that runs an agent or decides a problem is operator-only.
#[tokio::test]
async fn solution_actions_refuse_the_shared_token() {
    let d = daemon().await;
    let it = Item::new(&d, &[], "reply").await;
    for (method, path, body) in [
        ("POST", "solution/start-design", serde_json::json!({})),
        ("POST", "solution/review", serde_json::json!({})),
        (
            "PUT",
            "solution/docs/requirements.md",
            serde_json::json!({"content": "x"}),
        ),
        (
            "POST",
            "solution/problems/P-1/resolve",
            serde_json::json!({}),
        ),
        (
            "POST",
            "coordinator/proposals/T/0/apply",
            serde_json::json!({}),
        ),
    ] {
        let mut c = d.spa(method, &format!("/api/inbox/{}/{path}", it.id), TOKEN);
        c.body = Some(body);
        let (code, v) = d.send(c).await;
        assert_eq!(
            (code, v["code"].as_str()),
            (403, Some("operator_required")),
            "{path}"
        );
    }
    let (_, v) = d.send(d.spa("GET", "/api/factory/agents", TOKEN)).await;
    let grok = v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["agent"] == "grok")
        .unwrap()
        .clone();
    assert_eq!(grok["enforcement"], "unverified");
    assert_eq!(grok["canReview"], false);
}
