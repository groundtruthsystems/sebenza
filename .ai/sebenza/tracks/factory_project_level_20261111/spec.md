# Spec: Sebenza Software Factory — project level

Shares the design and test plan of [software_factory_20261111](../software_factory_20261111/design.md) ([test-plan.md](../software_factory_20261111/test-plan.md)). This track owns UC-07..UC-11 (P1–P4) and builds on that track's skills move, factory config, operator credential and role runner.

## Overview
Each project worktree created from an approved handover runs a more structured Sebenza workflow:
- Architecture runs in a fixed order: business; then application ∥ data; then technical ∥ security; then test.
- An independent, daemon-run reviewer checks the design, capped at 3 rounds.
- Implementation waits for operator approval.
- After the tracks finish, a code review (≤3 rounds) can send tasks back to `doing`.
- The operator does the final verification.

## Functional Requirements
- FR-01 `sebenza-architect` runs business first, then application ∥ data with the business document and `solution-handover.md`, then technical ∥ security, then test.
- FR-02 `sebenza-agentctl review --level design|code --track <id>` starts a daemon review job, polls it and prints the verdict and findings.
- FR-03 The daemon runs the configured project reviewer read-only on a snapshot (tracked + untracked-unignored, excluding agent config, secrets, `.git`), with hooks disabled.
- FR-04 A post-run `git status`/HEAD or snapshot change voids the round and opens a request.
- FR-05 ReviewGate counts rounds per scope key `<project>@<branch>/<track>`, persisted on the origin item, and refuses round 4 with 409 `round_limit`.
- FR-06 The reviewer profile resolves item override, then global `project.reviewer`; the independence rule applies, and Grok is refused with a reason.
- FR-07 An unlinked worktree reviews in-session and records the round locally, flagged `independent: false`.
- FR-08 After design review, the skill raises an `[approval]` request; `inbox.py approval` refuses implementation until it is answered.
- FR-09 Project requests appear on the origin item and raise Projects-screen notifications (existing channel).
- FR-10 `sebenza-review` runs code review against the code style guides; findings set the named tasks to `doing` with `review_round` and `review_findings`.
- FR-11 After acceptance or round 3, the skill raises a final-verification request carrying any open findings.
- FR-12 A new `project-reviewer` agent prompt (design and code levels) lives in `skills/agents` and is embedded in the server.
- FR-13 Each skill change is recorded in `skills/MIGRATION.md` under post-move changes.

## Acceptance Criteria
- AC-01 (FR-01) The skill order is enforced in the architect skill — TS-25.
- AC-02 (FR-02..06, FR-12) Independent read-only review on a snapshot, with tamper voiding — TS-26, TS-40, TS-42, TS-57, TS-17.
- AC-03 (FR-05) **Project design review is capped at 3 rounds and round 4 is refused, even for a re-created worktree** — TS-59, TS-54.
- AC-04 (FR-07) Unlinked review is flagged not independent — TS-27.
- AC-05 (FR-08, FR-09) **Implementation waits for explicit operator approval**; requests notify — TS-28, TS-29.
- AC-06 (FR-10) **Code review sends named tasks back to `doing`; it is capped at 3 rounds and round 4 is refused** — TS-30, TS-31.
- AC-07 (FR-11) Final verification is requested with open findings after acceptance or round 3 — TS-32, TS-33.

## Out of Scope
- Solution-level features (sibling track).
- Daemon-enforced refusal of implementation: a shell agent can always edit code, so the gate is skill-enforced.

## Open Questions
- Does a 600 s job timeout suffice for large code reviews? Revisit after TS-26 runs on a real repository.
