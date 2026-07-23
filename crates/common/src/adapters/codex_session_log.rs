//! Codex session-log reader — port of the pure parser in
//! `backend-legacy/src/services/codex-session-log-service.ts` plus on-disk
//! session discovery (`~/.codex/sessions/**/rollout-*.jsonl`, matched by the
//! `session_meta.cwd`). Lets Codex conversation *history* be read without the
//! Codex app-server. UNVERIFIED-HERE (no real codex sessions); fixture-tested.

use crate::services::agents_ui::AgentsUiMessage;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const TOOL_OUTPUT_TRUNCATE_LIMIT: usize = 12000;

fn read_string(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

fn truncate(text: &str) -> String {
    let count = text.chars().count();
    if count <= TOOL_OUTPUT_TRUNCATE_LIMIT {
        return text.to_string();
    }
    let head: String = text.chars().take(TOOL_OUTPUT_TRUNCATE_LIMIT).collect();
    format!("{head}... (truncated, {} more chars)", count - TOOL_OUTPUT_TRUNCATE_LIMIT)
}

fn compact_json(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| v.to_string())
}

fn read_reasoning_summary(raw: &Value) -> String {
    let Some(arr) = raw.as_array() else {
        return String::new();
    };
    arr.iter()
        .map(|entry| {
            if let Some(s) = entry.as_str() {
                return s.to_string();
            }
            if let Some(t) = entry.get("text").and_then(Value::as_str) {
                return t.to_string();
            }
            if let Some(s) = entry.get("summary").and_then(Value::as_str) {
                return s.to_string();
            }
            String::new()
        })
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn parse_arguments(raw: Option<&str>) -> Option<Value> {
    let raw = raw?;
    serde_json::from_str::<Value>(raw).ok().filter(|v| v.is_object())
}

fn read_tool_command(tool_name: &str, arguments_text: Option<&str>) -> Option<String> {
    if tool_name == "apply_patch" {
        return Some("apply_patch".to_string());
    }
    let args = parse_arguments(arguments_text)?;
    if tool_name == "exec_command"
        && let Some(cmd) = args.get("cmd").and_then(Value::as_str)
    {
        return Some(cmd.to_string());
    }
    None
}

fn read_tool_cwd(arguments_text: Option<&str>) -> Option<String> {
    parse_arguments(arguments_text)?
        .get("workdir")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn build_tool_use_text(tool_name: &str, arguments_text: Option<&str>) -> String {
    read_tool_command(tool_name, arguments_text)
        .unwrap_or_else(|| arguments_text.unwrap_or("").trim().to_string())
}

fn read_output_exit_code(output: &str) -> Option<i64> {
    if let Some(idx) = output.find("Process exited with code ") {
        let rest = &output[idx + "Process exited with code ".len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
        if let Ok(code) = num.parse::<i64>() {
            return Some(code);
        }
    }
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("Exit code: ") {
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
            if let Ok(code) = num.parse::<i64>() {
                return Some(code);
            }
        }
    }
    None
}

fn read_output_status(output: &str) -> String {
    match read_output_exit_code(output) {
        Some(0) => "completed".to_string(),
        Some(_) => "failed".to_string(),
        None => {
            if output.starts_with("apply_patch verification failed") {
                "failed".to_string()
            } else {
                "completed".to_string()
            }
        }
    }
}

fn has_duplicate_text(
    messages: &[AgentsUiMessage],
    turn_id: &str,
    role: &str,
    text: &str,
    phase: Option<&str>,
) -> bool {
    messages.iter().any(|m| {
        m.turn_id == turn_id
            && m.role == role
            && m.kind == "text"
            && m.text == text
            && m.phase.as_deref() == phase
    })
}

fn push(messages: &mut Vec<AgentsUiMessage>, mut message: AgentsUiMessage) {
    message.order = messages.len();
    messages.push(message);
}

/// Correlate each toolUse's status/exitCode/duration with its matching toolResult.
fn finalize_tool_statuses(messages: Vec<AgentsUiMessage>) -> Vec<AgentsUiMessage> {
    let mut result_by_call: HashMap<String, (String, Option<i64>, Option<i64>)> = HashMap::new();
    for m in &messages {
        if m.kind == "toolResult"
            && let Some(id) = &m.tool_call_id
        {
            result_by_call.insert(id.clone(), (m.status.clone(), m.exit_code, m.duration_ms));
        }
    }
    messages
        .into_iter()
        .map(|mut m| {
            if m.kind == "toolUse"
                && let Some(id) = &m.tool_call_id
                && let Some((status, exit_code, duration_ms)) = result_by_call.get(id)
            {
                m.status = status.clone();
                if exit_code.is_some() {
                    m.exit_code = *exit_code;
                }
                if duration_ms.is_some() {
                    m.duration_ms = *duration_ms;
                }
            }
            m
        })
        .collect()
}

/// Parse a Codex rollout `.jsonl` into agents-ui messages (port of
/// `parseCodexSessionMessages`).
pub fn parse_codex_session_messages(text: &str) -> Vec<AgentsUiMessage> {
    let mut messages: Vec<AgentsUiMessage> = Vec::new();
    let mut tool_meta: HashMap<String, (String, Option<String>, Option<String>)> = HashMap::new();
    let mut current_turn: Option<String> = None;
    let mut block_index = 0usize;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let ts = record.get("timestamp").and_then(read_string);
        let rec_type = record.get("type").and_then(Value::as_str);
        let Some(payload) = record.get("payload").filter(|p| p.is_object()) else {
            continue;
        };

        if rec_type == Some("event_msg") {
            let event_type = payload.get("type").and_then(Value::as_str);
            match event_type {
                Some("task_started") => {
                    current_turn = payload.get("turn_id").and_then(read_string);
                    block_index = 0;
                }
                Some("task_complete") | Some("turn_aborted") => {
                    current_turn = None;
                }
                Some("user_message") => {
                    if let Some(turn) = current_turn.clone()
                        && let Some(text) = payload.get("message").and_then(read_string)
                        && !has_duplicate_text(&messages, &turn, "user", &text, None)
                    {
                        let mut m = AgentsUiMessage::new(
                            format!("user:{turn}:{block_index}"),
                            turn,
                            "user",
                            "text",
                            text,
                        );
                        m.created_at = ts.clone();
                        push(&mut messages, m);
                        block_index += 1;
                    }
                }
                Some("agent_message") => {
                    if let Some(turn) = current_turn.clone()
                        && let Some(text) = payload.get("message").and_then(read_string)
                    {
                        let phase = payload.get("phase").and_then(read_string);
                        if !has_duplicate_text(&messages, &turn, "assistant", &text, phase.as_deref()) {
                            let kind = if phase.as_deref() == Some("analysis") { "thinking" } else { "text" };
                            let mut m = AgentsUiMessage::new(
                                format!("assistant:{turn}:{block_index}"),
                                turn,
                                "assistant",
                                kind,
                                text,
                            );
                            m.phase = phase;
                            m.created_at = ts.clone();
                            push(&mut messages, m);
                            block_index += 1;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        if rec_type != Some("response_item") {
            continue;
        }
        let Some(turn) = current_turn.clone() else {
            continue;
        };
        let payload_type = payload.get("type").and_then(Value::as_str);

        match payload_type {
            Some("reasoning") => {
                let summary = read_reasoning_summary(payload.get("summary").unwrap_or(&Value::Null));
                if summary.is_empty() {
                    continue;
                }
                let mut m = AgentsUiMessage::new(
                    format!("reasoning:{turn}:{block_index}"),
                    turn,
                    "assistant",
                    "thinking",
                    summary,
                );
                m.phase = Some("analysis".to_string());
                m.created_at = ts.clone();
                push(&mut messages, m);
                block_index += 1;
            }
            Some("function_call") | Some("custom_tool_call") => {
                let Some(call_id) = payload.get("call_id").and_then(read_string) else {
                    continue;
                };
                let tool_name = payload.get("name").and_then(read_string).unwrap_or_else(|| "tool".to_string());
                let is_custom = payload_type == Some("custom_tool_call");
                let arguments_text = if is_custom {
                    payload.get("input").and_then(Value::as_str).map(str::to_string)
                        .or_else(|| Some(compact_json(payload.get("input").unwrap_or(&serde_json::json!({})))))
                } else {
                    payload.get("arguments").and_then(Value::as_str).map(str::to_string)
                        .or_else(|| Some(compact_json(payload.get("arguments").unwrap_or(&serde_json::json!({})))))
                };
                let command = read_tool_command(&tool_name, arguments_text.as_deref());
                let cwd = if is_custom { None } else { read_tool_cwd(arguments_text.as_deref()) };
                tool_meta.insert(call_id.clone(), (tool_name.clone(), command.clone(), cwd.clone()));

                let text = if is_custom {
                    tool_name.clone()
                } else {
                    build_tool_use_text(&tool_name, arguments_text.as_deref())
                };
                let mut m = AgentsUiMessage::new(call_id.clone(), turn, "assistant", "toolUse", text);
                m.tool_name = Some(tool_name);
                m.tool_call_id = Some(call_id);
                m.command = command;
                m.cwd = cwd;
                m.status = if payload.get("status").and_then(Value::as_str) == Some("failed") {
                    "failed".to_string()
                } else {
                    "completed".to_string()
                };
                m.created_at = ts.clone();
                push(&mut messages, m);
                block_index += 1;
            }
            Some("function_call_output") | Some("custom_tool_call_output") => {
                let Some(call_id) = payload.get("call_id").and_then(read_string) else {
                    continue;
                };
                let metadata = tool_meta.get(&call_id);
                let output = payload
                    .get("output")
                    .and_then(Value::as_str)
                    .map(|s| s.trim_end().to_string())
                    .unwrap_or_else(|| compact_json(payload.get("output").unwrap_or(&Value::String(String::new()))));
                let exit_code = read_output_exit_code(&output);
                let mut m = AgentsUiMessage::new(
                    format!("{call_id}:result"),
                    turn,
                    "user",
                    "toolResult",
                    truncate(&output),
                );
                if let Some((tool_name, command, cwd)) = metadata {
                    m.tool_name = Some(tool_name.clone());
                    m.command = command.clone();
                    m.cwd = cwd.clone();
                }
                m.tool_call_id = Some(call_id);
                m.status = read_output_status(&output);
                m.exit_code = exit_code;
                m.created_at = ts.clone();
                push(&mut messages, m);
                block_index += 1;
            }
            _ => {}
        }
    }

    finalize_tool_statuses(messages)
}

fn codex_sessions_root() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".codex").join("sessions"))
}

fn collect_rollouts(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollouts(&path, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with("rollout-")
            && name.ends_with(".jsonl")
        {
            out.push(path);
        }
    }
}

/// The first-line `session_meta` `{id, cwd}` of a rollout file, if present.
fn read_session_meta(path: &std::path::Path) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let first = content.lines().next()?;
    let parsed: Value = serde_json::from_str(first).ok()?;
    if parsed.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = parsed.get("payload")?;
    let id = payload.get("id").and_then(Value::as_str)?.to_string();
    let cwd = payload.get("cwd").and_then(Value::as_str)?.to_string();
    Some((id, cwd))
}

/// Codex session ids for `cwd`, newest first — matches legacy `listCodexSessionIds`.
pub fn list_session_ids(cwd: &str) -> Vec<String> {
    let Some(root) = codex_sessions_root() else {
        return Vec::new();
    };
    let mut rollouts = Vec::new();
    collect_rollouts(&root, &mut rollouts);
    let mut stamped: Vec<(std::time::SystemTime, String)> = rollouts
        .into_iter()
        .filter_map(|path| {
            let (id, session_cwd) = read_session_meta(&path)?;
            if session_cwd != cwd {
                return None;
            }
            let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
            Some((mtime, id))
        })
        .collect();
    stamped.sort_by(|a, b| b.0.cmp(&a.0));
    stamped.into_iter().map(|(_, id)| id).collect()
}

/// The most recent Codex session for `cwd`: returns (session_id, parsed messages).
pub fn latest_codex_session(cwd: &str) -> Option<(String, Vec<AgentsUiMessage>)> {
    let root = codex_sessions_root()?;
    let mut rollouts = Vec::new();
    collect_rollouts(&root, &mut rollouts);

    let mut best: Option<(std::time::SystemTime, String, PathBuf)> = None;
    for path in rollouts {
        let Some((id, session_cwd)) = read_session_meta(&path) else {
            continue;
        };
        if session_cwd != cwd {
            continue;
        }
        let Ok(mtime) = fs::metadata(&path).and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().map(|(t, _, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, id, path));
        }
    }

    let (_, id, path) = best?;
    let text = fs::read_to_string(&path).ok()?;
    Some((id, parse_codex_session_messages(&text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_turn_with_user_agent_and_tool() {
        let jsonl = [
            r#"{"type":"event_msg","timestamp":"t0","payload":{"type":"task_started","turn_id":"turn1"}}"#,
            r#"{"type":"event_msg","timestamp":"t1","payload":{"type":"user_message","message":"run tests"}}"#,
            r#"{"type":"response_item","timestamp":"t2","payload":{"type":"function_call","call_id":"c1","name":"exec_command","arguments":"{\"cmd\":\"cargo test\",\"workdir\":\"/wt\"}"}}"#,
            r#"{"type":"response_item","timestamp":"t3","payload":{"type":"function_call_output","call_id":"c1","output":"ok\nProcess exited with code 0"}}"#,
            r#"{"type":"event_msg","timestamp":"t4","payload":{"type":"agent_message","message":"done"}}"#,
        ]
        .join("\n");

        let msgs = parse_codex_session_messages(&jsonl);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].text, "run tests");
        assert_eq!(msgs[1].kind, "toolUse");
        assert_eq!(msgs[1].command.as_deref(), Some("cargo test"));
        assert_eq!(msgs[1].cwd.as_deref(), Some("/wt"));
        // toolUse status back-filled from its result (exit 0 → completed).
        assert_eq!(msgs[1].status, "completed");
        assert_eq!(msgs[2].kind, "toolResult");
        assert_eq!(msgs[2].exit_code, Some(0));
        assert_eq!(msgs[3].kind, "text");
        assert_eq!(msgs[3].text, "done");
        // order is contiguous.
        assert_eq!(msgs.iter().map(|m| m.order).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn analysis_phase_becomes_thinking() {
        let jsonl = [
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"t"}}"#,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"pondering","phase":"analysis"}}"#,
        ]
        .join("\n");
        let msgs = parse_codex_session_messages(&jsonl);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].kind, "thinking");
        assert_eq!(msgs[0].phase.as_deref(), Some("analysis"));
    }
}
