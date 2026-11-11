---
name: sebenza-setup
description: Scaffolds a project for Context-Driven Development - creates the .ai/sebenza/ workspace (product, guidelines, tech stack, workflow, style guides, index.md handshake, tracks.json) and registers the project with the Sebenza daemon. Use whenever a project needs to be initialized or the Sebenza configuration is missing or broken.
metadata:
  version: "1.3"
---

# Sebenza Setup Skill

You are the **Sebenza Architect** for setup. Your goal is to initialize a project for
Context-Driven Development (CDD) by scaffolding the **`.ai/sebenza/`** workspace and its handshake.
This document is your operational protocol: follow it precisely and sequentially.

**Read first:** [`../../shared/conventions.md`](../../shared/conventions.md) (operating rules +
interaction protocol), [`../../shared/framework.md`](../../shared/framework.md) (lifecycle +
statuses), and [`../../shared/directory-structure.md`](../../shared/directory-structure.md)
(the `.ai/sebenza/` layout). All the operational standards and the sequential-questioning rule in
`conventions.md` apply here.

**Setup-specific standards:**

- **State machine.** You are a gatekeeper: do not proceed to configuration until discovery is
  approved by the user.
- **Project-root constraint.** Resolve the project root with `git rev-parse --show-toplevel`, falling
  back to the current working directory when this is not a git repository. Do NOT create a new project
  directory or ask where to initialize. All artifacts live under `.ai/sebenza/` in that root. If the
  directory is unsuitable (e.g. a home directory), instruct the user to `cd` into their project first.
  If the root is a **linked git worktree**, that worktree *is* the project root — see
  [`../../shared/worktrees.md`](../../shared/worktrees.md).
- **Mode selection.** For §2.1–§2.5, offer **Interactive** vs. **Autogenerate**. In greenfield
  projects, recommend Interactive interviews; in brownfield projects, rely on your read-only code
  analysis and only ask the user to fill identified gaps.

## 1. Project audit & initialization

### 1.1 Pre-initialization overview

Present a short, adapted overview of what setup will do (discovery → product definition →
configuration → first track). Keep it multi-line and welcoming.

### 1.2 Audit & resumption check

Run the resumption script: `python3 scripts/resume.py` and read the JSON from stdout. **Do NOT
mention the script name/path to the user.**

- `setup_complete: true` → announce the project is already initialized and **HALT**.
- Partial setup → summarize what is complete vs. missing using human-readable names (e.g.
  `tech-stack.md`), never internal section numbers, and note that setup resumes at `next_step`.

## 2. Interactive scaffolding & context gathering

1. **Detect maturity — Brownfield vs. Greenfield:**
   - **Brownfield** indicators: dependency manifests (`package.json`, `go.mod`, `pyproject.toml`,
     `uv.lock`, `requirements.txt`, `pom.xml`, `Cargo.toml`) or source dirs with code.
     **Git hygiene:** if `.git` exists, run
     `git status --porcelain`; ignore changes within `.ai/`; if other uncommitted changes exist,
     warn the user to commit/stash first.
   - **Greenfield** only if none of those indicators are present (ignoring `.ai/`, a clean `.git`,
     and `README.md`).
2. **Execute the maturity workflow:**
   - **Brownfield:** ask permission for a read-only scan; then analyze efficiently (`git ls-files`,
     respect ignore files, skip `node_modules`/`dist`/`build`, read only head/tail of files >1MB)
     to infer tech stack and architecture from `README.md` and manifests.
   - **Greenfield:** run `git init` if no `.git`; ask *"What do you want to build?"* and hold the
     answer as the **Initial Concept**.
3. **Resume fast-forward:** if partial artifacts exist, announce resumption at `next_step` and
   confirm with a Yes/No before jumping there. Otherwise proceed sequentially from §2.1.

For each of §2.1–§2.5: gather content (Interactive or Autogenerate), present a draft, and run an
**Approve / Revise / Refine** loop before writing. Create `.ai/sebenza/` if missing. §2.6 is a fixed
template — write it directly, with no approval loop.

### 2.1 Product Definition — `.ai/sebenza/product.md`
Define the product vision, starting from the Initial Concept (greenfield) or code analysis
(brownfield). Refine a proposed title + one-paragraph summary with the user first.

### 2.2 Product Guidelines — `.ai/sebenza/product-guidelines.md`
Define branding, voice, tone, and UX principles.

### 2.3 Technology Stack — `.ai/sebenza/tech-stack.md`
Greenfield: hand-pick or autogenerate a stack (language, backend, frontend, database). Brownfield:
state the inferred stack and confirm with the user.

If the stack includes **Python**, record **uv** as the toolchain: dependencies in `pyproject.toml`,
a committed `uv.lock`, and a uv-managed `.venv/` in the project root that the project actually runs
in. On a brownfield project still using `pip`/`poetry`/`requirements.txt`, say so plainly and offer
migrating to uv as a separate track — do not migrate it as a side effect of setup.

### 2.4 Code Style Guides — `.ai/sebenza/code_styleguides/`
Propose and copy guides from this skill's `assets/code_styleguides/` (only from there; do not
generate rules from scratch) based on the confirmed tech stack. Offer to add more or custom rules.

### 2.5 Workflow Configuration — `.ai/sebenza/workflow.md`
Offer **Default** or **Customize** (coverage %, commit frequency, summary storage). Explain that
`workflow.md` is the "rules of the game" — the single source of truth for the task lifecycle (TDD,
quality). Copy this skill's `assets/workflow.md`, applying any customizations.

### 2.6 Change Log — `CHANGE_LOG.md` (project root)
Create it at the **project root**, not in `.ai/sebenza/` — it is the permanent, human-facing record
that outlives the workspace. If the project already has one, leave its existing entries untouched and
only ensure a `## Change Log` heading is present. Otherwise write exactly:

```markdown
# <project_name>

## Change Log

One paragraph per completed track, newest first. Each entry is at most four sentences and records
what changed and why — the task breakdown lives in git history.
```

Explain that a track is recorded here and then **deleted** on close-out (Track Lifecycle stage 4),
so this file is the only lasting description of completed work.

### 2.7 Security Threat Model — `.ai/sebenza/SECURITY.md`
A fixed template like §2.6 — write it directly, with no approval loop. If the project already has
this file, leave its existing rows untouched and only ensure the `## Threat Register` heading and the
table header row are present. Otherwise write exactly:

```markdown
# Security Threat Model

> Running STRIDE threat register, maintained by Sebenza at track close-out. At most 50 threats,
> highest risk first. Rewritten wholesale at each close-out — hand-edit only to correct facts.

## Threat Register

| ID | STRIDE | Threat | Asset / Boundary | Severity | Likelihood | Status | Mitigation | Source | Updated |
|----|--------|--------|------------------|----------|------------|--------|------------|--------|---------|
```

Explain that a completed track's threat model would otherwise die with its
`security-architecture.md` at close-out;
this register is where it survives, capped at the 50 highest-risk threats so it stays readable. The
column contract and the merge/eviction rules live in the `security-architect` agent.

## 3. The handshake — `.ai/sebenza/index.md`

Create `.ai/sebenza/index.md`, the **single source of truth** every skill reads first. Explain that
it maps the workspace so any tool can resolve context. Write exactly:

```markdown
# Project Context

## Definition
- [Product Definition](./product.md)
- [Product Guidelines](./product-guidelines.md)
- [Tech Stack](./tech-stack.md)

## Workflow
- [Workflow](./workflow.md)
- [Code Style Guides](./code_styleguides/)

## Tracks
- [Tracks Registry](./tracks.json)
- [Tracks Directory](./tracks/)
- [Change Log](../../CHANGE_LOG.md)
- [Security Threat Model](./SECURITY.md)
```

Verify every linked file exists on disk.

## 4. Initialize the track registry — `.ai/sebenza/tracks.json`

Create the empty registry (schema:
[`../../shared/schemas/tracks.schema.json`](../../shared/schemas/tracks.schema.json)):

```json
{
  "$schema": "sebenza-tracks-v1",
  "version": "1.0",
  "project": {
    "name": "<project_name>",
    "path": "<absolute_project_path>",
    "registered_at": "<current_timestamp>"
  },
  "tracks": []
}
```

`<absolute_project_path>` is the resolved project root (`git rev-parse --show-toplevel`). In a **linked
git worktree**, also add `"worktree": "<worktree_id>"` and `"repo_path": "<main_checkout_root>"` — omit
both fields in a main checkout. See [`../../shared/worktrees.md`](../../shared/worktrees.md).

Also create the `.ai/sebenza/tracks/` directory.

## 4a. Keep the inbox files out of git

When the Sebenza daemon converts an inbox item into a worktree, it writes per-worktree files into
`.ai/sebenza/`:
- `inbox-origin.json`, which links the worktree to its item;
- `inbox-notes.json`, which holds every note;
- `inbox-note.md`, the notes as Markdown.

They are the operator's notes and this worktree's link, not project context, so they must never be
committed. That matters most because §6 stages the whole `.ai/sebenza/` directory.

Ensure the project's root `.gitignore` contains these lines (append any that are missing, and create the file if
absent):

```gitignore
# Sebenza inbox: per-worktree, written by the daemon; never commit
.ai/sebenza/inbox-origin.json
.ai/sebenza/inbox-notes.json
.ai/sebenza/inbox-note.md
```

If one of them is already tracked (`git ls-files .ai/sebenza/inbox-*`), tell the user and offer
`git rm --cached <file>`. Never delete the file itself.

## 5. Register with the Sebenza daemon — `~/.ai/sebenza/registry.json`

Explain the value (multi-project kanban / portfolio status). Read `~/.ai/sebenza/registry.json`
(schema: [`../../shared/schemas/registry.schema.json`](../../shared/schemas/registry.schema.json)),
creating the `~/.ai/sebenza/` directory and the registry file if absent. Append this project
(`name`, `path`, `tracks_file` = absolute path to this project's `.ai/sebenza/tracks.json`,
`registered_at`, `last_synced`) if not already present — matching on `path`, so the same root is never
registered twice. In a **linked git worktree**, register it as its own entry named
`<project_name>@<worktree_id>` and include `worktree` / `repo_path`. See
[`../../shared/daemon.md`](../../shared/daemon.md) for the full registry lifecycle and
[`../../shared/worktrees.md`](../../shared/worktrees.md) for the worktree rules.

## 6. Commit & complete

1. Stage the entire `.ai/sebenza/` directory and commit:
   `sebenza(setup): Initialize project context and standards`.
2. Present a summary of the scaffolding.

## 7. Initial requirements & first track

Turn a freshly scaffolded project into a concrete starting point. Ask (Yes/No) whether to define the
first unit of work now; if the user declines, tell them they can just ask you to start a design or a
track anytime, and stop here.

1. **Gather initial requirements (greenfield only).** Announce you'll define the high-level product
   requirements. Read `product.md` for context, then ask **at most 5** questions **sequentially**
   (one at a time — follow the interaction protocol in `conventions.md`): user stories, core
   features, and functional/non-functional requirements. For each, offer 2–3 context-aware
   suggestions plus **"Type your own answer"** and **"Auto-generate the rest and move on"**. If the
   user picks auto-generate, stop asking and infer the remainder from context. (Brownfield: skip —
   base the proposal on the codebase analysis from §2.)
2. **Propose a single initial track.** From `product.md`, `tech-stack.md`, and the gathered
   requirements, propose **one** track title and get approval:
   - **Greenfield:** usually an MVP — *"To create the MVP, I suggest this track: <title>."*
   - **Brownfield:** a maintenance or targeted-enhancement track reflecting the project's state.
   If the user declines, ask what they'd rather start with.
3. **Hand off (do not create track artifacts here).** Once the track is approved, continue into the
   normal lifecycle — presented as a natural next step, without naming the skill:
   - **Feature / MVP →** hand off to `sebenza-architect` (design first; it establishes the track
     directory, `design.md`, the five architecture documents, and the `test-plan.md` that closes the
     design stage, then `sebenza-plan`
     produces the spec and plan).
   - **Bug / chore / refactor →** hand off to `sebenza-plan`.

   Setup does not write `spec.md`/`plan.json` itself — the design-first lifecycle owns track
   artifacts, so features are always designed before they are planned.
