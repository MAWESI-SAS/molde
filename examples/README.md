# molde example

A two-table schema (`Customer` and `Order`) you can take from `.model` files to a
real database in about a minute. It uses **SQLite**, so there's no server to set
up — the database is just a file.

## Prerequisites

Install the `molde` binary (see the [root README](../README.md#install)):

```bash
cargo install --path ../crates/molde-cli
```

## The model

[`models/`](models/) holds one entity per file, plus `database.model` for globals:

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

Note there is no index or constraint name written out: molde generates the
conventional `pk_`/`fk_`/`ix_` names, and the foreign key on `Order.CustomerId`
gets its index automatically.

## Walkthrough

Run these from this `examples/` directory.

### 1. Create a migration from the models

```bash
molde migrate InitialCreate
```

This diffs the models against the (empty) snapshot and writes
`migrations/<timestamp>_InitialCreate.json` plus `migrations/snapshot.json`.

```bash
molde status      # list migrations
```

### 2. Lint it before applying

```bash
molde lint
```

You'll see two **warnings** (not errors): adding a foreign key and a unique index
can fail on a table that already has bad data. On a fresh database that's fine, so
`lint` exits 0. It only fails the build on *destructive* changes (dropping a table
or column).

### 3. Create the database and apply

```bash
molde db create --connection "sqlite://molde-demo.db"
molde apply      --connection "sqlite://molde-demo.db"
```

That creates `molde-demo.db`, runs the migration, and inserts the seed rows.

> SQLite declares foreign keys inline in `CREATE TABLE` and can't `ALTER TABLE ADD
> FOREIGN KEY`, so molde prints a notice and skips that one statement — expected
> on SQLite, where the FK is already part of the table definition.

Want a clean slate? `molde db reset --connection "sqlite://molde-demo.db"` drops,
recreates, and re-applies everything in one step.

## Going further

- `molde pull --connection "sqlite://molde-demo.db" --out models_from_db` —
  introspect the database back into `.model` files (database-first).
- `molde verify --connection "sqlite://molde-demo.db"` — check the database
  against the model for drift.
- Point `--connection` at PostgreSQL, MySQL, or SQL Server to run the same flow
  against another engine. See the [root README](../README.md) for connection
  string formats.
