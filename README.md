# Sebenza

**Manage Git worktrees with AI coding agents — from your browser.**

Sebenza is a self-hosted dashboard for running many coding tasks in parallel. Each
task lives in its own **Git worktree** with a dedicated AI agent (Claude, Codex, or
your own CLI) running in a **tmux-backed terminal** you can drive from the browser.
It watches your pull requests and CI, visualises task progress, and lets you spin
worktrees up, merge them, and tear them down without leaving the dashboard.

One server serves every project you register, on a single port, under per-project
URL prefixes — and everything the dashboard does is also available from the
`sebenza-cli` command line.

---

## Features

- **Worktree lifecycle** — create, open, close, label, archive, merge, and remove
  Git worktrees, each on its own branch, from the UI or CLI.
- **AI agents in the browser** — launch `claude`, `codex`, or a custom agent in a
  worktree; interact through an embedded terminal or the in-app **web chat**.
- **Conductor Tracks board** — a per-worktree Kanban view of a project's Conductor
  tracks (`conductor/tracks.json`): phases as cards grouped by track, drill-down into
  tasks/subtasks, and `spec.md` / `design.md` rendered with mermaid diagrams.
- **PR & CI monitoring** — surfaces GitHub PR state, review comments, and CI status
  per worktree (via the `gh` CLI); optionally auto-removes a worktree when its PR
  merges.
- **Oneshot mode** — run an agent worktree start-to-finish from the CLI, streaming
  the conversation to stdout and auto-closing when it's done.
- **"Open in…" launchers** — open a worktree in your editor of choice (e.g. Zed,
  IntelliJ) with one click.
- **Multi-project, one port** — register any number of repos; each is served under
  its own `/<prefix>` on the shared dashboard.
- **Runs as a service** — install a systemd (Linux) or launchd (macOS) unit so the
  dashboard starts on boot.

## Architecture

A Cargo workspace (Rust) plus a React frontend:

| Component | Path | What it is |
|---|---|---|
| **`common`** | `crates/common` | Shared library: config, domain model, adapters (git, tmux, fs, docker…), services. |
| **`sebenza-server`** | `crates/sebenza-server` | The axum HTTP/WebSocket daemon. Binary: **`sebenza-server`**. |
| **`sebenza-cli`** | `crates/sebenza-cli` | All-HTTP command-line client. Binary: **`sebenza-cli`**. |
| **frontend** | `frontend` | React 19 + Zustand + Vite + Tailwind SPA; talks to the server over `/api` + `/ws`. |

Terminals are real tmux windows bridged to the browser over a PTY WebSocket, so a
session survives even if you close the tab or the CLI process.

---

## Getting started

### 1. Prerequisites

- **Rust** — a recent stable toolchain (2024 edition, i.e. Rust **1.85+**).
- **Node.js 20+** and **npm** — to build the frontend.
- **git** and **tmux** — required for worktrees and terminal sessions.
- Optional: **`gh`** (PR/CI monitoring), **`claude`** / **`codex`** CLIs (built-in
  agents), **`docker`** (sandboxed worktree runtime).

### 2. Build

```bash
git clone <repo-url> sebenza && cd sebenza

# Backend (release binaries land in ./target/release/)
cargo build --release

# Frontend (produces ./frontend/dist/)
cd frontend && npm install && npm run build && cd ..
```

For convenience, put the two binaries on your `PATH`:

```bash
export PATH="$PWD/target/release:$PATH"   # sebenza-cli, sebenza-server
```

### 3. Configure a project

Every repo Sebenza serves needs a `.ai/sebenza.yaml`. From inside your project:

```bash
cd ~/code/my-project
sebenza-cli init          # dependency checks + scaffolds .ai/sebenza.yaml
```

`init` writes a starter config (and, if `claude`/`codex` is available, adapts it to
your project). You can also copy and edit [`.ai/sebenza.example.yaml`](.ai/sebenza.example.yaml)
by hand.

### 4. Run the dashboard

From your project directory:

```bash
sebenza-cli serve          # starts sebenza-server + finds the built frontend
```

Then open **http://localhost:5111** — the dashboard opens on your project. Add
`--app` to launch it in a minimal browser window.

> Running the server binary directly instead of `sebenza-cli serve`? Point it at the
> built SPA with `SEBENZA_FRONTEND_DIST=/path/to/sebenza/frontend/dist`, e.g.
> `SEBENZA_FRONTEND_DIST=$PWD/frontend/dist sebenza-server serve --port 5111`.

### 5. Create a worktree and start working

In the dashboard: pick a base branch, name a branch, choose an agent, and hit
create. Or from the CLI:

```bash
sebenza-cli add my-feature --agent claude   # create + open a worktree
sebenza-cli list                            # see worktrees and their status
sebenza-cli send my-feature "add a health check endpoint"
sebenza-cli merge my-feature                # merge into main and remove
```

### Register more projects

```bash
sebenza-cli project add ~/code/another-repo
sebenza-cli project ls
```

They all appear on the same dashboard, each under `/<prefix>`.

---

## CLI overview

`sebenza-cli --help` lists everything. Common commands:

| Command | Description |
|---|---|
| `serve` | Start the dashboard server (`--app` opens app mode). |
| `init` | Interactive project setup. |
| `add` / `remove` | Create / remove a worktree. |
| `open` / `close` | Open / close a worktree's session (without removing it). |
| `list` | List worktrees and status. |
| `send` | Send a prompt to a running worktree agent. |
| `merge` | Merge a worktree into the main branch and remove it. |
| `label` / `archive` / `unarchive` | Organise worktrees. |
| `tab` | List/create/switch/close agent tabs in a worktree. |
| `prune` / `restore` | Remove closed worktrees / re-open previously-open sessions. |
| `oneshot` | Run a worktree start-to-finish, streaming to stdout. |
| `project` | `ls` / `add` / `rm` / `migrate` the served projects. |
| `service` | Install/uninstall the systemd/launchd service. |
| `completion` | Print a bash/zsh completion script. |

Without `--port`, CLI commands target the live server for the current project
(discovered automatically); pass `--port N` to target a specific instance.

## Configuration & data

| What | Location |
|---|---|
| Project config | `<repo>/.ai/sebenza.yaml` (+ `.ai/sebenza.local.yaml` for local overrides) |
| Machine-wide launchers | `~/.ai/sebenza.yaml` |
| Server state (project registry, instances) | `~/.ai/sebenza/` |
| Control token | `~/.config/sebenza/control-token` |
| Environment | `PORT` (server port, default `5111`), `SEBENZA_FRONTEND_DIST` (SPA assets) |

## Development

```bash
# Backend: run the server for the current project with logs
cargo run -p sebenza-server --bin sebenza-server -- serve --port 5111

# Frontend: Vite dev server with hot reload (proxies /api + /ws to the backend)
cd frontend && npm run dev        # http://localhost:5112

# Tests
cargo test                        # Rust workspace
cd frontend && npm test           # frontend (vitest)
```

See [`CLAUDE.md`](CLAUDE.md) for AI-assistant / contribution guidance.

## License

[Apache-2.0](LICENSE)
