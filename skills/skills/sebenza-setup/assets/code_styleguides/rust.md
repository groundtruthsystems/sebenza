# Rust Code Style

Builds on [general.md](./general.md). Follows the official
[Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/) and
[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) — this document only records
where we commit to a choice the standard leaves open.

## Formatting & tooling
- **`cargo fmt` is the last command run on any touched Rust file.** Never hand-format, never commit
  unformatted Rust — after editing a `.rs` file, the final action before you stop is `cargo fmt`
  (project-wide) or `cargo fmt -- <file>` for a single file.
- Accept **rustfmt** defaults (4-space indent, 100-column width); configure only via `rustfmt.toml`,
  never inline overrides.
- Lint with **`cargo clippy`** and treat warnings as errors in CI (`cargo clippy -- -D warnings`).
- Pin the toolchain and edition in `rust-toolchain.toml` / `Cargo.toml`; prefer the latest stable
  edition.

## Naming
- `snake_case` for functions, methods, variables, modules, and crates; `PascalCase` for types,
  traits, and enum variants; `SCREAMING_SNAKE_CASE` for constants and statics.
- Conversions follow the standard: `as_` (cheap, borrowed), `to_` (expensive, owned), `into_`
  (consuming). Getters are named `field`, not `get_field`.
- Acronyms are treated as one word in `PascalCase` (`HttpClient`, not `HTTPClient`).

## Types & structure
- Prefer borrowing (`&str`, `&[T]`) over owned parameters (`String`, `Vec<T>`) in signatures.
- Model errors with an `enum` (via **`thiserror`** in libraries); use **`anyhow`** only at
  application boundaries. Propagate with `?`.
- **Each layer owns its error type and maps at its boundary.** A repository maps driver errors into
  its own enum, a service maps that into its own, and the transport layer maps service errors to
  responses in **one** place (middleware or a single `IntoResponse` impl) rather than per handler —
  a raw `sqlx::Error` reaching a handler means a layer leaked.
- **No `unwrap()`/`expect()`/`panic!` in library code** — return `Result`/`Option`. Reserve them for
  tests and truly unreachable invariants, and then `expect()` with a message, never bare `unwrap()`.
- Derive the standard traits where they apply (`Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`,
  `Default`); make public types `Debug` by default.

## Workspaces & project layout
- A single-crate project keeps its `Cargo.toml` at the root. **The moment a second crate appears,
  convert to a workspace**: a root `Cargo.toml` with `[workspace]` and every crate under a
  `crates/` directory (`crates/<name>/`).
- **Dependencies are inherited from the workspace.** Declare versions once in the root
  `[workspace.dependencies]`; each crate references them with `dep = { workspace = true }` — never
  pin a version in an individual crate's `Cargo.toml`.
- Share common package metadata (`edition`, `version`, `license`, `rust-version`) via
  `[workspace.package]` and inherit it with `field.workspace = true`.
- **Inside a service, dependencies flow one way: `handler → service → repository → database`.**
  Handlers never import repositories, services coordinate across repositories, and repositories
  never import services or handlers. The direction is the testable invariant — it is what lets a
  repository be exercised against a container without booting the transport layer.

## Serving HTML apps
- A served HTML/web app is **always embedded in the binary** (e.g. via `rust-embed` /
  `include_dir!`) so a single artifact ships with no external asset dependency.
- Always provide a runtime **override to serve from a folder** instead — a flag or env var (e.g.
  `--assets-dir` / `ASSETS_DIR`) that, when set, serves files from disk. Embedded is the default;
  the folder override exists for local development and hot-reload.

## Async
- **Never block the executor.** Use the runtime's async equivalents, and push genuinely blocking
  work (filesystem, CPU-bound, blocking C libraries) to `tokio::task::spawn_blocking`:

  ```rust
  tokio::time::sleep(Duration::from_secs(1)).await; // correct: yields the worker thread
  std::thread::sleep(Duration::from_secs(1)); // wrong: stalls every task on this thread
  ```

- **Clone or copy what an `async move` closure needs before entering it.** A borrow taken from the
  caller's arguments cannot outlive the closure, and the fix is a cheap clone at the boundary rather
  than a lifetime fight:

  ```rust
  pub async fn link_organisations(&self, user_id: &i64, organisations: &[i64]) -> RepoResult<()> {
      let user_id = *user_id;
      let organisations = organisations.to_vec();

      with_transaction(&self.db_pool, |tx| async move {
          for organisation_id in &organisations {
              // ... insert using user_id and organisation_id
          }
          Ok(())
      })
      .await
  }
  ```

- That concession is for the closure boundary only — inside loops and hot paths, be deliberate about
  clones and prefer borrowing the cloned local over cloning again per iteration.

## Data access & transactions
- **Reads may use the pool directly** — nothing is being mutated, so there is no transaction to
  join. Choose the fetch method by the cardinality you actually guarantee: `fetch_optional` for a
  single row that may be absent, `fetch_one` only when exactly one row is certain, `fetch_all` for
  a set:

  ```rust
  pub async fn get_user_profile(&self, user_id: i64) -> RepoResult<Option<UserProfile>> {
      sqlx::query_as::<_, UserProfile>("SELECT * FROM user_profile WHERE user_id = ?")
          .bind(user_id)
          .fetch_optional(&self.db_pool)
          .await
          .map_err(RepoError::from)
  }
  ```

- **Every mutation runs inside a transaction.** A mutation that owns its whole unit of work wraps
  its body in a `with_transaction` helper, so commit and rollback are handled once instead of at
  every early return:

  ```rust
  pub async fn save_invitations(&self, request: &InviteRequest) -> RepoResult<()> {
      let email = request.email.clone();
      let organisation_ids = request.organisation_ids.clone();

      with_transaction(&self.db_pool, |tx| async move {
          for organisation_id in &organisation_ids {
              sqlx::query("INSERT INTO user_invitation (email, organisation_id) VALUES (?, ?)")
                  .bind(&email)
                  .bind(organisation_id)
                  .execute(&mut **tx)
                  .await?;
          }
          Ok(())
      })
      .await
  }
  ```

- **A mutation that is only a fragment of a larger unit of work takes the transaction as its first
  parameter after `&self`** — `tx: &mut Transaction<'_, DB>` — and **never accepts a pool and a
  transaction together**, which leaves the caller guessing which one actually applies. Sequence and
  counter helpers have transactional variants for the same reason: the pool variant would leak its
  increment past a rollback.

  ```rust
  pub async fn register_user(
      &self,
      tx: &mut Transaction<'_, MySql>,
      entity_id: i64,
      request: &RegisterRequest,
  ) -> RepoResult<i64> {
      // The `_tx` variant: a rollback has to undo the counter too.
      let profile_id = increment_counter_tx("user_profile", tx, 1).await?;

      sqlx::query("INSERT INTO user (id, email) VALUES (?, ?)")
          .bind(entity_id)
          .bind(&request.email)
          .execute(&mut **tx)
          .await?;

      sqlx::query("INSERT INTO user_profile (id, user_id) VALUES (?, ?)")
          .bind(profile_id)
          .bind(entity_id)
          .execute(&mut **tx)
          .await?;

      Ok(entity_id)
  }
  ```

  The service that spans several repositories owns the transaction, threads it through each call,
  and commits once:

  ```rust
  let entities = repositories.entities();
  let users = repositories.users();

  let mut tx = pool.begin().await?;

  let entity_id = entities.register_entity(&mut tx, &identifier).await?;
  let user_id = users.register_user(&mut tx, entity_id, &request).await?;

  tx.commit().await?;
  ```

- **Inside a transaction, every statement executes on `&mut **tx`.** Reaching for the pool between
  `begin()` and `commit()` is a silent data-integrity bug rather than a style nit — the statements
  land outside the transaction, the commit has nothing to commit, and the rollback path undoes
  nothing:

  ```rust
  let mut tx = pool.begin().await?;
  for item in &items {
      sqlx::query("INSERT INTO audit_log (item) VALUES (?)")
          .bind(item)
          .execute(&self.db_pool) // wrong: writes on the pool, outside the transaction
          .await?;
  }
  tx.commit().await?; // commits nothing, and a rollback would have undone nothing
  ```

  Keep transactions short-lived — no network calls or user interaction while one is open — and
  propagate every failure with `?` so the guard rolls back on the error path.
- **Repository method names say whether they read or write.** Reads are `get_*`, `list_*`, `find_*`,
  `count_*`; writes are `create_*`, `register_*`, `update_*`, `delete_*`, `save_*`. A signature
  taking a `tx` is the marker that the method is one fragment of a larger atomic operation.
- **One pool per process**, constructed at the composition root and handed to repositories — never
  constructed inside a repository. A pool is `Clone` and `Arc`-backed internally, so hand out cheap
  clones instead of plumbing references through every layer.
- Prefer the **compile-time-checked macros** (`query_as!`, `query!`) wherever the project can give
  the compiler a schema — a live `DATABASE_URL` or a committed `.sqlx` offline cache. Where it
  cannot, the runtime `query_as::<_, T>` form is correct and expected; do not pretend a query is
  checked when nothing checks it.
- **List endpoints are paginated from the first commit**, not retrofitted: a page/size request type
  in, a `PagedResult<T>` carrying page metadata alongside the items out, and `LIMIT`/`OFFSET` bound
  as parameters. Batch child lookups in one query keyed by the parent ids rather than issuing a
  query per parent (N+1).

  ```rust
  let offset = pagination.size * pagination.page;
  let items = sqlx::query_as::<_, Invitation>(
      "SELECT * FROM user_invitation ORDER BY created_on DESC LIMIT ? OFFSET ?",
  )
  .bind(pagination.size)
  .bind(offset)
  .fetch_all(&self.db_pool)
  .await?;
  ```

## Idioms
- Prefer iterator adaptors (`map`, `filter`, `collect`) over manual index loops.
- Use `match`/`if let`/`let else` for control flow; keep `match` exhaustive rather than reaching for
  `_ =>` wildcards on closed enums.
- Return `impl Trait` for internal APIs; name concrete types in public ones.
- Minimize `unsafe`; every `unsafe` block carries a `// SAFETY:` comment justifying its invariants.

## Docs & tests
- `///` doc comments on all public items; `//!` for module/crate overviews. Document panics, errors,
  and safety in the standard `# Panics` / `# Errors` / `# Safety` sections.
- Every public repository method carries a doc comment, and it states the transaction boundary
  (does this method open one, or must the caller supply one?) plus any error case a caller has to
  branch on.
- Write runnable examples in doc comments — they double as **doctests**.
- Unit tests in a `#[cfg(test)] mod tests` block beside the code; integration tests in `tests/`.
  Run with `cargo test`. For services with a database, use **`testcontainers`** for MySQL/Redis and
  test transaction **rollback**, not just the happy path.
- **A rollback test asserts the absence of rows, not just an error.** Asserting `is_err()` alone
  passes even when the writes went to the pool instead of the transaction, which is the one bug the
  test exists to catch:

  ```rust
  #[tokio::test]
  async fn failed_invitation_leaves_no_rows() {
      let pool = start_mysql_container().await;
      let repository = InvitationRepository::new(pool.clone());

      let request = request_violating_a_constraint();
      let result = repository.save_invitations(&request).await;

      assert!(result.is_err());
      assert_eq!(count_invitations(&pool).await, 0); // the assertion that actually matters
  }
  ```

## Examples
- **Every crate has an `examples/` directory** that demonstrates how to execute its functions or
  drive its HTTP/gRPC/MCP APIs — a new reader should reach for `examples/` first.
- Each example is a self-contained `examples/<verb>_<noun>.rs` binary (`quick_create_prompt.rs`,
  `quick_execute_session.rs`) that runs end-to-end: acquire a token, make the call, print the
  result. Run with `cargo run --example <name>`.
- `println!`/`eprintln!` are allowed in `examples/` (and genuine CLI stdout) — this is the one place
  they are, since examples exist to show output.
