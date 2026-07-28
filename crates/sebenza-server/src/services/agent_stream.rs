use crate::adapters::claude_cli::parse_claude_stream_line;
use crate::util::id::random_uuid;
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StreamProvider {
    Claude,
    Codex,
}

impl StreamProvider {
    /// The streaming provider for a built-in agent, or `None` when that agent has no
    /// in-app chat implementation. Exhaustive on `BuiltinAgentId`, so a new built-in
    /// must decide here rather than silently inheriting Claude's provider.
    pub fn for_builtin(
        id: common::services::agent_registry::BuiltinAgentId,
    ) -> Option<StreamProvider> {
        use common::services::agent_registry::BuiltinAgentId;
        match id {
            BuiltinAgentId::Claude => Some(StreamProvider::Claude),
            BuiltinAgentId::Codex => Some(StreamProvider::Codex),
            // opencode has no streaming provider yet: in-app chat depends on the
            // generated plugin and the export-based history adapter, later in this phase.
            // Its capabilities declare in_app_chat: false, so nothing offers chat for it.
            BuiltinAgentId::Opencode => None,
        }
    }
}

/// A conversation message without its `order` (assigned per WS subscriber).
#[derive(Clone)]
pub struct DraftMessage {
    pub id: String,
    pub turn_id: String,
    pub role: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub created_at: Option<String>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

/// Live event broadcast to subscribers (order + revision are stamped per-subscriber).
#[derive(Clone)]
pub enum StreamEvent {
    Status { running: bool, active_turn_id: Option<String> },
    Delta { turn_id: String, item_id: String, delta: String },
    Upsert { message: DraftMessage, order_key: String },
    Error { message: String },
}

struct RunState {
    turn_id: String,
    completed: AtomicBool,
    tx: broadcast::Sender<StreamEvent>,
    live: Mutex<IndexMap<String, DraftMessage>>,
    interrupt: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

pub struct StartRunInput {
    pub provider: StreamProvider,
    pub conversation_id: String,
    pub cwd: String,
    pub prompt: String,
    pub env: HashMap<String, String>,
    pub permission_mode: Option<String>,
    pub resume_session_id: Option<String>,
    pub system_prompt: Option<String>,
}

/// What a subscriber receives on connect: a replay of the active run's current
/// state, then a live receiver for subsequent events.
pub struct Subscription {
    pub replay: Vec<StreamEvent>,
    pub receiver: broadcast::Receiver<StreamEvent>,
}

#[derive(Default)]
pub struct AgentStreamManager {
    runs: Mutex<HashMap<String, Arc<RunState>>>,
}

impl AgentStreamManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_active_run(&self, conversation_id: &str) -> bool {
        self.runs
            .lock()
            .unwrap()
            .get(conversation_id)
            .map(|r| !r.completed.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Start a Claude streaming turn. Returns the new turn id, or an error if a
    /// turn is already running for this conversation.
    pub fn start_run(&self, input: StartRunInput) -> Result<String, String> {
        if self.has_active_run(&input.conversation_id) {
            return Err("Claude is already responding in this conversation".to_string());
        }
        let turn_id = format!("claude-turn:{}", random_uuid());
        let (tx, _rx) = broadcast::channel::<StreamEvent>(1024);
        let run = Arc::new(RunState {
            turn_id: turn_id.clone(),
            completed: AtomicBool::new(false),
            tx,
            live: Mutex::new(IndexMap::new()),
            interrupt: Mutex::new(None),
        });
        self.runs
            .lock()
            .unwrap()
            .insert(input.conversation_id.clone(), run.clone());

        // Optimistic user message + running status, before the process starts.
        let user_msg = DraftMessage {
            id: format!("claude-user:{turn_id}"),
            turn_id: turn_id.clone(),
            role: "user".to_string(),
            kind: "text".to_string(),
            text: input.prompt.clone(),
            status: "completed".to_string(),
            created_at: None,
            tool_name: None,
            tool_call_id: None,
        };
        emit_status(&run, true);
        emit_upsert(&run, user_msg.clone(), user_msg.id.clone());

        // The completed run stays in the map (marked completed) so a subscriber
        // that connects mid/just-after the turn can replay it; the next turn for
        // the same conversation replaces it, keeping the map bounded.
        tokio::spawn(async move {
            match input.provider {
                StreamProvider::Claude => run_claude(input, run.clone()).await,
                StreamProvider::Codex => run_codex(input, run.clone()).await,
            }
            finish_run(&run, "completed");
        });

        Ok(turn_id)
    }

    /// Interrupt the active run, returning its turn id.
    pub fn interrupt(&self, conversation_id: &str) -> Option<String> {
        let run = self.runs.lock().unwrap().get(conversation_id).cloned()?;
        if run.completed.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(tx) = run.interrupt.lock().unwrap().take() {
            let _ = tx.send(());
        }
        finish_run(&run, "completed");
        Some(run.turn_id.clone())
    }

    /// Subscribe to a conversation's live stream: replay of current state + a
    /// receiver for future events. `None` if there is no run for the conversation.
    pub fn subscribe(&self, conversation_id: &str) -> Option<Subscription> {
        let run = self.runs.lock().unwrap().get(conversation_id).cloned()?;
        // Subscribe first so no event is missed between snapshot and receive.
        let receiver = run.tx.subscribe();
        let running = !run.completed.load(Ordering::Relaxed);
        let mut replay = Vec::new();
        if running {
            replay.push(StreamEvent::Status {
                running: true,
                active_turn_id: Some(run.turn_id.clone()),
            });
        }
        for (id, message) in run.live.lock().unwrap().iter() {
            replay.push(StreamEvent::Upsert {
                message: message.clone(),
                order_key: id.clone(),
            });
        }
        if !running {
            replay.push(StreamEvent::Status {
                running: false,
                active_turn_id: None,
            });
        }
        Some(Subscription { replay, receiver })
    }
}

fn emit_status(run: &RunState, running: bool) {
    let _ = run.tx.send(StreamEvent::Status {
        running,
        active_turn_id: running.then(|| run.turn_id.clone()),
    });
}

fn emit_upsert(run: &RunState, message: DraftMessage, order_key: String) {
    run.live.lock().unwrap().insert(message.id.clone(), message.clone());
    let _ = run.tx.send(StreamEvent::Upsert { message, order_key });
}

fn finish_run(run: &RunState, status: &str) {
    if run.completed.swap(true, Ordering::Relaxed) {
        return;
    }
    // Finalize any still-in-progress live messages.
    let finalized: Vec<DraftMessage> = {
        let mut live = run.live.lock().unwrap();
        let mut out = Vec::new();
        for message in live.values_mut() {
            if message.status == "inProgress" {
                message.status = status.to_string();
                out.push(message.clone());
            }
        }
        out
    };
    for message in finalized {
        let key = message.id.clone();
        let _ = run.tx.send(StreamEvent::Upsert { message, order_key: key });
    }
    emit_status(run, false);
}

/// Spawn `claude` and pump its stream-json output into the run's broadcast.
async fn run_claude(input: StartRunInput, run: Arc<RunState>) {
    let mut args: Vec<String> = vec![
        "-p".into(),
        "--verbose".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--include-partial-messages".into(),
    ];
    if let Some(resume) = &input.resume_session_id {
        args.push("-r".into());
        args.push(resume.clone());
    }
    if let Some(mode) = &input.permission_mode {
        args.push("--permission-mode".into());
        args.push(mode.clone());
    }
    if let Some(sys) = &input.system_prompt {
        args.push("--append-system-prompt".into());
        args.push(sys.clone());
    }

    let mut command = tokio::process::Command::new("claude");
    command
        .args(&args)
        .current_dir(&input.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &input.env {
        command.env(k, v);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = run.tx.send(StreamEvent::Error { message: format!("failed to spawn claude: {e}") });
            return;
        }
    };

    // Feed the prompt on stdin, then close it.
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = if input.prompt.ends_with('\n') {
            input.prompt.clone()
        } else {
            format!("{}\n", input.prompt)
        };
        let _ = stdin.write_all(prompt.as_bytes()).await;
        drop(stdin);
    }

    let stdout = child.stdout.take();
    let (int_tx, mut int_rx) = tokio::sync::oneshot::channel::<()>();
    *run.interrupt.lock().unwrap() = Some(int_tx);

    let mut message_id: Option<String> = None;
    let mut block_index: i64 = 0;

    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            tokio::select! {
                _ = &mut int_rx => {
                    let _ = child.start_kill();
                    break;
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            let line = line.trim();
                            if line.is_empty() { continue; }
                            handle_stream_line(line, &run, &mut message_id, &mut block_index);
                        }
                        _ => break,
                    }
                }
            }
        }
    }
    let _ = child.wait().await;
}

/// Apply one parsed stream line to the run (mirrors `handleStreamLine` +
/// the stream service's notify* methods).
fn handle_stream_line(
    line: &str,
    run: &RunState,
    message_id: &mut Option<String>,
    block_index: &mut i64,
) {
    let Some(parsed) = parse_claude_stream_line(line) else {
        return;
    };

    if let Some(mid) = parsed.message_start {
        *message_id = Some(mid);
    }
    if let Some(index) = parsed.block_start {
        *block_index = index;
    }

    if let Some((delta, delta_block)) = parsed.assistant_delta {
        let item_id = format!("{}:{delta_block}", message_id.as_deref().unwrap_or("msg"));
        // Accumulate into the live draft so a late subscriber sees full text.
        {
            let mut live = run.live.lock().unwrap();
            let entry = live.entry(item_id.clone()).or_insert_with(|| DraftMessage {
                id: item_id.clone(),
                turn_id: run.turn_id.clone(),
                role: "assistant".to_string(),
                kind: "text".to_string(),
                text: String::new(),
                status: "inProgress".to_string(),
                created_at: None,
                tool_name: None,
                tool_call_id: None,
            });
            entry.text.push_str(&delta);
        }
        let _ = run.tx.send(StreamEvent::Delta {
            turn_id: run.turn_id.clone(),
            item_id,
            delta,
        });
    }

    for block in parsed.blocks {
        let id = if block.kind == "toolResult" {
            format!("tool_result:{}", block.tool_call_id.clone().unwrap_or_default())
        } else {
            format!(
                "{}:{}",
                block.message_id.clone().or_else(|| message_id.clone()).unwrap_or_else(|| "msg".to_string()),
                block_index
            )
        };
        let message = DraftMessage {
            id: id.clone(),
            turn_id: run.turn_id.clone(),
            role: block.role,
            kind: block.kind,
            text: block.text,
            status: "inProgress".to_string(),
            created_at: block.created_at,
            tool_name: block.tool_name,
            tool_call_id: block.tool_call_id,
        };
        emit_upsert(run, message, id);
    }

    if parsed.complete_session_id.is_some() {
        finish_run(run, "completed");
    }
    if let Some(err) = parsed.error {
        let _ = run.tx.send(StreamEvent::Error { message: err });
        finish_run(run, "failed");
    }
}

/// Spawn `codex exec [resume <id>] --json` and pump its item events into the
/// run's broadcast. Codex emits completed items (no token deltas), so each item
/// becomes a finalized upsert.
async fn run_codex(input: StartRunInput, run: Arc<RunState>) {
    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(session) = &input.resume_session_id {
        args.push("resume".into());
        args.push(session.clone());
    }
    args.push("--json".into());
    args.push("--skip-git-repo-check".into());
    args.push(input.prompt.clone());

    let mut command = tokio::process::Command::new("codex");
    command
        .args(&args)
        .current_dir(&input.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &input.env {
        command.env(k, v);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = run.tx.send(StreamEvent::Error { message: format!("failed to spawn codex: {e}") });
            return;
        }
    };
    let stdout = child.stdout.take();
    let (int_tx, mut int_rx) = tokio::sync::oneshot::channel::<()>();
    *run.interrupt.lock().unwrap() = Some(int_tx);

    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            tokio::select! {
                _ = &mut int_rx => { let _ = child.start_kill(); break; }
                line = lines.next_line() => match line {
                    Ok(Some(line)) => {
                        let line = line.trim();
                        if !line.is_empty() {
                            handle_codex_line(line, &run);
                        }
                    }
                    _ => break,
                },
            }
        }
    }
    let _ = child.wait().await;
}

/// Apply one `codex exec --json` event line to the run.
fn handle_codex_line(line: &str, run: &RunState) {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return;
    };
    match event.get("type").and_then(Value::as_str) {
        Some("item.completed") | Some("item.started") => {
            let Some(item) = event.get("item") else {
                return;
            };
            let id = item.get("id").and_then(Value::as_str).unwrap_or("item").to_string();
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            let (role, kind) = match item_type {
                "agent_message" => ("assistant", "text"),
                "reasoning" => ("assistant", "thinking"),
                "user_message" => ("user", "text"),
                _ => ("assistant", "toolUse"),
            };
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| item.get("command").and_then(Value::as_str).unwrap_or("").to_string());
            let status = if event.get("type").and_then(Value::as_str) == Some("item.started") {
                "inProgress"
            } else {
                "completed"
            };
            let message = DraftMessage {
                id: id.clone(),
                turn_id: run.turn_id.clone(),
                role: role.to_string(),
                kind: kind.to_string(),
                text,
                status: status.to_string(),
                created_at: None,
                tool_name: (kind == "toolUse").then(|| item_type.to_string()),
                tool_call_id: None,
            };
            emit_upsert(run, message, id);
        }
        Some("error") => {
            let msg = event.get("message").and_then(Value::as_str).unwrap_or("codex error").to_string();
            let _ = run.tx.send(StreamEvent::Error { message: msg });
        }
        _ => {}
    }
}

#[cfg(test)]
mod stream_provider_tests {
    use super::*;
    use common::services::agent_registry::BuiltinAgentId;

    #[test]
    fn every_builtin_maps_to_a_stream_provider_or_explicitly_to_none() {
        // Exhaustive by construction: if a new BuiltinAgentId variant is added,
        // `for_builtin` fails to compile until it decides. This asserts every current
        // variant has been decided, so none silently inherits Claude's provider.
        for id in BuiltinAgentId::ALL {
            let provider = StreamProvider::for_builtin(*id);
            match id {
                BuiltinAgentId::Claude => assert!(matches!(provider, Some(StreamProvider::Claude))),
                BuiltinAgentId::Codex => assert!(matches!(provider, Some(StreamProvider::Codex))),
                // Explicitly no provider yet — chat is disabled for opencode via its
                // capabilities until the plugin and export adapter land.
                BuiltinAgentId::Opencode => assert!(provider.is_none()),
            }
        }
    }
}
