//! The only free text a review run submits, finding descriptions, and what
//! it must pass before it is stored (FR-13, NFR-02).
//!
//! - At most [`MAX_DESCRIPTION_CHARS`] characters.
//! - No code excerpts. The heuristic is deliberately simple: a description is
//!   refused when it holds a fence (```` ``` ```` or `~~~`) or more than
//!   [`MAX_CODE_LIKE_RUN`] consecutive code-like lines. A line is code-like
//!   when, trimmed, it ends with `;`, `{` or `}`, or starts with a common
//!   declaration or statement keyword ([`CODE_LINE_STARTS`]). One quoted
//!   identifier or a single line of code is allowed; a pasted block is not.
//! - Through `content_scan`: secrets are redacted in place; a likely-PHI hit
//!   or a scanner error rejects the whole submission (fail closed).

use common::services::content_scan::likely_phi;
use common::services::inbox_convert::redact_secrets;

/// The longest description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// The most consecutive code-like lines a description may hold.
pub const MAX_CODE_LIKE_RUN: usize = 2;
/// Trimmed line starts that mark a line as code-like.
pub const CODE_LINE_STARTS: [&str; 16] = [
    "fn ",
    "pub ",
    "let ",
    "const ",
    "var ",
    "def ",
    "class ",
    "import ",
    "return ",
    "function ",
    "#include",
    "if (",
    "for (",
    "while (",
    "struct ",
    "impl ",
];

/// Why a description is refused before scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextProblem {
    Empty,
    TooLong,
    CodeExcerpt,
}

impl TextProblem {
    /// The kebab-case rejection code.
    pub fn code(self) -> &'static str {
        match self {
            TextProblem::Empty => "empty-description",
            TextProblem::TooLong => "description-too-long",
            TextProblem::CodeExcerpt => "code-excerpt",
        }
    }
}

/// Check `text`'s bounds and refuse code excerpts.
pub fn check_description(text: &str) -> Result<(), TextProblem> {
    todo!()
}

/// Whether `text` holds a fence or a run of code-like lines.
pub fn has_code_excerpt(text: &str) -> bool {
    todo!()
}

/// What a scan made of one text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    /// The text with every secret match redacted.
    pub text: String,
    /// Secret labels that fired, once each.
    pub redactions: Vec<&'static str>,
    /// Likely-PHI hit names; any one rejects the submission.
    pub phi: Vec<&'static str>,
}

/// The scanner failed; the submission is rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("content scan failed: {0}")]
pub struct ScanError(pub String);

/// Scans free text for secrets and PHI. A seam so a failing scanner can be
/// tested.
pub trait TextScanner: Send + Sync {
    /// Scan `text`.
    fn scan(&self, text: &str) -> Result<Scanned, ScanError>;
}

/// The production scanner: `content_scan`'s PHI heuristic and the shared
/// secret redaction, plus the exact strings in `known` (live tokens).
#[derive(Debug, Clone, Default)]
pub struct ContentScanner {
    pub known: Vec<String>,
}

impl TextScanner for ContentScanner {
    fn scan(&self, text: &str) -> Result<Scanned, ScanError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_bounded_to_500_characters() {
        assert_eq!(check_description(&"é".repeat(500)), Ok(()));
        assert_eq!(
            check_description(&"a".repeat(501)),
            Err(TextProblem::TooLong)
        );
        assert_eq!(check_description("  "), Err(TextProblem::Empty));
    }

    #[test]
    fn fenced_code_is_an_excerpt() {
        assert!(has_code_excerpt("See:\n```rust\nfn a() {}\n```"));
        assert!(has_code_excerpt("~~~\nx\n~~~"));
        assert_eq!(check_description("```x```"), Err(TextProblem::CodeExcerpt));
    }

    #[test]
    fn three_consecutive_code_like_lines_are_an_excerpt_but_two_are_not() {
        let two = "Body too long:\nlet a = 1;\nlet b = 2;\nSplit it.";
        assert!(!has_code_excerpt(two));
        let three = "Body too long:\nfn parse() {\n    let a = 1;\n}\n";
        assert!(has_code_excerpt(three));
        assert!(!has_code_excerpt(
            "Function `parse` has 42 logical lines; the rule allows 20."
        ));
    }

    #[test]
    fn the_content_scanner_redacts_secrets_and_reports_phi() {
        let s = ContentScanner::default();
        let key = format!("AKIA{}", "ABCDEFGHIJKLMNOP");
        let got = s.scan(&format!("Hard-coded key {key} in config.")).unwrap();
        assert!(!got.text.contains(&key), "{}", got.text);
        assert!(got.text.contains("[redacted"));
        assert!(!got.redactions.is_empty());
        assert!(got.phi.is_empty());
        let phi = s.scan("Fixture holds SSN 123-45-6789.").unwrap();
        assert_eq!(phi.phi, vec![common::services::content_scan::PHI_SSN]);
    }

    #[test]
    fn the_content_scanner_redacts_known_tokens() {
        let token = format!("rt_{}", "a".repeat(64));
        let s = ContentScanner {
            known: vec![token.clone()],
        };
        let got = s.scan(&format!("token {token} leaked")).unwrap();
        assert!(!got.text.contains(&token));
    }
}
