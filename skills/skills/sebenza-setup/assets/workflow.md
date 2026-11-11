# Project Workflow

## Guiding Principles

1.  **Design First:** Every feature or system change begins with the
    **architect** (`sebenza-architect`), never with planning or code. The
    design examines the change from all five perspectives — business,
    application, technical, data, and security — so that system changes are
    well thought out before any commitment is made.
2.  **The Plan is the Source of Truth:** All work must be tracked in `plan.json`
3.  **The Tech Stack is Deliberate:** Changes to the tech stack must be
    documented in `tech-stack.md` *before* implementation
4.  **Test-Driven Development:** Write unit tests before implementing
    functionality
5.  **High Code Coverage:** Aim for >80% code coverage for all modules
6.  **User Experience First:** Every decision should prioritize user experience
7.  **Non-Interactive & CI-Aware:** Prefer non-interactive commands. Use
    `CI=true` for watch-mode tools (tests, linters) to ensure single execution.

## Track Lifecycle

Every track moves through a fixed sequence of stages. The order is **mandatory**
and exists to ensure changes are considered from multiple perspectives before
they are built:

1.  **Architect (Design) — `sebenza-architect`:** ALWAYS the first step. Define
    the use cases (each with a stable `UC-nn` ID) and diagrams, then define
    **and verify** the architecture across the **business, application,
    technical, data, and security** domains (each contributed and verified by
    its specialist architect). As the **last** step, the `test-architect` turns
    those use cases and that architecture into a `test-plan.md` of `TS-nn`
    scenarios with a traceability table. The outputs are an approved `design.md`,
    one `<domain>-architecture.md` per domain, and `test-plan.md`. A feature
    MUST NOT proceed to planning without them.
2.  **Plan (Spec & Plan) — `sebenza-plan`:** Only after the design is
    approved. Refine the design into a detailed `spec.md` and a phased
    `plan.json` **that provisions every `TS-nn`**: each phase ends with a task
    carrying `test_scenarios`, and the plan ends with a verification phase that
    re-runs the whole test plan. A bug, chore, or refactor gets its test plan
    here instead, keyed on the spec's `AC-nn`. `sebenza-plan` halts on a feature
    that has no approved `design.md` and hands off to the architect.
3.  **Implement — `sebenza-implement`:** Execute the plan's tasks following the
    **Task Workflow** below.
4.  **Close Out — `sebenza-implement`:** Once every phase is `done` and project
    docs are synchronized, fold the track's threat model into the running STRIDE
    register in `.ai/sebenza/SECURITY.md` (top 50 threats by risk), record the
    track in the project-root `CHANGE_LOG.md` as a single paragraph of **at most
    four sentences**, then **delete the track** — its directory and its
    `tracks.json` entry. The threat model must be captured before the delete:
    it lives in `security-architecture.md`, which the delete destroys. `test-plan.md` is
    destroyed too and nothing is consolidated out of it — what survives is the
    tests it caused and the commit SHAs of the tasks that ran them. Completed tracks are
    never archived and never left in the registry; `CHANGE_LOG.md`, `SECURITY.md`,
    and git history are the permanent record. This step is mandatory, not
    optional.

**Exception:** Bugs, chores, and refactors may skip the architect stage and
start at Plan, where they are specified from scratch. Anything that changes
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

    -   Create a new test file for the feature or bug fix.
    -   Write one or more unit tests that clearly define the expected behavior
        and acceptance criteria for the task.
    -   **If the task carries `test_scenarios`, it is a test task.** Read those
        `TS-nn` from the track's `test-plan.md` and implement them as written,
        rather than inventing coverage. Report each scenario by ID as passed or
        failed when the task completes; a failing scenario blocks the task and
        is never recorded as done with a note attached. If the scenario itself
        turns out to be wrong, say so and correct `test-plan.md`.
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

6.  **Verify Coverage:** Run coverage reports using the project's chosen tools.
    For example, in a Python project, this might look like: `bash pytest
    --cov=app --cov-report=html` Target: >80% coverage for new code. The
    specific tools and commands will vary by language and framework.

7.  **Document Deviations:** If implementation differs from tech stack:

    -   **STOP** implementation
    -   Update `tech-stack.md` with new design
    -   Add dated note explaining the change
    -   Resume implementation

8.  **Commit Code Changes:**

    -   Stage all code changes related to the task.
    -   Propose a clear, concise commit message e.g, `feat(ui): Create basic
        HTML structure for calculator`.
    -   Perform the commit.

9.  **Attach Task Summary with Git Notes:**

    -   **Step 9.1: Get Commit Hash:** Obtain the hash of the *just-completed
        commit* (`git log -1 --format="%H"`).
    -   **Step 9.2: Draft Note Content:** Create a detailed summary for the
        completed task. This should include the task name, a summary of changes,
        a list of all created/modified files, and the core "why" for the change.
    -   **Step 9.3: Attach Note:** Use the `git notes` command to attach the
        summary to the commit. `bash # The note content from the previous step
        is passed via the -m flag. git notes add -m "<note content>"
        <commit_hash>`

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
        `sebenza(plan): Mark task 'Create user model' as complete`).

### Task Correction & Plan Amendment Workflows

When an implemented task or phase requires corrections, amendments, or additions, follow these standard workflows to maintain plan integrity and avoid untracked code drift:

1.  **In-Flight Refinements:** If minor gaps are found while a task is actively
    in-progress (`status: "doing"`), make the adjustments directly in the active
    implementation stream and ensure passing tests before committing.
2.  **Code Review Corrections (`sebenza-review`):** If issues are identified
    during or after a code review, instruct the agent to review your changes
    (e.g., *"run a review"* or triggering the action manually in compatible
    clients). The review agent will append a `Review Fixes` phase to `plan.json`
    so that correction tasks are formally tracked and checkpointed.
3.  **Logical State Reversions (`sebenza-revert`):** If a task implementation
    is fundamentally flawed or needs to be redone, instruct the agent to revert
    the changes (e.g., *"revert the last task"* or triggering the action
    manually in compatible clients). This safely rolls back associated git
    commits and resets the task's `status` in `plan.json` back to `"backlog"`
    (clearing its `commit_sha`) to allow a clean restart.

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
        -   For each remaining code file, verify a corresponding test file
            exists.
        -   If a test file is missing, you **must** create one. Before writing
            the test, **first, analyze other test files in the repository to
            determine the correct naming convention and testing style.** The new
            tests **must** validate the functionality described in this phase's
            tasks (`plan.json`).

3.  **Execute Automated Tests with Proactive Debugging:**

    -   Before execution, you **must** announce the exact shell command you will
        use to run the tests.
    -   **Example Announcement:** "I will now run the automated test suite to
        verify the phase. **Command:** `CI=true npm test`"
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

        **For a Frontend Change:** ``` The automated tests have passed. For
        manual verification, please follow these steps:

        **Manual Verification Steps:** 1. **Start the development server with
        the command:** `npm run dev` 2. **Open your browser to:**
        `http://localhost:3000` 3. **Confirm that you see:** The new user
        profile page, with the user's name and email displayed correctly. ```

        **For a Backend Change:** ``` The automated tests have passed. For
        manual verification, please follow these steps:

        **Manual Verification Steps:** 1. **Ensure the server is running.** 2.
        **Execute the following command in your terminal:** `curl -X POST
        http://localhost:8080/api/v1/users -d '{"name": "test"}'` 3. **Confirm
        that you receive:** A JSON response with a status of `201 Created`. ```

5.  **Await Explicit User Feedback:**

    -   After presenting the detailed plan, ask the user for confirmation:
        "**Does this meet your expectations? Please confirm with yes or provide
        feedback on what needs to be changed.**"
    -   **PAUSE** and await the user's response. Do not proceed without an
        explicit yes or confirmation.

6.  **Identify Target Commit for Report:**

    -   Do NOT create a new empty commit for checkpointing.
    -   Identify the hash of the last functional commit made during this phase. This will be the target for the verification report.

7.  **Attach Auditable Verification Report using Git Notes:**

    -   **Step 7.1: Draft Note Content:** Create a detailed verification report
        including the automated test command, the manual verification steps, and
        the user's confirmation.
    -   **Step 7.2: Attach Note:** Use the `git notes` command to attach the full report to the target commit identified in step 6.

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

-   [ ] All tests pass
-   [ ] Code coverage meets requirements (>80%)
-   [ ] Code follows project's code style guidelines (as defined in
    `code_styleguides/`)
-   [ ] All public functions/methods are documented (e.g., docstrings, JSDoc,
    GoDoc)
-   [ ] Type safety is enforced (e.g., type hints, TypeScript types, Go types)
-   [ ] No linting or static analysis errors (using the project's configured
    tools)
-   [ ] Works correctly on mobile (if applicable)
-   [ ] Documentation updated if needed
-   [ ] No security vulnerabilities introduced
-   [ ] `.ai/sebenza/tracks.json`'s `project.path` equals
    `git rev-parse --show-toplevel` (the working root you are in, git worktree
    included)

## Development Commands

**AI AGENT INSTRUCTION: This section should be adapted to the project's specific
language, framework, and build tools.**

### Setup

```bash
# Example: Commands to set up the development environment (e.g., install dependencies, configure database)
# e.g., for a Node.js project: npm install
# e.g., for a Go project: go mod tidy
```

### Daily Development

```bash
# Example: Commands for common daily tasks (e.g., start dev server, run tests, lint, format)
# e.g., for a Node.js project: npm run dev, npm test, npm run lint
# e.g., for a Go project: go run main.go, go test ./..., go fmt ./...
```

### Before Committing

```bash
# Example: Commands to run all pre-commit checks (e.g., format, lint, type check, run tests)
# e.g., for a Node.js project: npm run check
# e.g., for a Go project: make check (if a Makefile exists)
```

## Testing Requirements

### Unit Testing

-   Every module must have corresponding tests.
-   Use appropriate test setup/teardown mechanisms (e.g., fixtures,
    beforeEach/afterEach).
-   Mock external dependencies.
-   Test both success and failure cases.

### Integration Testing

-   Test complete user flows
-   Verify database transactions
-   Test authentication and authorization
-   Check form submissions

### Mobile Testing

-   Test on actual iPhone when possible
-   Use Safari developer tools
-   Test touch interactions
-   Verify responsive layouts
-   Check performance on 3G/4G

## Code Review Process

### Self-Review Checklist

Before requesting review:

1.  **Functionality**

    -   Feature works as specified
    -   Edge cases handled
    -   Error messages are user-friendly

2.  **Code Quality**

    -   Follows style guide
    -   DRY principle applied
    -   Clear variable/function names
    -   Appropriate comments

3.  **Testing**

    -   Unit tests comprehensive
    -   Integration tests pass
    -   Coverage adequate (>80%)

4.  **Security**

    -   No hardcoded secrets
    -   Input validation present
    -   SQL injection prevented
    -   XSS protection in place

5.  **Performance**

    -   Database queries optimized
    -   Images optimized
    -   Caching implemented where needed

6.  **Mobile Experience**

    -   Touch targets adequate (44x44px)
    -   Text readable without zooming
    -   Performance acceptable on mobile
    -   Interactions feel native

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
git commit -m "feat(auth): Add remember me functionality"
git commit -m "fix(posts): Correct excerpt generation for short posts"
git commit -m "test(comments): Add tests for emoji reaction limits"
git commit -m "style(mobile): Improve button touch targets"
```

## Definition of Done

A task is complete when:

1.  All code implemented to specification
2.  Unit tests written and passing
3.  Code coverage meets project requirements
4.  Every `TS-nn` in the task's `test_scenarios` has been run and passed
5.  Documentation complete (if applicable)
6.  Code passes all configured linting and static analysis checks
7.  Works beautifully on mobile (if applicable)
8.  Task marked `done` with its `commit_sha` recorded in `plan.json`
9.  Changes committed with proper message
10. Git note with task summary attached to the commit

## Emergency Procedures

### Critical Bug in Production

1.  Create hotfix branch from main
2.  Write failing test for bug
3.  Implement minimal fix
4.  Test thoroughly including mobile
5.  Deploy immediately
6.  Document in `plan.json`

### Data Loss

1.  Stop all write operations
2.  Restore from latest backup
3.  Verify data integrity
4.  Document incident
5.  Update backup procedures

### Security Breach

1.  Rotate all secrets immediately
2.  Review access logs
3.  Patch vulnerability
4.  Notify affected users (if any)
5.  Document and update security procedures

## Deployment Workflow

### Pre-Deployment Checklist

-   [ ] All tests passing
-   [ ] Coverage >80%
-   [ ] No linting errors
-   [ ] Mobile testing complete
-   [ ] Environment variables configured
-   [ ] Database migrations ready
-   [ ] Backup created

### Deployment Steps

1.  Merge feature branch to main
2.  Tag release with version
3.  Push to deployment service
4.  Run database migrations
5.  Verify deployment
6.  Test critical paths
7.  Monitor for errors

### Post-Deployment

1.  Monitor analytics
2.  Check error logs
3.  Gather user feedback
4.  Plan next iteration

## Continuous Improvement

-   Review workflow weekly
-   Update based on pain points
-   Document lessons learned
-   Optimize for user happiness
-   Keep things simple and maintainable