//! Standards review at track completion: the daemon side of the standards
//! stage, beside (and separate from) the project review in
//! `project_review.rs`.
//!
//! - [`git_pinned`]: timed, environment-scrubbed, object-only git plumbing.
//! - [`standards_rev`] and [`standards_docs`]: the standards at the
//!   merge-base and their revision.
//! - [`manifest`]: the rule syntax and the rule manifest.
//! - [`scope`]: first-parent commits, the scope diff, hunks and anchors.
//! - [`plan_approval`] and [`boundary`]: the operator's plan approval, its
//!   per-task edit-boundary snapshot and the boundary globs.
//! - [`scope_proof`]: trailers, merges and pins that prove a job's scope,
//!   and the in-scope files changed outside their task's boundary.
//! - [`resolver`]: pins a job and resolves all of the above for a run.
//! - [`jobs`]: jobs, runs and findings as a fold over the review log.
//! - [`run_token`], [`run_store`] and [`workers`]: leases, single-use run
//!   tokens, claims, submissions, expiry and the worker pool.
//! - [`run_context`]: what a run reads and the matrix it must cover.
//! - [`submission`] and [`free_text`]: the closed submission schema, its
//!   matrix check, and the bounds and scan on finding descriptions.
//! - [`findings`], [`matcher`] and [`deriver`]: the finding transition
//!   table, the finding matcher and the daemon-only verdict.
//! - [`views`]: the JSON the run API and review routes return.

pub mod boundary;
pub mod deriver;
pub mod findings;
pub mod free_text;
pub mod git_pinned;
pub mod jobs;
pub mod manifest;
pub mod matcher;
pub mod objects;
pub mod plan_approval;
pub mod reasons;
pub mod resolver;
pub mod run_context;
pub mod run_store;
pub mod run_token;
pub mod scope;
pub mod scope_proof;
pub mod standards_docs;
pub mod standards_rev;
pub mod submission;
pub mod views;
pub mod workers;
