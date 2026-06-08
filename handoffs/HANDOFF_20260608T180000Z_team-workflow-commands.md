# Handoff — team-workflow-commands
Date (UTC): 20260608T180000Z | Status: DONE
Prev handoff: handoffs/HANDOFF_20260608T160000Z_molde-sync-sqlserver-done.md
Base commit: b13027a | Branch: main

## 1. Session Objective
Two threads on top of the SQL Server `sync` work:
1. **Rebuild the LSP + repackage the VS Code extension** (stale after the efrust→molde rename).
2. **Design and build a team database workflow**: a process + tooling so a large team, each with their own local DB, syncs easily with minimal conflicts. The user approved "model-in-git as source of truth" and "design the process first, then build". All gaps from the resulting playbook were then implemented.

Result: editor artifacts refreshed; `docs/team-database-workflow.md` written; **five new/changed CLI commands** shipped (`snapshot`, `verify`, `init-team`, `up`, `fresh`) plus help-text fix. Tree clean at `b13027a`.

## 2. What Was Done
### Editor (commit cdb4099)
- Rebuilt `molde-lsp` as a **static-musl** binary (`x86_64-unknown-linux-musl`, static-pie, ~3.2 MB stripped) → `editors/vscode/server/molde-lsp` and `~/.local/bin/molde-lsp`; removed old `efrust-lsp`. Smoke-tested on the host (glibc 2.31): full LSP `initialize` returns capabilities, exit 0.
- Repackaged the extension → `editors/vscode/molde-language-0.0.2.vsix` (324 files, 1.68 MB; bundles `server/molde-lsp` + prod `node_modules`). Removed stale `efm-language-0.0.2.vsix`. Only tracked change: `package-lock.json` name `efm-language`→`molde-language` (rename leftover).
- NOTE: vsix is **not installed** in VS Code yet — user installs with `code --install-extension editors/vscode/molde-language-0.0.2.vsix --force` (in the Dev Container context).

### Team workflow doc (commit f6a8204)
- `docs/team-database-workflow.md` — the process: **`.model` in git = source of truth**, local DBs are disposable projections, **git = write path (reviewed PRs of model+migration), `sync` = read path (trunk→local additive)**. Devs never peer-sync DBs. Closes with a gap inventory.

### Five commands (commits fa7d9eb, fc7b146, 0a9c2aa, b13027a)
- **`molde snapshot [--from-models D] [--output P] [--check]`** — regenerates `migrations/snapshot.json` from the models, **byte-identical** to what `molde migrate` writes (both now go through `molde_core::snapshot::to_json`, refactored as the single serializer). `--check` exits non-zero when the on-disk snapshot is stale. Basis of the merge driver.
- **`molde verify` (drift check)** — reads the live DB through the `.model` pipeline (`build_model_files` → `parse_project` → normalize) and diffs against the model. **Key:** compares column types by the engine's stored form (`SqlGenerator::store_type_for`) applied to both sides, so round-trip-lossy types (SQLite `int`↔`long`, `varchar` length) don't read as drift, while genuine type changes still do. Filters `RebuildTable` (SQLite-only). Reports drift by direction; `--check` exits non-zero.
- **`molde init-team [--ci github] [--force]`** — per-clone setup: `.gitattributes` line + `merge.molde-snapshot` driver in `.git/config` (`molde snapshot --output %A`) + a **`post-merge` hook** + optional GitHub Actions template. Idempotent.
- **`molde up`** — daily catch-up in one command: `apply` pending migrations (or `--from-trunk` → additive `sync`), then a non-failing `verify` drift report. Thin orchestration over apply/sync/verify (constructs and calls their `run`).
- **`molde fresh`** — rebuild the local DB: `apply --to 0` (roll back all) then `apply` (re-apply). Destructive → confirms first; refuses under `--no-input` without `--yes`.
- Help-text fix: `--provider` on apply/pull/verify now lists all 4 engines.

### README (this commit, pending)
- Capability matrix gains `verify` and `up`/`fresh` rows; Commands section lists snapshot/verify/sync/up/fresh; pointer to the team workflow doc.

## 3. Key Decisions / Discoveries
- **snapshot.json is derivable** (= normalized serialization of the model), so a merge conflict on it is resolved by *regeneration*, not hand-merge. This is the linchpin of conflict-minimization.
- **Merge driver alone is insufficient** (discovered in e2e): it runs *mid-merge*, before files added on the other branch are in the working tree, so the regenerated snapshot can be **stale**. Fix: `init-team` also installs a **`post-merge` hook** that re-derives the snapshot once the tree settles and stages it. The merge completes with no manual snapshot editing; the dev commits the staged fix; `snapshot --check` in CI is the backstop.
- **verify fidelity:** comparing full IR columns gave false drift on SQLite (int→long, varchar length lost). Solved by comparing the engine **store type** (`store_type_for`) on both sides — cancels engine round-trip loss, keeps real type changes. (On Postgres/MySQL/SQL Server types are preserved, so it's even cleaner there.)
- molde already uses **timestamp migration IDs** (no sequential collisions); `.model` is per-entity (mergeable). Those two classic team pains were already solved before this session.

## 4. Validation
- `cargo test --workspace` green at each step (21 "test result: ok" suites; new unit tests: snapshot `status`, verify `classify`, init-team template consistency). `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean. All via Docker `rust:1-bookworm` (volumes `molde-target`/`efrust-cargo-reg`/`efrust-rustup`).
- **e2e (SQLite, in-container):**
  - `snapshot`: regenerate, `--check` pass/stale with correct exit codes, byte-deterministic, agrees with `migrate`'s snapshot.
  - `verify`: in-sync→0 drift; unapplied model column + stray DB table detected; genuine int→string change flagged; `--check` exit codes.
  - `init-team` + real git merges: headline case (two branches, different entities) merges with **zero manual conflict resolution**, post-merge hook leaves `snapshot.json` correct; same-entity case auto-resolves the snapshot while the `.model` conflicts for manual resolution (as documented).
  - `up`: applies + drift report; `fresh`: drops seeded data, rebuilds schema, verify in-sync; `fresh --no-input` without `--yes` refuses.
- NOT tested: `up --from-trunk`/`verify`/`fresh` against Postgres/MySQL/SQL Server live DBs (logic reuses the already-verified apply/sync/scaffold paths). VS Code extension not installed yet.

## 5. Status of the Playbook
All 7 gaps in `docs/team-database-workflow.md` §11 are **closed**. The team workflow is fully supported by the CLI: `snapshot`(+`--check`)/`init-team` (conflict-free snapshot), `verify` (drift gate), `up`/`fresh` (daily ergonomics), on top of existing `pull`/`migrate`/`apply`/`sync`/`fmt`.

## 6. Entry Points / Files
- `crates/molde-cli/src/commands/{snapshot,verify,init_team,up,fresh}.rs` — the new commands; `main.rs`/`commands/mod.rs` wire them.
- `crates/molde-core/src/snapshot.rs` — `to_json` (shared serializer; `save` calls it).
- `crates/molde-cli/src/commands/migrate.rs` — `load_model_dir` is now `pub(crate)` (reused by snapshot/verify).
- `docs/team-database-workflow.md` — the process + closed gap inventory.
- Reuse map: `verify` uses `molde_scaffold::build_model_files` + `molde_lang::parse_project` + `molde_core::diff` + `SqlGenerator::store_type_for`; `up`/`fresh` call `apply::run`/`sync::run`/`verify::run` with constructed Args.

## 7. Possible Next Steps (all optional)
- Install the new vsix in VS Code (Dev Container) and confirm the LSP works end-to-end under the `molde` name.
- Exercise `verify`/`up`/`fresh` against a live Postgres/MySQL/SQL Server (only SQLite e2e done).
- Consider second-granularity migration-ID collisions if two migrations are ever authored in the same second (pre-existing; surfaced as a benign ordering note during `fresh` testing).

## 8. Context That Should NOT Be Lost
- Code/CLI/docs in **English** (open-source); chat in Spanish.
- Host has no cargo/node and glibc 2.31 → build/test/e2e via Docker; host-run binaries need static musl.
- User develops inside a Dev Container (repo at `/workspaces/EF_RUST`); editor artifacts and `.git`-local config (merge driver, hooks) are per-clone.
- `__EFMigrationsHistory` preserved verbatim (DB interop); `sync` stays an IR-independent catalog port.
