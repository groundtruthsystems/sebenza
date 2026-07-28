use serde_json::Value;

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    AgentStopped { worktree_id: String, branch: String },
    AgentStatusChanged { worktree_id: String, branch: String, lifecycle: String },
    PrOpened { worktree_id: String, branch: String, url: Option<String> },
    RuntimeError { worktree_id: String, branch: String, message: String },
    /// An agent reported the session id it just created. Only opencode uses this: its
    /// store is SQLite behind an internal schema, so the id cannot be recovered from disk
    /// the way claude's and codex's can.
    ConversationStarted { worktree_id: String, branch: String, session_id: String },
}

impl RuntimeEvent {
    pub fn worktree_id(&self) -> &str {
        match self {
            RuntimeEvent::AgentStopped { worktree_id, .. }
            | RuntimeEvent::AgentStatusChanged { worktree_id, .. }
            | RuntimeEvent::PrOpened { worktree_id, .. }
            | RuntimeEvent::RuntimeError { worktree_id, .. }
            | RuntimeEvent::ConversationStarted { worktree_id, .. } => worktree_id,
        }
    }

    pub fn branch(&self) -> &str {
        match self {
            RuntimeEvent::AgentStopped { branch, .. }
            | RuntimeEvent::AgentStatusChanged { branch, .. }
            | RuntimeEvent::PrOpened { branch, .. }
            | RuntimeEvent::RuntimeError { branch, .. }
            | RuntimeEvent::ConversationStarted { branch, .. } => branch,
        }
    }
}

fn non_empty(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Parse+validate a runtime event body. `None` if malformed.
pub fn parse_runtime_event(raw: &Value) -> Option<RuntimeEvent> {
    if !raw.is_object() {
        return None;
    }
    let worktree_id = non_empty(raw, "worktreeId")?;
    let branch = non_empty(raw, "branch")?;
    match raw.get("type").and_then(Value::as_str)? {
        "agent_stopped" => Some(RuntimeEvent::AgentStopped { worktree_id, branch }),
        "agent_status_changed" => {
            let lifecycle = raw.get("lifecycle").and_then(Value::as_str)?;
            if matches!(lifecycle, "starting" | "running" | "idle" | "stopped") {
                Some(RuntimeEvent::AgentStatusChanged {
                    worktree_id,
                    branch,
                    lifecycle: lifecycle.to_string(),
                })
            } else {
                None
            }
        }
        "pr_opened" => Some(RuntimeEvent::PrOpened {
            worktree_id,
            branch,
            url: raw.get("url").and_then(Value::as_str).map(str::to_string),
        }),
        // opencode's plugin reports the session id at creation. Unlike claude/codex, whose
        // ids are discoverable from on-disk logs, this is the ONLY way Sebenza learns it.
        "conversation_started" => {
            let session_id = non_empty(raw, "sessionId")?;
            Some(RuntimeEvent::ConversationStarted { worktree_id, branch, session_id })
        }
        "runtime_error" => {
            let message = non_empty(raw, "message")?;
            Some(RuntimeEvent::RuntimeError { worktree_id, branch, message })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conversation_started_requires_a_session_id() {
        let ok = json!({
            "worktreeId": "wt1", "branch": "feature",
            "type": "conversation_started", "sessionId": "ses_abc",
        });
        match parse_runtime_event(&ok) {
            Some(RuntimeEvent::ConversationStarted { session_id, branch, .. }) => {
                assert_eq!(session_id, "ses_abc");
                assert_eq!(branch, "feature");
            }
            other => panic!("expected ConversationStarted, got {other:?}"),
        }

        // A missing or blank id is rejected rather than recorded as an empty session,
        // which would make the conversation service export nothing forever.
        for bad in [
            json!({"worktreeId":"wt1","branch":"f","type":"conversation_started"}),
            json!({"worktreeId":"wt1","branch":"f","type":"conversation_started","sessionId":""}),
        ] {
            assert!(parse_runtime_event(&bad).is_none(), "must reject {bad}");
        }
    }

    #[test]
    fn unknown_event_types_are_still_rejected() {
        let raw = json!({"worktreeId":"wt1","branch":"f","type":"not_a_real_event"});
        assert!(parse_runtime_event(&raw).is_none());
    }
}
