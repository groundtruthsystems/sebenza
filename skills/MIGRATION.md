# Sebenza skills — migration record

The Sebenza Claude Code plugin (skills, agents, shared references, scripts and hooks) moved into
this repository on 2026-11-11 under track `software_factory_20261111`. Before the move it was the
`sebenza` plugin of the `gts-agent-skills` marketplace.

## Source

| Field | Value |
|---|---|
| Repository | `groundtruthsystems/gts-agent-extensions` |
| Path | `sebenza/` |
| Commit | `d5cc5a08c683d73a9e24507dfbb9b658d1682709` (2026-11-06, "Merge pull request #15 from groundtruthsystems/agent-extensions-skills") |
| Method | `git archive <commit> sebenza`, extracted into `skills/` byte for byte |
| Plugin version at the move | `1.1.0` |

`skills/` is the plugin root, so the layout inside it is the source layout unchanged:
`skills/.claude-plugin/plugin.json`, `skills/skills/sebenza-*`, `skills/agents`, `skills/shared`,
`skills/scripts` and `skills/hooks`. Relative links between skills and `shared/` resolve exactly as
before. The originals are removed from gts-agent-extensions by its `agent-extensions-factory`
worktree. Until then, change the skills **here** only.

## Install

The repository root is a Claude Code marketplace named `sebenza` with one plugin, `sebenza`:

```bash
claude plugin marketplace add groundtruthsystems/sebenza   # or a path to a local clone
claude plugin install sebenza@sebenza
```

These names are stable and are referenced by the gts-agent-extensions README, QUICKSTART and
CLAUDE.md. It replaces `sebenza@gts-agent-skills`.

## Source listing

Every file of the plugin at the source commit, relative to `skills/`:

```text
.claude-plugin/plugin.json
agents/application-architect.md
agents/business-architect.md
agents/data-architect.md
agents/security-architect.md
agents/technical-architect.md
agents/test-architect.md
hooks/hooks.json
scripts/eval_inbox.py
scripts/inbox.py
scripts/inbox_testkit.py
scripts/test_hooks.py
scripts/test_inbox.py
scripts/test_notes.py
scripts/test_skill_docs.py
scripts/test_validate_plan.py
scripts/testdata/contracts/inbox-notes.v1.json
scripts/testdata/contracts/plan.depends_on.json
scripts/testdata/legacy/docs_hygiene_20260727.plan.json
scripts/testdata/legacy/kaituhi_20260810.plan.json
scripts/testdata/legacy/pre-graph.tracks.json
scripts/validate_plan.py
shared/conventions.md
shared/daemon.md
shared/diagrams.md
shared/directory-structure.md
shared/document-budgets.md
shared/framework.md
shared/inbox-contract.md
shared/inbox.md
shared/schemas/plan.schema.json
shared/schemas/registry.schema.json
shared/schemas/tracks.schema.json
shared/worktrees.md
skills/sebenza-architect/SKILL.md
skills/sebenza-implement/SKILL.md
skills/sebenza-plan/SKILL.md
skills/sebenza-revert/SKILL.md
skills/sebenza-review/SKILL.md
skills/sebenza-setup/SKILL.md
skills/sebenza-setup/assets/code_styleguides/general.md
skills/sebenza-setup/assets/code_styleguides/go.md
skills/sebenza-setup/assets/code_styleguides/html-css.md
skills/sebenza-setup/assets/code_styleguides/javascript.md
skills/sebenza-setup/assets/code_styleguides/python.md
skills/sebenza-setup/assets/code_styleguides/rust.md
skills/sebenza-setup/assets/code_styleguides/typescript.md
skills/sebenza-setup/assets/workflow.md
skills/sebenza-setup/scripts/resume.py
skills/sebenza-status/SKILL.md
skills/sebenza-status/resources/kanban.html
skills/sebenza-status/resources/tracks-viewer.html
```

## Post-move additions

Files added under `skills/` after the move:

```text
MIGRATION.md
agents/comprehension-reviewer.md
agents/project-reviewer.md
agents/solution-coordinator.md
agents/solution-reviewer.md
agents/solutions-architect.md
agents/system-analyst.md
agents/zoom-out-reviewer.md
scripts/test_factory.py
scripts/test_migration.py
scripts/test_standards_stage.py
scripts/test_writing_guidance.py
shared/resolving-findings.md
skills/sebenza-review/standards-stage.md
skills/sebenza-setup/scripts/propose_rules.py
```

## Post-move changes

Newest first. Each row is a change made here after the move.

| Date | Track / task | Change |
|---|---|---|
| 2026-11-20 | no track (EGM review practices) | Added `agents/zoom-out-reviewer.md`, `agents/comprehension-reviewer.md` and `shared/resolving-findings.md`, adapted from the Elephant-Goldfish Model (google/elephant-goldfish-model, Apache-2.0). `sebenza-architect` gains §6a (zoom-out before verification), runs the comprehension reviewer and a second zoom-out in §7, and classifies every finding as FIX / CONTEST / ESCALATE / DECLINE in §7 and §7a; `sebenza-review` §5a hands code findings over with the same rules (skill 1.4). Plugin 1.2.0. Tests: `test_skill_docs.py` `FreshContextReviews`. |
| 2026-11-19 | standards_review_20261118 / phase-6-task-2 | `sebenza-review` §5a gains step 3: sign-off is the operator's `sebenza-cli track signoff <track_id>`, the daemon's compare-and-swap gate, which refuses `no_verdict`, `verdict_not_passing` or `stale_verdict`; the skill never signs off. §6 PASS points at it. Tests: `scripts/test_standards_stage.py`. |
| 2026-11-19 | standards_review_20261118 / phase-5-task-4 | `sebenza-review` gains §4a, the standards stage: it reports the four existing checks with `sebenza-agentctl review-checks`, reads `sebenza-cli review status` and `review findings`, reports each finding resolved or unresolved, reports `not-run` when the daemon is unreachable and never sets the verdict. Added `skills/sebenza-review/standards-stage.md`, the sandboxed run's instructions, which the server embeds as its agent prompt. `sebenza-setup`'s template `general.md` gains the three default review rules (restating comments, excessive comments, long methods at `max_lines=20`), the template `index.md` lists it, and §1.3 with `scripts/propose_rules.py` proposes the rules to existing projects as a diff and writes nothing. Tests: `scripts/test_standards_stage.py`. |
| 2026-11-19 | standards_review_20261118 / phase-4-task-3 | `sebenza-implement` gains "Writing code" (comment only for non-obvious intent, short methods, new and changed code only, thresholds deferred to the project's `.ai/sebenza/` documents) and "Commits and the edit boundary" (`Sebenza-Track`/`Sebenza-Task` trailers, the task's `files` boundary), and completes a track with `sebenza-agentctl complete`. `sebenza-plan` gives tasks a `files` boundary and gains §6a, plan approval followed by `sebenza-cli track approve-plan`. `shared/schemas/plan.schema.json` gains the optional task `files`. Tests: `scripts/test_writing_guidance.py`. |
| 2026-11-11 | factory_project_level_20261111 / phase-3-task-2, -3 | `sebenza-review` gains §5a, code review rounds (≤3, findings reopen named tasks, then the final-verification request). `inbox.py review --apply` writes `review_round`/`review_findings` and syncs `tracks.json`; `inbox.py verification` files the request with open findings. `shared/inbox.md` documents both. |
| 2026-11-11 | factory_project_level_20261111 / phase-2-task-2, -3 | `sebenza-architect` §4 runs three waves (business; application ∥ data with the business document and `solution-handover.md`; technical ∥ security) and gains §7a, independent design review (≤3 rounds) then the approval request. `sebenza-implement` gains §3a, the approval gate. `inbox.py` gains `approval` and `review` (exit codes 7-10; `--local` not-independent rounds in `reviews.json`); `shared/inbox.md` documents them. Tests: `scripts/test_factory.py`; `inbox_testkit.py`'s fake agentctl answers `review`. |
| 2026-11-11 | factory_project_level_20261111 / phase-1-task-4 | Added `agents/project-reviewer.md` (design and code levels), embedded in the server. |
| 2026-11-11 | software_factory_20261111 / phase-4-task-3 | Added `agents/solution-coordinator.md`, `system-analyst.md`, `solutions-architect.md` and `solution-reviewer.md`, embedded in the server. |
| 2026-11-11 | software_factory_20261111 / phase-1-task-2 | Moved the plugin unchanged. Added this record and `scripts/test_migration.py` (marketplace, layout, hook supply-chain checks). Outside `skills/`, the host repo's `.gitignore` now ignores the inbox copies, which `test_skill_docs.py` (TS-55) expects of the host repository. |
