//! What a run reads, resolved from pinned git objects before any lock: the
//! job's inputs, the rename maps that carry prior findings to its head, the
//! prior findings themselves and the matrix a submission must cover.
//!
//! The in-scope files of the matrix are the scope diff's files with hunks,
//! except binary and deleted files and anything under `.ai/sebenza/`, whose
//! edits are the daemon's `standards-change` finding rather than a rule
//! review.

use super::deriver::{PriorFinding, STANDARDS_CHANGE_RULE};
use super::git_pinned::Git;
use super::jobs::{Job, ReviewView};
use super::matcher::{RenameMap, rename_map};
use super::resolver::{ResolveError, ReviewInputs, resolve};
use super::scope::{DiffFile, FileStatus, ReviewLimits, SnapshotPins};
use super::standards_docs::WORKSPACE;
use super::submission::Matrix;
use common::domain::review_records::{FindingOrigin, FindingStatus, RunKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One job's resolved inputs and prior findings.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub inputs: ReviewInputs,
    /// Every non-terminal finding of the track, carried to this head.
    pub priors: Vec<PriorFinding>,
}

/// Resolve `job` from its pinned SHAs and carry the track's open findings
/// in `view` to its head. Holds no lock; reads git objects only.
pub fn resolve_context(
    view: &ReviewView,
    job: &Job,
    limits: &ReviewLimits,
) -> Result<RunContext, ResolveError> {
    todo!()
}

/// The pins of `job`. Its target is pinned as the merge-base: every target
/// commit the track merged is an ancestor of the merge-base, so reachability
/// from it classifies target merges exactly as the target tip would.
pub fn job_pins(job: &Job) -> SnapshotPins {
    SnapshotPins {
        head_sha: job.completion.head_sha.clone(),
        base_sha: job.completion.base_sha.clone(),
        target_sha: job.completion.base_sha.clone(),
    }
}

/// The repository a job's worktree belongs to.
pub fn job_git(job: &Job) -> Result<Git, ResolveError> {
    Ok(Git::open(Path::new(&job.completion.worktree_path))?)
}

impl RunContext {
    /// The manifest's rule ids.
    pub fn rule_ids(&self) -> BTreeSet<String> {
        self.inputs
            .manifest
            .rules
            .iter()
            .map(|r| r.rule_id.clone())
            .collect()
    }

    /// The files whose every rule a run dispositions.
    pub fn in_scope_files(&self) -> Vec<&DiffFile> {
        todo!()
    }

    /// Every path, old and new, with a hunk in the scope diff.
    pub fn scope_paths(&self) -> BTreeSet<String> {
        todo!()
    }

    /// The open prior findings a run of `kind` must disposition: agent or
    /// measured findings whose rule is still in the manifest and whose path
    /// is still in scope. A second run never sees prior output.
    pub fn required_priors(&self, kind: RunKind) -> Vec<&PriorFinding> {
        todo!()
    }

    /// What a run of `kind` must disposition.
    pub fn matrix(&self, kind: RunKind) -> Matrix {
        Matrix {
            rule_ids: self.rule_ids().into_iter().collect(),
            files: self
                .in_scope_files()
                .iter()
                .map(|f| f.path().to_string())
                .collect(),
            priors: self
                .required_priors(kind)
                .iter()
                .map(|p| p.finding.record.finding_id.clone())
                .collect(),
        }
    }
}

fn is_reserved(rule_id: &str) -> bool {
    rule_id == STANDARDS_CHANGE_RULE
}

/// Rename maps from each anchor SHA to `head`, keyed by anchor SHA.
fn rename_maps(git: &Git, shas: BTreeSet<&str>, head: &str) -> BTreeMap<String, RenameMap> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(status: FileStatus, old: Option<&str>, new: Option<&str>, hunks: usize) -> DiffFile {
        use super::super::scope::{Anchor, Hunk, Side};
        let hunk = Hunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 1,
            anchor: Anchor {
                path: new.or(old).unwrap_or_default().into(),
                line: 1,
                side: Side::Head,
                sha: "h".into(),
            },
        };
        DiffFile {
            status,
            old_path: old.map(String::from),
            new_path: new.map(String::from),
            new_mode: "100644".into(),
            new_oid: None,
            binary: false,
            hunks: vec![hunk; hunks],
        }
    }

    fn context(files: Vec<DiffFile>) -> RunContext {
        RunContext {
            inputs: ReviewInputs {
                pins: SnapshotPins {
                    head_sha: "h".into(),
                    base_sha: "b".into(),
                    target_sha: "b".into(),
                },
                standards_rev: "r".into(),
                standards: vec![],
                manifest: super::super::manifest::RuleManifest {
                    standards_rev: "r".into(),
                    rules: vec![],
                },
                commits: vec![],
                files,
                standards_change: None,
                blockers: vec![],
            },
            priors: vec![],
        }
    }

    #[test]
    fn in_scope_files_skip_deleted_binary_hunkless_and_workspace_files() {
        let mut binary = file(FileStatus::Modified, Some("img.png"), Some("img.png"), 0);
        binary.binary = true;
        let c = context(vec![
            file(FileStatus::Modified, Some("src/a.rs"), Some("src/a.rs"), 1),
            file(
                FileStatus::Renamed,
                Some("src/old.rs"),
                Some("src/new.rs"),
                2,
            ),
            file(FileStatus::Deleted, Some("src/gone.rs"), None, 1),
            file(
                FileStatus::Modified,
                Some("src/same.rs"),
                Some("src/same.rs"),
                0,
            ),
            file(
                FileStatus::Modified,
                Some(".ai/sebenza/workflow.md"),
                Some(".ai/sebenza/workflow.md"),
                1,
            ),
            binary,
        ]);
        let paths: Vec<_> = c.in_scope_files().iter().map(|f| f.path()).collect();
        assert_eq!(paths, vec!["src/a.rs", "src/new.rs"]);
        let scope: Vec<_> = c.scope_paths().into_iter().collect();
        assert_eq!(
            scope,
            vec![
                ".ai/sebenza/workflow.md",
                "src/a.rs",
                "src/gone.rs",
                "src/new.rs",
                "src/old.rs"
            ]
        );
    }
}
