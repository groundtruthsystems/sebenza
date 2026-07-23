use crate::adapters::fs::{build_runtime_env_map, read_worktree_meta, read_worktree_prs};
use crate::adapters::git::{GitGateway, GitWorktreeEntry};
use crate::adapters::tmux::{build_project_session_name, build_worktree_window_name, TmuxGateway, TmuxWindowSummary};
use crate::config::expand_template;
use crate::domain::config::ProjectConfig;
use crate::domain::model::{GitWorktreeRuntimeState, ServiceRuntimeState, WorktreeSource};
use crate::services::project_runtime::{ProjectRuntime, UpsertInput};
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn make_unmanaged_worktree_id(path: &str) -> String {
    let resolved = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());
    format!("unmanaged:{resolved}")
}

fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn resolve_branch(entry: &GitWorktreeEntry, meta_branch: Option<&str>) -> String {
    if let Some(branch) = &entry.branch {
        return branch.clone();
    }
    if let Some(branch) = meta_branch {
        return branch.to_string();
    }
    let fallback = Path::new(&entry.path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if fallback.is_empty() {
        "unknown".to_string()
    } else {
        fallback
    }
}

/// TCP probe: is something listening on 127.0.0.1:port?
fn is_listening(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

/// Reconciles git worktrees + tmux windows + `.ai/sebenza` meta into the ProjectRuntime.
/// Throttled to at most once per `freshness` and single-flighted via the shared lock.
pub struct ReconciliationService {
    config: ProjectConfig,
    git: GitGateway,
    tmux: TmuxGateway,
    freshness: Duration,
    last_reconciled: Mutex<Option<Instant>>,
}

impl ReconciliationService {
    pub fn new(config: ProjectConfig, git: GitGateway, tmux: TmuxGateway) -> Self {
        ReconciliationService {
            config,
            git,
            tmux,
            freshness: Duration::from_millis(500),
            last_reconciled: Mutex::new(None),
        }
    }

    pub fn reconcile(&self, repo_root: &str, runtime: &Arc<Mutex<ProjectRuntime>>, force: bool) {
        {
            let last = self.last_reconciled.lock().unwrap();
            if !force {
                if let Some(instant) = *last {
                    if instant.elapsed() < self.freshness {
                        return;
                    }
                }
            }
        }

        let normalized_repo_root = canonical(repo_root);
        self.run_reconcile(&normalized_repo_root, runtime);
        *self.last_reconciled.lock().unwrap() = Some(Instant::now());
    }

    fn run_reconcile(&self, normalized_repo_root: &str, runtime: &Arc<Mutex<ProjectRuntime>>) {
        let worktrees = self.git.list_live_worktrees(normalized_repo_root);
        let session_name = build_project_session_name(normalized_repo_root);
        let windows: Vec<TmuxWindowSummary> = self.tmux.list_windows().unwrap_or_default();

        let candidates: Vec<GitWorktreeEntry> = worktrees
            .into_iter()
            .filter(|entry| !entry.bare && canonical(&entry.path) != normalized_repo_root)
            .collect();

        let mut seen: HashSet<String> = HashSet::new();
        let mut rt = runtime.lock().unwrap();

        for entry in &candidates {
            let git_dir = self.git.resolve_worktree_git_dir(&entry.path).ok();
            let meta = git_dir.as_deref().and_then(read_worktree_meta);
            let branch = resolve_branch(entry, meta.as_ref().map(|m| m.branch.as_str()));
            let worktree_id = meta
                .as_ref()
                .map(|m| m.worktree_id.clone())
                .unwrap_or_else(|| make_unmanaged_worktree_id(&entry.path));
            let git_status = self.git.read_worktree_status(&entry.path);
            let window = windows.iter().find(|w| {
                w.session_name == session_name
                    && w.window_name == build_worktree_window_name(&branch)
            });

            let services = match &meta {
                Some(meta) => self.build_service_states(meta, &branch),
                None => Vec::new(),
            };
            let prs = git_dir.as_deref().map(read_worktree_prs).unwrap_or_default();

            seen.insert(worktree_id.clone());

            rt.upsert_worktree(UpsertInput {
                worktree_id: worktree_id.clone(),
                branch: branch.clone(),
                label: meta.as_ref().and_then(|m| m.label.clone()),
                base_branch: meta.as_ref().and_then(|m| m.base_branch.clone()),
                path: entry.path.clone(),
                profile: meta.as_ref().map(|m| m.profile.clone()),
                agent_name: meta.as_ref().map(|m| m.agent.clone()),
                agent_terminal_stale: meta
                    .as_ref()
                    .and_then(|m| m.agent_terminal_stale)
                    .unwrap_or(false),
                runtime: meta.as_ref().map(|m| m.runtime.clone()).unwrap_or_else(|| "host".to_string()),
                source: meta.as_ref().and_then(|m| m.source.clone()).unwrap_or(WorktreeSource::Ui),
                oneshot: meta.as_ref().and_then(|m| m.oneshot.clone()),
                tabs: meta.as_ref().and_then(|m| m.tabs.clone()).unwrap_or_default(),
                active_tab_id: meta.as_ref().and_then(|m| m.active_tab_id.clone()),
            });

            rt.set_git_state(
                &worktree_id,
                GitWorktreeRuntimeState {
                    exists: true,
                    branch: branch.clone(),
                    dirty: git_status.dirty,
                    ahead_count: git_status.ahead_count,
                    current_commit: git_status.current_commit,
                },
            );

            rt.set_session_state(
                &worktree_id,
                window.is_some(),
                window.map(|w| w.session_name.clone()),
                window.map(|w| w.pane_count).unwrap_or(0),
            );

            rt.set_services(&worktree_id, services);
            rt.set_prs(&worktree_id, prs);
        }

        for state in rt.list_worktrees() {
            if !seen.contains(&state.worktree_id) {
                rt.remove_worktree(&state.worktree_id);
            }
        }
    }

    fn build_service_states(
        &self,
        meta: &crate::domain::model::WorktreeMeta,
        branch: &str,
    ) -> Vec<ServiceRuntimeState> {
        let runtime_env = build_runtime_env_map(meta, &HashMap::new(), &HashMap::new());
        self.config
            .services
            .iter()
            .map(|service| {
                let port = meta.allocated_ports.get(&service.port_env).copied();
                let running = port.map(is_listening).unwrap_or(false);
                let url = match (&port, &service.url_template) {
                    (Some(_), Some(template)) => Some(expand_template(template, &runtime_env)),
                    _ => None,
                };
                let _ = branch;
                ServiceRuntimeState {
                    name: service.name.clone(),
                    port,
                    running,
                    url,
                }
            })
            .collect()
    }
}
