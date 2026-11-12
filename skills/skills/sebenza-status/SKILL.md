---
name: sebenza-status
description: Shows progress across tracks from tracks.json - a single-project kanban overview, or a multi-project portfolio view, or a registry health check. Use when the user asks for status, progress, "where are we", a portfolio/all-projects view, or to validate the Sebenza registry.
metadata:
  version: "1.2"
---

# Sebenza Status Skill

You are the **Sebenza Reporter**. Your goal is to summarize project progress from `tracks.json`
(and each track's `plan.json`) into a clear, readable overview.

**Read first:** [`../../shared/conventions.md`](../../shared/conventions.md),
[`../../shared/framework.md`](../../shared/framework.md) (kanban statuses),
[`../../shared/directory-structure.md`](../../shared/directory-structure.md), and
[`../../shared/inbox.md`](../../shared/inbox.md) (inbox requests). Workspace root is `.ai/sebenza/`.
`<sebenza>` is this plugin's directory, two levels above this skill's base directory.

## 1. Mode detection

- **Visual board** — if the user asks for a kanban board / visual view, go to §6.
- **Multi-project / portfolio** — if the user passes `--all`, `--multi`, `--portfolio`, or asks for
  "all projects", go to §4.
- **Registry validation** — if the user passes `--validate`, go to §5.
- **Default (single project)** — §2–§3.

## 2. Single-project setup check

Verify `.ai/sebenza/index.md` and its linked core files exist, plus `.ai/sebenza/tracks.json` (the
sole track registry). If `tracks.json` is missing, halt and tell the user the workspace is not set
up (hand off to `sebenza-setup`).

Then reconcile `project.path` against the root you are working in before parsing anything else — see
[`../../shared/worktrees.md`](../../shared/worktrees.md).

## 3. Single-project overview

1. Parse `tracks.json`. For each track, read its `plan.json` (via `plan_path`) for phase/task
   status. `*_path` values are **workspace-relative** — resolve them against `.ai/sebenza/`, tolerating
   a legacy leading `.ai/sebenza/` (project-root form) by resolving from the root instead. Report any
   track carrying that legacy form as a fixable defect: its documents will not open in the dashboard
   until normalized to `./tracks/<track_id>/<file>`. A track with a `design_path` but no `plan_path` is
   still in the **design stage** — it has no plan to read. Count it separately and label it as such
   rather than treating it as a zero-task track.
2. Aggregate: count tracks by status (`backlog`/`doing`/`blocked`/`unblocked`/`done`); total
   phases and tasks; overall completion %; collect every `blocked_reason`. Per track, take the current
   phase from `current_phase_id` (fall back to the first phase that is not `done`), and note its
   `origin` (inbox item and worktree) when present.
2a. **Dependencies.** Run `python3 "<sebenza>/scripts/validate_plan.py" --workspace .ai/sebenza`.
   Report errors (cycles, self or dangling references) as defects to fix. Report warnings in one line
   each. For the active track, name any task that is waiting on an unfinished `depends_on`.
3. Present a report:

   ```
   ═══════════════════════════════════════════════════════════
   SEBENZA STATUS REPORT
   ═══════════════════════════════════════════════════════════
   Project: <project_name>

   KANBAN OVERVIEW
   ───────────────────────────────────────────────────────────
   Backlog: <n>   Doing: <n>   Blocked: <n>   Unblocked: <n>   Done: <n>
   In design (no plan yet): <n>

   CURRENT WORK
   ───────────────────────────────────────────────────────────
   Active Track: <track_id> — <description>   [inbox <inbox_item_id> · worktree <worktree>]
   Current Phase: <phase> (<status>)   Current Task: <task>
   Waiting on: <task> ← <depends_on ref> (<status>)

   PER TRACK
   ───────────────────────────────────────────────────────────
   <track_id>  <status>  <done>/<total> (<pct>%)  phase <current_phase_id>  requests <n> open, <n> unsent

   DEPENDENCIES
   ───────────────────────────────────────────────────────────
   validate_plan.py: ok | <n> errors, <n> warnings

   BLOCKERS
   ───────────────────────────────────────────────────────────
   - <track_id> / <phase>: <blocked_reason>   [request <id> open | unsent | NOT RAISED]

   INBOX
   ───────────────────────────────────────────────────────────
   Requests: <track_id>: <n> open, <n> unsent, <n> answered
   Answers waiting to be acted on: <n>   New comments: <n>
   (or: inbox unavailable | not linked to an inbox item)

   PROGRESS
   ───────────────────────────────────────────────────────────
   Total Tasks: <total>   Completed: <done> (<pct>%)
   [████████░░░░░░░░░░░░] <pct>%
   ═══════════════════════════════════════════════════════════
   ```

4. Prominently surface any `blocked` items with their reasons.
4a. **Inbox.** If `.ai/sebenza/inbox-origin.json` exists, gather the inbox state:
   - Run `python3 "<sebenza>/scripts/inbox.py" feedback --peek`. `--peek` leaves the comment
     cursor alone, because status only reports.
   - Run `python3 "<sebenza>/scripts/inbox.py" check-blocked`.
   - Count each track's cached `request` fields (`open`, `unsent`, `resolved`) on its tasks, phases
     and the track itself.

   Then report:
   - every blocked item with **no** request as NOT RAISED. It is invisible to the operator, so make
     raising it the first suggested action;
   - every `unsent` request (not yet delivered);
   - each `drift` entry.

   On exit 4 (or 5) print "inbox unavailable" instead of the counts. A worktree with no
   `inbox-origin.json` is "not linked", which is normal for an ordinary project.
5. Suggest the next action in plain language (not a slash command): resolve blockers, continue
   implementing, start the backlog, or (if all done) create a new track.

## 4. Multi-project portfolio

See [`../../shared/daemon.md`](../../shared/daemon.md) for the registry structure and lifecycle.

1. Read `~/.ai/sebenza/registry.json`. If missing / invalid / empty, announce and halt with guidance
   to set up and register projects.
2. For each registered project, validate `path` and `tracks_file` exist and parse the JSON; skip
   inaccessible/corrupt projects with a warning.
3. Aggregate across all accessible projects: totals by status, tasks, overall %, and every blocker
   with its project + track + reason.
4. Present a `PORTFOLIO KANBAN` summary with per-project breakdown and a prominent
   `⚠️ BLOCKERS REQUIRING ATTENTION` section. Group entries that share a `repo_path` under that
   repository, listing each worktree beneath it, so one repo with several worktrees reads as one project
   rather than several (see [`../../shared/worktrees.md`](../../shared/worktrees.md)).
5. Refresh each accessed project's `last_synced` in the registry. If any projects were skipped,
   offer to remove the stale entries.

## 5. Registry validation

1. Read `~/.ai/sebenza/registry.json`.
2. For each project, categorize: **valid** (all checks pass), **stale** (directory missing — including a
   worktree that has been removed), **corrupt** (`tracks.json` invalid), **orphaned** (`.ai/sebenza/`
   missing).
3. Present a validation report listing valid projects and issues found, then offer to remove stale
   entries (deregistration removes the entry only — it never deletes files).

## 6. Visual kanban board

A self-contained HTML board ships with this skill:

- [`resources/kanban.html`](./resources/kanban.html) — 5-column kanban (backlog / doing / blocked /
  unblocked / done) with per-track swimlanes, phase cards, progress bars, and blocked-item
  highlighting. Open it in a browser and use **Load tracks.json** to select this project's
  `.ai/sebenza/tracks.json`.
- [`resources/tracks-viewer.html`](./resources/tracks-viewer.html) — a lighter viewer that loads one
  or more `metadata.json` files from `.ai/sebenza/tracks/` directories.

Tell the user the file path to open. Multi-project loading from `~/.ai/sebenza/registry.json`
requires serving the board from a backend with filesystem access; for a single project, the
**Load tracks.json** file picker works directly from the browser.