# Handoff — sqlserver-scaffold-close
Date (UTC): 20260607T153712Z | Status: DONE
Prev handoff: none (first handoff; prior history in Obsidian vault Sessions/2026-06-07-EF_RUST-*)
Base commit: NONE — repo has ZERO git commits; entire tree is untracked  | Branch: master

## 1. Session Objective
- Close the last capability gap: **scaffold (DB→C#) for SQL Server** via `tiberius` (Fase 8).
- Result: all 4 engines (Postgres, MySQL, SQLite, SQL Server) now support apply + scaffold + migrations. Project functionally complete per agreed scope.
- Status: DONE, verified e2e against real SQL Server 2022.

## 2. Current Work Summary
- Completed: `read_sqlserver` (tiberius) reader; dispatch routes SqlServer before sqlx pool; removed now-obsolete `Provider::supports_scaffold` + its CLI guard; README capability matrix + roadmap updated.
- Decisions treated as true:
  - SQL Server uses **tiberius (TDS)**; all other engines use **sqlx `Any`**. This split is permanent (sqlx 0.8 dropped MSSQL).
  - SQL Server connection = **ADO string** (`Server=h,1433;Database=..;User Id=..;Password=..;TrustServerCertificate=true;Encrypt=true`) passed via `--provider sqlserver --connection`. NOT a URL (so `Provider::from_url` can't infer it → `--provider sqlserver` is required).
  - Compatibility goal = *equivalent results*, not byte-identical to `dotnet ef`. Parity proven by comparing resulting DB schemas, not SQL text.
- Pending = only optional enhancements (see §6/§7). Nothing blocking.

## 3. Exact Files Modified (this session — Phase 8)
> NOTE: repo has no commits; `git diff` is empty. Claims below verified against current working-tree contents (symbols/line numbers grepped this session).
- `crates/efrust-scaffold/src/reader.rs` — modified. Added `read_sqlserver` (`reader.rs:694`), `ss_query`, `ss_store_type`, `ss_clr` (`reader.rs:917`). `read_model` now early-returns to tiberius for SqlServer (`reader.rs:37`). `ReadError` gained `Mssql`/`Io`, dropped `Unsupported` (`reader.rs:22-24`). Type alias `SsClient = tiberius::Client<Compat<TcpStream>>`. Quirk: SQL Server FK `delete_referential_action_desc` uses underscores (`NO_ACTION`/`SET_NULL`) → replaced `_`→` ` before `parse_action`. Quirk: numeric facets `CAST(... AS int)` for uniform tiberius reads; user-table filter `sys.tables.is_ms_shipped=0` excludes `spt_*`/`__EFMigrationsHistory`.
- `crates/efrust-scaffold/Cargo.toml` — modified. Added `tiberius 0.12` (default-features=false, features `tds73`,`rustls`), `tokio` (+`net`,`sync`), `tokio-util` (`compat`).
- `crates/efrust-providers/src/lib.rs` — modified. Removed `Provider::supports_scaffold`; updated `SqlServer` enum doc.
- `crates/efrust-cli/src/commands/scaffold.rs` — modified. Removed the SqlServer scaffold guard (now supported).
- `README.md` — modified. New capability matrix (4 engines full), architecture lists all 6 crates, roadmap rows 7–8, removed stale "pending SQL Server" notes.
- `handoffs/*` — created (this file, INDEX.md, LATEST).

## 4. Architectural Context
- Crates: `efrust-core` (Model IR + diff + snapshot + migration format), `efrust-providers` (`SqlGenerator` trait per engine), `efrust-migrate` (apply: `Backend` enum), `efrust-scaffold` (DB→IR reader + C# codegen), `efrust-design` (model-first orchestration: invokes sidecar + authors migrations), `efrust-cli` (clap), `sidecar/EfRust.Sidecar` (.NET, only non-Rust piece).
- **Execution abstraction** in `efrust-migrate`: `enum Backend { Sqlx(AnyPool), Mssql(Mutex<MssqlClient>) }` (`crates/efrust-migrate/src/lib.rs:38`). `Migrator::connect` dispatches by provider (`:136`). Same dual-driver pattern repeated in `efrust-scaffold/reader.rs` (`read_model` `:37`). MUST keep this split for any new engine work.
- Patterns to preserve: provider-agnostic `Operation` IR → each provider renders SQL; history table identifiers via `generator.quote_ident()`; history INSERT/DELETE inline escaped literals (no placeholders — avoids `Any` placeholder-dialect issues).
- `efrust-providers` does NOT have a runtime/DB dep — keep it pure (SQL string generation only). DB access lives in migrate/scaffold.

## 5. Important Runtime / Business Rules
- [verified] Migration file = JSON of `Operation` IR (up/down); SQL is rendered at apply time per `--provider`. Filename `<id>.json` where id `<utc yyyyMMddHHmmss>_<Name>`. `migration::load_dir` only loads files whose name starts with a digit → `model-snapshot.json` (same dir) is intentionally skipped.
- [verified] `__EFMigrationsHistory` schema matches EF (`MigrationId varchar(150)` PK, `ProductVersion varchar(32)`). DDL is provider-specific via `SqlGenerator::create_history_table_sql()` (default `CREATE TABLE IF NOT EXISTS`; SQL Server overrides with `IF OBJECT_ID(...) IS NULL` at `sqlserver.rs:121`).
- [verified] SQLite: `ALTER ADD FK` unsupported → FKs declared **inline in CREATE TABLE** (sqlite.rs create_table); the separate `AddForeignKey` op is a warn-skip on SQLite (avoids double FK). Adding FK to an *existing* SQLite table still unsupported (needs table-rebuild).
- [verified] MySQL `information_schema` string columns arrive as `BLOB` via sqlx `Any` (TEXT/BLOB share type code) → read via `my_str`/`my_opt_str` byte-fallback helpers (`reader.rs:465`); numerics `CAST AS SIGNED`.
- [verified] Postgres `information_schema` columns are type `name`/domains → all selected cols `::text`/`::int` cast in queries.
- [verified] Sidecar must use a **100% managed** EF provider for model extraction (Npgsql) — SQLite provider fails loading native `e_sqlite3` under dynamic assembly load. Sidecar uses `IDesignTimeModel` model, NOT `context.Model` (read-optimized lacks comments → throws).
- [verified] Scaffold name normalization: snake_case→PascalCase; emits `HasColumnName`/`ToTable` only when C# name differs from DB name.

## 6. Known Problems / Pending Risks
- **HIGH — Repo has zero git commits.** Impact: entire ~5400-line project is uncommitted/untracked; one `rm -rf` loses everything. Next action: `git add -A && git commit` (no commit made this session; user never requested it).
- LOW — SQLite table-rebuild not implemented. Impact: `AddForeignKey`/`AlterColumn` on existing SQLite tables are warn-skipped/Unsupported. Next: implement EF-style rebuild in `sqlite.rs` if needed.
- LOW — Scaffold does not singularize table names (`Customers` table → `Customers` class, not `Customer`). Only pascalizes. Next: add singularization in `efrust-scaffold/src/csharp.rs`.
- LOW — Parity (`scripts/parity-postgres.sh`) only covers Postgres + a simple model (Customer/Order). Advanced EF cases (inheritance, owned types, value converters, `HasData`) untested/unimplemented.
- LOW — Host has no `cargo`/`dotnet`; ALL builds/tests run via Docker. SQL Server/parity tests need heavy containers (mssql ~2GB).

## 7. Next Recommended Actions
1. **Commit the work**: `cd /home/mauricio/projects/LAB/EF_RUST && git add -A && git commit -m "efrust: EF Core-equivalent CLI in Rust (4 engines)"`. (Branch is `master`; consider `main` per repo convention.)
2. (Optional) SQLite table-rebuild for existing-table FK/alter: edit `crates/efrust-providers/src/sqlite.rs` (`SqliteGenerator::emit` AlterColumn/AddForeignKey arms). Verify: `docker run --rm -v "$PWD":/app -w /app rust:1-bookworm cargo test -p efrust-providers`.
3. (Optional) Table-name singularization: add `singularize()` in `crates/efrust-scaffold/src/csharp.rs`, use in `codegen.rs::class_name`. Verify: `docker run ... cargo test -p efrust-scaffold`.
4. (Optional) Extend parity to MySQL/SQL Server: generalize `scripts/parity-postgres.sh` + `scripts/schema-dump.sql`.
5. (Optional) Package sidecar as `dotnet tool`.

## 8. Validation Performed (this session)
- `docker run --rm -v "$PWD":/app -w /app rust:1-bookworm cargo test --workspace` → **PASS, 24/24** (diff 5, sidecar_contract 1, author 4, migrate 1, providers 8, scaffold 5).
- SQL Server 2022 e2e (Docker `mcr.microsoft.com/mssql/server:2022-latest` + `rust:1-bookworm` on shared `efrust-net`): `efrust database update --provider sqlserver` applied a clr migration (Customer/Order + FK + index); `efrust scaffold --provider sqlserver` regenerated C# — `Order.cs` had `public virtual Customer Customer`, `Customer.cs` had `ICollection<Order> Orders`, context had `HasMaxLength(200/320)`, `HasIndex`, `HasOne/WithMany`. PASS.
- NOT tested this session: `cargo clippy`/`fmt`; parity script re-run; Postgres/MySQL e2e (verified in prior sessions, see vault); advanced EF model cases. All Docker containers/networks removed after; `target/` + `Cargo.lock` deleted (host clean).

## 9. Recommended Entry Points for Next Session
- `crates/efrust-migrate/src/lib.rs:38` → `enum Backend` — the apply dual-driver split (sqlx + tiberius).
- `crates/efrust-scaffold/src/reader.rs:37` → `read_model` dispatch; `:694` `read_sqlserver` — schema-read per engine.
- `crates/efrust-providers/src/lib.rs` → `Provider` enum (parse/from_url/generator) — add engines here.
- `crates/efrust-core/src/diff.rs` → `diff()` + `apply_operation()` — IR diff engine + snapshot replay.
- `sidecar/EfRust.Sidecar/Program.cs` → `ModelMapper.Map` + `ContextActivator.Create` — EF model extraction.
- `README.md` (top) → capability matrix + roadmap (current truth).

## 10. Context That Should NOT Be Lost
- **Why tiberius, not sqlx, for SQL Server**: sqlx 0.8 removed the MSSQL driver. Verified dead-end — don't retry sqlx for SQL Server.
- **Sidecar provider choice (Npgsql)**: investigated SQLite for the sample → fails on native `e_sqlite3.so` under dynamic assembly load. InMemory provider is non-relational (no tables). Npgsql is fully managed and works offline (model build opens no connection). Don't switch the sample back to SQLite.
- **MySQL BLOB columns + Postgres `name`/MySQL CAST quirks**: already solved (byte-fallback helpers / `::text` / `CONVERT USING utf8mb4` / `CAST AS SIGNED`). Don't re-investigate "why does Any fail decoding information_schema".
- **Parity works because efrust's model comes from EF's own `IModel`** via the sidecar → store_types, constraint names, identity all match by construction. This is the core insight; keep the sidecar as source of truth for model-first.
- **EF migration files**: `scripts/parity-postgres.sh` runs `dotnet ef migrations add` which writes C# into `examples/SampleModel/Migrations/` — the script deletes them after; if it crashes mid-run, manually `rm -rf examples/SampleModel/Migrations`.
- **No host toolchains**: never assume `cargo`/`dotnet` exist locally — wrap in `docker run ... rust:1-bookworm` / `mcr.microsoft.com/dotnet/sdk:9.0`. Docker created files as root → clean with a `docker run rust rm -rf` afterward.
- Full phase-by-phase history (0→8) is in Obsidian `Sessions/2026-06-07-EF_RUST-*` if deeper rationale is needed.
