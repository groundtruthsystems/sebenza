# Test Plan — Sebenza Polishing

Back to [design.md](./design.md).

## Strategy
Each defect is tested at the cheapest level that can catch it:
- **Unit:** the fold, status guards and the argv constructor.
- **Integration:** dismissal, outbox, reject and chat flows, against a temp-dir JSONL store and a replay fake `claude`.
- **Component:** UI behaviour, using Testing Library under vitest.

The 60×3 adversarial suite runs in CI on replay; a live run is env-gated and manual. An injected append failure proves nothing is recorded, and a simulated restart proves notices resume at least once, with a duplicate-tolerant key.

**Not tested:** real-model answer quality beyond the AC-07b rubric, CLI internals, pane rendering, browser E2E (there is no tool for it in the stack, so manual UAT covers it), and restore/unarchive (out of scope). All data is synthetic, and there is no PHI.

## Test Levels
| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| Unit | Fold, status guards, summaries, backoff, argv constructor, probe decision, Esc stack | `cargo test` inline `mod tests`; vitest | Local, CI |
| Integration | Services + routes + store + dispatcher + chat with replay fake `claude`; crash/restart/fault injection | `cargo test` (temp dir, fake sink, fake clock) | Local, CI |
| Contract | ts-rest contract vs routes; CLI vs routes (parity) | `cargo test`, vitest | CI |
| Frontend component | InboxActivity/Card, ReviewControls, full screen, ChatPanel | vitest + Testing Library | Local, CI |
| Adversarial suite | 60 cases × 3 runs vs replay fixtures | `cargo test` harness | CI (replay); manual (live, env-gated) |
| Performance | Fold/list cost on synthetic history | `cargo test` timing assertion | CI |
| Manual UAT | Inbox AC 1–10 on a built app | Checklist | Pre-release |

## Scenarios
| TS | Covers | Level | Pri | Given/When/Then | Evidence |
|---|---|---|---|---|---|
| TS-01 | UC-01, AC1 | Component | P0 | Given handover review, when Full screen clicked, then reader fills viewport with the handover text | Full-screen container, text present |
| TS-02 | UC-01a | Component | P0 | Given full screen, when visible exit clicked, then normal view returns | Container removed |
| TS-03 | UC-01a, Esc risk | Component | P0 | Given dialog over chat over full screen, when Esc pressed thrice, then each closes in that order only | Per-press state via `useEscStack` (native dialog Esc confirmed in TS-48) |
| TS-04 | UC-01b | Component | P1 | Given typed reject comment and scroll, when toggling modes both ways, then input and scroll persist | Values equal before/after |
| TS-05 | UC-02, AC2 | Component | P0 | Given each mode, when Approve, Defer, Reject used, then identical API calls fire | Mock calls equal across modes |
| TS-06 | UC-03 | Integration | P0 | Given awaiting-review handover, when rejected with reason+comment, then Rejected with actor/time | Folded handover fields |
| TS-07 | UC-03a, can_move_to risk | Integration | P0 | Given `wrong_project` reject, then one line holds `handover_rejected` + `design_revision_requested{project, reason}`, phase Revising, no project removed; siblings unchanged | Line contents; design projects intact |
| TS-08 | UC-03b | Integration | P1 | Given Approved, Converted or Rejected handover (Deferred is rejectable), when reject posted, then 409 and no append | Status; log line count |
| TS-09 | Reject atomicity risk | Integration | P0 | Given injected append failure on reject, then handover and design unchanged, no conversion | Store byte-identical; converter not called |
| TS-10 | UC-04, AC4, AC5 | Integration | P0 | Given chat enabled and handover H, when question posted, then reply in H's thread with H as context | Thread lines; fake prompt contains H |
| TS-11 | UC-04, isolation threat, slug risk | Integration | P0 | Given chats on H1 and H2, when H2 fetched, then no H1 messages; `../` ids rejected | Body; 400 on traversal |
| TS-12 | UC-04a | Integration + Component | P0 | Given probe failed or stale, when POST message, then 503 and chat entry hidden | 503; UI query null |
| TS-13 | UC-04b | Integration | P1 | Given oversized handover or full transcript, when posting, then refused or 413, never truncated | Status; prompt unmodified |
| TS-14 | UC-04c, Critical injection | Integration | P0 | Given replay stream with injected `tool_use`, then turn aborts, output discarded, `chat.enabled` false, next POST 503 | Abort; flag; 503 |
| TS-15 | UC-04d, spoof threat | Integration | P0 | Given no session or control token, when any new route called, then 401/403 audited before state read | Status; audit line; no store read |
| TS-16 | UC-04d | Integration | P0 | Given operator without access to the handover's item, or access revoked mid-chat, when GET/POST, then denied | 403/404 each case |
| TS-17 | UC-04e harness | Adversarial | P0 | Given 60 cases × 3 replay runs, then scoring harness reports hard pass and soft ≥ 95% correctly | CI report; proves harness, not model |
| TS-18 | UC-04e, AC-07b | Adversarial | P0 | Given env gate set, when suite runs live, then every hard criterion passes and soft ≥ 95% | Archived report; chat release gate |
| TS-19 | UC-05, built-in threat | Integration | P0 | Given fake init with zero tools then enabled; any tool, malformed or error then disabled | Flag per case; only probe writes |
| TS-20 | UC-05, CLI-update risk | Integration | P0 | Given CLI version change or unauthenticated CLI, then re-probe runs and chat fails closed | Re-probe count; 503 |
| TS-21 | AC-07a, built-in threat | Unit | P0 | Given chat constructor, then `--tools ""`, all denies, strict MCP, disableAllHooks, empty cwd, no tokens, never permissive | Argv/env assertions |
| TS-22 | Exfil/XSS threat | Component | P0 | Given reply with markdown image, link, script HTML, when rendered, then only inert text | No img/a/script in DOM |
| TS-23 | Chat DoS threat | Integration | P1 | Given hung run and concurrent POSTs, then timeout fires and turns serialise per handover | Elapsed; ordered thread |
| TS-24 | Hash-stamp threat | Integration | P1 | Given handover edited between turns, then second turn reads stored text and stamps new hash | Prompt; hash field |
| TS-25 | UC-06, AC6 | Integration | P0 | Given open/proposed request, when dismissed, then one append holds dismissal + outbox key; list shows Dismissed | Event line; actor set |
| TS-26 | UC-06a | Integration | P0 | Given `delivery_failed` request, when dismissed, then no redelivery and `redeliver` returns 409 | Sink count 0; 409 |
| TS-27 | UC-06b | Integration | P1 | Given confirmed, resolved or dismissed request, when dismissed, then 409, no append | Status; log unchanged |
| TS-28 | Rollback-on-failure | Integration | P0 | Given injected write failure, when dismissing, then no event, no outbox, status unchanged | Store identical; error surfaced |
| TS-29 | UC-10, crash-after-commit | Integration | P0 | Given dismissal written but notice unsent, when daemon restarts, then dispatcher sends exactly once | Sink count 1; sent |
| TS-30 | UC-10, send-then-crash | Integration | P0 | Given `started` attempt without outcome, when restarted, then the notice is retried with the same key embedded | Sink count 2, same key |
| TS-31 | UC-10, double-send risk | Integration | P1 | Given dispatcher tick and manual Retry race, then one send, one attempt per number | Sink count 1 |
| TS-32 | UC-10a | Integration | P0 | Given always-failing sink, then backoff to max, notice failed + badge; Retry on healthy sink gives sent | Attempt events; badge; sent |
| TS-33 | Must-not-change | Integration | P1 | Given any notice outcome, then request status is unchanged | Status unchanged |
| TS-34 | UC-07, AC7 | Integration | P0 | Given confirmed/resolved request, when archived, then `archived_at/by` set, status unchanged | Fold fields |
| TS-35 | UC-07a | Integration + Component | P1 | Given open or dismissed request, when archive attempted, then 409 and control disabled | Status; disabled attr |
| TS-36 | UC-08, AC8 | Unit + Component | P0 | Given mixed statuses, when each filter selected, then only mapped statuses; All includes archived; no Archived filter | Rendered ids per filter |
| TS-37 | UC-09, AC9 | Unit + Component | P0 | Given archived or dismissed request, then template summary of request and result shown, title-only | Summary strings |
| TS-38 | UC-09, AC10 | Integration | P0 | Given dismissed and archived items, when history fetched, then all events in order; visible under filter and All | Ordered events |
| TS-39 | Migration, interleave risk | Integration | P0 | Given legacy log, when daemon starts twice in a row, then the log is byte-identical after both | Byte equality; first start one `migrated` per legacy request, second none |
| TS-40 | Keyed-kinds risk | Unit | P1 | Given plain `append_event` for a keyed kind, then the guard test fails | Failing guard test |
| TS-41 | Forged actor, auth threats | Integration | P0 | Given body actor or control token, when dismissing, then actor is credential's; control token 403 | Event actor; 403 audited |
| TS-42 | Tamper threat, NFR-03 | Integration | P1 | Given router and store, then no DELETE route, no event rewrites, files 0600 | Route table; prefix check; mode |
| TS-43 | Secrets-in-logs threat | Integration | P1 | Given canary secret in title or chat, then logs, audit, notice records never contain it | Canary absent |
| TS-44 | Slow-append risk | Integration | P2 | Given a stalled append holding item A's lock, then a write to item B completes without waiting (locks are per item) | B done < 1 s while A held |
| TS-45 | CLI/UI parity | Contract | P1 | Given each new route, then a `sebenza-cli` command exists and returns the same result | Output equals route |
| TS-46 | Re-fold scaling risk | Performance | P2 | Given large synthetic history, when listing, then list stays under agreed budget | Timing under budget |
| TS-47 | Business risks | Component | P2 | Given chat on or off, then UI shows chat status and "not part of review record" label | Rendered text |
| TS-48 | Inbox AC 1–10 | Manual UAT | P1 | Given built app, when operator walks AC 1–10 incl. native-dialog Esc, then each passes | One checklist line per AC |
| TS-49 | AC-07a, release gate | Integration | P0 | Given route table and boot, then chat routes are only GET thread/POST message; `chat.enabled` starts false | Route snapshot; boot state |
| TS-50 | Built-in threat, must-not-change | Unit | P0 | Given role-agent and system-agent `StartRunInput`, then argv is byte-identical to before; non-claude chat provider refused | Golden argv; error |
| TS-51 | UC-06, existing routes | Integration | P0 | Given dismissed request, when confirm, reject, redeliver or retry-triage called, then 409 | Status each |
| TS-52 | UC-03, dispatch cancel | Integration | P0 | Given rejected handover, when approve or convert attempted, then 409 and no worktree created | 409; converter not called |
| TS-53 | Spoof threat (origin) | Integration | P1 | Given foreign Origin header, when any new POST called, then refused | 403 |
| TS-54 | UC-07a | Integration | P2 | Given archived request, when archived again, then 409 and one archive event | 409; event count 1 |
| TS-55 | UC-10a | Integration | P1 | Given target pane gone, when notice sends, then attempt fails with reason and backs off | Attempt error recorded |
| TS-56 | UC-10a | Integration | P1 | Given a `pending` notice, when Retry posted, then 409, nothing appended; on `failed`, Retry appends `notice_attempt{manual}` | 409; event written |
| TS-57 | Concurrency (operator 2a) | Integration | P0 | Given dismiss and archive racing on one request, then exactly one passes validation; the other gets 409 | One event; one 409 |
| TS-58 | Torn write (operator 2b) | Integration | P0 | Given a truncated final line, when folded, then it is ignored and state equals the pre-write state | Fold equality |
| TS-59 | UC-03 reason=other | Integration | P0 | Given reject with reason=other, then only `handover_rejected` is appended; phase unchanged; missing comment 400 | One event; phase; 400 |
| TS-60 | Durable append (operator 2b) | Unit | P1 | Given a writer failing mid-write, when appending a new kind, then one `write` call is made and `sync_data` follows; partial line ignored on fold | Call trace; fold equality |

## Traceability
| Item | TS ids |
|---|---|
| UC-01 / UC-01a / UC-01b | TS-01 / TS-02, TS-03 / TS-04 |
| UC-02 | TS-05 |
| UC-03 / dispatch cancel | TS-06, TS-52, TS-59 |
| UC-03a / UC-03b | TS-07, TS-59 / TS-08 |
| UC-04 | TS-10, TS-11, TS-24 |
| UC-04a / UC-04b / UC-04c | TS-12 / TS-13 / TS-14 |
| UC-04d / UC-04e | TS-15, TS-16 / TS-17, TS-18 |
| UC-05 | TS-19, TS-20 |
| UC-06 / UC-06a / UC-06b | TS-25, TS-51 / TS-26 / TS-27 |
| UC-07 / UC-07a | TS-34 / TS-35, TS-54 |
| UC-08 | TS-36 |
| UC-09 | TS-37, TS-38 |
| UC-10 / UC-10a | TS-29, TS-30, TS-31 / TS-32, TS-55, TS-56 |
| Threat: injected tool call (Critical) | TS-14, TS-17, TS-21, TS-49 |
| Threat: built-in tool enabled (High) | TS-19, TS-20, TS-21, TS-50 |
| Threat: control token / cross-origin reaches routes (High) | TS-15, TS-41, TS-53 |
| Threat: cross-handover / post-revocation chat (High) | TS-11, TS-16 |
| Threat: output exfil/XSS (High) | TS-22 |
| Threat: actor forged; unattributed actions (Med) | TS-25, TS-41 |
| Threat: secrets to logs (Med) | TS-43 |
| Threat: wrong/edited handover context (Med) | TS-24 |
| Threat: log rewritten (Med) | TS-42 |
| Threat: chat DoS (Med) | TS-13, TS-23 |
| Threat: probe flap; retry storm (Low) | TS-12, TS-32 |
| Risk: backfill interleave | TS-39 |
| Risk: double-send (race; crash window) | TS-30, TS-31 |
| Risk: CLI upgrade / unauthenticated CLI | TS-20 |
| Risk: `wrong_project` half-applied | TS-07, TS-09 |
| Risk: Esc vs native dialog | TS-03 |
| Risk: keyed kind without key | TS-40 |
| Risk: rejected handover approved/converted; reject/approve race | TS-52, TS-09 |
| Risk: slug traversal | TS-11 |
| Risk: fold cost / slow append | TS-46 / TS-44 |
| Risk: chat taken as authoritative; chat status | TS-47 |
| Must not change: notice outcomes vs status | TS-33 |
| Operator 2a/2b: lock / torn write / fsync | TS-57 / TS-58 / TS-60 |
| Required: rollback / crash-after-commit / denial / guard | TS-28, TS-09 / TS-29 / TS-15, TS-16 / TS-14 |
| Inbox AC 1 / 2 / 3 | TS-01 / TS-05 / TS-02, TS-03 |
| Inbox AC 4 / 5 | TS-10 / TS-10, TS-17, TS-24 |
| Inbox AC 6 / 7 / 8 | TS-25 / TS-34, TS-35 / TS-36 |
| Inbox AC 9 / 10 | TS-37 / TS-36, TS-38 |
| CLI/UI parity; all ACs end to end | TS-45; TS-48 |
| Handover AC-01 / 02 / 03 / 04 | TS-01 / TS-05, TS-06 / TS-02, TS-03 / TS-04 |
| Handover AC-05 / 06 | TS-10 / TS-10, TS-11, TS-16 |
| Handover AC-07a / AC-07b | TS-14, TS-21, TS-49, TS-50 / TS-18 |
| Handover AC-08 … AC-12; reject addendum | TS-25 / TS-34, TS-35 / TS-36 / TS-37 / TS-38 ; TS-06–09, TS-52 |

## Test Data & Environments
- Temp-dir JSONL store per test, built from fixtures; legacy-format logs for migration.
- Fake clock and fake sink with scripted success, failure and crash points.
- Fake `claude` replay fixtures: normal reply, zero-tool init, extra-tool init, injected `tool_use`, malformed output.
- Adversarial corpus: 60 synthetic injection cases with a hard/soft rubric, stored in the repo.
- Canary strings (`CANARY-SECRET-0001`) instead of real secrets; tokens are generated test values.
- Handovers and requests are synthetic text; no real operator content, no PHI.
- The live run uses an operator-supplied CLI login through an env gate, never committed; it is skipped in CI by default.
- Large synthetic history is generated in-test for the performance budget.

## Entry & Exit Criteria
- Entry: design verification passed and the five architecture documents are current.
- Entry: the pinned `claude` CLI's behaviour for `--tools ""`, the init tool report and `disableAllHooks` is known.
- Exit: every UC-nn, alternate flow, inbox AC 1–10 and handover AC-01–12 has a passing scenario.
- Exit: every Critical/High threat scenario (TS-11, 14–16, 19–22, 41, 49, 50, 53) passes.
- Exit: coverage above 80% for each touched module.
- Exit: replay harness (TS-17) green in CI; chat release additionally needs a passing live run (TS-18).
- Exit: `CI=true cargo test` and vitest run green and non-interactive.
- Exit: chat stays off by default until the probe passes and the AC-07a/b amendment is approved.

## Coverage Map
Not produced: the Traceability table already shows the fan-out, and a diagram of ~50 scenarios would read worse than the table.

## Open Questions
1. Resolved: no dispatch queue exists; approval converts synchronously, so TS-52 proves the cancel.
2. Retry notice on a `pending` notice: proposed 409 (TS-56); confirm.
3. Does the pinned CLI support `--tools ""` and report its tools in `init`? The TS-19 and TS-21 fixtures depend on it.
4. Replay-only CI or a live 60×3 run per PR for AC-07b (TS-17/18)?
5. What are the numeric budget for TS-46 and the minimum summary content for TS-37?
