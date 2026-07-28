# Technology Stack

## Backend — Rust

Rust 1.85+ (2024 edition), Cargo workspace, three crates:

| Crate | Path | Role |
|---|---|---|
| `common` | `crates/common` | Shared library. **Ports-and-adapters**: `domain/` (config, model, events, policies), `adapters/` (git, tmux, fs, docker, `claude_cli`, `codex_session_log`, `agent_runtime`, hooks, registries), `services/` (orchestration logic). |
| `sebenza-server` | `crates/sebenza-server` | axum 0.8 HTTP/WebSocket daemon. Default `127.0.0.1:5111` — loopback by default because most routes are unauthenticated; `--host` / `$SEBENZA_HOST` opts into other interfaces. Binary `sebenza-server`. |
| `sebenza-cli` | `crates/sebenza-cli` | clap 4 all-HTTP client. Binary `sebenza-cli`. |

**Key crates.** tokio (full), axum (ws), tower-http, `rust-embed` (embeds the SPA into the server
binary), reqwest (rustls), `portable-pty`, serde / serde_json / serde_yaml, indexmap
(order-preserving YAML), chrono, anyhow + thiserror, tracing + tracing-subscriber.

## Frontend

React 19, Zustand 5, Vite 6, Tailwind 4, TypeScript 5.

`@xterm/xterm` (+ fit, web-links addons) for terminals, `mermaid` for diagrams, `marked` for
markdown, `diff2html` for diffs.

## Contract

`ts-rest` + Zod 3, shared contract in `frontend/src/lib/api-contract` — the typed seam between
frontend and backend. **Any new backend route must be added to the contract**, with a matching
wrapper in `frontend/src/lib/api.ts`.

## Testing

- Rust: `cargo test` — inline `#[cfg(test)] mod tests` colocated in the module under test.
- Frontend: `npm test` (vitest) + Testing Library — `*.test.ts` / `*.test.tsx` colocated.

## External dependencies

- **Required:** `git`, `tmux`.
- **Optional:** `gh` (PR/CI monitoring), `docker` (sandboxed worktree runtime), and the
  built-in agent CLIs below.

### Built-in agent CLIs and minimum supported versions

| Agent | Minimum verified | Notes |
|---|---|---|
| `claude` | — | Session logs under `~/.claude/projects/<encoded-cwd>/` |
| `codex` | — | Needs `--enable hooks`; assigns its own session id |
| `opencode` | **1.18.7** | Installs to `~/.opencode/bin`, which is **not** on a default `PATH`. History is read via `opencode export <id>` (never `--sanitize`, which redacts the transcript). Session store is SQLite; Sebenza never reads it directly |

`sebenza-cli init` reports each tool's detected version, so a session-format or hook change
that breaks history can be diagnosed against the version actually installed rather than
guessed at. opencode moves fast — 1.18.7 → 1.18.9 was observed within a day — so the
adapter tolerates unknown fields and degrades rather than failing.

### Verified opencode integration constraints

Established by direct observation during the opencode track (1.18.7/1.18.9). Each one
contradicted a reasonable assumption, so they are recorded here rather than left to be
rediscovered:

- **`project_id` is per-REPOSITORY, not per-worktree.** Every worktree of a repo shares one
  opencode project; `project.worktree` records only the first-seen directory. Correlate on
  `session.directory` (via `export` → `info.directory`), **never** on `project_id`.
- **`opencode session list` is project-scoped and has no directory column**, so it cannot
  identify which session belongs to a worktree. Sebenza instead records the id the agent
  reports at creation (`session.created` → `conversation_started`).
- **Never pass `--sanitize` when reading history.** It redacts message text, tool input,
  tool output *and* metadata, yielding `[redacted:…]` placeholders. It is a
  transcript-*sharing* feature.
- **`permission.ask` does not fire** (1.18.9). Only the observational `permission.asked` /
  `permission.replied` events arrive, on the generic `event` hook. See `TODO.md`.
- **`tool.execute.before` fires *before* the permission decision**, so it means "a tool was
  proposed", not "a tool is running".
- **No system-prompt flag.** A per-launch system prompt cannot be passed to an interactive
  session and is dropped.
- **goose's `message_count` header is only safe as a zero-vs-nonzero check** — 19 of 99 real
  sessions under-count. Exact matching misclassifies ~1 in 5 as broken.

`goose` is detected by `init` but is **not** a built-in agent; it remains usable as a custom
(terminal-only) agent. See `TODO.md`.

## Build order constraint

Build the frontend **before** the backend:

```bash
cd frontend && npm install && npm run build && cd ..
cargo build --release
```

A release build bakes `frontend/dist` into `sebenza-server`. Rebuild the backend after changing the
frontend to re-embed it.

## Known gap

There is no `code_styleguides/rust.md`, despite Rust being the bulk of the codebase. The available
style-guide assets did not include one, and it was deliberately not fabricated. Author it explicitly
when convenient — the conventions to capture are visible in the code: module-level `//!` doc
comments, the domain/adapters/services layering, colocated tests, and the `thiserror` (library
errors) vs `anyhow` (application errors) split.
