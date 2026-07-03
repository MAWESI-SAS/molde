# Investigación de competencia y comunidad — hacia dónde debería ir molde

> **Fecha:** 2026-06-08. **Método:** investigación web multi-fuente con
> verificación adversarial (23 fuentes consultadas, 25 afirmaciones
> verificadas por 3 votos → 18 confirmadas, 7 refutadas). La confianza se
> indica por ítem; consulta [Advertencias](#caveats) para ver qué *no* está
> bien evidenciado. Las URLs de las fuentes se listan al final.

Esto es apoyo para la toma de decisiones, no una especificación. Mapea el
panorama de gestión de migraciones de esquema, las peticiones de la
comunidad mejor evidenciadas, y una hoja de ruta priorizada para molde que
marca **lo que molde ya tiene** frente a **brechas genuinas**.

---

## 0. Alcance — molde es un gestor de bases de datos, no un ORM

molde gestiona el **ciclo de vida del esquema** de una base de datos:
create/drop de la base de datos, introspect, migrate, seed, sync,
drift-check. **No** realiza acceso a datos en tiempo de ejecución (mapeo de
entidades, query builders, seguimiento de cambios) — eso es territorio de
los ORM.

Así que los verdaderos pares de molde son los **gestores de esquema/migración
independientes (standalone)** — Atlas, Flyway, Liquibase, Sqitch, dbmate,
goose, golang-migrate, pgroll, Bytebase, Skeema, sqldef. Los ORM de la tabla
siguiente (Prisma, EF Core, TypeORM, Drizzle, Alembic) se listan solo como
contexto; sus subsistemas de *migración* son comparables, pero molde no
compite con ellos como ORM.

## 1. Dónde se ubica molde

El campo se divide en dos polos:

- **Declarativo / schema-as-code** (Atlas, Prisma Migrate, Drizzle Kit):
  declaras un estado deseado; la herramienta calcula el diff y planea el
  cambio.
- **Versionado** (Flyway, golang-migrate, goose, dbmate, Sqitch): escribes a
  mano y confirmas (check in) cada cambio.

**molde ya se ubica en el punto medio validado** — un DSL `.model`
declarativo compilado a un IR, pero que emite **migraciones versionadas y
confirmadas (committed)** calculadas por diff contra un snapshot. Su
pariente más cercano es **Atlas**. Este híbrido es una *fortaleza
confirmada*, no una brecha: la salida versionada significa que cada cambio
se revisa en code review y se despliega de forma determinista desde el
historial de git, sin necesitar que el CI/CD se conecte a (u obtenga
aprobación de plan contra) la base de datos de producción — la debilidad de
la planeación puramente declarativa en tiempo de despliegue. *(confianza
alta; fuentes: Atlas declarative-vs-versioned, Prisma mental-model.)*

Una segunda fortaleza validada: el **workflow de equipo / migraciones en
paralelo** de molde. El punto de dolor mejor evidenciado en toda la
categoría es el issue #32351 de EF Core ("Mejor soporte para el desarrollo
en equipo de migraciones de base de datos"), que afirma que la falta de
herramientas seguras para migraciones concurrentes "genera temor a hacer
cambios de esquema de base de datos en paralelo, y en cambio obliga a que el
desarrollo de la base de datos ocurra en serie. Esto reduce drásticamente la
capacidad de un equipo de desarrollo para avanzar rápido." Enumera tres
fuentes concretas de conflicto: timestamps de migración fuera de orden,
ediciones conflictivas del model-snapshot, y migraciones que referencian
objetos eliminados en la rama padre. **molde ya mitiga las dos primeras**
(archivos `.model` fusionables por entidad, IDs con timestamp, merge driver
de snapshot + hook `post-merge` vía `init-team`, `up` para ponerse al día
con el trunk). *(confianza alta; fuente: EF #32351.)*

---

## 2. Panorama de la competencia

| Herramienta | Modelo | Motores | Fortaleza destacada | Debilidad / queja destacada |
|---|---|---|---|---|
| **Atlas** (ariga) | Salida declarativa **+** versionada | Muchos (PG/MySQL/SQLite/SQL Server/…) | **Linting/analyzers** de migraciones, integraciones nativas de CI/CD con PR, detección de drift | Los analyzers avanzados + drift programado quedaron detrás del pago **Atlas Pro / Cloud** (v0.38, oct. 2025) |
| **Prisma Migrate** | Declarativo (model-first) | PG/MySQL/SQLite/SQL Server/… | DX, esquema model-first, drift vía historial de migraciones | Atado al ecosistema de Prisma; menos control sobre SQL crudo |
| **pgroll** (Xata) | Versionado, **expand-contract** | Solo PostgreSQL | Cambio de esquema en línea con **cero downtime** (view-schemas + backfill + sync triggers) | Solo PG; modelo mental multi-fase |
| **Flyway / Liquibase** | Versionado | Muchos | Maduro, adopción enterprise | SQL escrito a mano; los chequeos de seguridad están mayormente en los planes pagos |
| **Alembic / Django / EF Core** | Versionado (acoplado al ORM) | Motores del ORM | Integrado con el ORM | Conflictos de equipo/paralelismo (EF #32351); sesgo del autogenerate |
| **golang-migrate / goose / dbmate / Sqitch** | Versionado | Muchos | Simple, sin ORM | Sin diff/linting; todo manual |

> Advertencia de cobertura: solo Atlas, pgroll y EF produjeron afirmaciones
> que sobrevivieron la verificación de 3 votos. Las filas de
> Drizzle/Flyway/Liquibase/Alembic/goose/etc. reflejan conocimiento general,
> **no verificado de forma independiente** en esta pasada — trátalas como
> orientativas.

---

## 3. Capacidades más solicitadas (temas verificados)

| # | Capacidad | Evidencia | molde hoy |
|---|---|---|---|
| 1 | **Linting de migraciones pre-deploy / safety analyzers** — cambios destructivos, dependientes de datos, incompatibles hacia atrás | Analyzers de Atlas: DS101-103 (drop schema/table/column), MF101-104 (add unique index, non-unique→unique, add NOT NULL, nullable→NOT NULL), BC101-102 (rename table/column) — textual en la documentación | ❌ **ninguno — brecha principal** |
| 2 | Cambio de esquema en línea **zero-downtime / expand-contract** | pgroll: "esquemas virtuales … vistas sobre las tablas físicas", inicio aditivo sin romper compatibilidad, columna nueva + backfill + triggers bidireccionales durante la ventana de migración | ❌ ninguno |
| 3 | **CI/CD con comentarios en PR + compuerta de merge** | Atlas: "integraciones nativas … comentarios de código, sugerencias de código, resúmenes de PR y de ejecución, actualizaciones de estado del PR"; el linting "incorporado en CI … antes de hacer merge a main" | 🟡 parcial (`--check` + plantilla de CI de `init-team`) |
| 4 | **Detección de drift de esquema** frente al estado versionado | Atlas define/automatiza el drift ("comparando regularmente el estado real vs. el intencionado"); Prisma vía migration-history + `migrate diff` | ✅ **`verify`** (bajo demanda) |
| 5 | **Workflow de equipo / migraciones en paralelo** | EF #32351 (tres fuentes de conflicto; serialización → pérdida de velocidad) | ✅ `init-team` + snapshot + IDs con timestamp |
| 6 | **Fuente declarativa, salida versionada** | Documentación de Atlas/Prisma | ✅ arquitectura de molde |

---

## 4. Hoja de ruta priorizada para molde

Brechas genuinas, ordenadas por valor/factibilidad. La infraestructura
existente de molde (IR + diff + snapshot) hace que los primeros elementos
sean inusualmente baratos de implementar.

### ✅ Entregado — `molde db` (ciclo de vida de la base de datos)
`molde db create` / `db drop` / `db reset` gestionan la **base de datos en
sí** (no solo el esquema dentro de ella), en los cuatro motores —
`MigrateDatabase` de sqlx para Postgres/MySQL/SQLite, tiberius contra
`master` para SQL Server. Cierra una brecha obvia para un gestor de bases de
datos (el equivalente a `createdb`/`dropdb` / `rails db:create`/`db:reset`);
`db reset` = drop + recrear + aplicar todas las migraciones (y seeds).

### ✅ P1 — `molde lint` (safety analyzer de migraciones) — **listo**
Análisis estático sobre las operaciones `up` de una migración
(`molde_core::lint`), sin acceso a la base de datos — corre en CI sobre un
PR. Hallazgos, con códigos estables:
- **Destructivo** (pérdida de datos → bloquea): `drop-table`, `drop-column`.
- **Advertencia** (puede fallar con datos existentes / bloquear la tabla):
  `not-null-no-default`, `make-not-null`, `alter-column-type`,
  `add-unique-index`, `add-foreign-key`.

`molde lint` lintea la última migración (`--all` para todas); termina con
código distinto de cero ante cualquier hallazgo destructivo, y también ante
advertencias con `--strict`. Los renames aparecen como `drop-column` + add
(molde aún no detecta renames), así que la regla de destructivo ya cubre el
caso incompatible hacia atrás. Construido sobre el IR + diff existentes.
Desbloquea a P2.

### 🥈 P2 — `molde ci` / GitHub Action: plan + warnings como comentario en el PR · ALTO · MEDIO
molde ya tiene `verify --check` y una plantilla de CI de `init-team`.
Agregar una action que ejecute `verify` + `lint`, renderice el plan de
migración y las advertencias de seguridad como un **comentario en el PR**, y
termine con código distinto de cero para **bloquear el merge**. Sirve
directamente al caso de uso de sincronización de equipo de molde.

### 🥉 P3 — `verify --fix` / `molde reconcile` · MEDIO · BAJO-MEDIO
`verify` detecta el drift; el diferenciador que agregan los competidores es
**emitir la migración correctiva** a partir del drift detectado (y,
opcionalmente, monitoreo programado de drift). Barato porque la primitiva
`verify` ya existe.

### P4 — Detectar el tercer caso de conflicto de EF · MEDIO · BAJO
EF #32351 enumera tres fuentes de conflicto; molde cubre dos. Agregar
detección + advertencia para **una migración que referencia un objeto de
esquema eliminado en la rama padre**, mostrada durante `up`/merge (y más
adelante en `lint`).

### P5 — Modo expand-contract / zero-downtime · ALTO (Postgres) · ALTO
El más ambicioso (multi-fase al estilo pgroll: columna nueva + backfill +
sync triggers, o versionado por view-schema). Primer paso pragmático: un
**modo expand-contract / anotaciones de migración** en lugar de
virtualización completa y transparente por vistas. Diferir hasta que P1-P3
estén implementados.

### No recomendado
**Generación de migraciones asistida por IA** — esta afirmación **no**
sobrevivió la verificación en la investigación; no le des peso a la hoja de
ruta basándote en ella.

---

## Advertencias

1. **Ponderación de fuentes.** La evidencia verificada se concentra en la
   documentación primaria de Atlas + pgroll y en el issue EF #32351.
   Drizzle/Flyway/Liquibase/Alembic/goose/dbmate/ Sqitch/Skeema/sqldef/
   Bytebase/TypeORM/SchemaHero/Redgate **no** produjeron afirmaciones que
   sobrevivieran los 3 votos — sus fortalezas relativas aquí están *aún sin
   verificar*, no ausentes.
2. **El sentimiento de la comunidad está subrepresentado.** Las "quejas más
   votadas en Reddit/HN" agregadas no se verificaron de forma independiente;
   el único dato de dolor de comunidad bien respaldado (EF #32351) está bien
   evidenciado. Por eso la confianza en los rankings agregados de "más
   solicitado" es *media*, incluso cuando los mecanismos individuales son
   *altos*.
3. **Sensibilidad temporal.** Los analyzers de lint de Atlas se movieron
   parcialmente detrás de **Atlas Pro** (v0.38, oct. 2025) y la detección de
   drift programada necesita **Atlas Cloud + un agente** — "Atlas tiene X"
   significa que la capacidad existe, no que esté disponible gratis/OSS.
4. **Precisión de la analogía.** `verify` de molde (DB en vivo vs. modelo)
   es conceptualmente cercano al drift de Prisma (DB vs. historial de
   migraciones) pero no es mecánicamente idéntico.
5. **Refutado, por transparencia.** "AI-assisted migrations (AIM)" y un
   framing específico de "Atlas hybrid versioned-authoring" **no**
   sobrevivieron la verificación.

---

## Fuentes (verificadas)

- Atlas — lint analyzers: https://atlasgo.io/lint/analyzers
- Atlas — versioned lint: https://atlasgo.io/versioned/lint
- Atlas — CI/CD setup: https://atlasgo.io/versioned/setup-cicd
- Atlas — drift detection: https://atlasgo.io/monitoring/drift-detection
- Atlas — declarative vs versioned: https://atlasgo.io/concepts/declarative-vs-versioned
- Atlas — integrations: https://atlasgo.io/integrations
- pgroll — repositorio: https://github.com/xataio/pgroll
- pgroll — internals: https://xata.io/blog/pgroll-internals
- pgroll — expand/contract: https://xata.io/blog/pgroll-expand-contract
- pgroll — guía de Neon: https://neon.com/guides/pgroll
- EF Core — issue de desarrollo en equipo #32351: https://github.com/dotnet/efcore/issues/32351
- Prisma Migrate — mental model: https://www.prisma.io/docs/concepts/components/prisma-migrate/mental-model
- Bytebase — schema-change tool evolution: https://www.bytebase.com/blog/top-database-schema-change-tool-evolution/
- Postgres migration without downtime: https://www.bytebase.com/blog/postgres-schema-migration-without-downtime/
