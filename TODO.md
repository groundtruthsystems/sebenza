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

**If upstream restores it**, here is the design, including the part that is easy to get
wrong. (Inlined from the completed track's `design.md`, which has since been deleted.)

> **The obvious credential model is broken — split it.**
>
> `build_control_env_map` writes `SEBENZA_CONTROL_TOKEN` in plaintext into `control.env`,
> which the agent pane sources — that is precisely how `sebenza-agentctl` authenticates.
> **So the gated opencode process holds the control token.**
>
> If one shared token authenticates both *submitting* and *resolving* a permission request,
> the process being gated can **approve its own request**: it has the token, and it knows
> the request id it just created. A prompt-injected tool call, or a tainted binary, calls
> `resolve(request_id, "allow")` and it is indistinguishable from a dashboard click. That
> reduces the control to one holding only against a *cooperative* agent — i.e. not a
> security boundary against the threats that motivate it.
>
> **Required: asymmetric credentials.**
> - **Submit** may use the existing control token (the pane legitimately needs it).
> - **Resolve** must require a **per-request resolver secret minted server-side and
>   delivered only in the WS push payload or the CLI response** — never written to
>   `control.env`, `runtime.env`, or any file the agent process can read. Or: accept
>   resolutions only over the authenticated WS connection that received the push.
>
> Same tamper principle as the untrusted-plugin scan: *state Sebenza uses to make a trust
> decision must not be reachable by the process it is deciding about.*

Other pieces the design settled: fail **closed** on timeout/error/disconnect (a gate that
fails open is worse than none); the server's timeout is authoritative, not the plugin's; cap
concurrent pending requests per session; and mirror `agent_stream.rs`'s
`AgentStreamManager`/`RunState` idiom (a map of ids to `oneshot` senders) rather than
inventing a pending-decision store.

**Check on an opencode upgrade:** re-run the tmux probe. If `permission.ask` fires, the
gating work becomes viable and `permission_interception` can flip to true.

## goose as a built-in agent — investigate later (deferred)

goose was researched and verified (v1.8.0) as part of the opencode track, then
**descoped**; opencode shipped first. That track is complete and its folder has been
deleted, so **everything needed to start is below** — it is the record, not a pointer to
one. Verified by direct observation against goose 1.8.0, not from documentation.

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

**What the completed opencode track leaves ready:** the `BuiltinAgentId` enum and capability model,
the unified builtin registry, registry-resolved dispatch, capability-driven frontend gating,
git-exclusion as a path list, `0600` env files, the untrusted-plugin scan (data-driven path
set), and the docker mount pattern. Adding goose should be an adapter, not a fork. Note
goose's `.ai/sebenza.example.yaml` custom-agent entry was **intentionally left in place**,
so it keeps working as a terminal-only custom agent until then.

## Sebenza registry — write operations (not started)
`/registry` reads `~/.ai/sebenza/registry.json` only; the plugin owns writes.
If the dashboard should manage it: deregister an entry, batch stale-entry
cleanup, and `last_synced` refresh on read (per the plugin's `shared/daemon.md`).
