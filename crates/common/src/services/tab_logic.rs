use crate::domain::model::{WorktreeMeta, WorktreeTab, WorktreeTabKind, ROOT_TAB_ID};

pub fn list_tabs(meta: &WorktreeMeta) -> Vec<WorktreeTab> {
    meta.tabs.clone().unwrap_or_default()
}

pub fn find_tab(meta: &WorktreeMeta, tab_id: &str) -> Option<WorktreeTab> {
    list_tabs(meta).into_iter().find(|tab| tab.tab_id == tab_id)
}

pub fn root_tab(meta: &WorktreeMeta) -> Option<WorktreeTab> {
    let tabs = list_tabs(meta);
    tabs.iter()
        .find(|tab| tab.kind == WorktreeTabKind::Root)
        .cloned()
        .or_else(|| tabs.into_iter().next())
}

pub fn active_tab_id(meta: &WorktreeMeta) -> String {
    meta.active_tab_id
        .clone()
        .unwrap_or_else(|| ROOT_TAB_ID.to_string())
}

/// Next fork number. Monotonic via `fork_counter` so deleting Fork 2 still yields Fork 4.
pub fn next_fork_seq(meta: &WorktreeMeta) -> i32 {
    meta.fork_counter.unwrap_or(0) + 1
}

pub struct ForkTabInput {
    pub seq: i32,
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
    pub created_at: String,
}

pub fn build_fork_tab(input: ForkTabInput) -> WorktreeTab {
    WorktreeTab {
        tab_id: format!("fork-{}", input.seq),
        kind: WorktreeTabKind::Fork,
        label: format!("Fork {}", input.seq),
        seq: Some(input.seq),
        session_id: input.session_id,
        pane_id: input.pane_id,
        created_at: input.created_at,
    }
}

/// Append a fork tab, advance the monotonic counter, and make it active.
pub fn append_tab(mut meta: WorktreeMeta, tab: WorktreeTab) -> WorktreeMeta {
    let mut tabs = list_tabs(&meta);
    let fork_counter = tab.seq.or(meta.fork_counter).unwrap_or(0);
    let tab_id = tab.tab_id.clone();
    tabs.push(tab);
    meta.tabs = Some(tabs);
    meta.fork_counter = Some(fork_counter);
    meta.active_tab_id = Some(tab_id);
    meta
}

/// Remove a tab; if it was active, selection falls back to the root.
pub fn remove_tab(mut meta: WorktreeMeta, tab_id: &str) -> WorktreeMeta {
    let was_active = active_tab_id(&meta) == tab_id;
    let tabs = list_tabs(&meta)
        .into_iter()
        .filter(|tab| tab.tab_id != tab_id)
        .collect();
    meta.tabs = Some(tabs);
    if was_active {
        meta.active_tab_id = Some(ROOT_TAB_ID.to_string());
    }
    meta
}

/// Patch a single tab in place (session id and/or pane id) — only the fields
/// present in `patch` change.
pub struct TabPatch {
    pub session_id: Option<Option<String>>,
    pub pane_id: Option<Option<String>>,
}

pub fn update_tab(mut meta: WorktreeMeta, tab_id: &str, patch: TabPatch) -> WorktreeMeta {
    let tabs = list_tabs(&meta)
        .into_iter()
        .map(|mut tab| {
            if tab.tab_id == tab_id {
                if let Some(session_id) = patch.session_id.clone() {
                    tab.session_id = session_id;
                }
                if let Some(pane_id) = patch.pane_id.clone() {
                    tab.pane_id = pane_id;
                }
            }
            tab
        })
        .collect();
    meta.tabs = Some(tabs);
    meta
}

pub fn set_active_tab(mut meta: WorktreeMeta, tab_id: &str) -> WorktreeMeta {
    meta.active_tab_id = Some(tab_id.to_string());
    meta
}

/// Replace the full tab list (used by the reopen restore path).
pub fn with_tabs(mut meta: WorktreeMeta, tabs: Vec<WorktreeTab>) -> WorktreeMeta {
    meta.tabs = Some(tabs);
    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn meta() -> WorktreeMeta {
        WorktreeMeta {
            schema_version: 1,
            worktree_id: "wt".to_string(),
            branch: "feature".to_string(),
            label: None,
            base_branch: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            profile: "default".to_string(),
            agent: "claude".to_string(),
            runtime: "host".to_string(),
            startup_env_values: HashMap::new(),
            allocated_ports: HashMap::new(),
            source: None,
            oneshot: None,
            conversation: None,
            agent_terminal_stale: None,
            tabs: None,
            active_tab_id: None,
            fork_counter: None,
        }
    }

    #[test]
    fn append_advances_counter_and_activates() {
        let tab = build_fork_tab(ForkTabInput {
            seq: next_fork_seq(&meta()),
            session_id: None,
            pane_id: Some("%3".to_string()),
            created_at: "t".to_string(),
        });
        assert_eq!(tab.tab_id, "fork-1");
        let updated = append_tab(meta(), tab);
        assert_eq!(updated.fork_counter, Some(1));
        assert_eq!(updated.active_tab_id.as_deref(), Some("fork-1"));
        assert_eq!(list_tabs(&updated).len(), 1);
    }

    #[test]
    fn fork_counter_is_monotonic_across_deletes() {
        let mut m = append_tab(
            meta(),
            build_fork_tab(ForkTabInput {
                seq: 1,
                session_id: None,
                pane_id: None,
                created_at: "t".to_string(),
            }),
        );
        m = append_tab(
            m,
            build_fork_tab(ForkTabInput {
                seq: 2,
                session_id: None,
                pane_id: None,
                created_at: "t".to_string(),
            }),
        );
        // Deleting fork-2 leaves the counter at 2, so the next seq is 3, not 2.
        let after_delete = remove_tab(m, "fork-2");
        assert_eq!(next_fork_seq(&after_delete), 3);
    }

    #[test]
    fn update_tab_patches_only_named_tab_and_present_fields() {
        let m = append_tab(
            meta(),
            build_fork_tab(ForkTabInput {
                seq: 1,
                session_id: Some("sess-a".to_string()),
                pane_id: Some("%1".to_string()),
                created_at: "t".to_string(),
            }),
        );
        // Patch only the pane id; session id (None patch) is left untouched.
        let updated = update_tab(
            m,
            "fork-1",
            TabPatch { session_id: None, pane_id: Some(Some("%9".to_string())) },
        );
        let tab = find_tab(&updated, "fork-1").unwrap();
        assert_eq!(tab.pane_id.as_deref(), Some("%9"));
        assert_eq!(tab.session_id.as_deref(), Some("sess-a"));
        // A missing tab id is a no-op.
        assert_eq!(list_tabs(&update_tab(updated, "nope", TabPatch { session_id: Some(None), pane_id: None })).len(), 1);
    }

    #[test]
    fn removing_active_tab_falls_back_to_root() {
        let m = append_tab(
            meta(),
            build_fork_tab(ForkTabInput {
                seq: 1,
                session_id: None,
                pane_id: None,
                created_at: "t".to_string(),
            }),
        );
        assert_eq!(active_tab_id(&m), "fork-1");
        let removed = remove_tab(m, "fork-1");
        assert_eq!(active_tab_id(&removed), ROOT_TAB_ID);
    }
}
