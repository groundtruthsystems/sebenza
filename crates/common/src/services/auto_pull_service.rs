use crate::adapters::git::GitGateway;
use serde::Serialize;

/// Response shape for `/api/pull-main` (mirrors `PullMainResponseSchema`).
#[derive(Serialize)]
pub struct PullMainResult {
    /// `updated` | `already_up_to_date` | `fetch_failed` | `merge_failed`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PullMainResult {
    fn status(status: &'static str) -> Self {
        PullMainResult {
            status,
            from: None,
            to: None,
            error: None,
        }
    }
}

fn updated_or_unchanged(before: Option<String>, after: Option<String>) -> PullMainResult {
    if before == after {
        return PullMainResult::status("already_up_to_date");
    }
    PullMainResult {
        status: "updated",
        from: Some(before.unwrap_or_else(|| "unknown".to_string())),
        to: Some(after.unwrap_or_else(|| "unknown".to_string())),
        error: None,
    }
}

/// Fetch `origin/<main>` and fast-forward the main branch.
pub fn pull_main_branch(git: &GitGateway, project_root: &str, main_branch: &str) -> PullMainResult {
    let before = git.read_worktree_status(project_root).current_commit;

    if let Err(stderr) = git.fetch_branch(project_root, "origin", main_branch) {
        return PullMainResult {
            error: Some(stderr),
            ..PullMainResult::status("fetch_failed")
        };
    }
    if let Err(stderr) = git.fast_forward_merge(project_root, &format!("origin/{main_branch}")) {
        return PullMainResult {
            error: Some(stderr),
            ..PullMainResult::status("merge_failed")
        };
    }

    let after = git.read_worktree_status(project_root).current_commit;
    updated_or_unchanged(before, after)
}

/// Force-pull the main branch via fetch + hard reset. Discards local state.
pub fn force_pull_main_branch(
    git: &GitGateway,
    project_root: &str,
    main_branch: &str,
) -> PullMainResult {
    let before = git.read_worktree_status(project_root).current_commit;

    if let Err(stderr) = git.fetch_branch(project_root, "origin", main_branch) {
        return PullMainResult {
            error: Some(stderr),
            ..PullMainResult::status("fetch_failed")
        };
    }
    if let Err(stderr) = git.hard_reset(project_root, &format!("origin/{main_branch}")) {
        return PullMainResult {
            error: Some(stderr),
            ..PullMainResult::status("merge_failed")
        };
    }

    let after = git.read_worktree_status(project_root).current_commit;
    updated_or_unchanged(before, after)
}
