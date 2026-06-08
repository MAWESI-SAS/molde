# molde

Herramienta de **esquema de base de datos en Rust** construida sobre **molde**, un
lenguaje de modelos propio, declarativo y legible (estilo TOON/YAML). molde hace
**scaffold** (BD → modelos), genera **migraciones** (modelo → diff) y las
**aplica** sobre 4 motores. Sin .NET, sin C#: todo en Rust.

> molde gestiona el **esquema** (modelos, migraciones, aplicación). El acceso a
> datos en runtime (consultas de tu aplicación) queda fuera de su alcance.

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
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
```

Especificación completa: [`docs/molde-language-spec.md`](docs/molde-language-spec.md).

## Flujo

```
BD ──scaffold──▶ models/*.model
models/*.model ──migrations add──▶ migrations/*.json (diff contra snapshot)
migrations/*.json ──database update──▶ BD
```

Todo pivota sobre un **IR** común (`molde_core::DatabaseModel`): el lenguaje es su
forma textual, los readers lo producen desde la BD y el diff genera el SQL.

## Arquitectura

```
molde (CLI, Rust)
├── molde-core         IR del modelo + snapshot + diff + migraciones (agnóstico)
├── molde-lang         lenguaje molde: parser/emitter (.model ↔ IR)
├── molde-providers    SqlGenerator por motor (SQLite, Postgres, MySQL, SQL Server)
├── molde-migrate      apply de migraciones (Backend: sqlx Any + tiberius/TDS)
├── molde-scaffold     lectura de esquema (BD → IR) + emisión de .model
└── molde-design       autoría de migraciones (diff contra snapshot)
```

### Matriz de capacidades por motor

| Capacidad | PostgreSQL | MySQL | SQLite | SQL Server |
|---|:--:|:--:|:--:|:--:|
| `database update` (apply) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `migrations add/remove/list` | ✅ | ✅ | ✅ | ✅ |
| `scaffold` (BD → `.model`) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| Búsqueda / full-text (scaffold + round-trip) | ✅ pgvector+tsvector+triggers | ✅ FULLTEXT+generated | — | ✅ computed PERSISTED · FTS best-effort |

> SQLite: las FKs se declaran inline en `CREATE TABLE`; el cambio de tipo de
> columna y el alta/baja de FK sobre tablas existentes se aplican con
> reconstrucción de tabla (create-new/copy/drop/rename), estilo EF.
> SQL Server usa el driver TDS `tiberius`; el resto usa `sqlx` (`Any`).
> **PostgreSQL:** el scaffold preserva `vector(N)` (pgvector), `tsvector` (incl.
> columnas generadas `STORED`), índices con método/operator class (GIN, GiST,
> HNSW, IVFFlat) e índices parciales. Funciones, triggers e índices por expresión
> se preservan en `.model` (bloques `triggers:`/`functions:`/`indexes:` y `raw:`).
> Los tipos no convencionales (`jsonb`, arrays, `citext`, `vector`, `tsvector`…)
> se conservan con `dbtype=`.

### Backend TLS

Por defecto **rustls**. Para servidores con certificados X.509 v1 (legacy) que
rustls rechaza, compilar con **native-tls (OpenSSL)**:

```bash
cargo build -p molde-cli --no-default-features --features tls-native-tls
```

## Comandos

```bash
# 1. Database-first: generar los .model desde una BD existente.
molde scaffold --connection "$DATABASE_URL" --output-dir models

# 2. Model-first: crear una migración a partir de los .model (diff vs snapshot).
molde migrations add InitialCreate           # lee models/, escribe migrations/
molde migrations list
molde migrations remove

# 3. Aplicar / revertir migraciones contra la BD.
molde database update --connection "$DATABASE_URL"
molde database update --connection "$DATABASE_URL" --target 0            # revertir todo
molde database update --connection "$DATABASE_URL" --target InitialCreate
```

El provider se infiere de la URL (`postgres://`, `mysql://`, `sqlite://`) o se
indica con `--provider`. SQL Server usa cadena ADO:

```bash
molde database update --provider sqlserver \
  --connection "Server=host,1433;Database=db;User Id=sa;Password=***;TrustServerCertificate=true;Encrypt=true"
```

### Disposición del proyecto

```
models/                 # fuente de verdad (una entidad por .model)
  database.model        # globales: schema, extensiones, funciones, raw
  Customer.model
  Order.model
migrations/             # versionado en git
  snapshot.json         # estado previo (gestionado por molde)
  20260607_*.json       # migraciones (operaciones del IR; el SQL se renderiza al aplicar)
```

## Desarrollo

Workspace de Rust estándar (`cargo build` / `cargo test`). El
[`.devcontainer/`](.devcontainer/) trae Rust + PostgreSQL local; abre el repo en
VS Code y elige *"Reopen in Container"*.

```bash
cargo build && cargo test
cargo clippy --workspace --all-targets
```

## Licencia

MIT
