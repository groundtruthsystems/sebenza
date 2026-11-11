# Sebenza Operating Conventions (shared reference)

> Shared reference for all `sebenza-*` skills. No frontmatter — loaded on demand.
> Every skill follows these rules; skill bodies describe only what is specific to them.

## Operational standards

- **Precise execution.** Do not skip steps. Do not assume project state; verify it (read the file,
  list the directory) before acting.
- **Tool validation.** Validate the success of *every* tool call. On failure, review the error,
  self-correct once, or **halt and report to the user**. Never continue past a failed call.
- **Path integrity.** When *talking about* files or passing them to tools, use relative paths from the
  project root (e.g. `.ai/sebenza/tracks.json`), forward slashes only. The project root is
  `git rev-parse --show-toplevel` — **not** the current working directory, which may be a subdirectory
  or a linked git worktree. See [`worktrees.md`](./worktrees.md).
  **Exception — the `*_path` fields written *into* `tracks.json`.** `design_path` / `spec_path` /
  `plan_path` are resolved by readers against the workspace dir (`.ai/sebenza/`), not the project root,
  so they are the one place the project-root rule does not apply. Write them as
  `./tracks/<track_id>/<file>` — never `.ai/sebenza/tracks/...`, which readers resolve to a doubled
  `.ai/sebenza/.ai/sebenza/...` and fail to open. See
  [`directory-structure.md`](./directory-structure.md#track-document-paths-in-tracksjson).
- **User confirmation.** Before writing files or making changes, confirm with the user. The one
  exception is path reconciliation (see the handshake check below): it is housekeeping — write it, then
  report it.
- **Strategic transparency.** Before creating or modifying key infrastructure (tracks, plans,
  registry), briefly explain *why* it matters — act as a mentor, not just an executor.
- **Agnostic language.** Do NOT tell users to run slash commands like `/sebenza:xxx`. Instead tell
  them to ask you directly (e.g. "to start implementing, just ask me").
- **Terminal-readable output.** The user is reading a terminal: it renders neither Mermaid nor a long
  document. Confirm diagrams as **ASCII** sketches and write **Mermaid** into the artifact; review
  written artifacts by summarizing and giving the file path, not by reproducing them in chat. See
  [`diagrams.md`](./diagrams.md).

## Interaction protocol

When gathering information or asking the user to decide:

- Offer **single-choice** or **multiple-choice** options based on context-aware suggestions rather
  than raw open-ended questions.
- If one option is preferred, list it **first**, prefix it with **(Recommended)**, and give a brief
  italic reason.
- Always include an **"Other" / user-defined** option.
- For brainstorming/scope questions (multiple answers valid), add "(Select all that apply)".
- **In a linked worktree** (`.ai/sebenza/inbox-origin.json` exists), the operator watches the
  Sebenza inbox, not your terminal. **Before you stop to wait** for any answer, whether it is a
  question, an approval or a choice, raise an inbox request for it per [`inbox.md`](./inbox.md#raising-one),
  then ask in chat as usual. A question asked only in chat is never seen.

## Sequential questioning (CRITICAL)

If a native structured-input tool (modal/form) is available, you may group questions with it.
Otherwise, in plain text chat, ask questions **strictly one at a time** and wait for the answer
before the next. Never emit multiple questions in a single chat turn.

## Setup / handshake check

Every skill (except `sebenza-setup`) begins by verifying the workspace:

1. Locate `.ai/sebenza/index.md`. Resolve the context documents through its links — see the file
   resolution protocol in [`directory-structure.md`](./directory-structure.md).
2. Verify the linked core files exist (Product Definition, Tech Stack, Workflow), plus any file the
   specific skill needs (e.g. `tracks.json` for status/implement/revert).
3. If `index.md` or a required file is missing, **halt** and tell the user the workspace is not set
   up (or is corrupted) and to ask you to run setup — internally handing off to `sebenza-setup`.
4. When you load `tracks.json`, **reconcile its `project.path`** against the root you are actually
   working in, per [`worktrees.md`](./worktrees.md): if it does not match `git rev-parse
   --show-toplevel`, rewrite it (plus `worktree` / `repo_path` in a linked worktree) without asking, and
   mention it in one line. If the paths match — the normal case — do nothing.

## Skill handoffs

Skills chain by **invoking a sibling skill by name**, presenting the transition as a natural next
step (never naming the skill to the user):

```
setup  ->  architect (design)  ->  plan (spec & plan)  ->  implement
                                                                    |
                                              review / status / revert (as needed)
```

## Statuses & blocked protocol

See [`framework.md`](./framework.md) for the kanban statuses and the blocked/unblocked protocol.
Keep `plan.json` and `tracks.json` in sync on every status change, and refresh `updated_at`.