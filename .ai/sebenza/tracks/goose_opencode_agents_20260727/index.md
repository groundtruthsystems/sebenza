# Track — goose & opencode as first-class agents

`goose_opencode_agents_20260727` · feature · backlog

## Documents
- [Design](./design.md) — use cases, diagrams, and verified architecture across all five domains
- [Spec](./spec.md) — requirements, NFRs, acceptance criteria, out of scope
- [Plan](./plan.json) — 5 phases, 53 tasks
- [Metadata](./metadata.json)

## Summary

Embed **goose** (1.8.0+) and **opencode** (1.18.7+) as built-in agents alongside `claude` and `codex`,
selectable at worktree creation, with capability-driven chat, history, status, and resume/fork. Because
opencode's plugin API can genuinely **deny** a tool call, the track also builds Sebenza's first
enforcement path: a synchronous, authenticated permission approve/deny channel.

## Phases

| Phase | Name | Tasks |
|---|---|---:|
| 0 | Verification & prerequisites | 7 |
| 1 | Agent abstraction & capability model *(pure refactor)* | 11 |
| 2 | opencode at full parity (+ shared controls) | 16 |
| 3 | goose at full parity | 9 |
| 4 | Permission gating for opencode | 10 |

opencode leads, so its phase carries the shared security and runtime controls that goose then inherits.

## Blocking unknowns

Two Phase 0 tasks need an **authenticated** opencode and gate later phases:

- `phase-0-task-1` — does a linked git worktree get its own opencode `project` row? *(blocks Phase 2)*
- `phase-0-task-2` — does `permission.ask` fire under `--auto`? *(blocks Phase 4)*
