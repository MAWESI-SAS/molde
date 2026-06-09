# Authoring models — a practical guide

This guide teaches you how to write `.model` files by example: how to define
tables, every feature you can use, and how to take a model all the way to a real
database. It's the hands-on companion to the precise
[language specification](molde-language-spec.md) and the
[CLI reference](cli.md).

> A `.model` file describes the **schema** of one table. molde turns these files
> into migrations and applies them to PostgreSQL, MySQL, SQLite, or SQL Server.

## Contents

1. [The big picture](#1-the-big-picture)
2. [Your first model](#2-your-first-model)
3. [Fields and types](#3-fields-and-types)
4. [Nullability](#4-nullability)
5. [Primary keys](#5-primary-keys)
6. [Identity / auto-increment](#6-identity--auto-increment)
7. [Column facets (defaults, lengths, precision, comments…)](#7-column-facets)
8. [Computed columns](#8-computed-columns)
9. [Unique constraints](#9-unique-constraints)
10. [Relationships (foreign keys)](#10-relationships-foreign-keys)
11. [Indexes](#11-indexes)
12. [Seed data](#12-seed-data)
13. [Triggers](#13-triggers)
14. [The global `database.model` file](#14-the-global-databasemodel-file)
15. [Authoring shortcuts: owned types, enums, inheritance](#15-authoring-shortcuts)
16. [Engine-specific types and escape hatches](#16-engine-specific-types-and-escape-hatches)
17. [The everyday workflow](#17-the-everyday-workflow)
18. [A complete worked example](#18-a-complete-worked-example)

---

## 1. The big picture

- **One entity per file.** `Customer.model` describes the `Customer` table,
  `Order.model` the `Order` table, and so on. They all live in a `models/`
  directory.
- **One global file.** An optional `database.model` holds settings shared by the
  whole database (default schema, extensions, functions, raw DDL).
- **Indentation matters.** Structure is defined by **2-space** indentation — no
  tabs, no braces for blocks.
- **Two ways to start:**
  - *Model-first* — write the `.model` files by hand (this guide).
  - *Database-first* — run `molde pull` against an existing database and molde
    writes the `.model` files for you. Great for adopting molde on a project that
    already has a schema.

A `models/` directory typically looks like:

```
models/
  database.model     # global settings (optional)
  Customer.model
  Order.model
```

---

## 2. Your first model

Create `models/Customer.model`:

```
Customer
  fields:
    Id:    int pk
    Name:  string
    Email: string
```

That's a valid model: a table `Customer` with three columns and `Id` as the
primary key. Take it to a database (SQLite needs no server):

```bash
molde migrate InitialCreate                         # create the migration
molde db create --connection "sqlite://app.db"      # create the database
molde apply     --connection "sqlite://app.db"      # apply it
```

The first line on every entity file is the **header** — the entity name. If the
physical table name differs from the entity name, write it after a colon:

```
Customer: tbl_customers
  fields:
    ...
```

---

## 3. Fields and types

Columns go under `fields:`, one per line, as `Name: type facets…`.

```
fields:
  Id:        int
  Name:      string
  Price:     decimal
  CreatedAt: datetimeoffset
  IsActive:  bool
```

molde uses **logical types** that each engine maps to its own native type. The
common ones:

| Logical type | Typical SQL (PostgreSQL) |
|---|---|
| `int`, `long`, `short`, `byte` | integer, bigint, smallint |
| `bool` | boolean |
| `string` | text, or `varchar(n)` with `maxlen=` |
| `decimal` | numeric, or `numeric(p,s)` with `precision=` |
| `double`, `float` | double precision, real |
| `datetime`, `datetimeoffset` | timestamp, timestamptz |
| `date`, `time` | date, time |
| `guid` | uuid |
| `bytes` | bytea |
| `json` | jsonb |

(See the [spec](molde-language-spec.md#5-logical-types) for the full table.)

**Need an exact native type?** Write it directly — anything molde doesn't
recognize as a logical type is treated as the literal database type:

```
fields:
  Embedding: vector(1536)    # stored as exactly vector(1536)
  Search:    tsvector
```

Or keep a logical type but override how it's stored with `dbtype=`:

```
fields:
  Payload: json dbtype=jsonb
```

---

## 4. Nullability

Columns are **`NOT NULL` by default**. Add a `?` right after the type to make a
column nullable:

```
fields:
  Name:       string       # required
  MiddleName: string?      # optional (nullable)
```

There is no `required` keyword — the absence of `?` already means required.

---

## 5. Primary keys

**Single-column** — add the `pk` facet:

```
fields:
  Id: int pk
```

The constraint is named `pk_<table>` by convention. Want a custom name?

```
fields:
  Id: int pk=customers_pk
```

**Composite** (more than one column) — use the `key:` block instead:

```
TenantItem
  fields:
    TenantId: int
    Id:       int
  key: [TenantId, Id]
```

With a custom name:

```
  key: [TenantId, Id] name=tenant_items_pk
```

---

## 6. Identity / auto-increment

Add `identity` so the database generates the value (serial / auto-increment /
IDENTITY, depending on the engine):

```
fields:
  Id: int pk identity
```

---

## 7. Column facets

Facets are space-separated modifiers after the type. Boolean facets are just a
keyword; the rest take a `=value`.

| Facet | What it does | Example |
|---|---|---|
| `pk` / `pk=<name>` | primary key (optionally named) | `Id: int pk` |
| `identity` | database-generated value | `Id: int pk identity` |
| `unique` | single-column unique index | `Email: string unique` |
| `maxlen=<n>` | max length (→ `varchar(n)`) | `Name: string maxlen=200` |
| `precision=<p>[,<s>]` | numeric precision/scale | `Total: decimal precision=18,2` |
| `default=<sql>` | default value (**raw SQL**) | `IsActive: bool default=true` |
| `computed=<sql>` / `stored` | computed column (see §8) | `Slug: string computed="lower(name)" stored` |
| `collation=<name>` | column collation | `Code: string collation=C` |
| `comment="<text>"` | column comment | `Body: json comment="raw payload"` |
| `dbtype=<type>` | exact native store type | `Payload: json dbtype=jsonb` |
| `clr=<type>` | override the mapped .NET/CLR type (advanced; rarely needed) | `Id: int clr=System.Int32` |

A few notes by example:

```
fields:
  Name:     string maxlen=200
  Total:    decimal precision=18,2
  IsActive: bool default=true
  Status:   string maxlen=20 default="'active'"   # SQL string literal → quote it
  CreatedAt: datetimeoffset default="now()"
  Notes:    string? comment="free-form notes"
```

`default=` is **raw SQL**, so a textual default needs SQL quotes inside
(`default="'active'"`), and you can call functions (`default="now()"`). Quote the
whole value whenever it contains spaces.

---

## 8. Computed columns

A computed (generated) column derives its value from an expression. Use
`computed=<sql>`, and add `stored` if you want it materialized on disk (otherwise
it's virtual where the engine supports that):

```
fields:
  FirstName: string maxlen=100
  LastName:  string maxlen=100
  FullName:  string computed="FirstName || ' ' || LastName" stored
```

---

## 9. Unique constraints

For a **single column**, the `unique` facet is the shortcut — it creates a
single-column unique index named `ix_<table>_<col>`:

```
fields:
  Email: string? unique maxlen=320
```

For a **multi-column** unique constraint, declare it in the `indexes:` block with
`unique: true` (see §11).

---

## 10. Relationships (foreign keys)

A foreign key lives on the **dependent** table (the one that holds the reference),
under `belongs-to:`. Each entry is `NavName: { … }`:

```
Order
  fields:
    Id:         int pk identity
    CustomerId: int
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
```

| Key | Meaning |
|---|---|
| label (`Customer`) | navigation name — documentary only |
| `fk` | the local column(s); a list for composite: `[A, B]` |
| `references` | target as `Table.Column` (or `schema.Table.[A, B]`) |
| `onDelete` | `no_action` \| `restrict` \| `cascade` \| `set_null` \| `set_default` |
| `name` | optional constraint name (defaults to `fk_<table>_<principal>`) |
| `index` | set `false` to skip the automatic backing index |

**Foreign keys are indexed automatically.** Every `belongs-to` gets a non-unique
index on its column(s) — you don't write it, and it stays hidden from the
canonical `.model`. molde skips it when those columns are already covered (e.g.
they lead the primary key). To opt out explicitly:

```
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, index: false}
```

**Composite foreign key:**

```
  belongs-to:
    Tenant: {fk: [TenantId, OrgId], references: Org.[TenantId, Id], onDelete: restrict}
```

Only the dependent side is modeled — there's no "has-many"/inverse side to
declare. The relationship is fully described by the FK.

---

## 11. Indexes

Three kinds of indexes, three ways to get them:

1. **Foreign-key indexes** — automatic (§10). You don't write them.
2. **Single-column unique** — the `unique` facet (§9).
3. **Everything else** — declared in the `indexes:` block: performance indexes,
   composite, multi-column unique, partial/filtered, special methods, and
   expression/full-text indexes.

Each entry is `- <name>: { … }`, where the label is the index name (lowercase by
convention) and `on:` is the ordered column list:

```
indexes:
  - ix_order_status_created: {on: [Status, CreatedAt]}            # composite
  - ix_order_number:         {on: [Number], unique: true}        # multi-col unique
  - ix_order_active:         {on: [Status], filter: "Deleted = false"}  # partial
  - ix_docs_fts:             {on: [], expression: "to_tsvector('english', Body)", method: gin}
  - ix_emb:                  {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
```

| Key | Meaning |
|---|---|
| label | index name |
| `on` | ordered list of columns |
| `unique` | `true` for a unique index |
| `filter` | predicate for a partial/filtered index |
| `method` | index method: `gin`, `gist`, `hnsw`, `ivfflat`, … |
| `operators` | operator class per column (e.g. `vector_cosine_ops`) |
| `expression` | functional / full-text expression (takes precedence over `on`) |

---

## 12. Seed data

Reference/lookup rows that should always exist go under `seed:`. Each row is an
inline object of `Column: value`; use `~` for null:

```
seed:
  - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
  - {Id: 2, Name: "Globex", Email: ~}
```

molde turns seed rows into `INSERT`/`UPDATE`/`DELETE` as the data changes between
migrations — an upsert keyed by the **primary key**: add a row → `INSERT`, change
a value on an existing key → `UPDATE`, remove a row → `DELETE`. So every seed row
must include its primary key. Seed rows describe **data**, so they're
intentionally ignored by the schema drift check (`molde verify`).

> **Seeds live with their table — there are no separate seed files.** Put the
> `seed:` block in the entity's own `.model` file. Each `.model` file is one
> table, so a second file repeating the same entity header just to hold seeds
> would define a *duplicate* table, not add data to the existing one.
> (Separate/per-environment seed files and seeding from CSV/JSON are on the
> roadmap, not yet supported.)

### Seeding when the database generates the key (`seed-key`)

By default seeds match on the **primary key**, so each row must include it. That's
a problem when the PK is generated by the database — e.g. a `guid` with a
`gen_random_uuid()` default — because you don't have the value to write.

Declare a **natural key** with `seed-key:` and molde matches rows by that instead,
letting you omit the generated PK entirely:

```
Tenant
  fields:
    Id:   guid pk default="gen_random_uuid()"
    Code: string maxlen=40 unique
    Name: string maxlen=100
  seed-key: [Code]            # match seed rows by Code, not by Id
  seed:
    - {Code: "ACME", Name: "ACME Inc"}   # no Id — the database generates it
    - {Code: "GLX",  Name: "Globex"}
```

Now the `INSERT`s omit `Id` (the DB fills it in), and across migrations molde
upserts by `Code`: add a row → `INSERT` only that one; change a `Name` for an
existing `Code` → `UPDATE`; remove a row → `DELETE`. The `seed-key` columns should
be **unique** (here `Code` has `unique`) so the match is unambiguous.

> When to use which: hardcode the `Id` (a stable, ideally deterministic
> [UUIDv5](https://datatracker.ietf.org/doc/html/rfc4122)) if you want the *same*
> key in every environment — the classic choice for reference data. Use
> `seed-key` when you're fine with each environment generating its own key and
> you'd rather match on a business value.

---

## 13. Triggers

Database triggers attached to a table go under `triggers:`. The `sql:` block is
the source of truth — molde stores it verbatim and recreates it:

```
triggers:
  - trg_normalize:
      timing: before
      events: [insert, update]
      function: normalize_body
      sql: |
        CREATE TRIGGER trg_normalize BEFORE INSERT OR UPDATE ON public.documents
        FOR EACH ROW EXECUTE FUNCTION normalize_body()
```

| Key | Meaning |
|---|---|
| label | trigger name |
| `timing` | `before` \| `after` \| `instead_of` |
| `events` | any of `insert`, `update`, `delete`, `truncate` |
| `function` | the function it calls (informational) |
| `sql` | the full `CREATE TRIGGER` DDL (verbatim) |

The function the trigger calls is usually defined in `database.model` (§14).

---

## 14. The global `database.model` file

Settings shared across the whole database go in `models/database.model`. Every
section is optional:

```
schema: public                     # default schema for all tables
product-version: 16.0              # informational
extensions: [pg_trgm, unaccent, vector]
functions:
  - normalize_body: |
      CREATE OR REPLACE FUNCTION public.normalize_body() RETURNS trigger
      LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
raw:
  - |
      CREATE FULLTEXT CATALOG ft AS DEFAULT
```

| Section | Meaning |
|---|---|
| `schema` | default schema for tables that don't set their own |
| `product-version` | informational version tag |
| `extensions` | database extensions to ensure exist (e.g. PostgreSQL `vector`) |
| `functions` | reusable functions (each a name + verbatim DDL) |
| `raw` | escape hatch: verbatim DDL molde applies as-is |

A single table can override the default schema with a `schema:` line of its own.

---

## 15. Authoring shortcuts

These are **sugar**: convenient to write, but they expand to plain columns when
parsed. Scaffolding (`molde pull`) always emits the expanded form — so don't
expect `owns`/`enum`/`subtypes` to come back out of an introspected database.

### Owned types

Group a few related columns under a prefix:

```
fields:
  Contact: owns ContactInfo {Phone: string?, City: string? maxlen=80}
```

Expands to flat columns `Contact_Phone` and `Contact_City`.

### Enums

A constrained set of values, stored as a string or int:

```
fields:
  Status: enum[Pending, Shipped, Delivered] as=string maxlen=20
```

`as=string` stores it as text, `as=int` as a number. The listed values are
documentation — molde does not add a `CHECK` constraint by default.

### Single-table inheritance (TPH)

One table for a base type plus its subtypes, distinguished by a discriminator
column. Subtype columns become nullable:

```
Payment
  discriminator: Discriminator
  fields:
    Id:     int pk identity
    Amount: decimal precision=18,2
  subtypes:
    CardPayment: {CardNumber: string maxlen=20}
    CashPayment: {Note: string?}
```

Expands to one `Payment` table with `Id`, `Amount`, `CardNumber?`, `Note?`, and a
`Discriminator` column.

---

## 16. Engine-specific types and escape hatches

molde is engine-agnostic, but you can drop down to a specific database when you
need to:

- **Exact native type** — write it as the type, e.g. `vector(1536)`, `tsvector`,
  `citext`, `jsonb`, `int[]`.
- **`dbtype=`** — keep a logical type but pin the store type: `json dbtype=jsonb`.
- **`raw:`** in `database.model` — verbatim DDL for anything the model can't
  express.

PostgreSQL vector-search example, combining several features:

`database.model`
```
schema: public
extensions: [vector]
```

`Document.model`
```
Document
  fields:
    Id:        long pk identity
    Body:      string
    Embedding: vector(1536)
  indexes:
    - ix_document_embedding: {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
```

---

## 17. The everyday workflow

Once your models are written:

```bash
molde fmt                                  # canonicalize formatting (optional)
molde migrate AddDocuments                 # diff models → a new migration
molde lint                                 # check the migration for risky changes
molde apply --connection "$DATABASE_URL"   # apply pending migrations
molde verify --connection "$DATABASE_URL"  # confirm the DB matches the models
```

Editing models later? Change the `.model` files, run `molde migrate
<Name>` again, and molde writes a migration with just the difference. To adopt
molde on an existing database instead, start with `molde pull`. See the
[CLI reference](cli.md) for every command and flag.

---

## 18. A complete worked example

A small store, spread across the conventional files.

`models/database.model`
```
schema: public
```

`models/Customer.model`
```
Customer
  fields:
    Id:    int pk identity
    Name:  string maxlen=200
    Email: string? unique maxlen=320
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

`models/Product.model`
```
Product
  fields:
    Id:       int pk identity
    Sku:      string maxlen=40 unique
    Name:     string maxlen=200
    Price:    decimal precision=18,2
    IsActive: bool default=true
```

`models/Order.model`
```
Order
  fields:
    Id:         int pk identity
    CustomerId: int
    Status:     enum[Pending, Paid, Shipped, Cancelled] as=string maxlen=20 default="'Pending'"
    Total:      decimal precision=18,2
    CreatedAt:  datetimeoffset default="now()"
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
  indexes:
    - ix_order_status_created: {on: [Status, CreatedAt]}
```

`models/OrderLine.model`
```
OrderLine
  fields:
    Id:        int pk identity
    OrderId:   int
    ProductId: int
    Quantity:  int default=1
    UnitPrice: decimal precision=18,2
  belongs-to:
    Order:   {fk: OrderId,   references: Order.Id,   onDelete: cascade}
    Product: {fk: ProductId, references: Product.Id, onDelete: restrict}
```

Bring it to life on SQLite:

```bash
molde migrate InitialCreate
molde db create --connection "sqlite://store.db"
molde apply     --connection "sqlite://store.db"
molde status
```

Point `--connection` at PostgreSQL, MySQL, or SQL Server to run the exact same
models against another engine.

---

**Next:** the [language specification](molde-language-spec.md) for the precise
grammar and IR mapping, and the [CLI reference](cli.md) for the full command set.
```
