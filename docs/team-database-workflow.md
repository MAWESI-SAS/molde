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
| `molde up` *(proposed, §11)* | One command: `git`-aware catch-up = apply pending migrations **or** `sync` from trunk + drift report. |

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
molde up            # proposed: applies pending migrations, or sync-fast-forwards
                    # from trunk, then reports drift.
# Today, until `molde up` exists:
#   git pull && molde apply            # replay pending migrations on local DB
#   # …or fast-forward from the trunk DB without replay:
#   molde sync --source "$TRUNK_DB" --target "$LOCAL_DB" --yes
```

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

Wire it as a git **merge driver** so the resolution is automatic:

```gitattributes
# .gitattributes
migrations/snapshot.json merge=molde-snapshot
```
```ini
# .git/config (installed by `molde init-team`, §11 item 6)
[merge "molde-snapshot"]
    name = regenerate molde snapshot from models
    driver = molde snapshot --output %A
```

> The merge driver re-derives the snapshot from the **working-tree** `.model`
> files. In the common case (only `snapshot.json` conflicts, models merge
> cleanly) this is fully automatic. If a `.model` file *also* conflicted, resolve
> that text conflict first (§7.3), then run `molde snapshot` to fix the snapshot.

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
3. **Model ⇄ migrations consistency** — after applying all migrations, the DB
   structure must equal the model. Implementable today as: `molde migrate` on a
   clean tree produces **no** new migration (the model matches the snapshot), and
   a fresh-applied DB `sync --dry-run` against the model-built DB reports **0
   additive changes**. *(A dedicated `molde verify` would make this one step —
   §11 item 3.)*
4. **Snapshot consistency** — `molde snapshot --check` verifies `snapshot.json`
   equals the serialization of `models/` (no stale snapshot), exiting non-zero if
   it drifted. ✅ *exists today*

On merge to `main`: apply migrations to the **trunk DB** (§8).

---

## 10. Conventions (the rules that keep it calm)

1. **The model is the truth.** If your DB disagrees with the model, the DB is
   wrong — rebuild it, don't "fix" it by hand.
2. **Merged migrations are immutable.** Never edit or delete a migration that has
   reached `main`. To change schema, add a new migration.
3. **One logical schema change per migration**, named meaningfully.
4. **Always commit `.model` + migration + `snapshot.json` together.**
5. **Rebuild from scratch is cheap and encouraged.** A `molde fresh` (proposed)
   = drop local DB + `apply` all migrations. Doing this regularly prevents drift.
6. **`sync` is read-only catch-up, trunk → local. Never local → anything shared.**
7. **Run `molde fmt` before every commit** (and enforce in CI).

---

## 11. Gap inventory — what molde still needs for this to be frictionless

Everything above works **today** with `git + molde apply/migrate/sync`, except
the conveniences below. Prioritized by impact on conflict-minimization:

| # | Gap | Why it matters | Shape |
|---|---|---|---|
| 1 | **`molde snapshot`** ✅ **done** | The #1 remaining merge friction (§7.2). Regenerating the snapshot from the merged models makes concurrent migrations conflict-free. | `molde snapshot [--from-models D] [--output P]` regenerates `snapshot.json` byte-identically to `migrate`. Wire as a `.gitattributes` merge driver (`driver = molde snapshot --output %A`). |
| 2 | **`molde snapshot --check`** ✅ **done** | CI gate §9.4; lets devs detect a stale snapshot before pushing. | Same command, `--check` exits non-zero when the on-disk snapshot drifted from the models. |
| 3 | **`molde verify`** (drift check) | One-step CI gate §9.3 and local "is my DB in sync with the model?" Answers the question `sync --dry-run` approximates today. | Compare a target DB's live structure to the model (reuse `sync`'s reader + `migrate`'s diff); report drift, exit non-zero in `--check`. |
| 4 | **`molde up`** | Collapses the daily catch-up (§5.1) into one command: `git`-aware apply **or** sync-from-trunk + drift report. | Thin orchestration over `apply`/`sync` + `verify`. |
| 5 | **`molde fresh`** | Encourages the "rebuild is cheap" convention (§10.5). | Drop/recreate local DB + `apply` all. |
| 6 | **`molde init-team`** | One-shot setup of `.gitattributes` merge driver + sample CI. Lowers adoption cost for a large team. | Writes the merge-driver config and a CI template. |
| 7 | **Stale help text** | `molde apply --provider` help still says "sqlite \| postgres" but 4 engines exist. | Trivial copy fix. |

**Build order:** items 1–2 are **done** (this is the bulk of the
conflict-minimization value). Next is item 3 (`molde verify`) to complete the CI
gates; items 4–6 are ergonomics; item 7 is a one-line cleanup.

---

## 12. TL;DR for the team

- **Edit `models/`. Run `molde migrate`. Commit model + migration. Open a PR.**
- **To catch up: `git pull` then `molde sync` from trunk (fast) or `molde apply`
  (prod-accurate).**
- **Your local DB is disposable.** The model in git is the truth.
- **Snapshot conflicts auto-resolve** (regenerate from the model); everything
  else is a normal, small git merge or a real design decision CI will flag.
