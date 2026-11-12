# Python Code Style

Builds on [general.md](./general.md).

## Environment & dependencies
- **`uv` is the only Python toolchain.** It manages the interpreter, the virtual environment, and dependencies — never `pip`, `poetry`, `pipenv`, `conda`, or a hand-rolled `python -m venv`.
- **Every Python project has a `.venv/` directory in its root, created and managed by uv, and that is the environment the project runs in.** Create it with `uv venv` or implicitly with `uv sync`; never hand-build or hand-patch it, and never commit it — `.venv/` belongs in `.gitignore`.
- **Run everything through `uv run`** (`uv run pytest`, `uv run python -m app`) so the project's `.venv` is used without relying on shell activation. Reach for `uvx` only for one-off tools that are not project dependencies.
- Declare dependencies in `pyproject.toml` and change them with `uv add` / `uv remove` — never by editing the manifest and installing by hand. **Commit `uv.lock`**: it is the reproducible build, and `uv sync` is what reconstructs `.venv` from it.
- Pin the interpreter with `requires-python` and a `.python-version` file, and let `uv python install` provide it rather than depending on whatever Python happens to be on `PATH`.

## Formatting & tooling
- Format with **Black** and lint with **Ruff** (or flake8), invoked through uv (`uv run black .`, `uv run ruff check .`); target the project's configured line length (Black default 88).
- Sort imports with **isort** / Ruff: stdlib, third-party, then local — each group separated by a blank line.
- Prefer an automated pre-commit hook so style is enforced, not debated.

## Naming
- `snake_case` for functions, methods, variables, and modules; `PascalCase` for classes; `UPPER_SNAKE_CASE` for constants.
- Prefix intentionally private names with a single underscore.

## Types & structure
- Add **type hints** to all public function signatures; check with **mypy** or **pyright** (`uv run mypy .`).
- Prefer `dataclasses` (or `pydantic` where validation is needed) over ad-hoc dicts for structured data.
- Keep functions small and single-purpose; avoid mutable default arguments.

## Idioms
- Use context managers (`with`) for resources; f-strings for formatting.
- Prefer comprehensions over `map`/`filter` when they stay readable; EAFP over LBYL where it reads clearly.
- Raise specific exceptions; never use a bare `except:`.

## Docs & tests
- Docstrings (PEP 257) on public modules, classes, and functions.
- Tests with **pytest**, run as `uv run pytest`; use fixtures, and mock external dependencies.
