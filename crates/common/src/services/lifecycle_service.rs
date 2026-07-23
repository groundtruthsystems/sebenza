//! Worktree lifecycle operations — port of the remove/merge/label/archive paths
//! of `backend-legacy/src/services/lifecycle-service.ts`. Each op resolves the
//! worktree, mutates git/tmux/fs, then force-reconciles so the next snapshot
//! reflects the change. Blocking (shells out) — call via `spawn_blocking`.

use crate::adapters::agent_runtime::ensure_agent_runtime_artifacts;
use crate::adapters::control_token::load_control_token;
use crate::adapters::docker::{launch_container, LaunchContainerOpts};
use crate::adapters::fs::{
    build_control_env_map, build_runtime_env_map, get_worktree_storage_paths, load_dotenv_local,
    read_worktree_archive_state, read_worktree_meta, write_control_env, write_runtime_env,
    write_worktree_archive_state, write_worktree_meta,
};
use crate::adapters::git::{CreateWorktreeMode, GitGateway, GitWorktreeEntry};
use crate::adapters::hooks::{run_lifecycle_hook, RunLifecycleHookInput};
use crate::adapters::tmux::{
    build_project_session_name, build_worktree_parking_window_name, build_worktree_window_name,
    TmuxGateway,
};
use crate::config::expand_template;
use crate::domain::config::{ProfileConfig, ProjectConfig, RuntimeKind};
use crate::domain::model::{WorktreeMeta, WorktreeSource};
use crate::domain::policies::{
    allocate_service_ports, generate_fallback_branch_name, is_valid_branch_name, is_valid_env_key,
};
use crate::adapters::session_discovery::{
    capture_new_session_id, list_session_ids, DiscoverableAgentKind,
};
use crate::services::agent_registry::{get_agent_definition, AgentDefinition, AgentImplementation};
use crate::services::tab_logic::{
    active_tab_id as read_active_tab_id, append_tab, build_fork_tab, find_tab, list_tabs,
    next_fork_seq, remove_tab, root_tab, set_active_tab, update_tab, with_tabs, ForkTabInput,
    TabPatch,
};
use crate::domain::model::{WorktreeTab, WorktreeTabKind, ROOT_TAB_ID};
use crate::services::auto_name_service::generate_branch_name;
use crate::services::agent_service::{
    build_agent_pane_command, build_docker_agent_pane_command, build_docker_shell_command,
    build_managed_shell_command, AgentInvocation, AgentLaunchMode,
};
use crate::services::archive_service::set_archived_worktree_state;
use crate::services::config_view::get_default_profile_name;
use crate::services::project_runtime::ProjectRuntime;
use crate::services::reconciliation::ReconciliationService;
use crate::services::session_service::{
    ensure_session_layout, plan_session_layout, PaneCommandSet, SessionLayoutContext,
};
use crate::services::worktree_service::{
    build_create_worktree_targets, create_managed_worktree, CreateManagedWorktreeOptions,
    InitializeManagedWorktreeResult,
};
use crate::domain::model::OneshotMeta;
use chrono::{SecondsFormat, Utc};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAX_WORKTREE_LABEL_LENGTH: usize = 80;

/// Requested create mode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CreateMode {
    New,
    Existing,
}

/// Input to `create_worktrees` (mirrors `CreateLifecycleWorktreesInput`).
pub struct CreateWorktreesInput {
    pub mode: Option<CreateMode>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub prompt: Option<String>,
    pub profile: Option<String>,
    pub agent: Option<String>,
    pub agents: Option<Vec<String>>,
    pub env_overrides: Option<HashMap<String, String>>,
    pub source: Option<WorktreeSource>,
    pub oneshot: Option<OneshotMeta>,
}

pub struct CreateWorktreesResult {
    pub primary_branch: String,
    pub branches: Vec<String>,
}

struct ResolvedProfile {
    name: String,
    profile: ProfileConfig,
}

struct BranchAvailability {
    start_point: Option<String>,
    delete_branch_on_rollback: bool,
}

/// An operation error carrying the HTTP status the handler should surface.
#[derive(Debug)]
pub struct LifecycleError {
    pub message: String,
    pub status: u16,
}

impl LifecycleError {
    fn new(message: impl Into<String>, status: u16) -> Self {
        LifecycleError {
            message: message.into(),
            status,
        }
    }
}

/// Wrap a shell/IO failure (`Err(String)`) as an unprocessable-entity error,
/// matching the legacy `wrapOperationError` fallback of 422.
fn op(result: Result<(), String>) -> Result<(), LifecycleError> {
    result.map_err(|message| LifecycleError::new(message, 422))
}

struct ResolvedWorktree {
    entry: GitWorktreeEntry,
    git_dir: String,
    meta: Option<WorktreeMeta>,
}

/// Everything the tab ops need: a resolved, open, built-in-agent, host-runtime
/// worktree with its refreshed runtime artifacts and tmux coordinates.
struct TabContext {
    resolved: ResolvedWorktree,
    initialized: InitializeManagedWorktreeResult,
    meta: WorktreeMeta,
    worktree_path: String,
    agent: AgentDefinition,
    agent_kind: DiscoverableAgentKind,
    profile: ResolvedProfile,
    session_name: String,
    window_name: String,
    parking_window: String,
}

/// The built-in agent kind (`claude`/`codex`) whose sessions we can discover.
fn discoverable_agent_kind(agent: &AgentDefinition) -> Option<DiscoverableAgentKind> {
    match &agent.implementation {
        AgentImplementation::Builtin(id) if id == "claude" => Some(DiscoverableAgentKind::Claude),
        AgentImplementation::Builtin(id) if id == "codex" => Some(DiscoverableAgentKind::Codex),
        _ => None,
    }
}

/// Cheap-to-clone bundle of the dependencies the lifecycle ops need.
pub struct LifecycleService {
    project_root: String,
    config: Arc<ProjectConfig>,
    git: GitGateway,
    tmux: TmuxGateway,
    reconciliation: Arc<ReconciliationService>,
    runtime: Arc<Mutex<ProjectRuntime>>,
    control_base_url: String,
}

impl LifecycleService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_root: String,
        config: Arc<ProjectConfig>,
        git: GitGateway,
        tmux: TmuxGateway,
        reconciliation: Arc<ReconciliationService>,
        runtime: Arc<Mutex<ProjectRuntime>>,
        control_base_url: String,
    ) -> Self {
        LifecycleService {
            project_root,
            config,
            git,
            tmux,
            reconciliation,
            runtime,
            control_base_url,
        }
    }

    /// The git gateway (used by the oneshot watcher to resolve a worktree's git
    /// dir before reading its persisted meta).
    pub fn git(&self) -> &GitGateway {
        &self.git
    }

    pub fn remove_worktree(&self, branch: &str) -> Result<(), LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        self.remove_resolved_worktree(&resolved)
    }

    /// Clear a worktree's oneshot arm state from its persisted meta, if present.
    /// Idempotent: returns true when armed state was cleared, false otherwise.
    pub fn disarm_oneshot(&self, branch: &str) -> bool {
        let Ok(resolved) = self.resolve_existing_worktree(branch) else {
            return false;
        };
        let Some(mut meta) = resolved.meta else {
            return false;
        };
        if meta.oneshot.is_none() {
            return false;
        }
        meta.oneshot = None;
        write_worktree_meta(&resolved.git_dir, &meta).is_ok()
    }

    /// Close a worktree's tmux windows (without removing the worktree).
    pub fn close_worktree(&self, branch: &str) -> Result<(), LifecycleError> {
        self.resolve_existing_worktree(branch)?;
        self.close_branch_window(branch)
    }

    /// Reopen a managed worktree's tmux session, resuming the agent when it
    /// supports resume. An optional follow-up `prompt` is submitted on resume.
    pub fn open_worktree(
        &self,
        branch: &str,
        prompt: Option<&str>,
        oneshot: Option<OneshotMeta>,
    ) -> Result<(), LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        let Some(mut meta) = resolved.meta.clone() else {
            // NOTE: adopting an unmanaged worktree on open is deferred; require meta.
            return Err(LifecycleError::new(
                format!("Worktree {branch} has no managed metadata"),
                409,
            ));
        };

        if let Some(oneshot) = oneshot {
            meta.oneshot = Some(oneshot);
            op(write_worktree_meta(&resolved.git_dir, &meta))?;
        }

        let initialized =
            self.refresh_managed_artifacts_from_meta(&resolved.git_dir, &meta, &resolved.entry.path)?;
        let profile = self.resolve_profile(Some(&meta.profile))?;
        let agent = self.resolve_agent_definition(Some(&meta.agent))?;
        let launch_mode = if agent.capabilities.resume {
            AgentLaunchMode::Resume
        } else {
            AgentLaunchMode::Fresh
        };

        op(ensure_agent_runtime_artifacts(&resolved.git_dir, &resolved.entry.path))?;
        // NOTE: codex resume-conversation-id on open is still deferred.
        self.materialize_runtime_session(
            branch,
            &profile,
            &agent,
            &initialized,
            &resolved.entry.path,
            launch_mode,
            None,
            prompt,
            None,
            None,
        )?;
        self.restore_worktree_tabs(
            branch,
            &resolved.git_dir,
            &resolved.entry.path,
            &profile,
            &agent,
            &initialized.paths.runtime_env_path,
        )?;

        if meta.agent_terminal_stale == Some(true) {
            meta.agent_terminal_stale = Some(false);
            op(write_worktree_meta(&resolved.git_dir, &meta))?;
        }

        self.reconcile_force();
        Ok(())
    }

    /// Launch a configured external tool (editor) against a worktree's directory.
    /// Spawns the launcher command detached on the host (inherits DISPLAY etc.),
    /// pointed at the worktree dir via `${WORKTREE_PATH}`/`${REPO_PATH}`/`${BRANCH}`.
    pub fn launch_worktree(&self, branch: &str, launcher_id: &str) -> Result<(), LifecycleError> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        let resolved = self.resolve_existing_worktree(branch)?;
        let launcher = self
            .config
            .launchers
            .get(launcher_id)
            .ok_or_else(|| LifecycleError::new(format!("Unknown launcher: {launcher_id}"), 404))?;

        // Template vars are passed as env and referenced in the shell command, so
        // paths with spaces stay intact (no string interpolation into the shell).
        let command = launcher
            .command
            .replace("${WORKTREE_PATH}", "$SEBENZA_WORKTREE_PATH")
            .replace("${REPO_PATH}", "$SEBENZA_REPO_PATH")
            .replace("${BRANCH}", "$SEBENZA_BRANCH");

        // Validate the launcher binary (first token) is resolvable, for a clear error.
        if let Some(bin) = launcher.command.split_whitespace().next()
            && !bin.contains('$')
            && !crate::util::shell::which(bin)
        {
            return Err(LifecycleError::new(
                format!("Launcher binary not found on PATH: {bin}"),
                422,
            ));
        }

        Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&resolved.entry.path)
            .env("SEBENZA_WORKTREE_PATH", &resolved.entry.path)
            .env("SEBENZA_REPO_PATH", &self.project_root)
            .env("SEBENZA_BRANCH", branch)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0) // detach so the GUI outlives the request
            .spawn()
            .map_err(|e| LifecycleError::new(format!("Failed to launch: {e}"), 422))?;
        // Intentionally do not wait — the launched GUI runs independently.
        Ok(())
    }

    pub fn merge_worktree(&self, branch: &str) -> Result<(), LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        self.ensure_no_uncommitted_changes(&resolved.entry)?;

        let main_branch = self.config.workspace.main_branch.clone();
        op(self.git.merge_branch(&self.project_root, branch, &main_branch))?;

        self.remove_resolved_worktree(&resolved).map_err(|err| {
            LifecycleError::new(
                format!("Merged {branch} into {main_branch} but cleanup failed: {}", err.message),
                500,
            )
        })
    }

    pub fn set_worktree_archived(&self, branch: &str, archived: bool) -> Result<(), LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        if archived {
            self.close_branch_window(branch)?;
        }
        self.update_worktree_archived_state(&resolved.entry.path, archived)
    }

    pub fn set_worktree_label(
        &self,
        branch: &str,
        label: Option<&str>,
    ) -> Result<Option<String>, LifecycleError> {
        let normalized = normalize_worktree_label(label)?;
        let resolved = self.resolve_existing_worktree(branch)?;
        let Some(mut meta) = resolved.meta else {
            return Err(LifecycleError::new(
                format!("Worktree {branch} has no managed metadata to label"),
                409,
            ));
        };
        meta.label = normalized.clone();
        op(write_worktree_meta(&resolved.git_dir, &meta))?;
        self.reconcile_force();
        Ok(normalized)
    }

    /// Create one worktree per selected agent, rolling back everything created so
    /// far if any target fails.
    pub fn create_worktrees(
        &self,
        input: &CreateWorktreesInput,
    ) -> Result<CreateWorktreesResult, LifecycleError> {
        let mode = input.mode.unwrap_or(CreateMode::New);
        let agent_ids = self.resolve_selected_agents(input)?;
        if agent_ids.len() > 1 && mode == CreateMode::Existing {
            return Err(LifecycleError::new(
                "Creating multiple agents is only supported for new worktrees",
                400,
            ));
        }

        let branch = self.resolve_branch(input.branch.as_deref(), input.prompt.as_deref(), mode)?;
        let targets = build_create_worktree_targets(&branch, &agent_ids);
        let mut created_branches: Vec<String> = Vec::new();

        for target in targets {
            match self.create_resolved_worktree(input, mode, &target.branch, &target.agent) {
                Ok(created) => created_branches.push(created),
                Err(err) => {
                    let rollback = self.rollback_created_worktrees(&created_branches);
                    return Err(match rollback {
                        Some(cleanup) => {
                            LifecycleError::new(format!("{}; {cleanup}", err.message), err.status)
                        }
                        None => err,
                    });
                }
            }
        }

        Ok(CreateWorktreesResult {
            primary_branch: created_branches.first().cloned().unwrap_or_default(),
            branches: created_branches,
        })
    }

    fn create_resolved_worktree(
        &self,
        input: &CreateWorktreesInput,
        mode: CreateMode,
        branch: &str,
        agent_id: &str,
    ) -> Result<String, LifecycleError> {
        let requested_base = input.base_branch.as_deref().map(str::trim).filter(|b| !b.is_empty());
        if let Some(base) = requested_base {
            if !is_valid_branch_name(base) {
                return Err(LifecycleError::new("Invalid base branch name", 400));
            }
            if mode == CreateMode::Existing {
                return Err(LifecycleError::new(
                    "Base branch is only supported for new worktrees",
                    400,
                ));
            }
            if base == branch {
                return Err(LifecycleError::new("Base branch must differ from branch name", 400));
            }
        }

        let base_branch = if mode == CreateMode::New {
            Some(
                requested_base
                    .map(str::to_string)
                    .unwrap_or_else(|| self.config.workspace.main_branch.clone()),
            )
        } else {
            None
        };

        let availability = self.resolve_branch_availability(branch, mode)?;
        let profile = self.resolve_profile(input.profile.as_deref())?;
        let agent = self.resolve_agent_definition(Some(agent_id))?;
        let worktree_path = self.resolve_worktree_path(branch);
        let source = input.source.clone().unwrap_or(WorktreeSource::Ui);
        let delete_branch_on_rollback =
            mode == CreateMode::New || availability.delete_branch_on_rollback;

        if profile.profile.runtime == RuntimeKind::Docker && profile.profile.image.is_none() {
            return Err(LifecycleError::new("Docker profile is missing an image", 422));
        }

        // git worktree add + meta + env (session is built separately below).
        if let Some(parent) = Path::new(&worktree_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| LifecycleError::new(e.to_string(), 422))?;
        }

        let git_mode = match mode {
            CreateMode::New => CreateWorktreeMode::New {
                base_branch: base_branch.clone(),
            },
            CreateMode::Existing => CreateWorktreeMode::Existing {
                start_point: availability.start_point.clone(),
            },
        };
        let control_token = load_control_token().map_err(|e| LifecycleError::new(e, 422))?;
        let control_url = self.control_url(profile.profile.runtime);

        let create_result = create_managed_worktree(
            &self.git,
            CreateManagedWorktreeOptions {
                repo_root: self.project_root.clone(),
                worktree_path: worktree_path.clone(),
                branch: branch.to_string(),
                mode: git_mode,
                base_branch: base_branch.clone(),
                profile: profile.name.clone(),
                agent: agent.id.clone(),
                runtime: profile.profile.runtime,
                startup_env_values: self.build_startup_env_values(input.env_overrides.as_ref())?,
                allocated_ports: self.allocate_ports(),
                runtime_env_extras: HashMap::from([(
                    "SEBENZA_WORKTREE_PATH".to_string(),
                    worktree_path.clone(),
                )]),
                control_url: Some(control_url),
                control_token: Some(control_token),
                source: Some(source.clone()),
                oneshot: input.oneshot.clone(),
                delete_branch_on_rollback,
            },
        );

        let mut initialized = match create_result {
            Ok(initialized) => initialized,
            Err(e) => return Err(LifecycleError::new(e, 422)),
        };

        // From here the worktree exists on disk: clean it up on any later failure.
        let result = (|| -> Result<String, LifecycleError> {
            self.run_hook(
                "postCreate",
                self.config.lifecycle_hooks.post_create.as_deref(),
                Some(&initialized.meta),
                &worktree_path,
            )?;
            let git_dir = initialized.paths.git_dir.clone();
            let meta = initialized.meta.clone();
            initialized = self.refresh_managed_artifacts_from_meta(&git_dir, &meta, &worktree_path)?;
            op(ensure_agent_runtime_artifacts(&initialized.paths.git_dir, &worktree_path))?;
            self.materialize_runtime_session(
                branch,
                &profile,
                &agent,
                &initialized,
                &worktree_path,
                AgentLaunchMode::Fresh,
                input.prompt.as_deref(),
                None,
                Some(source),
                None,
            )?;
            self.reconcile_force();
            Ok(branch.to_string())
        })();

        if result.is_err() {
            let _ = self.cleanup_failed_create(branch, &worktree_path, delete_branch_on_rollback);
        }
        result
    }

    /// Rewrite runtime + control env from a worktree's meta (idempotent refresh),
    /// returning the materialized artifacts.
    fn refresh_managed_artifacts_from_meta(
        &self,
        git_dir: &str,
        meta: &WorktreeMeta,
        worktree_path: &str,
    ) -> Result<InitializeManagedWorktreeResult, LifecycleError> {
        let dotenv = load_dotenv_local(worktree_path);
        let extra = HashMap::from([(
            "SEBENZA_WORKTREE_PATH".to_string(),
            worktree_path.to_string(),
        )]);
        let runtime_env = build_runtime_env_map(meta, &extra, &dotenv);
        op(write_runtime_env(git_dir, &runtime_env))?;

        let control_token = load_control_token().map_err(|e| LifecycleError::new(e, 422))?;
        let control_env = build_control_env_map(
            &self.control_url(RuntimeKind::Host),
            &control_token,
            &meta.worktree_id,
            &meta.branch,
        );
        op(write_control_env(git_dir, &control_env))?;

        Ok(InitializeManagedWorktreeResult {
            meta: meta.clone(),
            paths: get_worktree_storage_paths(git_dir),
            runtime_env,
        })
    }

    /// Build the tmux session for the worktree, launching the agent + shell panes.
    #[allow(clippy::too_many_arguments)]
    fn materialize_runtime_session(
        &self,
        branch: &str,
        profile: &ResolvedProfile,
        agent: &AgentDefinition,
        initialized: &InitializeManagedWorktreeResult,
        worktree_path: &str,
        launch_mode: AgentLaunchMode,
        creation_prompt: Option<&str>,
        follow_up_prompt: Option<&str>,
        source: Option<WorktreeSource>,
        resume_conversation_id: Option<&str>,
    ) -> Result<(), LifecycleError> {
        let is_fresh = launch_mode == AgentLaunchMode::Fresh;
        // System prompt (profile + oneshot) applies to fresh launches only.
        let base_system_prompt = if is_fresh {
            profile
                .profile
                .system_prompt
                .as_deref()
                .map(|sp| expand_template(sp, &initialized.runtime_env))
        } else {
            None
        };
        let oneshot_prompt = if is_fresh && source == Some(WorktreeSource::Oneshot) {
            Some(self.config.oneshot.system_prompt.clone())
        } else {
            None
        };
        let system_prompt = match (base_system_prompt, oneshot_prompt) {
            (Some(base), Some(one)) => Some(format!("{base}\n\n{one}")),
            (base, one) => one.or(base),
        };
        // The prompt source depends on launch mode (guards against a creation
        // prompt re-firing on reopen, and vice-versa).
        let prompt_source = if launch_mode == AgentLaunchMode::Resume {
            follow_up_prompt
        } else {
            creation_prompt
        };
        let prompt = prompt_source.map(str::trim).filter(|p| !p.is_empty());

        let runtime_env_path = initialized.paths.runtime_env_path.clone();
        let invocation = AgentInvocation {
            agent,
            yolo: profile.profile.yolo == Some(true),
            system_prompt: system_prompt.as_deref(),
            prompt,
            launch_mode,
            worktree_path,
            repo_root: &self.project_root,
            branch,
            profile_name: &profile.name,
            resume_conversation_id,
            fork_from_session_id: None,
            pin_session_id: None,
        };

        let (agent_command, shell_command) = if profile.profile.runtime == RuntimeKind::Docker {
            // Launch (or reuse) the sandbox container, then exec into it.
            let image = profile.profile.image.clone().ok_or_else(|| {
                LifecycleError::new("Docker profile is missing an image", 422)
            })?;
            let container = launch_container(&LaunchContainerOpts {
                branch: branch.to_string(),
                wt_dir: worktree_path.to_string(),
                main_repo_dir: self.project_root.clone(),
                image,
                env_passthrough: profile.profile.env_passthrough.clone(),
                mounts: profile.profile.mounts.clone().unwrap_or_default(),
                service_port_envs: self.config.services.iter().map(|s| s.port_env.clone()).collect(),
                runtime_env: initialized.runtime_env.clone(),
            })
            .map_err(|e| LifecycleError::new(e, 422))?;
            (
                build_docker_agent_pane_command(&container, worktree_path, &runtime_env_path, &invocation),
                build_docker_shell_command(&container, worktree_path, &runtime_env_path),
            )
        } else {
            (
                build_agent_pane_command(&runtime_env_path, &invocation),
                build_managed_shell_command(&runtime_env_path),
            )
        };

        let plan = plan_session_layout(
            &self.project_root,
            branch,
            &profile.profile.panes,
            &SessionLayoutContext {
                repo_root: self.project_root.clone(),
                worktree_path: worktree_path.to_string(),
                pane_commands: PaneCommandSet {
                    agent: agent_command,
                    shell: shell_command,
                },
            },
        )
        .map_err(|e| LifecycleError::new(e, 422))?;

        op(ensure_session_layout(&self.tmux, &plan))
    }

    fn cleanup_failed_create(
        &self,
        branch: &str,
        worktree_path: &str,
        delete_branch: bool,
    ) -> Result<(), LifecycleError> {
        let _ = self.kill_worktree_windows(branch);
        op(self.git.remove_worktree(&self.project_root, worktree_path, true))?;
        if delete_branch {
            let _ = self.git.delete_branch(&self.project_root, branch, true);
        }
        Ok(())
    }

    fn rollback_created_worktrees(&self, branches: &[String]) -> Option<String> {
        let mut errors = Vec::new();
        for branch in branches {
            if let Ok(resolved) = self.resolve_existing_worktree(branch)
                && let Err(e) = self.remove_resolved_worktree(&resolved)
            {
                errors.push(e.message);
            }
        }
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    /// Relaunch a worktree's agent pane resuming an existing conversation
    /// (`resume_conversation_id` from the latest on-disk session). Built-in
    /// claude/codex only.
    pub fn refresh_agent_terminal(
        &self,
        branch: &str,
        resume_conversation_id: &str,
    ) -> Result<(), LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        let Some(meta) = resolved.meta.clone() else {
            return Err(LifecycleError::new(
                format!("Worktree {branch} has no managed metadata to refresh"),
                409,
            ));
        };
        let agent = self.resolve_agent_definition(Some(&meta.agent))?;
        if agent.kind != "builtin" {
            return Err(LifecycleError::new(
                "Refreshing the agent terminal is only available for built-in agent worktrees",
                409,
            ));
        }
        let profile = self.resolve_profile(Some(&meta.profile))?;
        let initialized =
            self.refresh_managed_artifacts_from_meta(&resolved.git_dir, &meta, &resolved.entry.path)?;

        self.materialize_runtime_session(
            branch,
            &profile,
            &agent,
            &initialized,
            &resolved.entry.path,
            AgentLaunchMode::Resume,
            None,
            None,
            None,
            Some(resume_conversation_id),
        )?;
        // Rebuilding the agent pane recreated the worktree window, so any parked
        // fork panes (and root.paneId in meta) are now stale — rebuild them.
        self.restore_worktree_tabs(
            branch,
            &resolved.git_dir,
            &resolved.entry.path,
            &profile,
            &agent,
            &initialized.paths.runtime_env_path,
        )?;
        self.reconcile_force();
        Ok(())
    }

    // --- tabs ---

    /// Fork the root agent session into a new parked pane and bring it on-screen.
    pub fn create_worktree_tab(&self, branch: &str) -> Result<WorktreeTab, LifecycleError> {
        let ctx = self.prepare_tab_context(branch)?;
        let Some(root_session_id) = self.ensure_root_session_id(&ctx)? else {
            return Err(LifecycleError::new(
                "The root session hasn't started yet — interact with it once before forking a tab",
                409,
            ));
        };
        // ensure_root_session_id may have persisted root.sessionId; re-read for a fresh base.
        let meta = self.read_meta_or_throw(&ctx.resolved.git_dir)?;
        let seq = next_fork_seq(&meta);
        // Claude can pin the forked child id (deterministic); Codex self-assigns.
        let pin_session_id = (ctx.agent_kind == DiscoverableAgentKind::Claude)
            .then(crate::util::id::random_uuid);
        let runtime_env_path = ctx.initialized.paths.runtime_env_path.clone();
        let invocation = AgentInvocation {
            agent: &ctx.agent,
            yolo: ctx.profile.profile.yolo == Some(true),
            system_prompt: None,
            prompt: None,
            launch_mode: AgentLaunchMode::Fork,
            worktree_path: &ctx.worktree_path,
            repo_root: &self.project_root,
            branch,
            profile_name: &ctx.profile.name,
            resume_conversation_id: None,
            fork_from_session_id: Some(&root_session_id),
            pin_session_id: pin_session_id.as_deref(),
        };
        let agent_command = build_agent_pane_command(&runtime_env_path, &invocation);

        let visible_slot = format!("{}:{}.0", ctx.session_name, ctx.window_name);
        // Record the currently-visible (active) tab's pane id before the swap parks it.
        let outgoing_active_id = read_active_tab_id(&meta);
        let outgoing_pane_id = self.tmux.get_pane_id(&visible_slot).ok();

        let before = list_session_ids(ctx.agent_kind, &ctx.worktree_path);
        let pane_id = self
            .tmux
            .create_parked_pane(
                &ctx.session_name,
                &ctx.parking_window,
                &ctx.worktree_path,
                &build_managed_shell_command(&runtime_env_path),
            )
            .map_err(|e| LifecycleError::new(e, 422))?;
        op(self.tmux.run_command(&pane_id, &agent_command))?;
        let session_id = pin_session_id
            .clone()
            .or_else(|| capture_new_session_id(ctx.agent_kind, &ctx.worktree_path, &before));

        let tab = build_fork_tab(ForkTabInput {
            seq,
            session_id,
            pane_id: Some(pane_id.clone()),
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        });
        let mut next_meta = append_tab(meta, tab.clone()); // makes the fork active
        next_meta = update_tab(
            next_meta,
            &outgoing_active_id,
            TabPatch { session_id: None, pane_id: Some(outgoing_pane_id) },
        );
        op(write_worktree_meta(&ctx.resolved.git_dir, &next_meta))?;
        // A new fork becomes the active tab — bring it into the visible agent slot.
        op(self.tmux.swap_panes(&pane_id, &visible_slot))?;
        self.reconcile_force();
        Ok(tab)
    }

    /// Create an on-demand shell tab: a parked managed-shell pane swapped into
    /// the visible slot. Available for any open host-runtime worktree (not just
    /// the built-in agents), so custom-agent worktrees can get a browser shell.
    pub fn create_worktree_shell_tab(&self, branch: &str) -> Result<WorktreeTab, LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        let Some(meta) = resolved.meta.clone() else {
            return Err(LifecycleError::new(
                format!("Worktree {branch} has no managed metadata"),
                409,
            ));
        };
        let session_name = build_project_session_name(&self.project_root);
        let window_name = build_worktree_window_name(branch);
        if !self.tmux.has_window(&session_name, &window_name) {
            return Err(LifecycleError::new(format!("Worktree {branch} is not open"), 409));
        }
        let profile = self.resolve_profile(Some(&meta.profile))?;
        if profile.profile.runtime == RuntimeKind::Docker {
            return Err(LifecycleError::new(
                "Shell tabs are not supported for Docker worktrees",
                409,
            ));
        }
        let initialized =
            self.refresh_managed_artifacts_from_meta(&resolved.git_dir, &meta, &resolved.entry.path)?;
        let meta = initialized.meta.clone();
        let runtime_env_path = initialized.paths.runtime_env_path.clone();
        let worktree_path = resolved.entry.path.clone();
        let parking_window = build_worktree_parking_window_name(branch);

        let visible_slot = format!("{session_name}:{window_name}.0");
        let outgoing_active_id = read_active_tab_id(&meta);
        let outgoing_pane_id = self.tmux.get_pane_id(&visible_slot).ok();

        // The parked pane IS the managed shell (no agent typed on top).
        let pane_id = self
            .tmux
            .create_parked_pane(
                &session_name,
                &parking_window,
                &worktree_path,
                &build_managed_shell_command(&runtime_env_path),
            )
            .map_err(|e| LifecycleError::new(e, 422))?;

        let shell_count = list_tabs(&meta)
            .iter()
            .filter(|t| t.kind == WorktreeTabKind::Shell)
            .count();
        let label = if shell_count == 0 {
            "Shell".to_string()
        } else {
            format!("Shell {}", shell_count + 1)
        };
        let tab = WorktreeTab {
            tab_id: format!("shell-{}", Utc::now().timestamp_millis()),
            kind: WorktreeTabKind::Shell,
            label,
            seq: None,
            session_id: None,
            pane_id: Some(pane_id.clone()),
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        let mut next_meta = append_tab(meta, tab.clone()); // makes the shell tab active
        next_meta = update_tab(
            next_meta,
            &outgoing_active_id,
            TabPatch { session_id: None, pane_id: Some(outgoing_pane_id) },
        );
        op(write_worktree_meta(&resolved.git_dir, &next_meta))?;
        op(self.tmux.swap_panes(&pane_id, &visible_slot))?;
        self.reconcile_force();
        Ok(tab)
    }

    /// Bring a parked tab's pane into the visible agent slot.
    pub fn select_worktree_tab(&self, branch: &str, tab_id: &str) -> Result<(), LifecycleError> {
        let ctx = self.prepare_tab_context(branch)?;
        let target = find_tab(&ctx.meta, tab_id)
            .ok_or_else(|| LifecycleError::new(format!("Tab not found: {tab_id}"), 404))?;
        let outgoing_active_id = read_active_tab_id(&ctx.meta);
        if outgoing_active_id == tab_id {
            return Ok(());
        }
        let Some(target_pane) = target.pane_id else {
            return Err(LifecycleError::new(
                format!("Tab {tab_id} has no live pane to show"),
                409,
            ));
        };
        let visible_slot = format!("{}:{}.0", ctx.session_name, ctx.window_name);
        let outgoing_pane_id = self.tmux.get_pane_id(&visible_slot).ok();
        op(self.tmux.swap_panes(&target_pane, &visible_slot))?;
        let mut next_meta = update_tab(
            ctx.meta,
            &outgoing_active_id,
            TabPatch { session_id: None, pane_id: Some(outgoing_pane_id) },
        );
        next_meta = set_active_tab(next_meta, tab_id);
        op(write_worktree_meta(&ctx.resolved.git_dir, &next_meta))?;
        self.reconcile_force();
        Ok(())
    }

    /// Kill a fork tab's pane and drop it from the tab list.
    pub fn delete_worktree_tab(&self, branch: &str, tab_id: &str) -> Result<(), LifecycleError> {
        let ctx = self.prepare_tab_context(branch)?;
        let target = find_tab(&ctx.meta, tab_id)
            .ok_or_else(|| LifecycleError::new(format!("Tab not found: {tab_id}"), 404))?;
        if target.kind == WorktreeTabKind::Root {
            return Err(LifecycleError::new("The root tab cannot be deleted", 400));
        }
        let root = root_tab(&ctx.meta);
        // If the deleted tab is on-screen, bring the root back into the visible slot
        // first. swap-pane moves pane *content* between slots while pane ids stay
        // attached to their content, so root.paneId remains valid after the swap.
        if read_active_tab_id(&ctx.meta) == tab_id
            && let Some(root_pane) = root.as_ref().and_then(|r| r.pane_id.clone())
        {
            op(self.tmux.swap_panes(
                &root_pane,
                &format!("{}:{}.0", ctx.session_name, ctx.window_name),
            ))?;
        }
        if let Some(pane) = target.pane_id {
            op(self.tmux.kill_pane(&pane))?;
        }
        op(write_worktree_meta(
            &ctx.resolved.git_dir,
            &remove_tab(ctx.meta, tab_id),
        ))?;
        self.reconcile_force();
        Ok(())
    }

    fn read_meta_or_throw(&self, git_dir: &str) -> Result<WorktreeMeta, LifecycleError> {
        read_worktree_meta(git_dir)
            .ok_or_else(|| LifecycleError::new("Worktree metadata is missing", 409))
    }

    fn prepare_tab_context(&self, branch: &str) -> Result<TabContext, LifecycleError> {
        let resolved = self.resolve_existing_worktree(branch)?;
        if resolved.meta.is_none() {
            return Err(LifecycleError::new(
                format!("Worktree {branch} has no managed metadata"),
                409,
            ));
        }

        let session_name = build_project_session_name(&self.project_root);
        let window_name = build_worktree_window_name(branch);
        if !self.tmux.has_window(&session_name, &window_name) {
            return Err(LifecycleError::new(format!("Worktree {branch} is not open"), 409));
        }

        let meta = resolved.meta.clone().unwrap();
        let profile = self.resolve_profile(Some(&meta.profile))?;
        if profile.profile.runtime == RuntimeKind::Docker {
            return Err(LifecycleError::new("Tabs are not supported for Docker worktrees", 409));
        }
        let agent = self.resolve_agent_definition(Some(&meta.agent))?;
        let agent_kind = discoverable_agent_kind(&agent).ok_or_else(|| {
            LifecycleError::new(
                "Tabs are only available for the built-in Claude and Codex agents",
                409,
            )
        })?;

        let initialized =
            self.refresh_managed_artifacts_from_meta(&resolved.git_dir, &meta, &resolved.entry.path)?;
        let worktree_path = resolved.entry.path.clone();
        Ok(TabContext {
            meta: initialized.meta.clone(),
            resolved,
            initialized,
            worktree_path,
            agent,
            agent_kind,
            profile,
            session_name,
            window_name,
            parking_window: build_worktree_parking_window_name(branch),
        })
    }

    /// Resolve the root tab's session id, discovering and persisting it on first
    /// use (safe because at first fork the root is the newest session for the cwd).
    fn ensure_root_session_id(&self, ctx: &TabContext) -> Result<Option<String>, LifecycleError> {
        let root = root_tab(&ctx.meta);
        if let Some(session_id) = root.as_ref().and_then(|r| r.session_id.clone()) {
            return Ok(Some(session_id));
        }
        let discovered = list_session_ids(ctx.agent_kind, &ctx.worktree_path)
            .into_iter()
            .next();
        if let (Some(discovered), Some(root)) = (discovered.clone(), root) {
            let next = update_tab(
                ctx.meta.clone(),
                &root.tab_id,
                TabPatch { session_id: Some(Some(discovered)), pane_id: None },
            );
            op(write_worktree_meta(&ctx.resolved.git_dir, &next))?;
        }
        Ok(discovered)
    }

    /// Rebuild parked panes for persisted fork tabs after a worktree's window is
    /// recreated, recapture the (ephemeral) pane ids, and restore the previously
    /// active tab on-screen. No-op unless the worktree has fork tabs.
    fn restore_worktree_tabs(
        &self,
        branch: &str,
        git_dir: &str,
        worktree_path: &str,
        profile: &ResolvedProfile,
        agent: &AgentDefinition,
        runtime_env_path: &str,
    ) -> Result<(), LifecycleError> {
        if profile.profile.runtime == RuntimeKind::Docker || agent.kind != "builtin" {
            return Ok(());
        }
        let Some(meta) = read_worktree_meta(git_dir) else {
            return Ok(());
        };
        let Some(root) = root_tab(&meta) else {
            return Ok(());
        };
        // Nothing to rebuild for the common (root-only) case.
        if !list_tabs(&meta).iter().any(|t| t.kind == WorktreeTabKind::Fork) {
            return Ok(());
        }

        let session_name = build_project_session_name(&self.project_root);
        let window_name = build_worktree_window_name(branch);
        let parking_window = build_worktree_parking_window_name(branch);
        // A parking window may still exist (agent-terminal refresh without a full
        // close/reopen): tear it down so we rebuild from a clean slate.
        let _ = self.tmux.kill_window(&session_name, &parking_window);
        let visible_slot = format!("{session_name}:{window_name}.0");
        // Capture the visible slot's pane id once: it is the root's on-screen pane
        // and, if a fork is restored on top, the swap target.
        let visible_slot_pane_id = self.tmux.get_pane_id(&visible_slot).ok();

        let mut restored: Vec<WorktreeTab> = vec![WorktreeTab {
            pane_id: visible_slot_pane_id.clone(),
            ..root
        }];
        for fork in list_tabs(&meta).into_iter().filter(|t| t.kind == WorktreeTabKind::Fork) {
            let Some(session_id) = fork.session_id.clone() else {
                continue; // cannot resume an unknown session — drop it
            };
            let invocation = AgentInvocation {
                agent,
                yolo: profile.profile.yolo == Some(true),
                system_prompt: None,
                prompt: None,
                launch_mode: AgentLaunchMode::Resume,
                worktree_path,
                repo_root: &self.project_root,
                branch,
                profile_name: &profile.name,
                resume_conversation_id: Some(&session_id),
                fork_from_session_id: None,
                pin_session_id: None,
            };
            let command = build_agent_pane_command(runtime_env_path, &invocation);
            let pane_id = self
                .tmux
                .create_parked_pane(
                    &session_name,
                    &parking_window,
                    worktree_path,
                    &build_managed_shell_command(runtime_env_path),
                )
                .map_err(|e| LifecycleError::new(e, 422))?;
            op(self.tmux.run_command(&pane_id, &command))?;
            restored.push(WorktreeTab { pane_id: Some(pane_id), ..fork });
        }

        let mut next_meta = with_tabs(meta.clone(), restored.clone());
        let want_active = read_active_tab_id(&meta);
        let active_tab = restored.iter().find(|t| {
            t.tab_id == want_active && t.kind == WorktreeTabKind::Fork && t.pane_id.is_some()
        });
        if let Some(active) = active_tab
            && let (Some(active_pane), Some(slot_pane)) =
                (active.pane_id.clone(), visible_slot_pane_id.clone())
        {
            op(self.tmux.swap_panes(&active_pane, &slot_pane))?;
            next_meta = set_active_tab(next_meta, &active.tab_id);
        } else {
            next_meta = set_active_tab(next_meta, ROOT_TAB_ID);
        }
        op(write_worktree_meta(git_dir, &next_meta))
    }

    // --- helpers ---

    /// Live, non-bare worktrees excluding the repo root itself.
    fn list_project_worktrees(&self) -> Vec<GitWorktreeEntry> {
        let root = canonical(&self.project_root);
        self.git
            .list_live_worktrees(&root)
            .into_iter()
            .filter(|entry| !entry.bare && canonical(&entry.path) != root)
            .collect()
    }

    fn resolve_existing_worktree(&self, branch: &str) -> Result<ResolvedWorktree, LifecycleError> {
        let entry = self
            .list_project_worktrees()
            .into_iter()
            .find(|candidate| candidate.branch.as_deref() == Some(branch))
            .ok_or_else(|| LifecycleError::new(format!("Worktree not found: {branch}"), 404))?;

        let git_dir = self
            .git
            .resolve_worktree_git_dir(&entry.path)
            .map_err(|e| LifecycleError::new(e, 422))?;
        let meta = read_worktree_meta(&git_dir);
        Ok(ResolvedWorktree { entry, git_dir, meta })
    }

    /// Tear down a worktree's main window and its hidden tab-parking window.
    fn kill_worktree_windows(&self, branch: &str) -> Result<(), LifecycleError> {
        let session = build_project_session_name(&self.project_root);
        op(self.tmux.kill_window(&session, &build_worktree_window_name(branch)))?;
        op(self
            .tmux
            .kill_window(&session, &build_worktree_parking_window_name(branch)))
    }

    fn close_branch_window(&self, branch: &str) -> Result<(), LifecycleError> {
        self.kill_worktree_windows(branch)?;
        self.reconcile_force();
        Ok(())
    }

    fn ensure_no_uncommitted_changes(&self, entry: &GitWorktreeEntry) -> Result<(), LifecycleError> {
        if self.git.read_worktree_status(&entry.path).dirty {
            let name = entry.branch.clone().unwrap_or_else(|| entry.path.clone());
            return Err(LifecycleError::new(
                format!("Worktree has uncommitted changes: {name}"),
                409,
            ));
        }
        Ok(())
    }

    fn remove_resolved_worktree(&self, resolved: &ResolvedWorktree) -> Result<(), LifecycleError> {
        self.run_pre_remove_hook(resolved)?;

        let branch = resolved
            .entry
            .branch
            .clone()
            .unwrap_or_else(|| resolved.entry.path.clone());

        if resolved.meta.as_ref().map(|m| m.runtime.as_str()) == Some("docker") {
            crate::adapters::docker::remove_container(&branch);
        }

        self.kill_worktree_windows(&branch)?;
        op(self
            .git
            .remove_worktree(&self.project_root, &resolved.entry.path, true))?;
        // Best-effort branch delete (force) — matches legacy deleteBranch: true.
        let _ = self.git.delete_branch(&self.project_root, &branch, true);
        self.update_worktree_archived_state(&resolved.entry.path, false)?;
        self.reconcile_force();
        Ok(())
    }

    fn run_pre_remove_hook(&self, resolved: &ResolvedWorktree) -> Result<(), LifecycleError> {
        self.run_hook(
            "preRemove",
            self.config.lifecycle_hooks.pre_remove.as_deref(),
            resolved.meta.as_ref(),
            &resolved.entry.path,
        )
    }

    /// Run a lifecycle hook (`postCreate`/`preRemove`), a no-op when the command
    /// or managed metadata is absent.
    fn run_hook(
        &self,
        name: &str,
        command: Option<&str>,
        meta: Option<&WorktreeMeta>,
        worktree_path: &str,
    ) -> Result<(), LifecycleError> {
        let (Some(command), Some(meta)) = (command, meta) else {
            return Ok(());
        };
        let dotenv = load_dotenv_local(worktree_path);
        let mut extra = std::collections::HashMap::new();
        extra.insert("SEBENZA_WORKTREE_PATH".to_string(), worktree_path.to_string());
        let env = build_runtime_env_map(meta, &extra, &dotenv);
        op(run_lifecycle_hook(RunLifecycleHookInput {
            name,
            command,
            cwd: worktree_path,
            env: &env,
        }))
    }

    /// Read → transform → write the project archive state (only if it changed).
    fn update_worktree_archived_state(
        &self,
        path: &str,
        archived: bool,
    ) -> Result<(), LifecycleError> {
        let project_git_dir = self
            .git
            .resolve_worktree_git_dir(&self.project_root)
            .map_err(|e| LifecycleError::new(e, 422))?;
        let state = read_worktree_archive_state(&project_git_dir);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let next = set_archived_worktree_state(&state, path, archived, &now);
        if !archive_states_equal(&state, &next) {
            op(write_worktree_archive_state(&project_git_dir, &next))?;
        }
        Ok(())
    }

    fn reconcile_force(&self) {
        self.reconciliation
            .reconcile(&self.project_root, &self.runtime, true);
    }

    fn control_url(&self, _runtime: RuntimeKind) -> String {
        // Docker loopback rewriting is deferred; host uses the base URL as-is.
        format!("{}/api/runtime/events", self.control_base_url.trim_end_matches('/'))
    }

    /// Resolve the branch name for a create: explicit (trimmed), else auto-name
    /// (deferred — always `None` for now), else a random fallback.
    fn resolve_branch(
        &self,
        raw_branch: Option<&str>,
        prompt: Option<&str>,
        mode: CreateMode,
    ) -> Result<String, LifecycleError> {
        let explicit = raw_branch.map(str::trim).filter(|b| !b.is_empty());
        let branch = if mode == CreateMode::Existing {
            explicit.map(str::to_string)
        } else {
            // New worktree: explicit branch, else auto-name from the prompt, else
            // a random fallback.
            Some(match explicit {
                Some(b) => b.to_string(),
                None => self.generate_auto_name(prompt).unwrap_or_else(generate_fallback_branch_name),
            })
        };
        let branch = branch.ok_or_else(|| LifecycleError::new("Existing branch is required", 400))?;
        if !is_valid_branch_name(&branch) {
            return Err(LifecycleError::new(format!("Invalid branch name: {branch}"), 400));
        }
        Ok(branch)
    }

    /// Generate a branch name from the prompt via the configured auto-name model,
    /// or `None` if auto-name is unconfigured, there's no prompt, or it fails.
    fn generate_auto_name(&self, prompt: Option<&str>) -> Option<String> {
        let config = self.config.auto_name.as_ref()?;
        let prompt = prompt.map(str::trim).filter(|p| !p.is_empty())?;
        match generate_branch_name(config, prompt) {
            Ok(branch) => Some(branch),
            Err(err) => {
                tracing::warn!("[auto-name] {err}; using fallback branch");
                None
            }
        }
    }

    fn resolve_branch_availability(
        &self,
        branch: &str,
        mode: CreateMode,
    ) -> Result<BranchAvailability, LifecycleError> {
        let root = canonical(&self.project_root);
        let local: std::collections::HashSet<String> =
            self.git.list_local_branches(&root).into_iter().collect();

        if mode == CreateMode::New {
            if local.contains(branch) {
                return Err(LifecycleError::new(format!("Branch already exists: {branch}"), 409));
            }
            return Ok(BranchAvailability {
                start_point: None,
                delete_branch_on_rollback: false,
            });
        }

        if local.contains(branch) {
            if self.checked_out_branches().contains(branch) {
                return Err(LifecycleError::new(
                    format!("Branch already has a worktree: {branch}"),
                    409,
                ));
            }
            return Ok(BranchAvailability {
                start_point: None,
                delete_branch_on_rollback: false,
            });
        }

        let remote: std::collections::HashSet<String> =
            self.git.list_remote_branches(&root).into_iter().collect();
        if !remote.contains(branch) {
            return Err(LifecycleError::new(format!("Branch not found: {branch}"), 404));
        }
        Ok(BranchAvailability {
            start_point: Some(format!("origin/{branch}")),
            delete_branch_on_rollback: true,
        })
    }

    /// Branches held by any (even stale) worktree registration — blocks reuse.
    fn checked_out_branches(&self) -> std::collections::HashSet<String> {
        self.git
            .list_worktrees(&canonical(&self.project_root))
            .into_iter()
            .filter(|e| !e.bare)
            .filter_map(|e| e.branch)
            .collect()
    }

    fn resolve_profile(&self, profile_name: Option<&str>) -> Result<ResolvedProfile, LifecycleError> {
        let name = profile_name
            .map(str::to_string)
            .unwrap_or_else(|| get_default_profile_name(&self.config));
        let profile = self
            .config
            .profiles
            .get(&name)
            .ok_or_else(|| LifecycleError::new(format!("Unknown profile: {name}"), 400))?;
        Ok(ResolvedProfile {
            name,
            profile: profile.clone(),
        })
    }

    fn resolve_agent_definition(
        &self,
        agent_id: Option<&str>,
    ) -> Result<AgentDefinition, LifecycleError> {
        let resolved = agent_id
            .map(str::to_string)
            .unwrap_or_else(|| self.config.workspace.default_agent.clone());
        get_agent_definition(&self.config, &resolved)
            .ok_or_else(|| LifecycleError::new(format!("Unknown agent: {resolved}"), 400))
    }

    fn resolve_selected_agents(
        &self,
        input: &CreateWorktreesInput,
    ) -> Result<Vec<String>, LifecycleError> {
        let selected: Vec<String> = match &input.agents {
            Some(agents) if !agents.is_empty() => agents.clone(),
            _ => vec![input
                .agent
                .clone()
                .unwrap_or_else(|| self.config.workspace.default_agent.clone())],
        };
        let mut seen = std::collections::HashSet::new();
        let mut deduped: Vec<String> = Vec::new();
        for agent in selected {
            let trimmed = agent.trim().to_string();
            if !trimmed.is_empty() && seen.insert(trimmed.clone()) {
                deduped.push(trimmed);
            }
        }
        if deduped.is_empty() {
            return Err(LifecycleError::new("At least one agent must be selected", 400));
        }
        deduped
            .iter()
            .map(|id| self.resolve_agent_definition(Some(id)).map(|a| a.id))
            .collect()
    }

    fn build_startup_env_values(
        &self,
        env_overrides: Option<&HashMap<String, String>>,
    ) -> Result<HashMap<String, String>, LifecycleError> {
        let mut values = self.config.startup_envs.clone();
        if let Some(overrides) = env_overrides {
            for (key, value) in overrides {
                if !is_valid_env_key(key) {
                    return Err(LifecycleError::new(
                        format!("Invalid env override key: {key}"),
                        400,
                    ));
                }
                values.insert(key.clone(), value.clone());
            }
        }
        Ok(values)
    }

    fn allocate_ports(&self) -> HashMap<String, u16> {
        let metas: Vec<WorktreeMeta> = self
            .list_project_worktrees()
            .iter()
            .filter_map(|entry| {
                self.git
                    .resolve_worktree_git_dir(&entry.path)
                    .ok()
                    .and_then(|git_dir| read_worktree_meta(&git_dir))
            })
            .collect();
        allocate_service_ports(&metas, &self.config.services)
    }

    fn resolve_worktree_path(&self, branch: &str) -> String {
        Path::new(&self.project_root)
            .join(&self.config.workspace.worktree_root)
            .join(branch)
            .to_string_lossy()
            .to_string()
    }
}

fn archive_states_equal(
    a: &crate::domain::model::WorktreeArchiveState,
    b: &crate::domain::model::WorktreeArchiveState,
) -> bool {
    a.schema_version == b.schema_version
        && a.entries.len() == b.entries.len()
        && a.entries.iter().zip(&b.entries).all(|(l, r)| {
            l.path == r.path && l.archived_at == r.archived_at
        })
}

fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// Trim and validate a worktree label. Empty → `None`; over the length cap → 400.
fn normalize_worktree_label(label: Option<&str>) -> Result<Option<String>, LifecycleError> {
    let trimmed = label.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_WORKTREE_LABEL_LENGTH {
        return Err(LifecycleError::new(
            format!("Worktree label must be {MAX_WORKTREE_LABEL_LENGTH} characters or fewer"),
            400,
        ));
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_normalization() {
        assert_eq!(normalize_worktree_label(None).unwrap(), None);
        assert_eq!(normalize_worktree_label(Some("   ")).unwrap(), None);
        assert_eq!(
            normalize_worktree_label(Some("  hi  ")).unwrap(),
            Some("hi".to_string())
        );
        let long = "x".repeat(81);
        let err = normalize_worktree_label(Some(&long)).unwrap_err();
        assert_eq!(err.status, 400);
    }
}
