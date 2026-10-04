//! The warn-only content scan for comments, requests, proposals and
//! resolutions (FR-10, T-07, BR-08).
//!
//! It reuses the convert-time secret patterns and adds a deliberately small
//! likely-PHI heuristic. Hits are names, never the matched text, and they are
//! advisory: the scanned text is stored unchanged and the operator decides
//! whether to redact (DA-R4). Like the secret scan, it will miss things; the
//! design assumes a no-PHI environment and this only prompts a second look.

use crate::services::inbox_convert::secret_hit_names;

/// Hit name for a US Social Security number shape (`123-45-6789`).
pub const PHI_SSN: &str = "likely PHI: SSN";
/// Hit name for a medical record number (`MRN 1234567`).
pub const PHI_MRN: &str = "likely PHI: medical record number";
/// Hit name for a date of birth (`DOB 1980-04-02`, `date of birth: 4/2/1980`).
pub const PHI_DOB: &str = "likely PHI: date of birth";

/// Every scan hit in `text`: secret pattern labels, then likely-PHI names.
/// One entry per distinct kind, not per occurrence.
pub fn scan_hits(text: &str) -> Vec<String> {
    secret_hit_names(text)
        .into_iter()
        .chain(likely_phi(text))
        .map(str::to_string)
        .collect()
}

/// The likely-PHI hit names in `text`.
pub fn likely_phi(text: &str) -> Vec<&'static str> {
    // ASCII lowercasing keeps byte offsets, so indices stay valid in `text`.
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut out = Vec::new();
    if has_ssn(b) {
        out.push(PHI_SSN);
    }
    if keyword_then(b, &["mrn", "medical record number"], |rest| {
        digits_at(rest) >= 5
    }) {
        out.push(PHI_MRN);
    }
    if keyword_then(b, &["dob", "date of birth"], looks_like_date) {
        out.push(PHI_DOB);
    }
    out
}

/// A byte that cannot sit next to a matched token.
fn joins(c: Option<&u8>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
}

/// `ddd-dd-dddd` standing alone, not inside a longer hyphenated token.
fn has_ssn(b: &[u8]) -> bool {
    const SHAPE: &[u8] = b"ddd-dd-dddd";
    (0..b.len().saturating_sub(SHAPE.len() - 1)).any(|at| {
        let fits = SHAPE.iter().enumerate().all(|(k, want)| match want {
            b'd' => b[at + k].is_ascii_digit(),
            _ => b[at + k] == *want,
        });
        fits && !joins(at.checked_sub(1).and_then(|p| b.get(p))) && !joins(b.get(at + SHAPE.len()))
    })
}

/// A whole-word `keyword`, then up to a few `:`, `#` or spaces, then text
/// that satisfies `then`.
fn keyword_then(b: &[u8], keywords: &[&str], then: impl Fn(&[u8]) -> bool) -> bool {
    keywords.iter().any(|kw| {
        let kw = kw.as_bytes();
        (0..b.len()).any(|at| {
            if !b[at..].starts_with(kw)
                || at
                    .checked_sub(1)
                    .and_then(|p| b.get(p))
                    .is_some_and(u8::is_ascii_alphanumeric)
                || b.get(at + kw.len()).is_some_and(u8::is_ascii_alphanumeric)
            {
                return false;
            }
            let mut i = at + kw.len();
            let mut gap = 0;
            while i < b.len() && gap < 4 && matches!(b[i], b':' | b'#' | b' ' | b'=') {
                i += 1;
                gap += 1;
            }
            then(&b[i..])
        })
    })
}

fn digits_at(b: &[u8]) -> usize {
    b.iter().take_while(|c| c.is_ascii_digit()).count()
}

/// `1980-04-02`, `4/2/1980`, `02.04.80`: digit groups joined by one of
/// `-`, `/` or `.`, with a year-sized group at one end.
fn looks_like_date(b: &[u8]) -> bool {
    let mut groups = Vec::new();
    let mut i = 0;
    loop {
        let n = digits_at(&b[i..]);
        if n == 0 || n > 4 {
            return false;
        }
        groups.push(n);
        i += n;
        if groups.len() == 3 {
            break;
        }
        match b.get(i) {
            Some(b'-' | b'/' | b'.') => i += 1,
            _ => return false,
        }
    }
    groups[1] <= 2 && (groups[0] == 4 || groups[2] >= 2) && !joins(b.get(i))
}

#[cfg(test)]
mod tests {
    use super::*;

    // TS-38: fabricated secrets and synthetic PHI shapes are named, not quoted.
    #[test]
    fn a_fake_secret_is_named_by_its_pattern() {
        let hits = scan_hits("use sk-TEST-0000000000000000000000 for now");
        assert_eq!(hits, vec!["OpenAI-style key".to_string()]);
        assert!(hits.iter().all(|h| !h.contains("sk-TEST")));
    }

    #[test]
    fn synthetic_phi_shapes_are_flagged() {
        assert_eq!(likely_phi("patient ssn 123-45-6789"), vec![PHI_SSN]);
        assert_eq!(likely_phi("see MRN: 00123456 for history"), vec![PHI_MRN]);
        assert_eq!(likely_phi("DOB 1980-04-02"), vec![PHI_DOB]);
        assert_eq!(likely_phi("Date of birth: 4/2/1980"), vec![PHI_DOB]);
    }

    #[test]
    fn ordinary_engineering_text_is_clean() {
        for text in [
            "Rework the claims scorer; see PR 123-45 and ticket 6789.",
            "The build took 2026-09-30 12:00 to 12:45.",
            "mrna sequencing pipeline, version 1234567",
            "Call it at 555-1234.",
            "branch fix-123-45-6789x",
        ] {
            assert!(scan_hits(text).is_empty(), "false positive on {text:?}");
        }
    }

    #[test]
    fn hits_are_distinct_and_combined() {
        let hits = scan_hits(
            "AKIAIOSFODNN7EXAMPLE and AKIAIOSFODNN7EXAMPLF, ssn 123-45-6789 and 987-65-4321",
        );
        assert_eq!(
            hits,
            vec!["AWS access key".to_string(), PHI_SSN.to_string()]
        );
    }
}
