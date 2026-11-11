# Sebenza Workspace Layout (shared reference)

> Shared reference for all `sebenza-*` skills. No frontmatter — loaded on demand.

The Sebenza workspace lives at **`.ai/sebenza/`** in the project root. The paths in the tree below are
written relative to the project root and MUST use forward slashes. The `*_path` fields *stored inside*
`tracks.json` are the one exception — they are workspace-relative, see
[Track document paths](#track-document-paths-in-tracksjson).

```
~/.ai/sebenza/
└── registry.json                 # central daemon registry (all projects)

<project-root>/
└── CHANGE_LOG.md                 # permanent history: one paragraph per completed track

<project-root>/.ai/sebenza/
├── index.md                      # HANDSHAKE: single source of truth linking context docs
├── product.md                    # product vision and goals
├── product-guidelines.md         # UX / brand guidelines
├── tech-stack.md                 # technology choices
├── workflow.md                   # task lifecycle rules (TDD, commits, checkpoints)
├── code_styleguides/             # language-specific style guides
├── SECURITY.md                   # running STRIDE threat register: top 50 threats by risk
├── tracks.json                   # kanban-compatible track registry (sole registry)
└── tracks/                       # active tracks only — deleted on close-out
    └── <track_id>/
        ├── index.md              # per-track handshake linking the docs below
        ├── metadata.json         # id, type, status, timestamps, description
        ├── design.md             # overview, use cases, diagrams, open questions (features)
        ├── business-architecture.md     # one document per architecture domain (features)
        ├── application-architecture.md
        ├── data-architecture.md
        ├── security-architecture.md     # holds the threat model until close-out
        ├── technical-architecture.md
        ├── test-plan.md          # test strategy, scenarios, traceability
        ├── spec.md               # detailed requirements
        └── plan.json             # phased task list (kanban-compatible)
```

There is no archive directory. A track that reaches `done` is recorded in `CHANGE_LOG.md` and then
deleted, so `tracks/` and `tracks.json` only ever describe work in flight.

The directory and the `tracks.json` entry are created at the **start** of the design stage, so a track
can legitimately exist with only its design documents — `test-plan.md` is completed as the design stage's
last step, and `spec.md` and `plan.json` arrive later from `sebenza-plan`. A track with a `design_path` but no `plan_path` is still being designed; it is not
broken, and nothing should treat it as a zero-task track.

**Architecture documents.** Each domain has its own `<domain>-architecture.md`, which always exists,
holding either the domain's architecture or the one-line reason it is out of scope. `design.md` links
them from its `## Architecture` index. `tracks.json` registers only `design_path`; the architecture
documents are found beside it. An older track, designed before this split, keeps all five domains
under `## Architecture` in `design.md`, and every reader falls back to that.

## Track ID

`shortname_YYYYMMDD`, matching `^[a-z0-9_]+_[0-9]{8}$`.

## Track document paths in `tracks.json`

A track entry's `design_path`, `test_plan_path`, `spec_path`, and `plan_path` are resolved **relative to the workspace
directory that contains `tracks.json`** — `.ai/sebenza/` — and **not** relative to the project root.
Every consumer (the Sebenza dashboard's tracks board, the registry portfolio, the `sebenza-status`
viewers) joins the value onto the already-resolved workspace dir. Write exactly:

```json
"design_path":    "./tracks/<track_id>/design.md",
"test_plan_path": "./tracks/<track_id>/test-plan.md",
"spec_path":      "./tracks/<track_id>/spec.md",
"plan_path":      "./tracks/<track_id>/plan.json"
```

Writing the project-root form — `.ai/sebenza/tracks/<track_id>/design.md` — is the common mistake,
because that is how these files are *named in conversation* and passed to file tools. Stored in
`tracks.json`, it resolves to `.ai/sebenza/.ai/sebenza/tracks/<track_id>/design.md`, which does not
exist: the document silently fails to open and the UI shows an empty or errored panel. The doubled
`.ai/sebenza/` is the signature of this bug.

**Repair on touch.** Whenever a skill writes a track's entry, normalize any legacy value first: strip a
leading `.ai/sebenza/` (and any bare `tracks/...` gains the `./` prefix) so the stored value matches the
canonical form above. This is housekeeping — apply it without asking, then mention it.

`project.path` and the registry's `tracks_file` are unaffected: both are **absolute**.

## The change log (`CHANGE_LOG.md`)

`CHANGE_LOG.md` sits at the **project root**, not inside `.ai/sebenza/`, because it outlives the
workspace and is the one Sebenza artifact written for humans rather than for skills. It holds a
single `## Change Log` heading followed by one entry per completed track, newest first:

```markdown
### <YYYY-MM-DD> — <track_description> (<track_id>)

<One paragraph, four sentences maximum.>
```

The four-sentence ceiling is deliberate. The entry is the residue of a deleted track, so it records
what changed and why — not the task breakdown, which git history already carries.

## The security threat model (`SECURITY.md`)

A feature's threat model is produced by the `security-architect` agent and lives in the track's
`security-architecture.md`, which close-out deletes. `SECURITY.md` is where it survives: a running STRIDE register
of the project's **50 highest-risk threats**, sorted worst-first.

Unlike `CHANGE_LOG.md`, it sits **inside** `.ai/sebenza/`. The change log is written for humans and
outlives the workspace; the register is a working artifact that skills read and rewrite, and it is
meaningless without the context docs beside it. The name also stays clear of the root `SECURITY.md`
convention, which GitHub reads as a vulnerability-disclosure policy.

The register is **rewritten wholesale** at each close-out by `sebenza-implement` step 8, from the
closing track's threats only — never a codebase-wide re-scan. The column contract, the risk-ordering
formula, and the merge/eviction rules are defined in
[`../agents/security-architect.md`](../agents/security-architect.md) under "Threat Register"; that
agent is the single source of truth for them. Hand-edit the file only to correct facts — anything
else is overwritten at the next close-out.

## The test plan (`test-plan.md`)

The test plan is the design stage's **last** artifact: the `test-architect` agent derives it from the
confirmed `UC-nn` use cases and the five written architecture sections, so it cannot be produced
before them. It carries the `TS-nn` scenarios, the level each runs at, and the traceability table
that maps every use case and every must-fix risk onto the scenarios that prove it.

**It does not survive close-out.** Unlike the threat model, which is cumulative and is consolidated
into `SECURITY.md`, a test plan describes one track's work and nothing is copied out of it. What
survives is what it caused: the tests committed to the repository and the `commit_sha` on each task
that carries `test_scenarios`. Close-out deletes `test-plan.md` with the rest of the track.

For a **bug, chore, or refactor** — which skip the design stage — `sebenza-plan` writes the test plan
itself from the spec's acceptance criteria, keyed on `AC-nn` instead of `UC-nn`. Every track has one.

## The handshake (`index.md`)

`.ai/sebenza/index.md` is the **single source of truth** every skill reads first. It maps the
workspace so any tool can resolve context without hardcoding paths. Resolve the context documents
(Product Definition, Tech Stack, Workflow, etc.) through the links in `index.md` rather than
assuming fixed filenames — this is the **file resolution protocol** referenced by every skill.

If `index.md` is missing or a linked file does not exist, the workspace is not set up correctly:
halt and direct the user to `sebenza-setup`.

The project root is `git rev-parse --show-toplevel`, not the current working directory. When that root
is a **linked git worktree**, the `.ai/sebenza/` workspace inside it is worktree-local and its
`tracks.json` records the worktree as `project.path` — reconciled automatically on load, per
[`worktrees.md`](./worktrees.md).

## Schemas

JSON artifacts are validated against the schemas in [`schemas/`](./schemas/):

- [`plan.schema.json`](./schemas/plan.schema.json) — `plan.json`
- [`tracks.schema.json`](./schemas/tracks.schema.json) — `tracks.json`
- [`registry.schema.json`](./schemas/registry.schema.json) — `~/.ai/sebenza/registry.json`