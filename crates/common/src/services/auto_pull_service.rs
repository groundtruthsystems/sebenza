use crate::adapters::git::GitGateway;
use serde::Serialize;

/// Response shape for `/api/pull-main` (mirrors `PullMainResponseSchema`).
#[derive(Serialize)]
pub struct PullMainResult {
    /// `updated` | `already_up_to_date` | `fetch_failed` | `merge_failed` |
    /// `skipped_wrong_branch`.
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

/// Pure decision: may we pull, given the checkout's current branch?
///
/// An unreadable HEAD (detached, or a git failure) counts as "not on main":
/// fail closed, because the cost of pulling onto the wrong branch is the user's
/// uncommitted or unpushed work.
fn should_pull(current_branch: Result<String, String>, main_branch: &str) -> bool {
    current_branch
        .map(|current| current == main_branch)
        .unwrap_or(false)
}

/// Whether the checkout at `project_root` is actually on `main_branch`.
///
/// Pull operates on the repo root by *path*, not by ref, so without this it
/// would happily fast-forward (or hard-reset) whatever branch the user has
/// checked out there. The main checkout is a place people work — and, with the
/// main repo openable, a place they get a shell — and the auto-pull loop runs
/// unattended every 30s, so this is not theoretical.
fn is_on_main_branch(git: &GitGateway, project_root: &str, main_branch: &str) -> bool {
    should_pull(git.current_branch(project_root), main_branch)
}

/// Fetch `origin/<main>` and fast-forward the main branch.
pub fn pull_main_branch(git: &GitGateway, project_root: &str, main_branch: &str) -> PullMainResult {
    if !is_on_main_branch(git, project_root, main_branch) {
        return PullMainResult::status("skipped_wrong_branch");
    }
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
    // Doubly important here: this path hard-resets, so pulling onto the wrong
    // branch destroys work rather than merely advancing it.
    if !is_on_main_branch(git, project_root, main_branch) {
        return PullMainResult::status("skipped_wrong_branch");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulls_when_the_checkout_is_on_the_main_branch() {
        assert!(should_pull(Ok("main".to_string()), "main"));
        assert!(should_pull(Ok("trunk".to_string()), "trunk"));
    }

    #[test]
    fn pull_is_skipped_when_the_main_checkout_is_on_another_branch() {
        // The scenario an openable main repo invites: the user runs
        // `git checkout -b tmp` in the main session, and the unattended 30s
        // auto-pull loop must not fast-forward *their* branch to origin/main.
        assert!(!should_pull(Ok("tmp".to_string()), "main"));
        assert!(!should_pull(Ok("feature/x".to_string()), "main"));
    }

    #[test]
    fn pull_is_skipped_when_head_cannot_be_read() {
        // Detached HEAD or a git failure — fail closed rather than guess.
        assert!(!should_pull(Err("detached".to_string()), "main"));
    }
}
