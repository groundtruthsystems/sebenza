# Spec — Continuous Agent Platform

## Overview

Sebenza grows from a single-node worktree orchestrator into a continuous agent
platform. The operator talks to a default agent in home chat (chat jail, no
work-item LXC). Files land in `/files`. Selected files seed a planned work
item (chat + canvas). Staffing attaches agents and projects; Todo dispatches
one worker, which gives the item one LXC and starts agents. Done is a computed
gate. A Headscale cluster lets a laptop worker pull work defined on a server.

This spec refines `.ai/sebenza/tracks/platform_20260917/design.md`. Business
rules BR1–BR16 apply.

**First ship:** `inbox_20260914` — draft + agent chat + convert to worktrees
via today's lifecycle. Do not start this track's SQLite/`/board`/cluster
phases until that inbox ships. Inbox convert is the v1 path onto projects;
work-item Todo + per-item LXC remains later phases here.

**Closed from design open questions**

| Question | Spec decision |
|---|---|
| Dispatch seed | Copy selected artifacts into each assignment worktree plus `SEBENZA_ITEM.md` pointing at the work-item id and catalog keys. Do not write `tracks.json`. |
| Excalidraw | npm embed of `@excalidraw/excalidraw`. Scene JSON is the file the planning agent reads/writes. |
| Operator session | All-in-one on loopback: chat jail is the agent boundary; operator UI stays as today. Cluster / non-loopback: operator password (HTTP basic or session cookie) **or** Tailscale identity headers if present. Worker join token is never an operator session. |
| SQLite | `rusqlite` with WAL, used from `spawn_blocking` (same pattern as git/lxc). `PRAGMA user_version` plus a `meta` table. IDs are UUIDs (`random_uuid()`), not ULID. |

## Functional Requirements

### Surfaces and routing

1. SPA routes `/chat`, `/inbox`, `/board`, `/ops`, `/files` are first-class.
   Reserve those prefixes in `RESERVED_PROJECT_PREFIXES` alongside `api`,
   `ws`, `assets`, `registry`.
2. Hub APIs live under `/api/...` with `createApi("")`. Do not treat the first
   path segment of `/chat` as a project prefix.
3. Existing `/{prefix}/...` worktree UI remains for ad-hoc worktrees and as
   execution drill-down (BR15). In cluster mode, control **proxies** PTY,
   conversation, and tracks over the worker WebSocket; the worker does not
   bind the tailnet.
4. Every new route is on the ts-rest contract and in `frontend/src/lib/api.ts`.
5. `sebenza-cli` gains `chat`, `item`, `ask`, `cluster`, `files`, `ops`
   subcommands covering the same operations (CLI/UI parity).

### Roles and process identity

6. `SEBENZA_ROLE` is `all-in-one` (default), `control`, or `worker`.
7. `all-in-one`: loopback bind by default; local worker loop against
   `ws://127.0.0.1:5111/api/cluster/worker`.
8. `control`: SPA + operator APIs + scheduler + catalog + home/planning chat
   jail. No item LXC. Bind loopback or tailnet IP only.
9. `worker`: `127.0.0.1` only. Runtime-event ingest, ItemRuntime,
   WorktreeLifecycle, tmux. No SPA, no tailnet bind.
10. `sebenza-server` always runs as the **operator user unit**
    (`loginctl enable-linger` on a headless Linux server). Headscale, if
    spawned, is a system unit. Unit order: headscale → tailscaled → sebenza-server.

### Platform data

11. Control-plane state is SQLite at `~/.ai/sebenza/platform.db` (mode 0600,
    WAL). Schema includes `schema_version`.
12. Entities match the design class/logical model: `WorkItem`, `Assignment`,
    `ChatThread`, `HumanAsk`, `ClusterNode`, `Artifact`, `TrackRef`
    (projection only).
13. `WorkItem.status` is only written by the state machine. `PATCH` cannot
    set `status`. Allowed operator transitions: Draft→Backlog (via staff +
    promote), Backlog→Todo, cancel/abandon per BR14, Failed→Todo (retry).
    Agents (via worker) write Todo→Doing. Blocking asks write Doing↔Blocked.
    `done` is written only when `can_complete` is true at that instant.
14. Artifact blobs live under `~/.ai/sebenza/objects/` by default (filesystem
    backend, atomic rename + fsync). Optional `artifactStore.endpoint` for
    S3. Keys only `chat/{threadId}/…`, `items/{id}/…`, `uploads/{id}/…`;
    reject `..`. Catalog row `state` is `pending|ready|failed`.
15. Snapshot-on-seed (BR1b): copy to a new key, same `sha256`, `parentKey`
    set, `role=seed-copy`. Originals remain seedable.

### Chat (home)

16. `/chat` talks to `workspace.defaultAgent` from the **platform** catalog
    (`~/.ai/sebenza/platform.yaml` custom agents + builtins). It never reads
    a project's `sebenza.yaml`.
17. Agent switch starts a new thread, or forks if `capabilities.fork`.
    Custom agents without `in_app_chat` get a jailed tmux pane.
18. Chat is a new runtime: `AgentStreamManager` + session-log adapters. It
    does **not** call `conversation_router` or
    `/{prefix}/api/agents/worktrees/...`.
19. `ChatThread` is an index (`id`, `kind`, `agentId`, `provider`,
    `sessionId`, `transcriptKey`). Message bytes live in the agent log plus
    a transcript artifact uploaded on turn end.
20. Home chat runs in a **chat jail** (bubblewrap on Linux; documented
    fallback on macOS control). The jail has: RW scratch cwd; RO/needed
    agent CLI + cred paths so the model authenticates (BR13, disclosed);
    **no** `platform.db`, `cluster.json`, S3 master keys, `control-token`,
    other worktrees, or loopback to operator routes. A dedicated submit
    listener, if any, is not the operator API.
21. Upload from scratch: regular files only, `O_NOFOLLOW`, realpath inside
    the scratch tree, size cap, never `control.env` / token files.
    `origin=chat`.
22. `POST /api/chat/threads/:id/work-item` snapshots that thread's ready
    artifacts into a new Draft (does not bind the home thread).

### Files and inbox

23. `/files` lists the artifact catalog (filter by origin, thread, work
    item). Multi-select → create Draft (`POST /api/work-items` with
    `artifactKeys[]`).
24. `/inbox` is planning: default-agent chat (`kind=planning`, same jail)
    plus canvas. A Draft is not work until staffed and promoted (BR1).
25. Canvas layers: markdown, mermaid UML, Excalidraw JSON. Versioned as
    `items/{id}/canvas/{layer}/r{rev}`. `WorkItem.canvas` points at latest.
    Dispatch copies those keys to `items/{id}/dispatch/{rev}/`.
26. Agent-authored canvas is untrusted: DOMPurify, mermaid
    `securityLevel: "strict"`, Excalidraw JSON not executed as HTML, CSP
    on the SPA. Planning-workspace sync is debounce + rev.
27. Markdown / mermaid / shapes populate canvas layers when seeding; other
    kinds attach. Creating from files **copies** (BR1b).
28. Inbox convert-to-worktrees (`inbox_20260914`) **is** the v1 start
    signal onto projects. This track's later Todo → ItemRuntime path
    must not replace that until it ships; do not implement a second
    convert API here.

### Staffing, dispatch, LXC

29. Staffing is 1..N `(projectPrefix, agentId, profileName, baseBranch,
    branch)` plus placement (`device` / `role: laptop|server` / `any`).
30. Staffing is refused unless one live worker has every clone, every
    agent binary, the runtime, remaining veth quota, and all assignments
    share one runtime + image (BR10). Error body names the reason.
31. Todo: operator only. Scheduler nominates `assignedNodeId` in SQLite.
    Worker claims on its existing WS (`offer`/`claim`). Never dial the
    worker.
32. Dispatch sequence: create **all** host worktrees first; then one
    instance `sebenza-item-<work_item_id>` with every repo/worktree/cred
    mount; then `lxc-attach` agents. Failed assignment N rolls back that
    worktree only.
33. **ItemRuntime** owns create/destroy/orphan-reconcile of the item
    instance. **WorktreeLifecycle** is today's git/meta/tmux/hooks with
    `SandboxLaunchSpec.instance: Option<&str>` meaning attach, do not
    launch. Close/remove of one worktree must not destroy the item LXC.
    Destroy on Done / Cancelled / Abandoned (configurable keep).
34. Ad-hoc `/{prefix}` create keeps per-branch LXC naming; both count
    against `lxc-usernet` quota. Advertised remaining veths include
    kept + leaked + running.
35. `SEBENZA_CONTROL_URL` inside the LXC is the **worker loopback** event
    ingest. Worker relays `event` on the control WS. Item LXCs stay off
    the tailnet.
36. Per-item (or per-pane) submit token bound to `workItemId` /
    `worktreeId`; `control.env` mode 0600. Home/planning do not receive
    an execution submit token.
37. After claim, copy seed artifacts into each worktree and write
    `SEBENZA_ITEM.md`. Execution agents create Sebenza tracks (BR11).
    Worker watches `.ai/sebenza/tracks.json` and emits `event.tracks`.
    `trackId` is recorded when it first appears in that worktree after
    claim.
38. Apple Container: disclose `$HOME` RW; per-item mount isolation is
    Linux LXC only. Advertise `runtime=apple` only on macOS 26+ Apple
    Silicon.
39. Heartbeat 10s; dead after 3 missed (30s); reconnect 15m while Doing
    if the same LXC is up; then Failed. Missing worker on Todo stays
    Todo + in-app alert (BR12).
40. Port allocation for published services is per worker host, not per
    project. Collision fails staffing or remaps.

### Done, cancel, asks

41. `can_complete` (BR6): every blocking ask is `resolved|cancelled`;
    every assignment has ≥1 `trackId`; each of those is `done` in **that
    assignment worktree's** `.ai/sebenza/tracks.json`. Unreachable file
    → false. Zero tracks cannot complete. No force-complete (BR9).
42. Cancel/withdraw (BR14, UC6a): confirm. Backlog/Todo → Abandoned
    (no LXC). Doing/Blocked → Cancelled (tear down LXC, cancel open
    asks, leave tracks). Failed → Abandoned (stop retry) or Todo
    (retry: clear `assignedNodeId`/`runtime`, keep last-known
    worktree/track ids, rebind on next claim). Cancelled ≠ Done.
43. Blocking asks: `kind=question|ask_user`. Observational
    `kind=permission` alerts only, does not block Done, auto-closes
    when worker reports feedback `none`. Timeout does not auto-resolve
    (BR5a).
44. Produce asks server-side (stream parser or agentctl), not by
    requiring the SPA to stay open. Resolve: operator session → worker
    WS → existing send/PTY. Hub SSE `/api/asks/stream`. Until `/ops`
    ships, the work-item card shows blockers (asks, missing tracks,
    placement reason).
45. Untrusted-plugin scan **blocks** dispatch if dirty (phase 4+).

### Cluster, artifacts, ops

46. `sebenza-cli cluster init`: adopt `HEADSCALE_URL` if set, else spawn
    Headscale. Single-use preauth keys tagged `tag:sebenza-operator` or
    `tag:sebenza-worker`. Install Headscale ACL so only operator tags
    reach `:5111`; workers reach only the worker WS. A pre-existing
    tailnet is not the operator boundary.
47. `cluster join`: install tailscale if missing, `tailscale up`, start
    role=worker, WS to control. Join token hashed, bound to Tailscale
    node id on first `hello`.
48. Bind allowlist: loopback or tailnet IP. Other interfaces require
    `SEBENZA_EXPOSE_PUBLIC=1` and a warning.
49. Worker WS messages: `hello`, `heartbeat`, `claim`/`offer`, `status`,
    `event`, `pty`, `tracks.get`, `conversation.send`, `ask.resolve`,
    `interrupt`. Not `artifact.put`. Blobs: `POST /api/artifacts`
    (join-token auth) for FS; pre-signed PUT (prefix-scoped, TTL
    minutes, max size) when S3 is configured. Workers never hold the
    S3 master key.
50. `/ops` shows nodes, heartbeats, LXC inventory (both naming
    schemes), work items, asks, scheduler reasons, artifact usage,
    event log. Audit events use allowlisted fields and an operator or
    worker subject — not artifact bodies or ask text.
51. Archive is `archived_at` on WorkItem / ChatThread / upload,
    independent, no bucket expiry. Abandoned ≠ archived. Event log
    retained 90 days. Do not reuse worktree `archive.json`.

### Authn (security)

52. Operator mutating routes require an operator session on every
    non-loopback bind. Agent submit token is rejected on those routes.
53. Chat jail is required before home chat ships (phase 2). Same-UID
    unsandboxed home chat is out of spec.
54. Canvas/`/files` preview is sanitized (FR26). Origin checks on
    mutating routes once operator auth exists.

## Non-Functional Requirements

- Scale: 1–5 nodes, tens of concurrent items, bounded by veth quota
  and RAM. Control metadata API < 100ms on the tailnet (exclude blob
  GET/PUT).
- New code >80% coverage (workflow).
- FS object store is crash-consistent (rename + fsync), not replicated.
- HTTP over WireGuard is acceptable on v1; prefer `tailscale serve` in
  cluster mode.
- Listing `/board` and `/files` stays usable at hundreds of items /
  artifacts without extra indexes in v1.

## Acceptance Criteria

- [ ] `/chat` talks to the default builtin, switches agent, and produces
      files in `/files` without creating an LXC or a worktree.
- [ ] A file written in home-chat scratch outside the scratch realpath
      (symlink escape) is not uploaded.
- [ ] Selecting those files in `/files` (or “Create work item” on the
      thread) creates a Draft whose canvas/attachments are snapshots;
      the originals remain.
- [ ] Staff + promote puts the item on `/board` in Backlog; moving to
      Todo on all-in-one creates one `sebenza-item-*` LXC **after** all
      host worktrees exist, with every assignment mounted.
- [ ] Closing one worktree does not destroy the item LXC; cancel of a
      Doing item does.
- [ ] Done is impossible with zero tracks, an open blocking ask, or a
      non-done item-scoped track. Observational permission asks do not
      block. There is no force-complete control.
- [ ] Ad-hoc `/{prefix}` create still works and does not appear on
      `/board`.
- [ ] `sebenza-cli item ls` and the dashboard show the same items.
- [ ] A mermaid `click` / unsanitized HTML in a canvas does not execute
      in the operator browser.
- [ ] Worker on a laptop: control never opens a connection to it;
      claim happens on the worker-originated WS.
- [ ] `cargo test` and `npm test` pass without LXC, Headscale, or S3.

## Out of Scope

- Multi-tenant SaaS, hosted inference, replacing agent CLIs.
- Force-complete, off-platform paging, multi-worker items, Postgres HA.
- Incus/LXD VMs, privileged LXC, tabs inside sandboxes.
- `inbox_20260914` convert-to-N-worktrees and additive reconversion.
- Bundled MinIO/Garage; goose as a builtin.
- HIPAA / PHI controls.
- Permission *interception* (answering a gated tool call from the
  dashboard) — observational status only, per `TODO.md`.
