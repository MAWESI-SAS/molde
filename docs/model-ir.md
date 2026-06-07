# Model IR — Representación Intermedia

El **Model IR** es el contrato central de efrust. Toda la herramienta gira en
torno a él: distintos productores lo generan y un único consumidor (el diff +
los providers) lo procesa.

```
                    ┌──────────────────────────┐
   Sidecar .NET ───▶│                          │
   (model-first)    │      DatabaseModel       │──▶ diff() ──▶ [Operation] ──▶ SqlGenerator ──▶ SQL
                    │        (Model IR)        │
   Scaffolder ─────▶│                          │──▶ plantillas .cs (scaffold)
   (db-first)       └──────────────────────────┘
```

Modela el lado **relacional** (tablas/columnas), no el conceptual (entidades
CLR). Esto es deliberado: las migraciones operan sobre el esquema, y mantener el
IR relacional lo hace independiente de las sutilezas del modelo de objetos de EF.

## Tipos (ver `crates/efrust-core/src/model.rs`)

- `DatabaseModel` — raíz: `format_version`, `product_version`, `default_schema`, `tables[]`.
- `Table` — `name`, `schema?`, `clr_type?`, `columns[]`, `primary_key?`, `foreign_keys[]`, `indexes[]`.
- `Column` — `name`, `store_type?`, `clr_type?`, `is_nullable`, `is_identity`,
  facetas (`max_length`, `precision`, `scale`), `default_value_sql?`,
  `computed_sql?`, `collation?`.
- `PrimaryKey`, `ForeignKey` (con `ReferentialAction`), `Index`.

### `store_type` vs `clr_type`

- `clr_type` es el tipo .NET de origen (`System.String`). Siempre presente desde
  el sidecar/scaffolder.
- `store_type` es el tipo del motor (`character varying(200)`). Es **opcional**:
  si viene explícito (porque el usuario lo fijó con `HasColumnType`), el provider
  lo respeta tal cual; si no, el provider lo **deriva** de `clr_type` + facetas.

Esto da fidelidad sin obligar al sidecar a conocer cada dialecto.

## Snapshot

El snapshot es un `DatabaseModel` serializado a JSON (`snapshot.rs`). Representa
el estado del último migrado. `migrations add` hace `diff(snapshot, modelo_actual)`.

- `normalize()` ordena tablas/columnas/FKs/índices para que los diffs sean
  estables y los snapshots no generen ruido por reordenamientos.
- `format_version` permite migrar snapshots antiguos ante cambios incompatibles.

## Contrato con el sidecar

El JSON que emite `efrust-sidecar` debe deserializar 1:1 en `DatabaseModel`. Los
DTOs en `sidecar/EfRust.Sidecar/Program.cs` usan `[JsonPropertyName]` en
snake_case para casar con `serde`. **Cualquier cambio en el IR debe replicarse en
ambos lados** (test de ida y vuelta previsto en Fase 3).

## Estado del differ (`diff.rs`)

| Caso | Estado |
|---|---|
| Crear/eliminar tabla | ✅ Fase 0 |
| Añadir/eliminar/alterar columna | ✅ Fase 0 |
| Detección de renombrados (tabla/columna) | ⬜ Fase 4 (hoy = drop+add) |
| FKs e índices en el diff | ⬜ Fase 4 |
| Orden por dependencias (topológico) | ⬜ Fase 4 |
| Seed data (`HasData`) | ⬜ Fase 5 |

## Casos avanzados pendientes (Fase 5)

Owned types, herencia (TPH/TPT/TPC), value converters, shadow properties,
concurrency tokens, columnas computadas, many-to-many con skip navigations.
