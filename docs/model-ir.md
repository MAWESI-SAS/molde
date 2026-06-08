# Model IR — Intermediate Representation

The **Model IR** is molde's central contract. The whole tool revolves around it:
various producers generate it and a single consumer (the diff + the providers)
processes it.

```
                    ┌──────────────────────────┐
   molde .model ─────▶│                          │
   (model-first)    │      DatabaseModel       │──▶ diff() ──▶ [Operation] ──▶ SqlGenerator ──▶ SQL
                    │        (Model IR)        │
   Scaffolder ─────▶│                          │──▶ emit() ──▶ .model files (scaffold)
   (db-first)       └──────────────────────────┘
```

It models the **relational** side (tables/columns), not the conceptual one. This
is deliberate: migrations operate on the schema, and keeping the IR relational
makes it independent of the subtleties of any object model.

## Types (see `crates/molde-core/src/model.rs`)

- `DatabaseModel` — root: `format_version`, `product_version`, `default_schema`,
  `tables[]`, `functions[]`, `extensions[]`, `raw_objects[]`.
- `Table` — `name`, `schema?`, `clr_type?`, `comment?`, `columns[]`,
  `primary_key?`, `foreign_keys[]`, `indexes[]`, `triggers[]`, `seed_data[]`.
- `Column` — `name`, `store_type?`, `clr_type?`, `is_nullable`, `is_identity`,
  facets (`max_length`, `precision`, `scale`), `default_value_sql?`,
  `computed_sql?`, `computed_stored`, `collation?`, `comment?`.
- `PrimaryKey`, `ForeignKey` (with `ReferentialAction`), `Index`, `Trigger`,
  `DbFunction`.

### `store_type` vs `clr_type`

- `clr_type` is the logical source type (`System.String`). It comes from the
  molde parser (when reading a logical type from the `.model`) or the scaffolder
  (when reading the DB).
- `store_type` is the engine type (`character varying(200)`). It is **optional**:
  if it comes explicitly (the `.model` set it with `dbtype=`), the provider
  respects it as-is; otherwise, the provider **derives** it from `clr_type` +
  facets on apply.

This gives fidelity without forcing every `.model` to know every dialect. The
scaffolder canonicalizes conventional `store_type`s to `None`
(`canonicalize_for_models`), leaving only the exotic ones (jsonb, vector, tsvector…).

## The molde language

The model-first producer is the **molde** language (`.model`, one entity per
file, indented style). The `molde-lang` crate guarantees the round-trip
`parse(emit(ir)) == ir`. See `docs/molde-language-spec.md` for the language
contract (lexical structure, sections, types, facets, sugar, and IR↔molde mapping).

> Historical: in earlier versions the model-first producer was a .NET sidecar
> that serialized EF Core to JSON. It was removed entirely; molde is 100% Rust.

## Snapshot

The snapshot is a `DatabaseModel` serialized to JSON (`snapshot.rs`). It
represents the state of the last migration. `migrations add` runs
`diff(snapshot, current_model)`.

- `normalize()` orders tables/columns/FKs/indexes so that diffs are stable and
  snapshots don't generate noise from reorderings.
- `format_version` allows migrating old snapshots in the face of incompatible changes.

## Differ status (`diff.rs`)

The diff emits operations in an order that is safe with respect to dependencies
(drop FKs/indexes → create tables → alter columns → add FKs/indexes → drop tables).

| Case | Status |
|---|---|
| Create/drop table | ✅ |
| Add/drop/alter column | ✅ |
| FKs and indexes in the diff | ✅ |
| Triggers, functions, and extensions | ✅ |
| Seed data (`InsertData`/`UpdateData`/`DeleteData`) | ✅ |
| Table rebuild (SQLite) on unsupported `ALTER` | ✅ |
| Rename detection (table/column) | ⬜ (today = drop+add) |

## Supported modeling sugar (in molde)

Owned types (`owns`), TPH inheritance (`subtypes`/`discriminator`), enums
(`enum[…]`), and computed columns (`computed=`, `stored`) are expanded when
parsing the `.model`. Pending: value converters, shadow properties, concurrency
tokens, many-to-many with skip navigations.
