# molde

[![CI](https://github.com/MAWESI-SAS/molde/actions/workflows/ci.yml/badge.svg)](https://github.com/MAWESI-SAS/molde/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
![MSRV](https://img.shields.io/badge/rustc-1.88%2B-orange.svg)

Una herramienta de **esquema de base de datos en Rust** construida sobre **molde**, un
lenguaje de modelos declarativo, legible y personalizado (estilo TOON/YAML). molde hace
**scaffolding** (DB → models), genera **migraciones** (model → diff), y las **aplica**
en 4 motores. Sin .NET, sin C#: todo en Rust.

> molde gestiona el **esquema** (models, migraciones, aplicación). El acceso a datos en
> tiempo de ejecución (las queries de tu aplicación) está fuera de alcance.

## El lenguaje molde (`.model`)

Una entidad por archivo; toda la configuración de la tabla vive junta. Ejemplo:

```
Customer
  fields:
    Id: int pk
    Name: string maxlen=200
    Email: string? unique maxlen=320
  seed:
    - {Id: 1, Name: "ACME", Email: "acme@example.com"}

Order
  fields:
    Id: int pk identity
    CustomerId: int
    Total: decimal precision=18,2
    Status: string maxlen=20
    CreatedAt: datetimeoffset
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
  indexes:
    - ix_order_status_created: {on: [Status, CreatedAt]}   # a query index you declare
```

### Convenciones (para que escribas menos)

molde completa el boilerplate, al estilo EF. Los nombres generados son **todos en
minúscula**: `pk_<table>`, `fk_<table>_<principal>`, `ix_<table>_<cols>`. Solo escribes
un nombre cuando quieres uno personalizado (los convencionales quedan ocultos en el
`.model`).

Los índices provienen de tres lugares:

- **Las foreign keys se indexan automáticamente.** Cada `belongs-to` obtiene un
  índice no único `ix_<table>_<cols>` — no lo declaras. Puedes excluirlo con
  `index: false`; molde también lo omite cuando las columnas ya están cubiertas por la
  PK u otro índice.
- **Unique de una sola columna** → el facet `unique` en el campo.
- **Todo lo demás** (rendimiento, compuestos, parciales, GIN/GiST/HNSW,
  expresión) → lo declaras en el bloque `indexes:`, por ejemplo
  `- ix_order_status_created: {on: [Status, CreatedAt]}` (la etiqueta es el nombre del
  índice; `on:` es la lista ordenada de columnas).

> Relaciones → molde las indexa por ti. Índices de query/rendimiento → tú los
> declaras.

¿Nuevo escribiendo modelos? Empieza con la
**[guía de autoría](docs/authoring-models.md)** basada en ejemplos. Gramática completa
y mapeo a IR: [`docs/molde-language-spec.md`](docs/molde-language-spec.md).

## Flujo

```
DB ──pull──▶ models/*.model
models/*.model ──migrate──▶ migrations/*.json (diff against snapshot)
migrations/*.json ──apply──▶ DB
```

Todo gira en torno a un **IR** compartido (`molde_core::DatabaseModel`): el lenguaje es
su forma textual, los readers lo producen a partir de la DB, y el diff genera el SQL.

## Arquitectura

```
molde (CLI, Rust)
├── molde-core         model IR + snapshot + diff + migrations (engine-agnostic)
├── molde-lang         molde language: parser/emitter (.model ↔ IR)
├── molde-providers    SqlGenerator per engine (SQLite, Postgres, MySQL, SQL Server)
├── molde-migrate      migration apply (Backend: sqlx Any + tiberius/TDS)
├── molde-scaffold     schema reading (DB → IR) + .model emission
└── molde-design       migration authoring (diff against snapshot)
```

### Matriz de capacidades por motor

| Capacidad | PostgreSQL | MySQL | SQLite | SQL Server |
|---|:--:|:--:|:--:|:--:|
| `db` (crear / eliminar / reiniciar la base de datos) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `apply` (aplicar migraciones) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `migrate` / `undo` / `status` | ✅ | ✅ | ✅ | ✅ |
| `pull` (DB → `.model`) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `sync` (DB → DB en vivo, aditivo) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `verify` (drift DB ⇄ model) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `up` / `fresh` (ponerse al día / reconstruir) | ✅ | ✅ | ✅ | ✅ |
| Búsqueda / texto completo (pull + round-trip) | ✅ pgvector+tsvector+triggers | ✅ FULLTEXT+generated | — | ✅ computed PERSISTED · FTS best-effort |

> SQLite: las FKs se declaran en línea dentro de `CREATE TABLE`; los cambios de tipo de
> columna y agregar/eliminar FKs en tablas existentes se aplican mediante
> reconstrucción de tabla (create-new/copy/drop/rename), al estilo EF.
> SQL Server usa el driver TDS `tiberius`; todo lo demás usa `sqlx` (`Any`).
> **PostgreSQL:** el scaffolding preserva `vector(N)` (pgvector), `tsvector` (incl.
> columnas generadas `STORED`), índices con method/operator class (GIN, GiST,
> HNSW, IVFFlat), e índices parciales. Functions, triggers, e índices de expresión
> se preservan en `.model` (bloques `triggers:`/`functions:`/`indexes:` y `raw:`).
> Los tipos no convencionales (`jsonb`, arrays, `citext`, `vector`, `tsvector`…) se
> conservan con `dbtype=`.

### Backend de TLS

Por defecto usa **rustls**. Para servidores con certificados X.509 v1 (legacy) que
rustls rechaza, compila con **native-tls (OpenSSL)**:

```bash
cargo build -p molde-cli --no-default-features --features tls-native-tls
```

## Comandos

Recorrido rápido a continuación; la **referencia completa** — cada comando y flag —
está en [`docs/cli.md`](docs/cli.md) (o ejecuta `molde <command> --help`).

```bash
# 0. Database lifecycle: create / drop / reset the database itself.
molde db create --connection "$DATABASE_URL"
molde db reset  --connection "$DATABASE_URL"   # drop + recreate + apply all migrations
molde db drop   --connection "$DATABASE_URL"

# 1. Database-first: introspect an existing DB into .model files.
molde pull --connection "$DATABASE_URL" --out models

# 2. Model-first: create a migration from the .model files (diff vs snapshot).
molde migrate InitialCreate                  # reads models/, writes migrations/
molde status                                 # list migrations
molde undo                                   # remove the latest migration
molde lint                                   # CI: flag destructive/risky migrations (no DB)
molde ci                                     # PR gate: lint + snapshot + optional verify → Markdown report

# 3. Apply / roll back migrations against the DB.
molde apply --connection "$DATABASE_URL"
molde apply --connection "$DATABASE_URL" --to 0               # roll back everything
molde apply --connection "$DATABASE_URL" --to InitialCreate

# 4. Snapshot, drift check, sync, daily catch-up, rebuild.
molde snapshot                               # regenerate snapshot.json from models/
molde snapshot --check                       # CI: fail if the snapshot is stale
molde verify --connection "$DATABASE_URL" --check        # fail if the DB drifts from the model
molde sync --source "$TRUNK_DB" --target "$DATABASE_URL"  # additive live DB → DB
molde up --connection "$DATABASE_URL"        # apply pending migrations + drift report
molde fresh --connection "$DATABASE_URL"     # roll back all + re-apply (rebuild)
```

Para un equipo grande donde cada quien corre su propia base de datos local, consulta
[`docs/team-database-workflow.md`](docs/team-database-workflow.md): los archivos
`.model` en git son la fuente de verdad, `molde init-team` instala un driver de merge
de snapshot para que las migraciones concurrentes no entren en conflicto, y
`molde sync`/`up` mantienen actualizada cada DB local.

Ejecuta un comando sin argumentos y te pedirá lo que falte (nombre de migración,
connection); `apply` confirma antes de tocar la base de datos. Usa `--yes` para
saltar la confirmación y `--no-input` para CI (sin prompts). El provider se infiere de
la URL (`postgres://`, `mysql://`, `sqlite://`) o se define con `--provider`. SQL
Server usa una connection string ADO:

```bash
molde apply --provider sqlserver \
  --connection "Server=host,1433;Database=db;User Id=sa;Password=***;TrustServerCertificate=true;Encrypt=true"
```

### Estructura del proyecto

```
models/                 # source of truth (one entity per .model)
  database.model        # globals: schema, extensions, functions, raw
  Customer.model
  Order.model
migrations/             # versioned in git
  snapshot.json         # previous state (managed by molde)
  20260607_*.json       # migrations (IR operations; SQL is rendered on apply)
```

## Instalación

`molde` es un binario único autocontenido — sin runtime, sin necesidad de Rust. El
one-liner descarga el binario precompilado para tu plataforma y lo agrega a tu `PATH`:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/MAWESI-SAS/molde/main/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/MAWESI-SAS/molde/main/install.ps1 | iex
```

Luego `molde update` lo mantiene actualizado. ¿Prefieres compilar desde el código
fuente? Con [rustup](https://rustup.rs): `cargo install --path crates/molde-cli`.
Pasos por sistema operativo, descargas manuales, y el build de TLS legacy están en
[docs/install.md](docs/install.md).

## Desarrollo

Workspace estándar de Rust (`cargo build` / `cargo test`). El
[`.devcontainer/`](.devcontainer/) incluye Rust + una PostgreSQL local; abre el
repositorio en VS Code y elige *"Reopen in Container"*.

```bash
cargo build && cargo test
cargo clippy --workspace --all-targets
```

## Contribuciones

¡Las contribuciones son bienvenidas! Por favor lee [CONTRIBUTING.md](CONTRIBUTING.md)
para saber cómo configurar un entorno de desarrollo y las verificaciones que ejecuta
CI, y ten en cuenta nuestro [Code of Conduct](CODE_OF_CONDUCT.md). Problemas de
seguridad: consulta [SECURITY.md](SECURITY.md).

## Licencia

Licenciado bajo cualquiera de las siguientes opciones

- Licencia MIT ([LICENSE-MIT](LICENSE-MIT) o http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) o
  http://www.apache.org/licenses/LICENSE-2.0)

a tu elección.

A menos que declares explícitamente lo contrario, cualquier contribución enviada
intencionalmente para su inclusión en el trabajo por ti, según se define en la
licencia Apache-2.0, quedará licenciada de forma dual como se indica arriba, sin
términos ni condiciones adicionales.
