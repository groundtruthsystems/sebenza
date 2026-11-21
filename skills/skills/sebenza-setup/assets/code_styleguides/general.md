# General Code Style Principles

This document outlines general coding principles that apply across all languages and frameworks used in this project.

## Readability
- Code should be easy to read and understand by humans.
- Let names and structure carry the meaning. A well-named function, variable or type replaces the
  comment that would have explained it.
- When a block needs a comment to say what it does, extract it into a function named for that.
- Keep comments for why: a reason, a constraint, or a surprising choice the code cannot show.
- Avoid overly clever or obscure constructs.

## Consistency
- Follow existing patterns in the codebase.
- Maintain consistent formatting, naming, and structure.

## Simplicity
- Prefer simple solutions over complex ones.
- Break down complex problems into smaller, manageable parts.

## Maintainability
- Write code that is easy to modify and extend.
- Minimize dependencies and coupling.

## Documentation
- Document *why* something is done, not just *what*.
- Keep documentation up-to-date with code changes.

## Review Rules

The standards review enforces only the rules tagged below, on new and changed code. Each tag
names the rule and its category; edit the prose or `max_lines` to suit this project, or delete a
rule to stop enforcing it.

- [rule:no-restating-comments category=restating-comments] A comment explains why the code is the way it is, or intent the code cannot show. It never restates what the code does.
- [rule:no-excessive-comments category=excessive-comments] Comment only where intent is not obvious. Do not narrate code line by line, and leave no commented-out code.
- [rule:long-methods category=long-methods max_lines=20] Keep each function or method body to at most 20 logical lines. Blank lines, comment-only lines and the signature do not count. Extract well-named functions instead.
