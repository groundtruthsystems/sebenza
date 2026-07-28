# TODO

## opencode permission gating — revisit if `permission.ask` starts firing

Sebenza can currently **observe** opencode permission prompts but not answer them. Verified
2026-07-28 against opencode **1.18.9** by driving an interactive TUI in tmux with a probe
plugin: opencode showed its own *Allow once / Allow always / Reject* dialog and emitted
`permission.asked` then `permission.replied` on the **generic `event` hook**, never calling
the named `permission.ask` hook.

`@opencode-ai/plugin` **1.18.7** still declares it:

```ts
"permission.ask"?: (input: Permission, output: { status: "ask" | "deny" | "allow" }) => Promise<void>
```

So this is either a binary/types skew or a path the hook no longer serves.

**What ships today:** `permission.asked` puts the worktree into a distinct
`AgentLifecycle::AwaitingPermission` so the dashboard shows *"needs approval"* rather than a
generic "waiting"; `permission.replied` clears it. `permission_interception` is `false` for
all four agents.

**If upstream restores it**, the design for a full approve/deny channel is already written up
in the track's `design.md` (Application §9) — including the part that is easy to get wrong:
the gated agent holds `SEBENZA_CONTROL_TOKEN` via `control.env`, so a single shared
credential would let it approve its own request. Submit and resolve need **asymmetric**
credentials, with the resolver secret delivered only over the WS push or CLI response.

**Check on an opencode upgrade:** re-run the tmux probe. If `permission.ask` fires, the
gating work becomes viable and `permission_interception` can flip to true.

## goose as a built-in agent — investigate later (deferred)

goose was researched and verified (v1.8.0) as part of the opencode track
(`.ai/sebenza/tracks/goose_opencode_agents_20260727/`), then **descoped**. opencode ships
first; goose is deferred to its own track. The full findings are preserved in that
track's `design.md` — start there rather than re-researching.

**What was already established (verified by direct observation, not docs):**

- **Hooks** — shell commands via the [Open Plugins](https://open-plugins.com) spec.
  Project scope `<worktree>/.agents/plugins/<name>/hooks/hooks.json`, auto-discovered at
  startup with **no enable flag** (unlike codex's `--enable hooks`). Because Sebenza would
  own the `sebenza` plugin subtree, **no merge strategy is needed** — plain overwrite,
  unlike the shared `.claude/settings.local.json` / `.codex/hooks.json` files.
- **Events** — `SessionStart`, `SessionEnd`, `Stop`, `UserPromptSubmit`, `PreToolUse`,
  `PostToolUse`, `PostToolUseFailure`, `BeforeReadFile`, `AfterFileEdit`,
  `BeforeShellExecution`, `AfterShellExecution`. Hook stdin JSON carries
  `{event, session_id, tool_name, tool_input, working_dir}`.
- **Session history** — `~/.local/share/goose/sessions/<id>.jsonl`. Line 1 is a header
  (`working_dir`, `description`, `message_count`, `total_tokens`, plus undocumented extras);
  subsequent lines are `{id, role, content, created}` where **`content` is a structured
  block array** (`text`, `toolRequest{id,toolCall}`, `toolResponse{id,toolResult{status}}`).
  Full tool-call fidelity is achievable — no text-only fallback needed.
- **Session id is pinnable** via `goose session -n NAME`, so `capture_new_session_id`
  polling is unnecessary (better than codex).
- **`message_count` is only safe as a zero-vs-nonzero check.** Verified: **19 of 99** real
  local sessions have `message_count` *below* their true message-line count. Exact-match
  validation would misclassify ~1 in 5 sessions as broken.
- **One-shot** — `goose run -t TEXT --system TEXT --no-session`, which fits
  `llm_spawn.rs::build_llm_args` and `init_authoring.rs` directly.
- **Bypass is `GOOSE_MODE=auto`, an environment variable, not a flag** — so it cannot use
  the existing string-append yolo pattern; route it through `runtime.env`.

**The blocking caveat, and why it deserves a deliberate decision rather than a quick port:**

goose hooks are **observe-only and non-blocking — they cannot deny a tool call.** There is
no analogue of opencode's `permission.ask`. So `GOOSE_MODE=auto` removes goose's own last
human checkpoint with nothing able to intercept, and the permission gating built for
opencode **cannot** be extended to goose. Any goose track must decide whether to offer
`auto` at all, and must disclose the asymmetry rather than reusing the opencode UI.

**Still unverified:**

- Does `maybe_send_pr_opened` work unmodified against a real goose `PostToolUse` payload?
- Is `goose session -n NAME` usable as a **fork** primitive (a new session branched off
  existing history), or only resume/rename? Determines whether `fork` can be `true`.
- Does goose honour XDG on macOS, or use `~/Library/Application Support`?
- Root cause of the `message_count` drift (interrupt handling? retries?).

**What the opencode track leaves ready:** the `BuiltinAgentId` enum and capability model,
the unified builtin registry, registry-resolved dispatch, capability-driven frontend gating,
git-exclusion as a path list, `0600` env files, the untrusted-plugin scan (data-driven path
set), and the docker mount pattern. Adding goose should be an adapter, not a fork. Note
goose's `.ai/sebenza.example.yaml` custom-agent entry was **intentionally left in place**,
so it keeps working as a terminal-only custom agent until then.

## Sebenza registry — write operations (not started)
`/registry` reads `~/.ai/sebenza/registry.json` only; the plugin owns writes.
If the dashboard should manage it: deregister an entry, batch stale-entry
cleanup, and `last_synced` refresh on read (per the plugin's `shared/daemon.md`).
