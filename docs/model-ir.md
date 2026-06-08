# Model IR — Representación Intermedia

El **Model IR** es el contrato central de molde. Toda la herramienta gira en
torno a él: distintos productores lo generan y un único consumidor (el diff +
los providers) lo procesa.

```
                    ┌──────────────────────────┐
   molde .model ─────▶│                          │
   (model-first)    │      DatabaseModel       │──▶ diff() ──▶ [Operation] ──▶ SqlGenerator ──▶ SQL
                    │        (Model IR)        │
   Scaffolder ─────▶│                          │──▶ emit() ──▶ archivos .model (scaffold)
   (db-first)       └──────────────────────────┘
```

Modela el lado **relacional** (tablas/columnas), no el conceptual. Esto es
deliberado: las migraciones operan sobre el esquema, y mantener el IR relacional
lo hace independiente de las sutilezas de cualquier modelo de objetos.

## Tipos (ver `crates/molde-core/src/model.rs`)

- `DatabaseModel` — raíz: `format_version`, `product_version`, `default_schema`,
  `tables[]`, `functions[]`, `extensions[]`, `raw_objects[]`.
- `Table` — `name`, `schema?`, `clr_type?`, `comment?`, `columns[]`,
  `primary_key?`, `foreign_keys[]`, `indexes[]`, `triggers[]`, `seed_data[]`.
- `Column` — `name`, `store_type?`, `clr_type?`, `is_nullable`, `is_identity`,
  facetas (`max_length`, `precision`, `scale`), `default_value_sql?`,
  `computed_sql?`, `computed_stored`, `collation?`, `comment?`.
- `PrimaryKey`, `ForeignKey` (con `ReferentialAction`), `Index`, `Trigger`,
  `DbFunction`.

### `store_type` vs `clr_type`

- `clr_type` es el tipo lógico de origen (`System.String`). Lo aporta el parser
  de molde (al leer un tipo lógico del `.model`) o el scaffolder (al leer la BD).
- `store_type` es el tipo del motor (`character varying(200)`). Es **opcional**:
  si viene explícito (el `.model` lo fijó con `dbtype=`), el provider lo respeta
  tal cual; si no, el provider lo **deriva** de `clr_type` + facetas al aplicar.

Esto da fidelidad sin obligar a cada `.model` a conocer cada dialecto. El
scaffolder canonicaliza los `store_type` convencionales a `None`
(`canonicalize_for_models`), dejando solo lo exótico (jsonb, vector, tsvector…).

## El lenguaje molde

El productor model-first es el lenguaje **molde** (`.model`, una entidad por
archivo, estilo indentado). La crate `molde-lang` garantiza el round-trip
`parse(emit(ir)) == ir`. Ver `docs/molde-language-spec.md` para el contrato del
lenguaje (léxico, secciones, tipos, facetas, azúcar y mapeo IR↔molde).

> Histórico: en versiones previas el productor model-first era un sidecar .NET
> que serializaba EF Core a JSON. Se retiró por completo; molde es 100% Rust.

## Snapshot

El snapshot es un `DatabaseModel` serializado a JSON (`snapshot.rs`). Representa
el estado del último migrado. `migrations add` hace `diff(snapshot, modelo_actual)`.

- `normalize()` ordena tablas/columnas/FKs/índices para que los diffs sean
  estables y los snapshots no generen ruido por reordenamientos.
- `format_version` permite migrar snapshots antiguos ante cambios incompatibles.

## Estado del differ (`diff.rs`)

El diff emite operaciones en un orden seguro frente a dependencias (drop de
FKs/índices → create tablas → alterar columnas → add FKs/índices → drop tablas).

| Caso | Estado |
|---|---|
| Crear/eliminar tabla | ✅ |
| Añadir/eliminar/alterar columna | ✅ |
| FKs e índices en el diff | ✅ |
| Triggers, funciones y extensiones | ✅ |
| Seed data (`InsertData`/`UpdateData`/`DeleteData`) | ✅ |
| Rebuild de tablas (SQLite) ante `ALTER` no soportado | ✅ |
| Detección de renombrados (tabla/columna) | ⬜ (hoy = drop+add) |

## Azúcar de modelado soportada (en molde)

Owned types (`owns`), herencia TPH (`subtypes`/`discriminator`), enums
(`enum[…]`) y columnas computadas (`computed=`, `stored`) se expanden al parsear
el `.model`. Pendientes: value converters, shadow properties, concurrency
tokens, many-to-many con skip navigations.
