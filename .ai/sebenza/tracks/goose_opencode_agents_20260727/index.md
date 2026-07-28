# Track — opencode as a first-class agent

`goose_opencode_agents_20260727` · feature · backlog

> **Scope:** opencode only. **goose is deferred** — see the repository's `TODO.md`. The track id retains
> the original name for continuity with the design and commit history.

## Documents
- [Design](./design.md) — use cases, diagrams, and verified architecture across all five domains.
  Deliberately retains the goose research as input to the deferred track.
- [Spec](./spec.md) — requirements, NFRs, acceptance criteria, out of scope
- [Plan](./plan.json) — 4 phases, 44 tasks
- [Metadata](./metadata.json)

## Summary

Make **opencode** (1.18.7+) a built-in agent alongside `claude` and `codex`, selectable at worktree
creation, with capability-driven chat, history, status, and resume/fork. Because opencode's plugin API can
genuinely **deny** a tool call, the track also builds Sebenza's first enforcement path: a synchronous,
authenticated permission approve/deny channel.

## Phases

| Phase | Name | Tasks |
|---|---|---:|
| 0 | Verification & prerequisites | 7 |
| 1 | Agent abstraction & capability model *(pure refactor)* | 11 |
| 2 | opencode at full parity (+ shared controls) | 16 |
| 3 | Permission gating for opencode | 10 |

Phase 1's abstraction is built so a future agent is an adapter, not a fork — which is what makes the
deferred goose work tractable.

## Blocking unknowns

Two Phase 0 tasks need an **authenticated** opencode and gate later phases:

- `phase-0-task-1` — does a linked git worktree get its own opencode `project` row? *(blocks Phase 2)*
- `phase-0-task-2` — does `permission.ask` fire under `--auto`? *(blocks Phase 3)*
