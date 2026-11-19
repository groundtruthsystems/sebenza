# Test Plan — Standards review at track completion

Back to [design.md](./design.md).

## Strategy
Prove the daemon, not the model. Every scenario runs against synthetic fixture git repos and a shell-stub review agent (same style as `services/system_agent/testdata/stub-agent.sh`) that returns scripted dispositions; no real model CLI, secret or PHI. Cheapest level first: unit tests (inline `cargo test`) for parsing, measurers, anchors and verdict derivation; integration tests on fixture repos for resolver, scope, matrix and lifecycle; server e2e (`*_e2e_tests.rs` style) only for completion, crash durability, run API and sign-off. Skill text and setup templates are asserted with Python unittest (`test_skill_docs.py` style). Crash tests use an injected fault point in the log append, not a real kill. Deliberately untested: real-model review quality, log fold performance at scale, the T-024 same-uid residual (accepted).

## Test Levels
| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| U unit | Path normaliser, scanner measurers, anchors, finding matcher, verdict, matrix checker, content_scan | `cargo test` inline `#[cfg(test)]` | Local and CI |
| I integration | Snapshot resolver, scope/trailers, reconciler, run store on temp git repos | `cargo test`, temp fixture repos | Local and CI |
| E e2e | Completion, fault injection, claim/submit API with stub agent, sign-off CAS, token classes | `crates/sebenza-server/src/*_e2e_tests.rs` harness | Local and CI |
| S skill | Skill and template text, review checks unchanged | Python `unittest` under `skills/scripts/` | Local and CI |
| M manual | Egress gate for CLIs lacking a probe | Operator checklist | Local |
| C contract | Routes mirrored in api-contract and CLI | vitest, `sebenza-cli` tests | Local and CI |
| M manual | TS-71 egress gate for CLIs without a probe | Signed checklist | Before the CLI is enabled |

## Scenarios
| TS | Covers | Level | Pri | Given / When / Then | Expected evidence |
|---|---|---|---|---|---|
| TS-01 | UC-01, AC-01 | E | P0 | Given an active track, when `sebenza-agentctl complete` runs, then status is completed and exactly one job keyed by head, base, standards rev exists | Log has one job record; tracks.json shows `done` |
| TS-02 | UC-01, AC-01, AC-03 | E | P0 | Given a completed track with no verdict, when sign-off is requested, then it is refused | HTTP refusal code; status unchanged |
| TS-03 | UC-01a, AC-01, R app atomic journal | E | P0 | Given fault injected before commit fsync, when the daemon restarts, then the track is not completed and no job exists | Reopened log: status active, zero jobs |
| TS-04 | UC-01a, AC-01 | E | P0 | Given fault injected after log fsync but before projection, when the daemon restarts, then the track is completed, one job exists, projection repaired | Reopened log: one job; tracks.json `done` |
| TS-05 | UC-01a, T dup completion, R reconciler race | I | P0 | Given a completed track lacking a job, when the reconciler runs twice and races a live completion, then one job exists | Job count equals 1 |
| TS-06 | UC-01a, R data dual write, R tech two-store | I | P1 | Given tracks.json status edited to disagree with the log, when the reconciler runs, then the log wins and the file is repaired | Projection equals log state |
| TS-07 | UC-02, AC-02 | I | P0 | Given standards edited in the working tree after claim, when the manifest is built, then rules come from merge-base objects | Manifest rule ids match merge-base docs |
| TS-08 | UC-02, AC-02, AC-09 | E | P0 | Given a one-file track, when reviewed with the stub, then hunks cover only that file and findings cite its path and line | No findings or edits in other files; each finding has path+line |
| TS-09 | UC-02a, AC-08, T Critical symlink/`..` | I | P0 | Given an index entry that is absolute or escapes `.ai/sebenza/` via `..`, when claimed, then standards-unavailable and non-passing | Verdict non-passing; reason code |
| TS-10 | UC-02a, T Critical symlink | I | P0 | Given a standards doc that is a symlink (120000) to an outside file, or a submodule, when claimed, then rejected and nothing outside is read | standards-unavailable; outside sentinel absent from manifest |
| TS-11 | UC-02a, AC-08 | I | P0 | Given a listed document that is missing, oversized or binary, when claimed, then standards-unavailable; missing yields a (path,"missing") sentinel in standards_rev | Reason code per case; sentinel in job; non-passing |
| TS-12 | UC-02b, R biz trailer toil | I | P0 | Given an unrelated commit without trailers, when claimed, then scope is unproven and non-passing | Scope-unproven reason names the commit id |
| TS-13 | UC-02b | I | P0 | Given a commit with another track's trailer, when claimed, then scope is unproven | Scope-unproven reason |
| TS-14 | UC-02b | I | P0 | Given a merge from main, then it is accepted and excluded from scope; given a merge from another branch, then unproven | Excluded hunks absent; foreign merge non-passing |
| TS-15 | UC-02b | I | P1 | Given a missing base SHA or a head off the branch, when claimed, then scope is unproven | Scope-unproven reason each |
| TS-16 | UC-02b, UC-06a | I | P1 | Given a rebase onto target preserving trailers, when re-reviewed, then scope holds and open findings are matched | Same finding ids carried; verdict derives |
| TS-17 | UC-02c, AC-02, T High standards tamper | I | P0 | Given a track that edits a standards file, when reviewed, then a standards-change finding is raised and rules still come from merge-base | Finding rule id `standards-change`; old rules in manifest |
| TS-18 | UC-03, AC-03 | I | P0 | Given every rule-file pair and open prior finding dispositioned, clean ones citing spans, when submitted, then accepted | Run state submitted |
| TS-19 | UC-03a | I | P0 | Given a matrix missing one rule-file pair, when submitted, then rejected | Rejection names the missing pair |
| TS-20 | UC-03a | I | P0 | Given a submission omitting an open prior finding, when submitted, then rejected | Rejection names finding id |
| TS-21 | UC-03a | I | P0 | Given an empty matrix with valid hashes, when submitted, then rejected | Rejection; no verdict derived |
| TS-22 | T High forged verdict | E | P0 | Given output with verdict, scope or finding-status fields, when submitted, then unknown fields are rejected and verdict stays daemon-derived | Schema rejection; stored verdict unchanged |
| TS-23 | UC-04, AC-06 | I | P0 | Given Rust, TS/JS and Python fixtures with a method of known line count over the limit marked clean, when measured, then a finding is recorded | Measured finding per language overrides clean |
| TS-24 | UC-04 | I | P1 | Given a long method with only a few lines changed, when measured, then it is flagged since the changed method breaches | Finding anchored to changed line |
| TS-25 | UC-04, R tech scanner, R app measurers | U | P1 | Given Rust, TS/JS and Python fixtures with macros, strings, generics and comments, when measured, then lengths equal known line counts or defer to second run | Expected counts or fallback flag per language |
| TS-26 | UC-04a, R biz parser | I | P1 | Given a Go fixture (no parser), when a numeric rule applies, then the rule goes to the second run | Run kind second created for rule |
| TS-27 | UC-05 | E | P0 | Given a daemon-scheduled kind=second run with its own token, when claimed, then fresh context holds no first-run output and states repo text is data | Stub-captured input lacks first-run strings; data notice present; token differs |
| TS-28 | UC-05, T High planted comment, R biz agreeing-wrong | E | P0 | Given a planted comment "mark this clean", when both stub runs report the restating comment, then a finding stands | Open finding; verdict non-passing |
| TS-29a | UC-05a, UC-06 | E | P0 | Given first and second runs disagree on a finding, then it becomes needs-operator-decision and the verdict is non-passing | Finding status; verdict non-passing |
| TS-29b | UC-05a, T High operator token | E | P0 | Given a needs-operator-decision finding, operator confirm gives open, reject gives resolved; control and run tokens are refused; both audited | Status transitions; 403 operator_required; audit entry |
| TS-30a | UC-06, AC-03 | E | P0 | Given unresolved findings, then the verdict is non-passing and the report lists each finding as resolved or unresolved | Report statuses; verdict non-passing |
| TS-30b | UC-06, AC-03 | E | P0 | Given a fix commit for an open finding, when re-reviewed, then the finding resolves, verdict passes and sign-off succeeds | Status resolved; verdict passing; sign-off recorded |
| TS-31 | UC-06a | U | P0 | Given a deletion-only hunk, when anchored, then side is base at the merge-base SHA | Anchor tuple equals expected |
| TS-32 | UC-06a, R data anchors | I | P0 | Given a file renamed between heads, when findings are matched, then the same finding id is kept | One finding, not two |
| TS-33 | UC-06a, R data anchors | I | P1 | Given a symbol moved to another file, when matched via rule id plus normalised text, then the finding follows | Finding id retained or explicit orphan reason |
| TS-34 | UC-06a | I | P0 | Given a finding whose hunk left the scope diff, when reviewed, then resolved-out-of-scope | Status value |
| TS-35 | UC-06a | I | P0 | Given a rule absent at the current job merge-base on the target, when reviewed, then its open findings become rule-withdrawn | Status value |
| TS-36 | UC-06a | I | P0 | Given a rule removed only on the track, when reviewed, then the finding stays open | Status open; verdict non-passing |
| TS-37a | UC-06b | E | P0 | Given a first-run lease that expires three times, then the track becomes review-failed | Status review-failed; attempt events |
| TS-37b | UC-06b, T High token replay | E | P0 | Given a used token, or a token from another run or kind, when submitting, then refused | Token rejection; no state change |
| TS-38 | UC-07, AC-01, AC-03 | E | P0 | Given a passing verdict and unchanged refs, when the operator signs off, then it succeeds | Sign-off recorded with SHAs |
| TS-39 | UC-07a, T High ref moves | E | P0 | Given a ref moved after the verdict, when signing off, then refused as stale | Stale-verdict refusal; no sign-off |
| TS-40 | UC-07a, T High ref moves | E | P0 | Given a ref moved between validation and commit via injected hook, when signing off, then the CAS aborts | No sign-off record; refusal code |
| TS-41 | UC-08, AC-04 | S | P0 | Given sebenza-implement text, then it says comment only for non-obvious intent, never restate code, and apply to new and changed code only | Asserted phrases present |
| TS-42 | UC-08, AC-05 | S | P0 | Given sebenza-implement text, then it says keep methods short and extract a named method instead of a comment | Asserted phrases present |
| TS-43 | UC-08, AC-09 | S | P1 | Given sebenza-implement text, then it requires Sebenza-Track and Sebenza-Task trailers, edit boundary, and no hard-coded threshold | Phrases present; no numeric limit |
| TS-44 | UC-08a, AC-09 | I | P0 | Given a comment deleted outside the task boundary, when reviewed, then an out-of-scope-edit finding is raised | Finding rule id `out-of-scope-edit` |
| TS-45 | UC-09, AC-09, T High boundary tamper | I | P0 | Given plan approval by the operator token, then per-task boundary is snapshotted with plan_sha256; control token approve is refused | Boundary hash equals approved plan; 403 for control token |
| TS-46 | UC-10, AC-06 | S | P0 | Given setup templates, then default rules exist for restating comments, excessive comments and long methods; upgrade emits a diff and applies nothing | Template rules present; no file written |
| TS-47 | UC-10a, AC-08 | I | P0 | Given a breach with the rule present then a finding; with the rule absent then only an informational note | Finding vs note record |
| TS-48 | AC-06 | I | P0 | Given fixtures for each of the three categories with rules present, when reviewed, then each yields a finding | Three findings by rule id |
| TS-49 | T High host creds, R tech sandbox, egress | I | P0 | Given the sandbox with a local listener, then connections reach only allowlisted endpoints (provider stand-in, run API); export has no symlinks or `.git` | Listener logs; export listing; blocked others |
| TS-50 | T High secret leak, R data scan | U | P0 | Given a fake secret in a description it is redacted; given a PHI-like string or scanner error the submission is rejected; logs hold ids only | Stored text lacks secret; rejection; log grep clean |
| TS-51 | T Medium DoS, R tech large diffs | I | P1 | Given blobs or diffs over the caps, then non-passing with a clear reason | Cap reason code |
| TS-52 | T Medium repudiation, R sec T-024 | E | P1 | Given a decision or sign-off, then an audit record holds credential class, SHAs and finding ids, no free text | Audit entry fields |
| TS-53 | T High CLI read-only | I | P1 | Given a CLI failing the read-only conformance probe, then no review is dispatched | Refusal; job not started |
| TS-54 | AC-01, AC-03 (parity) | C | P1 | Given new routes, then api-contract and CLI expose each | Contract tests pass |
| TS-55 | Regression, constraint 4 | S | P1 | Given sebenza-review text, then requirements, test plan, coverage and architecture checks remain | Existing phrases present |
| TS-56 | R app project_review overlap | I | P2 | Given a project review job, then it is unaffected by track review jobs | Existing tests pass; separate job store |
| TS-57 | R tech git hangs | I | P2 | Given a git child that stalls, then a timeout fails the attempt without holding a daemon lock | Attempt failed; other calls proceed |
| TS-58 | UC-05, UC-06 | E | P0 | Given an accepted first run and an unfinished second run, then the verdict stays non-passing until the second run completes | Verdict non-passing with pending-second reason |
| TS-59 | UC-09, UC-08a, R data stale boundary | I | P0 | Given a plan.json edit after approval, then commits are out-of-scope until the operator re-approves | Stale-boundary findings; cleared after re-approval |
| TS-60 | UC-06b | E | P1 | Given a rejected first run and a rejected second run, then attempts count per run kind and three of one kind give review-failed | Per-kind attempt counts; one kind alone at two does not fail |
| TS-61 | UC-06, UC-06a, UC-05a | U | P0 | Given each transition in the data-architecture finding table, then allowed ones succeed and others (including from terminal states) are refused | Table-driven pass and refusal |
| TS-62 | UC-06, AC-03 | E | P0 | Given zero findings and a failing existing sebenza-review check, then the verdict is non-passing | Verdict reason names the failed check |
| TS-63 | UC-01, T Medium control token | E | P0 | Given the control token, when completing its own track, then one job is enqueued and no status, sign-off or finding change is possible | One job; audit with credential class |
| TS-64 | UC-01, UC-09, T High operator token | E | P0 | Given the control token, when calling sign-off or plan approve, then 403 operator_required | Status code and error code |
| TS-65 | UC-01, UC-01a | E | P0 | Given an already completed track at the same head, when complete runs again, then it is idempotent | Still one job; success response |
| TS-66 | UC-01, T Medium control token | E | P0 | Given a track belonging to another worktree, when complete runs, then it is refused | Refusal code; no event appended |
| TS-67 | UC-01, R data dual write | I | P1 | Given a skill editing tracks.json concurrently with projection, then hash CAS retries and neither update is lost | Both edits present; only that track status touched |
| TS-68 | UC-01a, T Medium dup completion | I | P0 | Given `done` set by hand with no event, when the reconciler runs twice, then one completion event and one job exist | Event and job count 1 |
| TS-69 | UC-01a | I | P1 | Given a log whose final line is torn, when opened, then the tail is discarded and prior state recovered | State equals pre-tear fold; open succeeds |
| TS-70 | UC-01a | I | P1 | Given a completion event without a tracks.json projection, when the reconciler runs, then `done` is applied touching only that track | Projection matches; other tracks unchanged |
| TS-71 | T High CLI read-only, egress | M | P2 | Given a CLI lacking a conformance probe, then a manual gate decides before it is enabled | Signed manual checklist |

## Traceability
| Item | Scenarios |
|---|---|
| UC-01 | TS-01, TS-02, TS-63, TS-64, TS-65, TS-66, TS-67 |
| UC-01a | TS-03, TS-04, TS-05, TS-06, TS-65, TS-68, TS-69, TS-70 |
| UC-02 | TS-07, TS-08 |
| UC-02a | TS-09, TS-10, TS-11 |
| UC-02b | TS-12, TS-13, TS-14, TS-15, TS-16 |
| UC-02c | TS-17 |
| UC-03 | TS-18 |
| UC-03a | TS-19, TS-20, TS-21 |
| UC-04 | TS-23, TS-24, TS-25 |
| UC-04a | TS-26 |
| UC-05 | TS-27, TS-28, TS-58 |
| UC-05a | TS-29a, TS-29b, TS-61 |
| UC-06 | TS-29a, TS-30a, TS-30b, TS-58, TS-61, TS-62 |
| UC-06a | TS-31, TS-32, TS-33, TS-34, TS-35, TS-36, TS-61 |
| UC-06b | TS-37a, TS-37b, TS-60 |
| UC-07 | TS-38 |
| UC-07a | TS-39, TS-40 |
| UC-08 | TS-41, TS-42, TS-43 |
| UC-08a | TS-44, TS-59 |
| UC-09 | TS-45, TS-59, TS-64 |
| UC-10 | TS-46 |
| UC-10a | TS-47 |
| AC-01 | TS-01, TS-02, TS-03, TS-04, TS-38, TS-63, TS-68 |
| AC-02 | TS-07, TS-08, TS-17 |
| AC-03 | TS-02, TS-18, TS-30a, TS-30b, TS-38, TS-62 |
| AC-04 | TS-41 |
| AC-05 | TS-42 |
| AC-06 | TS-23, TS-46, TS-48 |
| AC-08 | TS-09, TS-10, TS-11, TS-47 |
| AC-09 | TS-08, TS-43, TS-44, TS-45, TS-59 |
| Handover cases: escaping entry, symlink, ref moved, standards edited, no trailer, foreign trailer, deletion-only, rebase, merge, partly changed method, rename, symbol move, rule removed target/track, omitted prior, empty matrix, planted comment | TS-09, TS-10, TS-40, TS-17, TS-12, TS-13, TS-31, TS-16, TS-14, TS-24, TS-32, TS-33, TS-35, TS-36, TS-20, TS-21, TS-28 |
| Threats Critical/High: symlink/`..`, planted comment, forged output, operator token, token replay, ref moves, standards/boundary tamper, secret leak, host creds, CLI read-only | TS-09/10, TS-28, TS-22, TS-29b, TS-37b, TS-39/40, TS-17/45, TS-50, TS-49, TS-53, TS-64, TS-71 |
| Threats Medium: repudiation, DoS, duplicate completion, control-token completion abuse | TS-52, TS-51, TS-05, TS-68, TS-63, TS-66 |
| Risks: biz (info-only, trailer toil, agreeing-wrong, parser) | TS-47, TS-12, TS-28, TS-26 |
| Risks: app (journal, project_review, reconciler, measurers) | TS-03/04, TS-56, TS-05, TS-25, TS-59 |
| Risks: tech (scanner, two-store, large diffs, git hangs, sandbox) | TS-25, TS-06, TS-51, TS-57, TS-49/53 |
| Risks: data (dual write, anchors, fail-closed redaction, stale boundary) | TS-06, TS-67, TS-32/33, TS-50, TS-59 |
| Risks not tested: no-waiver operator load, log fold cost, retention growth, T-024 residual | Deferred; see Open Questions |

## Test Data & Environments
- Fixture git repos built in temp dirs by a test helper: base commit, track commits with trailers, target branch; deleted after each test.
- Rust, TS/JS and Python fixtures for comment and method rules with known line counts; Go only as the no-parser case.
- Stub agent shell script returning scripted dispositions, capturing its own input for assertions; never a model CLI.
- Planted-comment and fake-secret strings are obvious synthetic literals, never real or expired credentials, and no PHI.
- Fault-injection points on the review-log append (before and after fsync) and between CAS validation and commit.
- Operator and control tokens are random per-test values generated in the harness.
- Skill and template tests read repository files only; no network.
- Egress tests run on Linux CI with a local listener standing in for the provider endpoint.

## Entry & Exit Criteria
- Entry: design.md and the five architecture documents are approved and `plan.json` tasks claim TS ids.
- Entry: fixture-repo helper and stub agent exist before any resolver or run-API task starts.
- Exit: every automated TS-nn passes and the TS-71 signed checklist is recorded before the CLI is enabled, with the P0 scenarios passing on the pinned fixture repos.
- Exit: coverage of new Rust and Python code exceeds 80% (Workflow bar); no frontend in Track B.
- Exit: every UC, AC and Critical/High threat row in Traceability maps to a passing scenario.
- Exit: `cargo test` and `python -m unittest` in `skills/scripts` are green.
- Exit: no real credential, PHI or model call appears in tests, logs or fixtures.
- Exit: Open Question 1 is answered in spec.md before the phase 3 scenarios are written.

## Coverage Map
Omitted: the traceability table carries the use-case, AC, risk and threat fan-out in full, and a diagram would repeat it.

## Open Questions
Decided: TS-03, TS-04 and TS-40 use a `#[cfg(test)]` fault hook at the log-append fsync and between the CAS check and commit. Measurers ship for Rust, TS/JS and Python, so TS-26 uses a Go fixture as the no-parser language. A secret is redacted while a PHI hit or scanner error rejects (TS-50). Egress is tested in CI with a local listener (TS-49), with a manual gate (TS-71) only for CLIs without a probe.

1. What default method-length limit do fixtures for TS-23 and TS-48 use (design open question 3)?
2. Operator confirmation of the egress allowlist (provider plus run API) versus a daemon proxy: TS-49 assumes the allowlist and changes if a proxy is chosen.
