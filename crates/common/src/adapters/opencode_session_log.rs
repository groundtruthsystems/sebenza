//! opencode session history.
//!
//! **Reads through the CLI, not the database.** opencode stores sessions in a SQLite
//! `opencode.db` (WAL mode), but its schema is internal and undocumented. `opencode export
//! <id>` is the supported read path and is what this adapter uses.
//!
//! Two verified constraints shape this module (see `spec.md` → *Verified findings*):
//!
//! 1. **Never pass `--sanitize`.** It redacts message text, tool input, tool output *and*
//!    metadata, yielding `[redacted:…]` placeholders. It is a transcript-*sharing*
//!    feature; a sanitized export is useless as chat history.
//! 2. **`project_id` is per-repository, not per-worktree.** Every worktree of a repo shares
//!    one project, and `opencode session list` is project-scoped with no directory column.
//!    Correlation therefore uses `info.directory` from the export, matched exactly.
//!
//! The primary path is not discovery at all: Sebenza records the session id it started
//! (the plugin's `session.created`, or the `sessionID` echoed by `run --format json`).
//! Discovery by directory costs one `export` per candidate, so it is an orphan-adoption
//! fallback only — never a per-request poll.

use crate::services::agents_ui::AgentsUiMessage;
use serde_json::Value;

/// An exported opencode session: its `info` metadata plus normalized messages.
pub struct OpencodeSession {
    pub id: String,
    /// `info.directory` — the exact worktree path this session belongs to.
    pub directory: Option<String>,
    /// `info.version` — useful for surfacing an unsupported-version state.
    pub version: Option<String>,
    pub messages: Vec<AgentsUiMessage>,
}

fn part_text(part: &Value) -> String {
    part.get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Map one `messages[]` entry's parts onto `AgentsUiMessage`s.
///
/// Part types observed in a real export: `text`, `reasoning`, `tool`, `step-start`,
/// `step-finish`. The step markers are turn boundaries and are not rendered.
fn map_message(message: &Value, turn_id: &str, order: &mut usize) -> Vec<AgentsUiMessage> {
    let role = message
        .get("info")
        .and_then(|i| i.get("role"))
        .and_then(Value::as_str)
        .unwrap_or("assistant");
    let created = message
        .get("info")
        .and_then(|i| i.get("time"))
        .and_then(|t| t.get("created"))
        .and_then(Value::as_i64)
        .map(|ms| ms.to_string());

    let mut out = Vec::new();
    let Some(parts) = message.get("parts").and_then(Value::as_array) else {
        return out;
    };

    for part in parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
        let id = part
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("oc-{}", *order));

        match kind {
            "text" => {
                let text = part_text(part);
                if text.is_empty() {
                    continue;
                }
                let mut m = AgentsUiMessage::new(id, turn_id.to_string(), role, "text", text);
                m.order = *order;
                m.created_at = created.clone();
                out.push(m);
                *order += 1;
            }
            "reasoning" => {
                let text = part_text(part);
                if text.is_empty() {
                    continue;
                }
                let mut m = AgentsUiMessage::new(id, turn_id.to_string(), role, "thinking", text);
                m.order = *order;
                m.created_at = created.clone();
                out.push(m);
                *order += 1;
            }
            "tool" => {
                let state = part.get("state").cloned().unwrap_or(Value::Null);
                let tool_name = part.get("tool").and_then(Value::as_str).map(str::to_string);
                let command = state
                    .get("input")
                    .and_then(|i| i.get("command"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let status = state
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed")
                    .to_string();

                // The invocation.
                let invocation_text = command.clone().unwrap_or_else(|| {
                    state
                        .get("input")
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                });
                let mut use_msg = AgentsUiMessage::new(
                    format!("{id}:use"),
                    turn_id.to_string(),
                    role,
                    "toolUse",
                    invocation_text,
                );
                use_msg.order = *order;
                use_msg.created_at = created.clone();
                use_msg.tool_name = tool_name.clone();
                use_msg.tool_call_id = Some(id.clone());
                use_msg.command = command;
                use_msg.status = status.clone();
                out.push(use_msg);
                *order += 1;

                // Its result. `state.metadata.exit` gives a real exit code, so no
                // Codex-style scraping of output text is needed.
                let output = state
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let exit_code = state
                    .get("metadata")
                    .and_then(|m| m.get("exit"))
                    .and_then(Value::as_i64);
                let truncated = state
                    .get("metadata")
                    .and_then(|m| m.get("truncated"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let text = if truncated {
                    format!("{output}\n[output truncated by opencode]")
                } else {
                    output.to_string()
                };
                let mut result = AgentsUiMessage::new(
                    format!("{id}:result"),
                    turn_id.to_string(),
                    role,
                    "toolResult",
                    text,
                );
                result.order = *order;
                result.created_at = created.clone();
                result.tool_name = tool_name;
                result.tool_call_id = Some(id.clone());
                result.exit_code = exit_code;
                result.status = status;
                out.push(result);
                *order += 1;
            }
            // step-start / step-finish are turn boundaries; anything unknown is skipped
            // rather than guessed at, so a new part type cannot break the transcript.
            _ => {}
        }
    }
    out
}

/// Parse the JSON emitted by `opencode export <id>` (**without** `--sanitize`).
///
/// Tolerates unknown fields and malformed input: a shape this does not recognise yields
/// an empty message list rather than an error, matching the claude/codex parsers.
pub fn parse_export(text: &str) -> Option<OpencodeSession> {
    let root: Value = serde_json::from_str(text).ok()?;
    let info = root.get("info")?;
    let id = info.get("id").and_then(Value::as_str)?.to_string();

    let mut order = 0usize;
    let mut messages = Vec::new();
    if let Some(list) = root.get("messages").and_then(Value::as_array) {
        for (turn, message) in list.iter().enumerate() {
            let turn_id = message
                .get("info")
                .and_then(|i| i.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("turn-{turn}"));
            messages.extend(map_message(message, &turn_id, &mut order));
        }
    }

    Some(OpencodeSession {
        id,
        directory: info
            .get("directory")
            .and_then(Value::as_str)
            .map(str::to_string),
        version: info
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
        messages,
    })
}

/// Does this exported session belong to `cwd`?
///
/// **Exact string equality, never a prefix match** — `/repo/wt-1` and `/repo/wt-10` would
/// otherwise collide.
pub fn session_belongs_to(session: &OpencodeSession, cwd: &str) -> bool {
    session.directory.as_deref() == Some(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `opencode export` payload captured in phase-0-task-3, from a session that
    /// exercised a bash tool call. Absolute paths were neutralised for portability.
    const FIXTURE: &str = include_str!("testdata/opencode_export.json");

    fn parsed() -> OpencodeSession {
        parse_export(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn reads_session_metadata_needed_for_correlation_and_version_gating() {
        let s = parsed();
        assert!(s.id.starts_with("ses_"), "session id: {}", s.id);
        assert_eq!(
            s.directory.as_deref(),
            Some("/repo/worktrees/example-branch")
        );
        assert_eq!(s.version.as_deref(), Some("1.18.7"));
    }

    #[test]
    fn maps_tool_parts_to_a_use_and_a_result_with_a_real_exit_code() {
        let s = parsed();
        let use_msg = s
            .messages
            .iter()
            .find(|m| m.kind == "toolUse")
            .expect("a toolUse message");
        assert_eq!(use_msg.tool_name.as_deref(), Some("bash"));
        assert_eq!(use_msg.command.as_deref(), Some("echo hello-from-tool"));

        let result = s
            .messages
            .iter()
            .find(|m| m.kind == "toolResult")
            .expect("a toolResult message");
        assert_eq!(
            result.exit_code,
            Some(0),
            "exit code comes from state.metadata.exit, not from scraping output text"
        );
        assert!(
            result.text.contains("hello-from-tool"),
            "result text: {}",
            result.text
        );
        // The pair must correlate.
        assert_eq!(use_msg.tool_call_id, result.tool_call_id);
    }

    #[test]
    fn maps_text_and_reasoning_and_skips_step_markers() {
        let s = parsed();
        let kinds: Vec<&str> = s.messages.iter().map(|m| m.kind.as_str()).collect();
        assert!(kinds.contains(&"text"), "kinds: {kinds:?}");
        assert!(
            !kinds.iter().any(|k| k.contains("step")),
            "step-start/step-finish are turn boundaries, not messages: {kinds:?}"
        );
        // Orders are dense and ascending, since the frontend renders by order.
        let orders: Vec<usize> = s.messages.iter().map(|m| m.order).collect();
        assert_eq!(orders, (0..orders.len()).collect::<Vec<_>>());
    }

    #[test]
    fn correlation_is_exact_never_a_prefix_match() {
        let s = parsed();
        assert!(session_belongs_to(&s, "/repo/worktrees/example-branch"));
        assert!(
            !session_belongs_to(&s, "/repo/worktrees/example-branch-2"),
            "a longer sibling path must not match"
        );
        assert!(
            !session_belongs_to(&s, "/repo/worktrees"),
            "a parent must not match"
        );
    }

    #[test]
    fn malformed_and_unknown_shapes_degrade_instead_of_panicking() {
        assert!(parse_export("not json").is_none());
        assert!(parse_export("{}").is_none(), "no info means no session");
        // Unknown part types and missing fields are skipped, not fatal.
        let odd = r#"{"info":{"id":"ses_x"},"messages":[
            {"info":{"role":"assistant"},"parts":[{"type":"brand-new-part"},{"type":"text"}]}
        ]}"#;
        let s = parse_export(odd).expect("parses despite unknown part");
        assert!(
            s.messages.is_empty(),
            "empty text is skipped, unknown type ignored"
        );
        assert_eq!(s.directory, None);
    }
}
