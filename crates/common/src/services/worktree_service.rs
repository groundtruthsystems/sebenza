use crate::adapters::fs::{
    build_control_env_map, build_runtime_env_map, ensure_worktree_storage_dirs, load_dotenv_local,
    write_control_env, write_runtime_env, write_worktree_meta,
};
use crate::adapters::git::{CreateGitWorktreeOptions, CreateWorktreeMode, GitGateway};
use crate::domain::config::RuntimeKind;
use crate::domain::model::{
    OneshotMeta, WorktreeMeta, WorktreeSource, WorktreeStoragePaths, WORKTREE_META_SCHEMA_VERSION,
};
use crate::util::id::random_uuid;
use chrono::{SecondsFormat, Utc};
use std::collections::HashMap;

pub fn runtime_kind_str(runtime: RuntimeKind) -> &'static str {
    match runtime {
        RuntimeKind::Host => "host",
        RuntimeKind::Docker => "docker",
    }
}

pub struct CreateManagedWorktreeOptions {
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    pub mode: CreateWorktreeMode,
    pub base_branch: Option<String>,
    pub profile: String,
    pub agent: String,
    pub runtime: RuntimeKind,
    pub startup_env_values: HashMap<String, String>,
    pub allocated_ports: HashMap<String, u16>,
    pub runtime_env_extras: HashMap<String, String>,
    pub control_url: Option<String>,
    pub control_token: Option<String>,
    pub source: Option<WorktreeSource>,
    pub oneshot: Option<OneshotMeta>,
    pub delete_branch_on_rollback: bool,
}

pub struct InitializeManagedWorktreeResult {
    pub meta: WorktreeMeta,
    pub paths: WorktreeStoragePaths,
    pub runtime_env: HashMap<String, String>,
}

struct InitializeManagedWorktreeOptions {
    git_dir: String,
    branch: String,
    base_branch: Option<String>,
    profile: String,
    agent: String,
    runtime: RuntimeKind,
    startup_env_values: HashMap<String, String>,
    allocated_ports: HashMap<String, u16>,
    runtime_env_extras: HashMap<String, String>,
    dotenv_values: HashMap<String, String>,
    control_url: Option<String>,
    control_token: Option<String>,
    source: Option<WorktreeSource>,
    oneshot: Option<OneshotMeta>,
}

fn initialize_managed_worktree(
    opts: InitializeManagedWorktreeOptions,
) -> Result<InitializeManagedWorktreeResult, String> {
    if opts.control_url.is_some() != opts.control_token.is_some() {
        return Err("controlUrl and controlToken must be provided together".to_string());
    }

    let meta = WorktreeMeta {
        schema_version: WORKTREE_META_SCHEMA_VERSION,
        worktree_id: random_uuid(),
        branch: opts.branch.clone(),
        label: None,
        base_branch: opts.base_branch.clone(),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        profile: opts.profile.clone(),
        agent: opts.agent.clone(),
        runtime: runtime_kind_str(opts.runtime).to_string(),
        startup_env_values: opts.startup_env_values.clone(),
        allocated_ports: opts.allocated_ports.clone(),
        source: opts.source.clone(),
        oneshot: opts.oneshot.clone(),
        conversation: None,
        agent_terminal_stale: None,
        tabs: None,
        active_tab_id: None,
        fork_counter: None,
    };

    let paths = ensure_worktree_storage_dirs(&opts.git_dir)?;
    write_worktree_meta(&opts.git_dir, &meta)?;

    let runtime_env = build_runtime_env_map(&meta, &opts.runtime_env_extras, &opts.dotenv_values);
    write_runtime_env(&opts.git_dir, &runtime_env)?;

    if let (Some(url), Some(token)) = (&opts.control_url, &opts.control_token) {
        let control_env = build_control_env_map(url, token, &meta.worktree_id, &meta.branch, &opts.git_dir);
        write_control_env(&opts.git_dir, &control_env)?;
    }

    Ok(InitializeManagedWorktreeResult {
        meta,
        paths,
        runtime_env,
    })
}

/// Create a git worktree and initialize its managed artifacts (meta + env).
/// Rolls back (removes the worktree, and optionally deletes the branch) if
/// initialization fails after the worktree was added.
pub fn create_managed_worktree(
    git: &GitGateway,
    opts: CreateManagedWorktreeOptions,
) -> Result<InitializeManagedWorktreeResult, String> {
    git.create_worktree(&CreateGitWorktreeOptions {
        repo_root: opts.repo_root.clone(),
        worktree_path: opts.worktree_path.clone(),
        branch: opts.branch.clone(),
        mode: opts.mode.clone(),
    })?;

    let git_dir = match git.resolve_worktree_git_dir(&opts.worktree_path) {
        Ok(dir) => dir,
        Err(e) => return Err(rollback(git, &opts, e)),
    };
    let dotenv_values = load_dotenv_local(&opts.worktree_path);

    let initialized = initialize_managed_worktree(InitializeManagedWorktreeOptions {
        git_dir,
        branch: opts.branch.clone(),
        base_branch: opts.base_branch.clone(),
        profile: opts.profile.clone(),
        agent: opts.agent.clone(),
        runtime: opts.runtime,
        startup_env_values: opts.startup_env_values.clone(),
        allocated_ports: opts.allocated_ports.clone(),
        runtime_env_extras: opts.runtime_env_extras.clone(),
        dotenv_values,
        control_url: opts.control_url.clone(),
        control_token: opts.control_token.clone(),
        source: opts.source.clone(),
        oneshot: opts.oneshot.clone(),
    });

    match initialized {
        Ok(result) => Ok(result),
        Err(e) => Err(rollback(git, &opts, e)),
    }
}

pub struct AdoptManagedWorktreeOptions {
    pub git_dir: String,
    pub worktree_path: String,
    pub branch: String,
    pub profile: String,
    pub agent: String,
    pub runtime: RuntimeKind,
    pub startup_env_values: HashMap<String, String>,
    pub allocated_ports: HashMap<String, u16>,
    pub control_url: Option<String>,
    pub control_token: Option<String>,
}

/// Adopt (import) an EXISTING, unmanaged worktree: write its managed artifacts
/// (meta + runtime/control env) — the create flow minus `git worktree add`.
/// Used when opening a worktree that has no `meta.json` yet.
pub fn adopt_managed_worktree(
    opts: AdoptManagedWorktreeOptions,
) -> Result<InitializeManagedWorktreeResult, String> {
    let dotenv_values = load_dotenv_local(&opts.worktree_path);
    initialize_managed_worktree(InitializeManagedWorktreeOptions {
        git_dir: opts.git_dir,
        branch: opts.branch,
        base_branch: None,
        profile: opts.profile,
        agent: opts.agent,
        runtime: opts.runtime,
        startup_env_values: opts.startup_env_values,
        allocated_ports: opts.allocated_ports,
        runtime_env_extras: HashMap::from([(
            "SEBENZA_WORKTREE_PATH".to_string(),
            opts.worktree_path,
        )]),
        dotenv_values,
        control_url: opts.control_url,
        control_token: opts.control_token,
        source: Some(WorktreeSource::Ui),
        oneshot: None,
    })
}

/// Undo a partially-created worktree, appending any cleanup failure to `error`.
fn rollback(git: &GitGateway, opts: &CreateManagedWorktreeOptions, error: String) -> String {
    let mut cleanup_errors = Vec::new();
    if let Err(e) = git.remove_worktree(&opts.repo_root, &opts.worktree_path, true) {
        cleanup_errors.push(format!("worktree rollback failed: {e}"));
    }
    if opts.delete_branch_on_rollback
        && let Err(e) = git.delete_branch(&opts.repo_root, &opts.branch, true)
    {
        cleanup_errors.push(format!("branch rollback failed: {e}"));
    }
    if cleanup_errors.is_empty() {
        error
    } else {
        format!("{error}; {}", cleanup_errors.join("; "))
    }
}

pub struct CreateWorktreeTarget {
    pub branch: String,
    pub agent: String,
}

pub fn prefix_agent_branch(agent: &str, branch: &str) -> String {
    format!("{agent}-{branch}")
}

/// One target per agent. A single agent keeps the plain branch; multiple agents
/// each get an `<agent>-<branch>` prefix.
pub fn build_create_worktree_targets(branch: &str, agent_ids: &[String]) -> Vec<CreateWorktreeTarget> {
    if agent_ids.len() <= 1 {
        return agent_ids
            .first()
            .map(|agent| {
                vec![CreateWorktreeTarget {
                    branch: branch.to_string(),
                    agent: agent.clone(),
                }]
            })
            .unwrap_or_default();
    }
    agent_ids
        .iter()
        .map(|agent| CreateWorktreeTarget {
            branch: prefix_agent_branch(agent, branch),
            agent: agent.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::fs::read_worktree_meta;
    use std::process::Command;

    fn git(args: &[&str], cwd: &std::path::Path) {
        let status = Command::new("git").args(args).current_dir(cwd).output().unwrap();
        assert!(status.status.success(), "git {args:?} failed");
    }

    #[test]
    fn create_managed_worktree_adds_worktree_and_writes_meta() {
        // A real temp git repo (no tmux involved on this path).
        let tmp = std::env::temp_dir().join(format!("sebenza-create-{}", random_uuid()));
        let repo = tmp.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&["init", "-b", "main"], &repo);
        git(&["config", "user.email", "t@t"], &repo);
        git(&["config", "user.name", "t"], &repo);
        std::fs::write(repo.join("README"), "hi").unwrap();
        git(&["add", "."], &repo);
        git(&["commit", "-m", "init"], &repo);

        let repo_root = repo.to_string_lossy().to_string();
        let worktree_path = tmp.join("wt").to_string_lossy().to_string();
        let git = GitGateway::new();
        let result = create_managed_worktree(
            &git,
            CreateManagedWorktreeOptions {
                repo_root: repo_root.clone(),
                worktree_path: worktree_path.clone(),
                branch: "feature".to_string(),
                mode: CreateWorktreeMode::New { base_branch: Some("main".to_string()) },
                base_branch: Some("main".to_string()),
                profile: "default".to_string(),
                agent: "claude".to_string(),
                runtime: RuntimeKind::Host,
                startup_env_values: HashMap::from([("NODE_ENV".to_string(), "test".to_string())]),
                allocated_ports: HashMap::from([("PORT".to_string(), 5121)]),
                runtime_env_extras: HashMap::new(),
                control_url: None,
                control_token: None,
                source: Some(WorktreeSource::Ui),
                oneshot: None,
                delete_branch_on_rollback: true,
            },
        )
        .unwrap();

        // Worktree checked out, meta persisted, ports recorded.
        assert!(std::path::Path::new(&worktree_path).join("README").exists());
        assert_eq!(result.meta.branch, "feature");
        assert_eq!(result.meta.agent, "claude");
        assert_eq!(result.meta.allocated_ports.get("PORT"), Some(&5121));
        let git_dir = git.resolve_worktree_git_dir(&worktree_path).unwrap();
        let meta = read_worktree_meta(&git_dir).unwrap();
        assert_eq!(meta.worktree_id, result.meta.worktree_id);
        assert_eq!(result.runtime_env.get("SEBENZA_BRANCH").map(String::as_str), Some("feature"));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
