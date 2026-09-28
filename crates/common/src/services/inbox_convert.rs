//! Turning one draft into worktrees, across one or more projects.
//!
//! This module owns the rules; the server owns the plumbing. Validation runs
//! over the whole request **before any side effect**, because a fan-out that
//! half-executes and then rejects target four is far worse than one that
//! refuses up front: worktrees and agent sessions are not free to undo.
//!
//! Once execution starts the posture inverts — targets are independent, and one
//! failure must not touch the others. See [`ConversionOutcome`].

use crate::domain::model::DraftStatus;
use crate::domain::policies::{is_safe_project_path, is_valid_branch_name};
use serde::{Deserialize, Serialize};

/// The most targets one request may carry.
///
/// Each costs a `git worktree add` plus a tmux launch, so an unbounded fan-out
/// is a denial-of-service against the developer's own machine.
pub const MAX_TARGETS: usize = 10;

/// One worktree to create from a draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionTarget {
    /// Absolute path of the project to create the worktree in.
    pub project_path: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub agent_id: Option<String>,
    /// The prompt this worktree's agent is started with.
    pub prompt: String,
}

/// Why a request was refused before anything ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetError {
    NoTargets,
    TooMany {
        count: usize,
    },
    UnsafeProjectPath {
        index: usize,
        path: String,
    },
    UnknownProject {
        index: usize,
        path: String,
    },
    InvalidBranch {
        index: usize,
        branch: String,
    },
    /// Two targets in the same request want the same branch in the same
    /// project. Git would reject the second; better to say so now.
    DuplicateBranch {
        index: usize,
        branch: String,
    },
    EmptyPrompt {
        index: usize,
    },
}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTargets => write!(f, "a conversion needs at least one target"),
            Self::TooMany { count } => {
                write!(f, "{count} targets exceeds the maximum of {MAX_TARGETS}")
            }
            Self::UnsafeProjectPath { index, path } => {
                write!(f, "target {index}: unsafe project path {path:?}")
            }
            Self::UnknownProject { index, path } => {
                write!(f, "target {index}: no registered project at {path:?}")
            }
            Self::InvalidBranch { index, branch } => {
                write!(f, "target {index}: invalid branch name {branch:?}")
            }
            Self::DuplicateBranch { index, branch } => {
                write!(f, "target {index}: branch {branch:?} is used twice")
            }
            Self::EmptyPrompt { index } => {
                write!(f, "target {index}: prompt is empty")
            }
        }
    }
}

/// Validate a whole conversion request. Returns **every** problem, not just the
/// first, so the dialog can mark each bad row at once instead of making the
/// user resubmit to discover the next one.
///
/// `known_projects` is the registry's set of project paths; a target naming
/// anything else is refused, which is what stops a request reaching a
/// repository the server was never told to serve.
pub fn validate_targets(
    targets: &[ConversionTarget],
    known_projects: &[String],
) -> Vec<TargetError> {
    let mut errors = Vec::new();
    if targets.is_empty() {
        return vec![TargetError::NoTargets];
    }
    if targets.len() > MAX_TARGETS {
        errors.push(TargetError::TooMany {
            count: targets.len(),
        });
    }

    let mut seen: Vec<(&str, &str)> = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        if !is_safe_project_path(&target.project_path) {
            errors.push(TargetError::UnsafeProjectPath {
                index,
                path: target.project_path.clone(),
            });
        } else if !known_projects.iter().any(|p| p == &target.project_path) {
            errors.push(TargetError::UnknownProject {
                index,
                path: target.project_path.clone(),
            });
        }

        if !is_valid_branch_name(&target.branch) {
            errors.push(TargetError::InvalidBranch {
                index,
                branch: target.branch.clone(),
            });
        }

        let key = (target.project_path.as_str(), target.branch.as_str());
        if seen.contains(&key) {
            errors.push(TargetError::DuplicateBranch {
                index,
                branch: target.branch.clone(),
            });
        }
        seen.push(key);

        if target.prompt.trim().is_empty() {
            errors.push(TargetError::EmptyPrompt { index });
        }
    }
    errors
}

/// What became of one target. Recorded whether it worked or not — a failure
/// that leaves no trace is indistinguishable from a target never attempted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionOutcome {
    pub project_path: String,
    pub branch: String,
    pub agent_id: Option<String>,
    pub prompt: String,
    /// `"created"` or `"failed"`.
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub at: String,
}

impl ConversionOutcome {
    pub fn created(target: &ConversionTarget, worktree_path: String, at: String) -> Self {
        Self {
            project_path: target.project_path.clone(),
            branch: target.branch.clone(),
            agent_id: target.agent_id.clone(),
            prompt: target.prompt.clone(),
            outcome: "created".to_string(),
            worktree_path: Some(worktree_path),
            error: None,
            at,
        }
    }

    pub fn failed(target: &ConversionTarget, error: String, at: String) -> Self {
        Self {
            project_path: target.project_path.clone(),
            branch: target.branch.clone(),
            agent_id: target.agent_id.clone(),
            prompt: target.prompt.clone(),
            outcome: "failed".to_string(),
            worktree_path: None,
            error: Some(error),
            at,
        }
    }

    pub fn is_created(&self) -> bool {
        self.outcome == "created"
    }
}

/// The status a draft should hold after a wave of conversions.
///
/// `Promoted` means at least one worktree exists. A wave where every target
/// failed leaves the draft exactly as it was — otherwise a listing filtered on
/// promotion would show work that was never created.
pub fn status_after(current: DraftStatus, outcomes: &[ConversionOutcome]) -> DraftStatus {
    if outcomes.iter().any(ConversionOutcome::is_created) {
        DraftStatus::Promoted
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(project: &str, branch: &str) -> ConversionTarget {
        ConversionTarget {
            project_path: project.to_string(),
            branch: branch.to_string(),
            base_branch: None,
            agent_id: Some("claude".to_string()),
            prompt: "do the thing".to_string(),
        }
    }

    fn known() -> Vec<String> {
        vec!["/code/acme".to_string(), "/code/beta".to_string()]
    }

    #[test]
    fn a_valid_request_has_no_errors() {
        let targets = vec![
            target("/code/acme", "feature/x"),
            target("/code/beta", "fix-y"),
        ];
        assert_eq!(validate_targets(&targets, &known()), vec![]);
    }

    #[test]
    fn an_empty_request_is_refused() {
        assert_eq!(
            validate_targets(&[], &known()),
            vec![TargetError::NoTargets]
        );
    }

    #[test]
    fn the_target_cap_is_enforced() {
        let targets: Vec<_> = (0..=MAX_TARGETS)
            .map(|i| target("/code/acme", &format!("branch-{i}")))
            .collect();
        let errors = validate_targets(&targets, &known());
        assert!(errors.contains(&TargetError::TooMany {
            count: MAX_TARGETS + 1
        }));

        let at_cap: Vec<_> = (0..MAX_TARGETS)
            .map(|i| target("/code/acme", &format!("branch-{i}")))
            .collect();
        assert_eq!(validate_targets(&at_cap, &known()), vec![]);
    }

    #[test]
    fn a_project_outside_the_registry_is_refused() {
        // The whole point: a global surface must not reach a repository the
        // server was never told to serve.
        let targets = vec![target("/code/not-registered", "x")];
        assert_eq!(
            validate_targets(&targets, &known()),
            vec![TargetError::UnknownProject {
                index: 0,
                path: "/code/not-registered".to_string()
            }]
        );
    }

    #[test]
    fn an_unsafe_project_path_is_refused_before_the_registry_lookup() {
        let targets = vec![target("../../etc", "x")];
        let errors = validate_targets(&targets, &known());
        assert_eq!(
            errors,
            vec![TargetError::UnsafeProjectPath {
                index: 0,
                path: "../../etc".to_string()
            }]
        );
    }

    #[test]
    fn branch_names_go_through_git_ref_rules() {
        for bad in ["", "has space", "..", "-leading", "trailing/"] {
            let targets = vec![target("/code/acme", bad)];
            let errors = validate_targets(&targets, &known());
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, TargetError::InvalidBranch { .. })),
                "{bad:?} should be rejected, got {errors:?}"
            );
        }
    }

    #[test]
    fn the_same_branch_twice_in_one_project_is_refused() {
        let targets = vec![target("/code/acme", "dup"), target("/code/acme", "dup")];
        let errors = validate_targets(&targets, &known());
        assert!(errors.contains(&TargetError::DuplicateBranch {
            index: 1,
            branch: "dup".to_string()
        }));
    }

    #[test]
    fn the_same_branch_in_different_projects_is_fine() {
        // Two repos can both have `main`-adjacent names; only a collision
        // within one project would make git refuse.
        let targets = vec![target("/code/acme", "same"), target("/code/beta", "same")];
        assert_eq!(validate_targets(&targets, &known()), vec![]);
    }

    #[test]
    fn an_empty_prompt_is_refused() {
        let mut t = target("/code/acme", "x");
        t.prompt = "   ".to_string();
        assert_eq!(
            validate_targets(&[t], &known()),
            vec![TargetError::EmptyPrompt { index: 0 }]
        );
    }

    #[test]
    fn every_problem_is_reported_not_just_the_first() {
        // The dialog marks all the bad rows at once; making the user resubmit
        // to discover the next problem is the behaviour this prevents.
        let targets = vec![
            target("/code/unknown", "bad branch"),
            target("/code/acme", "fine"),
        ];
        let errors = validate_targets(&targets, &known());
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, TargetError::UnknownProject { .. }))
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, TargetError::InvalidBranch { .. }))
        );
    }

    #[test]
    fn promoted_requires_at_least_one_created() {
        let t = target("/code/acme", "x");
        let ok = ConversionOutcome::created(&t, "/wt/x".into(), "now".into());
        let bad = ConversionOutcome::failed(&t, "boom".into(), "now".into());

        assert_eq!(
            status_after(DraftStatus::Draft, &[ok.clone()]),
            DraftStatus::Promoted
        );
        assert_eq!(
            status_after(DraftStatus::Draft, &[bad.clone(), ok]),
            DraftStatus::Promoted
        );
        // Every target failed: the draft must not claim to be promoted.
        assert_eq!(
            status_after(DraftStatus::Draft, &[bad.clone()]),
            DraftStatus::Draft
        );
        assert_eq!(status_after(DraftStatus::Draft, &[]), DraftStatus::Draft);
        // A dropped draft whose wave failed stays dropped, not resurrected.
        assert_eq!(
            status_after(DraftStatus::Dropped, &[bad]),
            DraftStatus::Dropped
        );
    }

    #[test]
    fn a_failed_outcome_keeps_the_prompt_and_the_error() {
        let t = target("/code/acme", "x");
        let out = ConversionOutcome::failed(&t, "worktree exists".into(), "now".into());
        assert!(!out.is_created());
        assert_eq!(out.error.as_deref(), Some("worktree exists"));
        assert_eq!(out.prompt, "do the thing");
        assert_eq!(out.worktree_path, None);
    }
}
