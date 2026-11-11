# Test Plan — Sebenza Software Factory

Part of [design.md](./design.md). Risk ids `R-B1..B5`, `R-A1..A5`, `R-D1..D5`, `R-T1..T5` and `R-S1..S5` are the `## Risks` bullets, in order, of the business, application, data, technical and security documents.

## Strategy
Each behaviour is proved at the cheapest level that catches the defect, and the default path runs no real model:

| Level | What it proves |
|---|---|
| Rust unit | Folds, hashes, round caps, auth classes |
| Daemon e2e (`e2e_tests.rs`) | Pipeline ordering, gates, read-only and injection behaviour, using shell stub agents |
| Vitest | Tabs, picker, chat, Design-tab rendering |
| Python unittest + `inbox.py` tests | Skill structure and ordering, because models are not run |

Real claude, codex and grok CLIs appear only in opt-in conformance tests, which skip when the CLI is absent.

**Deliberately untested:** model output quality, cost, and event-log compaction. Same-uid agents outside a sandbox can forge the gates (FT-01 residual); tests prove deterrence and audit, not enforcement.

## Test Levels
| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| L1 Rust unit | Event fold, round cap, hash binding, auth class, scan, independence, draft v3→v4 | `cargo test`, inline `#[cfg(test)]` | CI, local |
| L2 Rust e2e | Routes, pipeline, ReviewGate, handover, convert, `agentctl review` with stub agents | `e2e_tests.rs`, `testdata/*.sh` stubs | CI, local |
| L3 Frontend | Tabs, picker, chat, Design tab, settings, XSS render | vitest + Testing Library, `vi.mock("./api")` | CI, local |
| L4 Skill / structural | Skill ordering, review-call docs, hooks stdlib/loopback, layout, prompt source | `python3 -m unittest discover -s skills/scripts` | CI, local |
| L5 Conformance (opt-in) | Real CLI read-only / write / network behaviour, output format | cargo `#[ignore]` / skip-if-absent | Local |
| L6 Manual / UAT | Marketplace install, CLI login, one real end-to-end pass | Checklist | Operator machine |

## Scenarios
P0 = must pass to close the track; P1 = should (waiver recorded); P2 = nice to have.

| TS | Covers | Level | Pri | Given / When / Then | Evidence |
|---|---|---|---|---|---|
| TS-01 | UC-01, AC-S1.1 | L3 | P0 | Given New solution clicked, when the item opens, then `kind: solution` and tabs are exactly Coordinator, Design, Graph | InboxView test; route unit |
| TS-02 | UC-01a, AC-S1.2 | L1+L3 | P0 | Given a v3 draft, when opened, then notes show in the left panel; graph and requests behave as before | draft migration unit; vitest |
| TS-03 | UC-02, UC-02a, AC-S2.1 | L3 | P0 | Given codex not installed, when the picker renders, then Claude, Codex, Grok are listed and Codex is disabled | picker vitest |
| TS-04 | UC-02, AC-S2.2 | L2+L3 | P0 | Given an operator message, when the stub coordinator streams, then the reply is persisted and shown after reload | e2e transcript; vitest |
| TS-05 | UC-02, AC-S2.3, FT-10 | L2 | P0 | Given proposals of the three note types, when applied, then each appears badged `coordinator`, capped, fenced as data | e2e; fencing unit |
| TS-06 | UC-02b, AC-S2.3, R-D2 | L1+L2 | P0 | Given a stale expected hash, when a note is written, then 409 and both versions survive | e2e; hash unit |
| TS-07 | R-A4 | L2 | P2 | Given an agent switch mid-chat, when the next turn runs, then the stub receives the transcript digest | e2e arg capture |
| TS-08 | UC-03, AC-S3.1 | L2 | P0 | Given only a coordinator "start" proposal, when no operator start is sent, then no analyst job is queued | e2e job queue |
| TS-09 | UC-03, AC-S3.2 | L2+L3 | P0 | Given operator start, when stubs run, then analyst precedes architect and both documents show in the Design tab | e2e order log; vitest |
| TS-10 | UC-03a, AC-S3.3 | L1+L2 | P0 | Given an unregistered project in the architecture, when validated, then a review problem is raised; registered ones pass | parser unit; e2e |
| TS-11 | FT-13, R-T5 | L1+L2 | P1 | Given a flood or hung job, when limits hit, then FIFO and `maxConcurrent` hold and the job ends `failed` / `timeout`, no round consumed | e2e sleeper stub |
| TS-12 | UC-04, AC-S4.1 | L2+L3 | P0 | Given reviewer returns changes, when round 1 ends, then UI shows "round 1 of 3" and a revision precedes round 2 | e2e; vitest |
| TS-13 | UC-04, AC-S4.1, FT-09 | L1+L2 | P0 | Given three rounds done, when a 4th is requested, then 409 `round_limit` and the fold is unchanged | route test |
| TS-14 | UC-04a, AC-S4.2 | L2 | P0 | Given round 3 with N problems, when escalated, then exactly N requests exist, one per `problem_id` | e2e request count |
| TS-15 | UC-04b, AC-S4.3 | L2 | P1 | Given N open problems, when the coordinator proposes a resolution and the operator confirms it, then only that request resolves | e2e |
| TS-16 | UC-04c, AC-S4.3 | L2+L3 | P1 | Given a hand edit then "resolve", when applied, then only that request closes and the edit hash is current | e2e; vitest |
| TS-17 | UC-05, AC-S5.1 | L1+L2+L3 | P1 | Given global and per-item reviewers at both levels, when a review runs, then the stub sees the override, else the global | config unit; e2e |
| TS-18 | UC-05a, AC-S5.2 | L1+L2 | P0 | Given reviewer equals designer (same agent, same or unset model), when saved or run, then 422 `not_independent` | independence unit; e2e |
| TS-19 | UC-05a, AC-S5.2, R-B2 | L1 | P0 | Given claude designer opus, when reviewer is claude sonnet or haiku or unset, then refused with reason; codex or claude fable accepted | independence unit |
| TS-20 | UC-06, AC-S6.1 | L2 | P0 | Given a passed review, when the coordinator reports, then the report is in transcript and comments, phase `agreed`, convert refused | e2e fold |
| TS-21 | UC-06, UC-06b, AC-S6.2 | L2 | P0 | Given two affected projects, when one is approved and one deferred, then one worktree with `solution-handover.md`, none for the other | e2e worktree dirs |
| TS-22 | UC-06a, AC-S6.3, FT-07 | L1+L2 | P0 | Given an approval, when a doc or instruction is edited, then the approval is void and convert returns 409 `approval_required` | hash unit; e2e |
| TS-23 | AC-S6.4, FT-01, FT-08 | L2 | P0 | Given the shared token, when calling each operator route, then 403 `operator_required`; double approve creates one worktree | route table test |
| TS-24 | FT-07, R-A3 | L1+L2 | P0 | Given an edit racing an approval, when serialised under the item lock, then handover bytes equal approved bytes | concurrency test |
| TS-25 | UC-07, AC-P1.1 | L4 | P0 | Given the architect skill, when parsed, then business precedes application ∥ data (with handover brief), then technical ∥ security, then test | skill doc unittest |
| TS-26 | UC-08, AC-P2.1 | L2 | P0 | Given a linked worktree, when `agentctl review --level design` runs, then the configured reviewer runs read-only and a round is recorded | e2e stub |
| TS-27 | UC-08a, AC-P2.1 | L2+L4 | P1 | Given an unlinked worktree, when review runs, then the round is flagged not independent | inbox.py unit |
| TS-28 | UC-09, AC-P3.1 | L2+L3 | P0 | Given a project-level request, when raised, then it shows on the origin item and a Projects-screen notification appears | e2e; vitest |
| TS-29 | UC-09, AC-P3.1 | L2+L4 | P0 | Given an unanswered `[approval]` request, when implementation is attempted, then it is refused; answering unblocks | inbox.py unit; skill doc test |
| TS-30 | UC-10, AC-P4.1 | L1+L4 | P0 | Given findings naming tasks, when applied, then those tasks return to `doing`; others are unchanged | inbox.py plan test |
| TS-31 | UC-10, AC-P4.1 | L1+L2 | P0 | Given three code-review rounds, when a 4th is requested, then it is refused | round-cap unit; e2e |
| TS-32 | UC-10a, AC-P4.2 | L2+L4 | P1 | Given round 3 still failing, when it ends, then work stops and findings carry to verification | inbox.py unit |
| TS-33 | UC-11, AC-P4.2 | L2+L4 | P0 | Given acceptance or round 3, when finished, then a final-verification request carries open findings | inbox.py unit; e2e |
| TS-63 | UC-12, AC-M.0 | L4 | P0 | Given the repo, when the marketplace check runs, then `.claude-plugin/marketplace.json` names `sebenza` with source `./skills` and the plugin manifest validates | unittest + `claude plugin validate` if present |
| TS-34 | UC-12, AC-M.1 | L4 | P0 | Given `skills/`, when walked, then its tree matches the source listing in `MIGRATION.md` plus recorded additions; the manifest is valid | structure unittest |
| TS-35 | AC-M.1, R-B5 | L4 | P0 | Given `skills/scripts`, when run, then python tests pass and `MIGRATION.md` lists source and post-move changes | unittest |
| TS-36 | UC-12, AC-M.1 | L6 | P1 | Given a clean machine, when the repo marketplace is added, then the plugin installs | manual checklist |
| TS-37 | AC-M.2 | L1 | P0 | Given the server role prompts, when built, then each equals its `skills/agents/*.md` bytes | include_str unit |
| TS-38 | FT-02 | L2 | P1 | Given a spoofed caller header, when calling a gated route, then the audit caller comes from the credential class | e2e audit |
| TS-39 | FT-03, R-S3 | L2 | P0 | Given stub output carrying a gated action or file write, when processed, then it is rejected or inert; no state change | e2e injection stub |
| TS-40 | FT-04, R-A1 | L2 | P0 | Given a reviewer stub that mutates snapshot or live tree, when it ends, then `ReviewRoundVoided` is logged and a request is opened | e2e `git status` / HEAD |
| TS-41 | FT-04, R-T1, R-T2 | L5 | P1 | Given a real CLI present, when told to write or reach the network, then it is blocked; skipped if absent | conformance |
| TS-42 | FT-05 | L1+L2 | P0 | Given `.claude/`, `.env`, `CLAUDE.md`, `AGENTS.md`, keys in the repo, when snapshotted, then none included; hooks off | snapshot unit; stub cwd listing |
| TS-43 | FT-06, R-S2 | L1+L2 | P0 | Given token-pattern output, when scanned, then redacted; the claude stub's argv carries the `~/.ai`/`~/.ssh` deny settings | scan unit; e2e argv |
| TS-44 | FT-09 | L1+L2 | P1 | Given a client-supplied round number, when submitted, then it is ignored; reset only via operator `newCycle` | route test |
| TS-45 | FT-11, R-S4 | L1+L3 | P0 | Given fake secret/PHI strings, when input, output or paste occurs, then warn-confirm, reject/redact, pre-paste scan fire | scan unit; vitest |
| TS-46 | FT-12, R-S5 | L4 | P0 | Given `skills/` hooks, when parsed, then imports are stdlib, loopback only; install is explicit | unittest |
| TS-47 | FT-13 | L2 | P1 | Given a turn over the size cap, when posted, then it is rejected | route test |
| TS-48 | FT-14, FT-01 | L1+L2 | P0 | Given the operator token, when restarted or expired, then it is invalid; it never appears in agent env, URL or log | token unit; env capture |
| TS-49 | FT-15 | L3 | P0 | Given `<script>` and hostile mermaid in a doc, when rendered in any tab, then nothing executes | DOMPurify vitest |
| TS-50 | FT-01, FT-14 | L2 | P0 | Given no same-origin or a bad Origin/Host, when an operator token is requested, then refused; same-origin succeeds | route test |
| TS-51 | FT-14, UC-06 | L2+L6 | P1 | Given `sebenza-cli` login, when confirmed in the UI, then a 0600 TTL token is cached; unconfirmed is refused | e2e; file-mode unit |
| TS-52 | R-T3 | L1 | P1 | Given recorded vendor fixtures, when parsed, then results are correct and drift fails loudly | fixture unit |
| TS-53 | R-D1 | L1 | P2 | Given a `solution_problem` request, when folded by request logic, then it is tolerated with no pane delivery | fold unit |
| TS-54 | R-D4, R-D5 | L1+L2 | P1 | Given an edit after copy, when viewed, then the copy shows stale; a re-created worktree keeps its round count | e2e; scope-key unit |
| TS-55 | R-A5 | L2+L3 | P0 | Given legacy flows, when run, then `InboxView.test.tsx`, `GraphTab.test.tsx`, `inbox_service`/`inbox_routes` tests and e2e inbox tests pass unchanged | existing suites |
| TS-56 | AC-S2.1, R-S1, R-T2 | L2+L3 | P0 | Given grok, when shown in the picker, then the "read-only unverified" badge shows; as reviewer it is refused with a clear reason | vitest; e2e |
| TS-57 | R-A2, R-T4 | L2 | P1 | Given a project review, when run, then the reviewer's cwd is the snapshot, not the live tree | e2e cwd assertion |
| TS-58 | R-B4 | L2 | P1 | Given a shared-token call to a gated route, when made, then it is denied and audited | e2e audit |
| TS-59 | UC-08, AC-P2.1, R-D5 | L1+L2 | P0 | Given three project design rounds for a scope key, when a 4th or a re-created worktree's round is requested, then 409 `round_limit` | round-cap unit; e2e |
| TS-60 | UC-06, AC-S6.2 | L2 | P0 | Given a drafted instruction, when the operator edits then approves it, then `solution-handover.md` equals the edited bytes | e2e |
| TS-62 | FT-01 | L2 | P0 | Given any gated action, when it succeeds, then an Overall-thread comment records action, credential class and hash | e2e |
| TS-61 | FT-06, R-S2 | L5 | P1 | Given real claude present, when asked to read `~/.ai`, then the read is denied; skipped if absent | conformance (sign-off) |

Untested by decision: R-B1 (approval fatigue is a metric), R-D3 (compaction). R-B3 is covered by TS-41 and TS-56. TS-41 and TS-61 are required sign-off evidence for FT-04 and FT-06.

## Traceability
| Source | Scenarios |
|---|---|
| UC-01 / 01a | TS-01 / TS-02 |
| UC-02 / 02a / 02b | TS-03, 04, 05 / TS-03 / TS-06 |
| UC-03 / 03a | TS-08, 09, 11 / TS-10 |
| UC-04 / 04a / 04b / 04c | TS-12, 13 / TS-14 / TS-15 / TS-16 |
| UC-05 / 05a | TS-17 / TS-18, 19 |
| UC-06 / 06a / 06b | TS-20, 21, 51, 60 / TS-22, 24 / TS-21 |
| UC-07 | TS-25 |
| UC-08 / 08a | TS-26, 59 / TS-27 |
| UC-09 | TS-28, 29 |
| UC-10 / 10a | TS-30, 31 / TS-32 |
| UC-11 | TS-33 |
| UC-12 | TS-63, 34, 35, 36 |
| AC-S1.1, S1.2 | TS-01; TS-02 |
| AC-M.0 | TS-63 |
| AC-S2.1, S2.2, S2.3 | TS-03, 56; TS-04; TS-05, 06 |
| AC-S3.1, S3.2, S3.3 | TS-08; TS-09; TS-10 |
| AC-S4.1, S4.2, S4.3 | TS-12, 13; TS-14; TS-15, 16 |
| AC-S5.1, S5.2 | TS-17; TS-18, 19 |
| AC-S6.1, S6.2, S6.3, S6.4 | TS-20; TS-21, 60; TS-22; TS-23 |
| AC-P1.1 / P2.1 / P3.1 | TS-25 / TS-26, 27, 59 / TS-28, 29 |
| AC-P4.1, P4.2 | TS-30, 31; TS-32, 33 |
| AC-M.1, M.2 | TS-34, 35, 36; TS-37 |
| FT-01 / 02 / 03 / 04 / 05 | TS-23, 48, 50, 62 / TS-38 / TS-39 / TS-40, 41 / TS-42 |
| FT-06 / 07 / 08 / 09 / 10 | TS-43, 61 / TS-22, 24 / TS-23 / TS-13, 44 / TS-05 |
| FT-11 / 12 / 13 / 14 / 15 | TS-45 / TS-46 / TS-11, 47 / TS-48, 50, 51 / TS-49 |
| R-A1..A5 | TS-40, 41 / TS-57 / TS-24 / TS-07 / TS-55 |
| R-B2, B4, B5 | TS-19; TS-58; TS-35 |
| R-D1, D2, D4, D5 | TS-53; TS-06; TS-54; TS-54, 59 |
| R-T1..T5 | TS-41 / TS-41, 56 / TS-52 / TS-57 / TS-11 |
| R-S1..S5 | TS-56 / TS-43 / TS-05, 39 / TS-45 / TS-46 |

## Test Data & Environments
- Stub agents in `testdata/` (happy, changes, injection, mutating-reviewer, sleeper, malformed-JSON), selected by env var.
- Synthetic prose only; secret/PHI patterns are fabricated (`AKIAFAKE…`, `000-00-0000`).
- Each e2e test gets a temp `$HOME`, inbox store and git repo with registered and unregistered project names.
- Tokens are minted in-process per test; none in fixtures or logs.
- Vendor output fixtures for claude, codex and grok, recorded once with synthetic prompts.
- L5 runs only when the CLI is on PATH, in a throwaway repo; absent means skipped.
- Frontend tests mock `./api`; no daemon.
- L6 uses the operator's machine and a scratch repo.

## Entry & Exit Criteria
- Entry: design and architecture documents approved and unchanged since this plan.
- Entry: stub agents and the fixture-repo helper exist before any L2 scenario; L4 scenarios start after the migration task, and `workflow.md` gains the python command.
- Exit: all P0 scenarios pass in CI; P1 pass or carry a recorded waiver.
- Exit: `cargo test`, `CI=true npm run test:coverage`, `python3 -m unittest discover -s skills/scripts` green.
- Exit: >80% coverage on new code.
- Exit: every UC, AC and Critical/High FT has a passing scenario.
- Exit: TS-41 and TS-61 run once per installed CLI, results recorded.
- Exit: TS-36 and TS-51 signed off by the operator.

## Coverage Map
Omitted: the Traceability table already shows the full fan-out; a 63-scenario diagram adds nothing.

## Open Questions
1. Can Claude `Read` confinement be proved by L5, or must the daemon stage a copy? Decides TS-43.
2. If an audited independence override is allowed, TS-18 needs an alternate scenario.
3. If project findings after round 3 become one request each, TS-32/TS-33 change.
4. If the approval hash covers notes too, TS-22/TS-24 widen.
5. Is drift between the two skill copies checked by test, or only by `MIGRATION.md`?
