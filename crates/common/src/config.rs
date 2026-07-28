use crate::domain::config::{
    AutoNameConfig, AutoNameProvider, AutoPullConfig, CustomAgentConfig, GitHubIntegrationConfig,
    IntegrationConfig, LauncherConfig, LifecycleHooksConfig, LinkedRepoConfig, OneshotConfig,
    PaneKind, PaneSplit, PaneTemplate, ProfileConfig, ProjectConfig, RuntimeKind, ServiceSpec,
    WorkspaceConfig,
};
use anyhow::{Result, anyhow};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn default_oneshot_system_prompt() -> String {
    vec![
        "You are running in Sebenza ONESHOT mode. There is NO interactive user — nobody is watching the chat or will respond to questions, approvals, or status checks. Any message asking the user to review, approve, confirm, take a look, or 'let you know' is wasted output: it will not be answered.",
        "Your job is to take the task to its real conclusion without pausing:",
        "1) Make the change. 2) Validate it (run the relevant tests, typecheck, build, or quick manual check). 3) Commit. 4) Push. 5) Open a pull request. Only then are you done.",
        "When something is ambiguous, pick the most reasonable default and proceed. When you would normally ask 'should I X or Y?', just pick one and continue — note the choice in the PR description if it matters.",
        "Never end your turn with a question, a suggestion to 'take a look', or a request for approval. Stop only when the PR is open, or when you hit a technical error you cannot recover from yourself (in which case clearly state the blocker).",
    ].join(" ")
}

pub fn default_config() -> ProjectConfig {
    let mut profiles = IndexMap::new();
    profiles.insert(
        "default".to_string(),
        ProfileConfig {
            runtime: RuntimeKind::Host,
            system_prompt: None,
            env_passthrough: vec![],
            yolo: None,
            panes: vec![
                PaneTemplate {
                    id: "agent".to_string(),
                    kind: PaneKind::Agent,
                    split: None,
                    size_pct: None,
                    focus: Some(true),
                    command: None,
                    cwd: None,
                    working_dir: None,
                },
                PaneTemplate {
                    id: "shell".to_string(),
                    kind: PaneKind::Shell,
                    split: Some(PaneSplit::Right),
                    size_pct: Some(25),
                    focus: None,
                    command: None,
                    cwd: None,
                    working_dir: None,
                },
            ],
            image: None,
            mounts: None,
        },
    );

    ProjectConfig {
        name: "Webmux".to_string(),
        workspace: WorkspaceConfig {
            main_branch: "main".to_string(),
            worktree_root: "../worktrees".to_string(),
            default_agent: "claude".to_string(),
            auto_pull: AutoPullConfig {
                enabled: false,
                interval_seconds: 300,
            },
        },
        profiles,
        agents: HashMap::new(),
        launchers: HashMap::new(),
        services: vec![],
        startup_envs: HashMap::new(),
        integrations: IntegrationConfig {
            github: GitHubIntegrationConfig {
                linked_repos: vec![],
                auto_remove_on_merge: false,
            },
        },
        lifecycle_hooks: LifecycleHooksConfig {
            post_create: None,
            pre_remove: None,
        },
        auto_name: None,
        oneshot: OneshotConfig {
            system_prompt: default_oneshot_system_prompt(),
        },
    }
}

/// Resolve the git root directory using `git rev-parse --show-toplevel`
pub fn git_root(dir: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if root.is_empty() {
                dir.to_string()
            } else {
                root
            }
        }
        _ => dir.to_string(),
    }
}

/// Resolve the project root for a directory (follows git-common-dir if in linked worktree)
pub fn project_root(dir: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(dir)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let common_dir_raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !common_dir_raw.is_empty() {
                let path = Path::new(dir).join(common_dir_raw);
                if let Some(parent) = path.parent() {
                    if let Ok(canon) = fs::canonicalize(parent) {
                        return canon.to_string_lossy().to_string();
                    }
                }
            }
            git_root(dir)
        }
        _ => git_root(dir),
    }
}

/// Expand ${VAR} placeholders in a template string using an env map.
pub fn expand_template(template: &str, env: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut remaining = template;

    while let Some(start_idx) = remaining.find("${") {
        result.push_str(&remaining[..start_idx]);
        let search_range = &remaining[start_idx + 2..];
        if let Some(end_idx) = search_range.find('}') {
            let var_name = &search_range[..end_idx];
            if let Some(value) = env.get(var_name) {
                result.push_str(value);
            }
            remaining = &search_range[end_idx + 1..];
        } else {
            result.push_str("${");
            remaining = search_range;
        }
    }
    result.push_str(remaining);
    result
}

fn merge_hook_command(project_cmd: Option<&str>, local_cmd: Option<&str>) -> Option<String> {
    match (project_cmd, local_cmd) {
        (Some(p), Some(l)) => Some(format!("set -e\n{}\n{}", p, l)),
        (Some(p), None) => Some(p.to_string()),
        (None, Some(l)) => Some(l.to_string()),
        (None, None) => None,
    }
}

/// Parse a yaml value into a ProjectConfig, filling in defaults where missing
fn parse_project_config(val: serde_yaml::Value) -> ProjectConfig {
    let mut config = default_config();

    if let Some(mapping) = val.as_mapping() {
        if let Some(name) = mapping.get("name").and_then(|v| v.as_str()) {
            config.name = name.to_string();
        }

        if let Some(workspace) = mapping.get("workspace").and_then(|v| v.as_mapping()) {
            if let Some(main_branch) = workspace.get("mainBranch").and_then(|v| v.as_str()) {
                config.workspace.main_branch = main_branch.to_string();
            }
            if let Some(worktree_root) = workspace.get("worktreeRoot").and_then(|v| v.as_str()) {
                config.workspace.worktree_root = worktree_root.to_string();
            }
            if let Some(default_agent) = workspace.get("defaultAgent").and_then(|v| v.as_str()) {
                config.workspace.default_agent = default_agent.to_string();
            }
            if let Some(auto_pull) = workspace.get("autoPull").and_then(|v| v.as_mapping()) {
                if let Some(enabled) = auto_pull.get("enabled").and_then(|v| v.as_bool()) {
                    config.workspace.auto_pull.enabled = enabled;
                }
                if let Some(interval) = auto_pull.get("intervalSeconds").and_then(|v| v.as_u64()) {
                    if interval >= 30 {
                        config.workspace.auto_pull.interval_seconds = interval;
                    }
                }
            }
        }

        // Parse profiles
        if let Some(profiles_val) = mapping.get("profiles") {
            if let Ok(p) =
                serde_yaml::from_value::<IndexMap<String, ProfileConfig>>(profiles_val.clone())
            {
                config.profiles = p;
            }
        }

        // Parse custom agents
        if let Some(agents_val) = mapping.get("agents") {
            if let Ok(a) =
                serde_yaml::from_value::<HashMap<String, CustomAgentConfig>>(agents_val.clone())
            {
                config.agents = a;
            }
        }

        // Parse launchers (external editors/tools for "Open in…")
        if let Some(launchers_val) = mapping.get("launchers") {
            if let Ok(l) =
                serde_yaml::from_value::<HashMap<String, LauncherConfig>>(launchers_val.clone())
            {
                config.launchers = l;
            }
        }

        // Parse services
        if let Some(services_val) = mapping.get("services") {
            if let Ok(s) = serde_yaml::from_value::<Vec<ServiceSpec>>(services_val.clone()) {
                config.services = s;
            }
        }

        // Parse startup envs
        if let Some(envs_val) = mapping.get("startupEnvs").and_then(|v| v.as_mapping()) {
            for (k, v) in envs_val {
                if let Some(k_str) = k.as_str() {
                    let v_str = match v {
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::String(s) => s.clone(),
                        _ => String::new(),
                    };
                    config.startup_envs.insert(k_str.to_string(), v_str);
                }
            }
        }

        // Parse integrations
        if let Some(integrations) = mapping.get("integrations").and_then(|v| v.as_mapping()) {
            if let Some(github) = integrations.get("github") {
                if let Some(gh_map) = github.as_mapping() {
                    if let Some(repos_val) = gh_map.get("linkedRepos") {
                        if let Ok(repos) =
                            serde_yaml::from_value::<Vec<LinkedRepoConfig>>(repos_val.clone())
                        {
                            config.integrations.github.linked_repos = repos;
                        }
                    }
                    if let Some(auto_remove) =
                        gh_map.get("autoRemoveOnMerge").and_then(|v| v.as_bool())
                    {
                        config.integrations.github.auto_remove_on_merge = auto_remove;
                    }
                }
            }
        }

        // Parse lifecycle hooks
        if let Some(hooks) = mapping.get("lifecycleHooks").and_then(|v| v.as_mapping()) {
            if let Some(post_create) = hooks.get("postCreate").and_then(|v| v.as_str()) {
                config.lifecycle_hooks.post_create = Some(post_create.to_string());
            }
            if let Some(pre_remove) = hooks.get("preRemove").and_then(|v| v.as_str()) {
                config.lifecycle_hooks.pre_remove = Some(pre_remove.to_string());
            }
        }

        // Parse auto name. The config uses the snake_case key `auto_name`
        // and the snake_case `system_prompt` subkey.
        if let Some(auto_name_map) = mapping.get("auto_name").and_then(|v| v.as_mapping()) {
            let provider = match auto_name_map.get("provider").and_then(|v| v.as_str()) {
                Some("claude") => Some(AutoNameProvider::Claude),
                Some("codex") => Some(AutoNameProvider::Codex),
                Some("opencode") => Some(AutoNameProvider::Opencode),
                _ => None,
            };
            if let Some(provider) = provider {
                let trimmed_string = |key: &str| {
                    auto_name_map
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                };
                config.auto_name = Some(AutoNameConfig {
                    provider,
                    model: trimmed_string("model"),
                    system_prompt: trimmed_string("system_prompt"),
                });
            }
        }

        // Parse oneshot
        if let Some(oneshot) = mapping.get("oneshot").and_then(|v| v.as_mapping()) {
            if let Some(sys_prompt) = oneshot.get("systemPrompt").and_then(|v| v.as_str()) {
                config.oneshot.system_prompt = sys_prompt.to_string();
            }
        }
    }

    config
}

/// Load configuration by reading and merging `.ai/sebenza.yaml` and `.ai/sebenza.local.yaml`
pub fn load_config(dir: &str) -> ProjectConfig {
    let root = project_root(dir);
    let config_path = Path::new(&root).join(".ai").join("sebenza.yaml");
    let local_path = Path::new(&root).join(".ai").join("sebenza.local.yaml");

    let mut project_val = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    if config_path.exists() {
        if let Ok(content) = fs::read_to_string(config_path) {
            if let Ok(val) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                project_val = val;
            }
        }
    }

    let mut config = parse_project_config(project_val);

    // Apply local overlay if present
    if local_path.exists() {
        if let Ok(content) = fs::read_to_string(local_path) {
            if let Ok(local_val) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                if let Some(local_map) = local_val.as_mapping() {
                    // Local workspace worktreeRoot overlay
                    if let Some(workspace) = local_map.get("workspace").and_then(|v| v.as_mapping())
                    {
                        if let Some(worktree_root) =
                            workspace.get("worktreeRoot").and_then(|v| v.as_str())
                        {
                            config.workspace.worktree_root = worktree_root.to_string();
                        }
                        if let Some(auto_pull) =
                            workspace.get("autoPull").and_then(|v| v.as_mapping())
                        {
                            if let Some(enabled) =
                                auto_pull.get("enabled").and_then(|v| v.as_bool())
                            {
                                config.workspace.auto_pull.enabled = enabled;
                            }
                            if let Some(interval) =
                                auto_pull.get("intervalSeconds").and_then(|v| v.as_u64())
                            {
                                if interval >= 30 {
                                    config.workspace.auto_pull.interval_seconds = interval;
                                }
                            }
                        }
                    }

                    // Local profiles overlay (add or replace)
                    if let Some(profiles_val) = local_map.get("profiles") {
                        if let Ok(local_profiles) = serde_yaml::from_value::<
                            IndexMap<String, ProfileConfig>,
                        >(profiles_val.clone())
                        {
                            for (name, prof) in local_profiles {
                                config.profiles.insert(name, prof);
                            }
                        }
                    }

                    // Local custom agents overlay
                    if let Some(agents_val) = local_map.get("agents") {
                        if let Ok(local_agents) = serde_yaml::from_value::<
                            HashMap<String, CustomAgentConfig>,
                        >(agents_val.clone())
                        {
                            for (id, agent) in local_agents {
                                config.agents.insert(id, agent);
                            }
                        }
                    }

                    // Local launchers overlay
                    if let Some(launchers_val) = local_map.get("launchers") {
                        if let Ok(local_launchers) = serde_yaml::from_value::<
                            HashMap<String, LauncherConfig>,
                        >(launchers_val.clone())
                        {
                            for (id, launcher) in local_launchers {
                                config.launchers.insert(id, launcher);
                            }
                        }
                    }

                    // Local integrations overlay
                    if let Some(integrations) =
                        local_map.get("integrations").and_then(|v| v.as_mapping())
                    {
                        if let Some(github) =
                            integrations.get("github").and_then(|v| v.as_mapping())
                        {
                            if let Some(auto_remove) =
                                github.get("autoRemoveOnMerge").and_then(|v| v.as_bool())
                            {
                                config.integrations.github.auto_remove_on_merge = auto_remove;
                            }
                        }
                    }

                    // Local lifecycle hooks overlay
                    if let Some(hooks) =
                        local_map.get("lifecycleHooks").and_then(|v| v.as_mapping())
                    {
                        let local_post = hooks.get("postCreate").and_then(|v| v.as_str());
                        let local_pre = hooks.get("preRemove").and_then(|v| v.as_str());

                        config.lifecycle_hooks.post_create = merge_hook_command(
                            config.lifecycle_hooks.post_create.as_deref(),
                            local_post,
                        );
                        config.lifecycle_hooks.pre_remove = merge_hook_command(
                            config.lifecycle_hooks.pre_remove.as_deref(),
                            local_pre,
                        );
                    }
                }
            }
        }
    }

    // Global launchers (~/.ai/sebenza.yaml) apply to every project; a project
    // or local `launchers` entry with the same id overrides the global one.
    for (id, launcher) in global_launchers() {
        config.launchers.entry(id).or_insert(launcher);
    }

    config
}

/// Machine-wide launchers from `~/.ai/sebenza.yaml` (`launchers:` map), shown
/// in every project's "Open in…" menu. Editor choice is a per-user preference,
/// not project-specific. Best-effort: missing/malformed file → none.
fn global_launchers() -> HashMap<String, LauncherConfig> {
    let Ok(home) = std::env::var("HOME") else {
        return HashMap::new();
    };
    let path = Path::new(&home).join(".ai").join("sebenza.yaml");
    let Ok(content) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    serde_yaml::from_str::<serde_yaml::Value>(&content)
        .ok()
        .and_then(|val| {
            val.as_mapping()
                .and_then(|m| m.get("launchers"))
                .and_then(|v| {
                    serde_yaml::from_value::<HashMap<String, LauncherConfig>>(v.clone()).ok()
                })
        })
        .unwrap_or_default()
}

fn read_local_config_document(root: &str) -> (PathBuf, serde_yaml::Value) {
    let local_path = Path::new(root).join(".ai").join("sebenza.local.yaml");
    let mut doc = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());

    if local_path.exists() {
        if let Ok(content) = fs::read_to_string(&local_path) {
            if let Ok(parsed) = serde_yaml::from_str(&content) {
                doc = parsed;
            }
        }
    }

    (local_path, doc)
}

/// Persist local config document to file
fn write_local_config_document(path: &Path, doc: &serde_yaml::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(doc)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn persist_local_github_config(dir: &str, auto_remove: Option<bool>) -> Result<()> {
    let root = project_root(dir);
    let (local_path, mut doc) = read_local_config_document(&root);

    let doc_map = doc
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("Invalid YAML format"))?;

    let integrations = doc_map
        .entry(serde_yaml::Value::String("integrations".to_string()))
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("Integrations is not a mapping"))?;

    let github = integrations
        .entry(serde_yaml::Value::String("github".to_string()))
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("Github is not a mapping"))?;

    if let Some(val) = auto_remove {
        github.insert(
            serde_yaml::Value::String("autoRemoveOnMerge".to_string()),
            serde_yaml::Value::Bool(val),
        );
    }

    write_local_config_document(&local_path, &doc)?;
    Ok(())
}

pub fn persist_local_custom_agent(
    dir: &str,
    agent_id: &str,
    agent: &CustomAgentConfig,
) -> Result<()> {
    let root = project_root(dir);
    let (local_path, mut doc) = read_local_config_document(&root);

    let doc_map = doc
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("Invalid YAML format"))?;

    let agents = doc_map
        .entry(serde_yaml::Value::String("agents".to_string()))
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("Agents is not a mapping"))?;

    let serialized_agent = serde_yaml::to_value(agent)?;
    agents.insert(
        serde_yaml::Value::String(agent_id.to_string()),
        serialized_agent,
    );

    write_local_config_document(&local_path, &doc)?;
    Ok(())
}

pub fn remove_local_custom_agent(dir: &str, agent_id: &str) -> Result<()> {
    let root = project_root(dir);
    let (local_path, mut doc) = read_local_config_document(&root);

    if let Some(doc_map) = doc.as_mapping_mut() {
        if let Some(agents) = doc_map.get_mut("agents").and_then(|v| v.as_mapping_mut()) {
            agents.remove(&serde_yaml::Value::String(agent_id.to_string()));
            if agents.is_empty() {
                doc_map.remove("agents");
            }
        }
    }

    write_local_config_document(&local_path, &doc)?;
    Ok(())
}
