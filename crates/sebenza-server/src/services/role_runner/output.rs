//! The role output envelope (application-architecture.md): one JSON object per
//! reply, validated per role, capped, and scanned before anything is kept.
//!
//! Agent output is untrusted. An unknown key (an "action", a "command"), a
//! document name outside the allowlist, a field the role may not set, or
//! likely PHI rejects the whole reply. Secrets are redacted in place. Nothing
//! in a reply is ever executed: it is data the daemon may store as a proposal.

use super::Role;
use common::domain::solution::ReviewVerdict;
use common::services::content_scan::likely_phi;
use common::services::inbox_convert::redact_secrets;
use serde::{Deserialize, Serialize};

pub const ROLE_SCHEMA_VERSION: u32 = 1;
/// Whole reply, documents, and list caps (FT-13).
pub const MAX_REPLY_BYTES: usize = 256 * 1024;
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
pub const MAX_DOCUMENTS: usize = 8;
pub const MAX_NOTE_PROPOSALS: usize = 10;
pub const MAX_PROBLEMS: usize = 20;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentOut {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteProposal {
    #[serde(default)]
    pub note_id: Option<String>,
    #[serde(rename = "type")]
    pub note_type: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProblemResolution {
    pub problem_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProblemOut {
    #[serde(default)]
    pub problem_id: Option<String>,
    pub title: String,
    pub detail: String,
    /// Code review: the plan task this finding sends back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

/// A validated reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RoleOutput {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documents: Option<Vec<DocumentOut>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_proposals: Option<Vec<NoteProposal>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_resolutions: Option<Vec<ProblemResolution>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problems: Option<Vec<ProblemOut>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<ReviewVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// Secret labels redacted from this reply (set by the daemon, never read
    /// from the agent).
    #[serde(skip)]
    pub redactions: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputError {
    #[error("the reply is not a JSON object: {0}")]
    NotJson(String),
    #[error("the reply does not match the role envelope: {0}")]
    Shape(String),
    #[error("the reply is for role {got:?}, expected {want:?}")]
    WrongRole { got: String, want: &'static str },
    #[error("unsupported schema_version {0}")]
    Version(u32),
    #[error("{role} may not set {field}")]
    FieldNotAllowed {
        role: &'static str,
        field: &'static str,
    },
    #[error("{role} must set {field}")]
    FieldMissing {
        role: &'static str,
        field: &'static str,
    },
    #[error("document name {0:?} is not allowed")]
    DocumentName(String),
    #[error("{0} is over its size or count limit")]
    TooLarge(&'static str),
    #[error("an agreed verdict cannot carry problems")]
    AgreedWithProblems,
    #[error("the reply looks like it contains personal data ({0}); nothing was kept")]
    LikelyPhi(String),
}

impl OutputError {
    /// Whether one repair retry may fix it (a formatting slip), as opposed to
    /// a refusal on content.
    pub fn repairable(&self) -> bool {
        matches!(self, OutputError::NotJson(_) | OutputError::Shape(_))
    }
}

/// The JSON object in `text`: the whole text, a fenced ```json block, or the
/// span from the first `{` to the last `}`.
pub fn extract_json(text: &str) -> Option<&str> {
    let t = text.trim();
    if t.starts_with('{') && t.ends_with('}') {
        return Some(t);
    }
    if let Some(start) = t.find("```json") {
        let body = &t[start + 7..];
        if let Some(end) = body.find("```") {
            return Some(body[..end].trim());
        }
    }
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    (a < b).then(|| &t[a..=b])
}

/// Document names a role may write: the solution documents, or one handover
/// per project slug. Never a path that could climb out of `solution/`.
pub fn document_name_allowed(role: Role, name: &str) -> bool {
    const ARCH: [&str; 5] = [
        "business-architecture.md",
        "application-architecture.md",
        "data-architecture.md",
        "security-architecture.md",
        "technical-architecture.md",
    ];
    match role {
        Role::SystemAnalyst => name == "requirements.md",
        Role::SolutionsArchitect => {
            name == "solution-design.md"
                || ARCH.contains(&name)
                || name
                    .strip_prefix("handover/")
                    .and_then(|r| r.strip_suffix(".md"))
                    .is_some_and(|slug| {
                        !slug.is_empty()
                            && slug.len() <= 64
                            && slug
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    })
        }
        _ => false,
    }
}

/// Parse, validate and scan one reply for `role`. `known_secrets` are exact
/// strings (control and operator tokens) to redact wherever they appear.
pub fn parse_role_output(
    role: Role,
    text: &str,
    known_secrets: &[String],
) -> Result<RoleOutput, OutputError> {
    if text.len() > MAX_REPLY_BYTES {
        return Err(OutputError::TooLarge("reply"));
    }
    let json = extract_json(text).ok_or_else(|| OutputError::NotJson("no JSON object".into()))?;
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| OutputError::NotJson(e.to_string()))?;
    if !value.is_object() {
        return Err(OutputError::NotJson("not an object".into()));
    }
    let mut out: RoleOutput =
        serde_json::from_value(value).map_err(|e| OutputError::Shape(e.to_string()))?;
    if out.role != role.as_str() {
        return Err(OutputError::WrongRole {
            got: out.role,
            want: role.as_str(),
        });
    }
    if out.schema_version != ROLE_SCHEMA_VERSION {
        return Err(OutputError::Version(out.schema_version));
    }
    check_fields(role, &out)?;
    check_limits(role, &out)?;
    scan(&mut out, known_secrets)?;
    Ok(out)
}

fn check_fields(role: Role, o: &RoleOutput) -> Result<(), OutputError> {
    let r = role.as_str();
    let set: [(&'static str, bool); 6] = [
        ("reply", o.reply.is_some()),
        ("documents", o.documents.is_some()),
        ("note_proposals", o.note_proposals.is_some()),
        ("problem_resolutions", o.problem_resolutions.is_some()),
        ("problems", o.problems.is_some()),
        ("verdict", o.verdict.is_some()),
    ];
    let allowed: &[&str] = match role {
        Role::Coordinator => &["reply", "note_proposals", "problem_resolutions"],
        Role::SystemAnalyst | Role::SolutionsArchitect => &["documents"],
        Role::SolutionReviewer | Role::ProjectReviewer => &["problems", "verdict"],
    };
    for (field, present) in set {
        if present && !allowed.contains(&field) {
            return Err(OutputError::FieldNotAllowed { role: r, field });
        }
    }
    let required: &[&'static str] = match role {
        Role::Coordinator => &["reply"],
        Role::SystemAnalyst | Role::SolutionsArchitect => &["documents"],
        Role::SolutionReviewer | Role::ProjectReviewer => &["verdict"],
    };
    for field in required {
        if !set.iter().any(|(f, p)| f == field && *p) {
            return Err(OutputError::FieldMissing { role: r, field });
        }
    }
    if o.verdict == Some(ReviewVerdict::Agreed)
        && o.problems.as_ref().is_some_and(|p| !p.is_empty())
    {
        return Err(OutputError::AgreedWithProblems);
    }
    if let Some(docs) = &o.documents {
        for d in docs {
            if !document_name_allowed(role, &d.name) {
                return Err(OutputError::DocumentName(d.name.clone()));
            }
        }
    }
    Ok(())
}

fn check_limits(_role: Role, o: &RoleOutput) -> Result<(), OutputError> {
    let over = |s: &Option<String>| s.as_ref().is_some_and(|t| t.len() > MAX_TEXT_BYTES);
    if over(&o.reply) || over(&o.report) {
        return Err(OutputError::TooLarge("reply text"));
    }
    if let Some(d) = &o.documents
        && (d.len() > MAX_DOCUMENTS || d.iter().any(|d| d.content.len() > MAX_DOCUMENT_BYTES))
    {
        return Err(OutputError::TooLarge("documents"));
    }
    if o.note_proposals.as_ref().is_some_and(|n| {
        n.len() > MAX_NOTE_PROPOSALS || n.iter().any(|n| n.body.len() > MAX_TEXT_BYTES)
    }) {
        return Err(OutputError::TooLarge("note proposals"));
    }
    if o.problems.as_ref().is_some_and(|p| {
        p.len() > MAX_PROBLEMS || p.iter().any(|p| p.detail.len() > MAX_TEXT_BYTES)
    }) {
        return Err(OutputError::TooLarge("problems"));
    }
    if o.problem_resolutions
        .as_ref()
        .is_some_and(|p| p.len() > MAX_PROBLEMS || p.iter().any(|p| p.text.len() > MAX_TEXT_BYTES))
    {
        return Err(OutputError::TooLarge("problem resolutions"));
    }
    Ok(())
}

/// Every string the agent wrote: PHI rejects, secrets are redacted in place.
fn scan(o: &mut RoleOutput, known: &[String]) -> Result<(), OutputError> {
    let mut fields: Vec<&mut String> = Vec::new();
    fields.extend(o.reply.iter_mut());
    fields.extend(o.report.iter_mut());
    for d in o.documents.iter_mut().flatten() {
        fields.push(&mut d.content);
    }
    for n in o.note_proposals.iter_mut().flatten() {
        fields.push(&mut n.title);
        fields.push(&mut n.body);
    }
    for p in o.problem_resolutions.iter_mut().flatten() {
        fields.push(&mut p.text);
    }
    for p in o.problems.iter_mut().flatten() {
        fields.push(&mut p.title);
        fields.push(&mut p.detail);
    }
    let mut redactions: Vec<&'static str> = Vec::new();
    for f in fields {
        let phi = likely_phi(f);
        if !phi.is_empty() {
            return Err(OutputError::LikelyPhi(phi.join(", ")));
        }
        let (clean, hits) = redact_secrets(f, known);
        *f = clean;
        for h in hits {
            if !redactions.contains(&h) {
                redactions.push(h);
            }
        }
    }
    o.redactions = redactions;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reviewer(body: &str) -> String {
        format!(r#"{{"role":"solution-reviewer","schema_version":1,{body}}}"#)
    }

    #[test]
    fn a_valid_reviewer_reply_parses_from_prose_or_fences() {
        let json = reviewer(
            r#""verdict":"changes","problems":[{"problem_id":"P-1","title":"IdP","detail":"name it"}],"report":"r""#,
        );
        for text in [
            json.clone(),
            format!("Here you go:\n```json\n{json}\n```\nthanks"),
            format!("Sure. {json} Done."),
        ] {
            let o = parse_role_output(Role::SolutionReviewer, &text, &[]).expect("parses");
            assert_eq!(o.verdict, Some(ReviewVerdict::Changes));
            assert_eq!(o.problems.unwrap()[0].problem_id.as_deref(), Some("P-1"));
        }
    }

    // TS-39: output that smuggles an action or a file write is rejected or inert.
    #[test]
    fn injected_actions_and_paths_are_rejected() {
        let action = reviewer(r#""verdict":"agreed","action":"approve_design""#);
        assert!(matches!(
            parse_role_output(Role::SolutionReviewer, &action, &[]),
            Err(OutputError::Shape(_))
        ));
        let approve_field =
            r#"{"role":"solution-coordinator","schema_version":1,"reply":"ok","verdict":"agreed"}"#;
        assert!(matches!(
            parse_role_output(Role::Coordinator, approve_field, &[]),
            Err(OutputError::FieldNotAllowed {
                field: "verdict",
                ..
            })
        ));
        for name in [
            "../../.ssh/authorized_keys",
            "/etc/passwd",
            "handover/../x.md",
            "handover/Acme.md",
            "requirements.md",
        ] {
            let text = format!(
                r#"{{"role":"solutions-architect","schema_version":1,"documents":[{{"name":"{name}","content":"x"}}]}}"#
            );
            assert!(
                matches!(
                    parse_role_output(Role::SolutionsArchitect, &text, &[]),
                    Err(OutputError::DocumentName(_))
                ),
                "{name}"
            );
        }
        let ok = r#"{"role":"solutions-architect","schema_version":1,"documents":[{"name":"handover/acme-billing.md","content":"x"}]}"#;
        assert!(parse_role_output(Role::SolutionsArchitect, ok, &[]).is_ok());
        let impostor = r#"{"role":"solution-reviewer","schema_version":1,"verdict":"agreed"}"#;
        assert!(matches!(
            parse_role_output(Role::Coordinator, impostor, &[]),
            Err(OutputError::WrongRole { .. })
        ));
    }

    #[test]
    fn role_rules_and_limits_hold() {
        assert!(matches!(
            parse_role_output(Role::SolutionReviewer, &reviewer(r#""report":"x""#), &[]),
            Err(OutputError::FieldMissing {
                field: "verdict",
                ..
            })
        ));
        let agreed_with = reviewer(r#""verdict":"agreed","problems":[{"title":"t","detail":"d"}]"#);
        assert_eq!(
            parse_role_output(Role::SolutionReviewer, &agreed_with, &[]),
            Err(OutputError::AgreedWithProblems)
        );
        assert!(matches!(
            parse_role_output(
                Role::SolutionReviewer,
                &reviewer(r#""verdict":"agreed""#)
                    .replace("\"schema_version\":1", "\"schema_version\":2"),
                &[]
            ),
            Err(OutputError::Version(2))
        ));
        let big = format!(
            r#"{{"role":"system-analyst","schema_version":1,"documents":[{{"name":"requirements.md","content":"{}"}}]}}"#,
            "x".repeat(MAX_DOCUMENT_BYTES + 1)
        );
        assert_eq!(
            parse_role_output(Role::SystemAnalyst, &big, &[]),
            Err(OutputError::TooLarge("documents"))
        );
        assert!(matches!(
            parse_role_output(Role::SystemAnalyst, "I could not do it.", &[]),
            Err(OutputError::NotJson(_))
        ));
        assert!(OutputError::NotJson(String::new()).repairable());
        assert!(!OutputError::AgreedWithProblems.repairable());
    }

    // TS-43 / TS-45: token-shaped output is redacted; likely PHI rejects.
    #[test]
    fn secrets_are_redacted_and_phi_rejected() {
        let control = "0f6e2c1a-1111-4222-8333-944455556666".to_string();
        let text = format!(
            r#"{{"role":"solution-coordinator","schema_version":1,"reply":"the token is {control} and AKIAABCDEFGHIJKLMNOP"}}"#
        );
        let o =
            parse_role_output(Role::Coordinator, &text, std::slice::from_ref(&control)).unwrap();
        let reply = o.reply.unwrap();
        assert!(
            !reply.contains(&control) && !reply.contains("AKIAABCDEFGHIJKLMNOP"),
            "{reply}"
        );
        assert_eq!(o.redactions, vec!["Sebenza token", "AWS access key"]);

        let phi =
            r#"{"role":"solution-coordinator","schema_version":1,"reply":"patient MRN 1234567"}"#;
        assert!(matches!(
            parse_role_output(Role::Coordinator, phi, &[]),
            Err(OutputError::LikelyPhi(_))
        ));
    }
}
