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
agents/project-reviewer.md
agents/solution-coordinator.md
agents/solution-reviewer.md
agents/solutions-architect.md
agents/system-analyst.md
scripts/test_factory.py
scripts/test_migration.py
```

## Post-move changes

Newest first. Each row is a change made here after the move.

| Date | Track / task | Change |
|---|---|---|
| 2026-11-11 | factory_project_level_20261111 / phase-2-task-2, -3 | `sebenza-architect` §4 runs three waves (business; application ∥ data with the business document and `solution-handover.md`; technical ∥ security) and gains §7a, independent design review (≤3 rounds) then the approval request. `sebenza-implement` gains §3a, the approval gate. `inbox.py` gains `approval` and `review` (exit codes 7-10; `--local` not-independent rounds in `reviews.json`); `shared/inbox.md` documents them. Tests: `scripts/test_factory.py`; `inbox_testkit.py`'s fake agentctl answers `review`. |
| 2026-11-11 | factory_project_level_20261111 / phase-1-task-4 | Added `agents/project-reviewer.md` (design and code levels), embedded in the server. |
| 2026-11-11 | software_factory_20261111 / phase-4-task-3 | Added `agents/solution-coordinator.md`, `system-analyst.md`, `solutions-architect.md` and `solution-reviewer.md`, embedded in the server. |
| 2026-11-11 | software_factory_20261111 / phase-1-task-2 | Moved the plugin unchanged. Added this record and `scripts/test_migration.py` (marketplace, layout, hook supply-chain checks). Outside `skills/`, the host repo's `.gitignore` now ignores the inbox copies, which `test_skill_docs.py` (TS-55) expects of the host repository. |
