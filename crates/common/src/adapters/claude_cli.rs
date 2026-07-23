//! Claude Code conversation session reader — port of the read path of
//! `backend-legacy/src/adapters/claude-cli.ts`. Parses the JSONL session files
//! Claude writes under `~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl` into
//! structured conversation messages. `build_claude_session_from_text` is pure
//! (unit-tested). The live-streaming half of claude-cli is NOT ported here.
//!
//! UNVERIFIED-HERE: no `claude` in this environment to produce real session files;
//! covered only by fixtures.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const TOOL_PAYLOAD_TRUNCATE_LIMIT: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliMessage {
    pub id: String,
    pub turn_id: String,
    pub role: String, // "user" | "assistant"
    pub text: String,
    pub created_at: Option<String>,
    pub kind: String, // "text" | "toolUse" | "toolResult"
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeCliSession {
    pub session_id: String,
    pub cwd: String,
    pub path: String,
    pub git_branch: Option<String>,
    pub created_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub messages: Vec<ClaudeCliMessage>,
}

fn read_string(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

fn compact_json(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| v.to_string())
}

fn truncate(text: &str) -> String {
    let count = text.chars().count();
    if count <= TOOL_PAYLOAD_TRUNCATE_LIMIT {
        return text.to_string();
    }
    let head: String = text.chars().take(TOOL_PAYLOAD_TRUNCATE_LIMIT).collect();
    format!("{head}… (truncated, {} more chars)", count - TOOL_PAYLOAD_TRUNCATE_LIMIT)
}

fn extract_tool_result_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return truncate(s.trim());
    }
    let Some(arr) = content.as_array() else {
        return truncate(&compact_json(content));
    };
    let joined: String = arr
        .iter()
        .map(|entry| {
            if let Some(obj) = entry.as_object()
                && obj.get("type").and_then(Value::as_str) == Some("text")
                && let Some(t) = obj.get("text").and_then(Value::as_str)
            {
                return t.to_string();
            }
            compact_json(entry)
        })
        .collect();
    truncate(joined.trim())
}

/// The Claude project dir name for a cwd: every non-alphanumeric char → `-`.
pub fn encode_claude_project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn parse_session_records(text: &str) -> Vec<Value> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.starts_with('{'))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// Parse a Claude JSONL session into structured messages (port of
/// `buildClaudeSessionFromText`).
pub fn build_claude_session_from_text(path: &str, session_id: &str, text: &str) -> ClaudeCliSession {
    let records = parse_session_records(text);
    let mut messages: Vec<ClaudeCliMessage> = Vec::new();
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut created_at: Option<String> = None;
    let mut last_seen_at: Option<String> = None;
    let mut current_turn_id: Option<String> = None;
    let mut block_index_by_message: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for record in &records {
        let ts = record.get("timestamp").and_then(read_string);
        if cwd.is_none() {
            cwd = record.get("cwd").and_then(read_string);
        }
        if git_branch.is_none() {
            git_branch = record.get("gitBranch").and_then(read_string);
        }
        if created_at.is_none() {
            created_at = ts.clone();
        }
        if ts.is_some() {
            last_seen_at = ts.clone();
        }

        let rec_type = record.get("type").and_then(Value::as_str);
        let message = record.get("message");
        let uuid = record.get("uuid").and_then(Value::as_str);
        let role = message.and_then(|m| m.get("role")).and_then(Value::as_str);

        // Top-level user prompt: message.content is a non-empty string.
        if rec_type == Some("user")
            && role == Some("user")
            && let (Some(content), Some(uuid)) = (
                message.and_then(|m| m.get("content")).and_then(Value::as_str),
                uuid,
            )
            && !content.trim().is_empty()
        {
            current_turn_id = Some(uuid.to_string());
            messages.push(ClaudeCliMessage {
                id: uuid.to_string(),
                turn_id: uuid.to_string(),
                role: "user".to_string(),
                kind: "text".to_string(),
                text: content.trim().to_string(),
                created_at: ts.clone(),
                tool_name: None,
                tool_call_id: None,
            });
            continue;
        }

        let Some(turn_id) = current_turn_id.clone() else {
            continue;
        };

        // User tool_result records: message.content is an array.
        if rec_type == Some("user")
            && role == Some("user")
            && let Some(content) = message.and_then(|m| m.get("content")).and_then(Value::as_array)
        {
            for entry in content {
                if entry.get("type").and_then(Value::as_str) != Some("tool_result") {
                    continue;
                }
                let text = extract_tool_result_text(entry.get("content").unwrap_or(&Value::Null));
                if text.is_empty() {
                    continue;
                }
                let tool_call_id = entry.get("tool_use_id").and_then(read_string);
                messages.push(ClaudeCliMessage {
                    id: format!("tool_result:{}", tool_call_id.clone().unwrap_or_else(|| uuid.unwrap_or("").to_string())),
                    turn_id: turn_id.clone(),
                    role: "user".to_string(),
                    kind: "toolResult".to_string(),
                    text,
                    created_at: ts.clone(),
                    tool_name: None,
                    tool_call_id,
                });
            }
            continue;
        }

        // Assistant records: message.content is an array of blocks.
        if rec_type != Some("assistant") || role != Some("assistant") {
            continue;
        }
        let Some(content) = message.and_then(|m| m.get("content")).and_then(Value::as_array) else {
            continue;
        };
        let message_id = message
            .and_then(|m| m.get("id"))
            .and_then(read_string)
            .or_else(|| uuid.map(str::to_string))
            .unwrap_or_default();

        for block in content {
            let index = {
                let counter = block_index_by_message.entry(message_id.clone()).or_insert(0);
                let i = *counter;
                *counter += 1;
                i
            };
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }
                        messages.push(ClaudeCliMessage {
                            id: format!("{message_id}:{index}"),
                            turn_id: turn_id.clone(),
                            role: "assistant".to_string(),
                            kind: "text".to_string(),
                            text: text.to_string(),
                            created_at: ts.clone(),
                            tool_name: None,
                            tool_call_id: None,
                        });
                    }
                }
                Some("tool_use") => {
                    let tool_name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    let tool_call_id = block.get("id").and_then(read_string);
                    let text = truncate(&compact_json(block.get("input").unwrap_or(&serde_json::json!({}))));
                    messages.push(ClaudeCliMessage {
                        id: format!("{message_id}:{index}"),
                        turn_id: turn_id.clone(),
                        role: "assistant".to_string(),
                        kind: "toolUse".to_string(),
                        text,
                        created_at: ts.clone(),
                        tool_name: Some(tool_name),
                        tool_call_id,
                    });
                }
                _ => {}
            }
        }
    }

    ClaudeCliSession {
        session_id: session_id.to_string(),
        cwd: cwd.unwrap_or_default(),
        path: path.to_string(),
        git_branch,
        created_at,
        last_seen_at,
        messages,
    }
}

// --- Live stream-json parsing (`claude -p --output-format stream-json`) ---

/// A parsed content block from a stream line, before a stable id is stamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeStreamBlock {
    pub role: String,
    pub kind: String,
    pub text: String,
    pub created_at: Option<String>,
    pub message_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

/// The salient fields of a single `stream-json` line (port of `ParsedClaudeCliStreamLine`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedClaudeStreamLine {
    pub session_id: Option<String>,
    pub message_start: Option<String>,
    pub block_start: Option<i64>,
    /// (delta text, block index)
    pub assistant_delta: Option<(String, i64)>,
    pub blocks: Vec<ClaudeStreamBlock>,
    pub complete_session_id: Option<String>,
    pub error: Option<String>,
}

fn stream_blocks_from_assistant(raw: &Value) -> Vec<ClaudeStreamBlock> {
    let Some(message) = raw.get("message") else {
        return Vec::new();
    };
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return Vec::new();
    }
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    let message_id = message
        .get("id")
        .and_then(read_string)
        .or_else(|| raw.get("uuid").and_then(read_string));
    let created_at = raw.get("timestamp").and_then(read_string);

    content
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = block.get("text").and_then(Value::as_str)?.trim();
                if text.is_empty() {
                    return None;
                }
                Some(ClaudeStreamBlock {
                    role: "assistant".into(),
                    kind: "text".into(),
                    text: text.to_string(),
                    created_at: created_at.clone(),
                    message_id: message_id.clone(),
                    tool_name: None,
                    tool_call_id: None,
                })
            }
            Some("tool_use") => {
                let tool_name = block.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
                let tool_call_id = block.get("id").and_then(read_string);
                let text = truncate(&compact_json(block.get("input").unwrap_or(&serde_json::json!({}))));
                Some(ClaudeStreamBlock {
                    role: "assistant".into(),
                    kind: "toolUse".into(),
                    text,
                    created_at: created_at.clone(),
                    message_id: message_id.clone(),
                    tool_name: Some(tool_name),
                    tool_call_id,
                })
            }
            _ => None,
        })
        .collect()
}

fn stream_blocks_from_user(raw: &Value) -> Vec<ClaudeStreamBlock> {
    let Some(message) = raw.get("message") else {
        return Vec::new();
    };
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return Vec::new();
    }
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    let created_at = raw.get("timestamp").and_then(read_string);
    content
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                return None;
            }
            let text = extract_tool_result_text(block.get("content").unwrap_or(&Value::Null));
            if text.is_empty() {
                return None;
            }
            Some(ClaudeStreamBlock {
                role: "user".into(),
                kind: "toolResult".into(),
                text,
                created_at: created_at.clone(),
                message_id: None,
                tool_name: None,
                tool_call_id: block.get("tool_use_id").and_then(read_string),
            })
        })
        .collect()
}

/// Parse one `stream-json` line (port of `parseClaudeStreamLine`). Returns `None`
/// for unparseable lines.
pub fn parse_claude_stream_line(line: &str) -> Option<ParsedClaudeStreamLine> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if !parsed.is_object() {
        return None;
    }
    let mut out = ParsedClaudeStreamLine {
        session_id: parsed.get("session_id").and_then(read_string),
        ..Default::default()
    };

    match parsed.get("type").and_then(Value::as_str) {
        Some("stream_event") => {
            if let Some(event) = parsed.get("event") {
                match event.get("type").and_then(Value::as_str) {
                    Some("message_start") => {
                        out.message_start = event
                            .get("message")
                            .and_then(|m| m.get("id"))
                            .and_then(read_string);
                    }
                    Some("content_block_start") => {
                        out.block_start = event.get("index").and_then(Value::as_i64);
                    }
                    Some("content_block_delta") => {
                        if let Some(delta) = event.get("delta")
                            && delta.get("type").and_then(Value::as_str) == Some("text_delta")
                            && let (Some(text), Some(index)) = (
                                delta.get("text").and_then(read_string),
                                event.get("index").and_then(Value::as_i64),
                            )
                        {
                            out.assistant_delta = Some((text, index));
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("assistant") => out.blocks = stream_blocks_from_assistant(&parsed),
        Some("user") => out.blocks = stream_blocks_from_user(&parsed),
        Some("result") => {
            let is_error = parsed.get("is_error").and_then(Value::as_bool) == Some(true);
            if is_error {
                out.error = Some(
                    parsed.get("result").and_then(read_string).unwrap_or_else(|| "Claude returned an error".to_string()),
                );
            } else {
                out.complete_session_id = parsed.get("session_id").and_then(read_string);
            }
        }
        Some("error") => {
            out.error = Some(
                parsed.get("message").and_then(read_string).unwrap_or_else(|| "Claude returned an error".to_string()),
            );
        }
        _ => {}
    }
    Some(out)
}

fn claude_projects_root() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".claude").join("projects"))
}

/// `.jsonl` files in `dir`, most-recently-modified first.
fn list_jsonl_files_by_mtime(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((e.path(), mtime))
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.into_iter().map(|(p, _)| p).collect()
}

/// Claude session ids for `cwd`, newest first. Reads only the encoded project
/// dir (`~/.claude/projects/<encoded>/`) — matches legacy `listClaudeSessionIds`.
pub fn list_session_ids(cwd: &str) -> Vec<String> {
    let Some(root) = claude_projects_root() else {
        return Vec::new();
    };
    let dir = root.join(encode_claude_project_dir(cwd));
    list_jsonl_files_by_mtime(&dir)
        .into_iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect()
}

/// Read the most recent Claude session for `cwd`, or `None` if there is none.
pub fn latest_session(cwd: &str) -> Option<ClaudeCliSession> {
    let root = claude_projects_root()?;
    let primary = root.join(encode_claude_project_dir(cwd));
    let mut candidates = list_jsonl_files_by_mtime(&primary);
    if candidates.is_empty() {
        // Fall back to scanning all project dirs for the newest file.
        if let Ok(dirs) = fs::read_dir(&root) {
            let mut all: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
            for entry in dirs.flatten().filter(|e| e.path().is_dir()) {
                for f in list_jsonl_files_by_mtime(&entry.path()) {
                    if let Ok(m) = fs::metadata(&f).and_then(|m| m.modified()) {
                        all.push((f, m));
                    }
                }
            }
            all.sort_by(|a, b| b.1.cmp(&a.1));
            candidates = all.into_iter().map(|(p, _)| p).collect();
        }
    }
    let path = candidates.into_iter().next()?;
    let text = fs::read_to_string(&path).ok()?;
    let session_id = path.file_stem()?.to_string_lossy().to_string();
    Some(build_claude_session_from_text(&path.to_string_lossy(), &session_id, &text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_dir_replaces_non_alnum() {
        assert_eq!(encode_claude_project_dir("/home/u/my.proj"), "-home-u-my-proj");
    }

    #[test]
    fn parses_user_prompt_assistant_text_and_tool_use() {
        let jsonl = [
            r#"{"type":"user","uuid":"t1","timestamp":"2026-01-01T00:00:00Z","cwd":"/wt","gitBranch":"feature","message":{"role":"user","content":"do the thing"}}"#,
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-01-01T00:00:01Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"on it"},{"type":"tool_use","id":"tc1","name":"Read","input":{"path":"x"}}]}}"#,
            r#"{"type":"user","uuid":"u2","timestamp":"2026-01-01T00:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tc1","content":"file contents"}]}}"#,
        ]
        .join("\n");

        let session = build_claude_session_from_text("/p.jsonl", "sess", &jsonl);
        assert_eq!(session.cwd, "/wt");
        assert_eq!(session.git_branch.as_deref(), Some("feature"));
        assert_eq!(session.messages.len(), 4);

        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[0].text, "do the thing");
        assert_eq!(session.messages[0].turn_id, "t1");

        assert_eq!(session.messages[1].kind, "text");
        assert_eq!(session.messages[1].text, "on it");
        assert_eq!(session.messages[1].id, "m1:0");
        assert_eq!(session.messages[1].turn_id, "t1");

        assert_eq!(session.messages[2].kind, "toolUse");
        assert_eq!(session.messages[2].tool_name.as_deref(), Some("Read"));
        assert_eq!(session.messages[2].id, "m1:1");

        assert_eq!(session.messages[3].kind, "toolResult");
        assert_eq!(session.messages[3].tool_call_id.as_deref(), Some("tc1"));
        assert_eq!(session.messages[3].text, "file contents");
    }

    #[test]
    fn parses_real_claude_stream_json() {
        // Captured from a real `claude -p --verbose --output-format stream-json
        // --include-partial-messages` run on this machine.
        let fixture = include_str!("testdata/claude_stream.jsonl");
        let mut session_id: Option<String> = None;
        let mut message_id: Option<String> = None;
        let mut text_deltas = 0;
        let mut assistant_blocks = 0;
        let mut complete: Option<String> = None;

        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some(parsed) = parse_claude_stream_line(line) else {
                continue;
            };
            if let Some(sid) = parsed.session_id {
                session_id = Some(sid);
            }
            if let Some(mid) = parsed.message_start {
                message_id = Some(mid);
            }
            if parsed.assistant_delta.is_some() {
                text_deltas += 1;
            }
            assistant_blocks += parsed.blocks.iter().filter(|b| b.role == "assistant").count();
            if let Some(c) = parsed.complete_session_id {
                complete = Some(c);
            }
        }

        assert!(session_id.is_some(), "stream should carry a session id");
        assert!(message_id.is_some(), "message_start should set a message id");
        assert_eq!(text_deltas, 1, "one text_delta in the captured run (thinking_deltas ignored)");
        assert!(assistant_blocks > 0, "should finalize at least one assistant block");
        assert_eq!(complete, session_id, "result line resolves the same session id");
    }

    #[test]
    fn drops_content_before_first_user_prompt() {
        let jsonl = r#"{"type":"assistant","uuid":"a1","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"orphan"}]}}"#;
        let session = build_claude_session_from_text("/p", "s", jsonl);
        assert!(session.messages.is_empty());
    }
}
