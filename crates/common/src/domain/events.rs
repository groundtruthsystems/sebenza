//! Runtime events posted by agents to `/api/runtime/events` — port of
//! `backend-legacy/src/domain/events.ts`.

use serde_json::Value;

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    AgentStopped { worktree_id: String, branch: String },
    AgentStatusChanged { worktree_id: String, branch: String, lifecycle: String },
    PrOpened { worktree_id: String, branch: String, url: Option<String> },
    RuntimeError { worktree_id: String, branch: String, message: String },
}

impl RuntimeEvent {
    pub fn worktree_id(&self) -> &str {
        match self {
            RuntimeEvent::AgentStopped { worktree_id, .. }
            | RuntimeEvent::AgentStatusChanged { worktree_id, .. }
            | RuntimeEvent::PrOpened { worktree_id, .. }
            | RuntimeEvent::RuntimeError { worktree_id, .. } => worktree_id,
        }
    }

    pub fn branch(&self) -> &str {
        match self {
            RuntimeEvent::AgentStopped { branch, .. }
            | RuntimeEvent::AgentStatusChanged { branch, .. }
            | RuntimeEvent::PrOpened { branch, .. }
            | RuntimeEvent::RuntimeError { branch, .. } => branch,
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
        "runtime_error" => {
            let message = non_empty(raw, "message")?;
            Some(RuntimeEvent::RuntimeError { worktree_id, branch, message })
        }
        _ => None,
    }
}
