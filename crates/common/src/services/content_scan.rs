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
    todo!("phase-4-task-5: {}", text.len())
}

/// The likely-PHI hit names in `text`.
pub fn likely_phi(text: &str) -> Vec<&'static str> {
    todo!("phase-4-task-5: {}", text.len())
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
