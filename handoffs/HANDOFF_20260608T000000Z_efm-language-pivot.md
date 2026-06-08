# Handoff — efm-language-pivot
Date (UTC): 20260608T000000Z | Status: DONE
Prev handoff: handoffs/HANDOFF_20260607T201034Z_modelfirst-close-scaffold-fidelity.md
Base commit: 67be563 | Branch: main | Head: 7aaaf0c

## 1. Session Objective
- Pivote de producto: crear un lenguaje de modelos propio (EFM, `.model`, estilo TOON/YAML
  indentado, una entidad por archivo) que **reemplace por completo a C#/EF Core** como formato
  de definición. efrust pasa a ser herramienta de **capa de esquema 100% Rust** (scaffold,
  migraciones, apply); el acceso a datos en runtime queda fuera de alcance.
- Estado: DONE. Las 4 fases (A–D) completas en `main`. 42 tests + 3 ignored, clippy 0, fmt limpio.

## 2. Current Work Summary
5 commits (`e80534c`→`7aaaf0c`):
- `e80534c` — spec del lenguaje (`docs/efm-language-spec.md`).
- `8a270e7` — crate `efrust-lang` (parser/emitter + round-trip).
- `ca995e3` — `scaffold` a `.model` (Fase B).
- `8b30fab` — `migrations add` desde `.model`, sin .NET (Fase C).
- `7aaaf0c` — retiro del sidecar .NET y codegen C# (Fase D).

## 3. Exact Files (clave)
- **NUEVO** `crates/efrust-lang/` — `value.rs` (tokenizer/valores inline), `tree.rs` (árbol por
  indentación + block scalars `|`), `types.rs` (lógico↔CLR), `parse.rs` (texto→IR + azúcar),
  `emit.rs` (IR→texto canónico), `lib.rs` (API: `parse_project`/`emit_project`/`parse_entity`/
  `emit_entity`/`parse_database`/`emit_database`, `ModelFile`, `EfmError`), `tests/roundtrip.rs`.
- `docs/efm-language-spec.md` — spec formal (léxico, secciones, tipos, facetas, azúcar, EBNF,
  mapeo IR↔EFM campo a campo, decisiones resueltas).
- `crates/efrust-scaffold/src/lib.rs` — `build_model_files` (read_model→canonicalize→emit_project)
  + `canonicalize_for_models` (quita store_type convencional, precision/scale de enteros, schema
  redundante) + `exotic_store_type` (movido aquí desde el borrado codegen.rs).
- `crates/efrust-cli/src/commands/scaffold.rs` — solo `.model` (default `models/`).
- `crates/efrust-cli/src/commands/migrations.rs` — `add` lee `.model` (`--from-models`, default
  `models`); `load_model_dir`; defaults `migrations/` + `snapshot.json`. Sin sidecar.
- `crates/efrust-cli/src/commands/database.rs` — default `--migrations-dir migrations`.
- **BORRADO**: `sidecar/`, `crates/efrust-design/src/sidecar.rs`,
  `crates/efrust-scaffold/src/{codegen,csharp}.rs`, `crates/efrust-core/tests/sidecar_contract.rs`
  (+fixture), `scripts/parity-postgres.sh`, `scripts/schema-dump.sql`, `examples/SampleModel/`,
  `examples/Migrations/`.
- **NUEVO** `examples/models/{database,Customer,Order}.model` — ejemplo EFM.
- README reescrito; `.devcontainer/post-create.sh` des-dotnetizado.

## 4. Architectural Context
- Todo pivota sobre el IR `efrust_core::model::DatabaseModel`. Tres flujos, un centro:
  - BD ──reader──▶ IR ──emit──▶ `.model`     (scaffold)
  - `.model` ──parse──▶ IR ──diff(snapshot)──▶ migración   (migrations add; Rust puro)
  - migración ──▶ BD   (database update; ya existía)
- Crates: core (IR/diff/snapshot/migration), **lang** (EFM parse/emit), providers (SqlGenerator
  por motor), migrate (apply: sqlx Any + tiberius), scaffold (reader BD→IR + build_model_files),
  design (author: diff contra snapshot). El sidecar .NET y el codegen C# YA NO EXISTEN.
- Garantía: `parse(emit(ir)) == ir`. Azúcar (`owns`/`subtypes`/`enum`) se expande al parsear; el
  emit produce siempre la forma canónica plana.

## 5. Important Runtime / Business Rules
- [verified] Round-trip del lenguaje sobre fixture exhaustivo (PK simple/compuesta, identity,
  FK cascade/set_null, índice único inline, índices expresión/método/operadores/filtro, tipos
  nativos vector/tsvector, dbtype override, computed-stored, default, comment, trigger multilínea,
  globales functions/extensions/raw).
- [verified] `canonicalize_for_models`: store_type convencional→None (se re-deriva al aplicar;
  jsonb/vector se conservan vía dbtype=), precision/scale solo en decimales, schema de tabla/FK
  ==default→None. Test unit + e2e Postgres.
- [verified] e2e Fase C (Postgres limpio, sin .NET): migrations add desde `.model` → database
  update crea tablas/identity/FK-cascade/seed; editar `.model` → 1 op AddColumn; re-add → sin
  cambios; rollback `--target 0` → revierte todo dejando `__EFMigrationsHistory`.
- [verified] El ejemplo `examples/models/` parsea (2 tablas, default_schema public).

## 6. Known Problems / Pending Risks
- **LOW** — Fase E (opcional) pendiente: errores de parseo con snippet de línea/columna; test de
  integración `#[ignore]` del ciclo `.model`→migración→apply.
- **LOW** — `.devcontainer/Dockerfile` aún trae base .NET (post-create ya no usa dotnet).
- **LOW** — `docs/model-ir.md` menciona el sidecar (histórico, no actualizado).
- (Heredados) SQL Server FTS sin e2e; repo sin remoto git; cert real X.509 v1 requiere
  `--features tls-native-tls`.

## 7. Next Recommended Actions (opcionales)
1. Fase E: `EfmError` con snippet de la línea; test e2e ignorado del ciclo completo.
2. Limpiar el Dockerfile del devcontainer (quitar .NET) y `docs/model-ir.md`.
3. `git remote add origin … && git push -u origin main`.

## 8. Validation Performed
- `cargo test --workspace` (rust:1-bookworm) → PASS: core 11, design 4, efrust-lang 5,
  migrate 1, providers 18, scaffold 3 (+3 ignored). clippy 0, fmt limpio.
- e2e Fase B: scaffold de Postgres real (serial PK, índice único, jsonb, numeric(18,2), FK
  cascade) → `.model` limpio que reparsea a IR equivalente.
- e2e Fase C: ciclo model-first completo + rollback, 100% Rust sin .NET.
- Recursos Docker (efrust-pg, efrust-net) y scratch dirs eliminados; volúmenes de caché
  conservados.

## 9. Recommended Entry Points for Next Session
- `crates/efrust-lang/src/parse.rs` / `emit.rs` — núcleo del lenguaje.
- `docs/efm-language-spec.md` — contrato del lenguaje.
- `crates/efrust-scaffold/src/lib.rs:build_model_files` + `canonicalize_for_models`.
- `crates/efrust-cli/src/commands/{scaffold,migrations,database}.rs`.

## 10. Context That Should NOT Be Lost
- Decisión raíz: efrust = SOLO esquema; el lenguaje EFM reemplaza C# por completo (sin sidecar,
  sin codegen C#). Memoria del proyecto: `efrust-pivot-own-language` (actualizada a completo).
- Build/test/e2e vía Docker (host sin cargo/dotnet); ver memoria `efrust-docker-build-flow`.
- El IR `DatabaseModel` es el centro inamovible; cualquier capacidad nueva = campo IR (serde
  default) + parse/emit + diff + provider.
- `store_type` round-trip exacto: emit pone `dbtype=` iff store_type Some; parse lo reconstruye;
  los tipos lógicos no llevan store_type (lo deriva el provider al aplicar).
