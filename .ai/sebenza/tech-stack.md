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
- **Optional:** `gh` (PR/CI monitoring), `lxc` / `lxc-create` (Linux sandboxed worktree runtime) or Apple `container` (macOS 26 Apple Silicon), and the
  built-in agent CLIs below.

### Built-in agent CLIs and minimum supported versions

| Agent | Minimum verified | Notes |
|---|---|---|
| `claude` | — | Session logs under `~/.claude/projects/<encoded-cwd>/` |
| `grok` | **1.0.5** | xAI "Grok Build" (`curl -fsSL https://x.ai/cli/install.sh \| bash`). Installs to `~/.grok/bin`, which is **not** on a default `PATH`. History from `~/.grok/sessions/<url-encoded-cwd>/<id>/updates.jsonl`. Needs `GROK_CLAUDE_HOOKS_ENABLED=0` and folder trust — see below |
| `codex` | — | Needs `--enable hooks`; assigns its own session id |
| `opencode` | **1.18.7** | Installs to `~/.opencode/bin`, which is **not** on a default `PATH`. History is read via `opencode export <id>` (never `--sanitize`, which redacts the transcript). Session store is SQLite; Sebenza never reads it directly |

`sebenza-cli init` reports each tool's detected version, so a session-format or hook change
that breaks history can be diagnosed against the version actually installed rather than
guessed at. opencode moves fast — 1.18.7 → 1.18.9 was observed within a day — so the
adapter tolerates unknown fields and degrades rather than failing.

### Verified grok integration constraints

Established by direct observation against Grok Build 1.0.5. grok's surface is close enough
to Claude Code's that the differences are easy to assume away, and each of these was found
by testing rather than reading:

- **grok reads Sebenza's *Claude* hooks by default.** `[compat.claude] hooks` is on, and it
  scans `<worktree>/.claude/settings.json` **and `settings.local.json`** — exactly where
  Sebenza writes its claude hooks. Confirmed with `grok inspect`, which listed all five as
  `project [claude]`. Every grok pane command therefore sets
  **`GROK_CLAUDE_HOOKS_ENABLED=0`**; with it, those five report `[disabled]` and the
  Harness Compatibility table reads `claude → hooks OFF (env)`, while all seven of
  Sebenza's own `.grok/hooks/sebenza.json` hooks stay active.
- **Hook payloads are camelCase, and the tool output field is `toolResult`.** grok sends
  `sessionId`/`toolName`/`toolInput`/`toolResult` where Claude sends
  `session_id`/`tool_name`/`tool_input`/`tool_response`. Untranslated, `maybe_send_pr_opened`
  never fires and nothing reports an error — hence the `grok-*` agentctl subcommands
  normalise first.
- **The chat stream is the exception: it *is* snake_case.** `streaming-messages-json` lines
  carry `session_id`, matching the Messages wire format, so `parse_claude_stream_line`
  handles grok unchanged. Pinned by a committed fixture of a real captured run.
- **Project hooks are silently skipped in an untrusted folder.** With trust revoked
  `grok inspect` reports `Hooks (0)` and no diagnostic, so status reporting simply stops.
  **Trust resolves through the git common dir**: trusting the main repo also trusts every
  `git worktree` of it, even one living outside that directory. Sebenza does not pass
  `--trust` (that would let an unvetted repo run its own `.grok/hooks/` and load
  `.grok/plugins/`); it warns instead.
- **An extra observe-only `Stop` fires at session end**, so genuine turn ends must be
  filtered on `reason == "end_turn"` — while still admitting the `StopCancelled` reasons.
- **`StopCancelled` has no Claude/Codex analogue and is required.** It fires *instead of*
  `Stop` on an interrupt, a declined permission, `--max-turns`, or a no-progress bail-out;
  without it an interrupted worktree shows "running" forever.
- **Subagent events carry `subagentType`.** A background subagent outlives the parent turn,
  so unfiltered its events hold the worktree at "running" after the main agent went idle.
- **`Notification` must match `permission_prompt`, not `idle_prompt`.** grok fires
  `idle_prompt` on *any* turn end, including interrupted and errored ones.
- **Hooks default to a 5-second timeout**, short enough to cut off the control POST, so
  every generated hook sets `timeout` explicitly.
- **`grok -p` takes the prompt as the flag's value, not on stdin** (unlike `claude -p`).
- **`--rules` appends to the system prompt; `--system-prompt-override` replaces it** and
  would strip grok's own tool instructions.
- **`updates.jsonl` is the history source, not `chat_history.jsonl`.** Only the former has
  per-message timestamps and a `params.sessionId` on every line to verify the transcript
  with. `turn_completed` arrives with method `_x.ai/session/update`, so the method must not
  be filtered on; each `*_chunk` is a whole message, not a fragment; and `tool_call_update`
  has an enrichment flavour and a `status`-bearing result flavour, only the latter of which
  is a message.
- **`grok sessions list` has no `--json`**, so it cannot be used for correlation. Sebenza
  pins the id with `-s` at launch instead and cross-checks the `SessionStart` hook.
- **Do not use `-w/--worktree`** — that is grok's own worktree feature and would fight
  Sebenza for control of the checkout.

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
