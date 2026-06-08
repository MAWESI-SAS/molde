# Handoff — molde-rename-cli-sync
Date (UTC): 20260608T142232Z | Status: WIP
Prev handoff: handoffs/HANDOFF_20260608T000000Z_efm-language-pivot.md
Base commit: 6a117f8 | Branch: main

## 1. Session Objective
- Large session, multiple threads on top of the EFM pivot. Net result:
  - Project **renamed `efrust` → `molde`** (binary, all crates, language EFM→molde).
  - **Entire codebase + docs translated to English** (was Spanish).
  - **CLI redesigned**: friendly + interactive (`pull/migrate/apply/status/undo/fmt`),
    replacing the `dotnet ef`-style surface.
  - Added **`molde fmt`**, an **EFM language server** (`molde-lsp`) + **VS Code extension**.
  - New feature **`molde sync`** (crate `molde-sync`): additive live DB→DB sync,
    ported from a C# tool, multi-engine. **PostgreSQL, SQLite, MySQL done + e2e-verified.**
- Status WIP: **SQL Server `sync` engine is the only remaining piece** the user asked for ("a todos").

## 2. Current Work Summary
- DONE (committed, tree clean at 6a117f8):
  - Rename efrust→molde, EFM→molde (`EfmError`→`MoldeError`, lang id `molde`, grammar `source.molde`).
    `__EFMigrationsHistory` **intentionally kept** (DB compat).
  - English translation of all crates/docs/CLI (6 parallel subagents + infra files by hand).
  - CLI flat commands + interactivity via `dialoguer`/`indicatif`/`console` (module `commands/ui.rs`).
  - `molde fmt` + lang API `format_model`/`outline`; `molde-lsp` (tower-lsp); VS Code ext in `editors/vscode/`.
  - `molde sync` + `molde-sync` crate: shared `DbSchema`+additive `diff`, trait `SyncEngine`,
    engines Postgres/SQLite/MySQL. PG CHECK cosmetic-conflict normalization.
- PENDING (next session): **SQL Server engine** for `molde-sync` (the user confirmed "all engines, Postgres first"; PG/SQLite/MySQL shipped, SQL Server not started).
- Decisions treated as true:
  - `sync` is a **faithful text-based catalog port**, NOT IR-based (IR is lossy: no views/checks/history).
  - Engine selection is by connection string in `engine_for` (URL scheme). SQL Server uses an **ADO string** (no scheme) → `engine_for` must be extended to detect it (e.g. contains `Server=`/`Data Source=`).

## 3. Exact Files Modified (this session, vs prev handoff baseline)
Span `6d971ea..6a117f8` = 92 files. Highlights for continuation:
- **NEW crate `crates/molde-sync/`** (commits 160b46d, d4c8e2e, a99def3, 6a117f8):
  - `src/schema.rs` — `DbSchema`, `*Info` types, `Conflict`, `DiffResult`, `diff()`. **Engine-agnostic; do not special-case engines here.** `TableInfo.create_sql: Option<String>` (SQLite/MySQL carry full CREATE; PG/SQLServer reconstruct → None).
  - `src/engine.rs:30` — `engine_for(conn)` dispatch (pg/sqlite/mysql). **Add SQL Server here.**
  - `src/postgres.rs` — reader (pg_catalog), `build_ddl`, `normalize_constraint_def` (silences `(ARRAY[..])::text[]` ↔ distributed cosmetic CHECK conflicts), `apply` via `sqlx::raw_sql` wrapped `BEGIN/COMMIT`. **Best template for SQL Server (reconstruct-from-catalog).**
  - `src/sqlite.rs` — sqlite_master + PRAGMA; emits via stored `create_sql`; no constraints/functions/extensions.
  - `src/mysql.rs` — information_schema + `SHOW CREATE TABLE`; `mstr`/`mopt` helpers (MySQL info_schema returns VARBINARY); view bodies de-qualified of source DB; `FOREIGN_KEY_CHECKS` toggled; DDL non-transactional.
  - `Cargo.toml` — features `tls-rustls`(default)/`tls-native-tls`; deps `sqlx`, `async-trait`, `tokio`, `anyhow`.
- `crates/molde-cli/src/commands/sync.rs` — CLI orchestration; `run()` resolves source/target (flag→env `MOLDE_SYNC_{SOURCE,TARGET}`→prompt), summary, `.sql` file (atomic header), confirm/apply. **Non-interactive never applies without `--yes`** (sync.rs ~line 95).
- `crates/molde-cli/src/commands/{pull,apply,migrate,fmt,ui}.rs` + `main.rs` — new CLI surface.
- `crates/molde-cli/Cargo.toml` — tls feature passthrough now includes `molde-sync/...`.
- `Cargo.toml` (workspace) — added `crates/molde-sync` member + dep.
- Rename touched every crate (efrust_*→molde_*), `editors/vscode/`, `.devcontainer/`, `docs/`, `README.md`.

## 4. Architectural Context
- `molde-sync` is **independent of molde's IR** (`molde-core::DatabaseModel`). It reads raw catalog text. Keep it that way.
- Trait `SyncEngine` (`engine.rs`): `name / read_schema(conn)->DbSchema / write_ddl(&DiffResult)->String / apply(conn,body) / redact(conn)`. Uses `#[async_trait]` (needed for `Box<dyn>`; native async-in-trait isn't dyn-safe).
- sqlx executor + async_trait pitfall: **use a `Pool` (`&Pool` executor), not `&mut Connection`** (the latter triggers "Executor not general enough"). All engines connect with `*PoolOptions::new().max_connections(1)`.
- Two table-emission models: **reconstruct from columns** (PG; SQL Server should follow) vs **verbatim `create_sql`** (SQLite/MySQL). The shared `TableInfo.create_sql` accommodates both.
- CLI `sync.rs` compares `engine_for(source).name() == engine_for(target).name()`; both conns must be same engine.

## 5. Important Runtime / Business Rules
- [verified] `sync` is strictly **additive**: source-only→new; both-but-different→**conflict (reported, never applied)**; target-only→ignored (local work preserved).
- [verified] `__EFMigrationsHistory` table name is preserved verbatim across the whole repo (DB compat). `\bEFM\b` was used in the rename so it never matched `EFMigrationsHistory`.
- [verified] **TLS**: `db.mawesi.online:5443` serves an **X.509 v1 cert** → rustls rejects (`UnsupportedCertVersion`). Build with `--no-default-features --features tls-native-tls` for any real connection to it.
- [verified] Postgres CHECK constraints deparse non-round-trip (`(ARRAY[..])::text[]` → distributed). `normalize_constraint_def` canonicalizes at read time → no cosmetic conflicts. SQL Server may have analogous deparse quirks (watch CHECK/computed defs).
- [verified] MySQL DDL is **not transactional**; `information_schema` text cols come back VARBINARY; view definitions are source-DB-qualified (must strip).
- [verified] SQLite has no `ADD CONSTRAINT`/functions/extensions; constraints ride inline in `create_sql`.
- [suspected] SQL Server: no `CREATE TABLE IF NOT EXISTS` → must use `IF OBJECT_ID(N'[s].[t]','U') IS NULL BEGIN … END`, `IF COL_LENGTH(...) IS NULL …`; reconstruct PK/FK/unique/CHECK from `sys.*` + `OBJECT_DEFINITION`.

## 6. Known Problems / Pending Risks
1. **HIGH** — 4 stray generated `sync-*.sql` (~17k lines) committed to **repo root** (entered in 160b46d, a99def3). Impact: junk in repo. Action: `git rm sync-*.sql`, add `sync-*.sql` to `.gitignore`, commit. (`molde sync` default `--out` is cwd `sync-<ts>.sql`; e2e runs without `--out` dropped them in `/app`=repo root, then `git add -A` swept them in.)
2. **MED** — SQL Server `sync` engine missing (the requested remaining work). Action: §7.
3. **MED** — Editor artifacts stale after rename: installed VS Code ext is old (`efm` lang id), and `~/.local/bin/efrust-lsp` / `editors/vscode/server/efrust-lsp` predate the rename. `.vscode/settings.json` now points to `…/server/molde-lsp` (not yet built). Action: rebuild static musl `molde-lsp`, repackage vsix, reinstall — only if user resumes editor use.
4. **LOW** — MySQL `sync` v1 omits separate indexes/constraints on existing tables + functions/procedures/triggers (delimiter/non-transactional). Documented in `mysql.rs` module doc.
5. **LOW** — Test DBs left on `db.mawesi.online`: `prueba_efrust`, `prueba_sync` (source `mwssaas_sm_test_aqualia_v2` is real, untouched). `_scaffold_review/` is local scratch (gitignored).

## 7. Next Recommended Actions
1. **Clean the stray SQL**: `git rm sync-20260608-*.sql && printf 'sync-*.sql\n' >> .gitignore && git commit`. Verify: `git ls-files | grep -c 'sync-.*sql'` → 0.
2. **SQL Server engine** `crates/molde-sync/src/sqlserver.rs`:
   - Add `tiberius` + `tokio-util` deps to `crates/molde-sync/Cargo.toml` (copy feature/dep shape from `crates/molde-scaffold/Cargo.toml`); add `tls-native-tls` passthrough.
   - Implement `SyncEngine` for `SqlServerEngine` using tiberius (connection pattern in `molde-scaffold/src/reader.rs` SQL Server path and `molde-migrate/src/lib.rs` `Backend::Mssql`).
   - Reader from `sys.*`/`INFORMATION_SCHEMA` + `OBJECT_DEFINITION(...)` for views/functions/triggers/checks; reconstruct PK/FK/unique like `postgres.rs` constraints.
   - `write_ddl`: `[..]` quoting; idempotency via `IF OBJECT_ID/COL_LENGTH/NOT EXISTS(...)` wrappers.
   - `engine.rs:engine_for`: detect ADO string (`Server=`/`Data Source=` and no `://`).
   - Register in `lib.rs` (mod + `pub use`).
   - e2e: `docker run -d --name molde-mssql -e ACCEPT_EULA=Y -e MSSQL_SA_PASSWORD=... mcr.microsoft.com/mssql/server:2022-latest` (~2GB), two DBs, sync. Verify `cargo test -p molde-sync` + dry-run/apply/idempotency.
3. Optional: update `README.md` capability table for `sync` (4 engines) once SQL Server lands.

## 8. Validation Performed (this session)
- `cargo test --workspace` (rust:1-bookworm via Docker, volumes `molde-target`/`efrust-cargo-reg`/`efrust-rustup`) → PASS at each phase; final molde-sync **9 tests**, workspace green (core 11, design 4, lang 6+5, migrate 1, providers 19, scaffold 3 +4 ignored, lsp 3).
- `cargo clippy --workspace --all-targets -- -D warnings` → clean. `cargo fmt --all --check` → clean.
- e2e (real Postgres `db.mawesi.online`, native-tls binary): `molde sync` synced `aqualia_v2`→empty `prueba_sync` (extensions+220 tables+520 FK+triggers+views+169 history), atomic, **idempotent re-run = 0 changes, 0 conflicts** after CHECK normalization.
- e2e SQLite (two `/tmp/*.db`): new table+column+index synced; idempotent.
- e2e MySQL 8 (container): new table(FK inline)+column+view synced; view re-qualified to target; idempotent.
- NOT tested: SQL Server (not implemented); the VS Code extension/LSP under the new `molde` name (artifacts stale); `molde sync` against MySQL functions/triggers (out of v1 scope).

## 9. Recommended Entry Points for Next Session
- `crates/molde-sync/src/engine.rs:30` → `engine_for` — add SQL Server dispatch (ADO detection).
- `crates/molde-sync/src/postgres.rs` → `PostgresEngine` impl + `build_ddl`/`read_constraints` — closest template for SQL Server (reconstruct-from-catalog).
- `crates/molde-sync/src/schema.rs` → `DbSchema`/`diff` — shared contract; read but don't modify per-engine.
- `crates/molde-scaffold/src/reader.rs` (SQL Server section) + `crates/molde-migrate/src/lib.rs` → tiberius connection/query patterns to reuse.
- `crates/molde-cli/src/commands/sync.rs` → CLI flow; same-engine check ~line 62.
- `crates/molde-sync/src/mysql.rs` → `mstr`/`mopt` + module doc — pattern for engine-specific quirks + documenting v1 limits.

## 10. Context That Should NOT Be Lost
- User preference (memory `molde-language-english`): **code/CLI in English, not Spanish** (project to be open-sourced). Chat replies stay Spanish; artifacts English.
- Why `sync` is a faithful catalog port, not IR-reuse: IR doesn't model views/check-constraints/generated-cols/history and would create false conflicts via normalization. Investigated and rejected IR reuse.
- The 5 "phantom CHECK conflicts" were **not a bug** — PostgreSQL deparse (`pg_get_constraintdef`) is not round-trip-stable for array-cast-in-`ANY`; fixed via `normalize_constraint_def`. Don't re-chase as a real diff.
- sqlx + `#[async_trait]` "Executor not general enough" wasted time once — the fix is **Pool, not &mut Connection**. Apply this immediately in the SQL Server engine (tiberius doesn't have this issue, but if any sqlx is used, remember).
- Build/test/e2e all run via Docker (`rust:1-bookworm`); host has **no cargo/node** and **glibc 2.31** (binaries to run on host need static musl). Real-DB e2e needs `--features tls-native-tls` (X.509 v1 cert).
- The user develops inside a **Dev Container** (repo at `/workspaces/EF_RUST`), Remote-WSL. Paths differ from host `/home/mauricio/...`.
- `__EFMigrationsHistory` is the EF/Sequelize history table; molde keeps it for interop with existing MAWESI databases — never rename it.
