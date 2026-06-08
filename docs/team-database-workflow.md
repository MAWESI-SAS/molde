# Team database workflow — many local databases, easy sync, few conflicts

> **Status:** proposal for approval.
> **Audience:** a large team where every developer runs their own local
> database and the schema evolves concurrently.
> **Goal:** make day-to-day synchronization trivial and structurally minimize
> conflicts, while keeping the schema reviewed, versioned, and safe for
> production.

This document defines the **process**. It also closes with a precise inventory
of the small set of molde features still missing to support it end to end
(§11) — clearly marking what exists **today** vs. what is **proposed**.

---

## 1. The mental model (read this first)

The schema lives in **two representations**, and each has exactly one job:

| Representation | What it is | Role |
|---|---|---|
| **`.model` files** (one per entity) | Declarative *desired* state, in git | **Source of truth** |
| **`migrations/`** (timestamped) + `snapshot.json` | Ordered, reviewed *changes* derived from model diffs | Deterministic, replayable history |
| **A developer's local database** | A *projection* of the model | **Disposable** — rebuildable at any time |

Three principles follow:

1. **The model in git is the truth.** A local database is never the truth; it is
   a cache you can throw away and rebuild. This single rule removes most "it
   works on my machine" schema drift.
2. **Git is the *write* path; `sync` is the *read* path.**
   - To **contribute** a change you open a PR with `.model` + a generated
     migration. It is reviewed like any code.
   - To **catch up** you fast-forward your local database from a canonical
     "trunk" database with `molde sync` (additive, never destructive).
3. **Developer databases are never synced to each other.** Peer-to-peer DB sync
   is how teams get chaos. All contributions flow through reviewed git; `sync`
   only ever flows **trunk → your local**, one direction, additive.

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

## 2. Why this minimizes conflicts

The classic team-migration pain has three root causes. molde already neutralizes
two of them; this process handles the third.

| Classic failure | How it's avoided here |
|---|---|
| **Sequential migration IDs collide** (two devs both create `0042_*`) | molde already uses **timestamp IDs** `yyyyMMddHHmmss_name` — two devs never collide, and order is unambiguous. ✅ *exists today* |
| **One giant generated model snapshot conflicts on every merge** (EF's `ModelSnapshot.cs`) | `snapshot.json` is the **normalized serialization of the model**, so it is *derivable*: on conflict you regenerate it from the merged `.model` files instead of hand-merging (§5.2). |
| **Pulling teammates' changes clobbers local experiments** | `molde sync` is **additive and conflict-reporting** — it never drops or alters target-only objects, so your local WIP survives. ✅ *exists today* |
| **`.model` itself is hard to merge** | `.model` is **one file per entity**, plain text — concurrent edits to *different* entities never conflict; same-entity edits are small, readable text merges. ✅ *exists today* |

A key property worth calling out: **`molde migrate` diffs `.model` against
`snapshot.json` — pure files, no database required.** Migration authoring is
therefore deterministic and reviewable, independent of any developer's local DB
state.

---

## 3. Command roles in this workflow

All exist today unless marked *(proposed)*.

| Command | Role in the workflow |
|---|---|
| `molde pull` | DB → `.model`. Used **once** to bootstrap the model from an existing database, or to reconcile a hand-edited DB. Not part of the daily loop. |
| `molde migrate -m "<name>"` | `.model` → new migration + updated `snapshot.json`. The **contribution** step. |
| `molde apply` | Apply pending migrations to a database (or roll back with `--to`). The **deterministic** way to bring a DB to a given state. |
| `molde sync` | Additive live DB → DB. The **catch-up** step (trunk → local). |
| `molde status` | List known/applied migrations. |
| `molde undo` | Remove the latest (un-merged, local) migration while iterating. |
| `molde fmt` | Canonicalize `.model` files (run before commit; enforce in CI). |
| `molde snapshot` | Regenerate `snapshot.json` from `.model` with no migration (or verify with `--check`) — powers the merge driver (§7.2). ✅ *exists today* |
| `molde verify` | Check whether a live database matches the model (drift check). `--check` exits non-zero on drift — CI gate §9.3 and the local "is my DB in sync?" answer. ✅ *exists today* |
| `molde init-team` | One-shot, per-clone setup of the snapshot merge driver + `post-merge` hook (and `--ci github` template) — §7.2. ✅ *exists today* |
| `molde up` | Catch the local DB up in one command: apply pending migrations (or `--from-trunk` to additively sync), then a drift report. ✅ *exists today* |
| `molde fresh` | Rebuild the local DB from migrations (roll back all, re-apply) — the "rebuilding is cheap" convention (§10). ✅ *exists today* |

---

## 4. Repository layout

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

Convention: `models/` is edited by humans; `migrations/` and `snapshot.json` are
**generated artifacts that are committed** (so history is reviewable and prod
applies the exact reviewed SQL).

---

## 5. The daily developer loop

### 5.1 Catch up (start of day / before a new change)

```bash
git pull
molde up                         # apply pending migrations, then report drift
#   molde up --from-trunk "$TRUNK_DB"   # …or fast-forward from trunk (additive sync)
```

`molde up` is the one-command catch-up. Under the hood it is either
`molde apply` (replay) or `molde sync` from trunk, followed by `molde verify`.

`apply` is deterministic (prod-like) but replays every pending migration; `sync`
is instant (copies the live trunk structure additively) and preserves your local
experiments. Both are valid catch-up modes — see §6.

### 5.2 Make a schema change

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

The reviewer sees **the declarative intent** (`models/` diff) **and the exact
SQL** (`migrations/` diff) in one PR.

### 5.3 Iterating before the PR is merged

If you need to revise an *un-merged* migration, use `molde undo` to drop the last
one, edit the model, and re-run `molde migrate`. **Never** edit a migration that
has already merged to main (§7).

---

## 6. Two catch-up modes (when to use which)

| | `molde apply` (replay) | `molde sync` (fast-forward) |
|---|---|---|
| Mechanism | Re-runs pending migrations in order | Copies trunk's live structure additively |
| Speed | O(number of pending migrations) | One pass, instant |
| Fidelity | Exactly what prod will run | Structural superset; ignores order |
| Local WIP | Untouched (migrations are additive) | **Preserved** (sync is additive, reports conflicts) |
| Use when | You want prod-accuracy / are about to author a migration | You just want a current schema fast and have local experiments to keep |

Rule of thumb: **`sync` to keep coding fast; `apply` (on a fresh DB) before you
author or test a migration**, so what you generate matches what prod will apply.

---

## 7. Conflict resolution (the heart of this)

There are four kinds of "conflict", in increasing rarity:

### 7.1 `.model` conflict (different entities) — never happens
Two devs editing different `.model` files do not conflict. This is the common
case and it is free.

### 7.2 `snapshot.json` conflict (the EF trap) — auto-resolvable
When two branches each ran `molde migrate`, both regenerated `snapshot.json`, so
git reports a conflict on that one file. **Do not hand-merge it.** Because the
snapshot is just the normalized serialization of the model, the correct snapshot
after a merge is *the serialization of the merged `.model` files* — exactly what
`molde snapshot` writes (byte-identical to what `molde migrate` would write):

```bash
# regenerate from the merged model, no migration:
molde snapshot            # writes migrations/snapshot.json from models/
git add migrations/snapshot.json
```

**Don't do this by hand — `molde init-team` wires it up** (run once per clone).
It installs two cooperating pieces:

1. A git **merge driver** — `.gitattributes` routes `snapshot.json` to it, and
   it runs `molde snapshot --output %A` *during* the merge so the merge completes
   without halting on conflict markers:
   ```gitattributes
   # .gitattributes (committed)
   migrations/snapshot.json merge=molde-snapshot
   ```
   ```ini
   # .git/config (local, per clone — written by init-team)
   [merge "molde-snapshot"]
       driver = molde snapshot --output %A
   ```
2. A **`post-merge` hook** — the merge driver runs mid-merge, when files added on
   the other side may not be in the working tree yet, so its snapshot can be
   stale. The hook re-derives the snapshot once the tree has settled and stages
   the fix.

So in practice: the merge **completes with no manual snapshot editing**; the hook
leaves the correct `snapshot.json` staged; you commit it. `molde snapshot --check`
in CI (§9.4) is the backstop that guarantees nothing stale lands on `main`.

> If a `.model` file *also* conflicted (same entity edited two ways, §7.3),
> resolve that text conflict first, then `molde snapshot` — the hook can't
> re-derive a correct snapshot from a model that still has conflict markers.

### 7.3 Same `.model` entity edited two ways — a normal, small text merge
Resolve the `.model` text conflict like any code merge, then regenerate the
snapshot (§7.2). The per-entity granularity keeps this tiny.

### 7.4 Semantic conflict (two migrations do incompatible things)
E.g. both branches add a column `Invoice.Total` with different types. This is a
*real* design conflict, not a tooling artifact. It surfaces as either a `.model`
merge conflict (§7.3) or a failure when migrations are applied on a fresh DB.
**CI catches it** by applying all migrations on an empty database (§9). The fix
is a human decision, as it should be.

---

## 8. The canonical "trunk" database

A single, CI-maintained database that always reflects `main`:

- On every merge to `main`, CI applies the new migrations to the **trunk DB**
  (and runs the §9 gates).
- Developers `molde sync --source "$TRUNK_DB"` to fast-forward their local DB
  without replaying history (§6).
- The trunk DB is **read-only to humans**: nobody edits it directly. It is a
  projection of `main`, exactly like a local DB is a projection of the model.

This gives the team a fast, always-current "read replica of the schema" without
making any database the source of truth.

---

## 9. CI gates (what protects `main`)

On every PR:

1. **`molde fmt --check`** — models are canonical.
2. **Fresh-DB apply** — spin up an empty DB, `molde apply` *all* migrations from
   scratch. Catches ordering and semantic conflicts (§7.4).
3. **Model ⇄ migrations consistency** — apply all migrations to a fresh DB, then
   `molde verify --check` against it: it must report **no drift**, i.e. the
   migrated database equals the model. ✅ *exists today*
4. **Snapshot consistency** — `molde snapshot --check` verifies `snapshot.json`
   equals the serialization of `models/` (no stale snapshot), exiting non-zero if
   it drifted. ✅ *exists today*
5. **Migration safety** — `molde lint` statically flags destructive changes
   (drop table/column) and risky ones (NOT NULL without default, unique index,
   type change, …) without touching a database; non-zero on any destructive
   finding (`--strict` to fail on warnings too). ✅ *exists today*

On merge to `main`: apply migrations to the **trunk DB** (§8).

---

## 10. Conventions (the rules that keep it calm)

1. **The model is the truth.** If your DB disagrees with the model, the DB is
   wrong — rebuild it, don't "fix" it by hand.
2. **Merged migrations are immutable.** Never edit or delete a migration that has
   reached `main`. To change schema, add a new migration.
3. **One logical schema change per migration**, named meaningfully.
4. **Always commit `.model` + migration + `snapshot.json` together.**
5. **Rebuild from scratch is cheap and encouraged.** `molde fresh` rolls back all
   migrations and re-applies them. Doing this regularly prevents drift.
6. **`sync` is read-only catch-up, trunk → local. Never local → anything shared.**
7. **Run `molde fmt` before every commit** (and enforce in CI).

---

## 11. Gap inventory — all closed ✅

The whole workflow is supported by molde today. The pieces, and how they landed:

| # | Piece | What it does |
|---|---|---|
| 1 | **`molde snapshot`** ✅ | Regenerates `snapshot.json` from the merged models, byte-identically to `migrate` — the basis of the merge driver (§7.2). |
| 2 | **`molde snapshot --check`** ✅ | CI gate §9.4: exits non-zero when the on-disk snapshot drifted from the models. |
| 3 | **`molde verify`** ✅ | Drift check (§9.3 / local). Diffs the live DB against the model, comparing types by the engine's **stored** form (`store_type_for`) so round-trip-lossy types don't read as drift. `--check` exits non-zero. |
| 4 | **`molde up`** ✅ | One-command daily catch-up: `apply` (or `--from-trunk` additive sync) + `verify` drift report. |
| 5 | **`molde fresh`** ✅ | Rebuild the local DB from migrations (roll back all, re-apply); confirms first (destructive). |
| 6 | **`molde init-team`** ✅ | Per-clone setup: `.gitattributes` line + merge driver in `.git/config` + `post-merge` hook (+ `--ci github` template). |
| 7 | **Provider help text** ✅ | `--provider` help now lists all four engines (sqlite \| postgres \| mysql \| sqlserver). |

The snapshot merge driver + hook (`init-team`) plus the `verify` / `snapshot
--check` CI gates are what deliver the conflict-minimization; `up` and `fresh`
are the daily-loop ergonomics on top.

---

## 12. TL;DR for the team

- **Edit `models/`. Run `molde migrate`. Commit model + migration. Open a PR.**
- **To catch up: `git pull` then `molde up`** (replays migrations, or
  `--from-trunk` to additively sync), then reports drift.
- **Your local DB is disposable.** The model in git is the truth.
- **Snapshot conflicts auto-resolve** (regenerate from the model); everything
  else is a normal, small git merge or a real design decision CI will flag.
