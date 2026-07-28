//! `sebenza-cli oneshot` — run an agent worktree start-to-finish, streaming the
//! conversation to stdout. Creates/opens the worktree over HTTP, then consumes
//! the agent conversation WebSocket while polling project state for the
//! terminal conditions (PR opened, session closed, agent idle, user took over).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

use crate::http::{AgentsUiMessage, Http, WorktreeSnapshot};

const READY_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RECONNECTS: u32 = 30;
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);
const IDLE_GRACE: Duration = Duration::from_secs(15);

fn ts() -> String {
    format!("[{}]", chrono::Local::now().format("%H:%M:%S"))
}

fn out(level: &str, msg: &str) {
    println!("{} [{level}] {msg}", ts());
}

fn err(level: &str, msg: &str) {
    eprintln!("{} [{level}] {msg}", ts());
}

// ── Argument parsing ──────────────────────────────────────────────────────

struct ParsedOneshot {
    branch: Option<String>,
    prompt: Option<String>,
    resume: bool,
    body: serde_json::Map<String, Value>,
    keep_open: bool,
}

fn usage() -> &'static str {
    "Usage:\n  sebenza-cli oneshot [branch] --prompt <text> [--agent <id>] [--base <branch>] [--profile <name>]\n                          [--env KEY=VALUE]... [--keep-open]\n  sebenza-cli oneshot --resume <branch> --prompt <text>\n\nRuns an agent worktree start-to-finish, streaming the conversation to stdout.\nDoes not change the focused tmux session. The server-side oneshot watcher\ncloses the worktree session once the agent finishes — even if this CLI is\nkilled mid-run.\nOpening the worktree in the browser and interacting with it disarms the watcher.\n\nExit codes: 0 if the agent opened a PR / the user took over via the browser;\n1 if the agent went idle without opening a PR; 130 on Ctrl-C (worktree keeps\nrunning, resume with `sebenza-cli oneshot --resume <branch>`).\n\nOptions:\n  --resume <branch>        Resume an existing local worktree instead of creating one\n  --prompt <text>          Initial agent prompt (required; follow-up nudge when --resume)\n  --agent <id>             Agent id to launch\n  --base <branch>          Base branch for a new worktree (defaults to config)\n  --profile <name>         Worktree profile from .ai/sebenza.yaml\n  --env KEY=VALUE          Runtime env override (repeatable)\n  --keep-open              Don't auto-close the worktree session when the agent finishes\n  --help                   Show this help message"
}

fn read_value(args: &[String], index: usize, flag: &str) -> Result<(String, usize)> {
    let arg = &args[index];
    let prefix = format!("{flag}=");
    if let Some(rest) = arg.strip_prefix(&prefix) {
        return Ok((rest.to_string(), index));
    }
    let v = args
        .get(index + 1)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    Ok((v.clone(), index + 1))
}

fn parse(args: &[String]) -> Result<Option<ParsedOneshot>> {
    let mut branch: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut resume = false;
    let mut resume_branch: Option<String> = None;
    let mut keep_open = false;
    let mut body = serde_json::Map::new();
    let mut env = serde_json::Map::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            return Ok(None);
        } else if arg == "--resume" || arg.starts_with("--resume=") {
            let (v, n) = read_value(args, index, "--resume")?;
            resume = true;
            resume_branch = Some(v.trim().to_string());
            index = n;
        } else if arg == "--prompt" || arg.starts_with("--prompt=") {
            let (v, n) = read_value(args, index, "--prompt")?;
            prompt = Some(v);
            index = n;
        } else if arg == "--agent" || arg.starts_with("--agent=") {
            let (v, n) = read_value(args, index, "--agent")?;
            body.insert("agent".into(), json!(v.trim()));
            index = n;
        } else if arg == "--base" || arg.starts_with("--base=") {
            let (v, n) = read_value(args, index, "--base")?;
            body.insert("baseBranch".into(), json!(v));
            index = n;
        } else if arg == "--profile" || arg.starts_with("--profile=") {
            let (v, n) = read_value(args, index, "--profile")?;
            body.insert("profile".into(), json!(v));
            index = n;
        } else if arg == "--env" || arg.starts_with("--env=") {
            let (v, n) = read_value(args, index, "--env")?;
            let sep = v.find('=').unwrap_or(0);
            if sep == 0 {
                return Err(anyhow!("--env must use KEY=VALUE"));
            }
            env.insert(v[..sep].to_string(), json!(v[sep + 1..].to_string()));
            index = n;
        } else if arg == "--keep-open" {
            keep_open = true;
        } else if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        } else if branch.is_none() {
            branch = Some(arg.clone());
        } else {
            return Err(anyhow!("Unexpected argument: {arg}"));
        }
        index += 1;
    }

    if resume {
        let rb = resume_branch.ok_or_else(|| anyhow!("--resume requires a branch name"))?;
        if let Some(b) = &branch {
            if b != &rb {
                return Err(anyhow!("Cannot pass both a positional branch and --resume"));
            }
        }
        if prompt.is_none() {
            return Err(anyhow!(
                "--resume requires --prompt; use the dashboard to re-attach without re-prompting"
            ));
        }
        branch = Some(rb);
    } else if prompt.is_none() {
        return Err(anyhow!("oneshot requires --prompt"));
    }

    if let Some(b) = &branch {
        body.insert("branch".into(), json!(b));
    }
    if let Some(p) = &prompt {
        body.insert("prompt".into(), json!(p));
    }
    if !env.is_empty() {
        body.insert("envOverrides".into(), Value::Object(env));
    }

    Ok(Some(ParsedOneshot {
        branch,
        prompt,
        resume,
        body,
        keep_open,
    }))
}

// ── Shared streaming/poll state ─────────────────────────────────────────────

#[derive(Default)]
struct PrintState {
    printed_ids: HashSet<String>,
    streaming_item_id: Option<String>,
    needs_header: bool,
    last_stream_revision: i64,
}

#[derive(Default)]
struct PollState {
    seen_pr_urls: HashSet<String>,
    seen_merged_urls: HashSet<String>,
    had_open_session: bool,
    consecutive_closed: u32,
    idle_since: Option<Instant>,
    watcher_was_armed: bool,
}

struct Shared {
    http: Arc<Http>,
    base: String,
    branch: String,
    print: Mutex<PrintState>,
    poll: Mutex<PollState>,
    done: AtomicBool,
    exit_tx: Mutex<Option<oneshot::Sender<i32>>>,
}

impl Shared {
    fn finalize(&self, code: i32) {
        if !self.done.swap(true, Ordering::SeqCst) {
            self.flush_line();
            if let Some(tx) = self.exit_tx.lock().unwrap().take() {
                let _ = tx.send(code);
            }
        }
    }

    fn flush_line(&self) {
        let mut p = self.print.lock().unwrap();
        flush(&mut p);
    }
}

fn flush(p: &mut PrintState) {
    if let Some(id) = p.streaming_item_id.take() {
        println!();
        p.printed_ids.insert(id);
    }
    p.needs_header = false;
}

fn tool_summary(msg: &AgentsUiMessage) -> String {
    msg.command.clone().unwrap_or_default()
}

fn result_summary(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return "(empty)".to_string();
    }
    let mut first = lines[0].to_string();
    if first.len() > 200 {
        first.truncate(200);
        first.push('…');
    }
    if lines.len() > 1 {
        first.push_str(&format!(" (+{} lines)", lines.len() - 1));
    }
    first
}

fn print_new_messages(p: &mut PrintState, msgs: &[AgentsUiMessage]) {
    for msg in msgs {
        // If this item is currently streaming, close its line first (which
        // marks it printed) so we don't double-render it.
        if p.streaming_item_id.as_deref() == Some(msg.id.as_str()) {
            flush(p);
        }
        if p.printed_ids.contains(&msg.id) {
            continue;
        }
        match msg.kind.as_str() {
            "toolUse" => {
                let name = msg.tool_name.clone().unwrap_or_default();
                println!("{} ● {name}({})", ts(), tool_summary(msg));
            }
            "toolResult" => {
                let mut summary = result_summary(&msg.text);
                if let Some(code) = msg.exit_code {
                    if code != 0 {
                        summary.push_str(&format!(" (exit {code})"));
                    }
                }
                println!("{}   ⎿ {}", ts(), summary);
            }
            _ => {
                if msg.text.trim().is_empty() {
                    continue;
                }
                println!("{} [{}] {}", ts(), msg.role, msg.text);
            }
        }
        p.printed_ids.insert(msg.id.clone());
    }
}

// ── WebSocket event handling ────────────────────────────────────────────────

fn handle_ws_event(shared: &Shared, ev: &Value) {
    use std::io::Write;
    let kind = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let revision = ev.get("revision").and_then(|v| v.as_i64()).unwrap_or(0);
    let mut p = shared.print.lock().unwrap();

    match kind {
        "messageDelta" => {
            if revision <= p.last_stream_revision {
                return;
            }
            p.last_stream_revision = revision;
            let item_id = ev
                .get("itemId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let delta = ev.get("delta").and_then(|v| v.as_str()).unwrap_or("");
            if p.streaming_item_id.as_deref() != Some(item_id.as_str()) {
                flush(&mut p);
                p.streaming_item_id = Some(item_id);
                p.needs_header = true;
            }
            if p.needs_header {
                print!("{} [assistant] ", ts());
                p.needs_header = false;
            }
            print!("{delta}");
            let _ = std::io::stdout().flush();
        }
        "messageUpsert" => {
            if revision <= p.last_stream_revision {
                return;
            }
            p.last_stream_revision = revision;
            if let Some(m) = ev.get("message") {
                if let Ok(msg) = serde_json::from_value::<AgentsUiMessage>(m.clone()) {
                    print_new_messages(&mut p, &[msg]);
                }
            }
        }
        "conversationStatus" => {
            if revision <= p.last_stream_revision {
                return;
            }
            p.last_stream_revision = revision;
            let running = ev.get("running").and_then(|v| v.as_bool()).unwrap_or(true);
            if !running {
                flush(&mut p);
            }
        }
        "error" => {
            flush(&mut p);
            let m = ev
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("stream error");
            drop(p);
            err("error", m);
        }
        _ => {}
    }
}

async fn stream_conversation(shared: Arc<Shared>, port: u16, prefix_path: String) {
    let url = format!(
        "ws://localhost:{port}{prefix_path}/ws/agents/worktrees/{}",
        shared.branch
    );
    let mut failures: u32 = 0;

    loop {
        if shared.done.load(Ordering::SeqCst) {
            return;
        }
        match tokio_tungstenite::connect_async(&url).await {
            Ok((mut ws, _)) => {
                failures = 0;
                {
                    let mut p = shared.print.lock().unwrap();
                    p.last_stream_revision = 0;
                }
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                            Ok(ev) => handle_ws_event(&shared, &ev),
                            Err(_) => err("error", "received malformed conversation stream data"),
                        },
                        Ok(Message::Close(_)) | Err(_) => break,
                        _ => {}
                    }
                }
            }
            Err(_) => {}
        }

        if shared.done.load(Ordering::SeqCst) {
            return;
        }
        failures += 1;
        if failures == 3 || failures == 15 {
            err(
                "warn",
                &format!("Sebenza server unreachable, retrying ({failures}/{MAX_RECONNECTS})"),
            );
        }
        if failures >= MAX_RECONNECTS {
            err(
                "fatal",
                &format!("Sebenza server unreachable after {failures} reconnect attempts"),
            );
            shared.finalize(1);
            return;
        }
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

// ── Project-state polling ────────────────────────────────────────────────────

fn record_pr_events(shared: &Shared, wt: &WorktreeSnapshot) {
    let mut poll = shared.poll.lock().unwrap();
    for pr in &wt.prs {
        if poll.seen_pr_urls.insert(pr.url.clone()) {
            drop(poll);
            shared.flush_line();
            out("event", &format!("PR #{} opened: {}", pr.number, pr.url));
            poll = shared.poll.lock().unwrap();
        }
        if pr.state == "merged" && poll.seen_merged_urls.insert(pr.url.clone()) {
            drop(poll);
            shared.flush_line();
            out("event", &format!("PR #{} merged: {}", pr.number, pr.url));
            poll = shared.poll.lock().unwrap();
        }
    }
}

async fn poll_project_state(shared: Arc<Shared>) {
    loop {
        if shared.done.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        if shared.done.load(Ordering::SeqCst) {
            return;
        }

        let worktrees = match shared.http.fetch_worktrees(&shared.base).await {
            Ok(w) => w,
            Err(_) => continue,
        };
        let wt = worktrees.into_iter().find(|w| w.branch == shared.branch);

        let Some(wt) = wt else {
            let removed = { shared.poll.lock().unwrap().had_open_session };
            if removed {
                shared.flush_line();
                out("event", "worktree removed — exiting");
                shared.finalize(0);
            }
            continue;
        };

        {
            let mut poll = shared.poll.lock().unwrap();
            if wt.mux {
                poll.had_open_session = true;
                poll.consecutive_closed = 0;
            }
        }

        record_pr_events(&shared, &wt);

        // Watcher arm/disarm — the user opening the worktree disarms it.
        {
            let mut poll = shared.poll.lock().unwrap();
            if wt.oneshot.is_some() {
                poll.watcher_was_armed = true;
            } else if poll.watcher_was_armed && wt.mux {
                drop(poll);
                shared.flush_line();
                out("event", "user took over from the browser — exiting");
                shared.finalize(0);
                return;
            }
        }

        // Session-closed detection (two consecutive closed readings).
        {
            let mut poll = shared.poll.lock().unwrap();
            if poll.had_open_session && !wt.mux {
                poll.consecutive_closed += 1;
                if poll.consecutive_closed >= 2 {
                    drop(poll);
                    shared.flush_line();
                    out("event", "session closed — exiting");
                    shared.finalize(0);
                    return;
                }
            }
        }

        // Idle / terminal detection with a grace window.
        let is_terminal = wt.status == "stopped" || wt.status == "error";
        let is_idle = wt.status == "idle";
        if is_terminal || is_idle {
            let stable = {
                let mut poll = shared.poll.lock().unwrap();
                if poll.idle_since.is_none() {
                    poll.idle_since = Some(Instant::now());
                }
                is_terminal
                    || poll
                        .idle_since
                        .map(|t| t.elapsed() >= IDLE_GRACE)
                        .unwrap_or(false)
            };
            if stable {
                if let Err(e) = shared.http.sync_prs(&shared.base, &shared.branch).await {
                    err("warn", &format!("failed to sync PRs from server: {e}"));
                }
                if let Ok(worktrees) = shared.http.fetch_worktrees(&shared.base).await {
                    if let Some(wt2) = worktrees.iter().find(|w| w.branch == shared.branch) {
                        record_pr_events(&shared, wt2);
                    }
                }
                let has_pr = { !shared.poll.lock().unwrap().seen_pr_urls.is_empty() };
                shared.flush_line();
                if has_pr {
                    out(
                        "event",
                        &format!("agent {} after opening PR — exiting", wt.status),
                    );
                    shared.finalize(0);
                } else {
                    err(
                        "error",
                        &format!("agent {} without opening a PR", wt.status),
                    );
                    shared.finalize(1);
                }
                return;
            }
        } else {
            shared.poll.lock().unwrap().idle_since = None;
        }
    }
}

async fn poll_history(shared: Arc<Shared>) {
    loop {
        if shared.done.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        if shared.done.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(msgs) = shared.http.history(&shared.base, &shared.branch).await {
            let mut p = shared.print.lock().unwrap();
            print_new_messages(&mut p, &msgs);
        }
    }
}

// ── Entry ─────────────────────────────────────────────────────────────────

pub async fn run(args: &[String], port: u16, project_dir: &str) -> i32 {
    let parsed = match parse(args) {
        Ok(Some(p)) => p,
        Ok(None) => {
            println!("{}", usage());
            return 0;
        }
        Err(e) => {
            eprintln!("{e}");
            eprintln!("{}", usage());
            return 1;
        }
    };

    let http = Arc::new(Http::new(port));
    let base = match http.resolve_project_base(project_dir).await {
        Ok(b) => b,
        Err(e) => {
            err("error", &e.to_string());
            return 1;
        }
    };
    let prefix_path = base
        .strip_prefix(&format!("http://localhost:{port}"))
        .unwrap_or("")
        .to_string();

    let oneshot_cfg = json!({ "autoCloseOnDone": !parsed.keep_open });

    // Create or open the worktree.
    let branch = match prepare_worktree(&http, &base, &parsed, &oneshot_cfg).await {
        Ok(b) => b,
        Err(e) => {
            err("error", &e.to_string());
            return 1;
        }
    };

    // Wait for the session to be ready.
    let agent_name = match ensure_ready(&http, &base, &branch).await {
        Some(a) => a,
        None => return 1,
    };

    let (exit_tx, exit_rx) = oneshot::channel::<i32>();
    let shared = Arc::new(Shared {
        http: http.clone(),
        base: base.clone(),
        branch: branch.clone(),
        print: Mutex::new(PrintState::default()),
        poll: Mutex::new(PollState::default()),
        done: AtomicBool::new(false),
        exit_tx: Mutex::new(Some(exit_tx)),
    });

    // Initial history.
    if let Ok(msgs) = http.history(&base, &branch).await {
        let mut p = shared.print.lock().unwrap();
        print_new_messages(&mut p, &msgs);
    }

    tokio::spawn(stream_conversation(shared.clone(), port, prefix_path));
    tokio::spawn(poll_project_state(shared.clone()));
    // Claude-only history polling. This is NOT a declared capability: it compensates for
    // gaps in Claude's live stream, and whether another agent needs it can only be
    // established by watching that agent's stream in practice. The CLI also has no
    // capabilities endpoint today (only /api/agents/.../history), so reading a flag here
    // would mean new API plumbing. Revisit when opencode streaming lands: if it needs the
    // same compensation, this becomes a capability; if not, it stays agent-specific.
    if agent_name.as_deref() == Some("claude") {
        tokio::spawn(poll_history(shared.clone()));
    }

    // Signal handling.
    {
        let shared = shared.clone();
        let branch = branch.clone();
        tokio::spawn(async move {
            let sigterm = async {
                #[cfg(unix)]
                {
                    let mut s =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                            .expect("SIGTERM handler");
                    s.recv().await;
                }
                #[cfg(not(unix))]
                std::future::pending::<()>().await;
            };
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm => {}
            }
            println!(
                "\n{} [event] interrupted — worktree {branch} keeps running",
                ts()
            );
            println!(
                "{} [event] resume with: sebenza-cli oneshot --resume {branch}",
                ts()
            );
            shared.finalize(130);
        });
    }

    exit_rx.await.unwrap_or(1)
}

async fn prepare_worktree(
    http: &Http,
    base: &str,
    parsed: &ParsedOneshot,
    oneshot_cfg: &Value,
) -> Result<String> {
    // Does the worktree already exist?
    let existing = if let Some(b) = &parsed.branch {
        http.fetch_worktrees(base)
            .await?
            .into_iter()
            .any(|w| &w.branch == b)
    } else {
        false
    };

    if parsed.resume || existing {
        let branch = parsed
            .branch
            .clone()
            .ok_or_else(|| anyhow!("could not resolve branch"))?;
        if parsed.resume {
            out("event", "resuming");
        } else {
            out("event", &format!("worktree exists, resuming {branch}"));
        }
        let mut body = json!({ "oneshot": oneshot_cfg });
        if let Some(p) = &parsed.prompt {
            body["prompt"] = json!(p);
        }
        http.open_worktree_body(base, &branch, body).await?;
        if parsed.prompt.is_some() {
            out("event", "sent prompt");
        }
        Ok(branch)
    } else {
        match &parsed.branch {
            Some(b) => out("event", &format!("creating worktree {b}...")),
            None => out("event", "creating worktree..."),
        }
        let mut body = parsed.body.clone();
        body.insert("source".into(), json!("oneshot"));
        body.insert("oneshot".into(), oneshot_cfg.clone());
        let branch = http
            .create_worktree_primary(base, Value::Object(body))
            .await?;
        out("event", &format!("created {branch}"));
        Ok(branch)
    }
}

/// Poll until the worktree session is running. Returns its agent name, or None
/// on timeout (after printing an error).
async fn ensure_ready(http: &Http, base: &str, branch: &str) -> Option<Option<String>> {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(worktrees) = http.fetch_worktrees(base).await {
            if let Some(wt) = worktrees.into_iter().find(|w| w.branch == branch) {
                if wt.mux && wt.status != "creating" && wt.status != "closed" {
                    return Some(wt.agent_name);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    err(
        "error",
        &format!("timed out waiting for {branch} session to start"),
    );
    None
}
