# Spec — goose & opencode as first-class agents

**Track:** `goose_opencode_agents_20260727` · **Type:** feature
**Design:** [design.md](./design.md) — verified across business, application, technical, data, security

## Overview

Make **goose** (1.8.0+) and **opencode** (1.18.7+) built-in agents in Sebenza, peers of `claude` and
`codex`: selectable in the Create Worktree dialog and via `sebenza-cli add --agent`, with in-app chat,
conversation history, lifecycle status, interrupt, and resume/fork gated by *declared capability*
rather than hardcoded agent identity.

Because opencode is the only agent of the four whose plugin API can **deny** a tool call
(`permission.ask` returns a mutable `status`), this track additionally builds Sebenza's first
**enforcement** path: a synchronous, authenticated permission approve/deny channel.

Both CLIs were verified by direct observation, not documentation. Two questions still require an
**authenticated** opencode and are front-loaded as Phase 0 verification rather than assumed.

### Decisions taken during refinement

| # | Decision |
|---|---|
| D1 | Builtin registration **wins** over a shadowed custom-agent entry; `.ai/sebenza.example.yaml` drops its `goose`/`opencode` entries; the override is reported via a **durable** event |
| D2 | `GOOSE_MODE=auto` **is** offered, with disclosure distinct from claude/codex yolo |
| D3 | Pre-existing untrusted plugins: **scan, block auto-launch, require confirmation** |
| D4 | Both agents target **full parity**; phased on implementation shape, not capability |
| D4a | **opencode ships before goose.** The shared security and runtime controls (git-exclusion, `0600` env files, untrusted-plugin scan, docker mounts, shadow resolution) move into opencode's phase — whichever agent lands first pays for that infrastructure, and opencode needs it more, since in-process JS execution is a larger surface than a shell hook |
| D5 | Permission gating for opencode is **in scope** (Phase 4) |
| D6 | A **loopback-default bind** is included in this track, gating `GOOSE_MODE=auto` and the Phase 4 routes |
| D7 | The two duplicate builtin registries are **unified** in Phase 1 |
| D8 | The latent `claude_conversation_service` dispatch bug gets a **Phase 0 investigation** task |
| D9 | Untrusted-plugin confirmation is **per-repo, remembered** |

---

## Functional Requirements

> **Phase order.** `0` verification & prerequisites → `1` agent abstraction (pure refactor) →
> `2` **opencode** + shared controls → `3` **goose** → `4` permission gating.
> Requirement groups below are numbered by topic, not by phase: **FR-2 (goose) is delivered in Phase 3**
> and **FR-3 (opencode) in Phase 2**.

### FR-0 — Verification & prerequisites (Phase 0)

- **FR-0.1** Determine empirically whether opencode resolves a **linked git worktree** to its own
  `project` row or to the parent repository's. Record the finding. If it resolves to the parent, the
  per-worktree correlation design must be revised before implementation. **Blocking for Phase 2.**
- **FR-0.2** Determine whether **`permission.ask` still fires when `--auto` is passed**. Record whether
  bypass and Sebenza-side gating can coexist or are mutually exclusive; **blocking for Phase 4** wiring.
- **FR-0.3** Capture a real `opencode export` JSON payload and a real goose session containing
  `toolRequest`/`toolResponse` blocks, and commit both as `testdata/` fixtures.
- **FR-0.4** Determine whether `server.rs:1382` (interrupt) and `~1522` (streaming WebSocket), which call
  `claude_conversation_service::read_worktree_conversation` with **no agent match**, are a latent codex
  bug or intentional. Fix or document before any dispatch generalisation.
- **FR-0.5** Change the server to bind **`127.0.0.1` by default**, with an explicit opt-in flag/env for
  binding all interfaces. This is a **behaviour change** for anyone currently reaching the dashboard from
  another machine and must be called out in release notes.

### FR-1 — Agent registry and capability model (Phase 1)

- **FR-1.1** Replace `AgentImplementation::Builtin(String)` with `Builtin(BuiltinAgentId)`, an enum.
  Phase 1 adds **no new agents** — it carries only `Claude` and `Codex`, so it is a pure refactor.
  `Opencode` arrives in Phase 2 and `Goose` in Phase 3, each as its own exhaustiveness sweep, so neither
  phase quietly performs the other's work.
- **FR-1.2** **Unify the two builtin registries.** `config_view.rs::builtin_agent_summaries()` must derive
  from `agent_registry.rs` so there is one builtin list and one capability struct. `config_view.rs` is what
  feeds `build_app_config` → `config.agents` → the Create Worktree picker; leaving it separate would mean
  the new agents never appear in the UI.
- **FR-1.3** Add three capability fields — `fork`, `pinnable_session_id`, `permission_interception` — to
  `AgentCapabilities` and `AgentCapabilitiesWire`, with explicit `false` for custom agents. Widen the
  matching Zod schemas. `permission_interception` **is** wire-visible (Phase 4's UI needs it).
- **FR-1.4** Per-agent capability values must match the verified matrix in the design; no agent may
  declare a capability that was not verified.
- **FR-1.5** Every `server.rs` dispatch site must **resolve through `get_agent_definition` and match on
  the enum**, not add string arms. Applies to `refresh_agent_terminal`, `agents_conversation`,
  `prepare_agent_send`, and `submit_delay_for_branch`.
- **FR-1.6** Shadow resolution: a custom-agent entry whose id equals a builtin id is **overridden by the
  builtin**, and the override emits a durable `shadowed_custom_agent_detected` event naming the id and
  stating the key-rename escape hatch. Remove both entries from `.ai/sebenza.example.yaml`.

### FR-2 — goose integration (Phase 3)

- **FR-2.1** Launch/resume/fork invocations: fresh session, `-n <session>` for a pinned id,
  `-r` for resume, `--history` where appropriate.
- **FR-2.2** Write `<worktree>/.agents/plugins/sebenza/hooks/hooks.json` as a **full overwrite** (Sebenza
  owns the subtree — no merge logic). Map `UserPromptSubmit`, `PostToolUse`, `Stop`, and `SessionStart`
  onto the existing `sebenza-agentctl` primitives via new `goose-*` subcommands.
- **FR-2.3** Parse `~/.local/share/goose/sessions/<id>.jsonl` into `AgentsUiMessage`, mapping the
  **structured content blocks**: `text` → `kind: text`; `toolRequest{id, toolCall}` → `kind: toolUse`
  with `tool_call_id`/`tool_name`; `toolResponse{id, toolResult}` → `kind: toolResult` correlated by `id`,
  with `status` from `toolResult.status`.
- **FR-2.4** Correlate sessions to worktrees by **exact string equality** on the header `working_dir` —
  never prefix matching.
- **FR-2.5** Use the header `message_count` **only** as a zero-vs-nonzero check.
  `message_count > 0` with zero parsed messages → surface "history unavailable". `message_count == 0` or a
  0-byte file → legitimately empty. **Never compare exact counts** (19 of 99 real sessions under-count).
  Ignore unknown header fields.
- **FR-2.6** Skip `capture_new_session_id` for goose — the id is pinned at launch.
- **FR-2.7** Express `GOOSE_MODE` via **`runtime.env`**, not a command flag and not an inline
  `VAR=value` prefix. Validate the value against the closed set `{auto, approve, chat, smart_approve}`
  **at the point the map is built**. Protect the key from `env_passthrough` clobbering.
- **FR-2.8** `GOOSE_MODE=auto` must be presented with copy, a confirmation step, and a persistent
  in-session indicator **distinct** from the claude/codex yolo toggle, stating that Sebenza cannot
  intercept or deny any action. Re-confirm per project.
- **FR-2.9** Add a `goose` arm to `llm_spawn.rs::build_llm_args` (`goose run -t … --system … --no-session`)
  and to `init_authoring.rs` (`InitAgent`, `authoring_agent()`, `build_init_agent_command()`), plus the
  `AutoNameProvider` variant and its YAML-parsing counterpart in `config.rs:273-274`.

### FR-3 — opencode integration (Phase 2)


- **FR-3.1** Add the `Opencode` enum variant and handle every resulting exhaustiveness error.
- **FR-3.2** Invocations: fresh, `-c`/`--continue`, `-s <id>`, `--fork`, `--auto` for bypass (a plain
  flag, appended like claude/codex), `--title`, `--agent`, `--model`, `--dir`.
- **FR-3.3** Generate `<worktree>/.opencode/plugins/sebenza.js` as a full overwrite. It must **import
  nothing** (avoiding `@opencode-ai/plugin` module resolution) and use only runtime globals.
- **FR-3.4** Map the generic `event` hook: `EventSessionCreated` → session-id capture (**no polling**);
  `EventSessionIdle` → idle/agent-stopped; `EventSessionError` → runtime error;
  `tool.execute.before` → running; `tool.execute.after` → PR detection via the existing
  `maybe_send_pr_opened`.
- **FR-3.5** Read history via **`opencode session list`** and **`opencode export <id> --sanitize`**.
  **Do not read `opencode.db`.** Tolerate unknown fields in the export JSON.
- **FR-3.6** Correlate via the `project.worktree` → session `directory`/`path` relationship, per FR-0.1's
  finding.
- **FR-3.7** Probe `~/.opencode/bin` explicitly when resolving the binary — a plain `which("opencode")`
  can fail on a working install because opencode installs outside conventional directories.

### FR-4 — Permission gating for opencode (Phase 4)

- **FR-4.1** The generated shim implements `permission.ask`, POSTs the request, `await`s a verdict via
  `fetch` (a runtime global under opencode's bundled Bun), and writes it to `output.status`.
- **FR-4.2** **Asymmetric credentials.** The *submit* route may authenticate with the existing control
  token. The *resolve* route **must** require a per-request resolver secret minted server-side and
  delivered **only** in the WS push payload and the CLI response — never written to `control.env`,
  `runtime.env`, or any file the agent process can read. Rationale: the agent holds
  `SEBENZA_CONTROL_TOKEN` (`fs.rs:320-330`), so a shared credential would let it approve its own request.
  Verdicts are single-use and bound to session + request id.
- **FR-4.3** The pending-decision store mirrors `agent_stream.rs`'s `AgentStreamManager`/`RunState`
  idiom — a concurrent map of ids to `oneshot` senders resolved by a side-channel call. In-memory, not
  durable across restart.
- **FR-4.4** The submit handler `.await`s a `oneshot::Receiver`. **No lock may be held across the await.**
- **FR-4.5** **Fail closed.** Timeout, HTTP error, and connection reset (including server restart) all
  resolve to `deny`. The **server's** timeout is authoritative; any shim-side timeout is a longer network
  backstop only.
- **FR-4.6** A denial caused by timeout must be **visible** in the transcript and worktree status — a
  stalled worktree must read as "waited for you, then denied", not as a hung agent.
- **FR-4.7** Cap concurrent pending requests per session and server-wide, auto-denying beyond the cap and
  emitting `pending_permission_cap_hit`.
- **FR-4.8** Timeout enforcement is **independent of WS delivery**; the CLI list/answer path is the
  designed fallback for a missed push.
- **FR-4.9** Dashboard prompt card modelled on `AskUserQuestionCard`'s look (its plumbing is
  transcript-level and not reusable), plus `sebenza-cli` commands to list and answer pending requests
  (Guiding Principle 8).
- **FR-4.10** Per-profile decision timeout, configurable.
- **FR-4.11** Bind the two new routes to loopback unconditionally.

### FR-5 — Security controls (Phase 2, shared — goose inherits them in Phase 3)

- **FR-5.1** Git-exclude **all** generated in-worktree artifacts: `.agents/plugins/`,
  `.opencode/plugins/`, and retroactively `.claude/settings.local.json`. Generalise
  `ensure_generated_codex_hooks_ignored` to a path list.
- **FR-5.2** Before launching goose/opencode, scan for pre-existing plugin files Sebenza did not write.
  Compare against a **stored hash of the bytes Sebenza last wrote**, persisted outside the worktree in
  `<git-common-dir>/.ai/sebenza/` — **not** a freshly recomputed expectation, which would re-trigger on
  every Sebenza upgrade and train click-through.
- **FR-5.3** On detection, **block auto-launch** and require confirmation, **remembered per repository**,
  stored at control-token trust tier outside the worktree. Any state used to make a trust decision about
  a repo must not be writable by that repo's agent process.
- **FR-5.4** When the user declines, offer opencode's **`--pure`** (runs without external plugins) as a
  safe terminal-only fallback. goose has no equivalent — declining means hooks are not written.
- **FR-5.5** Apply `set_mode_600` to `control.env` **and** `runtime.env` (`runtime.env` merges
  `.env.local` project secrets, not just the control token).
- **FR-5.6** Never interpolate untrusted strings into generated artifacts. Hook commands stay
  `shell_quote(agentctl) + fixed literal`. The generated `.js` embeds no branch/worktree literals; per-
  worktree data arrives via `SEBENZA_*` env vars. The plugin directory name is a Sebenza-owned constant.
- **FR-5.7** New hooks and shims forward **only derived signals**, never raw `tool_input`, command text,
  or env values — matching the existing `sebenza-agentctl` discipline.
- **FR-5.8** Persist audit events under `<git-common-dir>/.ai/sebenza/`: `agent_launched` (with bypass
  mode), `artifact_written`/`artifact_overwritten` (with hash-mismatch boolean),
  `untrusted_plugin_detected`, `shadowed_custom_agent_detected`, and in Phase 4 `permission_decision`
  (with **resolution source**) and `pending_permission_cap_hit`. No event may carry secrets.

### FR-6 — Runtime, UI, and docs

- **FR-6.1** Docker: bind-mount `~/.config/goose`, `~/.config/opencode`, `~/.local/share/goose/sessions`,
  and `~/.local/share/opencode` **unconditionally** (matching the existing claude/codex mounts), so
  history parity holds on host and in docker without a runtime-dependent capability model. Add
  `/root/.opencode/bin` to `DOCKER_PATH_FALLBACK`.
- **FR-6.2** Replace all hardcoded agent-identity checks in the frontend with capability reads:
  `App.tsx:230` `canFork`; `App.tsx:162-169` `supportsWorktreeChat` fallback → **fail closed** (`false`,
  hiding the tab) rather than a hardcoded id list; `WorktreeConversationPanel.tsx:169-170` label and
  `supportsAgentChat`; `MobileChatSurface.tsx:90,321` provider dispatch.
- **FR-6.3** Widen `BuiltInAgentIdSchema`, `AutoNameProviderSchema`, and
  `WorktreeConversationProviderSchema` (plus its literal variants); extend `WorktreeConversationProvider`
  in `model.rs` additively without assuming field parity with claude's shape.
- **FR-6.4** Replace agent-count-hardcoded 409 messages ("only available for Claude and Codex worktrees")
  with text derived from which agents declare the capability.
- **FR-6.5** Act on `init.rs:52`'s existing goose/opencode probes (currently unused downstream), report
  detected versions, and document a minimum supported version per agent in `tech-stack.md`.
- **FR-6.6** Update `README.md` prerequisites and `.ai/sebenza.example.yaml`.

---

## Non-Functional Requirements

- **NFR-1** No blocking I/O on the async runtime — `spawn_blocking` for filesystem and subprocess work,
  `oneshot` awaits for the permission channel.
- **NFR-2** History parsing stays **parse-on-demand**; no persistent server-side history cache (Sebenza
  must not become a second unbounded retention surface over agent-owned logs).
- **NFR-3** Parsers tolerate unknown fields and malformed or empty input without panicking. No `unwrap()`
  on fallible paths.
- **NFR-4** Config backward compatibility: existing `sebenza.yaml` files keep loading. Note in release
  notes that `AutoNameProvider`/`WorktreeConversationProvider` additions are **one-way** — a config
  written with `provider: goose` cannot be read by an older binary.
- **NFR-5** >80% coverage on new code; `cargo fmt --check`, `cargo clippy`, and `npm run check` clean.
- **NFR-6** Cross-platform: probe both XDG and platform-native config/session paths rather than
  hardcoding one. macOS behaviour for both CLIs must be verified before release.
- **NFR-7** Tests must not require goose or opencode on `PATH` — mock the process boundary and use
  `testdata/` fixtures.
- **NFR-8** CLI/UI parity for every capability added.

---

## Acceptance Criteria

1. goose and opencode appear in the Create Worktree dialog and in multi-agent mode, and
   `sebenza-cli add --agent goose|opencode` creates a working worktree.
2. `default_agent: goose` and `default_agent: opencode` are honoured.
3. In-app chat and conversation history work for both, with goose transcripts rendering tool-call cards
   and status badges (not flattened prose).
4. Lifecycle status transitions (running / idle / stopped) are driven by real hook or event traffic for
   both agents.
5. Resume works for both; fork works wherever the capability is declared `true`.
6. No UI affordance is shown for a capability an agent does not declare, and where one is hidden the
   reason is stated.
7. A worktree whose repo ships pre-existing plugin files does **not** auto-launch the agent until
   confirmed; the decision is remembered per repository.
8. No generated artifact appears in `git status` in a fresh worktree.
9. `GOOSE_MODE=auto` is reachable only through its own confirmation, is visibly indicated for the
   session, and is disclosed as ungateable.
10. An opencode tool call requiring permission surfaces a dashboard prompt; approving allows it, denying
    blocks it, and no answer within the timeout denies it **visibly**.
11. The resolve route rejects a verdict presented with only the control token.
12. Conversation history works identically under the host and docker runtimes.
13. An existing config with `agents: { goose: … }` produces a durable override notice explaining the
    change and the rename escape hatch.
14. The server binds `127.0.0.1` unless the operator opts in explicitly.
15. Full suite green: `cargo test`, `CI=true npm test`, coverage and lint gates per `workflow.md`.

---

## Out of Scope

- Authenticating the remaining dashboard/PTY/chat routes — a separate security track. This track only
  changes the **bind default** and the two new permission routes.
- Permission gating for claude, codex, or goose — none can support it.
- `goose acp` / `opencode acp` as an alternative integration path.
- A Rust style guide (`code_styleguides/rust.md` remains a recorded gap).
- Making Conductor Tracks editable from the frontend (existing `TODO.md` item).
- Redacting secrets from agent-owned session logs beyond what `opencode export --sanitize` provides.
- Retention management of agent-owned session stores.
