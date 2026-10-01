//! The system agent's structured output: one JSON object per job, versioned
//! by `schema_version` and keyed by `job_kind`, with unknown fields rejected
//! (AA-D2). Anything that does not parse into the expected kind is a failed
//! job; the server, never the agent, applies what a valid result says.

use crate::domain::model::Priority;
use serde::{Deserialize, Serialize};

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
    todo!(
        "phase-3-task-5: {kind:?} {} {expected_targets:?}",
        message.len()
    )
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
