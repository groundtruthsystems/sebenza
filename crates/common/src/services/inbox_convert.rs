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

/// The side effects a conversion performs, behind a port so the sequence can be
/// tested without running git or tmux.
///
/// Every method is fallible and every failure stays confined to its own target
/// — [`run_conversion`] never lets one target's problem reach another.
pub trait ConversionRunner {
    /// Create the worktree **without** a creation prompt, returning its path.
    ///
    /// Unprompted on purpose: `create_worktrees` launches *and* prompts the
    /// agent the instant it returns, which would race the note into existence.
    fn create_worktree(&self, target: &ConversionTarget) -> Result<String, String>;

    /// Write the draft copy into the worktree at [`NOTE_REL_PATH`].
    fn write_note(&self, worktree_path: &str, body: &str) -> Result<(), String>;

    /// Keep the note out of `git status`, for this worktree only.
    fn exclude_note(&self, worktree_path: &str) -> Result<(), String>;

    /// Record which draft produced this worktree, so the trail reads from both
    /// ends rather than only from the inbox.
    fn record_origin(&self, worktree_path: &str, draft_id: &str) -> Result<(), String>;

    /// Send the target's prompt, now that the note is in place.
    fn send_prompt(&self, target: &ConversionTarget, worktree_path: &str) -> Result<(), String>;

    fn now(&self) -> String;
}

/// Where the draft copy lands inside a converted worktree.
pub const NOTE_REL_PATH: &str = ".ai/sebenza/inbox-note.md";

/// Run one wave of conversions.
///
/// Per target, in order: create the worktree unprompted, write the note and
/// exclude it, record the origin, then prompt. The note is on disk and ignored
/// before the agent is asked to do anything, so it can neither be committed by
/// accident nor be missing when the agent looks for it.
///
/// A target that fails is recorded and the wave continues; callers get one
/// outcome per target, in order.
///
/// `on_outcome` fires as each target finishes, before the next begins. That is
/// what makes a wave crash-safe: the caller persists each result immediately,
/// so a server killed midway leaves a draft that still knows what happened to
/// the targets that completed.
pub fn run_conversion<R, F>(
    runner: &R,
    draft_id: &str,
    body: &str,
    targets: &[ConversionTarget],
    mut on_outcome: F,
) -> Vec<ConversionOutcome>
where
    R: ConversionRunner,
    F: FnMut(&ConversionOutcome),
{
    targets
        .iter()
        .map(|target| {
            let outcome = run_one(runner, draft_id, body, target);
            on_outcome(&outcome);
            outcome
        })
        .collect()
}

/// One target, start to finish. Every early return is a recorded failure, never
/// a silent skip.
fn run_one<R: ConversionRunner>(
    runner: &R,
    draft_id: &str,
    body: &str,
    target: &ConversionTarget,
) -> ConversionOutcome {
    let fail = |e: String| ConversionOutcome::failed(target, e, runner.now());

    let worktree_path = match runner.create_worktree(target) {
        Ok(path) => path,
        // Nothing exists to write into or prompt, so the target stops here.
        Err(e) => return fail(e),
    };

    if let Err(e) = runner.write_note(&worktree_path, body) {
        // The worktree exists but has no notes. Prompting now would start an
        // agent that cannot see what it was asked to read.
        return fail(e);
    }
    if let Err(e) = runner.exclude_note(&worktree_path) {
        // Refusing here is deliberate: a note git can see is the leak the
        // design set out to prevent, so half-done is not good enough.
        return fail(e);
    }
    // Best-effort: the origin back-link is for traceability, and losing it is
    // not worth discarding a worktree that is otherwise ready to work.
    let _ = runner.record_origin(&worktree_path, draft_id);

    if let Err(e) = runner.send_prompt(target, &worktree_path) {
        return fail(e);
    }
    ConversionOutcome::created(target, worktree_path, runner.now())
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
            status_after(DraftStatus::Draft, std::slice::from_ref(&ok)),
            DraftStatus::Promoted
        );
        assert_eq!(
            status_after(DraftStatus::Draft, &[bad.clone(), ok]),
            DraftStatus::Promoted
        );
        // Every target failed: the draft must not claim to be promoted.
        assert_eq!(
            status_after(DraftStatus::Draft, std::slice::from_ref(&bad)),
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

    // --- run_conversion ---------------------------------------------------

    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeRunner {
        calls: RefCell<Vec<String>>,
        fail_create: Vec<String>,
        fail_note: Vec<String>,
        fail_prompt: Vec<String>,
    }

    impl FakeRunner {
        fn log(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl ConversionRunner for FakeRunner {
        fn create_worktree(&self, target: &ConversionTarget) -> Result<String, String> {
            self.calls
                .borrow_mut()
                .push(format!("create:{}", target.branch));
            if self.fail_create.contains(&target.branch) {
                return Err("worktree exists".into());
            }
            Ok(format!("/wt/{}", target.branch))
        }
        fn write_note(&self, worktree_path: &str, _body: &str) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("note:{worktree_path}"));
            if self.fail_note.iter().any(|b| worktree_path.ends_with(b)) {
                return Err("disk full".into());
            }
            Ok(())
        }
        fn exclude_note(&self, worktree_path: &str) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("exclude:{worktree_path}"));
            Ok(())
        }
        fn record_origin(&self, worktree_path: &str, draft_id: &str) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("origin:{worktree_path}:{draft_id}"));
            Ok(())
        }
        fn send_prompt(
            &self,
            target: &ConversionTarget,
            worktree_path: &str,
        ) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("prompt:{worktree_path}"));
            if self.fail_prompt.contains(&target.branch) {
                return Err("agent did not start".into());
            }
            Ok(())
        }
        fn now(&self) -> String {
            "2026-09-28T00:00:00Z".to_string()
        }
    }

    #[test]
    fn the_note_lands_and_is_excluded_before_the_agent_is_prompted() {
        // The whole ordering rule: an agent must never be asked to work before
        // its notes exist, and the note must never be visible to git.
        let runner = FakeRunner::default();
        let targets = vec![target("/code/acme", "x")];
        run_conversion(&runner, "01DRAFT", "# notes", &targets, |_| {});

        assert_eq!(
            runner.log(),
            vec![
                "create:x",
                "note:/wt/x",
                "exclude:/wt/x",
                "origin:/wt/x:01DRAFT",
                "prompt:/wt/x",
            ]
        );
    }

    #[test]
    fn every_target_runs_and_reports_in_order() {
        let runner = FakeRunner::default();
        let targets = vec![target("/code/acme", "one"), target("/code/beta", "two")];
        let outcomes = run_conversion(&runner, "01DRAFT", "b", &targets, |_| {});
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].branch, "one");
        assert_eq!(outcomes[1].branch, "two");
        assert!(outcomes.iter().all(ConversionOutcome::is_created));
    }

    #[test]
    fn one_failed_target_does_not_stop_the_others() {
        let runner = FakeRunner {
            fail_create: vec!["two".into()],
            ..Default::default()
        };
        let targets = vec![
            target("/code/acme", "one"),
            target("/code/acme", "two"),
            target("/code/beta", "three"),
        ];
        let outcomes = run_conversion(&runner, "01DRAFT", "b", &targets, |_| {});

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].is_created());
        assert!(!outcomes[1].is_created());
        assert_eq!(outcomes[1].error.as_deref(), Some("worktree exists"));
        assert!(outcomes[2].is_created(), "a later target must still run");
    }

    #[test]
    fn a_failed_create_skips_the_rest_of_that_target() {
        // No worktree means nothing to write into and nothing to prompt.
        let runner = FakeRunner {
            fail_create: vec!["x".into()],
            ..Default::default()
        };
        run_conversion(
            &runner,
            "01DRAFT",
            "b",
            &[target("/code/acme", "x")],
            |_| {},
        );
        assert_eq!(runner.log(), vec!["create:x"]);
    }

    #[test]
    fn a_failed_note_does_not_prompt_an_agent_with_no_notes() {
        let runner = FakeRunner {
            fail_note: vec!["x".into()],
            ..Default::default()
        };
        let outcomes = run_conversion(
            &runner,
            "01DRAFT",
            "b",
            &[target("/code/acme", "x")],
            |_| {},
        );
        assert!(!runner.log().iter().any(|c| c.starts_with("prompt:")));
        assert!(!outcomes[0].is_created());
        assert_eq!(outcomes[0].error.as_deref(), Some("disk full"));
    }

    #[test]
    fn a_worktree_that_never_got_its_prompt_is_recorded_as_failed() {
        // The worktree is real, so the outcome carries its path - but it is not
        // "created" in the sense that matters, because no agent is working.
        let runner = FakeRunner {
            fail_prompt: vec!["x".into()],
            ..Default::default()
        };
        let outcomes = run_conversion(
            &runner,
            "01DRAFT",
            "b",
            &[target("/code/acme", "x")],
            |_| {},
        );
        assert!(!outcomes[0].is_created());
        assert_eq!(outcomes[0].error.as_deref(), Some("agent did not start"));
    }

    #[test]
    fn the_whole_draft_body_is_written_to_every_target() {
        struct BodySpy(RefCell<Vec<String>>);
        impl ConversionRunner for BodySpy {
            fn create_worktree(&self, t: &ConversionTarget) -> Result<String, String> {
                Ok(format!("/wt/{}", t.branch))
            }
            fn write_note(&self, _p: &str, body: &str) -> Result<(), String> {
                self.0.borrow_mut().push(body.to_string());
                Ok(())
            }
            fn exclude_note(&self, _p: &str) -> Result<(), String> {
                Ok(())
            }
            fn record_origin(&self, _p: &str, _d: &str) -> Result<(), String> {
                Ok(())
            }
            fn send_prompt(&self, _t: &ConversionTarget, _p: &str) -> Result<(), String> {
                Ok(())
            }
            fn now(&self) -> String {
                "t".into()
            }
        }
        let spy = BodySpy(RefCell::new(Vec::new()));
        let targets = vec![target("/code/acme", "one"), target("/code/beta", "two")];
        run_conversion(&spy, "01DRAFT", "# the whole draft", &targets, |_| {});
        assert_eq!(
            spy.0.into_inner(),
            vec!["# the whole draft", "# the whole draft"],
            "each worktree gets the entire draft, not a slice"
        );
    }

    #[test]
    fn each_outcome_is_handed_over_before_the_next_target_starts() {
        // Crash-safety: the caller persists as it goes, so a server killed
        // midway still knows what happened to the targets that finished.
        let runner = FakeRunner {
            fail_create: vec!["two".into()],
            ..Default::default()
        };
        let targets = vec![
            target("/code/acme", "one"),
            target("/code/acme", "two"),
            target("/code/beta", "three"),
        ];
        let mut seen: Vec<String> = Vec::new();
        run_conversion(&runner, "01DRAFT", "b", &targets, |o| {
            seen.push(format!("{}:{}", o.branch, o.outcome));
        });
        assert_eq!(seen, vec!["one:created", "two:failed", "three:created"]);
    }
}
