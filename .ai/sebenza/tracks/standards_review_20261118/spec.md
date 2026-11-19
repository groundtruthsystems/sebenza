# Spec — Standards review at track completion

Track: `standards_review_20261118` · Type: feature · Design: [design.md](./design.md) (operator-approved 2026-11-19) · Test plan: [test-plan.md](./test-plan.md) · Handover: `.ai/sebenza/solution-handover.md` Track B.

## Overview
The daemon owns track completion: one fsynced review-log event records the completion and its review job, and `tracks.json` is a projection of it. A review job pins head, merge-base and target, reads the project's `.ai/sebenza/` standards from git objects at the merge-base, and checks only in-scope hunks. sebenza-review gains a standards stage: a complete disposition matrix, daemon-measured numeric rules, and an independent second run for subjective rules. Only the daemon derives the verdict; sign-off is a compare-and-swap. sebenza-implement gains writing guidance, commit trailers and an edit boundary.

## Operator decisions (2026-11-19, recorded per handover §6)
- D-01 AC-06 reading: a category (restating comments, excessive comments, long methods) is enforced only where the project's `.ai/sebenza/` documents define its rule; otherwise the review gives an informational note, not a finding. The sebenza-setup templates ship default rules for all three categories. For existing projects the note suggests adopting the template rules; the review never edits their documents.
- D-02 Standards source: only the guidelines and code style guides listed in `.ai/sebenza/index.md`, read at the merge-base. Nothing else is hard-coded or pulled in (no linters, external guides, agent preferences).
- D-03 Default method-length rule: 20 logical lines per function or method body, not counting blank lines, comment-only lines or the signature line; a project-editable template rule.
- D-04 Egress: the review agent has no network tools; process egress is allowlisted to the model-provider endpoint and the daemon run API; launch supports `claude` only.
- Phase 3 stays narrow: only what D-01..D-04 need.

## Functional Requirements
- FR-01: `POST /tracks/{id}/complete` (control token, own worktree only) appends one fsynced `track-completed` event with job key (track, head, base, standards_rev).
- FR-02: After the event, the daemon projects `status: "done"` into `tracks.json`, touching only that track's status and updated_at, under lock with hash CAS.
- FR-03: Completion is idempotent on job key; a repeat returns success and appends nothing.
- FR-04: A reconciler at startup and every 5 minutes appends missing completion events (`source=reconciled`), re-projects, and reaps expired leases, never duplicating jobs.
- FR-05: Opening the review log discards a torn final line and recovers the prior fold.
- FR-06: On claim, the resolver pins head, merge-base and target SHAs and reads git objects only, never the working tree.
- FR-07: Standards are `index.md` plus its listed documents at the merge-base; escaping, absolute or non-normalised paths, symlinks, submodules, non-text, oversized (>256 KiB) or missing entries give `standards-unavailable`.
- FR-08: The resolver builds the rule manifest (rule id, category, source document, line) and in-scope hunks; scope is first-parent commits base..head, excluding merges from the target.
- FR-09: Scope is unproven on a missing or foreign `Sebenza-Track`/`Sebenza-Task` trailer, a foreign merge, a missing base, or head off branch.
- FR-10: A track editing `.ai/sebenza/` gets a `standards-change` finding; rules still come from the merge-base.
- FR-11: Claim and submit require a single-use run token bound to one run id and kind, stored hashed, expiring with the lease.
- FR-12: A submission must disposition every (rule, in-scope file) pair and every open prior finding; `clean` cites a span; anything missing, empty or carrying unknown fields is rejected.
- FR-13: Finding text is bounded, has no code excerpts, has secrets redacted; a PHI hit or scanner error rejects the submission.
- FR-14: Each run kind has three attempts (lease expiry or rejection); the third failure sets `review-failed`.
- FR-15: The daemon measures numeric rules for Rust, TS/JS and Python; a measured breach overrides `clean`; other languages fall back to the second run.
- FR-16: Subjective rules get a daemon-scheduled `kind=second` run with its own token and fresh context; agreement decides, disagreement gives `needs-operator-decision`.
- FR-17: The deriver matches findings by rename map then rule id plus normalised-text hash, and sets `resolved`, `resolved-out-of-scope` and `rule-withdrawn` (target-side removal only).
- FR-18: Passing means a complete matrix, no open or undecided findings, no pending second run, and all existing sebenza-review checks green.
- FR-19: `POST /findings/{id}/decision` (operator token) moves `needs-operator-decision` to `open` or `resolved`, audited.
- FR-20: `POST /tracks/{id}/plan/approve` (operator token) snapshots each task's edit boundary with `plan_sha256`; a later plan edit makes it stale.
- FR-21: A change outside the commit's task boundary, including a comment deletion, gives `out-of-scope-edit`.
- FR-22: `POST /tracks/{id}/signoff` (operator token) succeeds only with a passing verdict and unchanged head and merge-base (CAS).
- FR-23: sebenza-implement text carries the §2.6 guidance, trailers and boundary, with no hard-coded threshold.
- FR-24: sebenza-review adds the standards stage and second run; its existing checks are unchanged; an unreachable daemon reports `not-run`.
- FR-25: sebenza-setup templates ship the three default rules (D-01, D-03); upgrade prints them as a diff and writes nothing; this repository adopts them.
- FR-26: A category whose rule is absent yields an informational note only.
- FR-27: Every new route is in the ts-rest contract and `sebenza-cli`.

## Non-Functional Requirements
- NFR-01: No new crates or npm packages.
- NFR-02: No tokens, secrets or code text in the log, findings or logs; ids and hashes only.
- NFR-03: Git children have timeouts; no daemon lock is held across a git call.
- NFR-04: The review agent sees only a read-only head export with symlinks and `.git` removed (D-04).

## Acceptance Criteria
- AC-01 (FR-01..FR-05, FR-14, FR-22; note "Acceptance Criteria" 1): TS-01..TS-06, TS-37a, TS-60, TS-63, TS-65..TS-70
- AC-02 (FR-06..FR-10; note item 2): TS-07, TS-08, TS-17, TS-31
- AC-03 (FR-12, FR-17..FR-19, FR-22; note item 3): TS-18..TS-22, TS-29a, TS-29b, TS-30a, TS-30b, TS-32..TS-36, TS-38..TS-40, TS-58, TS-61, TS-62
- AC-04 (FR-23; note item 4): TS-41
- AC-05 (FR-23; note item 5): TS-42
- AC-06 (FR-15, FR-16, FR-25; note item 6; D-01, D-03): TS-23..TS-28, TS-46, TS-48
- AC-08 (FR-07, FR-26; D-02): TS-09..TS-11, TS-47, TS-51
- AC-09 (FR-08, FR-09, FR-20, FR-21): TS-08, TS-12..TS-16, TS-43..TS-45, TS-59
- AC-10 (FR-11, FR-13, NFR-02..NFR-04; security): TS-37b, TS-49, TS-50, TS-52, TS-53, TS-57, TS-64, TS-71
- AC-11 (FR-24, FR-27; regression and parity): TS-54, TS-55, TS-56

## Out of Scope
- Waivers; full screen (Track A); a new skill; `codex` as a review CLI; recovery UI for `review-failed` beyond re-queue via CLI.

## Open Questions
- Retention periods (90 days after archive proposed); compaction ships, the period stays configurable.
- T-024 residual (same-uid agent can reach the operator token) is accepted for this release, as in SECURITY.md.
