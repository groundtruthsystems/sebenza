//! Cross-project active-worktree view.
//!
//! One server process owns the runtime state of every project it serves, but
//! `GET /api/worktrees` is scoped to a single URL prefix, so the dashboard can only ever
//! see the project being viewed. This assembles the hub-level answer: every loaded
//! project and its worktree snapshots, for a ticker that spans projects.
//!
//! Eligibility is deliberately *not* decided here. The frontend runs the same derivation
//! it uses for the single-project ticker, so the predicate lives in one place and cannot
//! drift from the spec.
//!
//! Only *loaded* projects appear. Projects initialize lazily, so one this process has not
//! touched since starting holds no runtime state and contributes nothing — the same
//! in-memory limitation as a server restart, widened to project scope.

use common::domain::model::WorktreeSnapshot;
use serde::Serialize;

/// One project's contribution to the cross-project view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWorktreeProject {
    /// URL prefix, and the identity the dashboard navigates to.
    pub prefix: String,
    pub name: String,
    pub worktrees: Vec<WorktreeSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWorktreesResponse {
    pub projects: Vec<ActiveWorktreeProject>,
}

/// Assemble the response from each loaded project's `(prefix, name, worktrees)`.
///
/// A project with no worktrees is kept rather than filtered out: "this project has
/// nothing running" and "this project is not loaded" are different facts, and collapsing
/// them would leave the caller unable to tell a quiet project from an absent one.
///
/// Input order is preserved — `ProjectManager` holds an `IndexMap`, so its order is the
/// order projects were registered, which is stable across polls.
pub fn build_active_worktrees(
    projects: Vec<(String, String, Vec<WorktreeSnapshot>)>,
) -> ActiveWorktreesResponse {
    ActiveWorktreesResponse {
        projects: projects
            .into_iter()
            .map(|(prefix, name, worktrees)| ActiveWorktreeProject {
                prefix,
                name,
                worktrees,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::domain::model::{AgentFeedbackState, WorktreeKind, WorktreeSource};

    fn snapshot(branch: &str) -> WorktreeSnapshot {
        WorktreeSnapshot {
            branch: branch.to_string(),
            kind: WorktreeKind::Linked,
            label: None,
            base_branch: None,
            path: format!("/repo/{branch}"),
            dir: format!("/repo/{branch}"),
            archived: false,
            profile: None,
            agent_name: None,
            agent_label: None,
            agent_terminal_stale: false,
            mux: true,
            dirty: false,
            unpushed: false,
            pane_count: 1,
            status: "running".to_string(),
            feedback_state: AgentFeedbackState::None,
            elapsed: "1m".to_string(),
            services: Vec::new(),
            prs: Vec::new(),
            creation: None,
            source: WorktreeSource::Ui,
            oneshot: None,
            tabs: Vec::new(),
            active_tab_id: None,
            reported_session_id: None,
        }
    }

    #[test]
    fn each_project_contributes_its_prefix_name_and_worktrees() {
        let response = build_active_worktrees(vec![
            (
                "alpha".to_string(),
                "Alpha".to_string(),
                vec![snapshot("feat-a")],
            ),
            (
                "beta".to_string(),
                "Beta".to_string(),
                vec![snapshot("feat-b"), snapshot("feat-c")],
            ),
        ]);

        assert_eq!(response.projects.len(), 2);
        assert_eq!(response.projects[0].prefix, "alpha");
        assert_eq!(response.projects[0].name, "Alpha");
        assert_eq!(
            response.projects[1]
                .worktrees
                .iter()
                .map(|w| w.branch.as_str())
                .collect::<Vec<_>>(),
            vec!["feat-b", "feat-c"]
        );
    }

    #[test]
    fn a_project_with_nothing_running_is_still_reported() {
        // Filtering it out would make a quiet project indistinguishable from one this
        // process never loaded, which is the distinction the caller most needs.
        let response = build_active_worktrees(vec![
            ("quiet".to_string(), "Quiet".to_string(), Vec::new()),
            (
                "busy".to_string(),
                "Busy".to_string(),
                vec![snapshot("feat-a")],
            ),
        ]);

        assert_eq!(
            response
                .projects
                .iter()
                .map(|p| p.prefix.as_str())
                .collect::<Vec<_>>(),
            vec!["quiet", "busy"]
        );
        assert!(response.projects[0].worktrees.is_empty());
    }

    #[test]
    fn project_order_is_preserved() {
        // Registration order, not alphabetical: the ticker must not reshuffle between
        // polls, and the frontend relies on this order for grouping.
        let response = build_active_worktrees(vec![
            ("zulu".to_string(), "Zulu".to_string(), Vec::new()),
            ("alpha".to_string(), "Alpha".to_string(), Vec::new()),
            ("mike".to_string(), "Mike".to_string(), Vec::new()),
        ]);

        assert_eq!(
            response
                .projects
                .iter()
                .map(|p| p.prefix.as_str())
                .collect::<Vec<_>>(),
            vec!["zulu", "alpha", "mike"]
        );
    }

    #[test]
    fn no_projects_yields_an_empty_list_rather_than_an_error() {
        assert!(build_active_worktrees(Vec::new()).projects.is_empty());
    }

    #[test]
    fn the_response_serializes_with_camel_case_keys() {
        let response = build_active_worktrees(vec![(
            "alpha".to_string(),
            "Alpha".to_string(),
            vec![snapshot("feat-a")],
        )]);

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["projects"][0]["prefix"], "alpha");
        // The worktree keeps the shape the frontend already knows, so the same
        // derivation can run over it.
        assert_eq!(json["projects"][0]["worktrees"][0]["feedbackState"], "none");
        assert_eq!(json["projects"][0]["worktrees"][0]["status"], "running");
    }
}
