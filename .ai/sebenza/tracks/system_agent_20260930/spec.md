# Spec: Sebenza 2.0 — System Agent & Inbox Collaboration

Refines [design.md](./design.md) · scenarios in [test-plan.md](./test-plan.md).

## Overview
Worktree agents cannot ask the operator for help, and the inbox has no ranked view. Each inbox item gets a resumable system agent (claude only). It drafts, triages worktree requests into priority plus a proposed resolution, and formulates architect-first conversion instructions. Items gain priority and grouped comments. The operator confirms every resolution (convention; T-01 accepted).

## Functional Requirements
**Priority & ordering**
- FR-01: Items carry `priority` `P0`–`P3` (default `P2`) and `priority_source` `agent|operator` in frontmatter `schema_version` 2.
- FR-02: The operator sets or clears an override (UI, CLI); while set, the agent never changes priority.
- FR-03: Inbox lists sort by priority, then created date descending.
- FR-04: Every priority change appends a `priority_changed {from, to, source}` event.

**Comments & requests**
- FR-05: Comments and requests are stored as immutable events in an append-only `<ULID>.events.jsonl` sidecar.
- FR-06: Operator, worktree agent (`agentctl comment`), and system agent comment; display groups per worktree plus an overall thread.
- FR-07: `sebenza-agentctl request` files a request against the origin item from `inbox-origin.json`, in that worktree's group.
- FR-08: A request whose claimed worktree is not in the item's `conversions[]` is refused.
- FR-09: Request state follows the design's state table: open, proposed, confirmed, resolved, delivery_failed.
- FR-10: Comments, requests, and proposals run the secret/PHI scan; hits are stored unchanged with a warning badge.
- FR-11: The operator can redact a comment via a `redacted` tombstone; comments render as sanitised markdown.

**System agent**
- FR-12: Config `systemAgent { enabled, agent, model, maxConcurrent=2, timeoutSecs=120, turnCap=40 }` is validated at startup.
- FR-13: Only `claude` is accepted as `systemAgent.agent`; other agents are a startup config error.
- FR-14: Each item has one session in `<ULID>.session.json`; each job resumes it headlessly.
- FR-15: A missing session, or a session past `turnCap`, is re-seeded from the item and its last 20 comments.
- FR-16: Jobs run serially per item, `maxConcurrent` globally, interactive before triage; timeouts kill the process group.
- FR-17: The agent child runs with an empty cwd, an env allowlist excluding `SEBENZA_CONTROL_TOKEN`, read-only tools, and never yolo.
- FR-18: Agent output must match the versioned `triage`, `draft_help`, or `convert` schema; anything else is a failed job.
- FR-19: Untrusted text is fenced as data in prompts; the triage digest holds at most 20 other items, titles and priority only.
- FR-20: Job status via `GET …/agent/jobs/:jobId` and WS `inbox.job`.
- FR-21: On restart, open requests with no proposal are re-enqueued once, deduped on `(request_id, job_kind, attempt)`.
- FR-22: `systemAgent.enabled=false` spawns no jobs; the inbox works without the agent.

**Triage & resolution**
- FR-23: A new request triggers triage, which sets priority (subject to FR-03) and proposes a resolution, advises, or does nothing.
- FR-24: Advice is a comment, never delivered; failed triage leaves the request open, flagged, and retryable.
- FR-25: The operator can confirm a proposal (optionally edited) or author a resolution directly when there is no proposal.
- FR-26: Confirm binds to the content hash of the displayed text; a mismatch returns 409.
- FR-27: Only a confirmed request reaches `ResolutionDelivery`, which pastes escape-stripped text into the origin worktree pane only.
- FR-28: Missing or mismatched pane → `delivery_failed`, redeliverable; reject with reason → open.

**Drafting & conversion**
- FR-29: Draft-help returns a proposed body, applied only through the existing hash-gated PUT.
- FR-30: Before convert, the agent drafts a `system_instruction` per target from the item and its comments; the dialog shows each, editable.
- FR-31: Each target launches architect-first, with the item description, operator prompt, and system instruction, when it is a feature and has a Sebenza workspace.
- FR-32: Otherwise a direct instruction; with the agent down, omit the system instruction but always include the item note; `conversions[]` records both fields.

**Parity, audit & data**
- FR-33: Every new route is in the ts-rest contract, `api.ts`, and `sebenza-cli inbox`.
- FR-34: Every proposal, priority change, confirm, reject, and delivery is audited metadata-only, with an unauthenticated `caller` marker.
- FR-35: Drop keeps sidecars; Delete removes sidecars then `.md`; startup sweeps orphans; files 0600; newer `schema_version` writes refused.
- FR-36: The editor and the comment box warn that PHI is prohibited and that content is sent to the model provider.

## Non-Functional Requirements
- NFR-01: Existing inbox, chat, and conversion behaviour and tests are unchanged.
- NFR-02: No test calls a real agent CLI or model provider.
- NFR-03: Coverage on new code is above 80% (vitest coverage-v8, `cargo-llvm-cov`).
- NFR-04: p95 triage completes in under 5 minutes at default limits.

## Acceptance Criteria
- AC-01 (FR-01–04): Priority override, ordering, and events — TS-05, TS-06, TS-07, TS-08, TS-24, TS-64.
- AC-02 (FR-05–08): Comments and requests group correctly, survive concurrency, refuse foreign worktrees — TS-09, TS-10, TS-11, TS-12, TS-13, TS-37.
- AC-03 (FR-09, FR-23–24): Triage proposes, advises, or flags without blocking — TS-14, TS-20, TS-21, TS-22, TS-23, TS-55, TS-63.
- AC-04 (FR-10–11, FR-36): Scan warns, redaction masks, rendering inert, PHI warning — TS-38, TS-39, TS-43, TS-49, TS-62, TS-66.
- AC-05 (FR-12–22): Sessions, queue, limits, timeout, kill switch, restart, isolation — TS-15, TS-16, TS-17, TS-18, TS-19, TS-35, TS-50, TS-51, TS-60, TS-65, TS-67.
- AC-06 (FR-25–28): Only confirmed text reaches the origin pane — TS-25, TS-26, TS-27, TS-28, TS-29, TS-30, TS-31, TS-33, TS-34, TS-36, TS-42, TS-61.
- AC-07 (FR-29): Draft-help never overwrites; conflicts merge — TS-01, TS-02, TS-03, TS-04.
- AC-08 (FR-30–32): Editable instructions; architect-first or fallback launch — TS-44, TS-45, TS-46, TS-47, TS-48, TS-57.
- AC-09 (FR-33–35): Parity, audit (incl. accepted T-01), retention, store hygiene — TS-32, TS-40, TS-41, TS-53, TS-54.
- AC-10 (NFR-01–04): End-to-end flows, no regressions, no real CLI calls — TS-52, TS-56, TS-58, TS-59.

## Out of Scope
- Scoped per-worktree agent tokens (T-01 follow-up track).
- grok, codex, and opencode as the system agent.
- Per-job models.
- Automatic secret redaction or blocking.
- Relaunching a removed worktree on delivery failure.
- Success-measure dashboards and a `P0` inflation indicator.
- Auto-purge of Dropped items.

## Open Questions
- None blocking; design OQ-1 (scoped token ownership) is tracked as a follow-up.
