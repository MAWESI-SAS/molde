# Handoff — molde-sync-sqlserver-done
Date (UTC): 20260608T160000Z | Status: DONE
Prev handoff: handoffs/HANDOFF_20260608T142232Z_molde-rename-cli-sync.md
Base commit: 806f9e0 | Branch: main

## 1. Session Objective
Close the WIP from the previous handoff:
1. Remove the stray generated `sync-*.sql` dumps committed to the repo root.
2. Implement the **SQL Server engine** for `molde-sync`, completing all four `sync` engines.

Both done and verified. Tree clean at `806f9e0`.

## 2. What Was Done
- **Cleanup (commit 64c876b)**: `git rm` of 4 stray `sync-20260608-*.sql` (~1.2 MB) from repo root; added `sync-*.sql` to `.gitignore`. `git ls-files | grep -c sync.*sql` → 0.
- **SQL Server engine (commit 806f9e0)** — `crates/molde-sync/src/sqlserver.rs` (new, ~590 LoC + tests):
  - `SqlServerEngine` implements `SyncEngine` over tiberius/TDS (same connection pattern as `molde-scaffold`/`molde-migrate`).
  - Reader (`dbo` schema) from `sys.*` + `INFORMATION_SCHEMA` + `OBJECT_DEFINITION(...)`: tables, columns (type via `store_type`, identity `IDENTITY(seed,incr)` folded into the type text, computed columns as `is_generated`), PK/unique/check/FK constraints (reconstructed text), non-constraint indexes, functions/procedures, triggers, views, `__EFMigrationsHistory`.
  - Writer reconstructs additive DDL like `postgres.rs`. Idempotency:
    - tables/columns/constraints/indexes → plain `IF OBJECT_ID/COL_LENGTH/NOT EXISTS(...)` guards.
    - views/functions/triggers → guarded **dynamic SQL** (`SET @sql = N'…' + N'…'; IF OBJECT_ID(...) IS NULL EXEC sp_executesql @sql;`) because `CREATE VIEW/FN/TRIGGER` must be first-in-batch. Definitions are chunked into `N'…'` literals ≤2000 chars (avoids the silent >4000-char literal truncation) without splitting an escaped `''` pair (`dynamic_literal`).
    - `apply` wraps the whole body in `SET XACT_ABORT ON; BEGIN TRANSACTION … COMMIT` (SQL Server DDL is transactional).
  - `engine.rs`: `engine_for` now detects the **ADO string** (no `://`, contains `server=`/`data source=`) → SQL Server. New default trait method `wrap_script(body)` makes the portable `.sql` transaction wrapper engine-specific (SQL Server overrides to `BEGIN TRANSACTION`; others keep `BEGIN;`). Added a dispatch unit test.
  - `lib.rs`: `mod sqlserver` + `pub use SqlServerEngine`.
  - `Cargo.toml`: added `tiberius 0.12` (`tds73`,`rustls`) + `tokio-util` (`compat`); `tokio` gains `net`.
  - `crates/molde-cli/src/commands/sync.rs`: generic "unrecognized source" error (was "PostgreSQL only"); `script_file` now calls `engine.wrap_script`.
  - `README.md`: capability matrix gains a `sync` row (4 engines).

## 3. Bug Found & Fixed (during e2e)
- `sys.triggers` has **`parent_id`**, not `parent_object_id`. The read_triggers join failed against the live DB; fixed to `JOIN sys.tables t ON t.object_id = tr.parent_id`. (Caught only by e2e — unit tests don't hit the catalog.)

## 4. Validation Performed
- `cargo test -p molde-sync` → **15 passed** (5 new SQL Server unit tests). Workspace test suite green (no regressions): core 11, design 4, lang 6+5, migrate 1, providers 19, scaffold 3 (+4 ignored), lsp 3.
- `cargo clippy --workspace --all-targets -- -D warnings` → clean. `cargo fmt --all --check` → clean.
- **e2e against `mcr.microsoft.com/mssql/server:2022-latest`** (two DBs `molde_src`/`molde_tgt`, CLI run inside `rust:1-bookworm` on a shared docker network, ADO string with `TrustServerCertificate=true`):
  1. Full sync src→empty target: 2 tables (identity PK), PK/UNIQUE/CHECK/FK(ON DELETE CASCADE), index, function, trigger, view, history row — applied atomically. All objects verified present in target.
  2. **Idempotent re-run = 0 changes, 0 conflicts** (the key SQL-Server-specific risk — no false-conflict deparse for CHECK/FK/computed). 
  3. Incremental: `ALTER ADD Phone` + new `Tag` table in source → synced (1 column, 1 table, 1 constraint); a target-only `LocalOnly` table was **preserved**.
  4. Conflict: target `Customer.Name` widened to `nvarchar(200)` → reported as `[column] Customer.Name`, **not applied**, target kept `nvarchar(200)`.
- e2e containers/network removed afterward (`molde-mssql`, `molde-net`).

## 5. How To Re-run e2e (if needed)
```
docker run -d --name molde-mssql -e ACCEPT_EULA=Y -e 'MSSQL_SA_PASSWORD=Molde_Test_2026!' mcr.microsoft.com/mssql/server:2022-latest
docker network create molde-net && docker network connect molde-net molde-mssql
# create molde_src/molde_tgt, then run CLI inside rust:1-bookworm on molde-net:
#   ADO = Server=molde-mssql,1433;Database=…;User Id=sa;Password=Molde_Test_2026!;TrustServerCertificate=true
#   ./target/debug/molde sync --source <ADO> --target <ADO> --no-input [--dry-run | --yes]
```
Build/test as before: `rust:1-bookworm`, volumes `molde-target` / `efrust-cargo-reg` / `efrust-rustup`.

## 6. Known Limits / Future Work (all LOW)
- SQL Server engine targets the **`dbo` schema only** (hardcoded `SCHEMA = "dbo"`), consistent with scaffold's default and typical EF usage. Multi-schema is not handled.
- Computed columns emit `[col] AS (expr)` but the **PERSISTED** flag is read yet not re-emitted (rare on new tables); identity on **new columns of existing tables** isn't a normal additive case.
- Index reader covers key columns (not `INCLUDE` columns / filtered-index predicates) — parity with the v1 scope of the other engines.
- Editor artifacts (LSP/vsix) still stale post-rename (carried over from prev handoff §6.3) — untouched this session; only relevant if editor use resumes.
- TLS: SQL Server uses tiberius+rustls with `TrustServerCertificate=true`; unrelated to the Postgres X.509-v1/native-tls caveat.

## 7. Entry Points (for reference)
- `crates/molde-sync/src/sqlserver.rs` — the engine (reader + writer + `dynamic_literal` chunker + `redact_ado`).
- `crates/molde-sync/src/engine.rs:engine_for` — ADO detection; `SyncEngine::wrap_script` default.
- `crates/molde-sync/src/schema.rs` — shared `DbSchema`/`diff` (unchanged; engine-agnostic).
- `crates/molde-cli/src/commands/sync.rs` — CLI flow (same-engine check, `wrap_script` usage).

## 8. Context That Should NOT Be Lost
- `__EFMigrationsHistory` is preserved verbatim (DB interop); never rename.
- `sync` is a faithful catalog text port, independent of molde's IR — keep it that way.
- Host has no cargo/node (glibc 2.31); all build/test/e2e run via Docker.
- User preference: code/CLI in English (open-source); chat in Spanish.
