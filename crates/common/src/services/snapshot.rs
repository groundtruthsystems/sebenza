use crate::domain::config::ProjectConfig;
use crate::domain::model::{
    AgentLifecycle, ManagedWorktreeRuntimeState, ProjectInfo, ProjectSnapshot, WorktreeSnapshot,
};
use crate::services::config_view::agent_label;
use chrono::{DateTime, Utc};

/// Human-readable elapsed time since `started_at` (ISO 8601), or "" when absent.
/// Mirrors `formatElapsedSince`: <1m→"0m", <60m→"Nm", <24h→"Nh", else "Nd".
pub fn format_elapsed_since(started_at: Option<&str>, now: DateTime<Utc>) -> String {
    let Some(started_at) = started_at else {
        return String::new();
    };
    let Ok(started) = DateTime::parse_from_rfc3339(started_at) else {
        return String::new();
    };
    let diff_ms = (now.timestamp_millis() - started.timestamp_millis()).max(0);
    let diff_minutes = diff_ms / 60_000;

    if diff_minutes < 1 {
        return "0m".to_string();
    }
    if diff_minutes < 60 {
        return format!("{diff_minutes}m");
    }
    let diff_hours = diff_minutes / 60;
    if diff_hours < 24 {
        return format!("{diff_hours}h");
    }
    let diff_days = diff_hours / 24;
    format!("{diff_days}d")
}

fn lifecycle_status(lifecycle: AgentLifecycle) -> String {
    match lifecycle {
        AgentLifecycle::Closed => "closed",
        AgentLifecycle::Starting => "starting",
        AgentLifecycle::Running => "running",
        AgentLifecycle::Idle => "idle",
        AgentLifecycle::Stopped => "stopped",
        AgentLifecycle::Error => "error",
    }
    .to_string()
}

fn map_worktree_snapshot(
    state: &ManagedWorktreeRuntimeState,
    config: &ProjectConfig,
    now: DateTime<Utc>,
    archived_paths: &std::collections::HashSet<String>,
) -> WorktreeSnapshot {
    WorktreeSnapshot {
        branch: state.branch.clone(),
        kind: state.kind,
        label: state.label.clone(),
        base_branch: state.base_branch.clone().filter(|b| !b.is_empty()),
        path: state.path.clone(),
        dir: state.path.clone(),
        archived: archived_paths.contains(&crate::services::archive_service::normalize_archive_path(
            &state.path,
        )),
        profile: state.profile.clone(),
        agent_name: state.agent_name.clone(),
        agent_label: agent_label(config, state.agent_name.as_deref()),
        agent_terminal_stale: state.agent_terminal_stale,
        mux: state.session.exists,
        dirty: state.git.dirty,
        unpushed: state.git.ahead_count > 0,
        pane_count: state.session.pane_count,
        status: lifecycle_status(state.agent.lifecycle),
        elapsed: format_elapsed_since(state.agent.last_started_at.as_deref(), now),
        services: state.services.clone(),
        prs: state.prs.clone(),
        creation: None,
        source: state.source.clone(),
        oneshot: state.oneshot.clone(),
        tabs: state.tabs.clone(),
        active_tab_id: state.active_tab_id.clone(),
    }
}

/// Build the `ProjectSnapshot` from the in-memory runtime worktrees.
/// `archived_paths` are the normalized worktree paths currently archived.
/// (Creating-worktrees and notifications are deferred in this increment.)
pub fn build_project_snapshot(
    config: &ProjectConfig,
    worktrees: &[ManagedWorktreeRuntimeState],
    now: DateTime<Utc>,
    archived_paths: &std::collections::HashSet<String>,
    notifications: Vec<crate::domain::model::NotificationView>,
) -> ProjectSnapshot {
    let mut snapshots: Vec<WorktreeSnapshot> = worktrees
        .iter()
        .map(|state| map_worktree_snapshot(state, config, now, archived_paths))
        .collect();
    snapshots.sort_by(|a, b| a.branch.cmp(&b.branch));

    ProjectSnapshot {
        project: ProjectInfo {
            name: config.name.clone(),
            main_branch: config.workspace.main_branch.clone(),
        },
        worktrees: snapshots,
        notifications,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn elapsed_boundaries() {
        let now = at("2024-01-02T00:00:00Z");
        assert_eq!(format_elapsed_since(None, now), "");
        assert_eq!(format_elapsed_since(Some("not-a-date"), now), "");
        // 30s → 0m
        assert_eq!(format_elapsed_since(Some("2024-01-01T23:59:30Z"), now), "0m");
        // 59m
        assert_eq!(format_elapsed_since(Some("2024-01-01T23:01:00Z"), now), "59m");
        // exactly 1h
        assert_eq!(format_elapsed_since(Some("2024-01-01T23:00:00Z"), now), "1h");
        // 25h → 1d
        assert_eq!(format_elapsed_since(Some("2024-01-01T00:00:00Z"), now), "1d");
    }

    #[test]
    fn future_start_clamps_to_zero() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(format_elapsed_since(Some("2100-01-01T00:00:00Z"), now), "0m");
    }

    fn state(branch: &str, kind: crate::domain::model::WorktreeKind) -> ManagedWorktreeRuntimeState {
        use crate::domain::model::{
            AgentLifecycle, AgentRuntimeState, GitWorktreeRuntimeState, SessionRuntimeState,
            WorktreeSource,
        };
        ManagedWorktreeRuntimeState {
            worktree_id: format!("id-{branch}"),
            kind,
            branch: branch.to_string(),
            label: None,
            base_branch: None,
            path: format!("/repo/{branch}"),
            profile: None,
            agent_name: None,
            source: WorktreeSource::Ui,
            oneshot: None,
            agent_terminal_stale: false,
            tabs: Vec::new(),
            active_tab_id: None,
            git: GitWorktreeRuntimeState {
                exists: true,
                branch: branch.to_string(),
                dirty: false,
                ahead_count: 0,
                current_commit: None,
            },
            session: SessionRuntimeState {
                exists: true,
                session_name: None,
                window_name: format!("sebenza-{branch}"),
                pane_count: 1,
            },
            agent: AgentRuntimeState {
                lifecycle: AgentLifecycle::Closed,
                runtime: "host".to_string(),
                last_started_at: None,
                last_event_at: None,
                last_error: None,
            },
            services: Vec::new(),
            prs: Vec::new(),
        }
    }

    #[test]
    fn worktree_kind_survives_into_the_snapshot() {
        use crate::domain::model::WorktreeKind;
        let config = crate::config::default_config();
        let now = at("2026-01-01T00:00:00Z");
        let empty = std::collections::HashSet::new();

        let main = map_worktree_snapshot(&state("main", WorktreeKind::Main), &config, now, &empty);
        assert_eq!(main.kind, WorktreeKind::Main);
        // No agent runs on the trunk, so no agent label is rendered for it.
        assert_eq!(main.agent_label, None);
        assert_eq!(main.base_branch, None);
        assert!(main.services.is_empty());

        let linked =
            map_worktree_snapshot(&state("feat-a", WorktreeKind::Linked), &config, now, &empty);
        assert_eq!(linked.kind, WorktreeKind::Linked);
    }

    #[test]
    fn worktree_kind_is_camel_case_on_the_wire() {
        use crate::domain::model::WorktreeKind;
        let config = crate::config::default_config();
        let snapshot = map_worktree_snapshot(
            &state("main", WorktreeKind::Main),
            &config,
            at("2026-01-01T00:00:00Z"),
            &std::collections::HashSet::new(),
        );
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["kind"], "main");
    }
}
