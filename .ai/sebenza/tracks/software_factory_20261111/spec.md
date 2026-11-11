# Spec: Sebenza Software Factory — solution level and skills move

Refines [design.md](./design.md), the five architecture documents and [test-plan.md](./test-plan.md).
Its sibling track [factory_project_level_20261111](../factory_project_level_20261111/spec.md) owns the project level (UC-07..UC-11).

## Overview
This track moves the Sebenza plugin into `skills/` with no change in behaviour. It then builds the solution level of the software factory:
- a solution document with Coordinator / Design / Graph tabs;
- a coordinator chat on Claude, Codex or Grok;
- the analyst → architect → independent reviewer pipeline, capped at 3 rounds, with each escalated problem raised as its own request;
- operator-only, hash-bound approval of the design and of each project's worktree handover.

## Functional Requirements
- FR-01 `skills/` is the plugin root with the seven skills, agents, shared, scripts and hooks moved from gts-agent-extensions unchanged.
- FR-02 A repo-root `.claude-plugin/marketplace.json` names marketplace `sebenza` and plugin `sebenza`, source `./skills`; both names are stable.
- FR-03 `skills/MIGRATION.md` records the source repo, commit, file listing and every post-move change.
- FR-04 CI runs `cargo test`, frontend tests and `python3 -m unittest discover -s skills/scripts`; `workflow.md` lists the python command.
- FR-05 `install.sh --with-plugin` adds the marketplace and installs the plugin; `sebenza-cli init` reports plugin presence.
- FR-06 Drafts gain schema v4 `kind` (note | solution) and `factory` overrides, stamped only when needed.
- FR-07 New events (NoteUpserted, CoordinatorTurn, SolutionPhaseChanged, ReviewCycleStarted, ReviewRoundRecorded, ProblemRaised, DesignApproved, Handover*) fold into SolutionState.
- FR-08 `factory:` config holds coordinator, designer and solution/project reviewer profiles; per-item overrides take precedence.
- FR-09 Reviewer independence: a different agent, or the same agent with a different, at-least-as-capable, non-lightweight explicit model; else 422 with reason.
- FR-10 An operator session token, issued to the SPA same-origin and to the CLI via UI-confirmed login, alone opens every operator route.
- FR-11 Every gated action writes an attributed Overall-thread audit comment.
- FR-12 Role jobs run read-only on Claude, Codex or Grok per a conformance-backed profile table, returning schema-validated JSON.
- FR-13 Role prompts are embedded at build time from `skills/agents/*.md`; new agents: solution-coordinator, system-analyst, solutions-architect, solution-reviewer.
- FR-14 Agent output is secret/PHI-scanned before persisting; operator chat input warns.
- FR-15 The coordinator chat streams per turn, persists a transcript and applies note proposals badged `coordinator`.
- FR-16 Only the operator's start runs analyst then architect; the architecture names affected registered projects; unregistered names become problems.
- FR-17 The reviewer runs up to 3 rounds per cycle with revisions between, shown "round n of 3"; round 4 returns 409 `round_limit`.
- FR-18 No agreement after round 3 raises one request per problem; each resolves via a confirmed coordinator proposal or a hand edit plus resolve.
- FR-19 On `agreed` or escalation the coordinator posts a report; the operator approves the design, bound to SHA-256 document hashes.
- FR-20 Approval drafts an editable instruction per affected project; each is approved (hash-bound, creating one worktree with `solution-handover.md`) or deferred.
- FR-21 Any later edit voids an approval; convert of a solution item without approvals returns 409 `approval_required`.
- FR-22 Inbox items open on Coordinator / Design / Graph tabs; legacy items keep notes, graph (incl. `%% sebenza:layout`) and requests.
- FR-23 The agent picker lists Claude, Codex, Grok; uninstalled agents are disabled; Grok carries a "read-only unverified" badge and is refused as reviewer with a reason.
- FR-24 Reviewer settings are editable globally and per item in the UI and through `sebenza-cli`.

## Non-Functional Requirements
- NFR-01 Chat turn timeout is 180 s and design/review jobs 600 s. One slot is reserved for chat when `maxConcurrent` ≥ 2.
- NFR-02 No real agent CLI runs in the default test path. Conformance tests skip when the CLI is absent.
- NFR-03 Coverage on new code is above 80%.
- NFR-04 Every new view renders through DOMPurify and mermaid strict.

## Acceptance Criteria
- AC-01 (FR-01..03; AC-M.0, AC-M.1) The move is behaviour-neutral, the marketplace validates, and the tree matches `MIGRATION.md` — TS-63, TS-34, TS-35, TS-36, TS-46.
- AC-02 (FR-13; AC-M.2) Embedded prompts equal their `skills/agents` files — TS-37.
- AC-03 (FR-06..09) The fold, v3 compatibility and the independence rule hold — TS-02, TS-06, TS-18, TS-19, TS-53, TS-54.
- AC-04 (FR-10, FR-11) Operator routes reject the shared token, the token never leaks, and approvals audit — TS-23, TS-38, TS-48, TS-50, TS-51, TS-58, TS-62.
- AC-05 (FR-12, FR-14) Role jobs are read-only, inert to injection, scanned and bounded — TS-11, TS-39, TS-41, TS-43, TS-45, TS-47, TS-52, TS-61.
- AC-06 (FR-15) Chat streams, persists, survives an agent switch, and badges proposals — TS-04, TS-05, TS-07.
- AC-07 (FR-16) Design starts only on the operator's start; affected projects are validated — TS-08, TS-09, TS-10.
- AC-08 (FR-17) **Solution review is capped at 3 rounds, visible as "round n of 3", and round 4 is refused** — TS-12, TS-13, TS-44.
- AC-09 (FR-18) **One request per escalated problem**, each resolvable individually — TS-14, TS-15, TS-16.
- AC-10 (FR-19..21) **Design and each project's worktree need explicit, hash-bound operator approval**; edits void them — TS-20, TS-21, TS-22, TS-24, TS-60.
- AC-11 (FR-22..24) Tabs, picker, Grok badge, settings, safe rendering and legacy flows work — TS-01, TS-03, TS-17, TS-49, TS-55, TS-56.

## Out of Scope
- Project-level reviewer, skill extensions and code-review rounds (sibling track).
- Per-worktree agent tokens and enforcement outside sandboxes (operator follow-up item).
- Event-log and transcript compaction.

## Open Questions
- Does Claude `--settings` deny confine `Read`? TS-61 decides; the fallback is a `HOME` pointed at scratch.
- What are Grok's `--sandbox` profiles? Until TS-41 passes, Grok stays "unverified".
