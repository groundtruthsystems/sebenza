# Sebenza

**Manage Git worktrees with AI coding agents — from your browser.**

Sebenza is a self-hosted dashboard for running many coding tasks in parallel. Each
task lives in its own **Git worktree** with a dedicated AI agent (Claude, Grok, Codex,
OpenCode, or your own CLI) running in a **tmux-backed terminal** you can drive from the browser.
It watches your pull requests and CI, visualises task progress, and lets you spin
worktrees up, merge them, and tear them down without leaving the dashboard.

One server serves every project you register, on a single port, under per-project
URL prefixes — and everything the dashboard does is also available from the
`sebenza-cli` command line.

---

## Features

- **Worktree lifecycle** — create, open, close, label, archive, merge, and remove
  Git worktrees, each on its own branch, from the UI or CLI.
- **AI agents in the browser** — launch `claude`, `grok`, `codex`, `opencode`, or a custom agent in a
  worktree; interact through an embedded terminal or the in-app **web chat**.
- **Inbox** — take notes before you know which repo they belong to. Drafts are plain
  markdown with mermaid, edited in the dashboard with a live preview, and converted
  into worktrees when you are ready — one draft can fan out to several worktrees
  across different projects, each with its own branch, agent, and prompt.
- **Tracks board** — a per-worktree Kanban view of a project's Sebenza tracks
  (`.ai/sebenza/tracks.json`, written by the `sebenza` Claude Code plugin):
  phases as cards grouped by track, drill-down into tasks/subtasks, and `spec.md` /
  `design.md` rendered with mermaid diagrams.
- **Registry portfolio** — `/registry` aggregates tracks across *every* project in
  the plugin's user-scoped registry (`~/.ai/sebenza/registry.json`), with a
  cross-project status rollup and every blocker in one list.
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
| **`common`** | `crates/common` | Shared library: config, domain model, adapters (git, tmux, fs, lxc, apple container…), services. |
| **`sebenza-server`** | `crates/sebenza-server` | The axum HTTP/WebSocket daemon. Binary: **`sebenza-server`**. |
| **`sebenza-cli`** | `crates/sebenza-cli` | All-HTTP command-line client. Binary: **`sebenza-cli`**. |
| **frontend** | `frontend` | React 19 + Zustand + Vite + Tailwind SPA; talks to the server over `/api` + `/ws`. |

Terminals are real tmux windows bridged to the browser over a PTY WebSocket, so a
session survives even if you close the tab or the CLI process.

---

## Getting started

### 1. Prerequisites

- **git** and **tmux** — required for worktrees and terminal sessions.
- Optional: **`gh`** (PR/CI monitoring), **`lxc-create`** (Linux sandbox) or **`container`** (macOS 26 Apple Silicon sandbox), and
  the built-in agent CLIs — **`claude`**, **`grok`** (1.0.5+), **`codex`**, **`opencode`** (1.18.7+).
  `sebenza-cli init` lists which it found and at what version.

Building from source additionally needs **Rust 1.85+** (2024 edition) and
**Node.js 20+** with npm — see [Build from source](#build-from-source).

### 2. Install

The install script grabs the latest [GitHub Release](https://github.com/groundtruthsystems/sebenza/releases)
build for your platform, verifies its checksum, and drops `sebenza-server` and
`sebenza-cli` into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/groundtruthsystems/sebenza/main/scripts/install.sh | bash
```

The dashboard UI is embedded in `sebenza-server`, so those two binaries are the
whole install — there is nothing else to put on disk.

Prebuilt binaries cover **Linux** (x86-64, arm64) and **macOS** (Apple Silicon).
On an Intel Mac, build from source.

If `~/.local/bin` isn't on your `PATH`, the script tells you what to add to your
shell profile. To customise the install, run the script directly:

```bash
curl -fsSL https://raw.githubusercontent.com/groundtruthsystems/sebenza/main/scripts/install.sh -o install.sh
bash install.sh --dir /usr/local/bin      # where to install (default ~/.local/bin)
bash install.sh --version v0.1.0          # pin a release (default: latest)
bash install.sh --uninstall               # remove the binaries again
```

Each flag has a matching environment variable — `SEBENZA_INSTALL_DIR`,
`SEBENZA_VERSION`, `SEBENZA_REPO` — and `GITHUB_TOKEN` is used for the release
lookup when set. Re-running the script upgrades an existing install in place.

### 3. Configure a project

Every repo Sebenza serves needs a `.ai/sebenza.yaml`. From inside your project:

```bash
cd ~/code/my-project
sebenza-cli init          # dependency checks + scaffolds .ai/sebenza.yaml
```

`init` writes a starter config (and, if a built-in agent CLI is available, adapts it to
your project). You can also copy and edit [`.ai/sebenza.example.yaml`](.ai/sebenza.example.yaml)
by hand.

### 4. Run the dashboard

From your project directory:

```bash
sebenza-cli serve          # starts sebenza-server with the embedded dashboard
```

Then open **http://localhost:5111** — the dashboard opens on your project. Add
`--app` to launch it in a minimal browser window. The UI is embedded in the
binary, so this works from any directory with nothing else on disk.

> **Dev override:** to serve a freshly-built SPA from disk without recompiling the
> server, set `SEBENZA_FRONTEND_DIST=/path/to/sebenza/frontend/dist` — when set, the
> server serves that directory instead of the embedded bundle.

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

## Build from source

Needed for Intel macOS, and for hacking on Sebenza. Requires **Rust 1.85+** (2024
edition) and **Node.js 20+** with npm.

```bash
git clone https://github.com/groundtruthsystems/sebenza.git && cd sebenza

# 1. Build the frontend first (produces ./frontend/dist/)
cd frontend && npm install && npm run build && cd ..

# 2. Build the backend — this EMBEDS ./frontend/dist into the sebenza-server
#    binary, so the dashboard UI ships inside the executable.
cargo build --release
```

> Build the frontend before the backend: a release build bakes `frontend/dist`
> into `sebenza-server`, so the server serves the UI from any directory with no
> extra files. (Rebuild the backend after changing the frontend to re-embed it.)

For convenience, put the two binaries on your `PATH`:

```bash
export PATH="$PWD/target/release:$PATH"   # sebenza-cli, sebenza-server
```

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
| `inbox` | `ls` / `show` / `new` / `edit` / `link` / `drop` / `rm` drafts, plus `convert` and `job`. |
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
| Inbox drafts | `~/.ai/sebenza/inbox/<ulid>.md` — global, outside every repo |
| Control token | `~/.config/sebenza/control-token` |
| Environment | `PORT` (server port, default `5111`); `SEBENZA_HOST` (bind host, default `127.0.0.1`); `SEBENZA_FRONTEND_DIST` (optional — serve the SPA from disk instead of the embedded bundle) |

## Inbox

An idea rarely arrives knowing which repository it belongs to. The inbox is where it
waits: a global store of markdown drafts at `~/.ai/sebenza/inbox/`, reachable at
`/inbox` or from the icon rail, and usable before any project is registered.

Each draft is one self-contained `.md` file — YAML frontmatter, then the body — named
by an opaque id rather than its title, so renaming never moves the file. They are
ordinary files: edit them in the dashboard or in your own editor, and the dashboard
notices an external change rather than overwriting it.

### Converting a draft

**Convert** turns one draft into worktrees. Add a row per worktree, each with its own
project, branch and prompt (up to ten per conversion). Every created worktree gets the
target's prompt *and* a copy of the whole draft at `.ai/sebenza/inbox-note.md`, so the
agent can re-read the notes and diagrams after its first turn.

From the CLI, a target is `project:branch:prompt`:

```bash
sebenza-cli inbox new "Rework the claims scorer"
sebenza-cli inbox convert <draft-id> \
  ~/code/acme:fix-scorer:'rewrite the scorer' \
  ~/code/beta:add-metrics:'add the metrics endpoint' --watch
```

Targets are validated as a set before anything runs, so a bad branch name or an
unregistered project fails the whole request rather than leaving half a fan-out
behind. Once it starts, targets are independent: one failure does not touch the
others, and each outcome — success or not — is recorded in the draft as it happens.

The draft survives conversion, marked promoted, carrying back-links to everything it
produced. Converting again pre-fills from the previous wave.

### What conversion writes, and what it does not

The draft copy and its origin marker are added to the worktree's
`$GIT_DIR/info/exclude`, not to `.gitignore` — `.gitignore` is tracked, so editing it
would dirty every worktree it touched. Your notes therefore never show up in
`git status` and cannot be committed by accident.

### Warnings

Before a fan-out, Sebenza flags two things and then gets out of the way — both are
advisory, neither blocks:

- **Credential-shaped text** in the draft. It is about to be copied into a real
  checkout and sent to whichever model provider the agent uses. The scan is narrow on
  purpose and will miss things; the inbox is not a secrets vault.
- **An unsandboxed target.** A draft is likelier than a typed prompt to contain text
  pasted from elsewhere, and an unsandboxed agent acts on it with shell access.
  Install `lxc` (Linux) or Apple `container` (macOS) and set a sandboxed profile to
  avoid this.

### Network exposure

The server binds **`127.0.0.1` by default**, so the dashboard is reachable only from the machine
running it. Most routes are unauthenticated — worktree creation, the terminal PTY, and agent control
included — so exposing the port to a network gives anyone who can reach it the same power you have.

To serve other machines deliberately, opt in:

```bash
sebenza-server serve --host 0.0.0.0        # or: SEBENZA_HOST=0.0.0.0 sebenza-cli serve
```

The server logs a warning whenever it binds a non-loopback address.

> **Changed behaviour:** earlier versions always bound `0.0.0.0`. If you reach the dashboard from
> another machine, set `SEBENZA_HOST=0.0.0.0` (or pass `--host`) after upgrading.

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
