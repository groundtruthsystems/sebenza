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
//! - [`launcher`]: the sandboxed `claude` launcher: read-only head export,
//!   egress-allowlisting proxy and the conformance gate (D-04).
//! - [`run_context`]: what a run reads and the matrix it must cover.
//! - [`submission`] and [`free_text`]: the closed submission schema, its
//!   matrix check, and the bounds and scan on finding descriptions.
//! - [`existing_checks`]: the four existing sebenza-review checks the
//!   skill reports through their own route (FR-18).
//! - [`findings`], [`matcher`] and [`deriver`]: the finding transition
//!   table, the finding matcher and the daemon-only verdict.
//! - [`measure`]: the in-tree method-length measurers (FR-15, D-03).
//! - [`second_run`]: the second run's plan, its reconciliation and the
//!   notes for absent categories (FR-16, FR-26).
//! - [`signoff`]: the operator's compare-and-swap sign-off gate (FR-22).
//! - [`views`]: the JSON the run API and review routes return.

pub mod boundary;
pub mod deriver;
pub mod existing_checks;
pub mod findings;
pub mod free_text;
pub mod git_pinned;
pub mod jobs;
pub mod launcher;
pub mod manifest;
pub mod matcher;
pub mod measure;
pub mod objects;
pub mod plan_approval;
pub mod reasons;
pub mod resolver;
pub mod run_context;
pub mod run_store;
pub mod run_token;
pub mod scope;
pub mod scope_proof;
pub mod second_run;
pub mod signoff;
pub mod standards_docs;
pub mod standards_rev;
pub mod submission;
pub mod views;
pub mod workers;
