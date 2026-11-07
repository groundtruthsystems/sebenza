# Spec: Inbox graph view & multiple notes

Design: [design.md](./design.md) · Tests: [test-plan.md](./test-plan.md)

## Overview
An inbox item is a single long note today, and once it is converted the operator cannot see how its
worktrees are progressing. This track adds two things:

- **Typed, ordered notes** (e.g. *Acceptance Criteria*). All of them are delivered to worktree agents
  and to the system agent.
- **A read-only Graph tab.** It shows the item's worktrees with live agent state, their tracks, phases
  and tasks with progress, and a dependency graph built from a new optional `depends_on` field in
  `plan.json`.

Legacy items migrate without loss.

## Functional Requirements

**Notes**
- FR-01: An item exposes an ordered list of notes, each `{id, type, title, body, hash}`, plus a `list_hash`.
- FR-02: Operator can add a note with a slug type (`[a-z0-9_]{1,40}`) and a non-empty title, optionally after a given note.
- FR-03: Operator can edit a note's type, title or body. It is gated on that note's `hash`, and other notes' edits never conflict.
- FR-04: Operator can reorder notes via `POST /notes:reorder`, which needs an exact permutation and the current `list_hash`.
- FR-05: Operator can delete a note after confirming. Deleting the last note leaves one empty `general` note.
- FR-06: Add, delete and reorder are gated on `list_hash`. A stale token returns 409 with the current state.
- FR-07: A pre-v3 item reads as one note `n1` without being rewritten. Any write that stamps v3 materialises an escaped `n1` losslessly.
- FR-08: Marker-like lines in bodies and titles round-trip byte-identically, and a malformed marker stays as body text.
- FR-09: Legacy `PUT /body` returns 409 if it would drop an existing note id.
- FR-10: Conversion writes `inbox-note.md` (all notes in order) and `inbox-notes.json` v1 with `siblings`. Both are git-excluded and mode 0600.
- FR-11: Each conversion outcome records `notes_list_hash` and `note_types`. The UI shows "notes changed since conversion" when they differ.
- FR-12: The system-agent prompt includes all notes, framed as untrusted data. DraftHelp's proposal targets the selected note only.
- FR-13: `conversions[]` is append-only, and a re-conversion appends.

**Graph**
- FR-14: `GET /api/inbox/{id}/graph` returns worktrees, tracks, phases, tasks and edges.
- FR-15: Worktree state is one of `ok`, `unavailable`, `failed` or `no_workspace`. Recorded failures are never dropped.
- FR-16: A track without `plan.json` is `designing` with 0/0, and a torn or oversize file yields a per-track error code.
- FR-17: `depends_on` refs (`[@branch:][track/]task`) resolve only against this item's conversions. Unknown, ambiguous and cyclic refs become flagged edges.
- FR-18: Progress is done/total per track, worktree and item over `ok` worktrees, with the current phase shown. Failed and unavailable worktrees are counted separately.
- FR-19: The Graph tab fetches on open and on Refresh, and polls every 15 s while visible with one request in flight. It keeps the last good graph, marked stale.
- FR-20: The graph renders as Mermaid. A track collapses to a summary node above 150 nodes or 400 edges, and expands on click.
- FR-21: Worktree nodes show live agent status and a "waiting on you" badge.
- FR-22: The CLI gains read-only `inbox notes <id>` and `inbox graph <id>`, whose JSON equals the API.
- FR-24: Requests accept optional `trackId`/`taskId` (agentctl `--track`/`--task`); the graph reads `current_phase_id` and task `request` fields when present.
- FR-23: Contract fixtures live in `crates/common/testdata/contracts/` and are published to agent-extensions-skills.
- FR-25: The track viewer shows a tab for each `business|application|data|security|technical-architecture.md` present beside the track's docs, reported as response-only `architecture_paths`.

## Non-Functional Requirements
- NFR-01: Every note mutation passes the existing inbox guard (control token, Origin, Host).
- NFR-02: Graph reads are confined: `conversions[]` ∩ registry worktrees, id pattern `[A-Za-z0-9_.-]{1,64}`, `O_NOFOLLOW`, 1 MiB per file.
- NFR-03: The graph payload holds error codes only, never file paths inside worktrees, recorded errors or note text.
- NFR-04: Labels are escaped and capped at 80 characters, Mermaid runs in `strict` mode, output passes through DOMPurify, and `maxEdges`/`maxTextSize` are set explicitly.
- NFR-05: Caps are 25 worktrees, 2,000 nodes and 1 MiB, with `truncated: true` and never a 500. p95 is under 300 ms at the caps (measured manually).
- NFR-06: `etag` comes from file stats, so `If-None-Match` returns 304 without a parse.
- NFR-07: Each note operation logs an event with note id, op, hashes, request id and caller, and never logs title or body text.
- NFR-08: No new crates or npm dependencies. New code meets the Workflow's >80% coverage.

## Acceptance Criteria
- AC-01 (FR-01, FR-02): A valid note can be added and listed in order, and invalid input is rejected. TS-17, TS-18, TS-19
- AC-02 (FR-03, FR-06): Per-note conflicts are isolated, and edits to different notes both succeed. TS-20, TS-21, TS-22, TS-58
- AC-03 (FR-04): Reorder preserves ids, and a non-permutation or stale token is refused. TS-23, TS-24, TS-57
- AC-04 (FR-05): Deleting is confirmed and token-gated, and the last note is cleared rather than removed. TS-25, TS-26, TS-27
- AC-05 (FR-07, FR-08): Legacy and marker-heavy bodies survive reads, upgrades and fuzzed round trips. TS-28, TS-29, TS-30, TS-45, TS-46, TS-64, TS-74
- AC-06 (FR-09): A legacy PUT cannot silently collapse notes. TS-47
- AC-07 (FR-10, FR-11): Worktrees receive every note, the siblings list and the hash, and the files are excluded from git. TS-31, TS-32, TS-33, TS-49, TS-67
- AC-08 (FR-12): The system agent and DraftHelp see and edit notes correctly, and notes are framed as untrusted. TS-37, TS-50, TS-72
- AC-09 (FR-13): A re-conversion keeps the existing conversion entries. TS-73
- AC-10 (FR-14, FR-15, FR-18): The graph lists every worktree with state and progress. TS-01, TS-02, TS-03, TS-04, TS-05, TS-06, TS-07, TS-53, TS-61
- AC-11 (FR-16): Designing, torn or oversize tracks degrade per track. TS-08, TS-43, TS-62
- AC-12 (FR-17): Dependency edges resolve, flag bad refs and terminate on cycles. TS-09, TS-11, TS-13, TS-36, TS-44, TS-51, TS-54, TS-63
- AC-13 (FR-19): The refresh policy holds. TS-14, TS-15, TS-16, TS-59, TS-60
- AC-14 (FR-20, NFR-04): Rendering is correct, safe and collapses large tracks. TS-10, TS-12, TS-48, TS-55
- AC-15 (FR-21): Live agent state and the feedback badge are shown. TS-75, TS-76
- AC-16 (FR-22): CLI output matches the API. TS-69
- AC-17 (FR-23): Both repos parse the contract fixtures, and unknown fields are ignored. TS-34, TS-35, TS-38, TS-39, TS-70
- AC-18 (NFR-01, NFR-07): Unauthenticated mutations are refused, and audit lines carry no text. TS-40, TS-52
- AC-19 (NFR-02, NFR-03): Path escapes are refused and the payload is redacted. TS-41, TS-42, TS-65, TS-66, TS-71
- AC-20 (NFR-05, NFR-06): Caps and the 304 response hold. TS-43, TS-68
- AC-22 (FR-24): Requests link to task nodes and sibling-written graph fields are honoured. TS-77, TS-78
- AC-23 (FR-25): Present architecture docs get a tab, absent ones none, and no path escapes the workspace. TS-79, TS-80, TS-81, TS-82
- AC-21 (PHI): The PhiNotice and convert reminder are shown, using synthetic data only. TS-56

## Out of Scope
- Requests and comments, and agent inbox prioritisation (part 3, a separate bug track).
- Retrying a failed conversion from the graph.
- Purging worktree note copies when a note or item is deleted.
- PHI detection or excluding notes from delivery.
- CLI note mutation commands.
- Skill-side `depends_on` validation, which the agent-extensions-skills worktree owns.

## Open Questions
- Should skills also validate `depends_on` refs? This is for agent-extensions-skills to decide.
