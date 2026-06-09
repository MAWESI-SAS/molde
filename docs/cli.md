# molde CLI reference

Complete reference for every `molde` command and option. Run `molde <command>
--help` for the same information from the binary itself.

```
molde [OPTIONS] <COMMAND>
```

## Contents

- [Global options & conventions](#global-options--conventions)
- Database-first: [`pull`](#molde-pull)
- Migrations: [`migrate`](#molde-migrate) · [`status`](#molde-status) ·
  [`undo`](#molde-undo) · [`snapshot`](#molde-snapshot) · [`lint`](#molde-lint)
- Apply: [`apply`](#molde-apply) · [`db`](#molde-db)
- Drift & sync: [`verify`](#molde-verify) · [`sync`](#molde-sync) ·
  [`up`](#molde-up) · [`fresh`](#molde-fresh)
- Authoring & teams: [`fmt`](#molde-fmt) · [`init-team`](#molde-init-team)

## Global options & conventions

These apply to every command:

| Option | Description |
|---|---|
| `-v, --verbose...` | Increase log verbosity. Repeat for more: `-v`, `-vv`. |
| `-h, --help` | Print help for the command. |
| `-V, --version` | Print the molde version (root command only). |

Conventions shared by the commands that touch a database:

- **`-c, --connection <CONNECTION>`** — connection string. Defaults to the
  `DATABASE_URL` environment variable; if missing, you are prompted (unless
  `--no-input`). The provider is inferred from the URL scheme (`postgres://`,
  `mysql://`, `sqlite://`). SQL Server uses an ADO string and usually needs
  `--provider sqlserver`.
- **`--provider <PROVIDER>`** — force the engine: `sqlite` | `postgres` |
  `mysql` | `sqlserver`. Only needed when it can't be inferred from the URL.
- **`--no-input`** — never prompt; fail instead if required data is missing.
  Use this in CI.
- **`-y, --yes`** — skip the confirmation prompt before a destructive or
  database-touching action.

---

## `molde pull`

Introspect an existing database into `.model` files (database-first).

```
molde pull [OPTIONS]
```

| Option | Description |
|---|---|
| `-c, --connection <CONNECTION>` | Source database. Defaults to `DATABASE_URL`; prompted if missing. |
| `--provider <PROVIDER>` | Engine; inferred from the URL if omitted. |
| `--schema <SCHEMA>` | Schema to read (PostgreSQL only). Defaults to `public`. |
| `-o, --out <OUT>` | Output directory for the `.model` files. Default: `models`. |
| `--force` | Overwrite existing files in the output directory. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

```bash
molde pull --connection "postgres://user:pass@localhost/app" --out models
```

---

## `molde migrate`

Create a migration from the diff between the models and the snapshot.

```
molde migrate [OPTIONS] [NAME]
```

| Argument | Description |
|---|---|
| `[NAME]` | Migration name (e.g. `AddInvoices`). If omitted, you are prompted. |

| Option | Description |
|---|---|
| `--from-models <FROM_MODELS>` | Directory with the `.model` source files. Default: `models`. |
| `--output-dir <OUTPUT_DIR>` | Directory where migrations are stored. Default: `migrations`. |
| `--snapshot <SNAPSHOT>` | Snapshot path. Defaults to `<output-dir>/snapshot.json`. |
| `--no-input` | Don't prompt (CI); fail if the name is missing. |

```bash
molde migrate InitialCreate
```

The migration id is `<UTC timestamp>_<Name>`; molde guarantees it sorts after
every existing migration, so two created in the same second never tie.

---

## `molde status`

List known migrations.

```
molde status [OPTIONS]
```

| Option | Description |
|---|---|
| `--output-dir <OUTPUT_DIR>` | Migrations directory. Default: `migrations`. |

---

## `molde undo`

Remove the latest migration and regenerate the snapshot from the rest.

```
molde undo [OPTIONS]
```

| Option | Description |
|---|---|
| `--output-dir <OUTPUT_DIR>` | Migrations directory. Default: `migrations`. |
| `--snapshot <SNAPSHOT>` | Snapshot path. Defaults to `<output-dir>/snapshot.json`. |

---

## `molde snapshot`

Regenerate (or, with `--check`, verify) the migration snapshot from the models.

```
molde snapshot [OPTIONS]
```

| Option | Description |
|---|---|
| `--from-models <FROM_MODELS>` | Directory with the `.model` source files. Default: `models`. |
| `-o, --output <OUTPUT>` | Where to write the snapshot. Defaults to `migrations/snapshot.json`. The git merge driver passes the conflicted file here (`%A`). |
| `--check` | Don't write; exit non-zero if the on-disk snapshot is stale. CI gate. |

```bash
molde snapshot --check    # fail the build if migrations/snapshot.json is out of date
```

---

## `molde lint`

Statically check migrations for risky or destructive changes — no database
access, meant for CI on a pull request. **Destructive** changes (dropping a
table/column) fail the command; data-dependent changes are **warnings**.

```
molde lint [OPTIONS] [FILE]...
```

| Argument | Description |
|---|---|
| `[FILE]...` | Specific migration file(s) to lint (e.g. just the ones your PR adds). When given, `--all`/`--since` and the directory scan are bypassed. |

| Option | Description |
|---|---|
| `--migrations-dir <MIGRATIONS_DIR>` | Directory of migrations. Default: `migrations`. |
| `--all` | Lint every migration, not just the most recent one. |
| `--since <ID>` | Lint only migrations newer than this id (exclusive) — e.g. the base your PR branched from. Takes precedence over `--all`. |
| `--strict` | Fail on warnings too (data-dependent / locking), not only destructive. |

Selection precedence: `FILE` args → `--since` → `--all` → latest migration only.

```bash
molde lint                       # the latest migration
molde lint --since 20260101000000_Base --strict
molde lint migrations/20260608_AddEmail.json
```

---

## `molde apply`

Apply pending migrations to the database (or roll back with `--to`). Renders the
SQL for the target engine and records each migration in the history table.

```
molde apply [OPTIONS]
```

| Option | Description |
|---|---|
| `-c, --connection <CONNECTION>` | Defaults to `DATABASE_URL`; prompted if missing. |
| `--provider <PROVIDER>` | Engine; inferred from the URL if omitted. |
| `--migrations-dir <MIGRATIONS_DIR>` | Directory of migrations to apply. Default: `migrations`. |
| `--to <TO>` | Bring the database up to this migration (id or name). `0` rolls back all. By default, applies every pending migration. |
| `-y, --yes` | Don't ask for confirmation before touching the database. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

```bash
molde apply --connection "$DATABASE_URL"
molde apply --connection "$DATABASE_URL" --to 0              # roll back everything
molde apply --connection "$DATABASE_URL" --to InitialCreate # up/down to a point
```

molde records applied migrations in an `__EFMigrationsHistory` table and applies
each one (DDL + history row) in a single transaction. See
[migrations.md](migrations.md) for the details.

---

## `molde db`

Database lifecycle: create / drop / reset the database itself (not its schema).

```
molde db <COMMAND> [OPTIONS]
```

| Subcommand | Description |
|---|---|
| `create` | Create the database if it doesn't exist. |
| `drop` | Drop the database (destructive). |
| `reset` | Drop, recreate, and apply all migrations from scratch. |

All three share these options:

| Option | Description |
|---|---|
| `-c, --connection <CONNECTION>` | Defaults to `DATABASE_URL`; prompted if missing. |
| `--provider <PROVIDER>` | Engine; inferred from the URL if omitted. |
| `-y, --yes` | Don't ask for confirmation before a destructive action. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

`db reset` additionally takes:

| Option | Description |
|---|---|
| `--migrations-dir <MIGRATIONS_DIR>` | Migrations to apply after recreating the database. Default: `migrations`. |

```bash
molde db create --connection "$DATABASE_URL"
molde db reset  --connection "$DATABASE_URL"   # drop + recreate + apply all
molde db drop   --connection "$DATABASE_URL" --yes
```

`drop`/`reset` are destructive: they refuse to run under `--no-input` unless
`--yes` is also passed.

---

## `molde verify`

Check whether a live database matches the model (drift check). This compares
**structure** only; seed/data rows are out of scope.

```
molde verify [OPTIONS]
```

| Option | Description |
|---|---|
| `-c, --connection <CONNECTION>` | Database to check. Defaults to `DATABASE_URL`; prompted if missing. |
| `--provider <PROVIDER>` | Engine; inferred from the URL if omitted. |
| `--schema <SCHEMA>` | Schema to read (PostgreSQL only). Defaults to `public`. |
| `--from-models <FROM_MODELS>` | Directory with the desired-state `.model` files. Default: `models`. |
| `--check` | Exit non-zero if the database drifts from the model. CI gate. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

```bash
molde verify --connection "$DATABASE_URL" --check
```

---

## `molde sync`

Additively sync a target database from a source (live DB → live DB). It computes
the additive changes, writes a `.sql`, and (unless `--dry-run`) applies them.

```
molde sync [OPTIONS]
```

| Option | Description |
|---|---|
| `--source <SOURCE>` | Database changes are brought FROM (e.g. the shared `test`). Falls back to `MOLDE_SYNC_SOURCE`; prompted if missing. |
| `--target <TARGET>` | Database that RECEIVES the changes (e.g. your local DB). Falls back to `MOLDE_SYNC_TARGET`; prompted if missing. |
| `-o, --out <OUT>` | Path for the generated `.sql`. Defaults to `./sync-<timestamp>.sql`. |
| `--dry-run` | Only generate the `.sql` and the report; don't apply anything. |
| `-y, --yes` | Apply without asking for confirmation. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

```bash
molde sync --source "$TRUNK_DB" --target "$DATABASE_URL"
molde sync --source "$TRUNK_DB" --target "$DATABASE_URL" --dry-run
```

---

## `molde up`

Catch the local database up — apply pending migrations, or fast-forward from a
trunk database — and print a drift report.

```
molde up [OPTIONS]
```

| Option | Description |
|---|---|
| `-c, --connection <CONNECTION>` | Local database. Defaults to `DATABASE_URL`; prompted if missing. |
| `--provider <PROVIDER>` | Engine; inferred from the URL if omitted. |
| `--from-trunk <FROM_TRUNK>` | Fast-forward from this trunk database (additive sync) instead of replaying migrations. Falls back to `MOLDE_SYNC_SOURCE`. |
| `--migrations-dir <MIGRATIONS_DIR>` | Migrations to apply in replay mode. Default: `migrations`. |
| `--from-models <FROM_MODELS>` | Models directory for the drift report. Default: `models`. |
| `--schema <SCHEMA>` | Schema to read (PostgreSQL only). Defaults to `public`. |
| `-y, --yes` | Apply/sync without asking for confirmation. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

```bash
molde up --connection "$DATABASE_URL"
```

---

## `molde fresh`

Rebuild the local database from migrations: roll back everything, then re-apply.

```
molde fresh [OPTIONS]
```

| Option | Description |
|---|---|
| `-c, --connection <CONNECTION>` | Local database. Defaults to `DATABASE_URL`; prompted if missing. |
| `--provider <PROVIDER>` | Engine; inferred from the URL if omitted. |
| `--migrations-dir <MIGRATIONS_DIR>` | Migrations to rebuild from. Default: `migrations`. |
| `-y, --yes` | Rebuild without asking for confirmation. |
| `--no-input` | Don't prompt (CI); fail if data is missing. |

```bash
molde fresh --connection "$DATABASE_URL"
```

---

## `molde fmt`

Format `.model` files to their canonical form (like `cargo fmt` for models).

```
molde fmt [OPTIONS] [PATHS]...
```

| Argument | Description |
|---|---|
| `[PATHS]...` | `.model` files or directories to format. Defaults to `models/`. |

| Option | Description |
|---|---|
| `--check` | Don't write; exit non-zero if any file is not formatted. |
| `--stdin` | Read from stdin and write the formatted result to stdout. |
| `--stdin-name <STDIN_NAME>` | File name used to infer the kind with `--stdin` (`database.model` = globals; anything else = entity). Default: `entity.model`. |

```bash
molde fmt                 # format models/
molde fmt --check         # CI gate: fail if anything is unformatted
```

---

## `molde init-team`

Set up the snapshot merge driver (and optionally a CI template) for team
workflows, so concurrent migrations don't conflict on `snapshot.json`. See
[team-database-workflow.md](team-database-workflow.md) for the full playbook.

```
molde init-team [OPTIONS]
```

| Option | Description |
|---|---|
| `--path <PATH>` | Repository root. Default: `.` (current directory). |
| `--ci <CI>` | Also write a CI template for this provider (currently: `github`). |
| `--force` | Overwrite an existing CI template if it differs. |

```bash
molde init-team --ci github
```
