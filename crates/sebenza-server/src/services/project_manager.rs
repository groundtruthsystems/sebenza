//! Multi-project manager — port of `backend-legacy/src/services/project-manager.ts`
//! plus the per-project `ProjectApp` (the state a single project's routes need).
//! One Sebenza process serves every known project on one port, keyed by URL prefix.

use crate::adapters::git::GitGateway;
use crate::adapters::projects_registry::{ProjectEntry, ProjectsRegistry};
use crate::adapters::tmux::TmuxGateway;
use crate::config::{load_config, project_root};
use crate::domain::config::ProjectConfig;
use crate::domain::policies::derive_project_prefix;
use crate::services::lifecycle_service::LifecycleService;
use crate::services::project_runtime::ProjectRuntime;
use crate::services::reconciliation::ReconciliationService;
use indexmap::IndexMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Everything a single project's routes operate on.
pub struct ProjectApp {
    pub prefix: String,
    pub path: String,
    pub added_at: u64,
    pub config: RwLock<Arc<ProjectConfig>>,
    pub runtime: Arc<Mutex<ProjectRuntime>>,
    pub reconciliation: Arc<ReconciliationService>,
    pub git: GitGateway,
    pub tmux: TmuxGateway,
    /// Branches with an in-flight mutating operation (409 on concurrent conflict).
    pub busy: Arc<Mutex<HashSet<String>>>,
    /// True while a client has a WebSocket open on this project (drives loops).
    pub active: AtomicBool,
    /// Last per-project API request time (gates the PR monitor to viewed projects).
    last_activity: Mutex<Option<std::time::Instant>>,
    /// Last successful auto-pull time (gates the auto-pull interval).
    last_pull: Mutex<Option<std::time::Instant>>,
    /// Dashboard notifications recorded from runtime events.
    pub notifications: NotificationStore,
    control_base_url: String,
}

/// In-memory ring of dashboard notifications (max 50), per project.
#[derive(Default)]
pub struct NotificationStore {
    items: Mutex<Vec<crate::domain::model::NotificationView>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl NotificationStore {
    pub fn list(&self) -> Vec<crate::domain::model::NotificationView> {
        self.items.lock().unwrap().clone()
    }

    pub fn dismiss(&self, id: i64) -> bool {
        let mut items = self.items.lock().unwrap();
        let before = items.len();
        items.retain(|n| n.id != id);
        items.len() != before
    }

    /// Record a runtime event as a notification, if it maps to one.
    pub fn record_event(
        &self,
        event: &crate::domain::events::RuntimeEvent,
    ) -> Option<crate::domain::model::NotificationView> {
        use crate::domain::events::RuntimeEvent;
        let (kind, message, url) = match event {
            RuntimeEvent::AgentStopped { branch, .. } => {
                ("agent_stopped", format!("Agent stopped on {branch}"), None)
            }
            RuntimeEvent::PrOpened { branch, url, .. } => {
                ("pr_opened", format!("PR opened on {branch}"), url.clone())
            }
            RuntimeEvent::RuntimeError { branch, message, .. } => {
                ("runtime_error", format!("Runtime error on {branch}: {message}"), None)
            }
            RuntimeEvent::AgentStatusChanged { .. } => return None,
        };
        let id = (self.next_id.fetch_add(1, Ordering::Relaxed) + 1) as i64;
        let notification = crate::domain::model::NotificationView {
            id,
            branch: event.branch().to_string(),
            r#type: kind.to_string(),
            message,
            url,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        let mut items = self.items.lock().unwrap();
        items.push(notification.clone());
        while items.len() > 50 {
            items.remove(0);
        }
        Some(notification)
    }
}

impl ProjectApp {
    /// A cheap snapshot of the current config (clones the inner `Arc`).
    pub fn config(&self) -> Arc<ProjectConfig> {
        self.config.read().unwrap().clone()
    }

    /// Replace the live config (after persisting an agents/settings change).
    pub fn set_config(&self, config: ProjectConfig) {
        *self.config.write().unwrap() = Arc::new(config);
    }

    /// Display name from the current config.
    pub fn name(&self) -> String {
        self.config().name.clone()
    }

    pub fn lifecycle(&self) -> LifecycleService {
        LifecycleService::new(
            self.path.clone(),
            self.config(),
            self.git.clone(),
            self.tmux.clone(),
            self.reconciliation.clone(),
            self.runtime.clone(),
            self.control_base_url.clone(),
        )
    }

    /// Record a per-project API request (for the PR monitor's activity gate).
    pub fn touch(&self) {
        *self.last_activity.lock().unwrap() = Some(std::time::Instant::now());
    }

    /// Whether a client is viewing this project (WS open, or a request within `within`).
    pub fn active_recently(&self, within: std::time::Duration) -> bool {
        self.active.load(Ordering::Relaxed)
            || self
                .last_activity
                .lock()
                .unwrap()
                .map(|t| t.elapsed() < within)
                .unwrap_or(false)
    }

    /// Whether the auto-pull interval has elapsed since the last pull.
    pub fn pull_due(&self, interval: std::time::Duration) -> bool {
        self.last_pull
            .lock()
            .unwrap()
            .map(|t| t.elapsed() >= interval)
            .unwrap_or(true)
    }

    pub fn mark_pulled(&self) {
        *self.last_pull.lock().unwrap() = Some(std::time::Instant::now());
    }
}

/// Build a `ProjectApp` for a resolved project root.
fn create_project_app(prefix: String, root: String, control_base_url: String, added_at: u64) -> Arc<ProjectApp> {
    let config = load_config(&root);
    let git = GitGateway::new();
    let tmux = TmuxGateway::new();
    let runtime = Arc::new(Mutex::new(ProjectRuntime::new()));
    let reconciliation = Arc::new(ReconciliationService::new(config.clone(), git.clone(), tmux.clone()));
    Arc::new(ProjectApp {
        prefix,
        path: root,
        added_at,
        config: RwLock::new(Arc::new(config)),
        runtime,
        reconciliation,
        git,
        tmux,
        busy: Arc::new(Mutex::new(HashSet::new())),
        active: AtomicBool::new(false),
        last_activity: Mutex::new(None),
        last_pull: Mutex::new(None),
        notifications: NotificationStore::default(),
        control_base_url,
    })
}

/// Owns the set of projects served by this process, keyed by URL prefix, plus the
/// registry that persists which projects are known across restarts.
pub struct ProjectManager {
    projects: Mutex<IndexMap<String, Arc<ProjectApp>>>,
    registry: ProjectsRegistry,
    control_base_url: String,
    /// Monotonic counter standing in for `Date.now()` on `added_at` (env has no clock).
    added_seq: std::sync::atomic::AtomicU64,
}

impl ProjectManager {
    pub fn new(registry: ProjectsRegistry, control_base_url: String) -> Self {
        ProjectManager {
            projects: Mutex::new(IndexMap::new()),
            registry,
            control_base_url,
            added_seq: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub fn get(&self, prefix: &str) -> Option<Arc<ProjectApp>> {
        self.projects.lock().unwrap().get(prefix).cloned()
    }

    pub fn list(&self) -> Vec<Arc<ProjectApp>> {
        self.projects.lock().unwrap().values().cloned().collect()
    }

    pub fn get_by_path(&self, path: &str) -> Option<Arc<ProjectApp>> {
        let root = project_root(path);
        self.projects
            .lock()
            .unwrap()
            .values()
            .find(|p| p.path == root)
            .cloned()
    }

    /// Load every persisted project (failures skipped, not fatal).
    pub fn load_persisted(&self) {
        for entry in self.registry.list() {
            self.register(&entry.path, false);
        }
    }

    /// Add (persisting to the registry) or return the existing project for `path`.
    pub fn add(&self, path: &str) -> Arc<ProjectApp> {
        self.register(path, true)
    }

    /// Add for this process only (used for the launch cwd) — not persisted, so
    /// other running servers don't cross-serve it.
    pub fn add_ephemeral(&self, path: &str) -> Arc<ProjectApp> {
        self.register(path, false)
    }

    pub fn remove(&self, prefix: &str) -> bool {
        let removed = self.projects.lock().unwrap().shift_remove(prefix);
        if let Some(app) = removed {
            self.registry.remove(&app.path);
            true
        } else {
            false
        }
    }

    fn register(&self, path: &str, persist: bool) -> Arc<ProjectApp> {
        let root = project_root(path);
        let mut projects = self.projects.lock().unwrap();

        if let Some(existing) = projects.values().find(|p| p.path == root).cloned() {
            if persist {
                self.registry.add(self.entry_for(&existing));
            }
            return existing;
        }

        let taken: Vec<String> = projects.keys().cloned().collect();
        let prefix = derive_project_prefix(&root, taken.iter().map(String::as_str));
        let added_at = self.added_seq.fetch_add(1, Ordering::Relaxed);
        let app = create_project_app(prefix.clone(), root, self.control_base_url.clone(), added_at);
        projects.insert(prefix, app.clone());
        drop(projects);

        if persist {
            self.registry.add(self.entry_for(&app));
        }
        app
    }

    fn entry_for(&self, app: &ProjectApp) -> ProjectEntry {
        ProjectEntry {
            path: app.path.clone(),
            name: app.name(),
            added_at: app.added_at,
        }
    }
}
