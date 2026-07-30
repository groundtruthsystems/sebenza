# Running Worktree Ticker — Specification

> Refines [`design.md`](./design.md). Where this spec and the design disagree, this spec wins —
> the design records the reasoning, this records the commitment.

## Overview

Add a full-width, read-only ticker above the dashboard workspace listing every active worktree and
every worktree awaiting an explicit agent permission request. Selecting a ticker item behaves
exactly like selecting that worktree in the sidebar. To satisfy CLI/UI parity, `sebenza-cli
worktree list` also gains the same signal in its output.

The feature is complete when the user never has to open a worktree merely to discover whether it is
blocked waiting on them — in either surface.

**Planning decisions fixed for this track:**

1. Only `permission_request` is ever set. `user_question` ships in the enum but is never produced.
2. A worktree in `starting` appears immediately; the predicate does not consult tmux liveness.
3. CLI parity is in scope, not deferred.

## Functional Requirements

### FR1 — Backend: `feedback_state` on runtime agent state

- FR1.1 Add a provider-neutral `feedback_state` to the runtime agent sub-state, with variants
  `none` (default), `permission_request`, `user_question`.
- FR1.2 `ProjectRuntime::apply_event` gains an `awaiting_permission` arm on `AgentStatusChanged`
  that sets `lifecycle = AgentLifecycle::AwaitingPermission` **and**
  `feedback_state = permission_request` in the **same arm**. The two fields must never be written
  from separate code paths.
- FR1.3 The `starting`, `running`, and `idle` arms of `AgentStatusChanged` set
  `feedback_state = none`.
- FR1.4 `AgentStopped` and `RuntimeError` unconditionally reset `feedback_state = none`, in
  addition to their existing `lifecycle`/`last_error` writes. A dead session must never advertise
  that it needs feedback.
- FR1.5 `feedback_state` is never set to `user_question` by any code path in this track.
- FR1.6 Every `feedback_state` transition emits one structured, **content-free** tracing record:
  timestamp, `worktree_id`, triggering event kind, old state, new state. It must not log prompt,
  tool, terminal, or branch content, and must never log the control token.
- FR1.7 Reconciliation continues not to touch agent sub-state. `feedback_state` is
  runtime-event-driven only.

### FR2 — Contract: carry the field to both clients

- FR2.1 Add `feedbackState` to the worktree snapshot serialization, adjacent to `status`.
- FR2.2 Add `feedbackState` to the ts-rest/Zod worktree snapshot schema in
  `frontend/src/lib/api-contract`. Type it as an enum of the three variants, not a bare string.
- FR2.3 Extend `mapWorktree` in `frontend/src/lib/api.ts` to carry `feedbackState` through, and add
  it to the `WorktreeInfo` type. **`mapWorktree` picks named fields rather than spreading, so
  omitting this step silently drops the field.**
- FR2.4 The field must be backward-compatible: an older client that does not know the field keeps
  working, and a missing field deserializes as `none`.

### FR3 — Frontend: derivation

- FR3.1 Add a pure module `deriveTickerItems` taking `WorktreeInfo[]` (plus the selected branch)
  and returning ticker items. It contains no JSX and no store access.
- FR3.2 Eligibility is exactly:
  `!archived && kind != main && creation == none &&
  (status in {starting, running, awaiting_permission} || feedbackState != none)`.
- FR3.3 Ordering: all feedback-needed items (`feedbackState != none`) before execution-only items;
  snapshot order preserved within each group. Ordering must be stable across refreshes.
- FR3.4 Each item's display name is the worktree label when present, otherwise its branch.
- FR3.5 Each item exposes: display name, branch (as identity), `status`, `feedbackState`, and
  whether it is the currently selected worktree.
- FR3.6 An item must never carry question text, prompts, tool input/output, terminal content,
  filesystem paths, session IDs, or tokens.

### FR4 — Frontend: the ticker component

- FR4.1 Add `ActiveWorktreeTicker`, a presentation-only component receiving derived items,
  `selectedBranch`, and the existing `handleSelectWorktree` as props. It must **not** subscribe to
  the Zustand store directly.
- FR4.2 Restructure the App root: wrap the existing `flex h-dvh` row in a new `flex-col` container
  with the ticker as its first child. The existing `<aside>`/`<main>` row must keep its current
  behaviour and fill the remaining height.
- FR4.3 Render nothing at all (no empty bar, no reserved height) when no worktree qualifies.
- FR4.4 Selecting an item calls `handleSelectWorktree` and nothing else. It must not resolve,
  acknowledge, approve, or reject the feedback state.
- FR4.5 Feedback-needed items are visually distinguished by colour **and** accompanying text — never
  colour alone.
- FR4.6 Overflow is handled by horizontal scrolling or a bounded overflow affordance. No marquee,
  no auto-scrolling, no animation that moves items without user input.
- FR4.7 The ticker is a labelled semantic navigation region containing buttons. Selected state,
  feedback kind, and display name must be available to assistive technology.
- FR4.8 Branch and label render as React text only. No `dangerouslySetInnerHTML`, and no
  constructing links, selectors, or DOM ids from a branch name.

### FR5 — CLI parity

- FR5.1 `sebenza-cli worktree list` renders each worktree's `status` in its human-readable output.
- FR5.2 It renders a feedback marker for any worktree with `feedbackState != none`, distinguishable
  in plain text without colour.
- FR5.3 Ordering in the CLI need not match the ticker, but feedback-needed worktrees must be
  identifiable at a glance without additional commands.
- FR5.4 No new CLI subcommand or operation is added.

## Non-Functional Requirements

- NFR1 **No new I/O.** No new route, no independent poll, no per-worktree conversation fetch, no
  browser persistence, no database or file write. The ticker reads the existing `/api/worktrees`
  snapshot on the existing foreground 5s poll.
- NFR2 **Derivation cost** is negligible on an already-polled array; no memoisation beyond what
  avoids re-deriving on unrelated renders.
- NFR3 **Eventual consistency** is acceptable — up to one poll interval of staleness. No new live
  channel.
- NFR4 **Coverage** >80% on new code, per workflow quality gates.
- NFR5 **Backward compatibility** — an existing `sebenza.yaml` and an older CLI binary must keep
  working against a server carrying the new field.
- NFR6 **Build order** — the frontend must be built before the backend, since the SPA is embedded
  via `rust-embed`.

## Acceptance Criteria

- AC1 A worktree whose agent emits `awaiting_permission` appears in the ticker, marked as needing
  feedback, and sorts above execution-only items.
- AC2 That same worktree appears with its status and a feedback marker in
  `sebenza-cli worktree list`.
- AC3 When the agent's permission is answered and a subsequent lifecycle event arrives, the
  feedback marker clears in both surfaces within one poll interval.
- AC4 An agent that stops or errors while awaiting permission leaves **no** feedback marker in
  either surface.
- AC5 Archived worktrees, the main checkout, worktrees in a creation phase, and plain
  idle/stopped/error worktrees never appear in the ticker.
- AC6 Selecting a ticker item selects that worktree, reveals it in filters if hidden, clears its
  unread marker, closes the sidebar on mobile, and resets the view to terminal — identical to
  selecting it in the sidebar.
- AC7 Selecting a ticker item does not change the worktree's `feedbackState`.
- AC8 With no qualifying worktree, the ticker occupies no vertical space.
- AC9 A `feedback_state` transition produces exactly one tracing record containing no prompt, tool,
  terminal, or branch content and no token.
- AC10 `cargo test`, `CI=true npm test`, `cargo fmt --check`, `cargo clippy`, and
  `npm run check` are all clean.
- AC11 Feedback state is distinguishable without relying on colour perception, in both the ticker
  and the CLI.

## Out of Scope

- Answering, approving, rejecting, or acknowledging feedback from the ticker or the CLI listing.
- Producing `user_question` — adapter work for observing free-text questions is a separate track.
- Per-worktree control-token scoping and control-route auth-outcome logging (deferred security
  items; see `design.md` threat table rows 1 and 2 for acceptance criteria).
- Reconstructing feedback state after a server restart. A worktree blocked mid-request across a
  restart will not reappear until a fresh event arrives — a known, accepted limitation.
- Admitting the main checkout to the ticker.
- Replacing the clear-on-next-lifecycle-event heuristic with an explicit correlation field on
  `RuntimeEvent`.
- Notifications, sound, badges, or any surface other than the ticker and the CLI listing.
- Multi-user or non-loopback hardening. This feature does not change the server's exposure model.
