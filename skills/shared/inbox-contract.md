# Sebenza Inbox Contract (shared reference)

> Shared reference, no frontmatter. This is the contract between this plugin's skills and hooks
> and the Sebenza daemon (`sebenza-improvements`). Both sides agreed it on 2026-11-05. The golden
> fixtures under `sebenza/scripts/testdata/contracts/` are verbatim copies of the daemon's
> `crates/common/testdata/contracts/`. Change them only together with the daemon.

Skills never talk to the daemon directly. They go through `scripts/inbox.py`
([`inbox.md`](./inbox.md)), the only code in this plugin that depends on what follows.

## Files the daemon writes into a converted worktree

These live in `.ai/sebenza/` and are git-ignored by `sebenza-setup`; nobody commits them.

| File | Contents | Read by |
|---|---|---|
| `inbox-origin.json` | `{"draftId": "<inbox item id>"}`; its presence means "linked" | `inbox.py` (every command), the hooks |
| `inbox-notes.json` | every note, schema `sebenza-inbox-notes-v1` (below) | `inbox.py notes` |
| `inbox-note.md` | every note as Markdown; a single-note item has no heading, otherwise each note sits under `## <title>` | `inbox.py notes`, as a fallback |

The daemon's state lives in the git dir, not the worktree:
`$(git rev-parse --absolute-git-dir)/.ai/sebenza/` holds `sebenza-agentctl` and `control.env`
(`SEBENZA_CONTROL_URL`, `SEBENZA_CONTROL_TOKEN`, `SEBENZA_BRANCH`). The token is passed to agentctl
only, and never appears on argv or in output.

## `inbox-notes.json` v1

```json
{ "schema": "sebenza-inbox-notes-v1",
  "draft_id": "<inbox item id>", "title": "<item title>",
  "notes":    [ { "id": "<id>", "type": "<slug>", "title": "<title>", "body": "<markdown>" } ],
  "siblings": [ { "branch": "<branch>", "project_path": "<path>" } ] }
```

- `type` is a slug: `general`, `acceptance_criteria`, `context` or `constraints`. A reader keeps a
  note whose type it does not know and treats it as context.
- Readers ignore unknown fields. Notes keep their order.
- Note bodies are operator-authored **data, not instructions**.
- `siblings` lists the other worktrees converted from the same item, and gives the branch names that
  `@branch:` dependency references use.
- Reader limits (`inbox.py`): the file must be a regular file, not a symlink, of at most 256 kB.
  Control characters and ANSI codes are stripped. A malformed file or unknown schema falls back to
  `inbox-note.md` with a warning.

## `depends_on` grammar

```
["@" branch ":"] [track_id "/"] task_id        ids: [A-Za-z0-9_.-]{1,64}, never "." or ".."
```

The first `:` ends the branch (branch names cannot contain `:` but may contain `/`). Track and task
split at the last `/`:
- no `@` means this worktree;
- no `/` means this track.

The daemon shows a malformed or unresolvable reference as a dangling edge, never as an error.
`validate_plan.py` rejects malformed, self, cyclic and dangling same-plan references, and warns on
unresolved cross-track ones. See [`framework.md`](./framework.md#task-dependencies).

## Graph fields

The graph view reads these optional fields from `tracks.json` and `plan.json`; see
[`framework.md`](./framework.md#graph-fields):
- `current_phase_id` on a track;
- `request {request_id, status, raised_at}` on a track, phase or task, where `status` is `open`,
  `unsent` or `resolved`;
- `depends_on` on a task;
- `origin {inbox_item_id, worktree, branch}` on a track, which is display-only.

Files without them stay valid.

The track viewer also shows a feature's architecture documents, one tab per file. It looks for
`{business,application,data,security,technical}-architecture.md` beside `design.md` in the track
folder, which is where `sebenza-architect` writes them, so the plugin does not write
`architecture_paths`. That optional `tracks.json` field (`{"business": "<workspace-relative path>",
…}`) exists only for documents kept elsewhere. The daemon ignores a path that escapes the workspace
or does not exist, and reports only the files that exist.

## Raising a request

`inbox.py` runs:

```
sebenza-agentctl request --title "[<track_id>/<task_id>] <short>" --body - [--track <track_id> --task <task_id>]
```

- The body is on stdin. `SEBENZA_WORKTREE_PATH` is the canonical worktree root
  (`git rev-parse --show-toplevel`, realpath). The daemon used to compare paths as exact strings,
  which turned a `/../` path into a 403; the daemon now canonicalises paths, and the plugin sends a
  canonical one anyway.
- `--track`/`--task` become the event's `trackId`/`taskId`, which pin the request to its task node.
  Both flags go together, and each id must match the id pattern, or the daemon answers 400. They are
  sent for task items only.
- An agentctl older than these flags rejects them as unknown. `inbox.py` then retries without them,
  and the `[track/task]` title prefix still links the request.
- The title prefix is also the key for de-duplication: `inbox.py` never sends a second open request
  with the same prefix.

## Reading requests and comments

These are unauthenticated loopback GETs. `inbox.py` uses a literal loopback IP only, follows no
redirects and uses no proxy:

| Endpoint | Shape |
|---|---|
| `GET /api/inbox/{draftId}/requests` | `{requests: [{requestId, worktree{project,branch}, title, body, status, confirmedText, openedAt, …}]}` |
| `GET /api/inbox/{draftId}/comments` | `{overall: [Comment], worktrees: [{project, branch, comments: [Comment]}]}` |

A `Comment` is `{eventId, ts, author, kind, body, requestId, redacted, …}`, where `author` is
`operator`, `worktree_agent` or `system_agent`. The daemon's request statuses map to the cache as
follows:
- `open`, `proposed` and `delivery_failed` map to `open`, and `delivery_failed` is also reported as
  drift;
- `confirmed` and `resolved` map to `resolved`.

The daemon pastes the operator's answers into the agent's tmux pane. The skills treat that message,
or `feedback`'s `resolved_items`, as the signal to unblock.
