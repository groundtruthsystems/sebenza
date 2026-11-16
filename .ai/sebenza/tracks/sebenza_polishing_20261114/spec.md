# Spec — Sebenza Polishing

Refines [design.md](./design.md) and the approved [solution handover](../../solution-handover.md). Scenarios: [test-plan.md](./test-plan.md).

## Overview
Adds a request lifecycle (dismiss, archive, filters, summaries, history, reliable dismissal notice), handover Reject, a full-screen reader with layered Esc, and a read-only zero-tool handover chat. Requests, reject and full screen ship first; chat stays behind `chat.enabled` until the probe passes and AC-07a/b is approved.

## Functional Requirements
- **FR-01** Dismiss moves an open, proposed or delivery_failed request to `dismissed`; any other status returns 409.
- **FR-02** Dismiss appends one durable line (`request_dismissed` + `notice_enqueued`, key `request_id:dismissal`) under the per-item lock.
- **FR-03** Dismissing a delivery_failed request cancels its redelivery; existing confirm, reject, redeliver and retry-triage return 409 on dismissed.
- **FR-04** Archive is allowed only from confirmed or resolved, sets `archived_at`/`archived_by`, keeps the status; otherwise 409, button disabled.
- **FR-05** `GET …/requests?filter=unanswered|answered|dismissed|all` returns only mapped statuses, with archived flag, notice state, summary and timestamps.
- **FR-06** Summaries come from a deterministic title-only template (≤140 chars) and show for archived and dismissed requests.
- **FR-07** `GET …/requests/{rid}/history` returns the request, its answers and ordered events (`kind, from, to, actor, at`).
- **FR-08** A dispatcher rebuilds pending notices at startup, retries with exponential backoff to `notify.max_attempts`, ending `sent` or `failed`.
- **FR-09** Every notice attempt is a `notice_attempt` event; manual Retry on a failed notice appends one; Retry on pending returns 409.
- **FR-10** Startup backfill appends one `migrated` event (actor `system:migration`, current time) per request lacking one.
- **FR-11** Handover review controls are one shared component used by the normal and full-screen views.
- **FR-12** `POST /api/inbox/{id}/handover/{project}/reject {reason, comment}` rejects a Drafted or Deferred handover; else 409; empty comment 400.
- **FR-13** `wrong_project` reject also appends `design_revision_requested{project, reason}` moving the solution to Revising, in one isolated branch.
- **FR-14** A rejected handover can never be approved or converted (409); other projects' handovers are unchanged.
- **FR-15** Full screen is a mode of the review route; review input and scroll position persist per handover across modes.
- **FR-16** Esc closes the innermost open layer: dialog, then chat, then full screen; a visible exit control is always present.
- **FR-17** A capability probe at start and on `claude` version change is the only path that sets `chat.enabled` true.
- **FR-18** A runtime guard aborts a chat turn on any tool event, discards output, sets `chat.enabled` false and logs it.
- **FR-19** Chat API is `GET …/handover/{project}/chat` and `POST …/chat/messages {text}` only; 503 `chat unavailable` when disabled.
- **FR-20** Each turn sends the fixed role, the full handover fenced as untrusted data, and the transcript; oversized handover refused, full transcript 413.
- **FR-21** Chat output renders as plain text; one shared thread per handover records each message's author.
- **FR-22** Every new capability has a matching `sebenza-cli` command and ts-rest contract entry.

## Non-Functional Requirements
- **NFR-01** Chat runs with zero tools: empty allow list, all built-ins denied, `--tools ""`, strict MCP with no config, no hooks, no tokens, empty cwd, timeout, provider pinned to `claude`.
- **NFR-02** No handover or chat content goes to any new external service.
- **NFR-03** Nothing is deleted or rewritten; the log is append-only and a torn final line is ignored.
- **NFR-04** Every new control is a native button with an accessible name, visible focus and keyboard operation.
- **NFR-05** All new routes require an operator credential; the actor comes from the credential, never the body.
- **NFR-06** Coverage above 80% for each touched module; existing role-agent argv stays byte-identical.

## Acceptance Criteria
- **AC-01** Full screen button opens the reader (FR-15; note "Acceptance Criteria (draft)" 1) — TS-01.
- **AC-02** Full-screen view has the same working controls, incl. Reject (FR-11, FR-12; note 2) — TS-05, TS-06.
- **AC-03** Leave full screen by visible control or layered Esc (FR-16; note 3) — TS-02, TS-03.
- **AC-04** Mode toggles keep review input and scroll (FR-15) — TS-04.
- **AC-05** Chat has the handover as context (FR-19, FR-20; note 4) — TS-10, TS-13.
- **AC-06** Answers are about this handover; no cross-handover chat; access re-checked (FR-21, NFR-05; note 5) — TS-11, TS-16, TS-24.
- **AC-07a** Chat cannot change outcomes or call tools (FR-17–19, NFR-01) — TS-12, TS-14, TS-19–21, TS-49, TS-50.
- **AC-07b** Live 60×3 adversarial run passes all hard and ≥95% soft criteria; harness in CI (FR-20) — TS-17, TS-18.
- **AC-08** Dismiss shows the item as Dismissed, notice delivered (FR-01–03, FR-08–09; note 6) — TS-25–33, TS-51, TS-55–57.
- **AC-09** Archive only confirmed/resolved items (FR-04; note 7) — TS-34, TS-35, TS-54.
- **AC-10** Filters show only mapped statuses (FR-05; note 8) — TS-36.
- **AC-11** Summaries visible, including archived (FR-06; note 9) — TS-37.
- **AC-12** Dismissed/archived keep full history and stay visible (FR-07, FR-10, NFR-03; note 10) — TS-38, TS-39, TS-42, TS-58, TS-60.
- **AC-13** Reject a wrong-project handover (FR-12–14) — TS-06–09, TS-52, TS-59.
- **AC-14** Security and robustness: auth, actor, origin, secrets, DoS, store safety (NFR-05) — TS-15, TS-22, TS-23, TS-40, TS-41, TS-43, TS-44, TS-46, TS-53.
- **AC-15** CLI/UI parity, UI chat status, end-to-end UAT (FR-22) — TS-45, TS-47, TS-48.

## Out of Scope
- Restore, unarchive and an Archived filter.
- Delete endpoints of any kind.
- Automatic removal of a project from the design after `wrong_project`.
- Chat tools, MCP servers or any fallback tool policy; chat with other providers.

## Open Questions
- Handover §6: AC-07a/b amendment approval (blocks chat release).
- Handover §6: does `wrong_project` mean `needs_revision` (Revising)? Built behind one branch.
- Handover §6: is `delivery_failed` under Unanswered?
- Handover §6: is one shared chat thread per handover acceptable?
- Who owns the design revision and re-review after a `wrong_project` reject?
- Does the pinned `claude` CLI support `--tools ""` and report tools in `init`? (Phase 4 entry check.)
