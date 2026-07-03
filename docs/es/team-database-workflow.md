# Flujo de trabajo de base de datos en equipo — muchas bases de datos locales, sincronización fácil, pocos conflictos

> **Estado:** propuesta pendiente de aprobación.
> **Audiencia:** un equipo grande donde cada desarrollador ejecuta su propia
> base de datos local y el esquema evoluciona de forma concurrente.
> **Objetivo:** hacer que la sincronización del día a día sea trivial y
> minimizar estructuralmente los conflictos, manteniendo el esquema revisado,
> versionado y seguro para producción.

Este documento define el **proceso**. También cierra con un inventario preciso
del pequeño conjunto de funcionalidades de molde que aún faltan para
soportarlo de punta a punta (§11) — marcando claramente qué existe **hoy**
frente a qué está **propuesto**.

---

## 1. El modelo mental (lee esto primero)

El esquema vive en **dos representaciones**, y cada una tiene exactamente un
trabajo:

| Representación | Qué es | Rol |
|---|---|---|
| Archivos **`.model`** (uno por entidad) | Estado *deseado* declarativo, en git | **Fuente de verdad** |
| **`migrations/`** (con timestamp) + `snapshot.json` | Cambios ordenados y revisados derivados de los diffs del modelo | Historial determinístico, reproducible |
| **La base de datos local de un desarrollador** | Una *proyección* del modelo | **Desechable** — reconstruible en cualquier momento |

De aquí se derivan tres principios:

1. **El modelo en git es la verdad.** Una base de datos local nunca es la
   verdad; es una caché que puedes descartar y reconstruir. Esta única regla
   elimina la mayoría de los casos de drift de esquema del tipo "en mi máquina
   funciona".
2. **Git es el camino de *escritura*; `sync` es el camino de *lectura*.**
   - Para **contribuir** un cambio, abres un PR con `.model` + una migración
     generada. Se revisa como cualquier código.
   - Para **ponerte al día** avanzas tu base de datos local desde una base de
     datos canónica de "trunk" con `molde sync` (aditivo, nunca destructivo).
3. **Las bases de datos de los desarrolladores nunca se sincronizan entre sí.**
   La sincronización de base de datos entre pares (peer-to-peer) es como los
   equipos generan caos. Todas las contribuciones fluyen a través de git
   revisado; `sync` solo fluye en una dirección, **trunk → tu local**, aditivo.

```
 WRITE (reviewed)
   you ──edit .model──▶ molde migrate ──▶ PR (.model + migration) ──▶ review ──▶ merge to main
                                                                                   │
                                                                                   ▼
                                                                       CI builds/updates the
                                                                       canonical "trunk" DB
                                                                                   │
 READ (fast, additive)                                                             ▼
   your local DB  ◄────────────── molde sync (additive, conflicts reported) ◄──── trunk DB
```

---

## 2. Por qué esto minimiza los conflictos

El clásico dolor de las migraciones en equipo tiene tres causas raíz. molde ya
neutraliza dos de ellas; este proceso resuelve la tercera.

| Falla clásica | Cómo se evita aquí |
|---|---|
| **Los IDs secuenciales de migración colisionan** (dos devs crean ambos `0042_*`) | molde ya usa **IDs con timestamp** `yyyyMMddHHmmss_name` — dos devs nunca colisionan, y el orden es inequívoco. ✅ *existe hoy* |
| **Un snapshot de modelo generado y gigante entra en conflicto en cada merge** (el `ModelSnapshot.cs` de EF) | `snapshot.json` es la **serialización normalizada del modelo**, por lo que es *derivable*: ante un conflicto lo regeneras a partir de los archivos `.model` fusionados en lugar de fusionarlo a mano (§5.2). |
| **Traer los cambios de tus compañeros arrasa con los experimentos locales** | `molde sync` es **aditivo y reporta conflictos** — nunca elimina ni altera objetos que solo existen en el destino, así que tu WIP local sobrevive. ✅ *existe hoy* |
| **`.model` en sí mismo es difícil de fusionar** | `.model` es **un archivo por entidad**, texto plano — ediciones concurrentes a entidades *distintas* nunca entran en conflicto; las ediciones a la misma entidad son fusiones de texto pequeñas y legibles. ✅ *existe hoy* |

Una propiedad clave que vale la pena destacar: **`molde migrate` compara
`.model` contra `snapshot.json` — solo archivos, sin necesidad de base de
datos.** Por lo tanto, la autoría de migraciones es determinística y
revisable, independiente del estado de la base de datos local de cualquier
desarrollador.

---

## 3. Rol de cada comando en este flujo de trabajo

Todos existen hoy salvo que se marquen como *(propuesto)*.

| Comando | Rol en el flujo de trabajo |
|---|---|
| `molde pull` | DB → `.model`. Se usa **una vez** para inicializar el modelo a partir de una base de datos existente, o para reconciliar una DB editada a mano. No forma parte del ciclo diario. |
| `molde migrate -m "<name>"` | `.model` → nueva migración + `snapshot.json` actualizado. El paso de **contribución**. |
| `molde apply` | Aplica las migraciones pendientes a una base de datos (o revierte con `--to`). La forma **determinística** de llevar una DB a un estado dado. |
| `molde sync` | DB → DB en vivo, aditivo. El paso de **ponerse al día** (trunk → local). |
| `molde status` | Lista las migraciones conocidas/aplicadas. |
| `molde undo` | Elimina la última migración (local, sin merge aún) mientras iteras. |
| `molde fmt` | Canonicaliza los archivos `.model` (ejecutar antes de commitear; exigido en CI). |
| `molde snapshot` | Regenera `snapshot.json` a partir de `.model` sin generar migración (o verifica con `--check`) — impulsa el merge driver (§7.2). ✅ *existe hoy* |
| `molde verify` | Verifica si una base de datos en vivo coincide con el modelo (chequeo de drift). `--check` sale con código distinto de cero ante drift — gate de CI §9.3 y la respuesta local a "¿mi DB está sincronizada?". ✅ *existe hoy* |
| `molde init-team` | Configuración única, por clon, del merge driver de snapshot + hook `post-merge` (y plantilla `--ci github`) — §7.2. ✅ *existe hoy* |
| `molde up` | Pone al día la DB local en un solo comando: aplica las migraciones pendientes (o `--from-trunk` para sincronizar de forma aditiva), y luego reporta el drift. ✅ *existe hoy* |
| `molde fresh` | Reconstruye la DB local a partir de las migraciones (revierte todo, vuelve a aplicar) — la convención de que "reconstruir es barato" (§10). ✅ *existe hoy* |

---

## 4. Estructura del repositorio

```
models/                 # .model files — the source of truth (one per entity)
  Customer.model
  Order.model
  ...
migrations/             # generated, reviewed; immutable once merged
  20260608153012_add_invoices.sql      (or molde's migration format)
  ...
  snapshot.json         # normalized model of the last migrated state (derived)
```

Convención: `models/` lo edita un humano; `migrations/` y `snapshot.json` son
**artefactos generados que se commitean** (para que el historial sea
revisable y producción aplique exactamente el SQL revisado).

---

## 5. El ciclo diario del desarrollador

### 5.1 Ponerse al día (al comenzar el día / antes de un cambio nuevo)

```bash
git pull
molde up                         # apply pending migrations, then report drift
#   molde up --from-trunk "$TRUNK_DB"   # …or fast-forward from trunk (additive sync)
```

`molde up` es el comando único para ponerse al día. Por debajo es o bien
`molde apply` (replay) o `molde sync` desde trunk, seguido de `molde verify`.

`apply` es determinístico (similar a producción) pero reproduce cada migración
pendiente; `sync` es instantáneo (copia la estructura en vivo de trunk de
forma aditiva) y preserva tus experimentos locales. Ambos son modos válidos
para ponerse al día — ver §6.

### 5.2 Hacer un cambio de esquema

```bash
# 1. Edit the declarative model
$EDITOR models/Invoice.model

# 2. Generate a migration from the model diff (timestamped id)
molde migrate -m "add invoices"

# 3. Apply it to your local DB and test your code
molde apply

# 4. Format + commit BOTH the model and the generated migration
molde fmt
git add models/ migrations/
git commit -m "schema: add invoices"
git push   # open PR
```

El revisor ve **la intención declarativa** (el diff de `models/`) **y el SQL
exacto** (el diff de `migrations/`) en un solo PR.

### 5.3 Iterar antes de que el PR se fusione

Si necesitas revisar una migración *sin fusionar aún*, usa `molde undo` para
eliminar la última, edita el modelo, y vuelve a ejecutar `molde migrate`.
**Nunca** edites una migración que ya se fusionó a main (§7).

---

## 6. Dos modos de ponerse al día (cuál usar)

| | `molde apply` (replay) | `molde sync` (fast-forward) |
|---|---|---|
| Mecanismo | Reejecuta en orden las migraciones pendientes | Copia de forma aditiva la estructura en vivo de trunk |
| Velocidad | O(número de migraciones pendientes) | Un solo pase, instantáneo |
| Fidelidad | Exactamente lo que ejecutará producción | Superconjunto estructural; ignora el orden |
| WIP local | Intacto (las migraciones son aditivas) | **Preservado** (sync es aditivo, reporta conflictos) |
| Cuándo usarlo | Quieres precisión de producción / estás por autorar una migración | Solo quieres un esquema actualizado rápido y tienes experimentos locales que conservar |

Regla práctica: **usa `sync` para seguir programando rápido; usa `apply` (en
una DB nueva) antes de autorar o probar una migración**, para que lo que
generes coincida con lo que aplicará producción.

---

## 7. Resolución de conflictos (el corazón de esto)

Hay cuatro tipos de "conflicto", en orden creciente de rareza:

### 7.1 Conflicto de `.model` (entidades distintas) — nunca ocurre
Dos devs editando archivos `.model` distintos no entran en conflicto. Este es
el caso común y es gratis.

### 7.2 Conflicto de `snapshot.json` (la trampa de EF) — se resuelve automáticamente
Cuando dos ramas ejecutaron cada una `molde migrate`, ambas regeneraron
`snapshot.json`, así que git reporta un conflicto en ese archivo. **No lo
fusiones a mano.** Dado que el snapshot es solo la serialización normalizada
del modelo, el snapshot correcto después de un merge es *la serialización de
los archivos `.model` fusionados* — exactamente lo que escribe `molde
snapshot` (idéntico byte a byte a lo que escribiría `molde migrate`):

```bash
# regenerate from the merged model, no migration:
molde snapshot            # writes migrations/snapshot.json from models/
git add migrations/snapshot.json
```

**No hagas esto a mano — `molde init-team` lo configura** (se ejecuta una vez
por clon). Instala dos piezas que cooperan entre sí:

1. Un **merge driver** de git — `.gitattributes` enruta `snapshot.json` hacia
   él, y este ejecuta `molde snapshot --output %A` *durante* el merge para que
   este se complete sin detenerse en marcadores de conflicto:
   ```gitattributes
   # .gitattributes (committed)
   migrations/snapshot.json merge=molde-snapshot
   ```
   ```ini
   # .git/config (local, per clone — written by init-team)
   [merge "molde-snapshot"]
       driver = molde snapshot --output %A
   ```
2. Un hook **`post-merge`** — el merge driver se ejecuta a mitad del merge,
   cuando los archivos agregados del otro lado pueden no estar aún en el
   árbol de trabajo, por lo que su snapshot puede quedar desactualizado. El
   hook vuelve a derivar el snapshot una vez que el árbol se ha asentado y
   deja el arreglo en stage.

Entonces, en la práctica: el merge **se completa sin edición manual del
snapshot**; el hook deja el `snapshot.json` correcto en stage; tú lo
commiteas. `molde snapshot --check` en CI (§9.4) es el respaldo que garantiza
que nada desactualizado llegue a `main`.

> Si un archivo `.model` *también* entró en conflicto (misma entidad editada
> de dos formas, §7.3), resuelve primero ese conflicto de texto, y luego
> `molde snapshot` — el hook no puede volver a derivar un snapshot correcto a
> partir de un modelo que todavía tiene marcadores de conflicto.

### 7.3 Misma entidad `.model` editada de dos formas — una fusión de texto pequeña y normal
Resuelve el conflicto de texto de `.model` como cualquier merge de código, y
luego regenera el snapshot (§7.2). La granularidad por entidad mantiene esto
mínimo.

### 7.4 Conflicto semántico (dos migraciones hacen cosas incompatibles)
Ej.: ambas ramas agregan una columna `Invoice.Total` con tipos distintos. Este
es un conflicto de diseño *real*, no un artefacto de la herramienta. Surge ya
sea como un conflicto de merge en `.model` (§7.3) o como una falla al aplicar
las migraciones sobre una DB nueva. **CI lo detecta** aplicando todas las
migraciones sobre una base de datos vacía (§9). La solución es una decisión
humana, como debe ser.

---

## 8. La base de datos canónica de "trunk"

Una única base de datos, mantenida por CI, que siempre refleja `main`:

- En cada merge a `main`, CI aplica las nuevas migraciones a la **DB de
  trunk** (y ejecuta los gates de §9).
- Los desarrolladores ejecutan `molde sync --source "$TRUNK_DB"` para
  avanzar su DB local sin reproducir el historial (§6).
- La DB de trunk es **de solo lectura para los humanos**: nadie la edita
  directamente. Es una proyección de `main`, exactamente igual que una DB
  local es una proyección del modelo.

Esto le da al equipo una "réplica de lectura del esquema" rápida y siempre
actualizada, sin convertir a ninguna base de datos en la fuente de verdad.

---

## 9. Gates de CI (qué protege a `main`)

En cada PR:

1. **`molde fmt --check`** — los modelos son canónicos.
2. **Apply sobre DB nueva** — levanta una DB vacía, `molde apply` *todas* las
   migraciones desde cero. Detecta conflictos de orden y semánticos (§7.4).
3. **Consistencia modelo ⇄ migraciones** — aplica todas las migraciones sobre
   una DB nueva, y luego `molde verify --check` contra ella: debe reportar
   **sin drift**, es decir, que la base de datos migrada es igual al modelo.
   ✅ *existe hoy*
4. **Consistencia del snapshot** — `molde snapshot --check` verifica que
   `snapshot.json` sea igual a la serialización de `models/` (sin snapshot
   desactualizado), saliendo con código distinto de cero si hay drift. ✅
   *existe hoy*
5. **Seguridad de migraciones** — `molde lint` marca de forma estática los
   cambios destructivos (drop de tabla/columna) y los riesgosos (NOT NULL sin
   default, índice único, cambio de tipo, …) sin tocar una base de datos;
   código distinto de cero ante cualquier hallazgo destructivo (`--strict`
   para fallar también ante advertencias). ✅ *existe hoy*

Al fusionar a `main`: se aplican las migraciones a la **DB de trunk** (§8).

---

## 10. Convenciones (las reglas que mantienen la calma)

1. **El modelo es la verdad.** Si tu DB no coincide con el modelo, la DB está
   equivocada — reconstrúyela, no la "arregles" a mano.
2. **Las migraciones fusionadas son inmutables.** Nunca edites ni elimines una
   migración que ya llegó a `main`. Para cambiar el esquema, agrega una
   migración nueva.
3. **Un cambio de esquema lógico por migración**, nombrado de forma
   significativa.
4. **Siempre commitea `.model` + migración + `snapshot.json` juntos.**
5. **Reconstruir desde cero es barato y se recomienda.** `molde fresh`
   revierte todas las migraciones y las vuelve a aplicar. Hacer esto con
   regularidad previene el drift.
6. **`sync` es un catch-up de solo lectura, trunk → local. Nunca local →
   cualquier cosa compartida.**
7. **Ejecuta `molde fmt` antes de cada commit** (y exígelo en CI).

---

## 11. Inventario de brechas — todas cerradas ✅

Todo el flujo de trabajo está soportado por molde hoy. Las piezas, y cómo se
completaron:

| # | Pieza | Qué hace |
|---|---|---|
| 1 | **`molde snapshot`** ✅ | Regenera `snapshot.json` a partir de los modelos fusionados, de forma idéntica byte a byte a `migrate` — la base del merge driver (§7.2). |
| 2 | **`molde snapshot --check`** ✅ | Gate de CI §9.4: sale con código distinto de cero cuando el snapshot en disco quedó desactualizado respecto a los modelos. |
| 3 | **`molde verify`** ✅ | Chequeo de drift (§9.3 / local). Compara la DB en vivo contra el modelo, comparando tipos por la forma **almacenada** del motor (`store_type_for`) para que los tipos con pérdida en el round-trip no se lean como drift. `--check` sale con código distinto de cero. |
| 4 | **`molde up`** ✅ | Catch-up diario en un solo comando: `apply` (o `--from-trunk` como sync aditivo) + reporte de drift de `verify`. |
| 5 | **`molde fresh`** ✅ | Reconstruye la DB local a partir de las migraciones (revierte todo, vuelve a aplicar); confirma primero (destructivo). |
| 6 | **`molde init-team`** ✅ | Configuración por clon: línea en `.gitattributes` + merge driver en `.git/config` + hook `post-merge` (+ plantilla `--ci github`). |
| 7 | **Texto de ayuda de provider** ✅ | La ayuda de `--provider` ahora lista los cuatro motores (sqlite \| postgres \| mysql \| sqlserver). |

El merge driver de snapshot + el hook (`init-team`) junto con los gates de CI
de `verify` / `snapshot --check` son lo que entrega la minimización de
conflictos; `up` y `fresh` son la ergonomía del ciclo diario encima de eso.

---

## 12. TL;DR para el equipo

- **Edita `models/`. Ejecuta `molde migrate`. Commitea modelo + migración.
  Abre un PR.**
- **Para ponerte al día: `git pull` y luego `molde up`** (reproduce las
  migraciones, o `--from-trunk` para sincronizar de forma aditiva), y luego
  reporta el drift.
- **Tu DB local es desechable.** El modelo en git es la verdad.
- **Los conflictos de snapshot se resuelven automáticamente** (se regeneran a
  partir del modelo); todo lo demás es un merge de git normal y pequeño, o
  una decisión de diseño real que CI señalará.
