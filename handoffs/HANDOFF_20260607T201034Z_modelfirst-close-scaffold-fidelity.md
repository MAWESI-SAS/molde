# Handoff — modelfirst-close-scaffold-fidelity
Date (UTC): 20260607T201034Z | Status: DONE
Prev handoff: handoffs/HANDOFF_20260607T193826Z_search-advanced-ef-realdb.md
Base commit: 73c2275  | Branch: main | Head: 1a48482

## 1. Session Objective
- Partía del baseline DONE de la sesión previa (51 tests, clippy 0, fmt limpio, round-trip Postgres real perfecto).
- Esta sesión abordó dos de los "Next Recommended Actions" opcionales del handoff previo:
  - **#1** Cerrar el ciclo model-first → apply: ejecutar `efrust database update` (no solo `migrations add`) contra un Postgres limpio y verificar tablas + seed + rollback.
  - **#3** Fidelidad C# scaffold: emitir `HasPrecision(p,s)` para numeric/decimal (y evaluar `HasConversion<>()`).
- Estado: DONE. 2 commits nuevos en `main`. **52 tests, clippy 0, fmt limpio.** Árbol limpio salvo los archivos de handoff (este + INDEX/LATEST).

## 2. Current Work Summary
- **Commit `7931073`** — `efrust: skip owned-type ownership FK when sharing owner table`.
- **Commit `1a48482`** — `efrust: emit HasPrecision(p,s) for numeric/decimal in C# scaffold`.
- Ambos verificados e2e contra Postgres real vía Docker, no solo unit tests.

## 3. Exact Files Modified
- `sidecar/EfRust.Sidecar/Program.cs` (commit 7931073, +9 líneas) — en el bucle de FKs de `ModelMapper.Map`, se omiten las FKs con `fk.IsOwnership && principalStore.Equals(store)`. Evita reproducir la FK de propiedad (Id→Id) de un owned type que comparte tabla con su dueño, que EF nunca emite como constraint física.
- `crates/efrust-scaffold/src/codegen.rs` (commit 1a48482, +85 líneas):
  - En el bucle de columnas de `write_entity_config` (~`:268`): tras `HasColumnType`, si `is_decimal_store_type(col)` y hay `precision`, emite `.HasPrecision(p[, s])` (escala 0 omitida).
  - Nuevo helper `is_decimal_store_type` (tras `exotic_store_type`, ~`:526`): base type `numeric`/`decimal` (por store_type, o clr `System.Decimal` si store_type es None). `money`/`smallmoney` excluidos.
  - Nuevo test `numeric_con_precision_emite_has_precision` (cubre numeric(18,2)→HasPrecision(18,2), numeric(10,0)→HasPrecision(10), numeric pelado→nada).

## 4. Architectural Context
- (Sin cambios respecto al handoff previo §4.) Crates: core/providers/migrate/scaffold/design/cli + sidecar .NET. Doble driver (tiberius SQL Server, sqlx Any resto).
- **`scaffold` es estrictamente DB-first**: `efrust_scaffold::build_files` lee SOLO del esquema de la BD (commands/scaffold.rs). El modelo que alimenta `codegen.rs` viene únicamente del reader, nunca del sidecar.

## 5. Important Runtime / Business Rules
- [verified] Owned type con `OwnsOne` (sin `.ToTable` propio) = `IEntityType` separado mapeado a la MISMA tabla del dueño. Su FK de ownership (Id→Id) se colaba en la fusión por tabla → FK auto-referencial espuria `FK_Customer_Customer`. EF NO la emite cuando comparten tabla. Guard `fk.IsOwnership && principalStore==store` la elimina; TPH, owned-con-tabla-propia y self-refs reales (ManagerId→Id, NO ownership) quedan intactos.
- [verified] El reader ya puebla `col.precision`/`col.scale` para Postgres (reader.rs `:336/:337`), MySQL (`:915/:916`) y SQL Server (`:1136/:1137`). SQLite los deja None.
- [verified] numeric/decimal son "convencionales" → `exotic_store_type` devuelve None → no emitían HasColumnType ni nada que conservara p/s. `HasPrecision` lo arregla. Escala 0 se omite (idiom EF). numeric pelado (precision None) no emite nada.
- [verified] `HasConversion<>()` NO es reconstruible desde DB-first: un value-converter (enum→string) es solo varchar en la BD; no hay tipo enum que recuperar (misma limitación que `dotnet ef`). El IR model-first (snapshot del sidecar) sí lo preserva, pero ese camino no pasa por codegen.rs. Se decidió NO emitir nada (no inventar).

## 6. Known Problems / Pending Risks
- (Heredados del handoff previo, sin cambios) **MED** SQL Server full-text sin e2e (imagen mssql sin componente Full-Text). **LOW** sin remoto git. **LOW** scaffold no reconstruye jerarquía TPH desde BD. **LOW** cert real X.509 v1 requiere `--features tls-native-tls`.
- **RESUELTO esta sesión**: FK ownership espuria (era un bug no listado); precision/scale en C# scaffold (era §6 LOW del handoff previo).

## 7. Next Recommended Actions (opcionales; nada bloquea)
1. **#2** e2e real de SQL Server full-text: imagen mssql con Full-Text; verificar `read_sqlserver_fulltext` + apply de `raw_objects`.
2. **#4** Backup git: `git remote add origin … && git push -u origin main`.
3. (Nuevo, opcional) Test de integración que cubra el ciclo model-first completo (`migrations add` + `database update` + rollback) como `#[ignore]`, para no depender de verificación manual vía Docker.

## 8. Validation Performed
- `cargo test --workspace` (rust:1-bookworm) → **PASS, 52 tests** (core 11, providers 18, scaffold **16** ←+1, design 5, migrate 1, sidecar_contract 1) + 3 ignored. `cargo clippy --workspace --all-targets` → 0. `cargo fmt --all -- --check` → CLEAN.
- **Ciclo model-first e2e** (SampleModel → sidecar → `migrations add InitialCreate` → `database update --provider postgres` contra `postgres:16` limpio):
  - Tablas creadas: Customer, Order, Payment (TPH con Discriminator), __EFMigrationsHistory.
  - Verificado: PK_Customer, IX_Customer_Email (única), FK_Order_Customer cascade, owned Contact_Phone embebido, Status varchar(20), identity en Order/Payment, Amount numeric.
  - Seed (HasData) aplicado: ACME (email), Globex (email null).
  - **Bug encontrado y corregido**: FK espuria FK_Customer_Customer (antes del fix la migración era 9up/7down; después 8up/6down).
  - **Rollback** (`database update --target 0`): down ops eliminan las 3 tablas, dejando solo __EFMigrationsHistory. ✔
- **Scaffold e2e** (Postgres con invoice: numeric(18,2)/numeric(10,0)/numeric/varchar(200)): genera `.HasPrecision(18, 2)` / `.HasPrecision(10)` / nada / `.HasMaxLength(200)` sin HasColumnType. ✔
- Recursos Docker (efrust-pg, efrust-net) y dirs scratch eliminados. Volúmenes de caché conservados (efrust-target, efrust-cargo-reg, efrust-nuget).

## 9. Recommended Entry Points for Next Session
- `sidecar/EfRust.Sidecar/Program.cs` → bucle de FKs en `ModelMapper.Map` (`:340`), guard `IsOwnership`.
- `crates/efrust-scaffold/src/codegen.rs:268` → bucle de columnas (HasMaxLength/HasColumnType/HasPrecision); `:526`-ish `is_decimal_store_type`.
- `crates/efrust-cli/src/commands/database.rs` → `update` (apply/rollback con `--target`).
- `scripts/parity-postgres.sh` → patrón Docker para correr el binario + sidecar + Postgres juntos.

## 10. Context That Should NOT Be Lost
- **Cómo correr el ciclo model-first vía Docker** (host sin cargo/dotnet): (a) build rust `docker run … rust:1-bookworm cargo build -p efrust-cli` (binario en volumen efrust-target:/app/target); (b) build .NET `docker run … mcr.microsoft.com/dotnet/sdk:9.0 dotnet build sidecar/EfRust.Sidecar -c Release && dotnet build examples/SampleModel -c Release`; (c) orquestar en la imagen dotnet/sdk:9.0 (tiene dotnet runtime para el sidecar) montando efrust-target para usar el binario rust, con `--network efrust-net` hacia el contenedor postgres. El binario rust (bookworm) corre sin problema en la imagen dotnet sdk (también bookworm/glibc).
- **El migrador identifica migraciones aplicadas por id contra __EFMigrationsHistory**: regenerar la migración con `migrations add` produce un timestamp NUEVO; para probar rollback hay que reutilizar el MISMO dir de migración (persistirlo en un bind mount), no regenerarlo.
- **El owned type seed**: HasData no sembró Contact, así que Contact_Phone quedó null en ambas filas (correcto).
- **HasConversion descartado conscientemente** — ver §5; no es una omisión por olvido.
