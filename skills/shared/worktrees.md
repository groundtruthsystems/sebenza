# Git Worktrees & Path Reconciliation (shared reference)

> Shared reference for all `sebenza-*` skills. No frontmatter — loaded on demand.

`.ai/sebenza/tracks.json` records an **absolute** `project.path`, but a git worktree gives the same
repository more than one working root. A workspace created in the main checkout and then used from a
linked worktree therefore carries a path that points somewhere else. This document defines how to
detect that and what to do about it.

## Resolving the project root

```bash
worktree_root=$(git rev-parse --show-toplevel)                        # the root you are working in
git_dir=$(git rev-parse --path-format=absolute --git-dir)             # .../.git  or  .../.git/worktrees/<id>
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)   # always the main .git
```

- **`worktree_root`** is the project root. Use it instead of the current working directory, which may be
  any subdirectory. If the project is not a git repository (or `git` is unavailable), fall back to the
  current working directory and skip everything below.
- **Linked worktree** when `git_dir != common_dir`. In the main checkout the two are identical.
- **`repo_path`** — the main checkout root — is the parent directory of `common_dir`.
- **`worktree_id`** is the basename of `worktree_root`; the branch is
  `git rev-parse --abbrev-ref HEAD`.

## Reconciliation rule

Whenever a skill loads `.ai/sebenza/tracks.json`:

1. Compare `project.path` with `worktree_root`. **Equal → do nothing.** This is the normal case and it
   is what keeps the main checkout from ever being rewritten to a worktree path.
2. **Mismatch → rewrite it**, without asking (this is housekeeping, not a change to the user's work):
   - `project.path` = `worktree_root`
   - `project.worktree` = `worktree_id` and `project.repo_path` = `repo_path`, but only when this is a
     linked worktree; remove both fields when it is not.
   - Leave `project.name` and `project.registered_at` untouched — identity and history do not move with
     the path.
3. Report it in one line as part of the skill's normal output, e.g.
   *"Reconciled the workspace path to this worktree (`…/worktrees/my-feature`)."*

The rule is idempotent: the next run finds no mismatch and writes nothing.

`project.worktree` and `project.repo_path` are optional in
[`schemas/tracks.schema.json`](./schemas/tracks.schema.json), so a workspace that has never been used
from a worktree stays exactly as it is.

## Registry entries

A linked worktree registers with the daemon as **its own project entry** so the portfolio kanban can show
per-worktree progress. See [`daemon.md`](./daemon.md) for the full lifecycle.

- `name` = `<project_name>@<worktree_id>`
- `path` / `tracks_file` point **into the worktree**
- `worktree` and `repo_path` mirror the `tracks.json` fields
- Match on **`path`**, not `name`, when deciding whether to append or refresh — re-running from the same
  worktree must never create a duplicate.

## Worked example

Main checkout at `/home/user/projects/acme`, worktree at `/home/user/worktrees/acme-billing`:

```
worktree_root = /home/user/worktrees/acme-billing
git_dir       = /home/user/projects/acme/.git/worktrees/acme-billing
common_dir    = /home/user/projects/acme/.git       → is_worktree = true
repo_path     = /home/user/projects/acme
worktree_id   = acme-billing
```

`.ai/sebenza/tracks.json` in the worktree:

```json
{
  "project": {
    "name": "acme",
    "path": "/home/user/worktrees/acme-billing",
    "worktree": "acme-billing",
    "repo_path": "/home/user/projects/acme",
    "registered_at": "2026-07-26T10:00:00Z"
  }
}
```

`~/.ai/sebenza/registry.json` then holds two entries — `acme` and `acme@acme-billing` — which
`sebenza-status` groups under the one `repo_path`.

## Merge hygiene

`tracks.json` is a **tracked file**, so reconciliation shows up as a diff on the worktree's branch and can
travel into the main branch on merge. That is expected and self-correcting:

- `repo_path` preserves the canonical root, so nothing is lost.
- The main checkout reconciles `project.path` back to itself the next time a skill runs there.
- When a merge conflicts on `project.path`, resolve it in favour of **the checkout you are in** — never
  hand-edit it to a path you are not working from.

A worktree that has been removed leaves a stale registry entry; the stale-cleanup pass in
[`daemon.md`](./daemon.md) sweeps it.
