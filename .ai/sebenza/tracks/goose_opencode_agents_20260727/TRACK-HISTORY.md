# Track history — task summaries and phase verification reports

`goose_opencode_agents_20260727`

> **Why this file exists.** These records were written as **git notes**, attached to each
> task's commit. A rebase onto updated `main` rewrote those commits, which orphaned 10 of
> the 11 notes — the entire audit trail became unreachable from the branch. Notes do not
> survive history rewrites, so the trail now lives in the tree, where it is reachable,
> diffable, and survives any future rebase. Notes are still attached where they could be
> re-mapped by commit subject; this file is the durable copy.

## test(opencode): add export fixture; correct the --sanitize claim

*commit `015aae9` — orphaned by the rebase; content preserved here*

```
Task: phase-0-task-3 — Capture an opencode export fixture

Method
------
Exported a real session that exercised a bash tool call, both with and without
--sanitize, and diffed the payloads. Committed the plain export as a fixture with
absolute paths neutralised.

Key finding (reverses a design claim)
-------------------------------------
--sanitize redacts message text, tool input, tool output AND tool metadata:

  sanitized: text='[redacted:text:prt_...]'
             tool.output='[redacted:tool-output:prt_...]'
  plain:     text='hello-from-tool'
             tool.output='hello-from-tool\n'
             tool.metadata={'output': ..., 'exit': 0, 'truncated': False}

So --sanitize is for sharing transcripts, not reading them. Sebenza must use the
plain export. The design's claim that --sanitize provided better secret redaction
than Sebenza could implement was wrong for this use case, and the
secrets-adjacency risk is therefore unmitigated - level with claude and codex.

Part -> AgentsUiMessage map (recorded as FR-3.5a)
------------------------------------------------
  text        -> kind=text, text
  reasoning   -> kind=thinking
  tool        -> kind=toolUse (state.input) + kind=toolResult (state.output);
                 tool_name=part.tool; command=state.input.command;
                 exit_code=state.metadata.exit; status=state.status
  step-start/step-finish -> turn boundaries, not rendered

state.metadata.truncated should suppress any "complete output" affordance.

Files changed
-------------
- crates/common/src/adapters/testdata/opencode_export.json  (new fixture)
- .ai/sebenza/tracks/.../spec.md   FR-0.3 findings; FR-3.5 rewritten; FR-3.5a added
- .ai/sebenza/tracks/.../design.md 7 --sanitize corrections; gating capability
                                   softened to type-level-yes/runtime-unconfirmed
```

## docs(sebenza): record phase-0-task-1 finding on opencode session correlation

*commit `1e9e580` — orphaned by the rebase; content preserved here*

```
Task: phase-0-task-1 — Verify opencode worktree-to-project resolution

Method
------
Created a throwaway git repo in the scratchpad with two linked worktrees
(linkedwt, linkedwt2) plus the main checkout. Ran a minimal `opencode run
--format json "Reply with exactly: OK"` in each worktree (opencode 1.18.7,
Google provider). Inspected ~/.local/share/opencode/opencode.db read-only via
python sqlite3 (mode=ro), plus `opencode session list` and `opencode export`.

Findings
--------
1. project_id is PER-REPOSITORY, not per-worktree. Sessions from linkedwt and
   linkedwt2 both landed under project_id f6521a77d0a7…
2. project.worktree held only "linkedwt" — the first-seen directory.
3. project_directory had three rows for that one project: linkedwt, linkedwt2,
   mainrepo.
4. session.directory IS the exact worktree path for each session.
5. `opencode session list` returned BOTH sessions from either worktree and has
   no directory column — project-scoped, cannot correlate.
6. `opencode export <id>` returns {info, messages}; info.directory is the exact
   worktree path. info also carries projectID, version ("1.18.7"), slug, title,
   agent, model, permission, time, cost, tokens.
7. `run --format json` echoes sessionID on every event (step_start, text,
   step_finish) — synchronous id capture, no polling needed.

Why it matters
--------------
The design asserted per-worktree projects and "no repo-level commingling". That
was wrong and would have produced cross-worktree session bleed for any repo with
more than one Sebenza worktree — the common case for this product.

Files changed
-------------
- .ai/sebenza/tracks/goose_opencode_agents_20260727/spec.md
    Added a "Verified findings" section; revised FR-3.5 and FR-3.6.
- .ai/sebenza/tracks/goose_opencode_agents_20260727/design.md
    Correction banner on the Data-architecture correlation paragraph; fixed the
    overview comparison table row, the resolved-questions list, and the open
    questions (opencode OQ1 and OQ2 now resolved).

Outcome: Phase 2 is not blocked. Correlation mechanism changes from
discover-by-directory to record-what-we-started.
```

## refactor(agents): replace Builtin(String) with a BuiltinAgentId enum

*commit `4984db4` — orphaned by the rebase; content preserved here*

```
Tasks: phase-1-task-1 (golden tests) + phase-1-task-2 (BuiltinAgentId) — the agent seam

Committed together: a commit holding only the failing golden tests would leave the
tree red. Both tasks share this SHA.

TDD
---
RED: wrote a GOLDEN argv table referencing BuiltinAgentId ->
     "unresolved import crate::services::agent_registry::BuiltinAgentId".
     The table itself was generated by running the ORIGINAL built_in_invocation via a
     temporary probe test, so it records real pre-refactor output rather than my
     assumptions about it. The probe was then removed.
GREEN: implemented the enum; 74 tests pass, all 12 golden cases byte-identical.

Golden table (pre-refactor output, unchanged after)
--------------------------------------------------
  claude fresh              claude
  claude fresh+yolo         claude --dangerously-skip-permissions
  claude fresh+sys+prompt   claude --append-system-prompt 'be x' -- 'do y'
  claude resume+last        claude --continue
  claude resume+id          claude --resume 'sid'
  claude fork               claude --resume 'fid' --fork-session --session-id 'pin'
  codex  fresh              codex --enable hooks
  codex  fresh+yolo         codex --enable hooks --yolo
  codex  fresh+sys+prompt   codex --enable hooks -c 'developer_instructions=be x' -- 'do y'
  codex  resume+last        codex --enable hooks resume --last
  codex  resume+id          codex --enable hooks resume 'sid'
  codex  fork               codex --enable hooks fork 'fid'

Note codex ignores pin_session_id (it assigns its own id) while claude passes
--session-id. That asymmetry is what the pinnable_session_id capability will encode.

The compiler enumerated the work
--------------------------------
Changing the variant's payload type broke exactly the sites that hardcoded agent
identity, which is the point:
  agent_service.rs:173      built_in_invocation took &str
  lifecycle_service.rs:137  Builtin(id) if id == "claude" string guards
  server.rs:931             Builtin(agent) if agent == "codex"

Also collapsed three independent ["claude","codex"] literals in agent_registry.rs
(builtin_definitions, the custom-agent shadow filter, is_builtin_agent_id) into
BuiltinAgentId::ALL, so the builtin set is declared once.

Deliberately NOT in this commit
-------------------------------
The three new capability fields (fork, pinnable_session_id,
permission_interception), the config_view registry unification, and the frontend
capability wiring are phase-1-task-3 onward. Keeping the payload-type change alone
makes it reviewable and keeps the golden table meaningful.

Files changed
-------------
- crates/common/src/services/agent_registry.rs    (enum + ALL; 3 lists collapsed)
- crates/common/src/services/agent_service.rs     (per-agent split + GOLDEN tests)
- crates/common/src/services/lifecycle_service.rs (enum match)
- crates/sebenza-server/src/server.rs             (enum match)
```

## refactor(agents): derive /api/config agent summaries from the registry

*commit `5c7f8b0` — orphaned by the rebase; content preserved here*

```
Tasks: phase-1-task-5 (tests) + phase-1-task-6 (registry unification)

The finding that justifies the task
-----------------------------------
The unification test failed on its first run with "capability drift for codex".

db25c52 (the previous commit) added fork / pinnable_session_id /
permission_interception to BOTH capability structs. In agent_registry, codex
correctly gets pinnable_session_id = false — it assigns its own session id. In
config_view's copy, the shared `full()` closure gave BOTH builtins true.

Since config_view is what serves /api/config, the picker was being told codex
supports session-id pinning when it does not — a wrong capability on the wire,
introduced one commit after the field existed. The duplicate drifted essentially
immediately, which is a better argument for this task than the design's prose was.

What changed
------------
- Deleted config_view::AgentCapabilities.
- AgentSummary.capabilities is now agent_registry::AgentCapabilitiesWire
  (+Debug +Clone added to it).
- list_agent_summaries maps list_agent_details, so builtin set, ordering, the
  custom-agent shadow filter and capability values all come from one place.
- config_view.rs now has zero hardcoded agent ids.
- Promoted the ProjectConfig builder into agent_registry::tests_support
  (minimal_config / custom_agent) so other modules' tests need not hand-roll a
  30-field struct.

Two endpoints, one source
-------------------------
  /api/config  -> build_app_config -> list_agent_summaries  ┐
                                                            ├─ list_agent_definitions
  /api/agents  -> list_agent_details                        ┘

Tests assert they agree on ids, order, labels, kind and serialized capabilities,
and that builtin summaries derive from BuiltinAgentId::ALL rather than a local list.

Effect on opencode visibility
-----------------------------
This removes the first of the two gates. The picker will list opencode as soon as
BuiltinAgentId gains an Opencode variant (phase-2-task-2) — no frontend change and
no further config_view change required.

cargo test: 79 passed. Frontend: 139 passed, tsc clean.

Files changed
-------------
- crates/common/src/services/config_view.rs    (duplicate deleted, derived instead)
- crates/common/src/services/agent_registry.rs (+Debug/+Clone, tests_support module)
```

## fix(server): route interrupt and streaming through the worktree's own agent

*commit `686e253` — orphaned by the rebase; content preserved here*

```
Task: phase-0-task-4 — Investigate unconditional claude_conversation_service dispatch

Verdict: it WAS a latent bug. Codex interrupt and live streaming were inoperable.

Root cause
----------
agent_stream keys in-flight runs by conversation_id (HashMap<String, Run>).
- prepare_agent_send DOES dispatch per agent (server.rs 1309/1314), registering a
  Codex run under a Codex-derived id (<session-uuid> or codex-pending:<path>).
- agents_interrupt (1382) and the agents-streaming WebSocket (~1522) called
  claude_conversation_service unconditionally. For a Codex worktree that resolves
  claude-pending:<path>, which never matches the registered key.
Result: interrupt() always returned None -> 409 "No active Claude response to
interrupt"; the streaming socket resolved the wrong conversation.
AgentCapabilities.interrupt = true for codex was therefore inaccurate.

TDD
---
RED: new common::services::conversation_router with a stub returning None; the two
routing tests failed on assertion (1 passed trivially).
GREEN: implemented the per-agent dispatch; 3/3 pass. Full suite 66 passed.

Note the tests assert on ConversationState.provider, not conversation_id. First
attempt asserted claude-pending:/wt and got 991dce80-... instead, which exposed a
second bug (below) and showed claude ids are not deterministic under test.

Incidental finding — NOT fixed
------------------------------
claude_cli::latest_session (claude_cli.rs ~460-467) falls back to scanning ALL
project dirs for the newest .jsonl when the requested cwd has none. So a freshly
created claude worktree can display another worktree's or project's transcript.
Same-user so not a confidentiality breach, but a correctness bug and misleading.
Left alone deliberately: out of scope, and the fallback may be intentional
"continue last session" behaviour, so changing it needs its own decision. The new
opencode adapter must not copy the pattern - FR-3.6's record-what-we-started
design already avoids it.

Toolchain note
--------------
cargo fmt --check reports 406 pre-existing diffs on the unmodified tree, so it
cannot serve as a gate without a dedicated reformatting chore. Clippy has 31
pre-existing warnings; none in the code added here.

Files changed
-------------
- crates/common/src/services/conversation_router.rs  (new, with tests)
- crates/common/src/services/mod.rs                  (register module)
- crates/sebenza-server/src/server.rs                (both call sites + 409 text)
- .ai/sebenza/tracks/.../spec.md                     (FR-0.4 + 2 further findings)
```

## spike(opencode): permission.ask does not fire - phase 3 gating is not buildable

*commit `9070b3a` — in branch history*

```
Task: phase-3-task-1 — Spike: can permission.ask fire and be honoured?

VERDICT: NO. The phase-3 hard gate has tripped.

Method
------
Earlier attempts (phase-0-task-2) only exercised `opencode run`, which ruled out the
non-interactive paths but could not answer the question. This spike used the mode Sebenza
actually launches: an interactive TUI in a real tmux session.

  tmux new-session -d -s ocspike -c <worktree>
  send-keys "opencode"                       # interactive TUI, not `run`
  worktree config: permission { bash: "ask" }
  probe plugin logging every hook, setting output.status = "allow"
  send-keys "Run the shell command: echo spike-ok"

opencode displayed its own dialog: "Permission required - Allow once / Allow always /
Reject". The probe log across the whole exchange:

  plugin-loaded
  tool.before bash          <- tool.execute.before fires BEFORE the permission decision
  event permission.asked    <- generic `event` hook; observational only
  event permission.replied  <- after pressing Enter in the TUI
  tool.after bash

permission.ask NEVER fired.

Version note: the binary is 1.18.9; the installed @opencode-ai/plugin types are 1.18.7 and
still declare "permission.ask"?: (input, output: { status: "ask"|"deny"|"allow" }). So this
is either a binary/types skew or a path the hook no longer serves. Either way it cannot be
relied on.

Consequence for phase 3
-----------------------
The design's gating feature is a synchronous, authenticated decision channel: the plugin
awaits a verdict and writes it into output.status. With permission.ask never called there
is nothing to write into. Tasks 2-11 of phase 3 are NOT buildable as designed.

This is a scope decision (redesign vs descope), so I stopped rather than improvise a
substitute - e.g. driving opencode's TUI by injecting keystrokes, which would be
fragile, invisible to the user, and a poor foundation for a security control.

permission_interception remains false for opencode. No agent of the four can gate a tool
call, which removes the asymmetry the design was built around and was the original
justification for pulling gating into this track.

What was salvaged (implemented in this commit)
---------------------------------------------
The observational events are genuinely useful, so they are now wired:
  permission.asked   -> report the worktree IDLE
  permission.replied -> back to running

This also FIXES A BUG shipped in phase 2: tool.execute.before fires before the permission
decision, so a worktree blocked on a human was reporting "running". For a product built to
supervise many parallel worktrees, "which one is waiting for me" is arguably the most
valuable signal available here, and it needs no gating.

Also corrected comments in the generated plugin that implied permission.ask might work.

Files changed
-------------
- crates/common/src/adapters/agent_runtime.rs        (plugin: permission.asked/replied)
- crates/common/src/adapters/testdata/sebenza-agentctl.py (two new subcommands)
- .ai/sebenza/tracks/.../spec.md                     (FR-0.2 resolved, negative)
```

## docs(sebenza): record that opencode permission gating is not buildable

*commit `a5755aa` — in branch history*

```
PHASE 3 VERIFICATION REPORT — "Permission visibility for opencode (redesigned)"
═══════════════════════════════════════════════════════════════════

Automated: cargo test 160 passed, frontend 186 passed, tsc clean, 0 build warnings.

This phase was REDESIGNED mid-flight. Its hard gate (phase-3-task-1) returned a negative
result, so permission ENFORCEMENT was replaced by permission VISIBILITY. 11 tasks -> 5.

Tasks
-----
  1    f4c9b53  Spike: permission.ask never fires on opencode 1.18.9
  2+3  d990faa  AwaitingPermission lifecycle end to end (+2 safety tests)
  4    a5755aa  Corrected every gating claim in design.md/spec.md; TODO added

The spike
---------
Drove an interactive opencode TUI in a real tmux session - the mode Sebenza actually
launches - with `permission: {bash: "ask"}` and a probe plugin attempting
`output.status = "allow"`. opencode displayed its own Allow/Reject dialog and emitted
`permission.asked` then `permission.replied` on the GENERIC `event` hook. The named
`permission.ask` hook never fired, though @opencode-ai/plugin 1.18.7 still declares it
with a mutable status. Binary/types skew, or a path the hook no longer serves.

Two findings worth keeping
--------------------------
1. tool.execute.before fires BEFORE the permission decision, so a worktree blocked on a
   human was reporting "running". Now AwaitingPermission -> "needs approval", distinct
   from plain "waiting". For a product built to supervise many parallel worktrees, "which
   one wants me, and why" is the signal that matters - and it needed no gating.
2. The oneshot watcher would have AUTO-CLOSED a worktree blocked on a permission prompt,
   killing the agent mid-task. AwaitingPermission is deliberately neither terminal nor
   grace-eligible, pinned by a test.

Not built, deliberately
-----------------------
permission_interception is false for ALL FOUR agents. No synchronous decision channel, no
authenticated verdict, no pending-decision store. design.md retains that design as the
record of what was attempted and why it was abandoned - including the credential trap
(the gated agent holds SEBENZA_CONTROL_TOKEN, so submit and resolve need asymmetric
credentials) which is the part worth not re-deriving.

Manual verification (proposed and confirmed by the user)
--------------------------------------------------------
1. While opencode shows its Allow/Reject dialog the dashboard reads "needs approval",
   not "running" and not plain "waiting"; it clears after answering
2. No approve/deny control exists anywhere in the UI
3. /api/config and /api/agents agree; all three agents listed; permissionInterception
   false for every one
4. claude and codex unaffected

User confirmation: "works" (2026-07-28)
```

## feat(server): bind loopback by default, with explicit opt-in for other interfaces

*commit `afa558a` — orphaned by the rebase; content preserved here*

```
Tasks: phase-0-task-5 (tests) + phase-0-task-6 (implementation) — loopback-default bind

Committed together deliberately: workflow.md gives each task its own commit, but
committing a failing test alone would leave the tree red at that commit. Both tasks
share this SHA.

Why
---
main.rs:112 hardcoded SocketAddr::from(([0,0,0,0], port)). server.rs has exactly one
authenticated route (/api/runtime/events, Bearer control-token); worktree creation,
terminal PTY, chat, interrupt and the agents WebSocket are unauthenticated. The old
default therefore exposed full local-user capability to any reachable host.

Change
------
- New --host flag and $SEBENZA_HOST env on `sebenza-server serve`.
- resolve_bind_addr(flag, env, port): precedence flag > env > 127.0.0.1; an
  unparseable host warns and falls back to loopback rather than binding something
  unintended.
- Non-loopback binds log a warning naming what is exposed.

TDD
---
RED: 5 tests added; compile error at the `Command::Serve { port }` match (pattern
did not mention `host`).
GREEN: wired host through serve(); 5/5 pass. Suite: 71 Rust, 139 frontend.

Empirical verification (not just unit tests)
--------------------------------------------
  default             -> LISTEN 127.0.0.1:5137
  --host 0.0.0.0      -> LISTEN 0.0.0.0:5141  + "NOT loopback" warning
  SEBENZA_HOST=0.0.0.0 -> LISTEN 0.0.0.0:5142 + warning
The env path matters because `sebenza-cli serve` spawns the daemon without
env_clear(), so SEBENZA_HOST is inherited.

Behaviour change
----------------
Users who reach the dashboard from another machine must set SEBENZA_HOST=0.0.0.0 or
pass --host after upgrading. Documented in README ("Network exposure", with a
changed-behaviour callout) and tech-stack.md.

Known gap (deliberate)
----------------------
`sebenza-cli serve` has no --host flag. Its root argument parser is hand-rolled
rather than clap-derived, so adding one touches shared parsing logic - more invasive
than this Phase 0 prerequisite justifies. $SEBENZA_HOST covers the need today; the
flag is recorded as a follow-up.

Files changed
-------------
- crates/sebenza-server/src/main.rs  (--host, resolve_bind_addr, warning, 5 tests)
- README.md                          (Network exposure section, env table)
- .ai/sebenza/tech-stack.md          (default bind documented)

═══════════════════════════════════════════════════════════════════
PHASE 0 VERIFICATION REPORT — "Verification & prerequisites"
═══════════════════════════════════════════════════════════════════

Automated verification
----------------------
Command: cargo test && cd frontend && CI=true npm test
Result:  71 Rust tests passed (8 new), 139 frontend tests passed, 0 failures.

Test coverage for phase changes
-------------------------------
  crates/common/src/services/conversation_router.rs  3 tests (new module)
  crates/sebenza-server/src/main.rs                  5 tests (new module)
  crates/sebenza-server/src/server.rs                no test module (pre-existing);
      its dispatch logic was extracted into conversation_router and is covered there
  crates/common/src/services/mod.rs                  registration only, no logic
  crates/common/src/adapters/testdata/opencode_export.json
      fixture; consumed by phase-2 parser tests

Tasks completed
---------------
  phase-0-task-1  1e9e580  opencode worktree->project resolution
  phase-0-task-2  f055c7f  permission.ask under --auto (partial; residue -> phase-3-task-1)
  phase-0-task-3  015aae9  export fixture + --sanitize correction
  phase-0-task-4  686e253  latent dispatch bug (was real; fixed)
  phase-0-task-5  afa558a  loopback bind tests
  phase-0-task-6  afa558a  loopback bind implementation

Three findings overturned design assertions
-------------------------------------------
1. opencode project_id is per-REPOSITORY, not per-worktree. The design claimed each
   Sebenza worktree becomes its own opencode project and that commingling "does not
   occur". Two linked worktrees shared one project_id. Correlation redesigned onto
   session.directory / export->info.directory (FR-3.6).
2. `--sanitize` must NOT be used for history. It redacts message text, tool input,
   output and metadata. The design had called it better redaction than Sebenza could
   implement; in fact it destroys the payload the chat UI needs (FR-3.5).
3. Codex interrupt and live streaming were genuinely broken - agents_interrupt and
   the agents WebSocket resolved a claude-derived conversation id that never matched
   the key a Codex run registers under. AgentCapabilities.interrupt=true was
   inaccurate for codex. Fixed via conversation_router.

Manual verification (proposed and confirmed by the user)
--------------------------------------------------------
1. Default bind is loopback:      ss -ltn | grep 5111 -> LISTEN 127.0.0.1:5111
2. Opt-in works and warns:        --host 0.0.0.0 -> LISTEN 0.0.0.0:5111 + "NOT loopback"
3. Dashboard still loads normally via `sebenza-cli serve`
4. Codex interrupt now succeeds instead of "No active Claude response to interrupt"

User confirmation: "yes" (2026-07-28)

Behaviour change shipped
------------------------
The server now binds 127.0.0.1 by default. Anyone reaching the dashboard from
another machine must set SEBENZA_HOST=0.0.0.0 or pass --host. Documented in README
("Network exposure") and tech-stack.md.

Residual items recorded, deliberately not fixed
-----------------------------------------------
- claude_cli::latest_session falls back to the newest session across ALL project
  dirs, so a fresh claude worktree can display another worktree's transcript.
  Possibly intentional "continue last session" behaviour; needs its own decision.
- `sebenza-cli serve` has no --host flag (hand-rolled root arg parser);
  $SEBENZA_HOST works via env inheritance.
- cargo fmt --check reports 406 pre-existing diffs on the unmodified tree and
  clippy 31 pre-existing warnings, so neither can gate this repo without a
  dedicated reformatting chore. No new warnings were introduced.
```

## feat(security): detect agent plugin code Sebenza did not write

*commit `ca3359e` — orphaned by the rebase; content preserved here*

```
═══════════════════════════════════════════════════════════════════
PHASE 2 VERIFICATION REPORT — "opencode at full parity (+ shared controls)"
═══════════════════════════════════════════════════════════════════

Automated: cargo test 99 passed, frontend 142 passed, tsc clean, 0 build warnings.
Live on the wire: opencode = conversationHistory, fork, pinnableSessionId, resume, terminal

Tasks completed
---------------
  1+2     812ba79  BuiltinAgentId::Opencode; appears in the picker (user confirmed launch)
  3+4+8+9 1ea4152  generated plugin, agentctl event mapping, git-exclusion path list
  5+6     58f4395  `opencode export` parser + conversation service
  7       f7270a7  session-id round trip; conversation_history -> true
  12-15   8d4ca75  docker mounts, shadow notice, auto-naming, version reporting
  10+11   ca3359e  untrusted-plugin DETECTION (partial)

Not complete, deliberately
--------------------------
FR-5.3 (block auto-launch pending a remembered per-repo confirmation) is NOT done.
Task 11 delivers detection only; it warns at launch. Blocking needs a confirmation UI
and a persisted decision store. The two subtle parts of detection ARE done and tested:
comparison against a STORED hash of the bytes actually written (a recomputed
expectation would flag every worktree on the next Sebenza upgrade, training
click-through), and the record living under the git dir so an agent in the worktree
cannot forge its own approval.

in_app_chat remains false for opencode: live chat needs a StreamProvider, left as None.
opencode has terminal + history + resume + fork.

Findings during the phase
-------------------------
1. opencode moved 1.18.7 -> 1.18.9 DURING this work, visible via init's new version
   reporting. Everything was verified against 1.18.7 - the churn risk the design
   flagged, demonstrating itself within a day.
2. opencode needs TWO docker mounts where claude/codex need one each. Those two keep
   config AND session data under one directory, which is the only reason docker history
   works for them today. opencode separates them, so both are required.
3. opencode has no system-prompt flag. Dropped for interactive launches (recorded as a
   limitation); folded into the message for one-shot auto-naming, where it is harmless.

Manual verification (proposed and confirmed by the user)
--------------------------------------------------------
1. opencode history populates after a terminal prompt, with tool cards and exit codes
2. Shadow notice fires for a re-added `opencode:` custom entry
3. Untrusted-plugin warning names a repo-supplied plugin and not sebenza.js
4. claude and codex unaffected

User confirmation: "lets continue" (2026-07-28)
```

## feat(agents): add fork, pinnableSessionId and permissionInterception capabilities

*commit `db25c52` — orphaned by the rebase; content preserved here*

```
Tasks: phase-1-task-3 (tests) + phase-1-task-4 (capability fields)

Committed together: this change spans Rust and the Zod contract, which must move
in lockstep or /api/config stops parsing. A test-only commit would also leave the
tree red.

Capability matrix (verified, not assumed)
----------------------------------------
                          claude  codex  custom
  fork                    true    true   false
  pinnable_session_id     true    false  false
  permission_interception false   false  false

pinnable_session_id is the interesting one: the phase-1 golden argv table showed
claude passes --session-id on fork while codex ignores pin_session_id entirely
(it assigns its own id, hence capture_new_session_id's 20x150ms poll). That
asymmetry is now data rather than a hardcoded branch at lifecycle_service.rs:791.

permission_interception is false everywhere today. It exists so the UI can be
honest about which agents Sebenza can actually gate - and so the phase-3 spike has
a flag to set if permission.ask proves drivable.

TDD
---
RED: three tests in a new agent_registry test module ->
     "no field `fork` on type AgentCapabilities" (x6).
GREEN: fields added to the domain struct, both constructors, and the wire type.
     77 Rust tests pass. The serialization test asserts camelCase on the wire
     (fork / pinnableSessionId / permissionInterception), since the Rust field
     names differ from the contract's.

Coordinated contract change
---------------------------
AgentCapabilitiesSchema fields are non-optional z.boolean(), so every producer
must set them explicitly. tsc immediately flagged 6 fixture sites in
App.test.tsx and SettingsDialog.test.tsx.

Rather than paste three fields into each, added
frontend/src/lib/api-contract/test-fixtures.ts exposing agentCapabilities() and
builtinAgentCapabilities(). Fixtures now declare only what the test cares about,
e.g. agentCapabilities({ resume: true }). The next capability addition edits one
file instead of six.

Also had to update
-------------------
config_view.rs's duplicate AgentCapabilities struct (its own 5-field copy). It is
what actually feeds /api/config and therefore the agent picker, so leaving it
alone would have broken schema parse. phase-1-task-6 deletes the duplicate; until
then the fields exist in two places, which is precisely the drift hazard that task
exists to remove.

Files changed
-------------
- crates/common/src/services/agent_registry.rs   (fields, matrix, 3 tests)
- crates/common/src/services/agent_service.rs    (test helper)
- crates/common/src/services/config_view.rs      (duplicate struct)
- frontend/src/lib/api-contract/schemas.ts       (AgentCapabilitiesSchema)
- frontend/src/lib/api-contract/test-fixtures.ts (new factories)
- frontend/src/App.test.tsx, src/lib/SettingsDialog.test.tsx (use factories)
```

## feat(ui): gate agent affordances on declared capabilities, not agent ids

*commit `f7dc21f` — orphaned by the rebase; content preserved here*

```
═══════════════════════════════════════════════════════════════════
PHASE 1 VERIFICATION REPORT — "Agent abstraction & capability model"
═══════════════════════════════════════════════════════════════════

Automated verification
----------------------
Command: cargo test && cd frontend && CI=true npm test
Result:  80 Rust passed, 142 frontend passed, tsc clean, 0 build warnings.

Acceptance criterion: this phase is a PURE REFACTOR. No new agents; claude and
codex behaviour unchanged. Held.

Tasks completed
---------------
  1+2   4984db4  BuiltinAgentId enum + per-agent invocation split
  3+4   db25c52  fork / pinnableSessionId / permissionInterception (Rust + Zod)
  5+6   5c7f8b0  registry unification
  7+8   83ccd3d  registry-resolved dispatch across server.rs
  9+10  f7dc21f  capability-driven frontend affordances

Test coverage for phase changes
-------------------------------
  agent_registry.rs      3 tests (capability matrix, custom-false, wire camelCase)
  agent_service.rs       6 tests (incl. the 12-case GOLDEN argv table)
  config_view.rs         2 tests (cross-endpoint agreement, derived builtins)
  agent_stream.rs        1 test  (exhaustive StreamProvider mapping)
  conversation_router.rs 3 tests (from phase 0, cover the routing this phase reuses)
  frontend               WorktreeConversationPanel 17 cases (3 new capability-gating)

Two findings worth recording
----------------------------
1. REAL DRIFT CAUGHT. The unification test failed on first run:
   "capability drift for codex". db25c52 had added the three capabilities to both
   structs and got codex's pinnable_session_id wrong in config_view's copy (true
   there, false in the registry). config_view serves /api/config, so the picker was
   being told codex supports session-id pinning when it does not. The duplicate
   drifted within minutes of the field existing.
2. FRONTEND BUG FIXED INCIDENTALLY. WorktreeConversationPanel's label fallback was
   `agentName === "claude" ? "Claude" : "Codex"`, so ANY unknown agent was labelled
   "Codex". Now falls back to the agent's own id.

The enum paid for itself
------------------------
Changing Builtin(String) -> Builtin(BuiltinAgentId) broke exactly the three sites
that hardcoded agent identity (agent_service.rs:173, lifecycle_service.rs:137,
server.rs:931). server.rs now contains ZERO Some("claude")/Some("codex") matches.

Manual verification (proposed and confirmed by the user)
--------------------------------------------------------
1. claude and codex behave as before: picker lists both, chat opens, fork button present
2. Fork button follows capabilities.fork (present for both builtins, absent for custom)
3. Chat fails closed for a custom agent, with a stated reason
4. /api/config and /api/agents report identical capabilities; codex shows
   pinnableSessionId: false

User confirmation: "continue" (2026-07-28)

Deliberate exceptions, commented in code
----------------------------------------
oneshot.rs:564 and MobileChatSurface.tsx:326 still test for claude specifically.
Both compensate for gaps in Claude's LIVE STREAM rather than expressing a declared
capability, and whether another agent needs the same compensation can only be judged
by watching that agent's stream. Revisit when opencode streaming lands. Inventing a
capability flag for an uncharacterised quirk would have been speculative.

Effect on opencode
------------------
The config_view unification removed the FIRST of the two gates on opencode appearing
in the Create Worktree picker. The second is the BuiltinAgentId::Opencode variant,
next up in phase-2. No further frontend or config_view change is required.
```

