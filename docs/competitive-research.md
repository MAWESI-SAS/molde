# Competitive & community research — where molde should go next

> **Date:** 2026-06-08. **Method:** multi-source web research with adversarial
> verification (23 sources fetched, 25 claims 3-vote-verified → 18 confirmed,
> 7 refuted). Confidence is noted per item; see [Caveats](#caveats) for what is
> *not* well-evidenced. Source URLs are listed at the end.

This is decision support, not a spec. It maps the schema-migration landscape,
the most-evidenced community asks, and a prioritized roadmap for molde that marks
**what molde already has** vs **genuine gaps**.

---

## 0. Scope — molde is a database manager, not an ORM

molde manages the **schema lifecycle** of a database: create/drop the database,
introspect, migrate, seed, sync, drift-check. It does **not** do runtime data
access (entity mapping, query builders, change tracking) — that is ORM territory.

So molde's true peers are the **standalone schema/migration managers** — Atlas,
Flyway, Liquibase, Sqitch, dbmate, goose, golang-migrate, pgroll, Bytebase,
Skeema, sqldef. The ORMs in the table below (Prisma, EF Core, TypeORM, Drizzle,
Alembic) are listed for context only; their *migration* sub-systems are
comparable, but molde does not compete with them as ORMs.

## 1. Where molde stands

The field splits into two poles:

- **Declarative / schema-as-code** (Atlas, Prisma Migrate, Drizzle Kit): you
  declare a desired state; the tool diffs it and plans the change.
- **Versioned** (Flyway, golang-migrate, goose, dbmate, Sqitch): you hand-author
  and check in each change.

**molde already sits in the validated middle** — a declarative `.model` DSL
compiled to an IR, but emitting **versioned, committed migrations** diffed
against a snapshot. Its closest sibling is **Atlas**. This hybrid is a
*confirmed strength*, not a gap: versioned output means each change is reviewed
in code review and deploys deterministically from git history without needing
CI/CD to connect to (or get plan-approval against) the production database — the
weakness of purely-declarative deploy-time planning. *(high confidence; sources:
Atlas declarative-vs-versioned, Prisma mental-model.)*

A second validated strength: molde's **team / parallel-migration workflow**. The
single best-evidenced pain point in the whole category is EF Core issue #32351
("Better support for team development of database migrations"), which states that
the lack of safe concurrent-migration tooling "leads to a fear of making database
schema changes in parallel, and instead forces database development to occur in
serial. This dramatically reduces a development team's ability to move quickly."
It lists three concrete conflict sources: out-of-order migration timestamps,
conflicting model-snapshot edits, and migrations referencing objects removed on
the parent branch. **molde already mitigates the first two** (per-entity mergeable
`.model` files, timestamped IDs, snapshot merge driver + `post-merge` hook via
`init-team`, `up` for trunk catch-up). *(high confidence; source: EF #32351.)*

---

## 2. Competitor landscape

| Tool | Model | Engines | Standout strength | Notable weakness / complaint |
|---|---|---|---|---|
| **Atlas** (ariga) | Declarative **+** versioned output | Many (PG/MySQL/SQLite/SQL Server/…) | Migration **linting/analyzers**, native CI/CD PR integrations, drift detection | Advanced analyzers + scheduled drift moved behind paid **Atlas Pro / Cloud** (v0.38, Oct 2025) |
| **Prisma Migrate** | Declarative (model-first) | PG/MySQL/SQLite/SQL Server/… | DX, model-first schema, drift via migration history | Tied to Prisma ecosystem; less control over raw SQL |
| **pgroll** (Xata) | Versioned, **expand-contract** | PostgreSQL only | **Zero-downtime** online schema change (view-schemas + backfill + sync triggers) | PG-only; multi-phase mental model |
| **Flyway / Liquibase** | Versioned | Many | Mature, enterprise adoption | Hand-authored SQL; safety checks largely in paid tiers |
| **Alembic / Django / EF Core** | Versioned (ORM-coupled) | ORM's engines | Integrated with the ORM | Team/parallel conflicts (EF #32351); autogenerate bias |
| **golang-migrate / goose / dbmate / Sqitch** | Versioned | Many | Simple, no-ORM | No diff/linting; manual everything |

> Breadth caveat: only Atlas, pgroll and EF produced claims that survived 3-vote
> verification. The rows for Drizzle/Flyway/Liquibase/Alembic/goose/etc. reflect
> general knowledge, **not independently verified** in this pass — treat as
> directional.

---

## 3. Most-requested capabilities (verified themes)

| # | Capability | Evidence | molde today |
|---|---|---|---|
| 1 | **Pre-deploy migration linting / safety analyzers** — destructive, data-dependent, backward-incompatible changes | Atlas analyzers: DS101-103 (drop schema/table/column), MF101-104 (add unique index, non-unique→unique, add NOT NULL, nullable→NOT NULL), BC101-102 (rename table/column) — verbatim in docs | ❌ **none — top gap** |
| 2 | **Zero-downtime / expand-contract** online schema change | pgroll: "virtual schemas … views on top of physical tables", additive non-breaking start, new column + backfill + bidirectional triggers during the migration window | ❌ none |
| 3 | **CI/CD with PR comments + merge gating** | Atlas: "native integrations … code comments, code suggestions, PR and run summaries, PR status updates"; linting "incorporated into CI … before merging into main" | 🟡 partial (`--check` + `init-team` CI template) |
| 4 | **Schema drift detection** vs version-controlled state | Atlas defines/automates drift ("regularly comparing actual vs intended"); Prisma via migration-history + `migrate diff` | ✅ **`verify`** (on-demand) |
| 5 | **Team / parallel-migration workflow** | EF #32351 (three conflict sources; serialization → lost velocity) | ✅ `init-team` + snapshot + timestamped IDs |
| 6 | **Declarative source, versioned output** | Atlas/Prisma docs | ✅ molde's architecture |

---

## 4. Prioritized roadmap for molde

Genuine gaps, ordered by value/feasibility. molde's existing IR + diff +
snapshot machinery makes the top items unusually cheap.

### ✅ Delivered — `molde db` (database lifecycle)
`molde db create` / `db drop` / `db reset` manage the **database itself** (not
just the schema inside it), across all four engines — sqlx's `MigrateDatabase`
for Postgres/MySQL/SQLite, tiberius against `master` for SQL Server. Closes an
obvious gap for a database manager (the equivalent of `createdb`/`dropdb` /
`rails db:create`/`db:reset`); `db reset` = drop + recreate + apply all
migrations (and seeds).

### ✅ P1 — `molde lint` (migration safety analyzer) — **done**
Static analysis over a migration's `up` operations (`molde_core::lint`), no DB
access — runs in CI on a PR. Findings, with stable codes:
- **Destructive** (data loss → blocks): `drop-table`, `drop-column`.
- **Warning** (may fail on existing data / lock the table): `not-null-no-default`,
  `make-not-null`, `alter-column-type`, `add-unique-index`, `add-foreign-key`.

`molde lint` lints the latest migration (`--all` for every one); exits non-zero on
any destructive finding, and on warnings too with `--strict`. Renames surface as
`drop-column` + add (molde doesn't yet detect renames), so the destructive rule
already covers the backward-incompatible case. Built on the existing IR + diff.
Unblocks P2.

### 🥈 P2 — `molde ci` / GitHub Action: plan + warnings as a PR comment · HIGH · MEDIUM
molde already has `verify --check` and an `init-team` CI template. Add an action
that runs `verify` + `lint`, renders the migration plan and safety warnings as a
**PR comment**, and exits non-zero to **gate the merge**. Directly serves molde's
team-sync use case.

### 🥉 P3 — `verify --fix` / `molde reconcile` · MEDIUM · LOW-MEDIUM
`verify` detects drift; the differentiator competitors add is **emitting the
corrective migration** from the detected drift (and, optionally, scheduled drift
monitoring). Cheap because the `verify` primitive already exists.

### P4 — Detect the third EF conflict case · MEDIUM · LOW
EF #32351 lists three conflict sources; molde covers two. Add detection +
warning for **a migration that references a schema object removed on the parent
branch**, surfaced during `up`/merge (and later in `lint`).

### P5 — Expand-contract / zero-downtime mode · HIGH (Postgres) · HIGH
The most ambitious (pgroll-style multi-phase: new column + backfill + sync
triggers, or view-schema versioning). Pragmatic first step: an **expand-contract
mode / migration annotations** rather than full transparent view virtualization.
Defer until P1-P3 land.

### Not recommended
**AI-assisted migration generation** — this claim did **not** survive
verification in the research; do not weight the roadmap on it.

---

## Caveats

1. **Source weighting.** Verified evidence is heavy on Atlas + pgroll primary
   docs and the EF #32351 issue. Drizzle/Flyway/Liquibase/Alembic/goose/dbmate/
   Sqitch/Skeema/sqldef/Bytebase/TypeORM/SchemaHero/Redgate did **not** produce
   surviving 3-vote claims — their relative strengths here are *not-yet-verified*,
   not absent.
2. **Community sentiment under-represented.** Aggregate "most-upvoted Reddit/HN
   complaints" were not independently verified; the one strong community-pain data
   point (EF #32351) is well-evidenced. Confidence on aggregate "most-requested"
   rankings is therefore *medium* even where individual mechanisms are *high*.
3. **Time-sensitivity.** Atlas's lint analyzers moved partly behind **Atlas Pro**
   (v0.38, Oct 2025) and scheduled drift detection needs **Atlas Cloud + an
   agent** — "Atlas has X" means capability exists, not free/OSS availability.
4. **Analogy precision.** molde `verify` (live-DB-vs-model) is conceptually close
   to Prisma drift (DB-vs-migration-history) but not mechanically identical.
5. **Refuted for transparency.** "AI-assisted migrations (AIM)" and a specific
   "Atlas hybrid versioned-authoring" framing did **not** survive verification.

---

## Sources (verified)

- Atlas — lint analyzers: https://atlasgo.io/lint/analyzers
- Atlas — versioned lint: https://atlasgo.io/versioned/lint
- Atlas — CI/CD setup: https://atlasgo.io/versioned/setup-cicd
- Atlas — drift detection: https://atlasgo.io/monitoring/drift-detection
- Atlas — declarative vs versioned: https://atlasgo.io/concepts/declarative-vs-versioned
- Atlas — integrations: https://atlasgo.io/integrations
- pgroll — repo: https://github.com/xataio/pgroll
- pgroll — internals: https://xata.io/blog/pgroll-internals
- pgroll — expand/contract: https://xata.io/blog/pgroll-expand-contract
- pgroll — Neon guide: https://neon.com/guides/pgroll
- EF Core — team development issue #32351: https://github.com/dotnet/efcore/issues/32351
- Prisma Migrate — mental model: https://www.prisma.io/docs/concepts/components/prisma-migrate/mental-model
- Bytebase — schema-change tool evolution: https://www.bytebase.com/blog/top-database-schema-change-tool-evolution/
- Postgres migration without downtime: https://www.bytebase.com/blog/postgres-schema-migration-without-downtime/
