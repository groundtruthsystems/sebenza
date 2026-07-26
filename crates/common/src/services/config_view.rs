use crate::domain::config::ProjectConfig;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub terminal: bool,
    pub in_app_chat: bool,
    pub conversation_history: bool,
    pub interrupt: bool,
    pub resume: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub id: String,
    pub label: String,
    pub kind: String, // "builtin" | "custom"
    pub capabilities: AgentCapabilities,
}

fn builtin_agent_summaries() -> Vec<AgentSummary> {
    let full = || AgentCapabilities {
        terminal: true,
        in_app_chat: true,
        conversation_history: true,
        interrupt: true,
        resume: true,
    };
    vec![
        AgentSummary {
            id: "claude".to_string(),
            label: "Claude".to_string(),
            kind: "builtin".to_string(),
            capabilities: full(),
        },
        AgentSummary {
            id: "codex".to_string(),
            label: "Codex".to_string(),
            kind: "builtin".to_string(),
            capabilities: full(),
        },
    ]
}

/// Builtin + custom agent summaries. Custom agents (from `config.agents`, minus
/// builtin ids) are sorted by label then id, matching `listAgentDefinitions`.
pub fn list_agent_summaries(config: &ProjectConfig) -> Vec<AgentSummary> {
    let builtins = builtin_agent_summaries();
    let builtin_ids: Vec<&str> = builtins.iter().map(|a| a.id.as_str()).collect();

    let mut customs: Vec<AgentSummary> = config
        .agents
        .iter()
        .filter(|(id, _)| !builtin_ids.contains(&id.as_str()))
        .map(|(id, cfg)| AgentSummary {
            id: id.clone(),
            label: cfg.label.clone(),
            kind: "custom".to_string(),
            capabilities: AgentCapabilities {
                terminal: true,
                in_app_chat: false,
                conversation_history: false,
                interrupt: false,
                resume: cfg.resume_command.is_some(),
            },
        })
        .collect();
    customs.sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));

    let mut all = builtins;
    all.extend(customs);
    all
}

/// Label for an agent id, or the id itself if unknown; `None` when id is `None`.
/// Mirrors `findAgentLabel` (`getAgentDefinition(config, id)?.label ?? id`).
pub fn agent_label(config: &ProjectConfig, agent_id: Option<&str>) -> Option<String> {
    let id = agent_id?;
    let label = list_agent_summaries(config)
        .into_iter()
        .find(|a| a.id == id)
        .map(|a| a.label)
        .unwrap_or_else(|| id.to_string());
    Some(label)
}

pub fn get_default_profile_name(config: &ProjectConfig) -> String {
    if config.profiles.contains_key("default") {
        return "default".to_string();
    }
    config
        .profiles
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceView {
    pub name: String,
    pub port_env: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedRepoView {
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherView {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub name: String,
    pub services: Vec<ServiceView>,
    pub profiles: Vec<ProfileView>,
    pub agents: Vec<AgentSummary>,
    pub launchers: Vec<LauncherView>,
    pub default_profile_name: String,
    pub default_agent_id: String,
    pub auto_name: bool,
    pub startup_envs: HashMap<String, String>,
    pub linked_repos: Vec<LinkedRepoView>,
    pub auto_remove_on_merge: bool,
    pub project_dir: String,
    pub main_branch: String,
}

/// Project `/api/config` payload. Mirrors `getFrontendConfig`.
pub fn build_app_config(config: &ProjectConfig, project_dir: &str) -> AppConfig {
    let default_profile_name = get_default_profile_name(config);

    // Default profile first; stable order for the rest (IndexMap preserves YAML order).
    let mut profile_entries: Vec<(&String, _)> = config.profiles.iter().collect();
    profile_entries.sort_by(|(left, _), (right, _)| {
        use std::cmp::Ordering;
        if **left == default_profile_name {
            Ordering::Less
        } else if **right == default_profile_name {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });

    AppConfig {
        name: config.name.clone(),
        services: config
            .services
            .iter()
            .map(|s| ServiceView {
                name: s.name.clone(),
                port_env: s.port_env.clone(),
            })
            .collect(),
        profiles: profile_entries
            .into_iter()
            .map(|(name, profile)| ProfileView {
                name: name.clone(),
                system_prompt: profile.system_prompt.clone(),
            })
            .collect(),
        agents: list_agent_summaries(config),
        launchers: {
            let mut launchers: Vec<LauncherView> = config
                .launchers
                .iter()
                .map(|(id, l)| LauncherView {
                    id: id.clone(),
                    label: l.label.clone(),
                })
                .collect();
            launchers.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
            launchers
        },
        default_profile_name,
        default_agent_id: config.workspace.default_agent.clone(),
        auto_name: config.auto_name.is_some(),
        startup_envs: config.startup_envs.clone(),
        linked_repos: config
            .integrations
            .github
            .linked_repos
            .iter()
            .map(|lr| LinkedRepoView {
                alias: lr.alias.clone(),
                dir: lr.dir.as_ref().map(|d| {
                    Path::new(project_dir)
                        .join(d)
                        .to_string_lossy()
                        .to_string()
                }),
            })
            .collect(),
        auto_remove_on_merge: config.integrations.github.auto_remove_on_merge,
        project_dir: project_dir.to_string(),
        main_branch: config.workspace.main_branch.clone(),
    }
}
