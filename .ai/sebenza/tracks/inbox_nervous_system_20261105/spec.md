# Spec: Inbox nervous system (bug)

Origin: inbox item "Sebenza Task Graph", part 3. Highest urgency; lands before `inbox_graph_notes_20261105`.

## Overview
A worktree agent that gets stuck should raise a request, and the operator should see it and reply. Right
now three separate faults stop that, and a fourth gap stops operator comments reaching agents:

1. **Agent requests are refused.** The daemon compares a non-canonical `SEBENZA_WORKTREE_PATH` (which
   comes from the relative `worktreeRoot: ../worktrees`) with the canonical path recorded at conversion,
   as plain strings. Every `sebenza-agentctl request` and `comment` therefore gets a 403.
2. **Agents are never told the channel exists.**
3. **Nothing alerts the operator** when a request opens.
4. **Operator comments never reach the agent.**

After this fix the round trip works end to end: agent → inbox → operator alert → reply or comment → agent pane.

## Functional Requirements
- FR-01: Ingest matches a claimed worktree path to the recorded conversion path after canonicalising both, with a lexical fallback when the path is missing.
- FR-02: A path that canonicalises to a worktree outside this item's conversions is still refused with a 403.
- FR-03: New worktrees get a canonical `SEBENZA_WORKTREE_PATH` in their environment and `runtime.env`.
- FR-04: The conversion launch prompt includes an "Operator channel" section. It covers `sebenza-agentctl request` for blocking questions and `comment` for updates, and says to handle pasted operator messages first.
- FR-05: Draft list entries carry `openRequestCount`. Items with open requests sort above other items of the same priority.
- FR-06: `GET /api/inbox/summary` returns the total `openRequests`. The Inbox nav icon shows it as a badge, refreshed on the existing inbox poll.
- FR-07: Opening a request records an `inbox_request` notification on the owning project. It names the item, branch and title, and links to the item.
- FR-08: An operator comment on a worktree thread is pasted into that worktree's agent pane, prefixed `Operator comment on <item>:`, when `deliver` is true (the default).
- FR-09: The comment composer shows a "Send to agent" checkbox for worktree threads, ticked by default. Overall-thread comments are never delivered.
- FR-10: The delivery outcome is appended as a status event (`delivered` or `failed`), and the UI shows a failure with a retry option.

## Non-Functional Requirements
- NFR-01: Canonicalising the path does not widen trust. The branch must still match, and the bearer token is still required.
- NFR-02: Notification text carries the item title and request title only, never the request body (it may contain PHI).
- NFR-03: No new crates or npm dependencies. New code meets the Workflow's >80% coverage.

## Acceptance Criteria
- AC-01 (FR-01, FR-02, NFR-01): A non-canonical path from this item is accepted, and foreign or mismatched paths are refused. TS-01, TS-02, TS-03
- AC-02 (FR-03): A worktree created under a relative `worktreeRoot` has a canonical env path. TS-04
- AC-03 (FR-04): The launch prompt documents the operator channel in both architect-first and direct modes. TS-05
- AC-04 (FR-05, FR-06): Open requests show on the item, sort it up, and appear as the nav badge. TS-06, TS-07, TS-08
- AC-05 (FR-07, NFR-02): A notification is recorded without the request body. TS-09
- AC-06 (FR-08, FR-09, FR-10): Worktree comments reach the pane by default, opting out and overall-thread comments do not, and failures are shown. TS-10, TS-11, TS-12, TS-13
- AC-07 (all): A full round trip works: agentctl payload → request listed → notification → confirm → pane receives the reply. TS-14

## Out of Scope
- Injecting operator-channel notices into worktrees that are already running.
- Skill-side changes to the blocked protocol (owned by agent-extensions-skills, contract below).
- Browser desktop notifications.

## Contract (agree with agent-extensions-skills)
- **Blocked protocol:** if `.ai/sebenza/inbox-origin.json` exists, set `plan.json` to `blocked`, then run `"$(git rev-parse --git-dir)/.ai/sebenza/sebenza-agentctl" request --title "<short>" --body "<what is needed>"`, then halt.
- **Non-blocking updates:** use `sebenza-agentctl comment --body …`.
- **Replies:** they arrive in the pane prefixed `Operator` and are handled before continuing. On reply, set the plan status to `unblocked`.

## Open Questions
- None blocking.
