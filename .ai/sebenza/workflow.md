# Project Workflow

## Guiding Principles

1.  **Design First:** Every feature or system change begins with the
    **architect**, never with planning or code. The design examines the change
    from all five perspectives — business, application, technical, data, and
    security — so that system changes are well thought out before any commitment
    is made.
2.  **The Plan is the Source of Truth:** All work must be tracked in `plan.json`
3.  **The Tech Stack is Deliberate:** Changes to the tech stack must be
    documented in `tech-stack.md` *before* implementation
4.  **Test-Driven Development:** Write unit tests before implementing
    functionality
5.  **High Code Coverage:** Aim for >80% code coverage for all modules
6.  **User Experience First:** Every decision should prioritize user experience
7.  **Non-Interactive & CI-Aware:** Prefer non-interactive commands. Use
    `CI=true` for watch-mode tools (tests, linters) to ensure single execution.
8.  **CLI/UI Parity:** Any capability added to the dashboard must also be
    reachable from `sebenza-cli`, and vice versa. A feature that lands in only
    one surface is incomplete.

## Track Lifecycle

Every track moves through a fixed sequence of stages. The order is **mandatory**
and exists to ensure changes are considered from multiple perspectives before
they are built:

1.  **Architect (Design):** ALWAYS the first step. Define the use cases and
    diagrams, then define **and verify** the architecture across the
    **business, application, technical, data, and security** domains (each
    contributed and verified by its specialist architect). The output is an
    approved `design.md`. A feature MUST NOT proceed to planning without one.
2.  **New Track (Spec & Plan):** Only after the design is approved. Refine
    `design.md` into a detailed `spec.md` and a phased `plan.json`. Planning
    halts on a feature that has no approved `design.md` and hands back to the
    architect.
3.  **Implement:** Execute the plan's tasks following the **Task Workflow**
    below.

**Exception:** Bugs, chores, and refactors may skip the architect stage and
start at New Track, where they are specified from scratch. Anything that changes
system behaviour or structure is treated as a feature and MUST start with the
architect.

## Task Workflow

All tasks follow a strict lifecycle:

### Standard Task Workflow

1.  **Select Task:** Choose the next available task from `plan.json` in
    sequential order (the first task whose `status` is not `done`)

2.  **Mark In Progress:** Before beginning work, set the task's `status` to
    `"doing"` in `plan.json`, and sync the change to `tracks.json` (the track's
    `phases_summary` entry and, if this is the first active task, the track
    `status`). Refresh `updated_at`.

3.  **Write Failing Tests (Red Phase):**

    -   Add tests for the feature or bug fix. Rust tests live in an inline
        `#[cfg(test)] mod tests` block colocated in the module under test;
        frontend tests live in a colocated `*.test.ts` / `*.test.tsx` file.
    -   Write one or more unit tests that clearly define the expected behavior
        and acceptance criteria for the task.
    -   **CRITICAL:** Run the tests and confirm that they fail as expected. This
        is the "Red" phase of TDD. Do not proceed until you have failing tests.

4.  **Implement to Pass Tests (Green Phase):**

    -   Write the minimum amount of application code necessary to make the
        failing tests pass.
    -   Run the test suite again and confirm that all tests now pass. This is
        the "Green" phase.

5.  **Refactor (Optional but Recommended):**

    -   With the safety of passing tests, refactor the implementation code and
        the test code to improve clarity, remove duplication, and enhance
        performance without changing the external behavior.
    -   Rerun tests to ensure they still pass after refactoring.

6.  **Verify Coverage:** Run coverage reports using the project's chosen tools:

    ```bash
    cd frontend && npm run test:coverage    # vitest + @vitest/coverage-v8
    cargo test                              # Rust (add cargo-llvm-cov if installed)
    ```

    Target: >80% coverage for new code.

7.  **Document Deviations:** If implementation differs from tech stack:

    -   **STOP** implementation
    -   Update `tech-stack.md` with new design
    -   Add dated note explaining the change
    -   Resume implementation

8.  **Commit Code Changes:**

    -   Stage all code changes related to the task.
    -   Propose a clear, concise commit message e.g, `feat(agents): Add goose to
        the built-in agent registry`.
    -   Perform the commit.

9.  **Attach Task Summary with Git Notes:**

    -   **Step 9.1: Get Commit Hash:** Obtain the hash of the *just-completed
        commit* (`git log -1 --format="%H"`).
    -   **Step 9.2: Draft Note Content:** Create a detailed summary for the
        completed task. This should include the task name, a summary of changes,
        a list of all created/modified files, and the core "why" for the change.
    -   **Step 9.3: Attach Note:** Use the `git notes` command to attach the
        summary to the commit:
        `git notes add -m "<note content>" <commit_hash>`

10. **Record Task Completion and Commit SHA:**

    -   **Step 10.1: Update Plan:** In `plan.json`, find the completed task, set
        its `status` to `"done"`, and set its `commit_sha` to the first 7
        characters of the *just-completed commit's* hash.
    -   **Step 10.2: Sync `tracks.json`:** Recalculate the track's `progress`
        (`total_tasks`, `completed_tasks`, `percentage`) and update the
        `phases_summary` entry. Refresh `updated_at`.

11. **Commit Plan Update:**

    -   **Action:** Stage the modified `plan.json` and `tracks.json` files.
    -   **Action:** Commit this change with a descriptive message (e.g.,
        `sebenza(plan): Mark task 'Add goose to agent registry' as complete`).

### Task Correction & Plan Amendment Workflows

When an implemented task or phase requires corrections, amendments, or additions,
follow these standard workflows to maintain plan integrity and avoid untracked
code drift:

1.  **In-Flight Refinements:** If minor gaps are found while a task is actively
    in-progress (`status: "doing"`), make the adjustments directly in the active
    implementation stream and ensure passing tests before committing.
2.  **Code Review Corrections:** If issues are identified during or after a code
    review, ask for a review (e.g. *"run a review"*). The review appends a
    `Review Fixes` phase to `plan.json` so that correction tasks are formally
    tracked and checkpointed.
3.  **Logical State Reversions:** If a task implementation is fundamentally
    flawed or needs to be redone, ask for a revert (e.g. *"revert the last
    task"*). This safely rolls back associated git commits and resets the task's
    `status` in `plan.json` back to `"backlog"` (clearing its `commit_sha`) to
    allow a clean restart.

### Phase Completion Verification and Checkpointing Protocol

**Trigger:** This protocol is executed immediately after a task is completed
that also concludes a phase in `plan.json`.

1.  **Announce Protocol Start:** Inform the user that the phase is complete and
    the verification and checkpointing protocol has begun.

2.  **Ensure Test Coverage for Phase Changes:**

    -   **Step 2.1: Determine Phase Scope:** To identify the files changed in
        this phase, you must first find the starting point. Read `plan.json` to
        find the previous phase's `checkpoint_sha`. If no previous checkpoint
        exists, the scope is all changes since the first commit.
    -   **Step 2.2: List Changed Files:** Execute `git diff --name-only
        <previous_checkpoint_sha> HEAD` to get a precise list of all files
        modified during this phase.
    -   **Step 2.3: Verify and Create Tests:** For each file in the list:
        -   **CRITICAL:** First, check its extension. Exclude non-code files
            (e.g., `.json`, `.md`, `.yaml`).
        -   For each remaining code file, verify corresponding tests exist — an
            inline `#[cfg(test)]` module for Rust, a colocated `*.test.tsx` for
            the frontend.
        -   If tests are missing, you **must** create them. Before writing them,
            **first analyze other tests in the repository to determine the
            correct naming convention and testing style.** The new tests
            **must** validate the functionality described in this phase's tasks
            (`plan.json`).

3.  **Execute Automated Tests with Proactive Debugging:**

    -   Before execution, you **must** announce the exact shell command you will
        use to run the tests.
    -   **Example Announcement:** "I will now run the automated test suite to
        verify the phase. **Command:** `cargo test && cd frontend && CI=true npm test`"
    -   Execute the announced command.
    -   If tests fail, you **must** inform the user and begin debugging. You may
        attempt to propose a fix a **maximum of two times**. If the tests still
        fail after your second proposed fix, you **must stop**, report the
        persistent failure, and ask the user for guidance.

4.  **Propose a Detailed, Actionable Manual Verification Plan:**

    -   **CRITICAL:** To generate the plan, first analyze `product.md`,
        `product-guidelines.md`, and `plan.json` to determine the user-facing
        goals of the completed phase.
    -   You **must** generate a step-by-step plan that walks the user through
        the verification process, including any necessary commands and specific,
        expected outcomes.
    -   The plan you present to the user **must** follow this format:

        **For a dashboard (frontend) change:**

        ```
        The automated tests have passed. For manual verification, please follow these steps:

        **Manual Verification Steps:**
        1. **Start the backend:** `cargo run -p sebenza-server --bin sebenza-server -- serve --port 5111`
        2. **Start the frontend dev server:** `cd frontend && npm run dev`
        3. **Open your browser to:** `http://localhost:5112`
        4. **Confirm that you see:** the new agent options listed in the Create Worktree dialog.
        ```

        **For a backend / CLI change:**

        ```
        The automated tests have passed. For manual verification, please follow these steps:

        **Manual Verification Steps:**
        1. **Ensure the server is running.**
        2. **Execute the following command in your terminal:** `sebenza-cli add test-wt --agent goose`
        3. **Confirm that you receive:** a worktree created on branch `test-wt` with a goose agent pane.
        ```

5.  **Await Explicit User Feedback:**

    -   After presenting the detailed plan, ask the user for confirmation:
        "**Does this meet your expectations? Please confirm with yes or provide
        feedback on what needs to be changed.**"
    -   **PAUSE** and await the user's response. Do not proceed without an
        explicit yes or confirmation.

6.  **Identify Target Commit for Report:**

    -   Do NOT create a new empty commit for checkpointing.
    -   Identify the hash of the last functional commit made during this phase.
        This will be the target for the verification report.

7.  **Attach Auditable Verification Report using Git Notes:**

    -   **Step 7.1: Draft Note Content:** Create a detailed verification report
        including the automated test command, the manual verification steps, and
        the user's confirmation.
    -   **Step 7.2: Attach Note:** Use the `git notes` command to attach the
        full report to the target commit identified in step 6.

8.  **Record Phase Checkpoint SHA:**

    -   **Step 8.1: Get Commit Hash:** Obtain the hash of the checkpoint target
        commit identified in step 6 (`git log -1 --format="%H"` if it is HEAD).
    -   **Step 8.2: Update Plan:** In `plan.json`, find the completed phase, set
        its `status` to `"done"`, and set its `checkpoint_sha` to the first 7
        characters of the commit hash.
    -   **Step 8.3: Sync `tracks.json`:** Update the phase's `phases_summary`
        entry and refresh `updated_at`.

9.  **Commit Plan Update:**

    -   **Action:** Stage the modified `plan.json` and `tracks.json` files.
    -   **Action:** Commit this change with a descriptive message following the
        format `sebenza(plan): Mark phase '<PHASE NAME>' as complete`.

10. **Announce Completion:** Inform the user that the phase is complete and the
    checkpoint has been created, with the detailed verification report attached
    as a git note.

### Quality Gates

Before marking any task complete, verify:

-   [ ] All tests pass (`cargo test`, `CI=true npm test`)
-   [ ] Code coverage meets requirements (>80%)
-   [ ] Code follows project's code style guidelines (as defined in
    `code_styleguides/`)
-   [ ] `cargo fmt --check` and `cargo clippy` are clean
-   [ ] `cd frontend && npm run check` (`tsc --noEmit`) is clean
-   [ ] All public functions/types are documented (`///` doc comments on public
    Rust items, module-level `//!` on new modules; JSDoc where non-obvious)
-   [ ] Type safety is enforced — no new `any` in TypeScript, no `unwrap()` on
    fallible paths in production Rust code
-   [ ] Any new backend route is added to the ts-rest contract
    (`frontend/src/lib/api-contract`) with a matching `api.ts` wrapper
-   [ ] CLI/UI parity preserved (see Guiding Principle 8)
-   [ ] Documentation updated if needed (`README.md`, `.ai/sebenza.example.yaml`)
-   [ ] No security vulnerabilities introduced

## Development Commands

### Setup

```bash
# Frontend dependencies
cd frontend && npm install && cd ..

# Rust toolchain: 1.85+ (2024 edition). Fetch dependencies:
cargo fetch
```

### Daily Development

```bash
# Backend: run the server for the current project with logs
cargo run -p sebenza-server --bin sebenza-server -- serve --port 5111

# Frontend: Vite dev server with hot reload (proxies /api + /ws to the backend)
cd frontend && npm run dev        # http://localhost:5112

# Tests
cargo test                        # Rust workspace
cd frontend && CI=true npm test   # frontend (vitest)
```

### Before Committing

```bash
cargo fmt --all
cargo clippy --all-targets
cargo test
cd frontend && npm run check && CI=true npm test
```

### Release Build

**Build order matters** — a release build bakes `frontend/dist` into the
`sebenza-server` binary, so the frontend must be built first. Rebuild the backend
after any frontend change to re-embed it.

```bash
cd frontend && npm run build && cd ..
cargo build --release
```

> Dev shortcut: set `SEBENZA_FRONTEND_DIST=/path/to/frontend/dist` to serve the
> SPA from disk without recompiling the server.

## Testing Requirements

### Unit Testing

-   Every module must have corresponding tests.
-   Rust: inline `#[cfg(test)] mod tests`, colocated in the module under test.
-   Frontend: colocated `*.test.ts` / `*.test.tsx` with vitest + Testing Library.
-   Mock external processes (`git`, `tmux`, `gh`, agent CLIs) rather than
    invoking them; prefer temp directories for filesystem tests and clean them
    up.
-   Test both success and failure cases.

### Integration Testing

-   Test complete worktree flows: create → open → send prompt → merge → remove.
-   Verify tmux session lifecycle and that sessions survive client disconnect.
-   Verify the ts-rest contract matches the server's actual routes.
-   Verify registry and config file read/write round-trips.

### Cross-Platform & Runtime Testing

Sebenza ships prebuilt binaries for Linux (x86-64, arm64) and macOS (Apple
Silicon), and supports both a host and a Docker worktree runtime.

-   Verify behaviour on **Linux and macOS** — path handling, `SHELL` resolution,
    and the systemd vs. launchd service units differ.
-   Exercise both **host** and **docker** runtimes for any change touching agent
    launch, pane commands, or environment passthrough.
-   Confirm generated artifacts (hook configs, `sebenza-agentctl`) are written
    idempotently and stay git-excluded.

## State & Compatibility

Sebenza has no database. Its state lives in config and JSON registries, so
compatibility is a schema concern rather than a migration concern:

-   **Project config** — `<repo>/.ai/sebenza.yaml` (+ `.ai/sebenza.local.yaml`).
    Adding a field must be backward-compatible: existing configs must keep
    loading. Update `.ai/sebenza.example.yaml` alongside any schema change.
-   **Registries** — `~/.ai/sebenza/` (project registry, instances). Treat these
    as on-disk contracts; never break a reader that predates your change.
-   **Agent session logs** — read-only, owned by the agent CLIs. Parsers must
    tolerate unknown fields and malformed lines without panicking.

## Code Review Process

### Self-Review Checklist

Before requesting review:

1.  **Functionality**

    -   Feature works as specified
    -   Edge cases handled
    -   Error messages state what failed and what to do next

2.  **Code Quality**

    -   Follows style guide
    -   DRY principle applied
    -   Clear variable/function names
    -   Layering respected: `domain` has no I/O, `adapters` wrap the outside
        world, `services` orchestrate

3.  **Testing**

    -   Unit tests comprehensive
    -   Integration tests pass
    -   Coverage adequate (>80%)

4.  **Security**

    -   No hardcoded secrets; control token never logged
    -   Input validation present
    -   Shell command construction is quoted/escaped (`quote_shell`) — no
        injection via branch names, prompts, or config templates
    -   Path traversal guarded on any filesystem-reading endpoint

5.  **Performance**

    -   No blocking I/O on the async runtime — use `spawn_blocking`
    -   PTY / WebSocket streams are drained so a full buffer cannot deadlock
    -   Polling loops have bounded retry budgets

6.  **Operability**

    -   Failures surface to the dashboard rather than dying silently
    -   New external-tool dependencies are optional and degrade gracefully when
        the binary is absent

## Commit Guidelines

### Message Format

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

-   `feat`: New feature
-   `fix`: Bug fix
-   `docs`: Documentation only
-   `style`: Formatting, missing semicolons, etc.
-   `refactor`: Code change that neither fixes a bug nor adds a feature
-   `test`: Adding missing tests
-   `chore`: Maintenance tasks

### Examples

```bash
git commit -m "feat(agents): Add goose to the built-in agent registry"
git commit -m "fix(tmux): Correct pane command quoting for branch names with spaces"
git commit -m "test(session): Add tests for opencode session id discovery"
git commit -m "refactor(adapters): Extract a session-log port behind a trait"
```

## Definition of Done

A task is complete when:

1.  All code implemented to specification
2.  Unit tests written and passing
3.  Code coverage meets project requirements
4.  Documentation complete (if applicable)
5.  Code passes `cargo fmt --check`, `cargo clippy`, and `npm run check`
6.  CLI/UI parity preserved
7.  Task marked `done` with its `commit_sha` recorded in `plan.json`
8.  Changes committed with proper message
9.  Git note with task summary attached to the commit

## Emergency Procedures

### Critical Bug in a Release

1.  Create hotfix branch from `main`
2.  Write failing test for bug
3.  Implement minimal fix
4.  Run the full suite on Linux and macOS if the fix is platform-sensitive
5.  Tag and cut a patch release
6.  Document in `plan.json`

### Corrupted State or Lost Worktree

1.  Stop the server to halt further writes
2.  Inspect `~/.ai/sebenza/` registries and the repo's `git worktree list` —
    they can drift apart
3.  Reconcile (`reconciliation` service) rather than hand-editing where possible
4.  Verify no branch or uncommitted work was lost before pruning anything
5.  Document the incident and harden the reconciliation path

### Security Breach

1.  Rotate the control token (`~/.config/sebenza/control-token`) immediately
2.  Review access logs
3.  Patch vulnerability
4.  Notify affected users (if any)
5.  Document and update security procedures

## Release Workflow

### Pre-Release Checklist

-   [ ] All tests passing
-   [ ] Coverage >80%
-   [ ] `cargo clippy` and `npm run check` clean
-   [ ] Cross-platform build verified (Linux x86-64/arm64, macOS Apple Silicon)
-   [ ] `README.md` and `.ai/sebenza.example.yaml` updated for any new config
-   [ ] Config backward-compatibility confirmed against an older `sebenza.yaml`

### Release Steps

1.  Merge feature branch to `main`
2.  Build the frontend, then the release binaries (order matters)
3.  Tag the release with its version
4.  Publish the GitHub Release with per-platform artifacts and checksums
5.  Verify `scripts/install.sh` resolves and installs the new version
6.  Smoke-test `sebenza-cli serve` from a clean directory

### Post-Release

1.  Check error logs and issue reports
2.  Gather user feedback
3.  Plan next iteration

## Continuous Improvement

-   Review workflow periodically
-   Update based on pain points
-   Document lessons learned
-   Keep things simple and maintainable
