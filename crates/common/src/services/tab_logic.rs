use crate::domain::model::{ROOT_TAB_ID, WorktreeMeta, WorktreeTab, WorktreeTabKind};

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
    /// The worktree's agent — a fork always continues *its* conversation.
    pub agent_id: String,
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
        agent: Some(input.agent_id),
        created_at: input.created_at,
    }
}

pub struct AgentTabInput {
    pub agent_id: String,
    /// The agent's display label, so custom agents keep their configured casing.
    pub agent_label: String,
    /// 1 renders without a numeric suffix (`Codex`, then `Codex 2`).
    pub ordinal: i32,
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
    pub created_at: String,
    /// Uniquifier for the tab id (timestamp millis), mirroring `shell-<ms>`.
    pub id_suffix: String,
}

/// Build a fresh-session tab.
///
/// `seq` holds the per-agent ordinal. That is safe because `append_tab` only lets
/// *fork* tabs move the monotonic `fork_counter`; storing it here rather than
/// re-parsing it out of the label avoids mangling custom agents whose own label
/// ends in a number (e.g. a "GPT 4" agent).
pub fn build_agent_tab(input: AgentTabInput) -> WorktreeTab {
    let label = if input.ordinal <= 1 {
        input.agent_label.clone()
    } else {
        format!("{} {}", input.agent_label, input.ordinal)
    };
    WorktreeTab {
        tab_id: format!("agent-{}-{}", input.agent_id, input.id_suffix),
        kind: WorktreeTabKind::Agent,
        label,
        seq: Some(input.ordinal),
        session_id: input.session_id,
        pane_id: input.pane_id,
        agent: Some(input.agent_id),
        created_at: input.created_at,
    }
}

/// The smallest positive ordinal not used by a live agent tab of `agent_id`.
///
/// Per-agent, not global: a Codex tab beside two Claude tabs is `Codex`, not
/// `Codex 3`. Gap-filling (rather than count + 1) means deleting `Codex 2` and
/// adding another yields `Codex 2` again instead of a duplicate label.
pub fn next_agent_ordinal(meta: &WorktreeMeta, agent_id: &str) -> i32 {
    let used: Vec<i32> = list_tabs(meta)
        .iter()
        .filter(|tab| tab.kind == WorktreeTabKind::Agent)
        .filter(|tab| tab.agent.as_deref() == Some(agent_id))
        .filter_map(|tab| tab.seq)
        .collect();
    let mut candidate = 1;
    while used.contains(&candidate) {
        candidate += 1;
    }
    candidate
}

/// The agent that owns a tab's pane, falling back to the worktree's agent for
/// tabs written before per-tab agents existed.
pub fn tab_agent_id<'a>(tab: &'a WorktreeTab, meta: &'a WorktreeMeta) -> &'a str {
    tab.agent.as_deref().unwrap_or(&meta.agent)
}

/// Append a tab and make it active. Only fork tabs advance the monotonic
/// `fork_counter`: letting any other kind's `seq` set it could *rewind* the
/// counter and hand the next fork a `tab_id` that is already live.
pub fn append_tab(mut meta: WorktreeMeta, tab: WorktreeTab) -> WorktreeMeta {
    let mut tabs = list_tabs(&meta);
    let fork_counter = if tab.kind == WorktreeTabKind::Fork {
        tab.seq.or(meta.fork_counter)
    } else {
        meta.fork_counter
    }
    .unwrap_or(0);
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
            agent_id: "claude".to_string(),
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
                agent_id: "claude".to_string(),
                session_id: None,
                pane_id: None,
                created_at: "t".to_string(),
            }),
        );
        m = append_tab(
            m,
            build_fork_tab(ForkTabInput {
                seq: 2,
                agent_id: "claude".to_string(),
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
    fn appending_a_non_fork_tab_does_not_rewind_the_fork_counter() {
        // Regression: `seq.or(fork_counter)` let any seq-carrying tab reset the
        // counter, so the next fork would reuse a live `fork-N` tab_id.
        let mut m = meta();
        m.fork_counter = Some(5);
        let stray = WorktreeTab {
            tab_id: "shell-1".to_string(),
            kind: WorktreeTabKind::Shell,
            label: "Shell".to_string(),
            seq: Some(1),
            session_id: None,
            pane_id: Some("%7".to_string()),
            agent: None,
            created_at: "t".to_string(),
        };
        let updated = append_tab(m, stray);
        assert_eq!(updated.fork_counter, Some(5));
        assert_eq!(next_fork_seq(&updated), 6);
    }

    #[test]
    fn update_tab_patches_only_named_tab_and_present_fields() {
        let m = append_tab(
            meta(),
            build_fork_tab(ForkTabInput {
                seq: 1,
                agent_id: "claude".to_string(),
                session_id: Some("sess-a".to_string()),
                pane_id: Some("%1".to_string()),
                created_at: "t".to_string(),
            }),
        );
        // Patch only the pane id; session id (None patch) is left untouched.
        let updated = update_tab(
            m,
            "fork-1",
            TabPatch {
                session_id: None,
                pane_id: Some(Some("%9".to_string())),
            },
        );
        let tab = find_tab(&updated, "fork-1").unwrap();
        assert_eq!(tab.pane_id.as_deref(), Some("%9"));
        assert_eq!(tab.session_id.as_deref(), Some("sess-a"));
        // A missing tab id is a no-op.
        assert_eq!(
            list_tabs(&update_tab(
                updated,
                "nope",
                TabPatch {
                    session_id: Some(None),
                    pane_id: None
                }
            ))
            .len(),
            1
        );
    }

    #[test]
    fn removing_active_tab_falls_back_to_root() {
        let m = append_tab(
            meta(),
            build_fork_tab(ForkTabInput {
                seq: 1,
                agent_id: "claude".to_string(),
                session_id: None,
                pane_id: None,
                created_at: "t".to_string(),
            }),
        );
        assert_eq!(active_tab_id(&m), "fork-1");
        let removed = remove_tab(m, "fork-1");
        assert_eq!(active_tab_id(&removed), ROOT_TAB_ID);
    }

    /// Mint the next agent tab for `id` and append it, exactly as the service
    /// does: read the ordinal from current meta, then push.
    fn push_agent_tab(meta: WorktreeMeta, id: &str, label: &str) -> WorktreeMeta {
        let tab = build_agent_tab(AgentTabInput {
            agent_id: id.to_string(),
            agent_label: label.to_string(),
            ordinal: next_agent_ordinal(&meta, id),
            session_id: None,
            pane_id: Some("%1".to_string()),
            created_at: "t".to_string(),
            id_suffix: format!("{}-{}", id, list_tabs(&meta).len()),
        });
        append_tab(meta, tab)
    }

    #[test]
    fn agent_tab_label_omits_the_ordinal_for_the_first_of_each_agent() {
        let mut m = meta();
        m = push_agent_tab(m, "claude", "Claude");
        assert_eq!(list_tabs(&m).last().unwrap().label, "Claude");
        m = push_agent_tab(m, "claude", "Claude");
        assert_eq!(list_tabs(&m).last().unwrap().label, "Claude 2");
        // Ordinals are per-agent: a Codex tab beside two Claude tabs is "Codex".
        m = push_agent_tab(m, "codex", "Codex");
        assert_eq!(list_tabs(&m).last().unwrap().label, "Codex");
        // Custom agents keep their configured casing.
        m = push_agent_tab(m, "goose", "Goose");
        assert_eq!(list_tabs(&m).last().unwrap().label, "Goose");
    }

    #[test]
    fn agent_tab_ordinals_reuse_gaps_instead_of_duplicating_labels() {
        let mut m = meta();
        for _ in 0..3 {
            m = push_agent_tab(m, "codex", "Codex");
        }
        let middle = list_tabs(&m)
            .into_iter()
            .find(|t| t.label == "Codex 2")
            .unwrap();
        m = remove_tab(m, &middle.tab_id);
        // Counting live tabs + 1 would give "Codex 3" — a duplicate.
        assert_eq!(next_agent_ordinal(&m, "codex"), 2);
    }

    #[test]
    fn agent_tabs_never_rewind_the_fork_counter() {
        // The ordinal lives in `seq`, which is only safe because append_tab
        // ignores seq for non-fork kinds. Guard that pairing directly.
        let mut m = meta();
        m.fork_counter = Some(5);
        m = push_agent_tab(m, "codex", "Codex");
        m = push_agent_tab(m, "codex", "Codex");
        assert_eq!(next_fork_seq(&m), 6);
    }

    #[test]
    fn agent_tab_ids_are_unique_and_name_their_agent() {
        let mut m = meta();
        m = push_agent_tab(m, "codex", "Codex");
        m = push_agent_tab(m, "codex", "Codex");
        let ids: Vec<String> = list_tabs(&m).into_iter().map(|t| t.tab_id).collect();
        assert!(ids.iter().all(|id| id.starts_with("agent-codex-")));
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn build_fork_tab_stamps_the_worktree_agent() {
        let tab = build_fork_tab(ForkTabInput {
            seq: 1,
            agent_id: "codex".to_string(),
            session_id: None,
            pane_id: None,
            created_at: "t".to_string(),
        });
        assert_eq!(tab.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn tab_agent_id_falls_back_to_the_worktree_agent_for_legacy_tabs() {
        let mut m = meta();
        m.agent = "codex".to_string();
        let legacy = WorktreeTab {
            tab_id: "fork-1".to_string(),
            kind: WorktreeTabKind::Fork,
            label: "Fork 1".to_string(),
            seq: Some(1),
            session_id: None,
            pane_id: None,
            agent: None, // written before per-tab agents existed
            created_at: "t".to_string(),
        };
        assert_eq!(tab_agent_id(&legacy, &m), "codex");

        let explicit = WorktreeTab {
            agent: Some("claude".to_string()),
            ..legacy
        };
        assert_eq!(tab_agent_id(&explicit, &m), "claude");
    }
}
