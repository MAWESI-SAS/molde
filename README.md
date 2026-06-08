# molde

A **Rust database schema** tool built on top of **molde**, a custom, declarative,
human-readable model language (TOON/YAML style). molde does **scaffolding**
(DB → models), generates **migrations** (model → diff), and **applies** them
across 4 engines. No .NET, no C#: everything in Rust.

> molde manages the **schema** (models, migrations, application). Runtime data
> access (your application's queries) is out of scope.

## The molde language (`.model`)

One entity per file; all the table configuration lives together. Example:

```
Customer
  fields:
    Id: int pk
    Name: string maxlen=200
    Email: string? unique maxlen=320
  seed:
    - {Id: 1, Name: "ACME", Email: "acme@example.com"}

Order
  fields:
    Id: int pk identity
    CustomerId: int
    Total: decimal precision=18,2
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
```

Full specification: [`docs/molde-language-spec.md`](docs/molde-language-spec.md).

## Flow

```
DB ──pull──▶ models/*.model
models/*.model ──migrate──▶ migrations/*.json (diff against snapshot)
migrations/*.json ──apply──▶ DB
```

Everything pivots on a shared **IR** (`molde_core::DatabaseModel`): the language is
its textual form, the readers produce it from the DB, and the diff generates the SQL.

## Architecture

```
molde (CLI, Rust)
├── molde-core         model IR + snapshot + diff + migrations (engine-agnostic)
├── molde-lang         molde language: parser/emitter (.model ↔ IR)
├── molde-providers    SqlGenerator per engine (SQLite, Postgres, MySQL, SQL Server)
├── molde-migrate      migration apply (Backend: sqlx Any + tiberius/TDS)
├── molde-scaffold     schema reading (DB → IR) + .model emission
└── molde-design       migration authoring (diff against snapshot)
```

### Capability matrix per engine

| Capability | PostgreSQL | MySQL | SQLite | SQL Server |
|---|:--:|:--:|:--:|:--:|
| `apply` (apply migrations) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `migrate` / `undo` / `status` | ✅ | ✅ | ✅ | ✅ |
| `pull` (DB → `.model`) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `sync` (additive live DB → DB) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| Search / full-text (pull + round-trip) | ✅ pgvector+tsvector+triggers | ✅ FULLTEXT+generated | — | ✅ computed PERSISTED · FTS best-effort |

> SQLite: FKs are declared inline in `CREATE TABLE`; column type changes and
> adding/dropping FKs on existing tables are applied via table rebuild
> (create-new/copy/drop/rename), EF-style.
> SQL Server uses the TDS driver `tiberius`; everything else uses `sqlx` (`Any`).
> **PostgreSQL:** scaffolding preserves `vector(N)` (pgvector), `tsvector` (incl.
> generated `STORED` columns), indexes with method/operator class (GIN, GiST,
> HNSW, IVFFlat), and partial indexes. Functions, triggers, and expression indexes
> are preserved in `.model` (`triggers:`/`functions:`/`indexes:` blocks and `raw:`).
> Unconventional types (`jsonb`, arrays, `citext`, `vector`, `tsvector`…) are
> retained with `dbtype=`.

### TLS backend

Defaults to **rustls**. For servers with X.509 v1 (legacy) certificates that
rustls rejects, compile with **native-tls (OpenSSL)**:

```bash
cargo build -p molde-cli --no-default-features --features tls-native-tls
```

## Commands

```bash
# 1. Database-first: introspect an existing DB into .model files.
molde pull --connection "$DATABASE_URL" --out models

# 2. Model-first: create a migration from the .model files (diff vs snapshot).
molde migrate InitialCreate                  # reads models/, writes migrations/
molde status                                 # list migrations
molde undo                                   # remove the latest migration

# 3. Apply / roll back migrations against the DB.
molde apply --connection "$DATABASE_URL"
molde apply --connection "$DATABASE_URL" --to 0               # roll back everything
molde apply --connection "$DATABASE_URL" --to InitialCreate
```

Run a command with no arguments and it prompts for what's missing (migration name,
connection); `apply` confirms before touching the database. Use `--yes` to skip the
confirmation and `--no-input` for CI (no prompts). The provider is inferred from the
URL (`postgres://`, `mysql://`, `sqlite://`) or set with `--provider`. SQL Server uses
an ADO connection string:

```bash
molde apply --provider sqlserver \
  --connection "Server=host,1433;Database=db;User Id=sa;Password=***;TrustServerCertificate=true;Encrypt=true"
```

### Project layout

```
models/                 # source of truth (one entity per .model)
  database.model        # globals: schema, extensions, functions, raw
  Customer.model
  Order.model
migrations/             # versioned in git
  snapshot.json         # previous state (managed by molde)
  20260607_*.json       # migrations (IR operations; SQL is rendered on apply)
```

## Development

Standard Rust workspace (`cargo build` / `cargo test`). The
[`.devcontainer/`](.devcontainer/) ships Rust + a local PostgreSQL; open the repo
in VS Code and choose *"Reopen in Container"*.

```bash
cargo build && cargo test
cargo clippy --workspace --all-targets
```

## License

MIT
