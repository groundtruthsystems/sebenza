# Go Code Style

Builds on [general.md](./general.md). Follow *Effective Go* and the standard review comments.

## Formatting & tooling
- Always run **gofmt**/**goimports**; code that isn't gofmt-clean does not merge.
- Vet with `go vet` and a linter (**golangci-lint**).

## Naming
- Exported identifiers are `PascalCase`; unexported are `camelCase`. Keep names short but clear.
- Package names are short, lowercase, no underscores; avoid stutter (`http.Server`, not `http.HTTPServer`).
- Acronyms keep their case (`URL`, `ID`, `HTTP`).

## Errors
- Return `error` as the last value; check it immediately. Do not use panics for ordinary errors.
- Wrap with context: `fmt.Errorf("doing X: %w", err)`; compare with `errors.Is`/`errors.As`.

## Idioms
- Accept interfaces, return structs. Keep interfaces small (often one method).
- Use `defer` for cleanup; guard goroutines against leaks and pass `context.Context` for cancellation.
- Prefer zero-value-usable structs; avoid unnecessary pointers.

## Docs & tests
- Doc comments begin with the identifier name and are full sentences.
- Table-driven tests with the standard `testing` package; use `t.Run` subtests.
