//! Each attach allocates a native pseudo-terminal via `portable-pty` (no
//! `python3`/`script` dependency) and runs the `build_attach_cmd` bash script
//! inside it, which `exec`s `tmux attach-session -t <grouped-session>`. tmux does
//! the real multiplexing; the PTY only gives it a tty and pipes bytes to/from the
//! socket. A grouped session (`new-session -t <owner>`) is created per attach so
//! multiple clients can share the owner window without fighting over its size.
//!
//! Wire protocol (see `server.rs`): outbound hot-path frames are prefix-encoded
//! `"o"+data` (output) / `"s"+scrollback`; other frames are JSON. Inbound frames
//! are JSON discriminated on `type`: `input`, `sendKeys`, `selectPane`, `resize`
//! (the first `resize` triggers attach).

use crate::adapters::tmux::run_tmux;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const MAX_SCROLLBACK_BYTES: usize = 1024 * 1024; // 1 MB

/// The tmux window a terminal attaches to (owner session + window name).
#[derive(Clone)]
pub struct TerminalAttachTarget {
    pub owner_session_name: String,
    pub window_name: String,
}

/// Output pumped from the PTY read thread to the WebSocket task.
pub enum TerminalEvent {
    Data(String),
    Exit(i32),
}

#[derive(Default)]
struct Scrollback {
    chunks: VecDeque<String>,
    bytes: usize,
}

impl Scrollback {
    fn push(&mut self, chunk: &str) {
        self.bytes += chunk.len();
        self.chunks.push_back(chunk.to_string());
        while self.bytes > MAX_SCROLLBACK_BYTES && !self.chunks.is_empty() {
            if let Some(removed) = self.chunks.pop_front() {
                self.bytes -= removed.len();
            }
        }
    }

    fn joined(&self) -> String {
        self.chunks.iter().cloned().collect()
    }
}

struct Session {
    grouped_session_name: String,
    window_name: String,
    writer: Box<dyn Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    scrollback: Arc<Mutex<Scrollback>>,
    cancelled: Arc<AtomicBool>,
}

/// Manages PTY-backed tmux attach sessions keyed by `attach_id`. Session names are
/// scoped by backend port so multiple backends sharing a tmux server don't collide.
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, Session>>,
    session_prefix: String,
    grouped_counter: AtomicU64,
    attach_counter: AtomicU64,
}

impl TerminalManager {
    pub fn new(port: u16) -> Self {
        TerminalManager {
            sessions: Mutex::new(HashMap::new()),
            session_prefix: format!("sebenza-dash-{port}-"),
            grouped_counter: AtomicU64::new(0),
            attach_counter: AtomicU64::new(0),
        }
    }

    /// A fresh, process-unique attach id for a worktree (`<worktreeId>:<seq>`).
    pub fn new_attach_id(&self, worktree_id: &str) -> String {
        let n = self.attach_counter.fetch_add(1, Ordering::Relaxed);
        format!("{worktree_id}:{n}")
    }

    fn grouped_name(&self) -> String {
        let n = self.grouped_counter.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{}{}", self.session_prefix, n)
    }

    /// Kill any orphaned `sebenza-dash-<port>-*` tmux sessions from previous runs.
    pub fn cleanup_stale_sessions(&self) {
        let (stdout, _stderr, code) = run_tmux(&["list-sessions", "-F", "#{session_name}"]);
        if code != 0 {
            return;
        }
        for name in stdout.lines() {
            if name.starts_with(&self.session_prefix) {
                run_tmux(&["kill-session", "-t", name]);
            }
        }
    }

    /// Spawn a PTY attached to `target`'s tmux window. Returns a receiver that
    /// yields terminal output and a final exit event. Any prior session under the
    /// same `attach_id` is detached first.
    pub fn attach(
        &self,
        attach_id: &str,
        target: &TerminalAttachTarget,
        cols: u16,
        rows: u16,
        initial_pane: Option<i64>,
    ) -> Result<UnboundedReceiver<TerminalEvent>, String> {
        if self.sessions.lock().unwrap().contains_key(attach_id) {
            self.detach(attach_id);
        }

        let g_name = self.grouped_name();
        // Kill a stale session with the same name (leftover from a previous run).
        kill_tmux_session(&g_name);

        let cmd_str = build_attach_cmd(
            &g_name,
            &target.window_name,
            &target.owner_session_name,
            cols,
            rows,
            initial_pane,
        );

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new("bash");
        cmd.arg("-c");
        cmd.arg(&cmd_str);
        cmd.env("TERM", "xterm-256color");
        for key in leaked_project_env_keys() {
            cmd.env_remove(&key);
        }

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let scrollback = Arc::new(Mutex::new(Scrollback::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded_channel::<TerminalEvent>();

        spawn_reader(reader, tx, scrollback.clone(), cancelled.clone());

        let session = Session {
            grouped_session_name: g_name,
            window_name: target.window_name.clone(),
            writer,
            master: pair.master,
            child,
            scrollback,
            cancelled,
        };
        self.sessions
            .lock()
            .unwrap()
            .insert(attach_id.to_string(), session);
        Ok(rx)
    }

    pub fn detach(&self, attach_id: &str) {
        let session = self.sessions.lock().unwrap().remove(attach_id);
        if let Some(mut session) = session {
            session.cancelled.store(true, Ordering::Relaxed);
            let _ = session.child.kill();
            kill_tmux_session(&session.grouped_session_name);
        }
    }

    pub fn write(&self, attach_id: &str, data: &str) {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(attach_id) {
            let _ = session.writer.write_all(data.as_bytes());
            let _ = session.writer.flush();
        }
    }

    /// Send raw hex bytes to the active tmux pane via `tmux send-keys -H`,
    /// bypassing tmux's input parser (needed for CSI u sequences).
    pub fn send_keys(&self, attach_id: &str, hex_bytes: &[String]) {
        let Some(target) = self.window_target(attach_id) else {
            return;
        };
        let mut args: Vec<&str> = vec!["send-keys", "-t", &target, "-H"];
        for hex in hex_bytes {
            args.push(hex);
        }
        run_tmux(&args);
    }

    pub fn resize(&self, attach_id: &str, cols: u16, rows: u16) {
        let target = {
            let mut map = self.sessions.lock().unwrap();
            let Some(session) = map.get_mut(attach_id) else {
                return;
            };
            let _ = session.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
            format!("{}:{}", session.grouped_session_name, session.window_name)
        };
        let (cols, rows) = (cols.to_string(), rows.to_string());
        let (_o, stderr, code) =
            run_tmux(&["resize-window", "-t", &target, "-x", &cols, "-y", &rows]);
        if code != 0 {
            tracing::warn!("[term] resize failed: {stderr}");
        }
    }

    pub fn select_pane(&self, attach_id: &str, pane_index: i64) {
        let target = {
            let map = self.sessions.lock().unwrap();
            let Some(session) = map.get(attach_id) else {
                return;
            };
            format!(
                "{}:{}.{}",
                session.grouped_session_name, session.window_name, pane_index
            )
        };
        run_tmux(&["select-pane", "-t", &target]);
        run_tmux(&["resize-pane", "-Z", "-t", &target]);
    }

    pub fn get_scrollback(&self, attach_id: &str) -> String {
        self.sessions
            .lock()
            .unwrap()
            .get(attach_id)
            .map(|s| s.scrollback.lock().unwrap().joined())
            .unwrap_or_default()
    }

    /// `<grouped-session>:<window>` target for tmux commands, or `None` if the
    /// session has gone away.
    fn window_target(&self, attach_id: &str) -> Option<String> {
        self.sessions
            .lock()
            .unwrap()
            .get(attach_id)
            .map(|s| format!("{}:{}", s.grouped_session_name, s.window_name))
    }

    /// Type a prompt into the worktree's owner pane and submit it. Loads the text
    /// via a tmux paste buffer (bracketed-paste safe), then sends Enter after an
    /// optional submit delay. Operates on the owner session, not a grouped attach.
    pub fn send_prompt(
        &self,
        target: &TerminalAttachTarget,
        text: &str,
        pane_index: i64,
        preamble: Option<&str>,
        submit_delay_ms: u64,
    ) -> Result<(), String> {
        let pane_target = format!(
            "{}:{}.{}",
            target.owner_session_name, target.window_name, pane_index
        );

        if let Some(preamble) = preamble {
            tmux_exec(
                &["send-keys", "-t", &pane_target, "-l", "--", preamble],
                None,
            )
            .map_err(|e| format!("send-keys preamble failed{}", detail(&e)))?;
        }

        let cleaned = text.replace('\0', "");
        let buffer_name = format!("sebenza-prompt-{}", crate::util::id::random_hex(8));
        tmux_exec(
            &["load-buffer", "-b", &buffer_name, "-"],
            Some(cleaned.as_bytes()),
        )
        .map_err(|e| format!("load-buffer failed{}", detail(&e)))?;
        tmux_exec(
            &[
                "paste-buffer",
                "-rp",
                "-b",
                &buffer_name,
                "-t",
                &pane_target,
                "-d",
            ],
            None,
        )
        .map_err(|e| format!("paste-buffer failed{}", detail(&e)))?;

        if submit_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(submit_delay_ms));
        }
        tmux_exec(&["send-keys", "-t", &pane_target, "Enter"], None)
            .map_err(|e| format!("send-keys Enter failed{}", detail(&e)))
    }

    /// Send Ctrl-C to the worktree's owner pane.
    pub fn interrupt_prompt(
        &self,
        target: &TerminalAttachTarget,
        pane_index: i64,
    ) -> Result<(), String> {
        let pane_target = format!(
            "{}:{}.{}",
            target.owner_session_name, target.window_name, pane_index
        );
        tmux_exec(&["send-keys", "-t", &pane_target, "C-c"], None)
            .map_err(|e| format!("send-keys C-c failed{}", detail(&e)))
    }
}

fn detail(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

/// Run a tmux command (env-stripped), optionally piping `stdin_data`. Returns
/// `Err(stderr)` on non-zero exit.
fn tmux_exec(args: &[&str], stdin_data: Option<&[u8]>) -> Result<(), String> {
    use std::io::Write as _;
    use std::process::Stdio;

    let mut command = Command::new("tmux");
    command.args(args);
    for key in leaked_project_env_keys() {
        command.env_remove(key);
    }
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().map_err(|e| e.to_string())?;
    if let Some(data) = stdin_data {
        child
            .stdin
            .take()
            .ok_or("failed to open tmux stdin")?
            .write_all(data)
            .map_err(|e| e.to_string())?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Read PTY output on a dedicated blocking thread, pushing chunks to the
/// scrollback ring and forwarding them to the WebSocket task. Emits a final
/// `Exit` unless the session was cancelled (intentionally detached).
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: UnboundedSender<TerminalEvent>,
    scrollback: Arc<Mutex<Scrollback>>,
    cancelled: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                    scrollback.lock().unwrap().push(&chunk);
                    if tx.send(TerminalEvent::Data(chunk)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        if !cancelled.load(Ordering::Relaxed) {
            let _ = tx.send(TerminalEvent::Exit(0));
        }
    });
}

/// Kill a tmux session by name, logging unexpected failures.
fn kill_tmux_session(name: &str) {
    let (_o, stderr, code) = run_tmux(&["kill-session", "-t", name]);
    if code != 0 && !stderr.contains("can't find session") {
        tracing::warn!("[term] kill_tmux_session({name}) exit={code} {stderr}");
    }
}

/// The launch project's `.env` keys that must be stripped from tmux spawns so a
/// not-yet-running tmux server isn't born with leaked secrets.
fn leaked_project_env_keys() -> Vec<String> {
    std::env::var("SEBENZA_PROJECT_ENV_KEYS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Build the bash pipeline that creates a grouped tmux session over the owner
/// session, selects the target window/pane, and `exec`s the attach. Ported
/// verbatim from `buildAttachCmd`.
fn build_attach_cmd(
    g_name: &str,
    window_name: &str,
    owner_session_name: &str,
    cols: u16,
    rows: u16,
    initial_pane: Option<i64>,
) -> String {
    let pane = initial_pane.unwrap_or(0);
    let pane_target = format!("{g_name}:{window_name}.{pane}");
    let mut parts = vec![
        format!("tmux new-session -d -s \"{g_name}\" -t \"{owner_session_name}\""),
        format!("tmux set-option -t \"{owner_session_name}\" window-size latest"),
        format!("tmux set-option -t \"{g_name}\" mouse on"),
        format!("tmux set-option -t \"{g_name}\" set-clipboard on"),
        format!("tmux select-window -t \"{g_name}:{window_name}\""),
        format!(
            "if [ \"$(tmux display-message -t '{g_name}:{window_name}' -p '#{{window_zoomed_flag}}')\" = \"1\" ]; then tmux resize-pane -Z -t '{g_name}:{window_name}'; fi"
        ),
        format!("tmux select-pane -t \"{pane_target}\""),
    ];
    if initial_pane.is_some() {
        parts.push(format!("tmux resize-pane -Z -t \"{pane_target}\""));
    }
    parts.push(format!("stty rows {rows} cols {cols}"));
    parts.push(format!("exec tmux attach-session -t \"{g_name}\""));
    parts.join(" && ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_cmd_wires_owner_and_grouped_sessions() {
        let cmd = build_attach_cmd(
            "sebenza-dash-5111-1",
            "sebenza-main",
            "sebenza-proj",
            80,
            24,
            None,
        );
        assert!(cmd.contains("tmux new-session -d -s \"sebenza-dash-5111-1\" -t \"sebenza-proj\""));
        assert!(cmd.contains("tmux select-window -t \"sebenza-dash-5111-1:sebenza-main\""));
        assert!(cmd.contains("#{window_zoomed_flag}"));
        assert!(cmd.contains("stty rows 24 cols 80"));
        assert!(cmd.ends_with("exec tmux attach-session -t \"sebenza-dash-5111-1\""));
        // Without an initial pane there is no forced zoom on the pane target.
        assert!(!cmd.contains("resize-pane -Z -t \"sebenza-dash-5111-1:sebenza-main.0\""));
    }

    #[test]
    fn attach_cmd_zooms_initial_pane_on_mobile() {
        let cmd = build_attach_cmd("g", "sebenza-main", "owner", 80, 24, Some(1));
        assert!(cmd.contains("tmux select-pane -t \"g:sebenza-main.1\""));
        assert!(cmd.contains("tmux resize-pane -Z -t \"g:sebenza-main.1\""));
    }

    #[test]
    fn scrollback_evicts_oldest_when_over_budget() {
        let mut sb = Scrollback::default();
        let big = "x".repeat(MAX_SCROLLBACK_BYTES);
        sb.push(&big);
        sb.push("tail");
        // The oversized first chunk is evicted once the tail pushes over budget.
        assert_eq!(sb.joined(), "tail");
        assert_eq!(sb.bytes, 4);
    }
}
