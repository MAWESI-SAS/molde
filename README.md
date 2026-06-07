# efrust

Un CLI en **Rust** que replica las funcionalidades de **Entity Framework Core**
(scaffolding, migraciones y aplicación de migraciones) para proyectos
**.NET Core 9** con modelos `.cs` y `ApplicationDbContext`.

> **Objetivo de compatibilidad:** *equivalente en resultados*, no drop-in. efrust
> gestiona tus modelos C# y el esquema de BD logrando lo mismo que `dotnet ef`,
> con su propio formato de migración y snapshot. No persigue ser bug-for-bug
> compatible con los archivos internos de EF.

## Arquitectura

```
efrust (CLI, Rust)
├── efrust-core         IR del modelo + snapshot + diff + migraciones (agnóstico)
├── efrust-providers    SqlGenerator por motor (SQLite, Postgres, MySQL, SQL Server)
├── efrust-migrate      apply de migraciones (Backend: sqlx Any + tiberius/TDS)
├── efrust-scaffold     lectura de esquema (BD → IR) + codegen C#
├── efrust-design       orquestación model-first (invoca el sidecar + autor de migraciones)
└── sidecar (.NET)      lee el DbContext del usuario → emite Model IR (JSON)
```

### Matriz de capacidades por motor

| Capacidad | PostgreSQL | MySQL | SQLite | SQL Server |
|---|:--:|:--:|:--:|:--:|
| `database update` (apply) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| `migrations add/remove/list` | ✅ | ✅ | ✅ | ✅ |
| `scaffold` (BD → C#) | ✅ | ✅ | ✅ | ✅ (tiberius) |
| Búsqueda / full-text en scaffold + round-trip | ✅ pgvector+tsvector+triggers | ✅ FULLTEXT+generated | — | ✅ computed PERSISTED · FTS best-effort |
| Paridad verificada con `dotnet ef` | ✅ | — | — | — |

> SQLite: las FKs se declaran inline en `CREATE TABLE`; el cambio de tipo de
> columna y el alta/baja de FK sobre tablas existentes se aplican con
> reconstrucción de tabla (create-new/copy/drop/rename), estilo EF.
> SQL Server usa el driver TDS `tiberius`; el resto usa `sqlx` (`Any`).
> **PostgreSQL — objetos de búsqueda:** el scaffold detecta y preserva columnas
> `vector(N)` (pgvector → `Pgvector.Vector`) y `tsvector` (→ `NpgsqlTsVector`,
> incl. columnas generadas `STORED`), índices con método/operator class
> (`HasMethod`/`HasOperators`: GIN, GiST, HNSW, IVFFlat) e índices parciales.
> Lo que EF Core no modela (funciones, triggers e índices por expresión) se
> exporta a `<Context>.DbObjects.sql` y se recrea en `database update`
> (round-trip completo verificado contra Postgres + pgvector).

La **única** pieza no-Rust es el sidecar .NET, y solo para *leer* el modelo:
deja que EF Core resuelva Fluent API + data annotations + convenciones, y emite
el resultado como JSON. Todo lo demás (diff, generación de SQL, aplicación,
scaffolding) es Rust puro.

Ver [`docs/model-ir.md`](docs/model-ir.md) para el contrato central.

## Estado: completo ✅ — 4 motores (apply + scaffold + migraciones)

| Fase | Alcance | Estado |
|---|---|---|
| **0** | Estructura del workspace + Model IR + diff base + 2 providers | ✅ |
| **1** | `database update` (aplicar/revertir migraciones + `__EFMigrationsHistory`) | ✅ verificado en SQLite |
| **2** | `scaffold` (BD → C#) para SQLite + Postgres | ✅ verificado en SQLite |
| **3** | Sidecar .NET → JSON → Model IR | ✅ verificado (contrato C#↔Rust) |
| **4** | `migrations add` (sidecar → diff vs snapshot → migración) | ✅ verificado e2e (model-first) |
| **5** | FK/índices en DDL + orden de dependencias + `migrations remove` con replay | ✅ verificado e2e |
| **5b** | Navegaciones en scaffold + provider **MySQL** (gen + reader + apply) | ✅ verificado vs MySQL real |
| **5c** | Normalización de nombres + FK inline SQLite + T-SQL de SQL Server | ✅ |
| **6** | Suite de paridad contra `dotnet ef` | ✅ **esquemas idénticos** |
| **7** | Apply de **SQL Server** vía `tiberius` (driver TDS) | ✅ verificado vs SQL Server real |
| **8** | **Scaffold de SQL Server** vía `tiberius` (cierra la matriz) | ✅ verificado vs SQL Server real |
| **9** | **Búsqueda en PostgreSQL**: pgvector + full-text (tsvector) + triggers/funciones en scaffold y round-trip | ✅ verificado vs Postgres + pgvector |
| **10** | Auto `CREATE EXTENSION` para pgvector en apply | ✅ verificado vs pgvector |
| **11** | Singularización de nombres en scaffold (`documents` → `Document`) | ✅ |
| **12** | **Búsqueda en MySQL**: índices FULLTEXT + columnas generadas en scaffold y round-trip | ✅ verificado vs MySQL 8 |
| **13** | **SQL Server**: columnas computadas `PERSISTED` (round-trip) + full-text best-effort (`raw_objects`/`RawSql`) | ✅ computed vs SQL Server 2022; FTS code-complete |
| **14** | **SQLite table-rebuild**: `ALTER COLUMN` y alta/baja de FK sobre tablas existentes vía reconstrucción | ✅ verificado (preserva datos) |
| **15** | Sidecar empaquetable como **`dotnet tool`** (`EFRUST_SIDECAR_CMD`) | ✅ pack + install + ejecución verificados |
| **16a** | **Herencia TPH**: el sidecar funde base+derivados en una sola tabla (columnas unidas + discriminador) en vez de duplicar | ✅ verificado vs EF real (1 tabla `Payment`) |
| **16b** | **Seed data (`HasData`)**: sidecar→IR→`INSERT`/`UPDATE`/`DELETE` + `HasData` en scaffold | ✅ extracción verificada vs EF real; diff/SQL unit |
| **16c** | **Owned types** (`OwnsOne`): se embeben como columnas en la tabla del owner (vía la fusión por tabla de 16a) | ✅ `Contact_Phone` en Customer |
| **16d** | **Value converters**: el `store_type` ya refleja el tipo convertido (enum→string) | ✅ `Status` → `varchar(20)` |

> **Notas Fase 5c:**
> - **Normalización de nombres**: el scaffold convierte `snake_case` → `PascalCase`
>   (`customer_order` → `CustomerOrder`, `created_at` → `CreatedAt`) y emite
>   `ToTable` / `HasColumnName` para mapear de vuelta a los nombres de BD.
> - **FK inline en SQLite**: las FKs se declaran dentro de `CREATE TABLE` (SQLite no
>   soporta `ALTER ADD FK`), así que las tablas nuevas sí obtienen integridad
>   referencial. Añadir una FK a una tabla existente sigue requiriendo el patrón
>   *table-rebuild* (pendiente).
> - **SQL Server**: generación de **T-SQL** (`[ ]`, `IDENTITY`, `nvarchar`,
>   `datetime2`…), **apply** y **scaffold** vía `tiberius` (driver TDS, Fases 7–8).

### Paridad con `dotnet ef` (Fase 6)

`scripts/parity-postgres.sh` aplica el **mismo modelo** (`examples/SampleModel`) con
`dotnet ef` y con `efrust` a dos bases Postgres distintas y compara los esquemas
resultantes (`information_schema`). Resultado verificado: **esquemas idénticos**
(tablas, columnas con tipos/nullability/identidad, PKs, FKs e índices).

```bash
PGHOST=<host> bash scripts/parity-postgres.sh   # requiere dotnet-ef + binario efrust + Postgres
```

Esto es posible porque el modelo que usa efrust proviene del propio modelo de EF
Core (vía el sidecar), de modo que el DDL generado es equivalente.

### SQL Server vía tiberius (Fase 7)

El motor de migraciones abstrae la ejecución en un `Backend`: **sqlx `Any`** para
Postgres/SQLite/MySQL y **tiberius** (driver TDS) para SQL Server. El apply de
SQL Server usa cadena de conexión ADO:

```bash
efrust database update --provider sqlserver \
  --connection "Server=host,1433;Database=db;User Id=sa;Password=***;TrustServerCertificate=true;Encrypt=true" \
  --migrations-dir Migrations
```

Verificado contra SQL Server 2022 real: `database update` crea tablas, FKs,
índices e historial; y `scaffold` lee el esquema de vuelta (vía `sys.*` /
`INFORMATION_SCHEMA`) generando los modelos C# con navegaciones.

> **Notas Fase 5:**
> - El diff emite `AddForeignKey`/`DropForeignKey`/`CreateIndex`/`DropIndex` con
>   orden seguro (crear tablas → FKs → índices; eliminar en orden inverso).
> - **Postgres** soporta FKs e índices completos. **SQLite** soporta índices; las
>   FKs vía `ALTER` no existen en SQLite, así que se **omiten con aviso** (las
>   tablas e índices se crean igual).
> - `migrations remove` regenera el snapshot reproduciendo las migraciones
>   restantes (`apply_operation`).
>
> **Notas Fase 5b:**
> - **Scaffold genera navegaciones**: propiedad de referencia (`HasOne`) en el lado
>   dependiente, colección (`WithMany`) en el principal, y la config de relación
>   con `HasForeignKey`/`OnDelete`/`HasConstraintName` en `OnModelCreating`.
> - **Provider MySQL** completo: generación de DDL (backticks, `AUTO_INCREMENT`,
>   FKs, índices), lectura de esquema (`information_schema`) y aplicación.
> - SQL Server se cerró en las Fases 7–8 (`tiberius`). Queda como mejora opcional el
>   patrón *table-rebuild* de SQLite (añadir FK / `ALTER COLUMN` en tablas existentes).

### Sidecar (`sidecar/EfRust.Sidecar`)

Es la única pieza no-Rust. Carga el assembly compilado del proyecto del usuario,
instancia su `DbContext` (vía `IDesignTimeDbContextFactory<>` o constructor sin
parámetros), deja que EF Core resuelva el modelo design-time y lo emite como JSON
con la forma exacta del Model IR.

```bash
efrust-sidecar --assembly <ruta.dll> [--context <NombreDbContext>]
```

Se puede usar compilado (`dotnet efrust-sidecar.dll …`) o instalado como
**`dotnet tool`**:

```bash
dotnet pack sidecar/EfRust.Sidecar -c Release -o ./nupkg
dotnet tool install --global --add-source ./nupkg EfRust.Sidecar
# y apuntar efrust al tool:
export EFRUST_SIDECAR_CMD=efrust-sidecar
```

efrust elige el modo así: si `EFRUST_SIDECAR_CMD` está definido lo usa (tool);
si no, ejecuta `dotnet <ruta-al-dll>` (ver `crates/efrust-design/src/sidecar.rs`).

El contrato C#↔Rust se verifica en `crates/efrust-core/tests/sidecar_contract.rs`
(deserializa salida real del sidecar). Ver `examples/SampleModel/` para un
proyecto de ejemplo con configuración Fluent API.

> **Nota:** construir el modelo no abre conexión a la BD, pero el sidecar carga el
> provider EF del proyecto. Providers con librería nativa (p. ej. SQLite/`e_sqlite3`)
> requieren que el binario nativo esté junto al assembly; los 100% gestionados
> (Npgsql, SqlServer) funcionan sin pasos extra.

> **Limitación conocida de scaffold (Fase 2):** se generan las columnas escalares,
> claves, facetas (`HasMaxLength`) e índices. Las **propiedades de navegación**
> (FKs como objetos `HasOne/WithMany`) y la normalización de nombres
> (snake_case → PascalCase) quedan para una fase posterior.

## Comandos

```bash
# Aplicar todas las migraciones pendientes (DATABASE_URL se toma del entorno):
efrust database update --migrations-dir examples/Migrations

# Revertir todo, o ir a una migración concreta:
efrust database update --migrations-dir examples/Migrations --target 0
efrust database update --migrations-dir examples/Migrations --target InitialCreate

# Generar modelos C# + DbContext desde una BD existente (database-first):
efrust scaffold --output-dir Models --namespace App.Data --context AppDbContext

# Crear una migración desde el modelo C# (model-first, vía sidecar):
export EFRUST_SIDECAR=path/to/efrust-sidecar.dll
efrust migrations add InitialCreate --assembly path/to/MiProyecto.dll
efrust migrations list
```

### Formato de migración

Cada migración es un JSON con las operaciones (`up`/`down`) del IR; el SQL se
renderiza al aplicar según el provider. Ver
[`examples/Migrations/`](examples/Migrations/) para un ejemplo y `docs/model-ir.md`
para el detalle.

## Desarrollo

### Devcontainer (recomendado)

El entorno completo (Rust stable + .NET 9 SDK + PostgreSQL local) está definido en
[`.devcontainer/`](.devcontainer/). Es la forma recomendada: no requiere instalar
ningún toolchain en la máquina anfitriona, solo Docker.

```
.devcontainer/
├── devcontainer.json    config (compose + extensiones + post-create)
├── docker-compose.yml   servicio `app` (Rust+.NET) + `db` (Postgres 17 desechable)
├── Dockerfile           base .NET 9 + toolchain Rust + deps nativas de sqlx
└── post-create.sh       instala dotnet-ef, cargo fetch, dotnet restore
```

**Para arrancar:** abre el repo en VS Code y elige *"Reopen in Container"* (o
`devcontainer up` con la CLI). Tras el build, el contenedor trae:

- `rustc` + `cargo` + `clippy` + `rustfmt` + `sqlx-cli`
- `dotnet` 9 + `dotnet-ef` 9.x (CLI oficial, para comparar paridad)
- PostgreSQL 17 en el servicio `db`, listo y con healthcheck

Variables de conexión disponibles dentro del contenedor:

| Variable | Formato | Consumidor |
|---|---|---|
| `DATABASE_URL` | `postgres://efrust:efrust@db:5432/efrust_dev` | CLI en Rust (sqlx) |
| `DATABASE_CONNECTION_STRING` | `Host=db;Port=5432;Database=efrust_dev;...` | sidecar / `dotnet-ef` |

```bash
cargo build && cargo test                       # tests del IR/diff y providers
dotnet run --project sidecar/EfRust.Sidecar     # emite el Model IR de ejemplo (Fase 0)
psql "$DATABASE_URL"                            # Postgres local
```

> SQLite no necesita servicio (es basado en archivo). SQL Server y MySQL se
> añadirán al compose en la Fase 5.

### Instalación manual (sin devcontainer)

```bash
# Rust (CLI + core + providers)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build
cargo test

# .NET 9 (sidecar) — https://dotnet.microsoft.com/download/dotnet/9.0
dotnet build sidecar/EfRust.Sidecar
dotnet run --project sidecar/EfRust.Sidecar
```

## Licencia

MIT
