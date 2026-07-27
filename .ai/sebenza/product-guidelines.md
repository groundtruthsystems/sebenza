# Product Guidelines

## Voice

Terse, technical, present-tense. Assumes a developer audience — no hand-holding, no marketing
register. Errors state what failed and what to do next.

## Visual identity

Single dark theme (label `Sebenza`, key `github-dark` — the key is retained so persisted values keep
resolving).

| Role | Colour |
|---|---|
| surface | `#0a0a0f` |
| sidebar | `#0d1626` |
| topbar | `#101d33` |
| hover | `#17294a` |
| edge | `#20375a` |
| primary | `#ffffff` |
| muted | `#b0bec5` |
| accent | `#00d4ff` |
| success | `#00e676` |
| warning | `#ffb74d` |
| danger | `#ff5252` |

Terminal chrome matches app chrome so an embedded xterm never looks bolted on.

## UX principles

1. **Information-dense** — small type (11–13px), tight spacing; a wide worktree list beats
   pagination.
2. **Terminal-first** — the PTY is the primary surface; chat and forms are conveniences layered over
   it, never replacements.
3. **Capability-driven UI** — features appear based on **declared agent capabilities**, never
   hardcoded agent identity. An agent that cannot do in-app chat hides the chat tab and says why.
4. **Never lose a session** — destructive actions (remove, merge) confirm; closing never destroys.
5. **Status at a glance** — agent lifecycle, PR state, and CI roll up into per-worktree icons.
