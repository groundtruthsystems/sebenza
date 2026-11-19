//! The existing sebenza-review checks (FR-18): requirements, test plan,
//! coverage and architecture conformance.
//!
//! The sebenza-review skill runs them in the track's own worktree, where
//! tests and coverage can run, and reports them through
//! `POST /api/review/tracks/{id}/existing-checks` (`sebenza-agentctl
//! review-checks`). The sandboxed review agent cannot run them, so its
//! submission carries none. A report holds each of the four checks exactly
//! once; a failed check is a verdict reason naming that check.

use serde::Deserialize;

/// One existing sebenza-review check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckId {
    Requirements,
    TestPlan,
    Coverage,
    Architecture,
}

impl CheckId {
    /// Every check a report must hold.
    pub const ALL: [CheckId; 4] = [
        CheckId::Requirements,
        CheckId::TestPlan,
        CheckId::Coverage,
        CheckId::Architecture,
    ];

    /// The snake_case id.
    pub fn as_str(self) -> &'static str {
        match self {
            CheckId::Requirements => "requirements",
            CheckId::TestPlan => "test_plan",
            CheckId::Coverage => "coverage",
            CheckId::Architecture => "architecture",
        }
    }
}

/// How an existing check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    NotApplicable,
}

/// One existing check's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExistingCheck {
    pub check: CheckId,
    pub status: CheckStatus,
}

/// The route's closed body: the worktree asking and its four results.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExistingChecksBody {
    pub worktree_id: String,
    pub checks: Vec<ExistingCheck>,
}

/// The ids of the failed checks in `checks`, or what is missing or
/// repeated when it does not hold each of the four exactly once.
pub fn failed_checks(checks: &[ExistingCheck]) -> Result<Vec<String>, Vec<String>> {
    let mut problems = Vec::new();
    for id in CheckId::ALL {
        match checks.iter().filter(|c| c.check == id).count() {
            0 => problems.push(format!("missing existing check `{}`", id.as_str())),
            1 => {}
            _ => problems.push(format!("repeated existing check `{}`", id.as_str())),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.check.as_str().to_string())
        .collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// All four checks, none failed.
    pub(crate) fn green_checks() -> Value {
        json!([
            {"check": "requirements", "status": "pass"},
            {"check": "test_plan", "status": "pass"},
            {"check": "coverage", "status": "not_applicable"},
            {"check": "architecture", "status": "pass"},
        ])
    }

    fn body(checks: Value) -> Result<ExistingChecksBody, serde_json::Error> {
        serde_json::from_value(json!({"worktreeId": "wt-1", "checks": checks}))
    }

    fn failed(checks: Value) -> Result<Vec<String>, Vec<String>> {
        failed_checks(&body(checks).unwrap().checks)
    }

    #[test]
    fn ts62_a_report_names_its_failed_checks() {
        assert_eq!(failed(green_checks()), Ok(vec![]));
        let mut checks = green_checks();
        checks[2]["status"] = json!("fail");
        checks[3]["status"] = json!("fail");
        assert_eq!(
            failed(checks),
            Ok(vec!["coverage".into(), "architecture".into()])
        );
    }

    #[test]
    fn ts62_each_of_the_four_checks_is_reported_exactly_once() {
        assert_eq!(failed(json!([])).unwrap_err().len(), 4);
        let mut checks = green_checks();
        checks[0]["check"] = json!("coverage");
        let problems = failed(checks).unwrap_err();
        assert!(problems.contains(&"missing existing check `requirements`".to_string()));
        assert!(problems.contains(&"repeated existing check `coverage`".to_string()));
    }

    #[test]
    fn the_body_is_closed() {
        for bad in [
            json!({"check": "lint", "status": "pass"}),
            json!({"check": "coverage", "status": "skipped"}),
            json!({"check": "coverage", "status": "pass", "note": "x"}),
        ] {
            let mut checks = green_checks();
            checks[2] = bad.clone();
            assert!(body(checks).is_err(), "{bad}");
        }
        let forged = json!({"worktreeId": "wt-1", "checks": green_checks(), "verdict": "passing"});
        assert!(serde_json::from_value::<ExistingChecksBody>(forged).is_err());
    }
}
