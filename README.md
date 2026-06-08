# molde

[![CI](https://github.com/MAWESI-SAS/molde/actions/workflows/ci.yml/badge.svg)](https://github.com/MAWESI-SAS/molde/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
![MSRV](https://img.shields.io/badge/rustc-1.75%2B-orange.svg)

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
    Status: string maxlen=20
    CreatedAt: datetimeoffset
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
  indexes:
    - ix_order_status_created: {on: [Status, CreatedAt]}   # a query index you declare
```

### Conventions (so you write less)

molde fills in the boilerplate, EF-style. Generated names are **all lowercase**:
`pk_<table>`, `fk_<table>_<principal>`, `ix_<table>_<cols>`. You only write a name
when you want a custom one (the conventional ones are hidden from the `.model`).

Indexes come from three places:

- **Foreign keys are indexed automatically.** Every `belongs-to` gets a
  non-unique `ix_<table>_<cols>` index — you don't declare it. Opt out with
  `index: false`; molde also skips it when the columns are already covered by the
  PK or another index.
- **Single-column unique** → the `unique` facet on the field.
- **Everything else** (performance, composite, partial, GIN/GiST/HNSW,
  expression) → you declare it in the `indexes:` block, e.g.
  `- ix_order_status_created: {on: [Status, CreatedAt]}` (the label is the index
  name; `on:` is the ordered column list).

> Relationships → molde indexes them for you. Query/performance indexes → you
> declare them.

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
| `db` (create / drop / reset the database) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `apply` (apply migrations) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `migrate` / `undo` / `status` | ✅ | ✅ | ✅ | ✅ |
| `pull` (DB → `.model`) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `sync` (additive live DB → DB) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `verify` (DB ⇄ model drift) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `up` / `fresh` (catch up / rebuild) | ✅ | ✅ | ✅ | ✅ |
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
# 0. Database lifecycle: create / drop / reset the database itself.
molde db create --connection "$DATABASE_URL"
molde db reset  --connection "$DATABASE_URL"   # drop + recreate + apply all migrations
molde db drop   --connection "$DATABASE_URL"

# 1. Database-first: introspect an existing DB into .model files.
molde pull --connection "$DATABASE_URL" --out models

# 2. Model-first: create a migration from the .model files (diff vs snapshot).
molde migrate InitialCreate                  # reads models/, writes migrations/
molde status                                 # list migrations
molde undo                                   # remove the latest migration
molde lint                                   # CI: flag destructive/risky migrations (no DB)

# 3. Apply / roll back migrations against the DB.
molde apply --connection "$DATABASE_URL"
molde apply --connection "$DATABASE_URL" --to 0               # roll back everything
molde apply --connection "$DATABASE_URL" --to InitialCreate

# 4. Snapshot, drift check, sync, daily catch-up, rebuild.
molde snapshot                               # regenerate snapshot.json from models/
molde snapshot --check                       # CI: fail if the snapshot is stale
molde verify --connection "$DATABASE_URL" --check        # fail if the DB drifts from the model
molde sync --source "$TRUNK_DB" --target "$DATABASE_URL"  # additive live DB → DB
molde up --connection "$DATABASE_URL"        # apply pending migrations + drift report
molde fresh --connection "$DATABASE_URL"     # roll back all + re-apply (rebuild)
```

For a large team where everyone runs their own local database, see
[`docs/team-database-workflow.md`](docs/team-database-workflow.md): the `.model`
files in git are the source of truth, `molde init-team` installs a snapshot merge
driver so concurrent migrations don't conflict, and `molde sync`/`up` keep each
local DB current.

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

## Install

`molde` is a single binary. With [rustup](https://rustup.rs) installed:

```bash
cargo install --path crates/molde-cli      # → molde on your PATH
```

The only native build dependency is a C compiler (for the bundled SQLite); TLS is
rustls (no OpenSSL). Per-OS steps (Linux/macOS/Windows), prebuilt binaries, and
static musl builds are in [docs/install.md](docs/install.md).

## Development

Standard Rust workspace (`cargo build` / `cargo test`). The
[`.devcontainer/`](.devcontainer/) ships Rust + a local PostgreSQL; open the repo
in VS Code and choose *"Reopen in Container"*.

```bash
cargo build && cargo test
cargo clippy --workspace --all-targets
```

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for how
to set up a dev environment and the checks CI runs, and note our
[Code of Conduct](CODE_OF_CONDUCT.md). Security issues: see [SECURITY.md](SECURITY.md).

## License

Licensed under either of

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
