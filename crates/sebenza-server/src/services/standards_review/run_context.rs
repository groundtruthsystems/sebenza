//! What a run reads, resolved from pinned git objects before any lock: the
//! job's inputs, its scope proof, the rename maps that carry prior findings
//! to its head, the prior findings themselves and the matrix a submission
//! must cover.
//!
//! The in-scope files of the matrix are the scope diff's files with hunks,
//! except binary and deleted files and anything under `.ai/sebenza/`, whose
//! edits are the daemon's `standards-change` finding rather than a rule
//! review. A second run's matrix holds only the rules and files of its
//! planned pairs, and it never sees prior findings.

use super::deriver::{OUT_OF_SCOPE_EDIT_RULE, PriorFinding, STANDARDS_CHANGE_RULE};
use super::git_pinned::Git;
use super::jobs::{Finding, Job, ReviewView};
use super::matcher::{RenameMap, rename_map};
use super::reasons::Blocker;
use super::resolver::{ResolveError, ReviewInputs, resolve};
use super::scope::{DiffFile, FileStatus, ReviewLimits, SnapshotPins};
use super::scope_proof::{ScopeProof, prove_scope};
use super::second_run::plan_pairs;
use super::standards_docs::WORKSPACE;
use super::submission::Matrix;
use common::domain::review_records::{FindingOrigin, FindingStatus, PairRef, RunKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One job's resolved inputs and prior findings.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub inputs: ReviewInputs,
    /// Whether the commits are the track's own, and what left the boundary.
    pub scope: ScopeProof,
    /// Every non-terminal finding of the track, carried to this head.
    pub priors: Vec<PriorFinding>,
    /// The pairs the first run scheduled for the second run; `None` until
    /// a first run is accepted.
    pub second_run: Option<Vec<PairRef>>,
}

/// Resolve `job` from its pinned SHAs and carry the track's open findings
/// in `view` to its head. Holds no lock; reads git objects only.
pub fn resolve_context(
    view: &ReviewView,
    job: &Job,
    limits: &ReviewLimits,
) -> Result<RunContext, ResolveError> {
    let git = job_git(job)?;
    let inputs = resolve(&git, &job_pins(job), limits)?;
    let track_id = &job.completion.track_id;
    let scope = prove_scope(&git, track_id, &inputs, view.plan_approval(track_id))?;
    let open: Vec<&Finding> = view
        .track_findings(&job.completion.track_id)
        .into_iter()
        .filter(|f| !f.status.is_terminal())
        .collect();
    let shas = open.iter().map(|f| f.record.anchor.sha.as_str()).collect();
    let renames = rename_maps(&git, shas, &job.completion.head_sha);
    let priors = open
        .into_iter()
        .map(|f| prior_finding(view, f, &renames))
        .collect();
    Ok(RunContext {
        inputs,
        scope,
        priors,
        second_run: job.second_run_plan().map(<[PairRef]>::to_vec),
    })
}

fn prior_finding(
    view: &ReviewView,
    f: &Finding,
    renames: &BTreeMap<String, RenameMap>,
) -> PriorFinding {
    let anchor = &f.record.anchor;
    let path_at_head = renames
        .get(&anchor.sha)
        .and_then(|m| m.get(&anchor.path))
        .unwrap_or(&anchor.path)
        .clone();
    let rule_in_prior_manifest = view
        .job(&f.record.job_id)
        .and_then(Job::manifest_rule_ids)
        .is_some_and(|ids| ids.contains(&f.record.rule_id));
    PriorFinding {
        finding: f.clone(),
        path_at_head,
        rule_in_prior_manifest,
    }
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
    /// Every reason the job cannot pass whatever a run finds, sorted.
    pub fn blockers(&self) -> Vec<Blocker> {
        let mut all: Vec<Blocker> = self
            .inputs
            .blockers
            .iter()
            .chain(&self.scope.blockers)
            .cloned()
            .collect();
        all.sort();
        all
    }

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
        let workspace = format!("{WORKSPACE}/");
        self.inputs
            .files
            .iter()
            .filter(|f| {
                !f.hunks.is_empty()
                    && !f.binary
                    && f.status != FileStatus::Deleted
                    && !f.path().starts_with(&workspace)
            })
            .collect()
    }

    /// Every path, old and new, with a hunk in the scope diff.
    pub fn scope_paths(&self) -> BTreeSet<String> {
        self.inputs
            .files
            .iter()
            .filter(|f| !f.hunks.is_empty())
            .flat_map(|f| [f.old_path.clone(), f.new_path.clone()])
            .flatten()
            .collect()
    }

    /// The open prior findings a run of `kind` must disposition: agent or
    /// measured findings whose rule is still in the manifest and whose path
    /// is still in scope. A second run never sees prior output.
    pub fn required_priors(&self, kind: RunKind) -> Vec<&PriorFinding> {
        if kind == RunKind::Second {
            return Vec::new();
        }
        let (rules, scope) = (self.rule_ids(), self.scope_paths());
        self.priors
            .iter()
            .filter(|p| {
                let r = &p.finding.record;
                p.finding.status == FindingStatus::Open
                    && r.origin != FindingOrigin::StandardsChange
                    && !is_reserved(&r.rule_id)
                    && rules.contains(&r.rule_id)
                    && scope.contains(&p.path_at_head)
            })
            .collect()
    }

    /// What a run of `kind` must disposition.
    pub fn matrix(&self, kind: RunKind) -> Matrix {
        let files: Vec<String> = self
            .in_scope_files()
            .iter()
            .map(|f| f.path().to_string())
            .collect();
        let (rule_ids, files) = match kind {
            RunKind::First => (self.rule_ids().into_iter().collect(), files),
            RunKind::Second => self.second_run_axes(&files),
        };
        Matrix {
            rule_ids,
            files,
            priors: self
                .required_priors(kind)
                .iter()
                .map(|p| p.finding.record.finding_id.clone())
                .collect(),
        }
    }

    /// The second run's pairs: as the first run scheduled them, or, before
    /// any first run is accepted, the subjective rules on every file.
    pub fn second_run_pairs(&self) -> Vec<PairRef> {
        match &self.second_run {
            Some(planned) => planned.clone(),
            None => {
                let files: Vec<String> = self
                    .in_scope_files()
                    .iter()
                    .map(|f| f.path().to_string())
                    .collect();
                plan_pairs(&self.inputs.manifest.rules, &files, &[])
            }
        }
    }

    /// The rules and in-scope files the second run's pairs span.
    fn second_run_axes(&self, files: &[String]) -> (Vec<String>, Vec<String>) {
        let pairs = self.second_run_pairs();
        let rules: BTreeSet<String> = pairs.iter().map(|p| p.rule_id.clone()).collect();
        let paths: BTreeSet<&str> = pairs.iter().map(|p| p.path.as_str()).collect();
        let files = files
            .iter()
            .filter(|f| paths.contains(f.as_str()))
            .cloned()
            .collect();
        (rules.into_iter().collect(), files)
    }
}

fn is_reserved(rule_id: &str) -> bool {
    rule_id == STANDARDS_CHANGE_RULE || rule_id == OUT_OF_SCOPE_EDIT_RULE
}

/// Rename maps from each anchor SHA to `head`, keyed by anchor SHA.
/// A failed diff, such as an anchor SHA rebased away, gives no renames and
/// the matcher falls back to the text hash.
fn rename_maps(git: &Git, shas: BTreeSet<&str>, head: &str) -> BTreeMap<String, RenameMap> {
    shas.into_iter()
        .map(|sha| {
            let map = rename_map(git, sha, head).unwrap_or_else(|e| {
                tracing::warn!(anchor = sha, head, "rename map unavailable: {e}");
                RenameMap::new()
            });
            (sha.to_string(), map)
        })
        .collect()
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
            scope: ScopeProof::default(),
            priors: vec![],
            second_run: None,
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
