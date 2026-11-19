//! The verdict deriver (FR-17, FR-18): only the daemon moves a finding or
//! decides a verdict, from an accepted submission and the job's inputs.
//!
//! For each prior finding still open, in order:
//! 1. `rule-withdrawn` when its rule was in the manifest of the job that
//!    raised it but is absent from this job's merge-base manifest. Rules
//!    always come from the merge-base, so a removal on the track alone never
//!    withdraws a rule and the finding stays open.
//! 2. `resolved-out-of-scope` when its path, carried through the rename
//!    map, has no hunk in this job's scope diff.
//! 3. `resolved` when the run dispositioned it `clean` and no reported
//!    finding continues it.
//!
//! Reported and measured findings that continue no open prior finding are
//! opened; the [`matcher`](super::matcher) never offers a terminal one, so a
//! regression is a new finding. A `standards_change` marker raises one
//! `standards-change` finding unless one is still open.
//!
//! The verdict passes only with a complete matrix (guaranteed by validation
//! before derivation), no open or needs-operator-decision finding, no
//! pending second run, and existing checks recorded with none failed.
//! Missing existing checks fail closed.

use super::findings::{Actor, check_transition, text_hash};
use super::jobs::Finding;
use super::matcher::{ClaimKey, PriorKey, match_claims};
use super::submission::{Claim, ValidSubmission};
use common::domain::review_records::{
    AnchorRecord, AnchorSide, FindingOrigin, FindingRecord, FindingStatus, Outcome,
    TransitionRecord, VerdictRecord,
};
use std::collections::BTreeSet;

/// The reserved rule id of the daemon's standards-change finding.
pub const STANDARDS_CHANGE_RULE: &str = "standards-change";

/// Verdict reason: a finding is open.
pub const OPEN_FINDINGS: &str = "open-findings";
/// Verdict reason: a finding needs an operator decision.
pub const UNDECIDED_FINDINGS: &str = "undecided-findings";
/// Verdict reason: a second run is scheduled but not finished.
pub const SECOND_RUN_PENDING: &str = "second-run-pending";
/// Verdict reason: no existing-check results were recorded for the job.
pub const EXISTING_CHECKS_MISSING: &str = "existing-checks-missing";
/// Verdict reason: an existing sebenza-review check failed.
pub const EXISTING_CHECKS_FAILED: &str = "existing-checks-failed";

/// A prior finding of the track that is not terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorFinding {
    pub finding: Finding,
    /// Its anchor path carried to this job's head through the rename map.
    pub path_at_head: String,
    /// Whether its rule was in the manifest of the job that raised it.
    pub rule_in_prior_manifest: bool,
}

/// A numeric-rule breach a daemon measurer found (phase 5 fills these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasuredFinding {
    pub rule_id: String,
    pub path: String,
    pub line: u32,
    /// Daemon-written, never agent text.
    pub description: String,
}

/// Everything one derivation reads.
#[derive(Debug, Clone)]
pub struct DeriveInputs<'a> {
    pub job_id: &'a str,
    pub track_id: &'a str,
    pub head_sha: &'a str,
    pub manifest_rule_ids: &'a BTreeSet<String>,
    /// Every path, old and new, with a hunk in this job's scope diff.
    pub scope_paths: &'a BTreeSet<String>,
    /// The standards documents the track edits, if any.
    pub standards_change: Option<&'a [String]>,
    pub priors: &'a [PriorFinding],
    pub submission: &'a ValidSubmission,
    pub measured_findings: &'a [MeasuredFinding],
    pub second_run_pending: bool,
    /// Failed existing-check ids; `None` when none were recorded.
    pub existing_checks: Option<&'a [String]>,
}

/// What a derivation decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derivation {
    pub opened: Vec<FindingRecord>,
    pub transitions: Vec<TransitionRecord>,
    pub verdict: VerdictRecord,
}

/// Derive findings and the verdict; `new_id` names each opened finding.
pub fn derive(inputs: &DeriveInputs, new_id: impl FnMut() -> String) -> Derivation {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::standards_review::jobs::tests::finding;
    use common::domain::review_records::{PriorRecord, SpanRecord};

    struct Fixture {
        manifest: BTreeSet<String>,
        scope: BTreeSet<String>,
        priors: Vec<PriorFinding>,
        submission: ValidSubmission,
        measured: Vec<MeasuredFinding>,
        standards_change: Option<Vec<String>>,
        second_run_pending: bool,
        existing_checks: Option<Vec<String>>,
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn open_prior(id: &str, rule: &str, path: &str) -> PriorFinding {
        PriorFinding {
            finding: Finding {
                record: finding(id, "job_prev", rule, path),
                status: FindingStatus::Open,
            },
            path_at_head: path.into(),
            rule_in_prior_manifest: true,
        }
    }

    fn fixture() -> Fixture {
        Fixture {
            manifest: set(&["long"]),
            scope: set(&["src/a.rs"]),
            priors: vec![],
            submission: ValidSubmission::default(),
            measured: vec![],
            standards_change: None,
            second_run_pending: false,
            existing_checks: Some(vec![]),
        }
    }

    fn run(f: &Fixture) -> Derivation {
        let mut n = 0;
        derive(
            &DeriveInputs {
                job_id: "job_now",
                track_id: "t1",
                head_sha: "h2",
                manifest_rule_ids: &f.manifest,
                scope_paths: &f.scope,
                standards_change: f.standards_change.as_deref(),
                priors: &f.priors,
                submission: &f.submission,
                measured_findings: &f.measured,
                second_run_pending: f.second_run_pending,
                existing_checks: f.existing_checks.as_deref(),
            },
            || {
                n += 1;
                format!("f_new{n}")
            },
        )
    }

    fn prior_disposition(id: &str, outcome: Outcome) -> PriorRecord {
        PriorRecord {
            finding_id: id.into(),
            outcome,
            span: Some(SpanRecord {
                start_line: 1,
                end_line: 5,
            }),
        }
    }

    fn claim(rule: &str, path: &str, text: &str) -> Claim {
        Claim {
            rule_id: rule.into(),
            path: path.into(),
            line: 7,
            description: text.into(),
            text_hash: text_hash(text),
        }
    }

    fn moved(d: &Derivation) -> Vec<(String, FindingStatus)> {
        d.transitions
            .iter()
            .map(|t| (t.finding_id.clone(), t.to))
            .collect()
    }

    #[test]
    fn a_clean_job_with_green_checks_passes() {
        let d = run(&fixture());
        assert_eq!(
            d.verdict,
            VerdictRecord {
                passing: true,
                reasons: vec![]
            }
        );
        assert!(d.opened.is_empty() && d.transitions.is_empty());
    }

    #[test]
    fn a_prior_dispositioned_clean_is_resolved() {
        let mut f = fixture();
        f.priors = vec![open_prior("f1", "long", "src/a.rs")];
        f.submission.priors = vec![prior_disposition("f1", Outcome::Clean)];
        let d = run(&f);
        assert_eq!(moved(&d), vec![("f1".into(), FindingStatus::Resolved)]);
        assert_eq!(d.transitions[0].cause, "verified-clean");
        assert!(d.verdict.passing);
    }

    #[test]
    fn a_prior_still_present_stays_open_and_blocks_the_verdict() {
        let mut f = fixture();
        f.priors = vec![open_prior("f1", "long", "src/a.rs")];
        f.submission.priors = vec![prior_disposition("f1", Outcome::Finding)];
        let d = run(&f);
        assert!(d.transitions.is_empty());
        assert!(!d.verdict.passing);
        assert_eq!(d.verdict.reasons, vec![OPEN_FINDINGS]);
    }

    #[test]
    fn ts34_a_finding_whose_hunk_left_the_scope_is_resolved_out_of_scope() {
        let mut f = fixture();
        f.priors = vec![open_prior("f1", "long", "src/gone.rs")];
        let d = run(&f);
        assert_eq!(
            moved(&d),
            vec![("f1".into(), FindingStatus::ResolvedOutOfScope)]
        );
        assert!(d.verdict.passing);
    }

    #[test]
    fn ts35_a_rule_absent_at_the_new_merge_base_withdraws_its_findings() {
        let mut f = fixture();
        f.manifest = set(&["other"]);
        f.priors = vec![open_prior("f1", "long", "src/a.rs")];
        let d = run(&f);
        assert_eq!(moved(&d), vec![("f1".into(), FindingStatus::RuleWithdrawn)]);
        assert_eq!(d.transitions[0].cause, "rule-withdrawn");
    }

    #[test]
    fn ts36_a_rule_removed_only_on_the_track_leaves_the_finding_open() {
        let mut f = fixture();
        f.priors = vec![open_prior("f1", "long", "src/a.rs")];
        f.submission.priors = vec![prior_disposition("f1", Outcome::Finding)];
        let d = run(&f);
        assert!(
            d.transitions.is_empty(),
            "the merge-base manifest still has the rule"
        );
        assert!(!d.verdict.passing);
    }

    #[test]
    fn a_rule_that_never_applied_does_not_withdraw() {
        let mut f = fixture();
        f.manifest = set(&["other"]);
        let mut p = open_prior("f1", "long", "src/a.rs");
        p.rule_in_prior_manifest = false;
        f.priors = vec![p];
        f.submission.priors = vec![prior_disposition("f1", Outcome::Finding)];
        assert!(run(&f).transitions.is_empty());
    }

    #[test]
    fn a_reported_finding_continuing_a_prior_opens_nothing() {
        let mut f = fixture();
        let mut p = open_prior("f1", "long", "src/a.rs");
        p.finding.record.text_hash = text_hash("Too long.");
        f.priors = vec![p];
        f.submission.priors = vec![prior_disposition("f1", Outcome::Clean)];
        f.submission.claims = vec![claim("long", "src/a.rs", "too LONG")];
        let d = run(&f);
        assert!(d.opened.is_empty());
        assert!(
            d.transitions.is_empty(),
            "a continued finding is not resolved"
        );
    }

    #[test]
    fn a_regression_after_resolution_is_a_new_finding() {
        let mut f = fixture();
        f.submission.claims = vec![claim("long", "src/a.rs", "Too long.")];
        let d = run(&f);
        assert_eq!(d.opened.len(), 1);
        let o = &d.opened[0];
        assert_eq!(o.finding_id, "f_new1");
        assert_eq!((o.job_id.as_str(), o.track_id.as_str()), ("job_now", "t1"));
        assert_eq!(o.origin, FindingOrigin::Agent);
        assert_eq!(
            o.anchor,
            AnchorRecord {
                path: "src/a.rs".into(),
                line: 7,
                side: AnchorSide::Head,
                sha: "h2".into()
            }
        );
        assert_eq!(o.text_hash, text_hash("Too long."));
        assert_eq!(d.verdict.reasons, vec![OPEN_FINDINGS]);
    }

    #[test]
    fn the_standards_change_marker_becomes_one_finding() {
        let mut f = fixture();
        f.standards_change = Some(vec![".ai/sebenza/code_styleguides/general.md".into()]);
        let d = run(&f);
        assert_eq!(d.opened.len(), 1);
        assert_eq!(d.opened[0].rule_id, STANDARDS_CHANGE_RULE);
        assert_eq!(d.opened[0].origin, FindingOrigin::StandardsChange);
        assert_eq!(
            d.opened[0].anchor.path,
            ".ai/sebenza/code_styleguides/general.md"
        );
        assert!(!d.verdict.passing);

        f.scope
            .insert(".ai/sebenza/code_styleguides/general.md".into());
        let mut still = open_prior(
            "f_sc",
            STANDARDS_CHANGE_RULE,
            ".ai/sebenza/code_styleguides/general.md",
        );
        still.rule_in_prior_manifest = false;
        f.priors = vec![still];
        assert!(
            run(&f).opened.is_empty(),
            "one standards-change finding at a time"
        );
    }

    #[test]
    fn measured_findings_open_like_reported_ones() {
        let mut f = fixture();
        f.measured = vec![MeasuredFinding {
            rule_id: "long".into(),
            path: "src/a.rs".into(),
            line: 3,
            description: "Body has 31 logical lines; the rule allows 20.".into(),
        }];
        let d = run(&f);
        assert_eq!(d.opened.len(), 1);
        assert_eq!(d.opened[0].origin, FindingOrigin::Measured);
    }

    #[test]
    fn an_undecided_prior_a_pending_second_run_or_missing_checks_block_passing() {
        let mut f = fixture();
        let mut p = open_prior("f1", "long", "src/gone.rs");
        p.finding.status = FindingStatus::NeedsOperatorDecision;
        f.priors = vec![p];
        f.second_run_pending = true;
        f.existing_checks = None;
        let d = run(&f);
        assert!(
            d.transitions.is_empty(),
            "the table refuses needs-operator-decision -> out of scope"
        );
        assert_eq!(
            d.verdict.reasons,
            vec![
                UNDECIDED_FINDINGS,
                SECOND_RUN_PENDING,
                EXISTING_CHECKS_MISSING
            ]
        );
        let mut f = fixture();
        f.existing_checks = Some(vec!["coverage".into()]);
        assert_eq!(run(&f).verdict.reasons, vec![EXISTING_CHECKS_FAILED]);
    }
}
