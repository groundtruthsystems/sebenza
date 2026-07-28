use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProjectInitPhase {
    CreatingConfig,
    Analyzing,
    Ready,
    Failed,
}

impl ProjectInitPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectInitPhase::CreatingConfig => "creating_config",
            ProjectInitPhase::Analyzing => "analyzing",
            ProjectInitPhase::Ready => "ready",
            ProjectInitPhase::Failed => "failed",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, ProjectInitPhase::Ready | ProjectInitPhase::Failed)
    }
}

#[derive(Clone)]
pub struct ProjectInitState {
    pub path: String,
    pub phase: ProjectInitPhase,
    pub prefix: Option<String>,
    pub name: Option<String>,
    pub error: Option<String>,
    updated_at_ms: u128,
}

const DEFAULT_TERMINAL_TTL: Duration = Duration::from_secs(60);

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// A phase update; `None` fields preserve the prior value (matching the TS
/// `update.x ?? existing?.x ?? null` merge).
#[derive(Default)]
pub struct PhaseUpdate {
    pub prefix: Option<String>,
    pub name: Option<String>,
    pub error: Option<String>,
}

/// Records in-flight and recently-finished setups. Terminal entries are kept
/// briefly (TTL) so a poller can observe the outcome, then evicted; in-flight
/// entries never expire.
pub struct ProjectInitTracker {
    inits: Mutex<HashMap<String, ProjectInitState>>,
    ttl: Duration,
}

impl Default for ProjectInitTracker {
    fn default() -> Self {
        ProjectInitTracker {
            inits: Mutex::new(HashMap::new()),
            ttl: DEFAULT_TERMINAL_TTL,
        }
    }
}

impl ProjectInitTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, path: &str, phase: ProjectInitPhase, update: PhaseUpdate) {
        let mut inits = self.inits.lock().unwrap();
        let existing = inits.get(path);
        let prefix = update
            .prefix
            .or_else(|| existing.and_then(|e| e.prefix.clone()));
        let name = update
            .name
            .or_else(|| existing.and_then(|e| e.name.clone()));
        let error = update.error.or_else(|| {
            if phase == ProjectInitPhase::Failed {
                existing.and_then(|e| e.error.clone())
            } else {
                None
            }
        });
        inits.insert(
            path.to_string(),
            ProjectInitState {
                path: path.to_string(),
                phase,
                prefix,
                name,
                error,
                updated_at_ms: now_ms(),
            },
        );
    }

    /// True while a setup is mid-flight for `path` (not yet ready/failed).
    pub fn is_active(&self, path: &str) -> bool {
        self.inits
            .lock()
            .unwrap()
            .get(path)
            .is_some_and(|s| !s.phase.is_terminal())
    }

    /// Live view: drops terminal entries past their TTL so the map doesn't grow
    /// unbounded across many setups.
    pub fn list(&self) -> Vec<ProjectInitState> {
        let mut inits = self.inits.lock().unwrap();
        let cutoff = now_ms().saturating_sub(self.ttl.as_millis());
        inits.retain(|_, s| !(s.phase.is_terminal() && s.updated_at_ms < cutoff));
        inits.values().cloned().collect()
    }
}

/// Drive an on-add project setup, updating `tracker` so the UI/CLI can watch:
/// scaffold the config → analyze with the agent (best-effort; skipped if
/// unavailable, non-fatal on error so the starter config still ships) →
/// register → ready. A scaffold/register failure is terminal.
pub fn run_project_init(
    tracker: &ProjectInitTracker,
    root: &str,
    analyzer_available: bool,
    scaffold: impl FnOnce() -> Result<(), String>,
    analyze: impl FnOnce() -> Result<(), String>,
    register: impl FnOnce() -> (String, String),
) {
    tracing::info!("[project-init] setting up {root}");
    tracker.set(
        root,
        ProjectInitPhase::CreatingConfig,
        PhaseUpdate::default(),
    );
    if let Err(e) = scaffold() {
        tracing::error!("[project-init] setup failed for {root}: {e}");
        tracker.set(
            root,
            ProjectInitPhase::Failed,
            PhaseUpdate {
                error: Some(e),
                ..Default::default()
            },
        );
        return;
    }

    if analyzer_available {
        tracker.set(root, ProjectInitPhase::Analyzing, PhaseUpdate::default());
        if let Err(e) = analyze() {
            // Best-effort enrichment: keep the starter config and finish setup.
            tracing::warn!(
                "[project-init] analysis failed for {root}, keeping starter config: {e}"
            );
        }
    }

    let (prefix, name) = register();
    tracker.set(
        root,
        ProjectInitPhase::Ready,
        PhaseUpdate {
            prefix: Some(prefix.clone()),
            name: Some(name),
            ..Default::default()
        },
    );
    tracing::info!("[project-init] {root} ready as \"{prefix}\"");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_failure_is_terminal_and_skips_register() {
        let tracker = ProjectInitTracker::new();
        let mut registered = false;
        run_project_init(
            &tracker,
            "/repo",
            true,
            || Err("disk full".to_string()),
            || Ok(()),
            || {
                registered = true;
                ("p".to_string(), "n".to_string())
            },
        );
        assert!(!registered);
        let state = &tracker.list()[0];
        assert_eq!(state.phase.as_str(), "failed");
        assert_eq!(state.error.as_deref(), Some("disk full"));
    }

    #[test]
    fn analysis_failure_is_non_fatal_and_still_registers() {
        let tracker = ProjectInitTracker::new();
        run_project_init(
            &tracker,
            "/repo",
            true,
            || Ok(()),
            || Err("agent crashed".to_string()),
            || ("sebenza".to_string(), "Sebenza".to_string()),
        );
        let state = &tracker.list()[0];
        assert_eq!(state.phase.as_str(), "ready");
        assert_eq!(state.prefix.as_deref(), Some("sebenza"));
    }

    #[test]
    fn analyzer_unavailable_skips_analyze() {
        let tracker = ProjectInitTracker::new();
        let mut analyzed = false;
        run_project_init(
            &tracker,
            "/repo",
            false,
            || Ok(()),
            || {
                analyzed = true;
                Ok(())
            },
            || ("p".to_string(), "n".to_string()),
        );
        assert!(!analyzed);
        assert_eq!(tracker.list()[0].phase.as_str(), "ready");
    }
}
