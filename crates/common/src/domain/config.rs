use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CustomAgentConfig {
    pub label: String,
    pub start_command: String,
    pub resume_command: Option<String>,
}

/// An external launcher (editor/tool) opened against a worktree directory via
/// the "Open in…" menu. `command` is a shell string with `${WORKTREE_PATH}`,
/// `${REPO_PATH}`, `${BRANCH}` template vars.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LauncherConfig {
    pub label: String,
    pub command: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PaneKind {
    Agent,
    Shell,
    Command,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PaneSplit {
    Right,
    Bottom,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PaneCwd {
    Worktree,
    Repo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PaneTemplate {
    pub id: String,
    pub kind: PaneKind,
    pub split: Option<PaneSplit>,
    #[serde(rename = "sizePct")]
    pub size_pct: Option<i32>,
    pub focus: Option<bool>,
    pub command: Option<String>,
    pub cwd: Option<PaneCwd>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MountSpec {
    pub host_path: String,
    pub guest_path: Option<String>,
    pub writable: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKind {
    Host,
    Docker,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConfig {
    pub runtime: RuntimeKind,
    pub system_prompt: Option<String>,
    pub env_passthrough: Vec<String>,
    pub yolo: Option<bool>,
    pub panes: Vec<PaneTemplate>,
    pub image: Option<String>,
    pub mounts: Option<Vec<MountSpec>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    pub name: String,
    pub port_env: String,
    pub port_start: Option<u16>,
    pub port_step: Option<u16>,
    pub url_template: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkedRepoConfig {
    pub repo: String,
    pub alias: String,
    pub dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIntegrationConfig {
    pub linked_repos: Vec<LinkedRepoConfig>,
    pub auto_remove_on_merge: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationConfig {
    pub github: GitHubIntegrationConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleHooksConfig {
    pub post_create: Option<String>,
    pub pre_remove: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AutoNameProvider {
    Claude,
    Codex,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoNameConfig {
    pub provider: AutoNameProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OneshotConfig {
    pub system_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoPullConfig {
    pub enabled: bool,
    pub interval_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    pub main_branch: String,
    pub worktree_root: String,
    pub default_agent: String, // "claude" | "codex"
    pub auto_pull: AutoPullConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: String,
    pub workspace: WorkspaceConfig,
    // IndexMap preserves YAML insertion order so `getDefaultProfileName` (first
    // key when no `default` profile) and the config profile list match the TS backend.
    pub profiles: IndexMap<String, ProfileConfig>,
    pub agents: HashMap<String, CustomAgentConfig>,
    pub launchers: HashMap<String, LauncherConfig>,
    pub services: Vec<ServiceSpec>,
    // Webmux supports boolean or string environment values. We deserialize into String to uniformize.
    pub startup_envs: HashMap<String, String>,
    pub integrations: IntegrationConfig,
    pub lifecycle_hooks: LifecycleHooksConfig,
    pub auto_name: Option<AutoNameConfig>,
    pub oneshot: OneshotConfig,
}
