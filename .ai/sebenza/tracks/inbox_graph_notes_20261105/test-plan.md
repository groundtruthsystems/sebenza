# Test Plan: Inbox graph view & multiple notes

## Strategy
Most defects here sit in pure logic and boundaries, so the tests stay at the cheapest levels:

- **Rust unit tests**: `note_codec` (round trip, escaping), `InboxStore` hashes and conflicts, and the `TaskGraphService` resolver and confinement.
- **Rust integration tests**: `inbox_convert` through the fake `ConversionRunner`, and `inbox_routes.rs` for guards and status codes.
- **Frontend (vitest + Testing Library)**: `NotesPanel`, `GraphTab` and `graphToMermaid`.
- **End to end**: `e2e_tests.rs` carries one thin path from conversion to graph.
- **Contract**: a shared fixture guards `inbox-notes.json` v1 and `depends_on` against drift with agent-extensions-skills.

Each alternate flow and each 409 gets its own scenario. These are deliberately not tested:

- Mermaid's pixel output.
- How a real agent behaves on injected notes. T-07 is checked only as prompt framing.
- The internals of the skills in the other repo.
- Low-end hardware timing.

No new tooling. The bar is the Workflow's >80% coverage on new code.

## Test Levels

| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| Unit (Rust) | `note_codec`, note hash/`list_hash`, store ops, graph builder, ref grammar, confinement, caps | inline `#[cfg(test)]`, `cargo test` | local, CI |
| Integration (Rust) | `inbox_convert` with fake `ConversionRunner`; `inbox_routes.rs` guard/status; tempdir stores and worktrees | `cargo test`, tempdir fixtures | local, CI |
| Contract | `inbox-notes.json` v1 and `plan.json` `depends_on` fixtures; Zod/ts-rest contract vs routes | shared fixtures, `cargo test`, vitest | local, CI; fixture reused by sibling worktree |
| Unit/component (frontend) | `NotesPanel`, `GraphTab`, `graphToMermaid`, conflict UX, polling | vitest + Testing Library, fake timers, mocked `api.ts` | local, CI (`CI=true`) |
| End-to-end | convert → graph through the real router | `e2e_tests.rs` | local, CI |
| Security | guard, confinement, XSS, marker forgery, PHI file hygiene | the levels above, tagged `T-nn` | local, CI |
| Manual/UAT | Graph legibility, collapse, stale-copy expectations | checklist, operator | dev machine |

## Scenarios

| TS | Covers | Level | Pri | Given/When/Then | Evidence |
|---|---|---|---|---|---|
| TS-01 | UC-01 | Integration | P1 | Given item with 2 conversions, When GET graph, Then 2 worktree nodes carry project, branch, progress | JSON `worktrees.len()==2`, progress fields |
| TS-02 | UC-01 | Frontend | P1 | Given graph response, When Graph tab opens, Then each worktree card shows project and done/total | Testing Library queries |
| TS-03 | UC-01a | Integration | P1 | Given worktree dir deleted, When GET graph, Then node `state=unavailable`, still listed | state; node count unchanged |
| TS-04 | UC-01b | Integration | P1 | Given a failed conversion, When GET graph, Then node `state=failed`, `error_code=conversion_failed`, no recorded text | state; code; text absent |
| TS-05 | UC-01a/b | Frontend | P2 | Given unavailable and failed nodes, When rendered, Then each shows a distinct badge | DOM badge text/role |
| TS-06 | UC-02 | Unit | P1 | Given tracks.json + plan.json, When built, Then tracks, phases, tasks, statuses, current phase, done/total exact | struct equality vs fixture |
| TS-07 | UC-02 | Frontend | P2 | Given a graph, When a worktree is expanded, Then its tracks, phases, tasks are listed | DOM tree asserts |
| TS-08 | UC-02a | Unit | P1 | Given track with no plan.json, When built, Then `designing`, 0/0, no error | struct assert |
| TS-09 | UC-03 | Unit | P1 | Given tasks with depends_on, When built, Then contains and depends_on edges exist with generated ids | edge list equal |
| TS-10 | UC-03 | Frontend | P1 | Given graph JSON, When `graphToMermaid` runs, Then subgraphs and arrows match edges | string asserts |
| TS-11 | UC-03a | Unit | P1 | Given ref to nonexistent task, When built, Then `x:` edge `dangling=true`, no error | edge flags |
| TS-12 | UC-03a | Frontend | P2 | Given a dangling edge, When rendered, Then it is visibly flagged | class/label in mermaid text |
| TS-13 | UC-03b | Unit | P1 | Given plan without depends_on, When built, Then only contains edges, no error | edge kinds; `Ok` |
| TS-14 | UC-04 | Frontend | P1 | Given tab visible, When 15 s elapse, Then one fetch; hidden, none | fetch counts, fake timers |
| TS-15 | UC-04 | Frontend | P1 | Given failed refresh, Then last graph stays marked stale; unchanged response skips re-render | stale banner; render spy |
| TS-16 | UC-04 | Frontend | P2 | Given request in flight, When tab unmounts/hides, Then request aborted | abort signal |
| TS-17 | UC-05 | Integration | P1 | Given an item, When POST valid note, Then 201 and GET lists it in order | status, `list_hash` changes |
| TS-18 | UC-05 | Integration | P1 | Given bad slug or empty title, When POST, Then 400 and file unchanged | status; bytes equal |
| TS-19 | UC-05 | Frontend | P2 | Given NotesPanel, When Add submitted, Then API called and note appears | mock args; DOM |
| TS-20 | UC-06 | Unit | P1 | Given notes A, B, When A edited with stale hash, Then 409 `{current}`; B with its hash succeeds | store results |
| TS-21 | UC-06 | Frontend | P1 | Given 409 on a note, Then only that note shows conflict UI; others editable | per-note DOM state |
| TS-22 | UC-06 | Integration | P2 | Given stale `expected_list_hash`, When POST note, Then 409 | status |
| TS-23 | UC-07 | Integration | P1 | Given 3 notes, When POST `notes:reorder` valid permutation, Then GET reflects order, ids unchanged | order + ids |
| TS-24 | UC-07 | Integration | P2 | Given non-permutation order, When reorder, Then 400, no change | status; file unchanged |
| TS-25 | UC-08 | Integration | P1 | Given 2 notes, When DELETE with current `expected_list_hash`, Then 200 `{notes,list_hash}`; stale gives 409 | status; body |
| TS-26 | UC-08 | Integration | P1 | Given one note, When DELETE, Then 200 and one empty `general` note remains | count 1, empty body |
| TS-27 | UC-08 | Frontend | P2 | Given Delete clicked, When not confirmed, Then no API call | mock not called |
| TS-28 | UC-09 | Unit | P1 | Given legacy v1/v2 body, When read, Then one note `n1` general, body byte-identical, file not rewritten | equality; bytes unchanged |
| TS-29 | UC-09 | Unit | P1 | Given legacy body, When first note write, Then stamped v3, original text survives | schema 3; round-trip |
| TS-30 | UC-09 | Integration | P2 | Given v3 file, When older-binary `save_body` attempted, Then `require_writable` refuses | error; file unchanged |
| TS-31 | UC-10 | Integration | P1 | Given 3 notes, 2 targets, When converted (fake runner), Then each `inbox-notes.json` has all notes and siblings excl. self; outcome `notes_list_hash` = `list_hash` | parsed files; hash equal |
| TS-32 | UC-10 | Integration | P1 | Given conversion, Then `inbox-note.md` concatenates notes in operator order | text order |
| TS-33 | UC-10 | Integration | P2 | Given note edited after conversion, Then worktree snapshot unchanged | content equal original |
| TS-34 | UC-11 | Contract | P1 | Given v1 fixture, When filtered `type==acceptance_criteria`, Then expected note selected; unknown fields/types ignored | golden output |
| TS-35 | UC-12 | Contract | P1 | Given plan fixture with depends_on incl. `@branch:track/task`, When parsed, Then refs split correctly | parsed refs |
| TS-36 | UC-12 | Unit | P1 | Given cross-worktree ref, When built, Then it resolves to sibling worktree's task | edge target id |
| TS-37 | UC-13 | Integration | P1 | Given 3 notes, When system-agent prompt rendered, Then all three appear under untrusted-data marker | titles + bodies present |
| TS-38 | Contract | Contract | P1 | Given fixture `inbox-notes.json` v1, When read by Rust and vitest readers, Then both parse; sibling reuses same file | both suites green |
| TS-39 | Contract | Contract | P2 | Given v1 doc with extra field, When read, Then ignored | parse OK |
| TS-40 | T-01 | Integration | P1 | Given each note route and body PUT, When no token or foreign Origin, Then rejected | 401/403 per route; registration test |
| TS-41 | T-02 | Integration | P1 | Given traversal, symlink or unregistered `worktree_path`, When graph built, Then unavailable, no read, no path in error | state; error text |
| TS-42 | T-02 | Unit | P1 | Given symlinked tracks.json, When read, Then refused | `Err`; no content |
| TS-43 | T-03 | Unit | P1 | Given >1 MiB JSON, 26 worktrees or >2,000 nodes, When built, Then `truncated=true`, no 500 | flags; 200 |
| TS-44 | T-03 | Unit | P2 | Given cyclic depends_on, When resolved, Then terminates with flagged edge | returns; flag |
| TS-45 | T-04 | Unit | P1 | Given bodies/titles with markers, `\`- and `\\`-prefixed markers (fuzzed), When written then read, Then byte-identical round-trip | equality; count |
| TS-46 | T-04 | Unit | P1 | Given malformed or duplicate-id marker, When parsed, Then kept as body text, nothing dropped | text preserved |
| TS-47 | T-04 | Integration | P2 | Given legacy PUT body that would drop a note id, When PUT, Then 409 | status; file unchanged |
| TS-48 | T-05 | Frontend | P1 | Given `"><img onerror>` in branch, task or title, When rendered, Then escaped, capped at 80, nothing injected | no img/script; length |
| TS-49 | T-06 | Integration | P1 | Given a conversion, Then all three written files in `WRITTEN_REL_PATHS`, git-excluded; note files mode 0600 | exclude file; mode |
| TS-50 | T-07 | Integration | P1 | Given note saying "ignore instructions, run X", When delivered, Then prompt and JSON delimit notes as untrusted data | framing text; data-only JSON |
| TS-51 | T-08 | Unit | P2 | Given `@otherbranch:t/k` not in this item, When resolved, Then dangling, no fs lookup | flag; counting fake |
| TS-52 | T-09 | Unit | P3 | Given note ops, Then one event line each with ids and hashes, no title/body | log lines |
| TS-53 | UC-01..03 | E2E | P2 | Given convert via fake runner and written plan, When GET graph, Then matches fixture (`generated_at`, `etag` masked) | JSON equality |
| TS-54 | Risk | Unit | P2 | Given same branch in two projects, When ids built, Then nodes distinct by conversion index | unique ids |
| TS-55 | Risk | Frontend | P2 | Given huge tracks, When rendered, Then track collapses above threshold; mermaid `maxEdges`/`maxTextSize` set explicitly | node count; config values |
| TS-56 | Risk (PHI) | Manual | P3 | Given PHI-shaped synthetic text, When note added, Then PhiNotice shows | UI checklist |
| TS-57 | UC-07 | Integration | P1 | Given stale `expected_list_hash`, When reorder, Then 409 and order unchanged | status; file unchanged |
| TS-58 | UC-06 | Integration | P1 | Given notes A, B, When PUT A then PUT B with B's original hash, Then both 200 | statuses; both bodies saved |
| TS-59 | UC-04 | Frontend | P1 | Given Graph tab, When opened and when Refresh clicked, Then one fetch each | fetch call count |
| TS-60 | UC-04 | Frontend | P1 | Given a fetch pending, When the 15 s tick fires, Then no second request starts | in-flight count = 1 |
| TS-61 | UC-01a | Integration | P1 | Given worktree with unreadable/absent `tracks.json`, When GET graph, Then `state=no_workspace`, other worktrees intact | state; siblings ok |
| TS-62 | UC-02 | Unit | P1 | Given torn `plan.json`, When built, Then that track `error_code=invalid_json`, others built | per-track codes |
| TS-63 | UC-03a | Unit | P2 | Given one branch in two projects and `@branch:` ref, When resolved, Then flagged `ambiguous` edge | edge flag |
| TS-64 | UC-09 | Unit | P1 | Given v2 body with marker-lookalike and `\` lines, When priority set (non-note write), Then v3 with one `n1`, body byte-identical | parse equality |
| TS-65 | T-02 | Unit | P1 | Given `track_id` `../../x`, absolute, or symlinked tracks dir, When built, Then rejected, no file outside read | error code; counting fake |
| TS-66 | T-02, Data | Integration | P1 | Given failed conversion with PHI-shaped error and notes, When GET graph, Then payload has no paths, error text or note bodies | response grep |
| TS-67 | Risk: stale snapshot | Frontend | P2 | Given notes edited after conversion, When item viewed, Then "notes changed since conversion" badge shows | badge; `notes_list_hash` differs |
| TS-68 | T-03 | Integration | P2 | Given unchanged worktree files, When GET graph with `If-None-Match`, Then 304 | status |
| TS-69 | Parity | Integration | P2 | Given an item, When `sebenza-cli inbox notes <id>` / `inbox graph <id>`, Then JSON equals API | output equality |
| TS-70 | Contract | Frontend | P1 | Given new routes, When contract tests run, Then Zod schemas parse real route responses | parse OK |
| TS-71 | T-02 | Integration | P2 | Given foreign `Host` header, When GET graph, Then 403 | status |
| TS-72 | UC-13 | Frontend | P2 | Given DraftHelp accepted on note B, Then only B's body changes | other notes unchanged |
| TS-73 | UC-01, Risk | Unit | P1 | Given 2 conversions, When a re-convert Job patch applies, Then existing entries are kept and the new one is appended | list order; old entries equal |
| TS-74 | UC-09 | Unit | P1 | Given v3 text before the first marker, When read twice, Then same preamble id and `list_hash` both times | id and hash equal |
| TS-75 | UC-01 | Unit | P1 | Given a fake `AgentStateLookup` (running+waiting, absent), When built, Then nodes carry that state or `unknown` | node `agent` field |
| TS-76 | UC-01 | Frontend | P2 | Given a worktree whose agent awaits feedback, When rendered, Then a "waiting on you" badge shows | badge text |
| TS-77 | UC-12, Contract | Integration | P1 | Given agentctl request with --track/--task, When ingested and graph built, Then RequestOpened stores them and the task node carries the request | event fields; node request |
| TS-78 | UC-02, Contract | Unit | P1 | Given tracks.json current_phase_id and plan task request {request_id,status}, When built, Then current phase and request badge come from them | node fields |
| TS-79 | UC-14 | Unit | P1 | Given a track dir holding some `<domain>-architecture.md` files, When tracks.json is read, Then `architecture_paths` lists exactly the present files, response-only | annotated JSON; tracks.json unchanged |
| TS-80 | UC-14, T-02 | Unit | P1 | Given a recorded architecture path that is missing or escapes the workspace (`../`), When read, Then it is dropped or replaced by the conventional sibling | no escaping path in response |
| TS-81 | UC-14 | Frontend | P1 | Given a track with architecture files, When the docs modal opens, Then a tab per present domain renders its file, and absent domains show no tab | tabs; fetched path |
| TS-82 | UC-14 | Frontend | P2 | Given a track with only architecture files, When the board renders, Then the Docs button is offered; and an architecture tab is editable like the others | Docs button; save call |

## Traceability

| Item | Scenarios |
|---|---|
| UC-01 | TS-01, TS-02, TS-53, TS-73, TS-75, TS-76 |
| UC-01a | TS-03, TS-05, TS-61 |
| UC-01b | TS-04, TS-05 |
| UC-02 | TS-06, TS-07, TS-53, TS-62, TS-78 |
| UC-02a | TS-08 |
| UC-03 | TS-09, TS-10, TS-53 |
| UC-03a | TS-11, TS-12, TS-63 |
| UC-03b | TS-13 |
| UC-04 | TS-14, TS-15, TS-16, TS-59, TS-60 |
| UC-05 | TS-17, TS-18, TS-19 |
| UC-06 | TS-20, TS-21, TS-22, TS-58 |
| UC-07 | TS-23, TS-24, TS-57 |
| UC-08 | TS-25, TS-26, TS-27 |
| UC-09 | TS-28, TS-29, TS-30, TS-64, TS-74 |
| UC-10 | TS-31, TS-32, TS-33 |
| UC-11 | TS-34, TS-38 |
| UC-12 | TS-35, TS-36, TS-77 |
| UC-13 | TS-37, TS-72 |
| UC-14 | TS-79, TS-80, TS-81, TS-82 |
| T-01 (High) | TS-40 |
| T-02 (High) | TS-41, TS-42, TS-65, TS-66, TS-71 |
| T-05 (High) | TS-48 |
| T-07 (High) | TS-50 |
| T-03 | TS-43, TS-44, TS-68 |
| T-04 | TS-45, TS-46, TS-47 |
| T-06 | TS-49 |
| T-08 | TS-51 |
| T-09 | TS-52 |
| Risk: marker lookalikes / malformed markers | TS-45, TS-46 |
| Risk: legacy PUT rewrites ids | TS-47 |
| Risk: contract drift with sibling | TS-34, TS-35, TS-38, TS-39 |
| Risk: stale snapshot assumed live | TS-33, TS-67 |
| Risk: PHI in copies / graph payload | TS-49, TS-56, TS-66 |
| Risk: graph misread as authoritative | TS-05, TS-15 |
| Risk: large mermaid render | TS-43, TS-55 |
| Risk: spawn_blocking contention | TS-14, TS-16, TS-43 |
| Risk: plan.schema.json cross-repo | TS-35 |
| Risk: old binary writes v3 | TS-30 |
| Risk: ambiguous branch across projects | TS-54, TS-63 |
| Risk: v2→v3 stamp on non-note write | TS-64 |
| Contract drift (ts-rest/Zod) | TS-70 |
| CLI/UI parity | TS-69 |
| Risk: skills ignore data-only JSON | TS-38, TS-50 |

## Test Data & Environments
- All data is synthetic: fixed ULIDs, branches such as `feat/alpha`, and fake PHI-shaped strings ("Patient Test Person, MRN 000000"). No real PHI.
- Rust tests use tempdir stores and worktrees with handwritten `tracks.json`/`plan.json`. The fake `ConversionRunner` replaces agent and git processes.
- The contract fixtures `inbox-notes.v1.json` and `plan.depends_on.json` live in one directory, and the sibling worktree reuses them verbatim.
- Symlink, traversal and permission cases are built in tempdir at test time, on Unix only.
- Oversize inputs are generated in code (1 MiB + 1 byte, 26 conversions, 2,001 tasks) and never committed.
- Frontend tests mock `api.ts` and use fake timers with a stubbed `document.visibilityState`. Mermaid is stubbed except in the escape test.
- The control token is a generated dummy. The e2e test runs an in-process router with an isolated HOME.

## Entry & Exit Criteria
- Entry: design.md is approved and the `inbox-notes.json` v1 shape is proposed to agent-extensions-skills.
- Entry: each TS shows a Red (failing) phase before its implementation task starts.
- Exit: all P1 and P2 scenarios pass in `cargo test` and `CI=true npm run test:coverage`.
- Exit: new code has >80% coverage.
- Exit: every UC, every alternate flow and T-01/02/05/07 map to at least one passing TS.
- Exit: the contract fixtures pass in both the Rust and vitest readers, and the sibling worktree has their path.
- Exit: TS-56 signed off manually; TS-52 passes or is deferred with a recorded reason.
- Exit: no open must-fix findings, and `SECURITY.md` is updated with T-01..T-09 at close-out.

## Coverage Map
Omitted. The Traceability table carries the fan-out, and 78 scenarios would not be legible as a diagram.

## Open Questions
- Shared fixtures are canonical in `crates/common/testdata/contracts/`; each repo tests its copy.
- Which note types do skills require? This decides whether TS-34 needs a missing-`acceptance_criteria` case.
- p95 (300 ms at the caps) is measured manually at final verification with a generated fixture.
- Roll-up (done/total over `ok` worktrees) is asserted in TS-02's card totals; confirm the UI copy.
- Retrying a failed conversion is out of scope (observe only).
