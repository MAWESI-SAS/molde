# Handoff — search-advanced-ef-realdb
Date (UTC): 20260607T193826Z | Status: DONE
Prev handoff: handoffs/HANDOFF_20260607T153712Z_sqlserver-scaffold-close.md
Base commit: 73c2275  | Branch: main

## 1. Session Objective
- Partía del baseline previo (repo SIN commits, 4 motores apply+scaffold+migraciones, 24 tests).
- Esta sesión: (a) commit inicial; (b) Fases 9–18 = búsqueda/full-text en los motores que aplican, singularización, SQLite table-rebuild, sidecar como `dotnet tool`, casos EF avanzados (TPH/HasData/owned/value-converters), limpieza fmt/clippy; (c) **endurecimiento contra una BD Postgres REAL de producción** (219 tablas) con round-trip **perfecto** (0 errores, 0 diff residual).
- Estado: DONE. `git status` limpio, todo commiteado en `main`. **51 tests, clippy 0, fmt limpio.**

## 2. Current Work Summary
- **Completado** (16 commits, `63fb417`→`73c2275`): ver §3 por archivo y `git log --oneline -16`.
- Decisiones tratadas como verdad:
  - **Rama `main`** (renombrada desde `master` antes del 1er commit). Repo solo local, **sin remoto**.
  - **Builds/tests vía Docker** (host sin cargo/dotnet). Volúmenes de caché: `-v efrust-target:/app/target -v efrust-cargo-reg:/usr/local/cargo/registry` (rust:1-bookworm); `-v efrust-nuget:/root/.nuget/packages` (mcr.microsoft.com/dotnet/sdk:9.0). Volumen aparte para native-tls: `efrust-target-nativetls`.
  - **Codegen es provider-aware** (`CodegenOptions.provider`): `HasMethod`/`HasOperators` solo Postgres; índices no-Fluent (expresión/FULLTEXT) → artefacto `<Context>.DbObjects.sql` con el dialecto del motor.
  - **SQL Server full-text = code-complete, NO verificado e2e** (la imagen Linux mssql no trae el componente Full-Text; `FULLTEXTSERVICEPROPERTY('IsFullTextInstalled')=0`).
- Pendiente = solo opcionales (§6/§7). Nada bloquea.

## 3. Exact Files Modified (este sesión; agregadas/ampliadas)
> Confirmado contra `git log`/`git show`. Árbol limpio (sin cambios sin commitear).
- `crates/efrust-core/src/model.rs` — IR ampliado: `Index.method/operators/expression` (`:215/:219/:223`); `Table.triggers` (`:114`), `Table.seed_data` (`:118`, `BTreeMap<String,serde_json::Value>`); `DatabaseModel.functions/raw_objects/extensions` (`:36/:41/:45`); nuevos `Trigger`/`TriggerTiming`/`TriggerEvent`/`DbFunction` (`:229/:249/:268`). Todos los campos nuevos `#[serde(default)]`. `normalize()` ordena los nuevos vec.
- `crates/efrust-core/src/diff.rs` — `Operation` +8 variantes (`EnsureExtension:65`, `CreateFunction:69`, `DropFunction`, `CreateTrigger:77`, `DropTrigger`, `RawSql:88`, `RebuildTable:96`, `InsertData:103`/`DeleteData:109`/`UpdateData:115`). `diff()` ordena: ensure_extensions → drops → create_tables → column_ops → add_fks → **create_functions** → create_indexes → create_triggers → rebuilds → data_ops → raw_sql → drop_tables (funciones tras tablas, antes de índices/triggers). Helpers: `requires_vector_extension`, `wanted_extensions`, `diff_seed_data`, `diff_triggers`. `apply_operation` maneja todas (idempotente: EnsureExtension/RawSql/InsertData pushean al modelo).
- `crates/efrust-providers/src/generator.rs` — trait: `bool_literal` (TRUE/FALSE PG vs 1/0), `sql_value`, `qualify`, `emit_insert_data/emit_update_data/emit_delete_data`, `skip_db_object`. Dep `serde_json`.
- `crates/efrust-providers/src/{postgres,mysql,sqlite,sqlserver}.rs` — arms para las nuevas ops. PG: `EnsureExtension`→`CREATE EXTENSION IF NOT EXISTS`, funciones/triggers verbatim, `create_index` con `USING method`+operator class+expresión, `column_def` con `GENERATED ALWAYS AS … STORED`, `bool_literal` TRUE/FALSE. MySQL: FULLTEXT/SPATIAL + `GENERATED … STORED|VIRTUAL`. SQLite: `RebuildTable` real (create-new/copy/drop/rename, `rebuild_table()`), `AlterColumn`/FK ahora warn-skip. SQLServer: `AS (expr) PERSISTED`. Otros motores: `RebuildTable`→no-op; funciones/triggers/extension→`skip_db_object`.
- `crates/efrust-scaffold/src/reader.rs` — `read_postgres` (`:263`): lee índices (pg_get_indexdef + catálogos), generated cols, vector/tsvector (`format_type`), triggers (`pg_get_triggerdef`), **todas** las funciones de usuario (pg_proc prokind='f', no-extension, no C/internal), **extensiones** (pg_extension≠plpgsql). `pg_store_type` (`:754`) ahora recibe precision/scale → `numeric(p,s)`. `read_mysql` (`:804`): INDEX_TYPE/FULLTEXT + generated cols. `read_sqlserver`: sys.computed_columns (PERSISTED) + `read_sqlserver_fulltext` (`:1304`, best-effort).
- `crates/efrust-scaffold/src/codegen.rs` — `CodegenOptions.provider` (`:23`); `index_is_fluent` (`:40`); `class_name` singulariza (`:89`); `exotic_store_type` (`:466`, emite `HasColumnType` para jsonb/array/citext/vector/tsvector); `db_objects_sql(model, provider)` (`:530`, incluye extensiones/funciones/índices-raw/triggers/raw_objects); `HasComputedColumnSql`, `HasMethod`/`HasOperators`/`HasFilter`, `HasData`.
- `crates/efrust-scaffold/src/csharp.rs` — `singularize()`; mapeo CLR `Pgvector.Vector`/`NpgsqlTypes.NpgsqlTsVector`.
- `crates/efrust-cli/Cargo.toml` + workspace `Cargo.toml` + scaffold/migrate `Cargo.toml` — **feature TLS**: `default=["tls-rustls"]`, `tls-native-tls` opcional; workspace deps efrust-scaffold/migrate con `default-features=false` (necesario para que el binario elija el backend).
- `crates/efrust-scaffold/tests/{roundtrip_postgres,roundtrip_mysql,roundtrip_sqlserver}.rs` — tests de integración `#[ignore]` (requieren BDs; leen SRC/DST de env).
- `sidecar/EfRust.Sidecar/Program.cs` — `ModelMapper.Map` agrupa entity types por tabla física (fusión TPH, dedup col/fk/idx) + extrae `GetSeedData()`. `.csproj`: `PackAsTool`/`ToolCommandName=efrust-sidecar`.
- `crates/efrust-design/src/sidecar.rs` — modo tool vía env `EFRUST_SIDECAR_CMD` (fallback `dotnet <dll>`).
- `examples/SampleModel/*` — TPH (Payment/CardPayment/CashPayment), `HasData`, owned `ContactInfo`, enum `OrderStatus` con `HasConversion<string>()`.

## 4. Architectural Context
- Crates: core (IR+diff+migration+snapshot), providers (`SqlGenerator` por motor, **sin** dep de runtime/BD), migrate (apply: `Backend::{Sqlx(AnyPool), Mssql(Box<Mutex<…>>)}`), scaffold (reader BD→IR + codegen C#), design (model-first: sidecar+author), cli (clap), sidecar .NET.
- **Doble driver** permanente: SQL Server = tiberius (TDS); resto = sqlx `Any`. (sqlx 0.8 sin MSSQL.)
- **Patrón a preservar**: `Operation` IR agnóstica → cada provider renderiza. Capacidades no soportadas por un motor = **warn-skip** (`skip_db_object` / `RebuildTable`→no-op), nunca romper el workspace. Campos IR nuevos siempre `#[serde(default)]`.
- **Escape hatches**: `RawSql`+`DatabaseModel.raw_objects` (DDL verbatim, p. ej. FTS SQL Server); `Trigger/DbFunction.definition` (DDL crudo de Postgres).

## 5. Important Runtime / Business Rules
- [verified] `diff()` orden funciones: **create_functions tras create_tables, antes de create_indexes/create_triggers** — necesario porque funciones SQL pueden referenciar tablas (validadas al crear) y los índices por expresión/triggers usan las funciones.
- [verified] `EnsureExtension` se emite por unión de `to.extensions` (leídas de pg_extension) ∪ heurística `vector` (uso de tipo/índice), menos las ya presentes en `from` → idempotente. `apply_operation` la añade a `model.extensions`.
- [verified] Round-trip Postgres exige: extensiones primero, luego funciones, luego índices. Con esto el residual fue **0** contra la BD real.
- [verified] `numeric`/`decimal` se renderiza `numeric(p,s)` desde precision/scale; antes "numeric" pelado perdía precisión (causaba AlterColumn+RebuildTable residuales).
- [verified] Codegen `HasColumnType` solo para tipos NO convencionales (lista `CONVENTIONAL` en `exotic_store_type`); varchar/int/numeric/uuid/timestamp NO lo emiten.
- [verified] SQLite `RebuildTable` lleva la `Table` completa + `copy_columns`; aplica por-op sin necesitar el modelo destino. Otros motores la ignoran (usan ALTER granular).
- [verified] Sidecar TPH: itera `GetEntityTypes()` agrupando por `(schema,tableName)`; sin esto, base+derivados generaban tablas duplicadas.
- [suspected] Funciones SQL de usuario que referencian tablas y se ordenan antes que ellas podrían fallar (check_function_bodies); no observado en la BD real (las suyas eran plpgsql / sin ref a tabla).

## 6. Known Problems / Pending Risks
- **MED** — SQL Server full-text (`read_sqlserver_fulltext` + raw_objects): code-complete, **sin e2e** (imagen mssql sin componente Full-Text). Acción: verificar con una imagen mssql con full-text, o aceptar como best-effort.
- **LOW** — Sin remoto git: todo solo local. Acción: `git remote add` + `push` si se quiere respaldo.
- **LOW** — Scaffold NO reconstruye jerarquía de clases TPH desde BD (la info no está en la BD; igual que `dotnet ef`). Solo aplica al flujo DB-first.
- **LOW** — Codegen no emite precision/scale para `numeric` ni `HasConversion<>()` para value converters (round-trip por IR sí los preserva; es fidelidad del C# scaffold, no del apply).
- **LOW** — Cert del servidor real es X.509 v1 → requiere `--features tls-native-tls` (rustls lo rechaza). Idealmente regenerar el cert como v3.
- **INFO** — Tests de integración round-trip son `#[ignore]` (necesitan BDs + imágenes pesadas).

## 7. Next Recommended Actions
1. (Opcional) Cerrar el ciclo model-first→apply: aplicar la migración generada por `efrust migrations add` con `efrust database update --provider postgres --connection <local>` contra un Postgres limpio; verificar tablas + seed. (Demo ya hecha de `migrations add`; falta el `database update` del mismo flujo.)
2. (Opcional) e2e real de SQL Server full-text: imagen mssql con Full-Text; verificar `read_sqlserver_fulltext` + apply de `raw_objects`.
3. (Opcional) Fidelidad C# scaffold: emitir `HasPrecision(p,s)` para numeric y `HasConversion<>()` en `codegen.rs` (ver `exotic_store_type`/bucle de columnas `:~247`). Verificar: `docker run … rust:1-bookworm cargo test -p efrust-scaffold`.
4. (Opcional) `git remote add origin … && git push -u origin main`.

## 8. Validation Performed
- `cargo test --workspace` (rust:1-bookworm, default rustls) → **PASS, 51 tests**. (core 11, providers 18, scaffold 15, design 5, migrate 1, sidecar_contract 1, doctests 0.)
- `cargo clippy --workspace --all-targets` → **0 warnings**. `cargo fmt --all -- --check` → **CLEAN**.
- **Round-trip e2e Postgres REAL** (BD prod `mwssaas_sm_test_aqualia_v2` SRC + contenedor local `postgres:16` DST, build `--features tls-native-tls`): leer→diff(vacío,modelo) 1680 ops→aplicar→releer→`diff(dst,src)` = **0 errores, 0 residual**. 219 tablas, 520 FKs, ~925 índices, 9 funciones, 8 triggers, ext [dblink,pg_trgm,unaccent].
- **Scaffold e2e** contra la BD real: 221 archivos; 187 `HasColumnType("jsonb")` + 8 tsvector; FTS triggers/funciones en `.sql`.
- e2e MySQL 8 (FULLTEXT+generated) y SQL Server 2022 (computed PERSISTED) round-trip vía tests `#[ignore]`: PASS (sesión).
- **model-first** `efrust migrations add InitialCreate` (SampleModel vía sidecar dotnet/sdk:9.0): migración 9 up/7 down con TPH(Discriminator)+owned(Contact_Phone)+value-converter(Status)+seed(2 insert_data); 2º add → "sin cambios"; `migrations list` → 1.
- **NO testeado**: SQL Server FTS e2e; `database update` del flujo model-first (solo `migrations add`); `cargo test` con `--features tls-native-tls` (solo build). Recursos Docker (efrust-pg-rt/efrust-mysql/efrust-mssql/efrust-pg/efrust-net) y dirs scratch eliminados; `target/` host limpio (caché en volúmenes Docker).

## 9. Recommended Entry Points for Next Session
- `crates/efrust-core/src/diff.rs:133` → `diff()` (orden de ops; `wanted_extensions`, `diff_seed_data`).
- `crates/efrust-core/src/model.rs:20` → `DatabaseModel` (campos functions/raw_objects/extensions) y `Index`/`Trigger`/`DbFunction`.
- `crates/efrust-scaffold/src/reader.rs:263` → `read_postgres` (índices/funciones/extensiones); `:754` `pg_store_type`.
- `crates/efrust-scaffold/src/codegen.rs:530` → `db_objects_sql`; `:466` `exotic_store_type`; `:40` `index_is_fluent`.
- `crates/efrust-providers/src/generator.rs` → trait `SqlGenerator` (defaults emit_*_data, bool_literal, qualify).
- `sidecar/EfRust.Sidecar/Program.cs` → `ModelMapper.Map` (agrupación por tabla / TPH / GetSeedData).

## 10. Context That Should NOT Be Lost
- **Round-trip real fue el mayor descubridor de bugs** (4 fixes que BDs sintéticas no revelaron): TLS v1, HasColumnType (jsonb), numeric(p,s), funciones de esquema+extensiones. El residual cayó 128→26→0 en 3 iteraciones.
- **TLS**: rustls rechaza certs X.509 v1 de forma irreconciliable (`UnsupportedCertVersion`); `sslmode=disable` falla porque el server exige TLS. Única vía = `tls-native-tls` (OpenSSL, necesita `libssl-dev`+`pkg-config` al compilar). NO reintentar arreglarlo en rustls.
- **default-features de workspace deps**: cargo NO permite override de `default-features` en el miembro si el workspace dep lo declara; hubo que poner `default-features=false` en `[workspace.dependencies]` para efrust-scaffold/migrate y que el binario forwardee la feature TLS.
- **Sidecar TPH**: el bug era tablas duplicadas (una por tipo CLR). El discriminador es una columna normal del modelo relacional → no requirió campo nuevo en el IR, solo agrupar por tabla.
- **owned types y value converters NO requirieron código nuevo**: owned embebido cae de la agrupación por tabla (16a); value converter ya queda reflejado en `store_type` (el sidecar lee el tipo convertido).
- **No correr `cargo fmt --all` dentro de fases de feature** — el baseline no estaba formateado; se aisló en commit `ab13b64`. Tras fmt, formatear solo el código nuevo.
- **Docker crea mountpoints root-owned** (`target/`, `scratch_*`) al montar volúmenes dentro del bind `/app`; limpiar con `docker run … rm -rf` o `chown -R $(id -u)`.
- **La cadena de conexión real se usó solo vía env, nunca en disco/repo/vault.** Detalle en Obsidian `Sessions/2026-06-07-EF_RUST-fases10-17-pendientes-completos.md`.
