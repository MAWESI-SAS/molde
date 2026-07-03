# Cómo se rastrean las migraciones

Esto explica cómo molde sabe qué migraciones ya se ejecutaron, cómo las aplica
y las revierte, y cómo eso interopera con Entity Framework. Para conocer los
flags de los comandos en sí, consulta la [referencia de la CLI](cli.md).

## La tabla de historial: `__EFMigrationsHistory`

molde registra las migraciones aplicadas en una tabla **dentro de la base de
datos que gestiona**, llamada `__EFMigrationsHistory` — el mismo nombre que
usa Entity Framework Core, a propósito (ver [interoperabilidad con EF](#ef-interoperability)).

| Column | Type | Purpose |
|---|---|---|
| `MigrationId` | `varchar(150)`, **primary key** | el id de la migración, p. ej. `20260608120000_InitialCreate` |
| `ProductVersion` | `varchar(32)` | la versión de molde que la aplicó |

Se crea automáticamente la primera vez que aplicas algo, con
`CREATE TABLE IF NOT EXISTS` (SQL Server usa una guarda equivalente
`IF OBJECT_ID … IS NULL`). Nunca la creas ni la editas manualmente.

## Cómo se calcula lo "pendiente"

Cuando ejecutas `molde apply` (o `up` / `fresh` / `db reset`), molde:

1. Se asegura de que la tabla de historial exista.
2. Lee el conjunto de ids aplicados: `SELECT MigrationId FROM __EFMigrationsHistory
   ORDER BY MigrationId`.
3. Lista los archivos de migración en `migrations/`, ordenados por id.
4. **Pendiente = archivos cuyo id no está en la tabla de historial.** Esos se
   aplican, en orden de id.

Debido a que los ids de migración se ordenan lexicográficamente *y* molde
garantiza que cada id nuevo se ordena estrictamente después del anterior, el
orden de id también es el orden cronológico — así que "qué está aplicado vs.
pendiente" siempre se calcula contra un ordenamiento estable y correcto.

## Aplicar y revertir son atómicos junto con el registro

El cambio de esquema de cada migración y su fila de historial viven y mueren juntos:

- **Aplicar** ejecuta el DDL `up` de la migración **y** un
  `INSERT INTO __EFMigrationsHistory (MigrationId, ProductVersion) VALUES (…)`
  en una **única transacción**.
- **Rollback** (`apply --to <id>`, o `apply --to 0` para revertir todo) ejecuta
  el DDL `down` de la migración **y** un
  `DELETE FROM __EFMigrationsHistory WHERE MigrationId = …`, también en una
  transacción.

Si una migración falla a mitad de camino, la transacción revierte tanto el
cambio de esquema como la fila de historial — así que nunca obtienes un
esquema aplicado a medias ni una entrada de historial "fantasma". molde aplica
las migraciones de una en una, cada una en su propia transacción.

### Advertencia de atomicidad según el motor

PostgreSQL y SQLite admiten **DDL transaccional**, así que la garantía de todo
o nada anterior se cumple por completo. **MySQL hace auto-commit de cada
sentencia DDL** (commit implícito en `CREATE`/`ALTER`/etc.), así que una
migración que contiene varias sentencias DDL no es completamente atómica en
MySQL — una falla a mitad de la migración puede dejar algunas sentencias
confirmadas. Esta es una limitación de MySQL, no específica de molde;
mantén las migraciones pequeñas y prefiere ejecutarlas donde puedas
recuperarte (por ejemplo, un backup o `db reset` en desarrollo).

## Interoperabilidad con EF

Debido a que el nombre y la forma de la tabla coinciden con
`__EFMigrationsHistory` de EF Core, molde puede tomar el control de una base
de datos que antes gestionaba EF, y el historial de migraciones se lee de la
misma manera. Ten en cuenta que el **formato de id** difiere en esencia: los
ids de molde son `<timestamp>_<Name>` igual que los de EF, así que las filas
de historial de EF existentes se reconocen; las migraciones nuevas que crees
con molde se registran con el `ProductVersion` de molde.

## Inspeccionándolo tú mismo

El historial es solo una tabla — puedes consultarla directamente:

```sql
SELECT "MigrationId", "ProductVersion"
FROM "__EFMigrationsHistory"
ORDER BY "MigrationId";
```

(Entrecomilla los identificadores de la forma que tu motor espera.) Del lado
de molde, `molde status` lista las migraciones conocidas en disco, y
`molde verify` verifica el esquema en vivo contra tus modelos.
