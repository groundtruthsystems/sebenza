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
    Grok,
    Codex,
}

impl StreamProvider {
    /// Namespace for this provider's turn and message ids. Claude and Codex share
    /// `claude` because they always have: the value is a diagnostic label, not something
    /// the frontend parses, and changing Codex's would alter existing behaviour for no
    /// gain. A third provider emitting `claude-turn:` ids is the cross-provider confusion
    /// `conversation_router`'s module docs were written about, so grok gets its own.
    pub fn id_prefix(self) -> &'static str {
        match self {
            StreamProvider::Claude | StreamProvider::Codex => "claude",
            StreamProvider::Grok => "grok",
        }
    }

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
            // grok's `--output-format streaming-messages-json` IS the Messages
            // `stream-json` wire format, verified against grok 1.0.5 down to the
            // snake_case `session_id` field, so it shares Claude's line parser.
            BuiltinAgentId::Grok => Some(StreamProvider::Grok),
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
    Status {
        running: bool,
        active_turn_id: Option<String>,
    },
    Delta {
        turn_id: String,
        item_id: String,
        delta: String,
    },
    Upsert {
        message: DraftMessage,
        order_key: String,
    },
    Error {
        message: String,
    },
}

struct RunState {
    turn_id: String,
    completed: AtomicBool,
    tx: broadcast::Sender<StreamEvent>,
    live: Mutex<IndexMap<String, DraftMessage>>,
    interrupt: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// What awaiting callers get back; chat ignores it.
    outcome: Mutex<RunOutcome>,
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
    /// The agent binary; `None` runs the provider's CLI from `PATH`.
    pub binary: Option<String>,
    /// Passed as `--model`; `None` keeps the CLI default (in-app chat).
    pub model: Option<String>,
    /// Tool restrictions; `None` leaves the CLI's own defaults (in-app chat).
    pub tools: Option<ToolPolicy>,
    /// Start the child with only `env` (no inherited environment) in its own
    /// process group, so a timeout can kill everything it spawned.
    pub isolated: bool,
    /// Wall-clock limit; past it the child (and, when isolated, its whole
    /// process group) is killed and the outcome is marked timed out.
    pub timeout: Option<std::time::Duration>,
}

/// Which tools a headless run may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicy {
    pub allowed: Vec<String>,
    pub disallowed: Vec<String>,
    /// Ignore every MCP server from user or project settings.
    pub strict_mcp: bool,
}

/// How a run ended, for callers that await it rather than stream it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOutcome {
    pub turn_id: String,
    /// The CLI's session id, from any line that carried one.
    pub session_id: Option<String>,
    /// The final assistant message (the `result` line's text).
    pub final_message: Option<String>,
    /// A spawn failure or an error the stream reported.
    pub error: Option<String>,
    /// `None` when the child was killed or never started.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

impl RunOutcome {
    /// Exited zero, in time, without a stream error, with a final message.
    pub fn succeeded(&self) -> bool {
        !self.timed_out
            && self.error.is_none()
            && self.exit_code == Some(0)
            && self.final_message.is_some()
    }
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

    /// Start a streaming turn for `input.provider`. Returns the new turn id, or an error if
    /// a turn is already running for this conversation.
    pub fn start_run(&self, input: StartRunInput) -> Result<String, String> {
        self.start(input).map(|(turn_id, _done)| turn_id)
    }

    /// Start a turn and wait for it to end. Same rules as [`Self::start_run`],
    /// but resolves to the run's final message, session id and exit status.
    pub async fn run_to_completion(&self, input: StartRunInput) -> Result<RunOutcome, String> {
        let (turn_id, done) = self.start(input)?;
        Ok(done.await.unwrap_or_else(|_| RunOutcome {
            turn_id,
            error: Some("the run ended without reporting an outcome".to_string()),
            ..RunOutcome::default()
        }))
    }

    fn start(
        &self,
        input: StartRunInput,
    ) -> Result<(String, tokio::sync::oneshot::Receiver<RunOutcome>), String> {
        let prefix = input.provider.id_prefix();
        let turn_id = format!("{prefix}-turn:{}", random_uuid());
        let (tx, _rx) = broadcast::channel::<StreamEvent>(1024);
        let run = Arc::new(RunState {
            turn_id: turn_id.clone(),
            completed: AtomicBool::new(false),
            tx,
            live: Mutex::new(IndexMap::new()),
            interrupt: Mutex::new(None),
            outcome: Mutex::new(RunOutcome {
                turn_id: turn_id.clone(),
                ..RunOutcome::default()
            }),
        });
        {
            // Check and insert under one lock, so two racing starts cannot
            // both see the conversation idle (TA-R2).
            let mut runs = self.runs.lock().unwrap();
            if runs
                .get(&input.conversation_id)
                .is_some_and(|r| !r.completed.load(Ordering::Relaxed))
            {
                return Err("The agent is already responding in this conversation".to_string());
            }
            runs.insert(input.conversation_id.clone(), run.clone());
        }
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        // Optimistic user message + running status, before the process starts.
        let user_msg = DraftMessage {
            id: format!("{prefix}-user:{turn_id}"),
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
                StreamProvider::Grok => run_grok(input, run.clone()).await,
                StreamProvider::Codex => run_codex(input, run.clone()).await,
            }
            finish_run(&run, "completed");
            let _ = done_tx.send(run.outcome.lock().unwrap().clone());
        });

        Ok((turn_id, done_rx))
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
    run.live
        .lock()
        .unwrap()
        .insert(message.id.clone(), message.clone());
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
        let _ = run.tx.send(StreamEvent::Upsert {
            message,
            order_key: key,
        });
    }
    emit_status(run, false);
}

/// Spawn a Messages-`stream-json` agent and pump its NDJSON into the run's broadcast.
///
/// Shared by Claude and grok because grok's `streaming-messages-json` IS the Messages
/// `stream-json` wire format - verified against grok 1.0.5, whose lines carry `session_id`,
/// `message.id`, `content_block_start.index` and `text_delta` exactly where
/// `parse_claude_stream_line` looks for them. Keeping one pump means interrupt and teardown
/// behaviour cannot drift between the two providers.
///
/// `stdin_prompt` is the one real difference: `claude -p` reads the prompt from stdin, while
/// `grok -p` takes it as the flag's value, so grok passes `false` and puts the prompt in
/// `args`.
async fn run_messages_stream_agent(
    binary: &str,
    args: Vec<String>,
    stdin_prompt: bool,
    input: StartRunInput,
    run: Arc<RunState>,
) {
    let binary = input.binary.clone().unwrap_or_else(|| binary.to_string());
    let mut command = tokio::process::Command::new(&binary);
    command
        .args(&args)
        .current_dir(&input.cwd)
        .stdin(if stdin_prompt {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        // Nothing reads stderr; a piped one could fill and wedge the child.
        .stderr(if input.isolated {
            Stdio::null()
        } else {
            Stdio::piped()
        });
    if input.isolated {
        // Only what the caller allowlisted, and a group of its own so a
        // timeout reaches everything the agent started.
        command.env_clear().process_group(0).kill_on_drop(true);
    }
    for (k, v) in &input.env {
        command.env(k, v);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let message = format!("failed to spawn {binary}: {e}");
            run.outcome.lock().unwrap().error = Some(message.clone());
            let _ = run.tx.send(StreamEvent::Error { message });
            return;
        }
    };
    let group = if input.isolated { child.id() } else { None };
    let deadline = input.timeout.map(|t| tokio::time::Instant::now() + t);

    // Feed the prompt on stdin, then close it.
    if stdin_prompt && let Some(mut stdin) = child.stdin.take() {
        let prompt = if input.prompt.ends_with('\n') {
            input.prompt.clone()
        } else {
            format!("{}\n", input.prompt)
        };
        if deadline.is_some() {
            // A child that never reads stdin must not stall us past the deadline.
            tokio::spawn(async move {
                let _ = stdin.write_all(prompt.as_bytes()).await;
            });
        } else {
            let _ = stdin.write_all(prompt.as_bytes()).await;
            drop(stdin);
        }
    }

    let stdout = child.stdout.take();
    let (int_tx, mut int_rx) = tokio::sync::oneshot::channel::<()>();
    *run.interrupt.lock().unwrap() = Some(int_tx);

    let mut message_id: Option<String> = None;
    let mut block_index: i64 = 0;

    let mut timed_out = false;
    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            tokio::select! {
                _ = &mut int_rx => {
                    let _ = child.start_kill();
                    break;
                }
                _ = sleep_until_deadline(deadline) => {
                    timed_out = true;
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
    // The stream can close while the child (or something it started) lives
    // on, so the deadline also bounds the wait.
    let status = if timed_out {
        None
    } else {
        tokio::select! {
            status = child.wait() => status.ok(),
            _ = sleep_until_deadline(deadline) => {
                timed_out = true;
                None
            }
        }
    };
    if timed_out {
        kill_group(group);
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    let mut outcome = run.outcome.lock().unwrap();
    outcome.timed_out = timed_out;
    outcome.exit_code = status.and_then(|s| s.code());
}

/// Resolves at `deadline`, or never when there is none.
async fn sleep_until_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// SIGKILL the process group led by `pgid`. Uses `kill(1)` rather than a libc
/// binding, which the workspace does not otherwise need.
fn kill_group(pgid: Option<u32>) {
    let Some(pgid) = pgid else {
        return;
    };
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The `claude` argv for `input`. In-app chat sets none of `model` and
/// `tools`, so its argv is exactly what it always was (TS-52).
pub fn claude_args(input: &StartRunInput) -> Vec<String> {
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
    if let Some(model) = &input.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(tools) = &input.tools {
        if tools.strict_mcp {
            args.push("--strict-mcp-config".into());
        }
        // One comma-joined value each: the flags are variadic, so a separate
        // value per tool would swallow whatever argument came next.
        if !tools.allowed.is_empty() {
            args.push("--allowedTools".into());
            args.push(tools.allowed.join(","));
        }
        if !tools.disallowed.is_empty() {
            args.push("--disallowedTools".into());
            args.push(tools.disallowed.join(","));
        }
    }
    args
}

/// Spawn `claude` and pump its stream-json output into the run's broadcast.
async fn run_claude(input: StartRunInput, run: Arc<RunState>) {
    let args = claude_args(&input);
    run_messages_stream_agent("claude", args, true, input, run).await;
}

/// Spawn `grok` headless and pump its Messages-format NDJSON into the run's broadcast.
///
/// Differences from `run_claude`, all verified against grok 1.0.5:
/// - `--output-format streaming-messages-json` is grok's name for Messages `stream-json`.
///   There is no `--verbose`; the stream is already fully framed.
/// - **the prompt is `-p`'s value, not stdin.** `grok -p/--single <PROMPT>` requires a
///   value, so there is nothing to write to stdin (`--prompt-file` is the file form).
/// - `--rules` carries the system prompt, the additive form. `--system-prompt-override`
///   would replace grok's own system prompt and strip its tool instructions.
/// - `GROK_CLAUDE_HOOKS_ENABLED=0`, for the same reason the pane command sets it: grok
///   reads `.claude/settings.local.json` hooks by default, which are Sebenza's *Claude*
///   hooks and would fire `claude-*` handlers with grok's camelCase payloads.
async fn run_grok(input: StartRunInput, run: Arc<RunState>) {
    let mut args: Vec<String> = vec![
        "-p".into(),
        input.prompt.clone(),
        "--output-format".into(),
        "streaming-messages-json".into(),
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
        args.push("--rules".into());
        args.push(sys.clone());
    }

    let mut input = input;
    input
        .env
        .insert("GROK_CLAUDE_HOOKS_ENABLED".to_string(), "0".to_string());
    run_messages_stream_agent("grok", args, false, input, run).await;
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
    {
        let mut outcome = run.outcome.lock().unwrap();
        if let Some(sid) = &parsed.session_id {
            outcome.session_id = Some(sid.clone());
        }
        if let Some(text) = &parsed.result_text {
            outcome.final_message = Some(text.clone());
        }
        if let Some(err) = &parsed.error {
            outcome.error = Some(err.clone());
        }
    }

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
            format!(
                "tool_result:{}",
                block.tool_call_id.clone().unwrap_or_default()
            )
        } else {
            format!(
                "{}:{}",
                block
                    .message_id
                    .clone()
                    .or_else(|| message_id.clone())
                    .unwrap_or_else(|| "msg".to_string()),
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
            let _ = run.tx.send(StreamEvent::Error {
                message: format!("failed to spawn codex: {e}"),
            });
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
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("item")
                .to_string();
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
                .unwrap_or_else(|| {
                    item.get("command")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                });
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
            let msg = event
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("codex error")
                .to_string();
            let _ = run.tx.send(StreamEvent::Error { message: msg });
        }
        _ => {}
    }
}

#[cfg(test)]
mod stream_provider_tests {
    use super::*;
    use common::services::agent_registry::BuiltinAgentId;

    /// grok must not emit `claude-turn:`/`claude-user:` ids. Claude and Codex keep sharing
    /// the `claude` namespace, because that is what they already do and the value is a
    /// diagnostic label rather than something the frontend parses - changing Codex's would
    /// be a behaviour change for no gain.
    #[test]
    fn each_provider_namespaces_its_ids_and_grok_does_not_borrow_claudes() {
        assert_eq!(StreamProvider::Claude.id_prefix(), "claude");
        assert_eq!(StreamProvider::Codex.id_prefix(), "claude");
        assert_eq!(StreamProvider::Grok.id_prefix(), "grok");
    }

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
                BuiltinAgentId::Grok => assert!(matches!(provider, Some(StreamProvider::Grok))),
                // Explicitly no provider yet — chat is disabled for opencode via its
                // capabilities until the plugin and export adapter land.
                BuiltinAgentId::Opencode => assert!(provider.is_none()),
            }
        }
    }
}

#[cfg(test)]
mod run_outcome_tests {
    use super::*;
    use std::time::Duration;

    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// A fresh directory holding an executable `agent.sh` with `body`.
    fn stub(body: &str) -> (std::path::PathBuf, String) {
        use std::os::unix::fs::PermissionsExt;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("sebenza-agent-stream-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("agent.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path.to_string_lossy().to_string())
    }

    fn input(binary: Option<String>, cwd: &std::path::Path) -> StartRunInput {
        StartRunInput {
            provider: StreamProvider::Claude,
            conversation_id: format!("test:{}", random_uuid()),
            cwd: cwd.to_string_lossy().to_string(),
            prompt: "ping".into(),
            env: HashMap::new(),
            permission_mode: None,
            resume_session_id: None,
            system_prompt: None,
            binary,
            model: None,
            tools: None,
            isolated: false,
            timeout: None,
        }
    }

    fn fixture_path() -> String {
        format!(
            "{}/../common/src/adapters/testdata/claude_stream.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    // TS-52: in-app chat sets no model and no tool policy, and its argv is
    // byte-for-byte what `run_claude` built before the system agent existed.
    #[test]
    fn chat_argv_is_unchanged() {
        let mut chat = input(None, std::path::Path::new("/tmp"));
        chat.resume_session_id = Some("sid".into());
        chat.permission_mode = Some("bypassPermissions".into());
        chat.system_prompt = Some("sys".into());
        assert_eq!(
            claude_args(&chat),
            [
                "-p",
                "--verbose",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
                "-r",
                "sid",
                "--permission-mode",
                "bypassPermissions",
                "--append-system-prompt",
                "sys",
            ]
        );
        let fresh = input(None, std::path::Path::new("/tmp"));
        assert_eq!(
            claude_args(&fresh),
            [
                "-p",
                "--verbose",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
            ]
        );
    }

    #[test]
    fn model_and_tool_policy_extend_the_argv() {
        let mut run = input(None, std::path::Path::new("/tmp"));
        run.model = Some("claude-sonnet-4-5".into());
        run.tools = Some(ToolPolicy {
            allowed: vec!["Read".into(), "Grep".into()],
            disallowed: vec!["Bash".into(), "Write".into()],
            strict_mcp: true,
        });
        let args = claude_args(&run);
        let after = |flag: &str| {
            let i = args.iter().position(|a| a == flag).expect(flag);
            args[i + 1].clone()
        };
        assert_eq!(after("--model"), "claude-sonnet-4-5");
        assert_eq!(after("--allowedTools"), "Read,Grep");
        assert_eq!(after("--disallowedTools"), "Bash,Write");
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    }

    // TS-16: the recorded claude stream yields its final message and session id.
    #[tokio::test]
    async fn run_to_completion_returns_the_final_message_and_session_id() {
        let (dir, bin) = stub(&format!("cat >/dev/null\ncat '{}'", fixture_path()));
        let outcome = AgentStreamManager::new()
            .run_to_completion(input(Some(bin), &dir))
            .await
            .expect("started");
        assert_eq!(outcome.final_message.as_deref(), Some("pong"));
        assert_eq!(
            outcome.session_id.as_deref(),
            Some("8fd04c17-2ee0-4a02-9867-118288169ac2")
        );
        assert_eq!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out);
        assert!(outcome.succeeded());
    }

    #[tokio::test]
    async fn a_non_zero_exit_is_not_a_success() {
        let (dir, bin) = stub(&format!("cat >/dev/null\ncat '{}'\nexit 3", fixture_path()));
        let outcome = AgentStreamManager::new()
            .run_to_completion(input(Some(bin), &dir))
            .await
            .unwrap();
        assert_eq!(outcome.exit_code, Some(3));
        assert!(!outcome.succeeded());
    }

    #[tokio::test]
    async fn a_missing_binary_is_reported_not_panicked() {
        let dir = std::env::temp_dir();
        let outcome = AgentStreamManager::new()
            .run_to_completion(input(Some("/nonexistent/agent".into()), &dir))
            .await
            .unwrap();
        assert!(outcome.error.is_some());
        assert!(!outcome.succeeded());
    }

    // TS-21 / TS-65: a hung child is killed at the deadline, and so is every
    // process it started (the whole process group).
    #[tokio::test]
    async fn timeout_kills_the_whole_process_group() {
        let (dir, bin) =
            stub("cat >/dev/null\nsleep 300 &\necho $! > \"$(dirname \"$0\")/grandchild\"\nwait");
        let mut run = input(Some(bin), &dir);
        run.isolated = true;
        run.env
            .insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
        run.timeout = Some(Duration::from_millis(800));
        let started = std::time::Instant::now();
        let outcome = AgentStreamManager::new()
            .run_to_completion(run)
            .await
            .unwrap();
        assert!(outcome.timed_out);
        assert!(!outcome.succeeded());
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = std::fs::read_to_string(dir.join("grandchild"))
            .unwrap()
            .trim()
            .to_string();
        assert!(process_gone(&pid), "grandchild {pid} survived the timeout");
    }

    /// True once `pid` no longer exists (or is only a zombie awaiting reaping).
    fn process_gone(pid: &str) -> bool {
        for _ in 0..50 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(stat) if stat.contains(") Z") => return true,
                Ok(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        false
    }

    #[tokio::test]
    async fn an_isolated_child_sees_only_the_env_it_is_given() {
        let (dir, bin) = stub(&format!(
            "cat >/dev/null\nenv > \"$(dirname \"$0\")/env\"\ncat '{}'",
            fixture_path()
        ));
        let mut run = input(Some(bin), &dir);
        run.isolated = true;
        run.env.insert("ONLY_ME".into(), "1".into());
        run.env.insert("PATH".into(), "/usr/bin:/bin".into());
        AgentStreamManager::new()
            .run_to_completion(run)
            .await
            .unwrap();
        let env = std::fs::read_to_string(dir.join("env")).unwrap();
        let keys: Vec<&str> = env.lines().filter_map(|l| l.split('=').next()).collect();
        assert!(keys.contains(&"ONLY_ME"), "{env}");
        assert!(!keys.contains(&"HOME"), "{env}");
        assert!(!keys.contains(&"CARGO_MANIFEST_DIR"), "{env}");
    }

    // TS-52: one turn at a time per conversation is still enforced.
    #[tokio::test]
    async fn a_second_turn_in_the_same_conversation_is_still_rejected() {
        let (dir, bin) = stub("cat >/dev/null\nsleep 2");
        let manager = AgentStreamManager::new();
        let first = input(Some(bin.clone()), &dir);
        let mut second = input(Some(bin), &dir);
        second.conversation_id = first.conversation_id.clone();
        manager.start_run(first).expect("first turn starts");
        assert!(manager.start_run(second).is_err());
    }
}
