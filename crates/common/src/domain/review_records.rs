//! The standards-review records the review log carries: runs, leases,
//! attempts, submissions, findings and verdicts.
//!
//! Records hold ids, hashes, paths and line numbers. The only free text is a
//! finding's description, which the daemon bounds, refuses code excerpts in
//! and secret-redacts before it is ever built into a record. A run token is
//! present only as its SHA-256 hash.

use serde::{Deserialize, Serialize};

/// Which review a run performs. Attempts are counted per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunKind {
    /// The review agent's full pass over the matrix.
    First,
    /// The independent, fresh-context pass over subjective rules (phase 5).
    Second,
}

impl RunKind {
    /// The kebab-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            RunKind::First => "first",
            RunKind::Second => "second",
        }
    }
}

/// Where a finding is in its lifecycle (data-architecture finding states).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingStatus {
    Open,
    NeedsOperatorDecision,
    Resolved,
    ResolvedOutOfScope,
    RuleWithdrawn,
}

impl FindingStatus {
    /// Every status, for table-driven checks.
    pub const ALL: [FindingStatus; 5] = [
        FindingStatus::Open,
        FindingStatus::NeedsOperatorDecision,
        FindingStatus::Resolved,
        FindingStatus::ResolvedOutOfScope,
        FindingStatus::RuleWithdrawn,
    ];

    /// A terminal status never changes again; a regression is a new finding.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            FindingStatus::Resolved
                | FindingStatus::ResolvedOutOfScope
                | FindingStatus::RuleWithdrawn
        )
    }

    /// Whether a finding in this status keeps the verdict from passing.
    pub fn blocks_passing(self) -> bool {
        matches!(
            self,
            FindingStatus::Open | FindingStatus::NeedsOperatorDecision
        )
    }

    /// The kebab-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            FindingStatus::Open => "open",
            FindingStatus::NeedsOperatorDecision => "needs-operator-decision",
            FindingStatus::Resolved => "resolved",
            FindingStatus::ResolvedOutOfScope => "resolved-out-of-scope",
            FindingStatus::RuleWithdrawn => "rule-withdrawn",
        }
    }
}

/// Which tree an anchor's line is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorSide {
    Base,
    Head,
}

/// Where a finding points: a path and line in the tree at `sha`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorRecord {
    pub path: String,
    pub line: u32,
    pub side: AnchorSide,
    pub sha: String,
}

/// Who raised a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingOrigin {
    /// A review run reported it.
    Agent,
    /// The daemon: the track edits its own standards (FR-10).
    StandardsChange,
    /// A daemon measurer (phase 5).
    Measured,
    /// The daemon: a change outside its task's approved edit boundary (FR-21).
    Boundary,
}

/// A finding as first raised. Its status lives in the fold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingRecord {
    pub finding_id: String,
    pub track_id: String,
    /// The job that raised it.
    pub job_id: String,
    pub rule_id: String,
    pub anchor: AnchorRecord,
    /// SHA-256 of the normalised description, for matching across jobs.
    pub text_hash: String,
    /// Bounded, excerpt-free, secret-redacted.
    pub description: String,
    pub origin: FindingOrigin,
}

/// A run's verdict on one (rule, file) pair or one prior finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Finding,
    Clean,
}

/// The line range a `clean` disposition cites, inclusive and 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanRecord {
    pub start_line: u32,
    pub end_line: u32,
}

/// The disposition of one (rule, in-scope file) pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRecord {
    pub rule_id: String,
    pub path: String,
    pub outcome: Outcome,
    pub span: Option<SpanRecord>,
}

/// The disposition of one open prior finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorRecord {
    pub finding_id: String,
    pub outcome: Outcome,
    pub span: Option<SpanRecord>,
}

/// One finding status change and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub finding_id: String,
    pub from: FindingStatus,
    pub to: FindingStatus,
    /// A kebab-case cause such as `verified-clean` or `out-of-scope`.
    pub cause: String,
}

/// The daemon's verdict on a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictRecord {
    pub passing: bool,
    /// Kebab-case reasons it is not passing; empty when it is.
    pub reasons: Vec<String>,
}

/// A run leased to a worker, with the hash of its single-use token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLeased {
    pub job_id: String,
    pub track_id: String,
    pub run_id: String,
    /// Serialised as `run_kind`: `kind` is the event tag.
    #[serde(rename = "run_kind")]
    pub kind: RunKind,
    /// 1-based, per (job, kind).
    pub attempt: u32,
    pub lease_id: String,
    pub token_hash: String,
    /// Unix seconds; the lease and its token die then.
    pub lease_until: u64,
}

/// Why an attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptCause {
    LeaseExpired,
    Rejected,
    /// The job's inputs could not be resolved from git; the run ends at
    /// once rather than waiting out its lease.
    InputsUnavailable,
}

/// A run that ended without an accepted submission. Idempotent on
/// (job, attempt, lease id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptFailed {
    pub job_id: String,
    pub run_id: String,
    /// Serialised as `run_kind`: `kind` is the event tag.
    #[serde(rename = "run_kind")]
    pub kind: RunKind,
    pub attempt: u32,
    pub lease_id: String,
    pub cause: AttemptCause,
    /// A kebab-case rejection or input-failure code; `None` for an expiry.
    pub reason: Option<String>,
}

/// One reason a job's inputs cannot pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerRecord {
    pub code: String,
    pub reason: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    /// The commit a `scope-unproven` blocker is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// A claim found the job's inputs blocked: the run ends without an attempt
/// and the job is `standards-unavailable` or `scope-unproven`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobBlocked {
    pub job_id: String,
    pub run_id: String,
    pub lease_id: String,
    pub blockers: Vec<BlockerRecord>,
}

/// An accepted submission, which consumes the run token, and what the
/// daemon derived from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSubmitted {
    pub job_id: String,
    pub run_id: String,
    /// Serialised as `run_kind`: `kind` is the event tag.
    #[serde(rename = "run_kind")]
    pub kind: RunKind,
    pub lease_id: String,
    /// The rule ids of the job's manifest, so a later job can tell a
    /// withdrawn rule from one that never applied.
    pub manifest_rule_ids: Vec<String>,
    pub pairs: Vec<PairRecord>,
    pub priors: Vec<PriorRecord>,
    pub opened: Vec<FindingRecord>,
    pub transitions: Vec<TransitionRecord>,
    pub verdict: VerdictRecord,
    /// First run only: the (rule, file) pairs the daemon scheduled for the
    /// second run. Empty when no second run is needed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub second_run: Vec<PairRef>,
    /// First run only: informational notes, never findings (FR-26).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<NoteRecord>,
}

/// One (rule, in-scope file) pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PairRef {
    pub rule_id: String,
    pub path: String,
}

/// An informational note: a code and the category it concerns. Notes carry
/// no free text; the views render the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRecord {
    /// A kebab-case code such as `category-rule-absent`.
    pub code: String,
    pub category: String,
}

/// The operator decided a `needs-operator-decision` finding (FR-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingDecided {
    pub finding_id: String,
    pub track_id: String,
    /// The track's latest job when decided.
    pub job_id: String,
    pub head_sha: String,
    pub base_sha: String,
    pub from: FindingStatus,
    pub to: FindingStatus,
    /// The credential class that decided (`operator_session`, `operator_cli`).
    pub credential: String,
    /// The job's verdict re-derived after the move.
    pub verdict: VerdictRecord,
}

/// The operator signed off a track's latest job (FR-22): the audit record of
/// the compare-and-swap gate. Ids, SHAs and the credential class only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedOff {
    pub track_id: String,
    pub job_id: String,
    /// The verdict's head SHA, still the track ref's at sign-off.
    pub head_sha: String,
    /// The verdict's merge-base, still the target's at sign-off.
    pub base_sha: String,
    /// The credential class that signed off (`operator_session`, `operator_cli`).
    pub credential: String,
    /// Every finding of the track at sign-off, none of them unresolved.
    pub finding_ids: Vec<String>,
}

/// The existing sebenza-review check results for a job: a required verdict
/// input (FR-18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingChecksRecorded {
    pub job_id: String,
    /// The ids of failed checks; empty when all passed.
    pub failed: Vec<String>,
    /// The job's verdict re-derived with these results, once a run of the
    /// job has been accepted; `None` before that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<VerdictRecord>,
}

/// One task's edit boundary: the path globs its commits may change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBoundary {
    pub task_id: String,
    /// Empty when the task declares no `files`: nothing is in bounds.
    pub files: Vec<String>,
}

/// The operator approved a track's plan (FR-20): the plan as committed at
/// `head_sha` and the per-task boundary snapshot taken from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanApproved {
    pub track_id: String,
    pub head_sha: String,
    /// SHA-256 of the plan.json blob at `head_sha`.
    pub plan_sha256: String,
    /// SHA-256 of the plan without its progress fields; a later plan whose
    /// content hash differs makes this boundary stale.
    pub plan_content_sha256: String,
    pub boundaries: Vec<TaskBoundary>,
    /// The credential class that approved (`operator`).
    pub credential: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_serialise_as_kebab_case_and_know_terminality() {
        let names: Vec<_> = FindingStatus::ALL.iter().map(|s| s.as_str()).collect();
        for (s, name) in FindingStatus::ALL.iter().zip(&names) {
            assert_eq!(serde_json::to_value(s).unwrap(), *name);
        }
        let terminal: Vec<_> = FindingStatus::ALL
            .into_iter()
            .filter(|s| s.is_terminal())
            .collect();
        assert_eq!(
            terminal,
            vec![
                FindingStatus::Resolved,
                FindingStatus::ResolvedOutOfScope,
                FindingStatus::RuleWithdrawn
            ]
        );
        assert!(FindingStatus::NeedsOperatorDecision.blocks_passing());
        assert!(!FindingStatus::Resolved.blocks_passing());
        assert_eq!(serde_json::to_value(RunKind::Second).unwrap(), "second");
    }
}
