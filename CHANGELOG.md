# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.4] - 2026-06-09

### Added
- `molde update` — self-update to the latest GitHub release. Picks the archive
  matching this platform and TLS variant (a native-tls build pulls the native-tls
  asset) and atomically replaces the running binary. `--check` reports without
  changing anything. The download uses rustls against GitHub regardless of
  molde's database TLS backend.

### Changed
- Release archives are now FLAT (the binary sits at the archive root, no wrapping
  folder) so `molde update` can locate it. Manual installs become
  `tar xzf … && sudo install molde /usr/local/bin/molde`.

## [0.0.3] - 2026-06-09

### Changed
- `--help` output rewritten for agent/LLM consumption: the global help states the
  purpose, what molde does NOT do, the order of operations (model-first and
  database-first), conventions, connection handling, and exit codes; every
  command's `--help` now documents its purpose, preconditions/order, validations,
  and a runnable example. Help text only — no behavior changed.

## [0.0.2] - 2026-06-09

### Added
- `molde ci`: one command for pull-request gating — runs lint + snapshot, plus an
  optional from-scratch `verify` against an ephemeral database, and emits a
  Markdown report suitable for posting as a PR comment. Exits non-zero if any
  check fails. See `docs/ci-github-actions.md` for a ready-to-copy workflow.
- `seed-key:` — match seed rows by a natural key instead of the primary key, so a
  database-generated PK (e.g. a `guid` with `gen_random_uuid()`) can be omitted
  from seed rows while still producing incremental `INSERT`/`UPDATE`/`DELETE`.
- Release builds now include a **static musl binary with native-tls** (vendored
  OpenSSL): `molde-<ver>-x86_64-unknown-linux-musl-nativetls.tar.gz`. It runs on
  any Linux (including old glibc / WSL) and connects to servers presenting legacy
  X.509 v1 certificates that the default rustls build rejects.

## [0.0.1] - 2026-06-09

Initial public release.

### Added
- `.model` language (parser + emitter) with a single-sourced naming convention
  (`pk_`/`fk_`/`ix_`) and automatic foreign-key indexes.
- Database-first introspection (`pull`) and model-first migration authoring
  (`migrate` / `undo` / `status`).
- Migration apply runtime across PostgreSQL, MySQL, SQLite, and SQL Server,
  tracked in an `__EFMigrationsHistory` table.
- Database lifecycle (`db create` / `drop` / `reset`).
- Drift checking (`verify`), snapshot management (`snapshot`), additive live
  synchronization (`sync`), catch-up (`up`), and rebuild (`fresh`).
- Static migration safety analysis (`lint`), including explicit file arguments
  and a `--since <id>` flag to scope checks to a pull request's migrations.
- Team workflow tooling (`init-team`) and a VS Code extension with a language
  server for `.model` files.

### Fixed
- Migration ids are guaranteed to sort strictly after every existing migration,
  so two created in the same second no longer tie.

[Unreleased]: https://github.com/MAWESI-SAS/molde/compare/v0.0.4...HEAD
[0.0.4]: https://github.com/MAWESI-SAS/molde/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/MAWESI-SAS/molde/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/MAWESI-SAS/molde/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/MAWESI-SAS/molde/releases/tag/v0.0.1
