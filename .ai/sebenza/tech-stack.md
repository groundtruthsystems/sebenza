# Technology Stack

## Backend — Rust

Rust 1.85+ (2024 edition), Cargo workspace, three crates:

| Crate | Path | Role |
|---|---|---|
| `common` | `crates/common` | Shared library. **Ports-and-adapters**: `domain/` (config, model, events, policies), `adapters/` (git, tmux, fs, docker, `claude_cli`, `codex_session_log`, `agent_runtime`, hooks, registries), `services/` (orchestration logic). |
| `sebenza-server` | `crates/sebenza-server` | axum 0.8 HTTP/WebSocket daemon. Default port 5111. Binary `sebenza-server`. |
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
- **Optional:** `gh` (PR/CI monitoring), `claude` / `codex` CLIs (built-in agents), `docker`
  (sandboxed worktree runtime).

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
