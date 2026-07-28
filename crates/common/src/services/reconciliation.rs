use crate::adapters::fs::{build_runtime_env_map, read_worktree_meta, read_worktree_prs};
use crate::adapters::git::{GitGateway, GitWorktreeEntry, canonical_path, split_repo_root_entry};
use crate::adapters::tmux::{
    TmuxGateway, TmuxWindowSummary, build_project_session_name, build_worktree_window_name,
};
use crate::config::expand_template;
use crate::domain::config::ProjectConfig;
use crate::domain::model::{
    GitWorktreeRuntimeState, ServiceRuntimeState, WorktreeKind, WorktreeMeta, WorktreeSource,
};
use crate::services::project_runtime::{ProjectRuntime, UpsertInput};
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn make_unmanaged_worktree_id(path: &str) -> String {
    format!("unmanaged:{}", canonical_path(path))
}

/// Runtime identity of the repo root. Derived from the path alone — deliberately
/// not from `meta.json` — so a missing or corrupt meta cannot churn the runtime
/// map or the terminal attach-id namespace.
pub(crate) fn make_main_worktree_id(normalized_repo_root: &str) -> String {
    format!("main:{normalized_repo_root}")
}

/// Build the main checkout's runtime entry.
///
/// The branch key is ALWAYS the configured main branch, never the checkout's live
/// HEAD. The tmux window name derives from this key, so if the user runs
/// `git checkout feature/x` inside the main session, trusting HEAD would re-key
/// the row, orphan the window and break the live terminal WebSocket.
fn build_main_upsert(
    normalized_repo_root: &str,
    root_path: &str,
    main_branch: &str,
    meta: Option<&WorktreeMeta>,
) -> UpsertInput {
    UpsertInput {
        worktree_id: make_main_worktree_id(normalized_repo_root),
        kind: WorktreeKind::Main,
        branch: main_branch.to_string(),
        label: meta.and_then(|m| m.label.clone()),
        // The trunk has no parent; a base branch here would make the sidebar tree
        // try to nest the main row beneath itself.
        base_branch: None,
        path: root_path.to_string(),
        // No profile semantics for the repo root: no service ports, no system prompt.
        profile: None,
        // No agent ever runs on the main checkout, so the UI shows no agent status
        // and the in-app chat routes correctly refuse it.
        agent_name: None,
        agent_terminal_stale: false,
        runtime: "host".to_string(),
        source: WorktreeSource::Ui,
        oneshot: None,
        tabs: meta.and_then(|m| m.tabs.clone()).unwrap_or_default(),
        active_tab_id: meta.and_then(|m| m.active_tab_id.clone()),
    }
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

        let normalized_repo_root = canonical_path(repo_root);
        self.run_reconcile(&normalized_repo_root, runtime);
        *self.last_reconciled.lock().unwrap() = Some(Instant::now());
    }

    fn run_reconcile(&self, normalized_repo_root: &str, runtime: &Arc<Mutex<ProjectRuntime>>) {
        let worktrees = self.git.list_live_worktrees(normalized_repo_root);
        let session_name = build_project_session_name(normalized_repo_root);
        let windows: Vec<TmuxWindowSummary> = self.tmux.list_windows().unwrap_or_default();

        // The repo root is split off rather than filtered inline: it is a real,
        // openable session but not a linked worktree, and it must not flow through
        // the loop below, which derives branch/agent/services from per-worktree
        // meta that does not apply to it.
        let (root_entry, candidates) = split_repo_root_entry(worktrees, normalized_repo_root);

        let mut seen: HashSet<String> = HashSet::new();
        let mut rt = runtime.lock().unwrap();

        if let Some(root) = root_entry.as_ref() {
            let git_dir = self.git.resolve_worktree_git_dir(&root.path).ok();
            let meta = git_dir.as_deref().and_then(read_worktree_meta);
            let input = build_main_upsert(
                normalized_repo_root,
                &root.path,
                &self.config.workspace.main_branch,
                meta.as_ref(),
            );
            let worktree_id = input.worktree_id.clone();
            let branch = input.branch.clone();
            let git_status = self.git.read_worktree_status(&root.path);
            let window = windows.iter().find(|w| {
                w.session_name == session_name
                    && w.window_name == build_worktree_window_name(&branch)
            });

            seen.insert(worktree_id.clone());
            rt.upsert_worktree(input);
            rt.set_git_state(
                &worktree_id,
                GitWorktreeRuntimeState {
                    exists: true,
                    branch,
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
            // The repo root allocates no service ports; leaving these empty also
            // keeps a stray `prs.json` under `.git/` from rendering on the row.
            rt.set_services(&worktree_id, Vec::new());
            rt.set_prs(&worktree_id, Vec::new());
        }

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
            let prs = git_dir
                .as_deref()
                .map(read_worktree_prs)
                .unwrap_or_default();

            seen.insert(worktree_id.clone());

            rt.upsert_worktree(UpsertInput {
                worktree_id: worktree_id.clone(),
                kind: WorktreeKind::Linked,
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
                runtime: meta
                    .as_ref()
                    .map(|m| m.runtime.clone())
                    .unwrap_or_else(|| "host".to_string()),
                source: meta
                    .as_ref()
                    .and_then(|m| m.source.clone())
                    .unwrap_or(WorktreeSource::Ui),
                oneshot: meta.as_ref().and_then(|m| m.oneshot.clone()),
                tabs: meta
                    .as_ref()
                    .and_then(|m| m.tabs.clone())
                    .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::WorktreeTabKind;

    fn meta(branch: &str, agent: &str) -> WorktreeMeta {
        WorktreeMeta {
            schema_version: 1,
            worktree_id: "persisted-id".to_string(),
            branch: branch.to_string(),
            label: Some("Trunk".to_string()),
            base_branch: Some("main".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            profile: "default".to_string(),
            agent: agent.to_string(),
            runtime: "host".to_string(),
            startup_env_values: HashMap::new(),
            allocated_ports: HashMap::from([("WEB_PORT".to_string(), 4000u16)]),
            source: None,
            oneshot: None,
            conversation: None,
            agent_terminal_stale: Some(true),
            tabs: Some(vec![crate::domain::model::WorktreeTab {
                tab_id: "root".to_string(),
                kind: WorktreeTabKind::Root,
                label: "Root".to_string(),
                seq: None,
                session_id: None,
                pane_id: Some("%1".to_string()),
                agent: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }]),
            active_tab_id: Some("root".to_string()),
            fork_counter: Some(0),
        }
    }

    #[test]
    fn main_entry_is_keyed_by_config_main_branch_not_by_head() {
        // The user checked something else out inside the main session. The runtime
        // key must stay `main`, because the tmux window name derives from it — a
        // re-key would orphan the window and break the live terminal WebSocket.
        let input = build_main_upsert("/repo", "/repo", "main", Some(&meta("feature/x", "claude")));
        assert_eq!(input.branch, "main");
        assert_eq!(input.kind, WorktreeKind::Main);
    }

    #[test]
    fn main_worktree_id_is_derived_from_the_path_not_from_meta() {
        // Stable across a missing or rewritten meta.json.
        let with_meta = build_main_upsert("/repo", "/repo", "main", Some(&meta("main", "claude")));
        let without_meta = build_main_upsert("/repo", "/repo", "main", None);
        assert_eq!(with_meta.worktree_id, without_meta.worktree_id);
        assert_eq!(with_meta.worktree_id, "main:/repo");
        // ...and not the id persisted in meta.json.
        assert_ne!(with_meta.worktree_id, "persisted-id");
    }

    #[test]
    fn main_entry_has_no_agent_no_profile_and_no_base_branch() {
        // Even though meta.json carries all three, they are wrong for the trunk:
        // no agent runs there, it allocates no ports, and it has no parent row.
        let input = build_main_upsert("/repo", "/repo", "main", Some(&meta("main", "claude")));
        assert_eq!(input.agent_name, None);
        assert_eq!(input.profile, None);
        assert_eq!(input.base_branch, None);
        assert!(input.oneshot.is_none());
        assert_eq!(input.runtime, "host");
        assert!(!input.agent_terminal_stale);
    }

    #[test]
    fn main_entry_keeps_its_tabs_and_label_from_meta() {
        // Tabs and the label are the parts of meta.json that do apply.
        let input = build_main_upsert("/repo", "/repo", "main", Some(&meta("main", "claude")));
        assert_eq!(input.label.as_deref(), Some("Trunk"));
        assert_eq!(input.tabs.len(), 1);
        assert_eq!(input.active_tab_id.as_deref(), Some("root"));
    }

    #[test]
    fn main_entry_works_without_meta_at_all() {
        // First open, before ensure_main_repo_meta has written anything.
        let input = build_main_upsert("/repo", "/repo", "trunk", None);
        assert_eq!(input.branch, "trunk");
        assert!(input.tabs.is_empty());
        assert_eq!(input.active_tab_id, None);
        assert_eq!(input.label, None);
    }

    #[test]
    fn split_off_root_is_not_processed_as_a_linked_worktree() {
        // Guards the reconcile partition: the root must never reach the loop that
        // derives agent/services from per-worktree meta.
        let entries = crate::adapters::git::parse_git_worktree_porcelain(
            "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
             worktree /repo/wt/a\nHEAD def\nbranch refs/heads/feat-a\n",
        );
        let (root, linked) = split_repo_root_entry(entries, "/repo");
        assert_eq!(root.unwrap().path, "/repo");
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].path, "/repo/wt/a");
    }
}
