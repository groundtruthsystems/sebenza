//! The system agent's structured output: one JSON object per job, versioned
//! by `schema_version` and keyed by `job_kind`, with unknown fields rejected
//! (AA-D2). Anything that does not parse into the expected kind is a failed
//! job; the server, never the agent, applies what a valid result says.

use crate::domain::model::Priority;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::JobKind;

/// The only output version this binary reads.
pub const OUTPUT_SCHEMA_VERSION: u64 = 1;

/// Longest `body` or `proposed_body` accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 32 * 1024;
/// Longest `rationale`, `summary` or `system_instruction` accepted, in bytes.
pub const MAX_NOTE_BYTES: usize = 8 * 1024;

/// What triage recommends doing about a request.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Recommendation {
    /// Priority only; the request waits for the operator (UC-05b).
    None,
    /// A comment for the operator; never delivered (BR-07).
    Advice,
    /// A resolution the operator may confirm and deliver.
    Proposal,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriageOutput {
    pub priority: Priority,
    pub rationale: String,
    pub recommendation: Recommendation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DraftHelpOutput {
    pub proposed_body: String,
    pub summary: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConvertTargetOutput {
    pub project: String,
    pub system_instruction: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConvertOutput {
    pub targets: Vec<ConvertTargetOutput>,
}

/// A validated job result.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "jobKind", rename_all = "snake_case")]
pub enum JobOutput {
    Triage(TriageOutput),
    DraftHelp(DraftHelpOutput),
    Convert(ConvertOutput),
}

/// Why an agent reply was refused. Messages never echo the reply's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputError {
    NotJson(String),
    NotAnObject,
    UnsupportedVersion(String),
    WrongKind { expected: &'static str, got: String },
    Schema { kind: &'static str, detail: String },
    Invalid(String),
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputError::NotJson(e) => write!(f, "agent output is not valid JSON: {e}"),
            OutputError::NotAnObject => write!(f, "agent output is not a JSON object"),
            OutputError::UnsupportedVersion(v) => {
                write!(f, "agent output has unsupported schema_version {v}")
            }
            OutputError::WrongKind { expected, got } => {
                write!(f, "agent output is for job kind {got}, expected {expected}")
            }
            OutputError::Schema { kind, detail } => {
                write!(f, "agent output does not match the {kind} schema: {detail}")
            }
            OutputError::Invalid(e) => write!(f, "agent output is invalid: {e}"),
        }
    }
}

impl std::error::Error for OutputError {}

/// Parse and validate the final message of a `kind` job. A single surrounding
/// markdown code fence is tolerated; prose around the object is not.
/// `expected_targets` lists the projects a convert job was asked about; each
/// must appear exactly once.
pub fn parse_job_output(
    kind: JobKind,
    message: &str,
    expected_targets: &[String],
) -> Result<JobOutput, OutputError> {
    let text = strip_code_fence(message.trim());
    let value: Value =
        serde_json::from_str(text).map_err(|e| OutputError::NotJson(e.to_string()))?;
    let Value::Object(mut fields) = value else {
        return Err(OutputError::NotAnObject);
    };
    match fields.remove("schema_version") {
        Some(Value::Number(n)) if n.as_u64() == Some(OUTPUT_SCHEMA_VERSION) => {}
        Some(Value::Number(n)) => return Err(OutputError::UnsupportedVersion(n.to_string())),
        Some(_) => return Err(OutputError::UnsupportedVersion("(not a number)".into())),
        None => return Err(OutputError::UnsupportedVersion("(missing)".into())),
    }
    let expected = kind.as_str();
    match fields.remove("job_kind") {
        Some(Value::String(got)) if got == expected => {}
        Some(Value::String(got)) => {
            // Name a known kind; never echo free text.
            let got = [JobKind::Triage, JobKind::DraftHelp, JobKind::Convert]
                .iter()
                .map(|k| k.as_str())
                .find(|k| *k == got)
                .unwrap_or("(unknown)")
                .to_string();
            return Err(OutputError::WrongKind { expected, got });
        }
        _ => {
            return Err(OutputError::WrongKind {
                expected,
                got: "(missing)".into(),
            });
        }
    }
    match kind {
        JobKind::Triage => {
            check_keys(
                expected,
                &fields,
                &["priority", "rationale", "recommendation"],
                &["body"],
            )?;
            let out: TriageOutput = typed(expected, fields)?;
            validate_triage(&out)?;
            Ok(JobOutput::Triage(out))
        }
        JobKind::DraftHelp => {
            check_keys(expected, &fields, &["proposed_body", "summary"], &[])?;
            let out: DraftHelpOutput = typed(expected, fields)?;
            non_blank("proposed_body", &out.proposed_body, MAX_BODY_BYTES)?;
            capped("summary", &out.summary, MAX_NOTE_BYTES)?;
            Ok(JobOutput::DraftHelp(out))
        }
        JobKind::Convert => {
            check_keys(expected, &fields, &["targets"], &[])?;
            if let Some(Value::Array(targets)) = fields.get("targets") {
                for target in targets {
                    let Value::Object(t) = target else {
                        return Err(schema(expected, "each target must be an object"));
                    };
                    check_keys(expected, t, &["project", "system_instruction"], &[])?;
                }
            }
            let out: ConvertOutput = typed(expected, fields)?;
            validate_convert(&out, expected_targets)?;
            Ok(JobOutput::Convert(out))
        }
    }
}

/// The text inside one surrounding ```` ``` ```` fence (with or without a
/// language tag), or `text` unchanged.
fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let Some(body) = rest.strip_suffix("```") else {
        return text;
    };
    // Drop the language tag line (`json`, or nothing).
    match body.split_once('\n') {
        Some((tag, inner)) if !tag.contains('{') => inner.trim(),
        _ => body.trim(),
    }
}

fn schema(kind: &'static str, detail: impl Into<String>) -> OutputError {
    OutputError::Schema {
        kind,
        detail: detail.into(),
    }
}

/// Every `required` key present, nothing outside `required` + `optional`.
/// Names an offending key (capped), never a value.
fn check_keys(
    kind: &'static str,
    fields: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
) -> Result<(), OutputError> {
    if let Some(unknown) = fields
        .keys()
        .find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        let name: String = unknown.chars().take(64).collect();
        return Err(schema(kind, format!("unknown field `{name}`")));
    }
    if let Some(missing) = required.iter().find(|k| !fields.contains_key(**k)) {
        return Err(schema(kind, format!("missing field `{missing}`")));
    }
    Ok(())
}

/// Deserialize the checked fields. serde's own message can quote values, so
/// it is replaced by a generic one.
fn typed<T: serde::de::DeserializeOwned>(
    kind: &'static str,
    fields: Map<String, Value>,
) -> Result<T, OutputError> {
    serde_json::from_value(Value::Object(fields))
        .map_err(|_| schema(kind, "a field has the wrong type or an unsupported value"))
}

fn capped(field: &str, value: &str, max: usize) -> Result<(), OutputError> {
    if value.len() > max {
        return Err(OutputError::Invalid(format!(
            "{field} is longer than {max} bytes"
        )));
    }
    Ok(())
}

fn non_blank(field: &str, value: &str, max: usize) -> Result<(), OutputError> {
    if value.trim().is_empty() {
        return Err(OutputError::Invalid(format!("{field} is empty")));
    }
    capped(field, value, max)
}

fn validate_triage(out: &TriageOutput) -> Result<(), OutputError> {
    non_blank("rationale", &out.rationale, MAX_NOTE_BYTES)?;
    match (out.recommendation, &out.body) {
        (Recommendation::Advice | Recommendation::Proposal, Some(body)) => {
            non_blank("body", body, MAX_BODY_BYTES)
        }
        (Recommendation::Advice | Recommendation::Proposal, None) => Err(OutputError::Invalid(
            "advice and proposals need a body".to_string(),
        )),
        (Recommendation::None, body) => {
            capped("body", body.as_deref().unwrap_or(""), MAX_BODY_BYTES)
        }
    }
}

fn validate_convert(out: &ConvertOutput, expected: &[String]) -> Result<(), OutputError> {
    if out.targets.is_empty() {
        return Err(OutputError::Invalid("targets is empty".to_string()));
    }
    let mut seen = std::collections::HashSet::new();
    for target in &out.targets {
        non_blank("project", &target.project, MAX_NOTE_BYTES)?;
        non_blank(
            "system_instruction",
            &target.system_instruction,
            MAX_BODY_BYTES,
        )?;
        if !seen.insert(target.project.as_str()) {
            return Err(OutputError::Invalid(
                "a target project appears twice".to_string(),
            ));
        }
        if !expected.is_empty() && !expected.contains(&target.project) {
            return Err(OutputError::Invalid(
                "a target project was not asked for".to_string(),
            ));
        }
    }
    if expected.iter().any(|p| !seen.contains(p.as_str())) {
        return Err(OutputError::Invalid(
            "a requested target project is missing".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triage_json(extra: &str) -> String {
        format!(
            r#"{{"schema_version":1,"job_kind":"triage","priority":"P1","rationale":"r","recommendation":"proposal","body":"do it"{extra}}}"#
        )
    }

    #[test]
    fn a_valid_triage_parses() {
        let out = parse_job_output(JobKind::Triage, &triage_json(""), &[]).unwrap();
        assert_eq!(
            out,
            JobOutput::Triage(TriageOutput {
                priority: Priority::P1,
                rationale: "r".into(),
                recommendation: Recommendation::Proposal,
                body: Some("do it".into()),
            })
        );
    }

    #[test]
    fn a_fenced_reply_is_tolerated() {
        let fenced = format!("```json\n{}\n```", triage_json(""));
        assert!(parse_job_output(JobKind::Triage, &fenced, &[]).is_ok());
        let bare_fence = format!("```\n{}\n```\n", triage_json(""));
        assert!(parse_job_output(JobKind::Triage, &bare_fence, &[]).is_ok());
    }

    // TS-22: malformed JSON, an unknown field and a wrong version each fail.
    #[test]
    fn malformed_unknown_field_and_wrong_version_are_rejected() {
        let bad = parse_job_output(JobKind::Triage, r#"{"schema_version":1,"#, &[]);
        assert!(matches!(bad, Err(OutputError::NotJson(_))), "{bad:?}");

        let unknown =
            parse_job_output(JobKind::Triage, &triage_json(r#","deliver_now":true"#), &[]);
        assert!(
            matches!(&unknown, Err(OutputError::Schema { detail, .. }) if detail.contains("deliver_now")),
            "{unknown:?}"
        );

        let v2 = triage_json("").replace(r#""schema_version":1"#, r#""schema_version":2"#);
        assert!(matches!(
            parse_job_output(JobKind::Triage, &v2, &[]),
            Err(OutputError::UnsupportedVersion(_))
        ));
        let missing = triage_json("").replace(r#""schema_version":1,"#, "");
        assert!(matches!(
            parse_job_output(JobKind::Triage, &missing, &[]),
            Err(OutputError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn prose_arrays_and_the_wrong_kind_are_rejected() {
        let prose = format!("Here you go: {}", triage_json(""));
        assert!(parse_job_output(JobKind::Triage, &prose, &[]).is_err());
        assert_eq!(
            parse_job_output(JobKind::Triage, "[1]", &[]),
            Err(OutputError::NotAnObject)
        );
        let as_draft = triage_json("").replace("\"triage\"", "\"draft_help\"");
        assert!(matches!(
            parse_job_output(JobKind::Triage, &as_draft, &[]),
            Err(OutputError::WrongKind { .. })
        ));
    }

    #[test]
    fn triage_values_are_validated() {
        let bad_priority = triage_json("").replace("\"P1\"", "\"P9\"");
        assert!(parse_job_output(JobKind::Triage, &bad_priority, &[]).is_err());
        let bad_rec = triage_json("").replace("\"proposal\"", "\"deliver\"");
        assert!(parse_job_output(JobKind::Triage, &bad_rec, &[]).is_err());
        // A proposal or advice needs a body; `none` needs none.
        let no_body = triage_json("").replace(r#","body":"do it""#, "");
        assert!(parse_job_output(JobKind::Triage, &no_body, &[]).is_err());
        let advice_blank = triage_json("")
            .replace("\"proposal\"", "\"advice\"")
            .replace("do it", "  ");
        assert!(parse_job_output(JobKind::Triage, &advice_blank, &[]).is_err());
        let none = no_body.replace("\"proposal\"", "\"none\"");
        assert!(parse_job_output(JobKind::Triage, &none, &[]).is_ok());
        let huge = triage_json("").replace("do it", &"x".repeat(MAX_BODY_BYTES + 1));
        assert!(parse_job_output(JobKind::Triage, &huge, &[]).is_err());
    }

    #[test]
    fn draft_help_parses_and_needs_a_body() {
        let ok =
            r#"{"schema_version":1,"job_kind":"draft_help","proposed_body":"new","summary":"s"}"#;
        assert_eq!(
            parse_job_output(JobKind::DraftHelp, ok, &[]).unwrap(),
            JobOutput::DraftHelp(DraftHelpOutput {
                proposed_body: "new".into(),
                summary: "s".into(),
            })
        );
        let empty = ok.replace("\"new\"", "\"  \"");
        assert!(parse_job_output(JobKind::DraftHelp, &empty, &[]).is_err());
    }

    #[test]
    fn convert_must_cover_exactly_the_requested_targets() {
        let targets = vec!["acme-demo".to_string(), "acme-api".to_string()];
        let both = r#"{"schema_version":1,"job_kind":"convert","targets":[
            {"project":"acme-demo","system_instruction":"a"},
            {"project":"acme-api","system_instruction":"b"}]}"#;
        let JobOutput::Convert(out) = parse_job_output(JobKind::Convert, both, &targets).unwrap()
        else {
            panic!("convert");
        };
        assert_eq!(out.targets.len(), 2);

        let missing = r#"{"schema_version":1,"job_kind":"convert","targets":[
            {"project":"acme-demo","system_instruction":"a"}]}"#;
        assert!(parse_job_output(JobKind::Convert, missing, &targets).is_err());
        let stranger = both.replace("acme-api", "other");
        assert!(parse_job_output(JobKind::Convert, &stranger, &targets).is_err());
        let blank = both.replace("\"b\"", "\"\"");
        assert!(parse_job_output(JobKind::Convert, &blank, &targets).is_err());
        let nested_unknown = both.replace("\"a\"}", "\"a\",\"run\":true}");
        assert!(parse_job_output(JobKind::Convert, &nested_unknown, &targets).is_err());
    }

    #[test]
    fn errors_never_echo_the_reply() {
        let secret = "sk-TEST-0000000000000000";
        let reply = format!(r#"{{"schema_version":1,"job_kind":"triage","{secret}":1}}"#);
        let err = parse_job_output(JobKind::Triage, &reply, &[]).unwrap_err();
        // Field names are agent-chosen text: an unknown one is named, but
        // never a value.
        let not_json = parse_job_output(JobKind::Triage, &format!("{secret} {{"), &[]).unwrap_err();
        assert!(!not_json.to_string().contains(secret), "{not_json}");
        let _ = err;
    }
}
