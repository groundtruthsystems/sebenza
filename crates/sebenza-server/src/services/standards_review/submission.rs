//! The closed submission schema and its validation (FR-12, FR-13).
//!
//! A run submits only dispositions and findings. The schema denies unknown
//! fields, so a verdict, scope or status field is a schema rejection: those
//! are the daemon's alone. Validation then requires:
//!
//! - a disposition for every (rule, in-scope file) pair of the [`Matrix`]
//!   and for every open prior finding it lists, and nothing else;
//! - a span on every `clean` disposition;
//! - at least one finding for every pair dispositioned `finding`, and every
//!   finding on such a pair;
//! - descriptions within [`free_text`](super::free_text)'s bounds, scanned:
//!   secrets redacted, a PHI hit or a scanner error rejects.
//!
//! A rejection names what is missing or wrong by rule id, path and finding
//! id, never by quoting submitted text.

use super::findings::text_hash;
use super::free_text::{TextScanner, check_description};
use common::domain::review_records::{Outcome, PairRecord, PriorRecord, SpanRecord};
use serde::Deserialize;
use std::collections::BTreeSet;

/// The most problems one rejection lists.
pub const MAX_PROBLEMS: usize = 50;

/// A submitted line span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Span {
    pub start_line: u32,
    pub end_line: u32,
}

/// The disposition of one (rule, file) pair.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDisposition {
    pub rule_id: String,
    pub path: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub span: Option<Span>,
}

/// The disposition of one open prior finding.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriorDisposition {
    pub finding_id: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub span: Option<Span>,
}

/// One reported finding.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FindingClaim {
    pub rule_id: String,
    pub path: String,
    pub line: u32,
    pub description: String,
}

/// The whole submission body.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Submission {
    pub dispositions: Vec<PairDisposition>,
    #[serde(default)]
    pub prior_findings: Vec<PriorDisposition>,
    #[serde(default)]
    pub findings: Vec<FindingClaim>,
}

/// What a run must disposition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matrix {
    pub rule_ids: Vec<String>,
    /// In-scope file paths at head.
    pub files: Vec<String>,
    /// Ids of the open prior findings the claim listed.
    pub priors: Vec<String>,
}

impl Matrix {
    /// Whether nothing at all must be dispositioned.
    pub fn is_empty(&self) -> bool {
        (self.rule_ids.is_empty() || self.files.is_empty()) && self.priors.is_empty()
    }
}

/// A refused submission: a kebab-case code and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub code: &'static str,
    pub problems: Vec<String>,
}

impl Rejection {
    fn new(code: &'static str, mut problems: Vec<String>) -> Rejection {
        problems.truncate(MAX_PROBLEMS);
        Rejection { code, problems }
    }
}

/// A validated finding, its description scanned and hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub rule_id: String,
    pub path: String,
    pub line: u32,
    pub description: String,
    pub text_hash: String,
}

/// A submission that passed every check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidSubmission {
    pub pairs: Vec<PairRecord>,
    pub priors: Vec<PriorRecord>,
    pub claims: Vec<Claim>,
    /// Secret labels redacted from descriptions.
    pub redactions: Vec<&'static str>,
}

/// Parse, check and scan `body` against `matrix`.
pub fn validate(
    body: &str,
    matrix: &Matrix,
    scanner: &dyn TextScanner,
) -> Result<ValidSubmission, Rejection> {
    let mut sub = parse(body)?;
    check_matrix(&sub, matrix)?;
    check_descriptions(&sub)?;
    let redactions = scan_descriptions(&mut sub, scanner)?;
    Ok(into_valid(sub, redactions))
}

/// Parse `body` against the closed schema.
pub fn parse(body: &str) -> Result<Submission, Rejection> {
    serde_json::from_str(body).map_err(|e| Rejection::new("schema", vec![e.to_string()]))
}

/// Check that `sub` dispositions exactly `matrix`, with spans on `clean`.
pub fn check_matrix(sub: &Submission, matrix: &Matrix) -> Result<(), Rejection> {
    if sub.dispositions.is_empty() && sub.prior_findings.is_empty() && !matrix.is_empty() {
        let pairs = matrix.rule_ids.len() * matrix.files.len();
        let need = format!(
            "{pairs} (rule, file) pairs and {} prior findings need a disposition",
            matrix.priors.len()
        );
        return Err(Rejection::new("empty-matrix", vec![need]));
    }
    let mut problems = pair_problems(sub, matrix);
    problems.extend(prior_problems(sub, matrix));
    problems.extend(finding_problems(sub));
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Rejection::new("incomplete-matrix", problems))
    }
}

fn pair_name(rule_id: &str, path: &str) -> String {
    format!("rule `{rule_id}` x file `{path}`")
}

fn span_problem(outcome: Outcome, span: Option<Span>, what: &str) -> Option<String> {
    match (outcome, span) {
        (Outcome::Clean, None) => Some(format!("clean disposition for {what} cites no span")),
        (_, Some(s)) if s.start_line == 0 || s.start_line > s.end_line => {
            Some(format!("invalid span for {what}"))
        }
        _ => None,
    }
}

fn pair_problems(sub: &Submission, matrix: &Matrix) -> Vec<String> {
    let rules: BTreeSet<&str> = matrix.rule_ids.iter().map(String::as_str).collect();
    let files: BTreeSet<&str> = matrix.files.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut problems = Vec::new();
    for d in &sub.dispositions {
        let name = pair_name(&d.rule_id, &d.path);
        if !rules.contains(d.rule_id.as_str()) {
            problems.push(format!("unknown rule `{}`", d.rule_id));
        } else if !files.contains(d.path.as_str()) {
            problems.push(format!("`{}` is not an in-scope file", d.path));
        } else if !seen.insert((d.rule_id.as_str(), d.path.as_str())) {
            problems.push(format!("repeated disposition for {name}"));
        }
        problems.extend(span_problem(d.outcome, d.span, &name));
    }
    for rule in &rules {
        for file in files.iter().filter(|f| !seen.contains(&(*rule, **f))) {
            problems.push(format!("missing disposition for {}", pair_name(rule, file)));
        }
    }
    problems
}

fn prior_problems(sub: &Submission, matrix: &Matrix) -> Vec<String> {
    let wanted: BTreeSet<&str> = matrix.priors.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut problems = Vec::new();
    for p in &sub.prior_findings {
        let name = format!("prior finding `{}`", p.finding_id);
        if !wanted.contains(p.finding_id.as_str()) {
            problems.push(format!("{name} is not open for this run"));
        } else if !seen.insert(p.finding_id.as_str()) {
            problems.push(format!("repeated disposition for {name}"));
        }
        problems.extend(span_problem(p.outcome, p.span, &name));
    }
    for id in wanted.iter().filter(|id| !seen.contains(*id)) {
        problems.push(format!("missing disposition for prior finding `{id}`"));
    }
    problems
}

fn finding_problems(sub: &Submission) -> Vec<String> {
    let flagged: BTreeSet<(&str, &str)> = sub
        .dispositions
        .iter()
        .filter(|d| d.outcome == Outcome::Finding)
        .map(|d| (d.rule_id.as_str(), d.path.as_str()))
        .collect();
    let reported: BTreeSet<(&str, &str)> = sub
        .findings
        .iter()
        .map(|f| (f.rule_id.as_str(), f.path.as_str()))
        .collect();
    let mut problems = Vec::new();
    for f in &sub.findings {
        let name = pair_name(&f.rule_id, &f.path);
        if !flagged.contains(&(f.rule_id.as_str(), f.path.as_str())) {
            problems.push(format!("a finding on {name} has no `finding` disposition"));
        }
        if f.line == 0 {
            problems.push(format!("a finding on {name} has line 0"));
        }
    }
    for (rule, path) in flagged.difference(&reported) {
        let name = pair_name(rule, path);
        problems.push(format!(
            "{name} is dispositioned `finding` but reports none"
        ));
    }
    problems
}

fn finding_name(index: usize, f: &FindingClaim) -> String {
    format!(
        "finding {} on {}",
        index + 1,
        pair_name(&f.rule_id, &f.path)
    )
}

fn check_descriptions(sub: &Submission) -> Result<(), Rejection> {
    for (i, f) in sub.findings.iter().enumerate() {
        check_description(&f.description)
            .map_err(|p| Rejection::new(p.code(), vec![finding_name(i, f)]))?;
    }
    Ok(())
}

/// Redact each description in place; a PHI hit or a scanner error rejects.
fn scan_descriptions(
    sub: &mut Submission,
    scanner: &dyn TextScanner,
) -> Result<Vec<&'static str>, Rejection> {
    let mut redactions: Vec<&'static str> = Vec::new();
    for (i, f) in sub.findings.iter_mut().enumerate() {
        let scanned = scanner
            .scan(&f.description)
            .map_err(|_| Rejection::new("scanner-error", vec![finding_name(i, f)]))?;
        if !scanned.phi.is_empty() {
            let hit = format!("{}: {}", finding_name(i, f), scanned.phi.join(", "));
            return Err(Rejection::new("likely-phi", vec![hit]));
        }
        f.description = scanned.text;
        for r in scanned.redactions {
            if !redactions.contains(&r) {
                redactions.push(r);
            }
        }
    }
    Ok(redactions)
}

fn span_record(s: Option<Span>) -> Option<SpanRecord> {
    s.map(|s| SpanRecord {
        start_line: s.start_line,
        end_line: s.end_line,
    })
}

fn into_valid(sub: Submission, redactions: Vec<&'static str>) -> ValidSubmission {
    let pairs = sub
        .dispositions
        .into_iter()
        .map(|d| PairRecord {
            rule_id: d.rule_id,
            path: d.path,
            outcome: d.outcome,
            span: span_record(d.span),
        })
        .collect();
    let priors = sub
        .prior_findings
        .into_iter()
        .map(|p| PriorRecord {
            finding_id: p.finding_id,
            outcome: p.outcome,
            span: span_record(p.span),
        })
        .collect();
    let claims = sub
        .findings
        .into_iter()
        .map(|f| Claim {
            text_hash: text_hash(&f.description),
            rule_id: f.rule_id,
            path: f.path,
            line: f.line,
            description: f.description,
        })
        .collect();
    ValidSubmission {
        pairs,
        priors,
        claims,
        redactions,
    }
}

#[cfg(test)]
mod tests {
    use super::super::free_text::{ContentScanner, ScanError, Scanned};
    use super::*;
    use serde_json::{Value, json};

    pub(crate) fn matrix() -> Matrix {
        Matrix {
            rule_ids: vec!["long-methods".into(), "no-restating".into()],
            files: vec!["src/a.rs".into(), "src/b.rs".into()],
            priors: vec!["f_prior".into()],
        }
    }

    fn clean(rule: &str, path: &str) -> Value {
        json!({"ruleId": rule, "path": path, "outcome": "clean", "span": {"startLine": 1, "endLine": 9}})
    }

    fn full() -> Value {
        json!({
            "dispositions": [
                clean("long-methods", "src/a.rs"),
                {"ruleId": "long-methods", "path": "src/b.rs", "outcome": "finding"},
                clean("no-restating", "src/a.rs"),
                clean("no-restating", "src/b.rs"),
            ],
            "priorFindings": [
                {"findingId": "f_prior", "outcome": "clean", "span": {"startLine": 4, "endLine": 7}}
            ],
            "findings": [
                {"ruleId": "long-methods", "path": "src/b.rs", "line": 12,
                 "description": "Function `load` has 31 logical lines; the rule allows 20."}
            ]
        })
    }

    fn check(v: &Value) -> Result<ValidSubmission, Rejection> {
        validate(&v.to_string(), &matrix(), &ContentScanner::default())
    }

    struct Failing;
    impl TextScanner for Failing {
        fn scan(&self, _: &str) -> Result<Scanned, ScanError> {
            Err(ScanError("scanner down".into()))
        }
    }

    #[test]
    fn ts18_a_complete_matrix_with_spans_is_accepted() {
        let ok = check(&full()).unwrap();
        assert_eq!(ok.pairs.len(), 4);
        assert_eq!(ok.priors.len(), 1);
        assert_eq!(ok.claims.len(), 1);
        assert_eq!(ok.claims[0].text_hash, text_hash(&ok.claims[0].description));
        assert_eq!(
            ok.pairs[0].span,
            Some(SpanRecord {
                start_line: 1,
                end_line: 9
            })
        );
    }

    #[test]
    fn ts19_a_missing_pair_is_rejected_by_name() {
        let mut v = full();
        v["dispositions"].as_array_mut().unwrap().remove(3);
        let r = check(&v).unwrap_err();
        assert_eq!(r.code, "incomplete-matrix");
        assert_eq!(r.problems.len(), 1);
        assert!(r.problems[0].contains("no-restating"), "{:?}", r.problems);
        assert!(r.problems[0].contains("src/b.rs"), "{:?}", r.problems);
    }

    #[test]
    fn ts20_an_omitted_prior_finding_is_rejected_by_id() {
        let mut v = full();
        v["priorFindings"] = json!([]);
        let r = check(&v).unwrap_err();
        assert_eq!(r.code, "incomplete-matrix");
        assert!(
            r.problems.iter().any(|p| p.contains("f_prior")),
            "{:?}",
            r.problems
        );
    }

    #[test]
    fn ts21_an_empty_matrix_is_rejected() {
        let r = check(&json!({"dispositions": []})).unwrap_err();
        assert_eq!(r.code, "empty-matrix");
    }

    #[test]
    fn nothing_to_disposition_accepts_an_empty_submission() {
        let none = Matrix::default();
        assert!(none.is_empty());
        let ok = validate(r#"{"dispositions": []}"#, &none, &ContentScanner::default());
        assert_eq!(ok, Ok(ValidSubmission::default()));
    }

    #[test]
    fn ts22_verdict_scope_and_status_fields_are_unknown() {
        for (path, field) in [
            (None, "verdict"),
            (None, "scope"),
            (None, "status"),
            (Some("dispositions"), "status"),
            (Some("findings"), "status"),
            (Some("priorFindings"), "verdict"),
        ] {
            let mut v = full();
            match path {
                None => v[field] = json!("passing"),
                Some(list) => v[list][0][field] = json!("resolved"),
            }
            let r = check(&v).unwrap_err();
            assert_eq!(r.code, "schema", "{field}");
            assert!(
                r.problems[0].contains(&format!("unknown field `{field}`")),
                "{:?}",
                r.problems
            );
        }
    }

    #[test]
    fn a_clean_disposition_without_a_span_is_rejected() {
        let mut v = full();
        v["dispositions"][0].as_object_mut().unwrap().remove("span");
        let r = check(&v).unwrap_err();
        assert_eq!(r.code, "incomplete-matrix");
        assert!(r.problems[0].contains("span"), "{:?}", r.problems);
        let mut v = full();
        v["dispositions"][0]["span"] = json!({"startLine": 9, "endLine": 2});
        assert_eq!(check(&v).unwrap_err().code, "incomplete-matrix");
    }

    #[test]
    fn pairs_and_priors_outside_the_matrix_or_repeated_are_rejected() {
        for (list, extra) in [
            ("dispositions", clean("long-methods", "src/other.rs")),
            ("dispositions", clean("unknown-rule", "src/a.rs")),
            ("dispositions", clean("long-methods", "src/a.rs")),
            (
                "priorFindings",
                json!({"findingId": "f_other", "outcome": "finding"}),
            ),
        ] {
            let mut v = full();
            v[list].as_array_mut().unwrap().push(extra.clone());
            assert_eq!(check(&v).unwrap_err().code, "incomplete-matrix", "{extra}");
        }
    }

    #[test]
    fn findings_must_sit_on_a_pair_dispositioned_finding() {
        let mut v = full();
        v["findings"] = json!([]);
        let r = check(&v).unwrap_err();
        assert_eq!(r.code, "incomplete-matrix");
        assert!(r.problems[0].contains("src/b.rs"), "{:?}", r.problems);
        let mut v = full();
        v["findings"][0]["path"] = json!("src/a.rs");
        assert_eq!(check(&v).unwrap_err().code, "incomplete-matrix");
    }

    #[test]
    fn long_or_excerpted_descriptions_are_rejected() {
        let mut v = full();
        v["findings"][0]["description"] = json!("x".repeat(501));
        assert_eq!(check(&v).unwrap_err().code, "description-too-long");
        let mut v = full();
        v["findings"][0]["description"] = json!("```\nfn a() {}\n```");
        assert_eq!(check(&v).unwrap_err().code, "code-excerpt");
    }

    #[test]
    fn ts50_a_secret_is_redacted_and_phi_or_a_scanner_error_rejects() {
        let key = format!("ghp_{}", "a1b2c3d4e5f6g7h8i9j0");
        let mut v = full();
        v["findings"][0]["description"] = json!(format!("Token {key} is hard-coded."));
        let ok = check(&v).unwrap();
        assert!(!ok.claims[0].description.contains(&key));
        assert_eq!(ok.redactions, vec!["GitHub token"]);

        let mut v = full();
        v["findings"][0]["description"] = json!("Fixture holds SSN 123-45-6789.");
        let r = check(&v).unwrap_err();
        assert_eq!(r.code, "likely-phi");
        assert!(
            r.problems.iter().all(|p| !p.contains("123-45-6789")),
            "{:?}",
            r.problems
        );

        let r = validate(&full().to_string(), &matrix(), &Failing).unwrap_err();
        assert_eq!(r.code, "scanner-error");
    }

    #[test]
    fn a_body_that_is_not_json_is_a_schema_rejection() {
        assert_eq!(parse("not json").unwrap_err().code, "schema");
        assert_eq!(parse(r#"{"dispositions": {}}"#).unwrap_err().code, "schema");
    }
}
