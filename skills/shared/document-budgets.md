# Document Budgets (shared reference)

> Shared reference for all `sebenza-*` skills and architect agents. No frontmatter — loaded on demand.

Sebenza's written artifacts are **working documents, not essays**. They are read before code is
written and again while it is reviewed, so length is a cost, not a signal of rigour. Every section
below has a cap. The caps are deliberately tight enough to force a choice about what matters.

## Budgets

| Artifact / section | Cap |
|---|---|
| `design.md` **total** | ~600 words, excluding ` ```mermaid ` blocks |
| `## Overview` | ≤ 100 words |
| Each `<domain>-architecture.md` **total** | ~400 words, excluding ` ```mermaid ` blocks and tables |
| Each architecture domain narrative | ≤ 200 words |
| Each domain's **Decisions** | ≤ 5 bullets, one line each |
| Each domain's **Risks** | ≤ 5 bullets, one line each |
| `## Impact Analysis` | ≤ 10 bullets |
| `## Open Questions for Refinement` | ≤ 10 questions, one line each |
| `test-plan.md` **total** | ~900 words, excluding tables and ` ```mermaid ` blocks |
| `## Strategy` | ≤ 150 words |
| Each scenario row | one line; its Given/When/Then ≤ 25 words |
| `## Test Data & Environments` | ≤ 8 bullets, one line each |
| `## Entry & Exit Criteria` | ≤ 8 checkable one-liners |
| `spec.md` **total** | ~900 words |
| Each functional requirement | one numbered, testable line (≤ 25 words) |
| Each acceptance criterion | one checkable line |
| Each `plan.json` task `description` | one sentence |

Diagrams, tables, and threat-model rows are exempt from the word counts — they are the dense form the
prose is supposed to defer to. That exemption is why `test-plan.md` fits in 900 words: its scenario
and traceability tables carry nearly all of it, and the prose is only the strategy.

## Trim rules

1. **Over budget means cut.** Do not summarize the section and keep the long version underneath it.
2. **Detail that will not fit moves into a diagram, a table, or `## Open Questions for Refinement`** —
   never into more prose. An open question is the correct home for something that matters but is not
   yet decided.
3. **Never restate in prose what the diagram already shows.** The diagram is authoritative for
   structure; the narrative explains only what the picture cannot.
4. **Never restate the Product Definition, Tech Stack, or Workflow.** Link to them.
5. **Prefer a table or a bullet list to a paragraph.** Reserve prose for reasoning — trade-offs,
   rationale, consequences — which is the one thing a table cannot carry.
6. **No preamble, no recap.** Do not open a section by describing what the section is about, and do
   not close it by summarizing what was just said.

## Measuring

Count words with fenced blocks excluded:

```bash
sed '/^```/,/^```/d' design.md | wc -w     # likewise each <domain>-architecture.md
```

When you report a section as complete, check it against its cap first. If it is over, cut before
presenting it — not after the user complains about the length.

## Chat output is a separate budget

These caps govern the **files**. What you print in the terminal is governed by
[`conventions.md`](./conventions.md) (Terminal-readable output) and [`diagrams.md`](./diagrams.md):
summarize a written section in a few lines and give its path — never reproduce it in chat. A section
that is within its document budget is still too long to paste into a terminal.
