# Test Plan: Sebenza 2.0 — System Agent & Inbox Collaboration

Derived from [design.md](./design.md). Scenario IDs `TS-nn` cite `UC-nn`, risk, or threat IDs from the design.

## Strategy
Fast, deterministic levels carry the weight. Rust unit tests cover the pure logic: the request state machine, the event fold, priority precedence, schema validation, the prompt builders, and escape stripping. Rust integration tests cover routes, the per-item queue, the semaphore and timeout, and delivery, using a fake stream-json **stub CLI** and a fake `send_prompt` sink. Vitest and Testing Library cover the Inbox UI. Two end-to-end checks run the real daemon with the stub CLI. Real agent CLIs are never called: no model spend, no flakiness. Recorded fixture contract tests pin each CLI's resume and stream contract (TA-R4). Deliberately untested: model output quality, real tmux scrollback, and real LLM injection resistance, which fencing and sanitiser tests proxy. T-01 is an accepted residual risk, so its test documents the behaviour and audit rather than a denial.

## Test Levels
| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| Unit (Rust) | State machine, event fold, priority ordering, output parser, prompt builders, sanitiser, secret scan | `cargo test`, inline `mod tests` | Dev, CI |
| Integration (Rust) | Routes, `InboxService` on a tempdir store, `SystemAgentService` queue/semaphore/timeout, delivery, runtime-event ingress, restart recovery | `cargo test` + stub CLI + fake pane sink | Dev, CI |
| Contract | Per-CLI resume argv and stream-json fixtures (claude, grok, codex; opencode rejected); ts-rest contract vs `api.ts` | `cargo test` on recorded fixtures; ts-rest types | CI |
| UI component | `InboxView` sort, priority control, grouped comments, request card, `ConvertDraftDialog` | vitest + Testing Library | Dev, CI |
| End-to-end | Request → triage → confirm → delivery; convert | `cargo test` harness on loopback (stub CLI, fake pane) | CI |
| Manual / UAT | Live pane paste, real CLI smoke, disclosure text | Checklist | Operator machine, synthetic item |

## Scenarios
| TS | Covers | Level | Pri | Given / When / Then | Evidence |
|---|---|---|---|---|---|
| TS-01 | UC-01 | Integration | High | Draft + stub agent; draft-help runs; `proposed_body` returned, draft file unchanged | Response; file hash unchanged |
| TS-02 | UC-01 | UI | Med | Proposal shown; operator accepts; editor holds body, saves via hash-gated PUT | Editor value; PUT hash |
| TS-03 | UC-01a | Integration | High | Body changed after proposal; save with old hash; 409, nothing overwritten | 409; file unchanged |
| TS-04 | UC-01a | UI | Med | 409 on save; merge view shows both; saves only after operator resolves | Merge view; new hash |
| TS-05 | UC-02, BR-02 | Unit | High | `priority_src=operator`; agent writes priority; value unchanged | Fields unchanged |
| TS-06 | UC-02 | Integration | High | P2 by agent; PATCH P0 → operator source; PATCH null reverts to agent control | Frontmatter; audit |
| TS-07 | UC-02, BR-03 | Unit | Med | Items P0–P3, mixed dates; sorted; priority then created date, default P2 | Ordered ids |
| TS-08 | UC-02, DA-R1 | Integration | Med | Concurrent editor save and priority write; neither update lost | Both fields present |
| TS-09 | UC-03, T-13 | UI | High | Comments from all three authors with script markup; rendered in correct group as inert text | No script node; groups |
| TS-10 | UC-03, DA-R2 | Integration | Med | 50 concurrent comment posts; 50 intact JSONL lines; torn tail tolerated | Line count; parse OK |
| TS-11 | UC-04, BR-04 | Integration | High | `inbox-origin.json` present; agentctl request; lands open in origin item's worktree group | Event; group key |
| TS-12 | UC-04, T-06 | Integration | High | Caller worktree not in `conversions[]`; posts request; refused, nothing appended | 4xx; no event |
| TS-13 | UC-04, T-09, TA-R1 | Integration | Med | Flood over body cap, rate, or depth; excess rejected or coalesced | 413/429; depth bounded |
| TS-14 | UC-05, AA-D2 | Integration | High | Stub returns triage with proposal; priority set, proposal comment posted, status proposed | Events; status |
| TS-15 | UC-05, T-05 | Unit | High | 30 open items; digest built; 20 entries, titles and priority only | Digest content |
| TS-16 | UC-05, TA-R4 | Contract | High | Per-CLI fixtures parsed; final message + session_id extracted; argv correct; opencode rejected | Fields; argv; config error |
| TS-17 | UC-05, AA-D1 | Integration | High | Two requests on one item; runs serial, never hit active-run rejection | Non-overlapping starts |
| TS-18 | UC-05, TA-R2 | Integration | High | 10 jobs race on one item; exactly one child at a time | Spawn count |
| TS-19 | UC-05, TD-1 | Integration | Med | Semaphore 2, five items trigger; at most two children run | Peak = 2 |
| TS-20 | UC-05a | Integration | High | Stub exits non-zero; request stays open and flagged; queue continues | Flag; next job runs |
| TS-21 | UC-05a, TD-1 | Integration | High | Stub hangs; timeout elapses; child killed, request flagged | Child gone; flag |
| TS-22 | UC-05a, AA-D2 | Unit | High | Malformed JSON, unknown field, wrong version; rejected as failed run | Parse error; no mutation |
| TS-23 | UC-05b, BR-07 | Integration | Med | `recommendation=none` or `advice`; priority set, advice only as comment, nothing delivered, status open | No proposal; sink idle |
| TS-24 | UC-05, BR-02 | Integration | High | Operator override; triage returns other priority; stored priority unchanged | Priority unchanged |
| TS-25 | UC-06, T-02 | Integration | Critical | Proposed request confirmed; `send_prompt` once to origin pane; resolved | Sink call; delivered |
| TS-26 | UC-06 | Integration | High | Confirm with edited text; edited text sent; original proposal retained | Sink text; event chain |
| TS-27 | UC-06a, T-11 | Integration | High | Pane missing or other worktree's; confirm; `delivery_failed`, open, nothing pasted | Status; sink idle |
| TS-28 | UC-06a | Integration | Med | `delivery_failed`; operator redelivers; new attempt, idempotent on `(request_id, attempt)` | One send per attempt |
| TS-29 | UC-06b | Integration | High | Proposed request rejected with reason; open, reason recorded, nothing delivered | Event; sink idle |
| TS-30 | UC-06, BR-R1 | UI | Med | Edited proposal; card shows verbatim text and diff; decider recorded | Diff; `decided_by` |
| TS-31 | T-02 | Integration | Critical | Unconfirmed request; system-agent path or tampered hash tries delivery; refused; router enumeration shows only confirm/redeliver reach `send_prompt` | Sink idle; 4xx; route list |
| TS-32 | T-01, SA-R1 | Integration | Critical | Control token used by a worktree caller on confirm, PATCH priority, and convert; each succeeds (accepted); audit records `actor: operator` plus unauthenticated `caller: worktree`; agentctl lacks confirm/priority/convert | 2xx; audit; help text |
| TS-33 | T-03 | Unit | High | Injected escape and tmux key syntax; prepared; stripped, capped, fenced as data | Sanitised output |
| TS-34 | T-03 | Integration | High | Injected request text, proposal confirmed; paste only to origin worktree, text stripped | Sink target; sink text |
| TS-35 | T-04, SA-R2 | Unit | High | Any agent config; argv built; read-only allowlist, no yolo, empty cwd; unrestrictable CLI refused | Argv assertions |
| TS-36 | T-05 | Integration | High | Item B triage digest built while item A exists; digest exposes no bodies or comments of A | Prompt content |
| TS-37 | T-06, SA-R3 | Integration | High | Forged `inbox-origin.json` naming another item; request posted; path cross-check refuses | 4xx; no event |
| TS-38 | T-07, DA-R4 | Unit | High | Fake secret in comment, request, proposal; scanned; flagged or redacted before model or pane | Redacted text |
| TS-39 | T-07 | Integration | Med | New store writes; files created with mode 0600 | File mode |
| TS-40 | T-08 | Integration | Med | Full request lifecycle; each event audited metadata-only with actor, no body | Audit lines |
| TS-41 | T-10 | Integration | Med | Missing bearer, foreign Origin, bad Host on new routes; each rejected | 401/403 |
| TS-42 | T-12 | Integration | Med | Proposal altered on disk after display; confirm with old hash refused | 409; no delivery |
| TS-43 | T-13 | UI | Med | Markdown with script or `javascript:` link; rendered sanitised | DOM assertions |
| TS-44 | UC-07 | Integration | High | Item with comments; convert/instructions; per-target `system_instruction` returned | Targets |
| TS-45 | UC-07 | Unit | High | Item, operator prompt, system instruction; builder runs; architect-first prompt contains all three | Prompt text |
| TS-46 | UC-07, BR-06 | UI | Med | Operator edits an instruction in dialog; edited text is submitted | Payload |
| TS-47 | UC-07a, BR-06 | Integration | High | Agent unavailable or fails; convert proceeds with operator prompt only | Launch prompt |
| TS-48 | UC-07b | Unit | Med | `architect_first=false`; direct instruction, no architect wrapper | Prompt text |
| TS-49 | UC-07, T-07 | Unit | Med | Fake secret in comments; convert; secret scan covers comments | Scan hit |
| TS-50 | TD-2 | Integration | Med | Restart with open, unproposed requests; re-enqueued once, deduped on `(request_id, job_kind, attempt)` | Job count |
| TS-51 | TD-3, TA-R3 | Integration | Med | Missing session id or turn cap reached; job runs; re-seeded from item + last N comments | Seed prompt; new session.json |
| TS-52 | AA-R1 | Integration | High | `AgentStreamManager` change; existing chat tests run; behaviour unchanged | Existing tests green |
| TS-53 | DD-3 | Unit | Med | v1, v2, newer frontmatter; v1 upgrades additively, newer write refused | Fields; error |
| TS-54 | DD-4, DA-R3 | Integration | Med | Drop, Delete, orphans; Drop keeps sidecars, Delete removes them first, sweep clears orphans | Dir listing |
| TS-55 | BR-05 | UI | Med | Failed triage or delivery; item flagged, nothing dropped | Flag state |
| TS-56 | UC-04, UC-05, UC-06 | E2E | High | Stub CLI + fake pane; request raised, triaged, edited, confirmed; pane gets edited text, resolved | Event chain |
| TS-57 | UC-07 | E2E | Med | Item with two targets converted; each worktree gets architect-first launch | Two prompts |
| TS-60 | T-04, TD-6 | Unit | Critical | System-agent spawn built; child env lacks `SEBENZA_CONTROL_TOKEN` and is allowlisted | Env assertions |
| TS-61 | UC-06c | Integration | High | Open request, no proposal; operator authors and confirms resolution; delivered, resolved | Events; sink text |
| TS-62 | DA-R4, T-07 | Integration | High | Comment with fake secret; `redacted` tombstone appended; reader masks body in API and UI | Masked body |
| TS-63 | UC-05a | Integration | Med | Flagged request; operator calls retry-triage; job re-runs, proposal produced | Job; proposal event |
| TS-64 | BR-02, T-08 | Integration | High | Agent then operator priority changes; `priority_changed` events record from, to, source | Events |
| TS-65 | TD-6 | Integration | Med | `enabled=false`: no jobs spawn; hung stub times out; whole process group killed | No children; pgid gone |
| TS-66 | BR-08 | UI | Med | Inbox editor and comment box show PHI warning; likely-PHI synthetic string flagged | Warning; scan hit |
| TS-67 | UC-05, TD-5 | Integration | Med | Triage queued and draft-help submitted; draft-help runs first | Start order |
| TS-58 | AA-R2 | Manual | Low | Live pane; resolution confirmed; pastes once, UI says "sent" | Sign-off |
| TS-59 | BR-R4, TD-6 | Manual | Low | Metrics reviewed; per-item `duration` and queue-depth counters present for spend visibility | Metrics sample |

## Traceability
| Item | Scenarios |
|---|---|
| UC-01 | TS-01, TS-02 |
| UC-01a | TS-03, TS-04 |
| UC-02 | TS-05, TS-06, TS-07, TS-08 |
| UC-03 | TS-09, TS-10 |
| UC-04 | TS-11, TS-12, TS-13, TS-56 |
| UC-05 | TS-14–TS-19, TS-24, TS-56, TS-67 |
| UC-05a | TS-20, TS-21, TS-22, TS-63 |
| UC-05b | TS-23 |
| UC-06 | TS-25, TS-26, TS-30, TS-56 |
| UC-06a | TS-27, TS-28 |
| UC-06b | TS-29 |
| UC-06c | TS-61 |
| UC-07 | TS-44, TS-45, TS-46, TS-49, TS-57 |
| UC-07a | TS-47 |
| UC-07b | TS-48 |
| T-01 (Critical, accepted) | TS-32 |
| T-02 (Critical) | TS-25, TS-31 |
| T-03 / T-04 / T-05 (High) | TS-33, TS-34 / TS-35, TS-60 / TS-15, TS-36 |
| T-06 / T-07 (High) | TS-12, TS-37 / TS-38, TS-39, TS-49, TS-62 |
| T-08 … T-13 (Medium) | T-08 TS-40, TS-64; T-09 TS-13; T-10 TS-41; T-11 TS-27; T-12 TS-42; T-13 TS-09, TS-43 |
| Risks | BR-R1 TS-30; BR-R4 TS-59; AA-R1 TS-52; AA-R2 TS-58; TA-R1 TS-13; TA-R2 TS-18; TA-R3 TS-51; TA-R4 TS-16; DA-R1 TS-08; DA-R2 TS-10; DA-R3 TS-54; DA-R4 TS-38, TS-62; SA-R1 TS-32; SA-R2 TS-35; SA-R3 TS-37; BR-R2, AA-R3 not testable (design OQ) |
| Rules & decisions | BR-02 TS-05, TS-24, TS-64; BR-03 TS-07; BR-05 TS-55; BR-06 TS-46, TS-47; BR-07 TS-23; BR-08 TS-66; TD-2 TS-50; TD-5 TS-67; TD-6 TS-60, TS-65; DD-3 TS-53; DD-4 TS-54 |

## Test Data & Environments
- Synthetic drafts with placeholder projects (`acme-demo`) and fixed ULIDs; no real customer data, PHI, or PII.
- Secret-scan cases use fabricated tokens (`sk-TEST-0000000000000000`); no real or expired credentials in fixtures.
- Stub agent CLI script emits stream-json fixtures: valid triage, draft_help, convert; bad JSON; unknown field; non-zero exit; hang; slow.
- Per-CLI contract fixtures: recorded, scrubbed stream samples for claude, grok, and codex, committed to the repo.
- Fake pane sink records `send_prompt` calls and simulates missing or mismatched panes.
- Every store test uses a tempdir inbox root and a per-test control token; nothing touches `~/.ai/sebenza`.
- E2E binds the daemon to loopback on an ephemeral port; no LLM egress in CI.
- Injection payloads are inert strings (escape sequences, tmux key syntax, script tags), never executed.

## Entry & Exit Criteria
- Entry: design.md and test-plan.md are approved; the stub CLI and fake pane helpers exist before the tests that need them.
- Entry: `cargo-llvm-cov` is available for the Rust coverage bar.
- Entry: the `AgentStreamManager` extension lands with TS-52 green before triage tests run.
- Exit: coverage on new and changed code is above 80% (Rust and vitest).
- Exit: every High and Critical TS passes, and every UC, alternate flow, and Critical/High threat has a passing TS.
- Exit: no test calls a real agent CLI or model provider; the fixture secret grep is clean.
- Exit: the TS-32 acceptance of T-01 is documented; TS-58 and TS-59 are signed off.
- Exit: no open Critical or High defects; existing inbox and chat tests unchanged.

## Coverage Map
Omitted: the Traceability table is authoritative, and a map of 67 scenarios would be unreadable.

## Open Questions
1. Do recorded stream-json fixtures exist for codex and grok, or must they be captured first (TS-16)?
2. Does `ResolutionDelivery` need an injectable pane trait, or can it reuse an existing `send_prompt` test seam?
3. Where does the stub CLI live, and does `systemAgent` allow a binary-path override for tests?
4. Rust coverage uses `cargo-llvm-cov` (Workflow); confirm it is installed in CI before entry.
5. TS-32 flips to a denial when scoped tokens land (design OQ-1).
