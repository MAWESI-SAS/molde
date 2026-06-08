# molde — molde model language specification

> **Status:** draft for approval (Phase A, deliverable 1).
> **Provisional name:** molde (*molde model language*). File extension: `.model`.
> **Goal:** fully replace C# as the model definition format within molde. molde
> is a **schema layer only**: it manages and versions models, generates
> migrations, and does scaffolding. Runtime data access is out of scope. There is
> no C# generation or .NET sidecar.

---

## 1. Design principles

1. **Human-readable and token-cheap for agents.** Indented syntax,
   TOON/YAML style, no braces or superfluous punctuation.
2. **One entity per file.** `Customer.model`, `Order.model`, … plus a
   `database.model` for global concerns. Everything about a table lives together
   (columns, PK, FKs, indexes, triggers, seed) — there is no separate "DbContext".
3. **The language is a projection of the IR.** Each construct maps 1:1 to a field
   of `molde-core::model::DatabaseModel`. The IR remains the center; `.model` is
   its canonical textual form.
4. **Round-trip guaranteed at the IR level.** `parse(emit(ir)) == ir`. The
   high-level constructs (owned, inheritance, enums) are **authoring sugar** that
   expands to the flat relational form; scaffolding from the DB always emits the
   canonical (flat) form.
5. **Engine-agnostic, with escape hatches.** Logical types that each provider
   translates; `dbtype=` and raw `sql:` blocks for engine-specific concerns.

---

## 2. Project structure

```
models/
  database.model        # global metadata (schema, extensions, functions, raw)
  Customer.model        # one entity = one table
  Order.model
  Payment.model
migrations/             # sibling of models/, VISIBLE and versioned in git
  snapshot.json         # previous state (managed by molde, not hand-edited)
  20260607_*.json       # generated migrations
```

- A `<Entity>.model` file defines **exactly one table**.
- The file name is indicative; the real entity/table name is taken from the content.
- `database.model` is optional; if missing, defaults are assumed.

---

## 3. Lexical structure

| Element | Syntax | Notes |
|---|---|---|
| Indentation | 2 spaces | defines the structure; no tabs |
| Comment | `# …` to end of line | discarded; not the semantic `comment` |
| Identifier | `[A-Za-z_][A-Za-z0-9_]*` | entity/column/etc. names |
| Simple scalar | `int`, `42`, `cascade` | no quotes if no spaces/specials |
| String | `"text with spaces"` | double quotes; escapes `\"` `\\` |
| Null | `~` | represents `null` (e.g. in seed) |
| Boolean | `true` / `false` | |
| Inline list | `[a, b, c]` | |
| Inline object | `{k: v, k2: v2}` | |
| Block list | lines with `- …` | |
| Text block | `|` + indented lines | multiline SQL (triggers, functions, raw, computed) |

Text block (block scalar), identical in spirit to YAML `|`:

```
function: |
  CREATE OR REPLACE FUNCTION normalize_body() RETURNS trigger
  LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
```

---

## 4. Entity file

General form (all sections except the header and `fields:` are optional):

```
<Entity>[: <table>]            # header; ": <table>" only if it differs from Entity
  schema: <name>               # if it differs from the global default
  comment: "<text>"            # table COMMENT
  fields:
    <Col>: <type>[?] <facets…>
    …
  key: [<cols>] [name=<n>]     # composite PK or PK with explicit name
  belongs-to:                  # foreign keys (dependent side)
    <Nav>: {fk: …, references: …, onDelete: …, name: …}
  indexes:
    - <name>: {on: […], unique: …, method: …, operators: […], filter: …, expression: …}
  triggers:
    - <name>: {timing: …, events: […], function: …, sql: | …}
  seed:
    - {<Col>: <val>, …}
  # authoring sugar:
  owns <Type> { … }            # owned type → prefixed columns
  discriminator: <Col>         # TPH inheritance
  subtypes:
    <SubType>: { <extra fields> }
```

### 4.1 Header

- `Customer` → entity and table are both named `Customer`.
- `Customer: tbl_customer` → entity `Customer`, physical table `tbl_customer`.
- `schema:` overrides the global default `schema` for this table.

---

## 5. Logical types

The **logical type** replaces `clr_type`. The provider derives the default
`store_type` from the logical type + facets. Canonical mapping:

| Logical | clr_type (IR) | default store_type (PG example) |
|---|---|---|
| `int` | System.Int32 | integer |
| `long` | System.Int64 | bigint |
| `short` | System.Int16 | smallint |
| `byte` | System.Byte | smallint |
| `bool` | System.Boolean | boolean |
| `string` | System.String | text / varchar(n) if `maxlen=` |
| `decimal` | System.Decimal | numeric / numeric(p,s) if `precision=` |
| `double` | System.Double | double precision |
| `float` | System.Single | real |
| `datetime` | System.DateTime | timestamp |
| `datetimeoffset` | System.DateTimeOffset | timestamptz |
| `date` | System.DateOnly | date |
| `time` | System.TimeOnly | time |
| `guid` | System.Guid | uuid |
| `bytes` | System.Byte[] | bytea |
| `json` | System.String | jsonb (override with `dbtype=`) |

**Raw native type.** If the type is not a known logical type, it is interpreted
as a literal `store_type` and `clr_type` is left undetermined:

```
Embedding: vector(1536)     # store_type = "vector(1536)"
Search:    tsvector         # store_type = "tsvector"
```

Equivalent with an override on a logical type:

```
Metadata: json dbtype=jsonb # clr System.String, store_type jsonb
```

A `?` after the type marks the column as **nullable**. Its absence = `NOT NULL`.
(There is no `required` facet: it would be redundant with the absence of `?`.)

---

## 6. Column facets

After `<Col>: <type>[?]`, a sequence of space-separated facets.

**Boolean** (presence = enabled):

| Facet | IR field |
|---|---|
| `pk` | adds the column to `primary_key` (simple PK; name by convention `pk_<table>`) |
| `identity` | `is_identity = true` |
| `unique` | creates a single-column unique index (`ix_<table>_<col>`) |
| `stored` | `computed_stored = true` (use together with `computed=`) |

**With a value** (`key=value`):

| Facet | IR field |
|---|---|
| `maxlen=<n>` | `max_length` |
| `precision=<p>[,<s>]` | `precision`, `scale` |
| `default=<sql>` | `default_value_sql` (raw SQL; use quotes if it contains spaces) |
| `computed=<sql>` | `computed_sql` |
| `collation=<name>` | `collation` |
| `column=<db_name>` | physical name if it differs from the property name |
| `dbtype=<store_type>` | `store_type` (override of the exact native type) |
| `comment="<text>"` | column `comment` |
| `pk=<name>` | simple PK with an explicit name |

Examples:

```
Id:    int pk identity
Name:  string maxlen=200
Total: decimal precision=18,2
Flag:  bool default=false
Slug:  string computed="lower(name)" stored
Body:  json dbtype=jsonb comment="raw payload"
```

---

## 7. Primary key

- **Simple:** the `pk` facet on the column. Name by convention `pk_<table>`
  (all lowercase), or explicit with `pk=<name>`.
- **Composite / named at the table level:** the `key:` block.

```
key: [TenantId, Id]                 # name defaults to pk_<table>
key: [TenantId, Id] name=tenant_items_pk   # or an explicit custom name
```

Maps to `PrimaryKey { name, columns }`.

---

## 8. Relationships (foreign keys)

The FK lives in the **dependent** table, under `belongs-to:`:

```
belongs-to:
  Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
  # name defaults to fk_<table>_<principal>; add `name: <custom>` to override
```

| Key | IR field | Notes |
|---|---|---|
| (label) | — | navigation name (documentary) |
| `fk` | `columns` | local column(s); a list if composite: `[A, B]` |
| `references` | `principal_table` (+ `principal_schema`) and `principal_columns` | `Table.Col` or `schema.Table.[A,B]` |
| `onDelete` | `on_delete` | `no_action`\|`restrict`\|`cascade`\|`set_null`\|`set_default` |
| `name` | `name` | optional; convention `fk_<table>_<principal>` (all lowercase) |
| `index` | — | `false` opts out of the auto-generated backing index (default `true`) |

**Backing index (by convention).** Every `belongs-to` gets a non-unique index
on its FK column(s) automatically — named `ix_<table>_<cols>` (lowercase) — just
like EF. You don't declare it, and it is hidden from the canonical `.model`
(re-synthesized on parse). It is **skipped** when the FK columns are already the
leading columns of the primary key or of an existing index, or when you set
`index: false`. A foreign key with no covering index is written back as
`index: false`, so the state always round-trips.

**Inverse** navigations (principal side) are **not modeled**: the FK is already
described on the dependent side. Listing them would duplicate information without
generating DDL; if diagrams/documentation are needed they are derived from the
set of FKs.

---

## 9. Indexes

Three ways an index gets created:

1. **Automatic — foreign keys.** Every `belongs-to` gets a non-unique backing
   index by convention (§8), named `ix_<table>_<cols>`. You don't declare it and
   it isn't shown in the canonical `.model`. Opt out with `index: false`.
2. **Inline — single-column unique.** The `unique` facet on a column creates a
   single-column unique index (`ix_<table>_<col>`).
3. **The `indexes:` block — everything else** (you declare these): performance
   indexes on regular columns, composite indexes, partial/filtered, special
   methods (GIN/GiST/HNSW), expression indexes.

> Rule of thumb: relationships → molde indexes them for you; query/performance
> indexes → you declare them.

Each `indexes:` entry is `- <name>: { <options> }`:

```
indexes:
  - ix_order_status_created: {on: [Status, CreatedAt]}          # composite, non-unique
  - ix_customer_email:       {on: [Email], unique: true}
  - ix_docs_fts:             {on: [], expression: "to_tsvector('english', body)", method: gin}
  - ix_emb:                  {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
  - ix_active:               {on: [Status], filter: "Deleted = false"}
```

The label before the `:` is the index **name** (you choose it; lowercase by
convention). `on:` is the ordered list of columns. The rest are optional:

| Key | IR field |
|---|---|
| (label) | `name` |
| `on` | `columns` |
| `unique` | `is_unique` |
| `filter` | `filter` (partial index) |
| `method` | `method` (`gin`, `gist`, `hnsw`, `ivfflat`, …) |
| `operators` | `operators` (operator class per column) |
| `expression` | `expression` (functional / full-text index; takes precedence over `on`) |

---

## 10. Owned types (sugar)

```
fields:
  Contact: owns ContactInfo {Phone: string?, City: string? maxlen=80}
```

Expands to flat columns with the `<Prop>_<Field>` prefix:
`Contact_Phone`, `Contact_City`. In the IR **only the flat columns exist**.
Scaffolding from the DB emits the flat columns (it does not reconstruct `owns`).

---

## 11. TPH inheritance (sugar)

```
Payment: Payment
  discriminator: Discriminator
  fields:
    Id:     int pk identity
    Amount: decimal
  subtypes:
    CardPayment: {CardNumber: string}
    CashPayment: {Note: string?}
```

Expands to **a single table** with: all the base columns + those of each subtype
(forced to `nullable`) + a discriminator column (`Discriminator string`). In the
IR it is a flat table. Scaffolding from the DB does not reconstruct `subtypes`
(it emits the flat columns + the discriminator column as a normal column).

---

## 12. Value converters / enums (sugar)

```
Status: enum[Pending, Shipped] as=string maxlen=20
```

- `as=string` → `string` column (varchar store_type); the valid values are
  documentation (molde does not add a CHECK by default).
- `as=int` → `int` column.

In the IR it is a normal scalar column. Scaffolding from the DB sees the scalar
column (it does not reconstruct the enum).

---

## 13. Seed data

```
seed:
  - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
  - {Id: 2, Name: "Globex", Email: ~}
```

Maps to `Table.seed_data` (a list of column→JSON-value maps). `~` = null.
The diff materializes them as `INSERT`/`UPDATE`/`DELETE`.

---

## 14. Triggers (per entity)

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

| Key | IR field (`Trigger`) |
|---|---|
| (label) | `name` |
| (entity) | `table` (+ `schema`) |
| `timing` | `timing` (`before`\|`after`\|`instead_of`) |
| `events` | `events` (`insert`\|`update`\|`delete`\|`truncate`) |
| `function` | `function` (informational) |
| `sql` | `definition` (raw DDL; source of truth for recreating it) |

---

## 15. Global `database.model` file

```
schema: public                 # default_schema
product-version: 9.0.0         # informational (optional)
extensions: [pg_trgm, unaccent, vector]
functions:
  - normalize_body: |
      CREATE OR REPLACE FUNCTION public.normalize_body() RETURNS trigger
      LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
raw:
  - |
      CREATE FULLTEXT CATALOG ft AS DEFAULT
```

| Key | IR field (`DatabaseModel`) |
|---|---|
| `schema` | `default_schema` |
| `product-version` | `product_version` |
| `extensions` | `extensions` |
| `functions` | `functions` (list of `DbFunction { name[, schema], definition }`) |
| `raw` | `raw_objects` (verbatim DDL) |

`format_version` is managed by molde internally (not edited).

---

## 16. Formal DSL ↔ IR mapping (full coverage)

| IR | molde |
|---|---|
| `DatabaseModel.default_schema` | `database.model: schema:` |
| `DatabaseModel.product_version` | `database.model: product-version:` |
| `DatabaseModel.extensions` | `database.model: extensions:` |
| `DatabaseModel.functions` | `database.model: functions:` |
| `DatabaseModel.raw_objects` | `database.model: raw:` |
| `Table.name` / `.schema` | header `<Entity>: <table>` / `schema:` |
| `Table.clr_type` | derived (not written; metadata) |
| `Table.comment` | `comment:` |
| `Table.columns` | `fields:` |
| `Table.primary_key` | `pk` facet or `key:` block |
| `Table.foreign_keys` | `belongs-to:` |
| `Table.indexes` | `unique` facet or `indexes:` block |
| `Table.triggers` | `triggers:` |
| `Table.seed_data` | `seed:` |
| `Column.name` | field label (physical: `column=`) |
| `Column.clr_type` | logical type |
| `Column.store_type` | raw native type or `dbtype=` |
| `Column.is_nullable` | `?` suffix |
| `Column.is_identity` | `identity` |
| `Column.max_length` | `maxlen=` |
| `Column.precision` / `.scale` | `precision=p,s` |
| `Column.default_value_sql` | `default=` |
| `Column.computed_sql` / `.computed_stored` | `computed=` / `stored` |
| `Column.collation` | `collation=` |
| `Column.comment` | `comment=` |
| `PrimaryKey.{name,columns}` | `pk` / `pk=` / `key:` |
| `ForeignKey.*` | `belongs-to:` (see §8) |
| `Index.*` | `indexes:` (see §9) |
| `Trigger.*` | `triggers:` (see §14) |
| `DbFunction.*` | `functions:` (see §15) |

**No orphans:** every IR field has a representation. The only ones not written
are `format_version` (internal) and the table `clr_type` (derived metadata).

---

## 17. Canonical forms vs sugar

- **Canonical** = what the emitter produces (scaffold DB→`.model`): flat columns,
  explicit FKs, explicit indexes, without `owns`/`subtypes`/`enum`.
- **Sugar** = human authoring shortcuts (`owns`, `subtypes`, `enum[…]`,
  `has-many`) that expand on parsing. They do **not** survive a re-emit.

Guarantee: `parse(emit(ir)) == ir` for any IR. `emit(parse(dsl))` produces the
equivalent canonical form (it may differ textually from the original sugar; like
a formatter).

---

## 18. Grammar (informal EBNF)

```ebnf
file        = entity_file | database_file ;

entity_file = header NEWLINE INDENT { section } DEDENT ;
header      = IDENT [ ":" ws name ] ;
section     = "schema:"  ws name
            | "comment:" ws string
            | "discriminator:" ws IDENT
            | fields_blk | key_line | belongs_blk
            | indexes_blk | triggers_blk | seed_blk
            | owns_inline | subtypes_blk ;

fields_blk  = "fields:" NEWLINE INDENT { field } DEDENT ;
field       = IDENT ":" ws type [ "?" ] { ws facet } NEWLINE ;
type        = logical_type | native_type | enum_type | owns_type ;
enum_type   = "enum[" idlist "]" ;
owns_type   = "owns" ws IDENT ws inline_obj ;
facet       = "pk" | "identity" | "unique" | "stored"
            | "maxlen=" INT | "precision=" INT [ "," INT ]
            | "default=" value | "computed=" value | "collation=" name
            | "column=" name | "dbtype=" name | "comment=" string
            | "pk=" name | "as=" ("string"|"int") ;

key_line    = "key:" ws list [ ws "name=" name ] ;
belongs_blk = "belongs-to:" NEWLINE INDENT { fk_entry } DEDENT ;
fk_entry    = IDENT ":" ws inline_obj ;       (* keys: fk, references, onDelete, name *)

indexes_blk = "indexes:" NEWLINE INDENT { "-" ws IDENT ":" ws inline_obj } DEDENT ;
triggers_blk= "triggers:" NEWLINE INDENT { trigger } DEDENT ;
trigger     = "-" ws IDENT ":" ( inline_obj | NEWLINE INDENT { kv } DEDENT ) ;

seed_blk    = "seed:" NEWLINE INDENT { "-" ws inline_obj } DEDENT ;

database_file = { db_section } ;
db_section  = "schema:" ws name | "product-version:" ws name
            | "extensions:" ws list
            | "functions:" NEWLINE INDENT { "-" ws IDENT ":" ws block } DEDENT
            | "raw:" NEWLINE INDENT { "-" ws block } DEDENT ;

value       = scalar | string | list | inline_obj | block ;
list        = "[" [ value { "," value } ] "]" ;
inline_obj  = "{" [ kv { "," kv } ] "}" ;
kv          = IDENT ":" ws value ;
block       = "|" NEWLINE INDENT { TEXT } DEDENT ;
```

---

## 19. Full example (see `examples/models/`)

`database.model`
```
schema: public
```

`Customer.model`
```
Customer
  fields:
    Id:      int pk
    Name:    string maxlen=200
    Email:   string? unique maxlen=320
    Contact: owns ContactInfo {Phone: string?}
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

`Order.model`
```
Order
  fields:
    Id:         int pk identity
    CustomerId: int
    Total:      decimal precision=18,2
    Status:     enum[Pending, Shipped] as=string maxlen=20
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade, name: fk_order_customer}
```

`Payment.model`
```
Payment
  discriminator: Discriminator
  fields:
    Id:     int pk identity
    Amount: decimal
  subtypes:
    CardPayment: {CardNumber: string}
    CashPayment: {Note: string?}
```

**Canonical** form of `Customer.model` after a re-emit (sugar expanded):
```
Customer
  fields:
    Id:            int pk
    Name:          string maxlen=200
    Email:         string? maxlen=320
    Contact_Phone: string?
  indexes:
    - ix_customer_email: {on: [Email], unique: true}
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

---

## 20. Decisions (resolved)

1. **Name and extension:** molde language, extension **`.model`**. ✔
2. **`?` as the only nullability marker** (no `required`). ✔
3. **Inverse navigations: NOT modeled** (see §8). Avoids duplication; no effect
   on the DDL. ✔
4. **Snapshot and migrations in `migrations/`** (visible, sibling of `models/`,
   versioned in git; `migrations/snapshot.json`). ✔
5. **Enums without CHECK** by default: `enum` only sets the column type (aligned
   with EF and with a clean round-trip). A `check` facet remains an optional
   future extension. ✔
