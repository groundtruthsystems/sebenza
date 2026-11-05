# Test Plan: Inbox nervous system (bug)

## Strategy
This is a bug track, so testing is regression-first: every scenario must fail against the current code before it is fixed. Most of the proof sits at the cheapest levels:

- **Rust unit tests**: `inbox_service` (path matching, comment delivery, notification), `worktree_service` (environment path) and `inbox_convert` (launch prompt).
- **Axum route tests** with `FakePane`: HTTP status codes, the summary endpoint and delivery wiring.
- **One real-router round trip** (TS-14): proves the pieces work together.
- **Vitest**: the nav badge and the comment composer.

Not tested: real tmux or agent panes, browser desktop notifications (out of scope), worktrees that are already running, and the real `sebenza-agentctl` binary (the test posts its payload directly). Symlink cases are skipped on non-Unix platforms.

## Test Levels
| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| Unit | `inbox_service` ingest/comment/notification; `worktree_service` env; `inbox_convert` prompt | `cargo test -p common`, tempdirs | local, CI |
| Integration (route) | `/api/inbox/*`, `/api/runtime/events`, FakePane | `cargo test -p sebenza-server` (`inbox_routes.rs`) | local, CI |
| End-to-end (API) | Full round trip through real router | `e2e_tests.rs` | local, CI |
| Frontend component | Nav badge, composer, failure/retry UI | vitest + Testing Library | local, CI |

## Scenarios
| TS | Covers | Level | Pri | Given/When/Then | Evidence |
|---|---|---|---|---|---|
| TS-01 | AC-01, FR-01 | Unit + route | P0 | Given converted item, When ingest claims recorded path with `..` segments (or a missing path, lexically), Then accepted | Ok; request `open`; no 403 |
| TS-02 | AC-01, FR-02 | Unit + route | P0 | Given tempdir outside every conversion, When claimed, Then refused | 403; zero events written |
| TS-03 | AC-01, NFR-01 | Unit | P0 | Given right path with wrong branch, or symlink to another item's worktree, When ingested, Then refused | 403 both; missing bearer still 401 |
| TS-04 | AC-02, FR-03 | Unit | P0 | Given `worktreeRoot: ../worktrees`, When worktree created, Then env path is canonical | env and `runtime.env` equal `canonicalize(path)` |
| TS-05 | AC-03, FR-04 | Unit | P1 | Given architect-first and direct modes, When launch prompt built, Then Operator channel section present | contains `request`, `comment`, pasted-first rule |
| TS-06 | AC-04, FR-05 | Unit + route | P1 | Given equal-priority items with/without open requests, When listed, Then counts shown and open-request item first | `openRequestCount`; order; resolved not counted |
| TS-07 | AC-04, FR-06 | Route | P1 | Given N open requests on non-dropped items, When `GET /api/inbox/summary`, Then `openRequests == N` | JSON; 0 when none; drops after confirm |
| TS-08 | AC-04, FR-06 | Frontend | P2 | Given summary 3 then 0, When nav renders and polls, Then badge "3" then hidden | DOM badge |
| TS-09 | AC-05, FR-07, NFR-02 | Unit + route | P0 | Given request body with `SYNTHETIC-PHI-MARKER-123`, When opened, Then `inbox_request` notification without the marker | row names item, branch, title, link |
| TS-10 | AC-06, FR-08 | Unit + route | P0 | Given worktree thread, When operator comments (`deliver` omitted), Then pane receives prefixed text | FakePane text; delivery `delivered` |
| TS-11 | AC-06, FR-08, FR-09 | Unit + route | P0 | Given `deliver=false` or overall thread, When commenting, Then nothing pasted | FakePane empty; comment stored |
| TS-12 | AC-06, FR-09 | Frontend | P1 | Given worktree and overall threads, When composer renders, Then "Send to agent" ticked on worktree only | checkbox; payload `deliver` |
| TS-13 | AC-06, FR-10 | Unit + frontend | P1 | Given failing FakePane, When commenting then retrying, Then failure recorded and shown; retry appends a `delivered` status | status events; Retry button |
| TS-14 | AC-07 | E2E (API) | P0 | Given relative-root conversion, When agentctl payload with non-canonical path posted, Then listed, notified, confirm reaches pane | open request; notification; FakePane reply; summary 1→0 |

## Traceability
| Item | Scenarios |
|---|---|
| AC-01 | TS-01, TS-02, TS-03 |
| AC-02 | TS-04 |
| AC-03 | TS-05 |
| AC-04 | TS-06, TS-07, TS-08 |
| AC-05 | TS-09 |
| AC-06 | TS-10, TS-11, TS-12, TS-13 |
| AC-07 | TS-14 |
| NFR-01 | TS-03 (existing TS-37 forged-origin test must stay green) |
| NFR-02 | TS-09 |

## Test Data & Environments
- Each test creates its own tempdir git worktree with a synthetic item id, branch and title. No real repositories and no tmux.
- The non-canonical path is built by inserting `/../<dir>/` into the tempdir path. The symlink fixture is Unix-only.
- Request bodies use `SYNTHETIC-PHI-MARKER-123`. There is no real PHI.
- Bearer tokens are throwaway test strings.
- Tests use `FakePane`, plus a failing variant for TS-13.
- Vitest mocks the API client, so no network is involved.

## Entry & Exit Criteria
- Entry: spec approved, and the existing inbox tests (including TS-37) are green.
- Entry: each TS is seen failing on the unfixed code where a failure is possible (at least TS-01, TS-04, TS-10 and TS-14).
- Exit: TS-01..TS-14 pass, and `cargo test --workspace` and vitest are green.
- Exit: new code has more than 80% coverage, and clippy, lint and typecheck are clean.
- Exit: no fixture contains a real credential or PHI.

## Coverage Map
Omitted. The AC → TS table is a clean one-to-few mapping.

## Open Questions
- Resolved: a missing claimed path uses the lexical fallback (FR-01).
- Resolved: events are append-only, so a retry appends a delivery-status event rather than mutating one.
- Resolved: the summary counts open requests on items that are not dropped.
