# Sebenza Framework (shared reference)

> Shared reference for all `sebenza-*` skills. This file has **no frontmatter**: it is
> never triggered as a skill on its own. Skills link to it and load it on demand.

Sebenza is a Context-Driven Development (CDD) framework. The philosophy is
**"Measure twice, code once."** Every feature follows a strict lifecycle:

```
Context  ->  Design  ->  Plan  ->  Implement
```

- **Context** — the project is initialized: product, tech stack, guidelines, workflow. (`sebenza-setup`)
- **Design** — a feature's use cases, diagrams, and business/application/technical/data/security
  architecture are defined and verified as an overview requirements document (`design.md`) plus one
  document per architecture domain (`<domain>-architecture.md`), and then —
  as the stage's **last** step — the use cases and that architecture are turned into a `test-plan.md`.
  The track is created and registered in `tracks.json` at the **start** of this stage, and both documents
  are written section by section as each is confirmed — so a design in progress is visible, reviewable in
  an editor, and resumable. **Mandatory for features.** (`sebenza-architect`)
- **Plan** — the design is refined into a detailed `spec.md` and a phased `plan.json`. Bugs, chores,
  and refactors skip Design and start here. (`sebenza-plan`)
- **Implement** — plan tasks are executed following the project workflow (TDD, commits, checkpoints). (`sebenza-implement`)

## Core concepts

- **Track** — a unit of work (feature, bug, chore, refactor) with its own `design.md` and five
  `<domain>-architecture.md` documents (features), `test-plan.md`, `spec.md`, and `plan.json`.
- **Design** — overview requirements document for a feature; produced before spec/plan.
- **Test Plan** — the scenarios that prove the feature works (`test-plan.md`): `TS-nn` scenarios traced
  from the `UC-nn` use cases and the architecture's risks, executed by the plan tasks that claim them.
- **Spec** — detailed requirements document (`spec.md`).
- **Plan** — phased task list in JSON (`plan.json`), validated against
  [`schemas/plan.schema.json`](./schemas/plan.schema.json).
- **Workflow** — the project's rules for the task lifecycle (TDD, coverage, commits, checkpoints).

## Kanban columns (status values)

Every track, phase, and task carries one of these statuses (a `status` field in `tracks.json` /
`plan.json`). They map 1:1 to kanban columns.

| Status | Column | Meaning |
|--------|--------|---------|
| `backlog` | Backlog | Not yet started |
| `doing` | Doing | Currently in progress |
| `blocked` | Blocked | Agent needs user feedback (has `blocked_reason`) |
| `unblocked` | Unblocked | User provided feedback, ready to resume |
| `done` | Done | Completed |

## Blocked protocol

Use this protocol whenever a skill is blocked, is unsure about a requirement, or is waiting on a
decision. [`inbox.md`](./inbox.md) explains when to raise a request and when to proceed. Never guess.

1. Set the task/phase status to `"blocked"` in `plan.json`.
2. Set `"blocked_reason"` describing exactly what input is needed.
3. Sync the blocked status + reason to `tracks.json`.
4. **Raise an inbox request** if the worktree is linked (`.ai/sebenza/inbox-origin.json` exists):
   `python3 "<sebenza>/scripts/inbox.py" request --item <track_id>/<task_id> --title "<short>" --body -`.
   The operator watches the inbox, not your terminal. On exit 3 (not linked), skip this step; on exit 4
   or 5, the request is kept as `unsent`; tell the user. See [`inbox.md`](./inbox.md#raising-one).
5. Announce in chat what is needed and that a request was raised.
6. **Switch to another** task that is not blocked and whose dependencies are done. If there is none,
   **HALT**. Never poll for the answer.

When the operator responds (a pasted answer, or `feedback` reporting it in `resolved_items`), or the
user answers in chat for an unlinked worktree:

1. Set status to `"unblocked"`, then resume by setting it back to `"doing"`.
2. Clear or update `"blocked_reason"`.

Only an operator resolution unblocks a request-backed item; never clear one yourself.

## Work priority

Every skill that picks up work takes it in this order:

1. **Operator answers and comments** — run `inbox.py feedback --retry` first, and act on what it returns.
2. **Unraised blockers** — any `blocked` item with no `request` gets one before anything else
   (`inbox.py check-blocked` lists them).
3. **Ready tasks** — in plan order, the first task that is not `done` or `blocked` and whose
   `depends_on` are all `done`.
4. **Halt** — if nothing is ready, say what you are waiting for and stop.

Never start a `blocked` task, and never start a task whose dependencies are unfinished.

## Task dependencies

A task may list the tasks it waits on in `depends_on` (`plan.json`). Each entry is a reference:

```
["@" branch ":"] [track_id "/"] task_id        ids: [A-Za-z0-9_.-]{1,64}, never "." or ".."
```

| Reference | Means |
|---|---|
| `phase-1-task-2` | a task in this plan |
| `other_track_20261105/phase-2-task-1` | a task in another track of this worktree |
| `@sebenza-improvements:inbox_graph_notes_20261105/phase-2-task-2` | a task in a sibling worktree of the same inbox item |

The first `:` ends the branch; track and task split at the last `/`. `sebenza-plan` fills
`depends_on` and checks it with `scripts/validate_plan.py`, which rejects self references, cycles and
same-plan references to tasks that do not exist. `sebenza-implement` never starts a task until every
dependency is `done`. A plan without `depends_on` runs in plan order, as it always has.

## Graph fields

The Sebenza graph view reads these optional fields. Skills keep them current; older files without
them stay valid.

| Field | Where | Written by |
|---|---|---|
| `origin {inbox_item_id, worktree, branch}` | track in `tracks.json` | `sebenza-architect` / `sebenza-plan` at registration, from `.ai/sebenza/inbox-origin.json` |
| `current_phase_id` | track in `tracks.json` | `sebenza-implement`, at every status change |
| `request {request_id, status, raised_at}` | track, phase or task | `scripts/inbox.py` only |
| `depends_on` | task in `plan.json` | `sebenza-plan` |

## Central daemon (multi-project)

Projects register with a central daemon registry at `~/.ai/sebenza/registry.json`
(schema: [`schemas/registry.schema.json`](./schemas/registry.schema.json)) so status can be
aggregated across projects. Deregistration removes the entry only — it never deletes project files.
See [`daemon.md`](./daemon.md) for the full registry lifecycle.

## Key workflow principles

1. **The plan is the source of truth** — all work is tracked in `plan.json`.
2. **Test-Driven Development** — write tests before implementing.
3. **High coverage** — target >80%.
4. **Commit after each task** — record the commit's short SHA in the task's `commit_sha` (`plan.json`)
   for traceability and revert.
5. **Phase checkpoints** — manual verification at each phase boundary; record the checkpoint's short
   SHA in the phase's `checkpoint_sha`.
6. **Every scenario is provisioned** — each `TS-nn` in `test-plan.md` is claimed by a task's
   `test_scenarios`. A scenario no task claims is a planning defect, not an oversight.

## Convention note

These skills target the **JSON / kanban** convention (`tracks.json`, `plan.json`). See
[`directory-structure.md`](./directory-structure.md) for the workspace layout and
[`conventions.md`](./conventions.md) for the operating rules every skill shares.