# Sebenza Daemon Registry (shared reference)

> Shared reference for `sebenza-setup` and `sebenza-status`. No frontmatter — loaded on demand.

The daemon registry gives Sebenza a single, user-scoped index of every Sebenza-enabled project so
status can be aggregated into a multi-project (portfolio) kanban view.

## Location

```
~/.ai/sebenza/
└── registry.json     # central registry of all Sebenza-enabled projects
```

- **User-scoped & persistent** — one registry per user, survives sessions.
- Create the `~/.ai/sebenza/` directory and `registry.json` on first `sebenza-setup` if absent.

## Structure

Conforms to [`schemas/registry.schema.json`](./schemas/registry.schema.json) (`$schema:
"sebenza-registry-v1"`):

```json
{
  "$schema": "sebenza-registry-v1",
  "version": "1.0",
  "projects": [
    {
      "name": "my-project",
      "path": "/home/user/projects/my-project",
      "tracks_file": "/home/user/projects/my-project/.ai/sebenza/tracks.json",
      "registered_at": "2026-07-26T10:00:00Z",
      "last_synced": "2026-07-26T14:30:00Z"
    }
  ]
}
```

`tracks_file` is the **absolute** path to that project's `.ai/sebenza/tracks.json`. An empty registry
has `"projects": []`. Entries for a linked git worktree also carry the optional `worktree` and
`repo_path` fields.

## Lifecycle

- **Register** (`sebenza-setup`): create `~/.ai/sebenza/` + `registry.json` if missing, then append
  this project's entry if it is not already present, setting `registered_at` and `last_synced`.
- **Worktrees**: a linked git worktree registers as **its own entry**, so the portfolio can show
  per-worktree progress — `name` = `<project_name>@<worktree_id>`, with `path` / `tracks_file` pointing
  into the worktree and `worktree` / `repo_path` mirroring its `tracks.json`. Decide append-vs-refresh by
  matching on **`path`**, not `name`: re-running from the same worktree refreshes `last_synced` and never
  duplicates. See [`worktrees.md`](./worktrees.md).
- **Sync** (`sebenza-status` portfolio/validation; kanban board reads): refresh `last_synced` for
  each accessed project.
- **Deregister** (user request): match by name or path, confirm, remove the entry. Deregistration
  **never** deletes any project files — only the registry entry.
- **Stale cleanup** (during aggregation/validation): a project is stale/invalid if its `path` is
  gone, its `.ai/sebenza/` is missing (orphaned), or its `tracks.json` is missing/invalid (corrupt).
  Skip such projects with a warning and offer batch removal. A removed worktree falls under the same
  rule — its `path` no longer exists, so its entry is stale.

## Error handling

- **Corrupt registry** (invalid JSON): back up to `registry.json.bak`, create a fresh empty
  registry, and tell the user to re-register projects.
- **Inaccessible / missing `tracks.json`**: log a warning, skip the project in aggregation, continue
  with the rest.

## Notes

- Contains only local file paths — no secrets, no network, single-user design.
