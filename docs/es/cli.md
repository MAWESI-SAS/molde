# Referencia de la CLI de molde

Referencia completa de cada comando y opción de `molde`. Ejecuta `molde <command>
--help` para obtener la misma información desde el propio binario.

```
molde [OPTIONS] <COMMAND>
```

## Contenido

- [Opciones globales y convenciones](#global-options--conventions)
- Database-first: [`pull`](#molde-pull)
- Migraciones: [`migrate`](#molde-migrate) · [`status`](#molde-status) ·
  [`undo`](#molde-undo) · [`snapshot`](#molde-snapshot) · [`lint`](#molde-lint)
- Compuerta de CI: [`ci`](#molde-ci)
- Apply: [`apply`](#molde-apply) · [`db`](#molde-db)
- Drift y sincronización: [`verify`](#molde-verify) · [`sync`](#molde-sync) ·
  [`up`](#molde-up) · [`fresh`](#molde-fresh)
- Autoría y equipos: [`fmt`](#molde-fmt) · [`init-team`](#molde-init-team)
- Mantenimiento: [`update`](#molde-update)

## Opciones globales y convenciones

Estas aplican a todos los comandos:

| Opción | Descripción |
|---|---|
| `-v, --verbose...` | Aumenta el nivel de detalle del log. Repite para más: `-v`, `-vv`. |
| `-h, --help` | Imprime la ayuda del comando. |
| `-V, --version` | Imprime la versión de molde (solo en el comando raíz). |

Convenciones compartidas por los comandos que interactúan con una base de datos:

- **`-c, --connection <CONNECTION>`** — cadena de conexión. Por defecto usa la
  variable de entorno `DATABASE_URL`; si falta, se te pide (a menos que uses
  `--no-input`). El proveedor se infiere del esquema de la URL (`postgres://`,
  `mysql://`, `sqlite://`). SQL Server usa una cadena ADO y por lo general necesita
  `--provider sqlserver`.
- **`--provider <PROVIDER>`** — fuerza el motor: `sqlite` | `postgres` |
  `mysql` | `sqlserver`. Solo se necesita cuando no se puede inferir de la URL.
- **`--no-input`** — nunca pregunta; falla en su lugar si falta algún dato requerido.
  Úsalo en CI.
- **`-y, --yes`** — omite el prompt de confirmación antes de una acción destructiva o
  que toque la base de datos.

---

## `molde pull`

Hace introspección de una base de datos existente hacia archivos `.model` (database-first).

```
molde pull [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `-c, --connection <CONNECTION>` | Base de datos de origen. Por defecto `DATABASE_URL`; se pregunta si falta. |
| `--provider <PROVIDER>` | Motor; se infiere de la URL si se omite. |
| `--schema <SCHEMA>` | Esquema a leer (solo PostgreSQL). Por defecto `public`. |
| `-o, --out <OUT>` | Directorio de salida para los archivos `.model`. Por defecto: `models`. |
| `--force` | Sobrescribe los archivos existentes en el directorio de salida. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

```bash
molde pull --connection "postgres://user:pass@localhost/app" --out models
```

---

## `molde migrate`

Crea una migración a partir del diff entre los modelos y el snapshot.

```
molde migrate [OPTIONS] [NAME]
```

| Argumento | Descripción |
|---|---|
| `[NAME]` | Nombre de la migración (p. ej. `AddInvoices`). Si se omite, se pregunta. |

| Opción | Descripción |
|---|---|
| `--from-models <FROM_MODELS>` | Directorio con los archivos fuente `.model`. Por defecto: `models`. |
| `--output-dir <OUTPUT_DIR>` | Directorio donde se guardan las migraciones. Por defecto: `migrations`. |
| `--snapshot <SNAPSHOT>` | Ruta del snapshot. Por defecto `<output-dir>/snapshot.json`. |
| `--no-input` | No pregunta (CI); falla si falta el nombre. |

```bash
molde migrate InitialCreate
```

El id de la migración es `<UTC timestamp>_<Name>`; molde garantiza que ordene después
de cada migración existente, de modo que dos creadas en el mismo segundo nunca empaten.

---

## `molde status`

Lista las migraciones conocidas.

```
molde status [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--output-dir <OUTPUT_DIR>` | Directorio de migraciones. Por defecto: `migrations`. |

---

## `molde undo`

Elimina la última migración y regenera el snapshot a partir del resto.

```
molde undo [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--output-dir <OUTPUT_DIR>` | Directorio de migraciones. Por defecto: `migrations`. |
| `--snapshot <SNAPSHOT>` | Ruta del snapshot. Por defecto `<output-dir>/snapshot.json`. |

---

## `molde snapshot`

Regenera (o, con `--check`, verifica) el snapshot de migraciones a partir de los modelos.

```
molde snapshot [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--from-models <FROM_MODELS>` | Directorio con los archivos fuente `.model`. Por defecto: `models`. |
| `-o, --output <OUTPUT>` | Dónde escribir el snapshot. Por defecto `migrations/snapshot.json`. El merge driver de git pasa aquí el archivo en conflicto (`%A`). |
| `--check` | No escribe; sale con código distinto de cero si el snapshot en disco está desactualizado. Compuerta de CI. |

```bash
molde snapshot --check    # fail the build if migrations/snapshot.json is out of date
```

---

## `molde lint`

Revisa estáticamente las migraciones en busca de cambios riesgosos o destructivos — sin
acceso a la base de datos, pensado para CI en un pull request. Los cambios
**destructivos** (eliminar una tabla/columna) hacen fallar el comando; los cambios
dependientes de datos son **advertencias**.

```
molde lint [OPTIONS] [FILE]...
```

| Argumento | Descripción |
|---|---|
| `[FILE]...` | Archivo(s) de migración específico(s) a lintear (p. ej. solo los que agrega tu PR). Cuando se especifica, se omiten `--all`/`--since` y el escaneo del directorio. |

| Opción | Descripción |
|---|---|
| `--migrations-dir <MIGRATIONS_DIR>` | Directorio de migraciones. Por defecto: `migrations`. |
| `--all` | Lintea todas las migraciones, no solo la más reciente. |
| `--since <ID>` | Lintea solo las migraciones más nuevas que este id (exclusivo) — p. ej. la base desde la que se ramificó tu PR. Tiene precedencia sobre `--all`. |
| `--strict` | Falla también en las advertencias (dependientes de datos / locking), no solo en las destructivas. |

Precedencia de selección: argumentos `FILE` → `--since` → `--all` → solo la última migración.

```bash
molde lint                       # the latest migration
molde lint --since 20260101000000_Base --strict
molde lint migrations/20260608_AddEmail.json
```

---

## `molde ci`

Ejecuta de una vez las verificaciones del pull request e imprime un reporte en Markdown (lint +
snapshot, más un verify opcional desde cero). Sale con código distinto de cero si falla alguna
verificación — esa es la compuerta de merge. Consulta
[Gating pull requests](ci-github-actions.md) para un workflow listo para copiar.

```
molde ci [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--from-models <FROM_MODELS>` | Directorio con los archivos `.model`. Por defecto: `models`. |
| `--migrations-dir <MIGRATIONS_DIR>` | Directorio de migraciones. Por defecto: `migrations`. |
| `--since <ID>` | Lintea solo las migraciones más nuevas que este id. Por defecto se lintean todas. |
| `--strict` | Trata también las advertencias de lint como fallas, no solo los cambios destructivos. |
| `-c, --connection <CONNECTION>` | Base de datos efímera para aplicar desde cero y verificar. Se omite para saltar la verificación. Por defecto `DATABASE_URL`. |
| `--provider <PROVIDER>` | Motor para `--connection`; se infiere de la URL si se omite. |
| `--schema <SCHEMA>` | Esquema a leer para verify (solo PostgreSQL). Por defecto `public`. |
| `--report <PATH>` | También escribe el reporte en Markdown a este archivo (para publicarlo como comentario del PR). |

```bash
molde ci                                              # lint + snapshot (no DB)
molde ci --connection "$DATABASE_URL" --report ci.md  # + from-scratch verify
molde ci --strict --since 20260101000000_Base         # PR-scoped, warnings fail too
```

---

## `molde apply`

Aplica las migraciones pendientes a la base de datos (o hace rollback con `--to`). Renderiza el
SQL para el motor de destino y registra cada migración en la tabla de historial.

```
molde apply [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `-c, --connection <CONNECTION>` | Por defecto `DATABASE_URL`; se pregunta si falta. |
| `--provider <PROVIDER>` | Motor; se infiere de la URL si se omite. |
| `--migrations-dir <MIGRATIONS_DIR>` | Directorio de migraciones a aplicar. Por defecto: `migrations`. |
| `--to <TO>` | Lleva la base de datos hasta esta migración (id o nombre). `0` hace rollback de todo. Por defecto, aplica todas las migraciones pendientes. |
| `-y, --yes` | No pide confirmación antes de tocar la base de datos. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

```bash
molde apply --connection "$DATABASE_URL"
molde apply --connection "$DATABASE_URL" --to 0              # roll back everything
molde apply --connection "$DATABASE_URL" --to InitialCreate # up/down to a point
```

molde registra las migraciones aplicadas en una tabla `__EFMigrationsHistory` y aplica
cada una (DDL + fila de historial) en una sola transacción. Consulta
[migrations.md](migrations.md) para los detalles.

---

## `molde db`

Ciclo de vida de la base de datos: crear / eliminar / reiniciar la base de datos en sí (no su esquema).

```
molde db <COMMAND> [OPTIONS]
```

| Subcomando | Descripción |
|---|---|
| `create` | Crea la base de datos si no existe. |
| `drop` | Elimina la base de datos (destructivo). |
| `reset` | Elimina, recrea y aplica todas las migraciones desde cero. |

Los tres comparten estas opciones:

| Opción | Descripción |
|---|---|
| `-c, --connection <CONNECTION>` | Por defecto `DATABASE_URL`; se pregunta si falta. |
| `--provider <PROVIDER>` | Motor; se infiere de la URL si se omite. |
| `-y, --yes` | No pide confirmación antes de una acción destructiva. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

`db reset` además admite:

| Opción | Descripción |
|---|---|
| `--migrations-dir <MIGRATIONS_DIR>` | Migraciones a aplicar después de recrear la base de datos. Por defecto: `migrations`. |

```bash
molde db create --connection "$DATABASE_URL"
molde db reset  --connection "$DATABASE_URL"   # drop + recreate + apply all
molde db drop   --connection "$DATABASE_URL" --yes
```

`drop`/`reset` son destructivos: se niegan a ejecutarse bajo `--no-input` a menos que
también se pase `--yes`.

---

## `molde verify`

Verifica si una base de datos en vivo coincide con el modelo (chequeo de drift). Esto compara
únicamente la **estructura**; las filas de datos/seed quedan fuera del alcance.

```
molde verify [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `-c, --connection <CONNECTION>` | Base de datos a verificar. Por defecto `DATABASE_URL`; se pregunta si falta. |
| `--provider <PROVIDER>` | Motor; se infiere de la URL si se omite. |
| `--schema <SCHEMA>` | Esquema a leer (solo PostgreSQL). Por defecto `public`. |
| `--from-models <FROM_MODELS>` | Directorio con los archivos `.model` del estado deseado. Por defecto: `models`. |
| `--check` | Sale con código distinto de cero si la base de datos difiere (drift) del modelo. Compuerta de CI. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

```bash
molde verify --connection "$DATABASE_URL" --check
```

---

## `molde sync`

Sincroniza de forma aditiva una base de datos destino a partir de una fuente (DB en vivo → DB en vivo). Calcula
los cambios aditivos, escribe un `.sql` y (a menos que uses `--dry-run`) los aplica.

```
molde sync [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--source <SOURCE>` | Base de datos DESDE la cual se traen los cambios (p. ej. la `test` compartida). Recurre a `MOLDE_SYNC_SOURCE`; se pregunta si falta. |
| `--target <TARGET>` | Base de datos que RECIBE los cambios (p. ej. tu base de datos local). Recurre a `MOLDE_SYNC_TARGET`; se pregunta si falta. |
| `-o, --out <OUT>` | Ruta para el `.sql` generado. Por defecto `./sync-<timestamp>.sql`. |
| `--dry-run` | Solo genera el `.sql` y el reporte; no aplica nada. |
| `-y, --yes` | Aplica sin pedir confirmación. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

```bash
molde sync --source "$TRUNK_DB" --target "$DATABASE_URL"
molde sync --source "$TRUNK_DB" --target "$DATABASE_URL" --dry-run
```

---

## `molde up`

Pone al día la base de datos local — aplica las migraciones pendientes, o avanza rápido desde una
base de datos trunk — e imprime un reporte de drift.

```
molde up [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `-c, --connection <CONNECTION>` | Base de datos local. Por defecto `DATABASE_URL`; se pregunta si falta. |
| `--provider <PROVIDER>` | Motor; se infiere de la URL si se omite. |
| `--from-trunk <FROM_TRUNK>` | Avanza rápido desde esta base de datos trunk (sincronización aditiva) en lugar de reproducir migraciones. Recurre a `MOLDE_SYNC_SOURCE`. |
| `--migrations-dir <MIGRATIONS_DIR>` | Migraciones a aplicar en modo replay. Por defecto: `migrations`. |
| `--from-models <FROM_MODELS>` | Directorio de modelos para el reporte de drift. Por defecto: `models`. |
| `--schema <SCHEMA>` | Esquema a leer (solo PostgreSQL). Por defecto `public`. |
| `-y, --yes` | Aplica/sincroniza sin pedir confirmación. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

```bash
molde up --connection "$DATABASE_URL"
```

---

## `molde fresh`

Reconstruye la base de datos local a partir de las migraciones: hace rollback de todo, luego vuelve a aplicar.

```
molde fresh [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `-c, --connection <CONNECTION>` | Base de datos local. Por defecto `DATABASE_URL`; se pregunta si falta. |
| `--provider <PROVIDER>` | Motor; se infiere de la URL si se omite. |
| `--migrations-dir <MIGRATIONS_DIR>` | Migraciones desde las cuales reconstruir. Por defecto: `migrations`. |
| `-y, --yes` | Reconstruye sin pedir confirmación. |
| `--no-input` | No pregunta (CI); falla si falta algún dato. |

```bash
molde fresh --connection "$DATABASE_URL"
```

---

## `molde fmt`

Formatea los archivos `.model` a su forma canónica (como `cargo fmt` para modelos).

```
molde fmt [OPTIONS] [PATHS]...
```

| Argumento | Descripción |
|---|---|
| `[PATHS]...` | Archivos o directorios `.model` a formatear. Por defecto `models/`. |

| Opción | Descripción |
|---|---|
| `--check` | No escribe; sale con código distinto de cero si algún archivo no está formateado. |
| `--stdin` | Lee desde stdin y escribe el resultado formateado en stdout. |
| `--stdin-name <STDIN_NAME>` | Nombre de archivo usado para inferir el tipo con `--stdin` (`database.model` = globales; cualquier otro = entidad). Por defecto: `entity.model`. |

```bash
molde fmt                 # format models/
molde fmt --check         # CI gate: fail if anything is unformatted
```

---

## `molde init-team`

Configura el merge driver del snapshot (y opcionalmente una plantilla de CI) para flujos de
trabajo en equipo, de modo que las migraciones concurrentes no entren en conflicto en `snapshot.json`. Consulta
[team-database-workflow.md](team-database-workflow.md) para la guía completa.

```
molde init-team [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--path <PATH>` | Raíz del repositorio. Por defecto: `.` (directorio actual). |
| `--ci <CI>` | También escribe una plantilla de CI para este proveedor (actualmente: `github`). |
| `--force` | Sobrescribe una plantilla de CI existente si difiere. |

```bash
molde init-team --ci github
```

---

## `molde update`

Se autoactualiza al último release de GitHub: descarga el archivo correspondiente a esta
plataforma y variante de TLS, y reemplaza atómicamente el binario en ejecución. La descarga
usa rustls contra GitHub sin importar el backend de TLS de base de datos de molde.

```
molde update [OPTIONS]
```

| Opción | Descripción |
|---|---|
| `--check` | Solo informa si existe una versión más nueva; no modifica nada. |

Necesita acceso de escritura al binario instalado (usa `sudo` si está en una ruta del sistema).

```bash
molde update          # update to the latest release
molde update --check  # report only
```
