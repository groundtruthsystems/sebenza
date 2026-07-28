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
    /// Can branch a new session off an existing one, keeping its history.
    /// Claude: `--resume <id> --fork-session`. Codex: `fork <id>`.
    pub fork: bool,
    /// Sebenza can choose the session id at launch, so it never has to discover it.
    /// Claude accepts `--session-id`; Codex assigns its own and must be polled for
    /// (`capture_new_session_id`).
    pub pinnable_session_id: bool,
    /// The agent's hook layer can *gate* a tool call — deny or allow it — rather than
    /// only observing after the fact. False for every current built-in: Claude's and
    /// Codex's hooks observe, and Codex's PermissionRequest only signals a lifecycle
    /// change. Reserved for an agent that can genuinely block.
    pub permission_interception: bool,
}

/// A built-in agent, i.e. one Sebenza knows how to launch, hook and read history for.
///
/// An enum rather than a `String` so every dispatch site is exhaustiveness-checked:
/// adding a variant turns each unhandled site into a compile error instead of a silent
/// fallthrough. That is the defect class that made Codex interrupt inoperable — see
/// `conversation_router`.
///
/// Third-party extensibility is *not* this type's job; that is
/// `AgentImplementation::Custom`, which stays data-driven via command templates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinAgentId {
    Claude,
    Codex,
    Opencode,
}

impl BuiltinAgentId {
    /// Every built-in agent, in the order they are offered.
    pub const ALL: &'static [BuiltinAgentId] =
        &[BuiltinAgentId::Claude, BuiltinAgentId::Codex, BuiltinAgentId::Opencode];

    /// The stable wire/config id. Part of the ts-rest contract and of user config
    /// (`workspace.defaultAgent`), so these strings must not change.
    pub fn as_str(self) -> &'static str {
        match self {
            BuiltinAgentId::Claude => "claude",
            BuiltinAgentId::Codex => "codex",
            BuiltinAgentId::Opencode => "opencode",
        }
    }

    /// Human-facing label shown in the agent picker.
    pub fn label(self) -> &'static str {
        match self {
            BuiltinAgentId::Claude => "Claude",
            BuiltinAgentId::Codex => "Codex",
            BuiltinAgentId::Opencode => "OpenCode",
        }
    }

    /// Parse a wire/config id. Case-sensitive; `None` for anything that is not a
    /// built-in (custom agent ids included).
    pub fn from_wire(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.as_str() == id)
    }
}

#[derive(Clone)]
pub enum AgentImplementation {
    /// A built-in agent Sebenza launches directly.
    Builtin(BuiltinAgentId),
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

/// Per-agent capabilities, from verified behaviour. See the design's comparison table
/// and `spec.md` → *Verified findings*.
fn builtin_capabilities(id: BuiltinAgentId) -> AgentCapabilities {
    match id {
        BuiltinAgentId::Claude => AgentCapabilities {
            terminal: true,
            in_app_chat: true,
            conversation_history: true,
            interrupt: true,
            resume: true,
            fork: true,
            // `--session-id` lets Sebenza choose the id up front.
            pinnable_session_id: true,
            permission_interception: false,
        },
        BuiltinAgentId::Codex => AgentCapabilities {
            terminal: true,
            in_app_chat: true,
            conversation_history: true,
            interrupt: true,
            resume: true,
            fork: true,
            // Codex assigns its own id; it must be discovered by polling.
            pinnable_session_id: false,
            permission_interception: false,
        },
        // opencode arrives incrementally: the terminal works as soon as the agent is
        // registered, while chat, history and lifecycle status depend on the generated
        // plugin and the export-based adapter landing later in this phase. Each flag is
        // flipped by the task that makes it true, so the UI never advertises something
        // that does not work yet.
        BuiltinAgentId::Opencode => AgentCapabilities {
            terminal: true,
            in_app_chat: false,
            conversation_history: false,
            interrupt: false,
            // `-c` / `--session <id>` resume works from launch.
            resume: true,
            // `--fork` plus session.parent_id lineage.
            fork: true,
            // No pin flag, but `session.created` (plugin event) and `run --format json`
            // both surface the id at creation, so no polling is needed either.
            pinnable_session_id: true,
            // `permission.ask` has a mutable status in the plugin types, but
            // phase-0-task-2 could not observe it firing. Stays false until the
            // phase-3 spike proves it drivable.
            permission_interception: false,
        },
    }
}

fn builtin(id: BuiltinAgentId) -> AgentDefinition {
    AgentDefinition {
        id: id.as_str().to_string(),
        label: id.label().to_string(),
        kind: "builtin",
        capabilities: builtin_capabilities(id),
        implementation: AgentImplementation::Builtin(id),
    }
}

fn builtin_definitions() -> Vec<AgentDefinition> {
    BuiltinAgentId::ALL.iter().copied().map(builtin).collect()
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
            fork: false,
            pinnable_session_id: false,
            permission_interception: false,
        },
        implementation: AgentImplementation::Custom(config.clone()),
    }
}

/// Built-in agents followed by custom agents (sorted by label, then id),
/// excluding any custom entry that shadows a built-in id.
pub fn list_agent_definitions(config: &ProjectConfig) -> Vec<AgentDefinition> {
    let builtin_ids: std::collections::HashSet<&str> =
        BuiltinAgentId::ALL.iter().map(|a| a.as_str()).collect();
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
    BuiltinAgentId::from_wire(agent_id).is_some()
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesWire {
    pub terminal: bool,
    pub in_app_chat: bool,
    pub conversation_history: bool,
    pub interrupt: bool,
    pub resume: bool,
    pub fork: bool,
    pub pinnable_session_id: bool,
    pub permission_interception: bool,
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
            fork: agent.capabilities.fork,
            pinnable_session_id: agent.capabilities.pinnable_session_id,
            permission_interception: agent.capabilities.permission_interception,
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

/// Config builders shared by tests in this crate.
#[cfg(test)]
pub mod tests_support {
    use crate::domain::config::*;
    use indexmap::IndexMap;
    use std::collections::HashMap;

    /// A ProjectConfig with no profiles, agents, services or hooks — the minimum that
    /// deserializes. Mutate the fields a test cares about.
    pub fn minimal_config() -> ProjectConfig {
        ProjectConfig {
            name: "p".to_string(),
            workspace: WorkspaceConfig {
                main_branch: "main".to_string(),
                worktree_root: "/wt".to_string(),
                default_agent: "claude".to_string(),
                auto_pull: AutoPullConfig { enabled: false, interval_seconds: 0 },
            },
            profiles: IndexMap::new(),
            agents: HashMap::new(),
            launchers: HashMap::new(),
            services: Vec::new(),
            startup_envs: HashMap::new(),
            integrations: IntegrationConfig {
                github: GitHubIntegrationConfig {
                    linked_repos: Vec::new(),
                    auto_remove_on_merge: false,
                },
            },
            lifecycle_hooks: LifecycleHooksConfig { post_create: None, pre_remove: None },
            auto_name: None,
            oneshot: OneshotConfig { system_prompt: String::new() },
        }
    }

    /// A custom agent definition; `resume` grants the resume capability.
    pub fn custom_agent(label: &str, resume: Option<&str>) -> CustomAgentConfig {
        CustomAgentConfig {
            label: label.to_string(),
            start_command: "run".to_string(),
            resume_command: resume.map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{custom_agent, minimal_config};
    use super::*;

    fn config_with(agents: &[(&str, crate::domain::config::CustomAgentConfig)]) -> ProjectConfig {
        let mut c = minimal_config();
        for (id, cfg) in agents {
            c.agents.insert(id.to_string(), cfg.clone());
        }
        c
    }

    fn custom(label: &str, resume: Option<&str>) -> crate::domain::config::CustomAgentConfig {
        custom_agent(label, resume)
    }

    /// The verified per-agent capability matrix. `fork` and `pinnable_session_id` differ
    /// between the two built-ins today; `permission_interception` is false for both
    /// because neither Claude's nor Codex's hooks can gate a tool call.
    #[test]
    fn builtin_capability_matrix_matches_verified_behaviour() {
        let config = config_with(&[]);
        let defs = list_agent_definitions(&config);

        let claude = defs.iter().find(|d| d.id == "claude").expect("claude is builtin");
        assert!(claude.capabilities.fork, "claude forks via --resume ID --fork-session");
        assert!(
            claude.capabilities.pinnable_session_id,
            "claude accepts --session-id, so Sebenza can pin the id at launch"
        );
        assert!(
            !claude.capabilities.permission_interception,
            "claude hooks observe; they cannot deny a tool call"
        );

        let codex = defs.iter().find(|d| d.id == "codex").expect("codex is builtin");
        assert!(codex.capabilities.fork, "codex forks via `fork <id>`");
        assert!(
            !codex.capabilities.pinnable_session_id,
            "codex assigns its own session id, so it must be discovered by polling"
        );
        assert!(
            !codex.capabilities.permission_interception,
            "codex's PermissionRequest hook only signals; it cannot deny"
        );

        let oc = defs.iter().find(|d| d.id == "opencode").expect("opencode is builtin");
        assert!(oc.capabilities.terminal, "the terminal works from registration");
        assert!(oc.capabilities.resume, "`-c` / `--session <id>` resume works from launch");
        assert!(oc.capabilities.fork, "`--fork` plus session.parent_id lineage");
        assert!(
            oc.capabilities.pinnable_session_id,
            "no pin flag, but session.created and `run --format json` both surface the id \
             at creation, so no polling is needed"
        );
        // These three are enabled by later tasks in this phase. They must stay false until
        // the code behind them exists, or the UI advertises features that do not work.
        assert!(
            !oc.capabilities.in_app_chat,
            "chat depends on the generated plugin + export adapter, not yet landed"
        );
        assert!(!oc.capabilities.conversation_history, "history adapter not yet landed");
        assert!(!oc.capabilities.interrupt, "interrupt needs the streaming provider");
        assert!(
            !oc.capabilities.permission_interception,
            "permission.ask was not observed firing (phase-0-task-2); gated on the phase-3 spike"
        );
    }

    /// Every new capability must be explicitly false for custom agents. The wire schema
    /// declares them non-optional booleans, so a missing field fails schema parse.
    #[test]
    fn custom_agents_declare_every_new_capability_as_false() {
        let config = config_with(&[("mine", custom("Mine", Some("resume")))]);
        let defs = list_agent_definitions(&config);
        let mine = defs.iter().find(|d| d.id == "mine").expect("custom agent listed");

        assert!(!mine.capabilities.fork);
        assert!(!mine.capabilities.pinnable_session_id);
        assert!(!mine.capabilities.permission_interception);
        // Unchanged pre-existing behaviour: resume is granted by a resume_command.
        assert!(mine.capabilities.resume);
        assert!(!mine.capabilities.in_app_chat);
    }

    /// The new fields must reach the frontend, not just exist in Rust: they are part of
    /// AgentCapabilitiesSchema in the ts-rest contract.
    #[test]
    fn new_capabilities_are_serialized_on_the_wire_in_camel_case() {
        let config = config_with(&[]);
        let details = get_agent_details(&config, "claude").expect("claude details");
        let json = serde_json::to_value(&details.capabilities).expect("serializes");

        for field in ["fork", "pinnableSessionId", "permissionInterception"] {
            assert!(
                json.get(field).is_some(),
                "wire payload must carry {field}; got {json}"
            );
        }
        assert_eq!(json.get("fork"), Some(&serde_json::json!(true)));
        assert_eq!(json.get("pinnableSessionId"), Some(&serde_json::json!(true)));
        assert_eq!(json.get("permissionInterception"), Some(&serde_json::json!(false)));
    }
}
