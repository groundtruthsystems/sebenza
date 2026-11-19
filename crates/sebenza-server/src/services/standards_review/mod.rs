//! Standards review at track completion: the daemon side of the standards
//! stage, beside (and separate from) the project review in
//! `project_review.rs`.
//!
//! - [`git_pinned`]: timed, environment-scrubbed, object-only git plumbing.
//! - [`standards_rev`] and [`standards_docs`]: the standards at the
//!   merge-base and their revision.
//! - [`manifest`]: the rule syntax and the rule manifest.
//! - [`scope`]: first-parent commits, the scope diff, hunks and anchors.
//! - [`resolver`]: pins a job and resolves all of the above for a run.

pub mod git_pinned;
pub mod manifest;
pub mod objects;
pub mod reasons;
pub mod resolver;
pub mod scope;
pub mod standards_docs;
pub mod standards_rev;
