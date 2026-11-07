## Change Log

### 2026-11-06 — Inbox graph view and multiple notes (inbox_graph_notes_20261105)

Inbox items now hold several typed notes (e.g. Acceptance Criteria) stored as `<!-- sebenza:note -->` markers in the one draft file (schema v3, existing single-note drafts migrate losslessly), with hash-gated add/edit/reorder/delete, and conversion delivers every note to each worktree as `inbox-note.md` plus a `sebenza-inbox-notes-v1` `inbox-notes.json` listing sibling worktrees. A new Graph tab and `GET /api/inbox/{id}/graph` (also `sebenza-cli inbox graph`) show each converted worktree's state, live agent, tracks, phases, task progress and a Mermaid dependency graph built from plan.json `depends_on` refs (`[@branch:][track/]task`), flagging dangling, ambiguous and cyclic edges; it is fetched on open/Refresh and polled every 15 s while visible, with ETag/304. Graph reads are confined to daemon-registered worktrees and fixed file names with no symlinks, 1 MiB files and 25-worktree/2,000-task caps, and return fixed error codes rather than paths; p95 at the caps measured 7.5 ms. The track docs viewer also shows a tab per split `<domain>-architecture.md` the design stage writes, and the contract fixtures in `crates/common/testdata/contracts/` are shared with agent-extensions-skills.

### 2026-11-06 — Inbox nervous system: requests and comments reach operator and agents (inbox_nervous_system_20261105)

Every agent request and comment was rejected with 403 ForeignWorktree because a relative `worktreeRoot` gave agents a non-canonical `SEBENZA_WORKTREE_PATH` that ingest compared as a string; ingest now compares canonicalised paths (branch and token checks unchanged) and new worktrees get normalised paths. Agents' launch prompts now explain the request/comment channel, opened requests raise an Inbox nav badge, an open-request count and sort boost in the list and a project notification carrying titles only, never the body. Operator comments on a worktree thread are delivered into that agent's pane as a fenced block (opt-out per comment, with delivery state and Retry), and a full round-trip end-to-end test guards the path.

### 2026-09-28 — Inbox: markdown/mermaid drafts converted into worktrees (inbox_20260914)

Added a global inbox of markdown and mermaid drafts at `~/.ai/sebenza/inbox/<ULID>.md`, edited in a split dashboard editor or any external editor and optionally linked to a project. A draft converts into up to ten worktrees across registered projects, each with its own branch, base, agent and seed prompt and each receiving the draft as `.ai/sebenza/inbox-note.md`. Saves are body-hash gated (409 on conflict), mutating routes need the control token plus a same-origin check, and conversion warns on secrets and unsandboxed profiles before launching. The draft survives as Promoted with a conversions ledger and can convert again.

### 2026-08-22 — LXC / Apple worktree sandboxes (lxc_sandbox_20260819)

Worktrees can run in an unprivileged LXC container on Linux or an Apple Container machine on macOS 26 Apple Silicon, with the worktree mounted at the same absolute host path and panes running as the host user so files stay host-owned. Sandboxes fail closed if the uid/gid idmap cannot be applied, survive close, and are deleted on remove or merge. Docker is no longer launchable; existing `runtime: docker` profiles still load but return 422 on create.
