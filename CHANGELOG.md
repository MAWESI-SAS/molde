# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `molde lint`: accept explicit migration file arguments and a `--since <id>`
  flag to scope checks to a pull request's migrations.

### Fixed
- Migration ids are now guaranteed to sort strictly after every existing
  migration, so two migrations created in the same second no longer tie.

## [0.0.1] - 2026-06-08

Initial public release.

### Added
- `.model` language (parser + emitter) with a single-sourced naming convention
  (`pk_`/`fk_`/`ix_`) and automatic foreign-key indexes.
- Database-first introspection (`pull`) and model-first migration authoring
  (`migrate` / `undo` / `status`).
- Migration apply runtime across PostgreSQL, MySQL, SQLite, and SQL Server.
- Database lifecycle (`db create` / `drop` / `reset`).
- Drift checking (`verify`), snapshot management (`snapshot`), additive live
  synchronization (`sync`), catch-up (`up`), and rebuild (`fresh`).
- Static migration safety analysis (`lint`).
- Team workflow tooling (`init-team`) and a VS Code extension with a language
  server for `.model` files.

[Unreleased]: https://github.com/mawesi/molde/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/mawesi/molde/releases/tag/v0.0.1
