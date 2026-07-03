# Model IR — Representación Intermedia

El **Model IR** es el contrato central de molde. Toda la herramienta gira en
torno a él: varios productores lo generan y un único consumidor (el diff + los
providers) lo procesa.

```
                    ┌──────────────────────────┐
   molde .model ─────▶│                          │
   (model-first)    │      DatabaseModel       │──▶ diff() ──▶ [Operation] ──▶ SqlGenerator ──▶ SQL
                    │        (Model IR)        │
   Scaffolder ─────▶│                          │──▶ emit() ──▶ .model files (scaffold)
   (db-first)       └──────────────────────────┘
```

Modela el lado **relacional** (tablas/columnas), no el conceptual. Esto es
deliberado: las migraciones operan sobre el esquema, y mantener el IR
relacional lo hace independiente de las particularidades de cualquier modelo
de objetos.

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

- `clr_type` es el tipo lógico de origen (`System.String`). Proviene del
  parser de molde (al leer un tipo lógico desde el `.model`) o del scaffolder
  (al leer la BD).
- `store_type` es el tipo del motor (`character varying(200)`). Es
  **opcional**: si viene explícito (el `.model` lo definió con `dbtype=`), el
  provider lo respeta tal cual; de lo contrario, el provider lo **deriva** de
  `clr_type` + facetas al aplicar.

Esto da fidelidad sin forzar a que cada `.model` conozca cada dialecto. El
scaffolder canonicaliza los `store_type`s convencionales a `None`
(`canonicalize_for_models`), dejando solo los exóticos (jsonb, vector, tsvector…).

## El lenguaje molde

El productor model-first es el lenguaje **molde** (`.model`, una entidad por
archivo, estilo indentado). El crate `molde-lang` garantiza el round-trip
`parse(emit(ir)) == ir`. Consulta `docs/molde-language-spec.md` para el
contrato del lenguaje (estructura léxica, secciones, tipos, facetas, azúcar
sintáctica, y el mapeo IR↔molde).

> Histórico: en versiones anteriores el productor model-first era un sidecar
> de .NET que serializaba EF Core a JSON. Se eliminó por completo; molde es
> 100% Rust.

## Snapshot

El snapshot es un `DatabaseModel` serializado a JSON (`snapshot.rs`).
Representa el estado de la última migración. `migrations add` ejecuta
`diff(snapshot, current_model)`.

- `normalize()` ordena tablas/columnas/FKs/índices para que los diffs sean
  estables y los snapshots no generen ruido por reordenamientos.
- `format_version` permite migrar snapshots antiguos frente a cambios incompatibles.

## Estado del differ (`diff.rs`)

El diff emite operaciones en un orden seguro respecto a las dependencias
(eliminar FKs/índices → crear tablas → alterar columnas → agregar FKs/índices
→ eliminar tablas).

| Case | Status |
|---|---|
| Crear/eliminar tabla | ✅ |
| Agregar/eliminar/alterar columna | ✅ |
| FKs e índices en el diff | ✅ |
| Triggers, funciones y extensiones | ✅ |
| Datos semilla (`InsertData`/`UpdateData`/`DeleteData`) | ✅ |
| Reconstrucción de tabla (SQLite) en `ALTER` no soportado | ✅ |
| Detección de renombrado (tabla/columna) | ⬜ (hoy = drop+add) |

## Azúcar sintáctica de modelado soportada (en molde)

Los tipos owned (`owns`), la herencia TPH (`subtypes`/`discriminator`), los
enums (`enum[…]`), y las columnas calculadas (`computed=`, `stored`) se
expanden al parsear el `.model`. Pendiente: value converters, shadow
properties, concurrency tokens, many-to-many con skip navigations.
