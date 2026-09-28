# Spec — Inbox

## Overview

Sebenza has nowhere to put a thought that is not yet a worktree. The Inbox is
the first slice of the continuous agent platform: a global store of markdown
drafts at `~/.ai/sebenza/inbox/`, a split editor (markdown + mermaid) plus a
chat pane with the default builtin agent, and convert-into-worktrees across
projects. The agent helps write the design; convert uses today's worktree
lifecycle (not the later work-item LXC board). The draft survives, marked
promoted, and can convert again.

## Functional Requirements

**Store and model**

1. A draft is one file at `~/.ai/sebenza/inbox/<ulid>.md`: YAML frontmatter, then a markdown body.
2. `DraftId` is a server-issued ULID; `title` is frontmatter only and never forms part of a path.

3. Frontmatter holds `schema_version`, `title`, optional `project`, `status` (`Draft`/`Promoted`/`Dropped`), timestamps and `conversions[]`.
4. Every write to a draft file uses temp-file-then-atomic-rename.
5. Listing scans the directory and parses frontmatter — no index, no database; unparseable frontmatter degrades that draft to a raw view without blocking the rest.

**Editing and concurrency**

6. A save conflicts if and only if the on-disk `body_hash` differs from the one loaded; mtime never gates.
7. Every frontmatter write is a read-merge-write scoped to the keys its author owns.
8. The job owns `conversions[]` and `Draft → Promoted`; `Promoted` wins a race with `Dropped`.
9. Resolving a conflict by force-write rewrites the body only and never shortens `conversions[]`.
10. The editor autosaves on a debounce and renders markdown and mermaid beside the source.
11. Preview HTML is sanitized with DOMPurify and mermaid runs at `securityLevel: "strict"`; an unparseable block shows an inline error and never blocks saving.

**Agent collaboration**

12. The draft screen is split: markdown/mermaid editor and a chat pane with
    `workspace.defaultAgent`, switchable among builtins that declare
    `in_app_chat`. Custom agents without that capability are hidden from the
    pane (terminal-only, same as today).
13. Chat is bound to the draft, not a worktree and not an LXC. A per-draft
    scratch directory contains `draft.md` (the current body). The chat runtime
    is `AgentStreamManager` + session-log adapters — not `conversation_router`.
14. When the agent writes `draft.md`, the body is saved through the inbox
    store using the same `body_hash` rules as the human editor. Human autosave
    and agent writes share that protocol; a stale hash is a conflict, not a
    silent overwrite.
15. The chat agent process does **not** receive `SEBENZA_CONTROL_TOKEN` or
    `control.env`. Mutating `/api/inbox` still requires the bearer + origin
    guard. Full bubblewrap chat jail remains a later platform task.
16. `sebenza-cli inbox chat` sends a prompt to the draft's bound agent
    (parity with the pane).

**Organising**

17. The inbox supports list, search over title and body, rename, link/unlink a project, drop and delete.
18. Deleting a promoted draft warns it is the only record of its prompts, then proceeds on confirm.
19. `Dropped` drafts are hidden from the default list but reachable by filter.
20. A project link is a soft foreign key by absolute path; an unresolvable path renders as *unresolved*.

**Conversion**

21. A convert request carries 1–10 targets — project path, branch, base branch, agent id, prompt — capped in the Zod schema and re-checked server-side.
22. `POST /api/inbox/:id/convert` returns a ULID job id immediately and does not block on the fan-out.
23. Per target, in order: create the worktree unprompted, write the draft copy and exclude it via the worktree's own `$GIT_DIR/info/exclude`, then send that target's prompt via the existing prompt path.
24. The copy lands at `<worktree>/.ai/sebenza/inbox-note.md` and holds the whole draft body.
25. Each target's outcome merges into `conversions[]` as that target completes.
26. One target failing neither aborts nor rolls back the others; every outcome records its error.
27. `status` becomes `Promoted` only if at least one target reached `Created`.
28. The created worktree's `meta.json` records the originating `DraftId`.
29. Conversion prefers a sandboxed runtime and, when none is available, warns and proceeds.
30. Converting an already-promoted draft pre-fills the dialog from the previous wave's targets.

**Jobs and progress**

31. A per-job WebSocket streams per-target progress and `GET /api/inbox/jobs/:id` returns the same state for `sebenza-cli`; both gated on the ULID job id.
32. A job's durable state is the draft's frontmatter; after a restart progress is read from there.
33. Conversion emits `tracing` spans correlated by job id, carrying draft, project, branch and outcome.

**Security**

34. Mutating `/api/inbox` routes require a bearer secret from `~/.config/sebenza/control-token` **and** an `Origin`/`Referer` that is same-origin or absent; absent never waives the secret.
35. Every derived path is canonicalized, rejecting `..`, absolute paths, symlink escapes, null bytes and non-`.md`; branch names validate against git ref rules before any shell call.
36. Conversion writes a structured audit line per target: draft id, project, branch, agent, timestamp.

**Surfaces**

37. `/inbox` is a top-level route using the existing reserved-prefix dispatch, and `inbox` is reserved server-side.
38. Every new route is added to the ts-rest contract with a matching wrapper in `api.ts`.
39. `sebenza-cli inbox` covers list, show, new, edit, link, drop, delete, convert, job status, and chat.

## Non-Functional Requirements

- Listing stays responsive at hundreds of drafts; a directory scan per request suffices at that scale.
- New code carries >80% test coverage, per the project workflow.

## Acceptance Criteria

- [ ] A draft created in the dashboard appears as `~/.ai/sebenza/inbox/<ulid>.md` and opens unchanged in an external editor.
- [ ] Editing that file externally while it is open in the dashboard raises a conflict offering reload or force-write.
- [ ] A conversion job writing a back-link mid-edit does **not** raise a conflict in the open editor.
- [ ] Force-writing after a conflict leaves `conversions[]` intact.
- [ ] A draft containing `<img src=x onerror=alert(1)>` and a mermaid `click` directive renders inert.
- [ ] Converting to three targets across two projects creates three worktrees, each holding the whole draft at `.ai/sebenza/inbox-note.md`, ignored by git.
- [ ] When one of three targets fails, the other two exist and the failure is recorded against its target with an error.
- [ ] A conversion where every target fails leaves `status` unchanged, not `Promoted`.
- [ ] Killing the server mid-fan-out and restarting shows the already-completed targets' outcomes.
- [ ] A convert POST is rejected without the bearer secret (with or without `Origin`), and rejected with a cross-origin `Origin` even when the secret is valid.
- [ ] A draft id or branch name containing `../` is rejected and writes nothing outside the store or worktree.
- [ ] `sebenza-cli inbox convert` returns a job id and `sebenza-cli inbox job <id>` reports per-target progress.
- [ ] A project named `inbox` cannot shadow the `/inbox` route.
- [ ] The draft screen chats with the default builtin; an agent write to `draft.md` appears in the editor without dropping the human's unconflicting edits.
- [ ] The chat agent process environment does not contain `SEBENZA_CONTROL_TOKEN`.

## Out of Scope

- Work-item kanban, SQLite `platform.db`, Headscale cluster, `/ops`, S3 catalog, per-item LXC (those stay on `platform_20260917`).
- Full bubblewrap chat jail (platform). Token-stripping and scratch cwd are this track's boundary.
- Extending the `Origin` and secret check beyond `/api/inbox` to the rest of the control plane.
- Surfacing the originating draft on the Tracks board — `meta.json` records the id; rendering it is a Tracks change.
- Capping or archiving `conversions[]`, and reuniting an externally renamed draft with its history.
- Encryption at rest, and any blocking secret scan — the pre-conversion scan is advisory only.

## Open Questions

1. What secret-pattern set should the advisory pre-conversion scan use, and is it configurable?
2. What is the job's cancellation contract, and what happens to an in-flight `git worktree add` at shutdown?
3. Is the sandbox warning a one-time notice, or dismissible per project?
