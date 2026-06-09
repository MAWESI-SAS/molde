# How migrations are tracked

This explains how molde knows which migrations have already run, how it applies
and rolls them back, and how that interoperates with Entity Framework. For the
command flags themselves, see the [CLI reference](cli.md).

## The history table: `__EFMigrationsHistory`

molde records applied migrations in a table **inside the database it manages**,
named `__EFMigrationsHistory` — the same name Entity Framework Core uses, on
purpose (see [EF interoperability](#ef-interoperability)).

| Column | Type | Purpose |
|---|---|---|
| `MigrationId` | `varchar(150)`, **primary key** | the migration id, e.g. `20260608120000_InitialCreate` |
| `ProductVersion` | `varchar(32)` | the molde version that applied it |

It's created automatically the first time you apply anything, with
`CREATE TABLE IF NOT EXISTS` (SQL Server uses an equivalent `IF OBJECT_ID … IS
NULL` guard). You never create or edit it by hand.

## How "pending" is computed

When you run `molde apply` (or `up` / `fresh` / `db reset`), molde:

1. Ensures the history table exists.
2. Reads the set of applied ids: `SELECT MigrationId FROM __EFMigrationsHistory
   ORDER BY MigrationId`.
3. Lists the migration files in `migrations/`, sorted by id.
4. **Pending = files whose id is not in the history table.** Those get applied,
   in id order.

Because migration ids sort lexicographically *and* molde guarantees each new id
sorts strictly after the previous one, id order is also chronological order — so
"what's applied vs pending" is always computed against a stable, correct
ordering.

## Apply and rollback are atomic with the record

Each migration's schema change and its history row live and die together:

- **Apply** runs the migration's `up` DDL **and** an
  `INSERT INTO __EFMigrationsHistory (MigrationId, ProductVersion) VALUES (…)`
  in a **single transaction**.
- **Rollback** (`apply --to <id>`, or `apply --to 0` to revert everything) runs
  the migration's `down` DDL **and** a
  `DELETE FROM __EFMigrationsHistory WHERE MigrationId = …`, also in one
  transaction.

If a migration fails partway through, the transaction rolls back both the schema
change and the history row — so you never get a half-applied schema or a "ghost"
history entry. molde applies migrations one at a time, each in its own
transaction.

### Per-engine caveat on atomicity

PostgreSQL and SQLite support **transactional DDL**, so the all-or-nothing
guarantee above holds fully. **MySQL auto-commits each DDL statement** (implicit
commit on `CREATE`/`ALTER`/etc.), so a migration that contains several DDL
statements is not fully atomic on MySQL — a failure mid-migration can leave some
statements committed. This is a MySQL limitation, not specific to molde; keep
migrations small and prefer running them where you can recover (e.g. a backup or
`db reset` in development).

## EF interoperability

Because the table name and shape match EF Core's `__EFMigrationsHistory`, molde
can take over a database that EF previously managed, and the migration history is
read the same way. Note that the **id format** differs in spirit: molde ids are
`<timestamp>_<Name>` just like EF's, so existing EF history rows are recognized;
new migrations you author with molde are recorded with molde's `ProductVersion`.

## Inspecting it yourself

The history is just a table — you can query it directly:

```sql
SELECT "MigrationId", "ProductVersion"
FROM "__EFMigrationsHistory"
ORDER BY "MigrationId";
```

(Quote the identifiers the way your engine expects.) From the molde side,
`molde status` lists the migrations known on disk, and `molde verify` checks the
live schema against your models.
