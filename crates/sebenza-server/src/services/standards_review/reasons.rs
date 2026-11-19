//! Why a review job's inputs cannot yield a passing verdict. Every blocker
//! carries a top-level code (`standards-unavailable` or `cap-exceeded`) and
//! a specific reason, both stable kebab-case strings.

use serde::Serialize;

/// The job-level outcome a blocker forces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockCode {
    /// The standards at the merge-base cannot be read safely (FR-07).
    StandardsUnavailable,
    /// The review inputs exceed a configured size cap.
    CapExceeded,
}

/// The specific cause of a blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    AbsolutePath,
    NonNormalisedPath,
    PathTraversal,
    OutsideWorkspace,
    Symlink,
    Submodule,
    NotUtf8,
    Binary,
    /// A standards blob over the blob cap.
    Oversized,
    Missing,
    MalformedRule,
    DuplicateRule,
    /// An in-scope blob at head over the blob cap.
    BlobTooLarge,
    DiffTooLarge,
    TooManyHunks,
}

impl Reason {
    /// The job-level code this reason belongs to.
    pub fn code(self) -> BlockCode {
        match self {
            Reason::BlobTooLarge | Reason::DiffTooLarge | Reason::TooManyHunks => {
                BlockCode::CapExceeded
            }
            _ => BlockCode::StandardsUnavailable,
        }
    }
}

/// One reason the job is non-passing, with the path (and line) it concerns.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Blocker {
    pub code: BlockCode,
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

impl Blocker {
    /// A blocker about `path`.
    pub fn at(reason: Reason, path: &str) -> Blocker {
        Blocker {
            code: reason.code(),
            reason,
            path: Some(path.to_string()),
            line: None,
        }
    }

    /// A blocker about one line of `path`.
    pub fn at_line(reason: Reason, path: &str, line: usize) -> Blocker {
        Blocker {
            line: Some(line),
            ..Blocker::at(reason, path)
        }
    }

    /// A blocker about the job as a whole.
    pub fn whole(reason: Reason) -> Blocker {
        Blocker {
            code: reason.code(),
            reason,
            path: None,
            line: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_and_standards_reasons_map_to_their_codes() {
        assert_eq!(Reason::Symlink.code(), BlockCode::StandardsUnavailable);
        assert_eq!(Reason::Oversized.code(), BlockCode::StandardsUnavailable);
        assert_eq!(Reason::DiffTooLarge.code(), BlockCode::CapExceeded);
        assert_eq!(Reason::BlobTooLarge.code(), BlockCode::CapExceeded);
    }

    #[test]
    fn blockers_serialise_as_kebab_case_codes() {
        let v = serde_json::to_value(Blocker::at_line(Reason::NotUtf8, "a.md", 3)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "code": "standards-unavailable", "reason": "not-utf8", "path": "a.md", "line": 3
            })
        );
        let v = serde_json::to_value(Blocker::whole(Reason::TooManyHunks)).unwrap();
        assert_eq!(v["code"], "cap-exceeded");
        assert_eq!(v["reason"], "too-many-hunks");
    }
}
