# JavaScript Code Style

Builds on [general.md](./general.md).

## Formatting & tooling
- Format with **Prettier**; lint with **ESLint** (extend a shared config such as `eslint:recommended`).
- Use ES modules (`import`/`export`); avoid CommonJS in new code.

## Naming
- `camelCase` for variables and functions; `PascalCase` for classes and React components; `UPPER_SNAKE_CASE` for constants.
- Boolean names read as predicates (`isReady`, `hasChildren`).

## Language use
- Prefer `const`; use `let` only when reassigning; never `var`.
- Use `===`/`!==` (never `==`); prefer optional chaining (`?.`) and nullish coalescing (`??`).
- Favor pure functions and immutable updates (spread/`map`/`filter`) over in-place mutation.
- Use `async`/`await` over raw promise chains; always handle rejections.

## Structure
- Small modules with a single responsibility; avoid deep nesting (early returns).
- No unused exports or dead code.

## Docs & tests
- JSDoc on exported functions where types aren't otherwise obvious.
- Tests with **Jest** or **Vitest**; test success and failure paths; mock network/IO.
