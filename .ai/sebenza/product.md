# Product Definition — Sebenza

**Self-hosted parallel AI coding orchestration.**

## Vision

Let one developer supervise many AI coding tasks at once, with no agent vendor lock-in.

## What it is

A self-hosted dashboard that gives each coding task its own Git worktree and dedicated coding
agent, running in a tmux-backed terminal drivable from the browser or an in-app chat. It monitors
pull requests and CI, visualises progress on a Conductor Tracks Kanban board, and manages the whole
worktree lifecycle — create, open, label, archive, merge, remove — without leaving the dashboard.

One server serves every registered project on a single port under per-project URL prefixes, and
everything the dashboard does is also available from `sebenza-cli`.

## Core goal — agent-agnostic parallelism

Agents are interchangeable. A worktree's agent is a **choice made at creation time**, and the
dashboard's capabilities should **degrade gracefully** rather than assume a specific CLI. Adding a
new agent should be an adapter, not a fork.

## Principles

1. **CLI/UI parity** — everything the dashboard does is available from `sebenza-cli`.
2. **Sessions outlive clients** — tmux-backed, so closing the tab or the CLI never kills work.
3. **Own your infrastructure** — your keys, your machine, no SaaS intermediary.
4. **Single binary** — the UI is embedded in `sebenza-server`; two binaries are the whole install.

## Non-goals

- Hosting or proxying model inference.
- Replacing the agents' own CLIs.
- Multi-tenant SaaS.
