//! Agent registry.
//! Resolves built-in (claude/codex) and custom agent definitions used when
//! building a worktree's agent launch command.

use crate::domain::config::{CustomAgentConfig, ProjectConfig};
use serde::Serialize;

#[derive(Clone)]
pub struct AgentCapabilities {
    pub terminal: bool,
    pub in_app_chat: bool,
    pub conversation_history: bool,
    pub interrupt: bool,
    pub resume: bool,
}

#[derive(Clone)]
pub enum AgentImplementation {
    /// Built-in agent binary: `"claude"` or `"codex"`.
    Builtin(String),
    Custom(CustomAgentConfig),
}

#[derive(Clone)]
pub struct AgentDefinition {
    pub id: String,
    pub label: String,
    /// `"builtin"` | `"custom"`.
    pub kind: &'static str,
    pub capabilities: AgentCapabilities,
    pub implementation: AgentImplementation,
}

fn builtin(id: &str, label: &str) -> AgentDefinition {
    AgentDefinition {
        id: id.to_string(),
        label: label.to_string(),
        kind: "builtin",
        capabilities: AgentCapabilities {
            terminal: true,
            in_app_chat: true,
            conversation_history: true,
            interrupt: true,
            resume: true,
        },
        implementation: AgentImplementation::Builtin(id.to_string()),
    }
}

fn builtin_definitions() -> Vec<AgentDefinition> {
    vec![builtin("claude", "Claude"), builtin("codex", "Codex")]
}

fn build_custom_definition(id: &str, config: &CustomAgentConfig) -> AgentDefinition {
    AgentDefinition {
        id: id.to_string(),
        label: config.label.clone(),
        kind: "custom",
        capabilities: AgentCapabilities {
            terminal: true,
            in_app_chat: false,
            conversation_history: false,
            interrupt: false,
            resume: config.resume_command.is_some(),
        },
        implementation: AgentImplementation::Custom(config.clone()),
    }
}

/// Built-in agents followed by custom agents (sorted by label, then id),
/// excluding any custom entry that shadows a built-in id.
pub fn list_agent_definitions(config: &ProjectConfig) -> Vec<AgentDefinition> {
    let builtin_ids: std::collections::HashSet<&str> = ["claude", "codex"].into_iter().collect();
    let mut custom: Vec<(&String, &CustomAgentConfig)> = config
        .agents
        .iter()
        .filter(|(id, _)| !builtin_ids.contains(id.as_str()))
        .collect();
    custom.sort_by(|(l_id, l), (r_id, r)| l.label.cmp(&r.label).then_with(|| l_id.cmp(r_id)));

    let mut defs = builtin_definitions();
    defs.extend(custom.into_iter().map(|(id, cfg)| build_custom_definition(id, cfg)));
    defs
}

pub fn get_agent_definition(config: &ProjectConfig, agent_id: &str) -> Option<AgentDefinition> {
    list_agent_definitions(config)
        .into_iter()
        .find(|agent| agent.id == agent_id)
}

pub fn is_builtin_agent_id(agent_id: &str) -> bool {
    agent_id == "claude" || agent_id == "codex"
}

/// Slug a label into a custom agent id (`[^a-z0-9]+` → `-`, trimmed), or `agent`.
pub fn normalize_custom_agent_id(label: &str) -> String {
    let lowered = label.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "agent".to_string()
    } else {
        trimmed
    }
}

// --- Wire types ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesWire {
    pub terminal: bool,
    pub in_app_chat: bool,
    pub conversation_history: bool,
    pub interrupt: bool,
    pub resume: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetailsWire {
    pub id: String,
    pub label: String,
    pub kind: &'static str,
    pub capabilities: AgentCapabilitiesWire,
    // `.nullable()` in the contract → emit `null` (not omitted) for built-ins.
    pub start_command: Option<String>,
    pub resume_command: Option<String>,
}

fn to_details(agent: AgentDefinition) -> AgentDetailsWire {
    let (start_command, resume_command) = match &agent.implementation {
        AgentImplementation::Custom(config) => {
            (Some(config.start_command.clone()), config.resume_command.clone())
        }
        AgentImplementation::Builtin(_) => (None, None),
    };
    AgentDetailsWire {
        id: agent.id,
        label: agent.label,
        kind: agent.kind,
        capabilities: AgentCapabilitiesWire {
            terminal: agent.capabilities.terminal,
            in_app_chat: agent.capabilities.in_app_chat,
            conversation_history: agent.capabilities.conversation_history,
            interrupt: agent.capabilities.interrupt,
            resume: agent.capabilities.resume,
        },
        start_command,
        resume_command,
    }
}

pub fn list_agent_details(config: &ProjectConfig) -> Vec<AgentDetailsWire> {
    list_agent_definitions(config).into_iter().map(to_details).collect()
}

pub fn get_agent_details(config: &ProjectConfig, agent_id: &str) -> Option<AgentDetailsWire> {
    get_agent_definition(config, agent_id).map(to_details)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateCustomAgentResult {
    pub normalized_id: String,
    pub warnings: Vec<String>,
}

/// Warn about custom-agent commands that won't receive prompts / can't resume.
pub fn validate_custom_agent_input(
    label: &str,
    start_command: &str,
    resume_command: Option<&str>,
) -> ValidateCustomAgentResult {
    let mut warnings = Vec::new();
    if !start_command.contains("${PROMPT}") && !start_command.contains("${SYSTEM_PROMPT}") {
        warnings.push("Start command does not reference ${PROMPT} or ${SYSTEM_PROMPT}; initial prompts will not be passed automatically".to_string());
    }
    if resume_command.map(str::trim).unwrap_or("").is_empty() {
        warnings.push("Resume command is not configured; reopening the worktree will restart the agent".to_string());
    }
    ValidateCustomAgentResult {
        normalized_id: normalize_custom_agent_id(label),
        warnings,
    }
}
