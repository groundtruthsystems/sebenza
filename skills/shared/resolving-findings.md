# Resolving Review Findings (shared reference)

> Shared reference for every `sebenza-*` skill that receives findings from a reviewer — a verifying
> architect, the comprehension or zoom-out reviewer, or the independent project reviewer. No
> frontmatter — loaded on demand. Adapted from the resolve step of the Elephant-Goldfish Model
> (google/elephant-goldfish-model, Apache-2.0).

You wrote the documents under review, and you remember the conversation that produced them. The
reviewer has neither. That gap is the point of the review: it finds what the documents do not say.
It also means **you are the party most likely to talk yourself out of a valid finding.** A wish to
contest is not evidence that the reviewer is wrong.

## Classify every finding

Give each finding exactly one of these:

| Class | When | What you do |
|---|---|---|
| **FIX** | The finding is valid. | Revise the document. For substantive rework, re-invoke the owning agent in Contribution mode. |
| **CONTEST** | The finding is wrong, and you can cite evidence **the reviewer could have read**: a file and symbol, or a decision already written in the track's documents. | Do not change the document. Send the finding and your evidence back to the same reviewer on the re-run. |
| **ESCALATE** | The finding is wrong, but the evidence is somewhere no reviewer could read it: a team convention, a spoken decision, an operator preference. | Do not change the document. Put it to the user — in a linked worktree, raise an inbox request per [`inbox.md`](./inbox.md#raising-one). |
| **DECLINE** | The finding is advisory (a `Nit`, or an architect's CONCERNS item) and not worth the words. | Record it in the verdict table, or under `## Open Questions for Refinement` if it is a question. Fixing it anyway is fine. |

## Rules

- **A contest must be checkable.** "This was intentional" or "we discussed this" is not evidence. If
  you cannot point at something the reviewer could read, the class is ESCALATE or FIX.
- **A contest goes back to the reviewer.** On the re-run, give the reviewer the finding and your
  evidence. If it withdraws the finding, the finding is closed. If it maintains it, ESCALATE — you do
  not get to overrule a reviewer twice.
- **Never contest a comprehension gap.** If a fresh reader could not follow the document, the document
  is missing something, whatever you know from the conversation. The class is FIX.
- **Never change a severity.** If you think a must-fix finding is minor, that is a CONTEST with
  evidence, not a quiet downgrade to DECLINE.
- **Do not add text only to satisfy a reviewer.** Add context a human reader of the document needs.
  If only a zero-context subagent would need it, ESCALATE instead, and say so. This keeps the
  documents inside their budgets ([`document-budgets.md`](./document-budgets.md)).
- **DECLINE is never available for must-fix findings**: anything behind a FAIL verdict, a `Blocker`
  or `Major`, a Critical or High security finding, or an uncovered use case.
- **The independent reviewer reads only its snapshot.** In `sebenza-architect` §7a there is no
  channel to hand it your evidence, so a CONTEST must cite something already in the worktree. If the
  next round repeats the finding, treat it as ESCALATE: it travels in the approval request.

## Report

In the verdict table you present to the user, give each reviewer's finding counts by class, and list
every ESCALATE first: those need the user's knowledge, not only their approval. Point at the files for
the substance of any change rather than reproducing it in chat.
