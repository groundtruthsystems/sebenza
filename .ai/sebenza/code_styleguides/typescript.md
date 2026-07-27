# TypeScript Code Style

Builds on [general.md](./general.md) and [javascript.md](./javascript.md).

## Compiler
- Enable `strict` mode (and `noUncheckedIndexedAccess`) in `tsconfig.json`.
- Treat type errors as build failures; do not ship with `// @ts-ignore` unless justified with a comment.

## Types
- Prefer precise types over `any`; use `unknown` at boundaries and narrow explicitly.
- Model data with `interface` (extensible object shapes) or `type` (unions/aliases); use discriminated unions for variants.
- Derive types where possible (`ReturnType`, `Parameters`, `as const`) instead of duplicating them.
- Avoid non-null assertions (`!`); handle `null`/`undefined` explicitly.

## API design
- Export explicit types for public function inputs and outputs.
- Keep enums small or prefer string-literal unions.

## Everything else
- Follow the JavaScript guide for naming, `const`/`let`, async, and formatting (Prettier + ESLint with `@typescript-eslint`).
- Tests in TypeScript with **Vitest**/**Jest**; type test fixtures too.
