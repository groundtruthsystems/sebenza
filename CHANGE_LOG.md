## Change Log

### 2026-09-28 — Inbox: markdown/mermaid drafts converted into worktrees (inbox_20260914)

Added a global inbox of markdown and mermaid drafts at `~/.ai/sebenza/inbox/<ULID>.md`, edited in a split dashboard editor or any external editor and optionally linked to a project. A draft converts into up to ten worktrees across registered projects, each with its own branch, base, agent and seed prompt and each receiving the draft as `.ai/sebenza/inbox-note.md`. Saves are body-hash gated (409 on conflict), mutating routes need the control token plus a same-origin check, and conversion warns on secrets and unsandboxed profiles before launching. The draft survives as Promoted with a conversions ledger and can convert again.

### 2026-08-22 — LXC / Apple worktree sandboxes (lxc_sandbox_20260819)

Worktrees can run in an unprivileged LXC container on Linux or an Apple Container machine on macOS 26 Apple Silicon, with the worktree mounted at the same absolute host path and panes running as the host user so files stay host-owned. Sandboxes fail closed if the uid/gid idmap cannot be applied, survive close, and are deleted on remove or merge. Docker is no longer launchable; existing `runtime: docker` profiles still load but return 422 on create.
