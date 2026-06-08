# molde documentation

Reference and guides for molde. New here? Start with the
[project README](../README.md) and the [example walkthrough](../examples/README.md).

## Reference

- **[cli.md](cli.md)** — complete CLI reference: every command and flag, with
  examples. (Or run `molde <command> --help`.)
- **[molde-language-spec.md](molde-language-spec.md)** — the `.model` language:
  syntax, fields, facets, relationships, indexes, seeds, and globals.
- **[model-ir.md](model-ir.md)** — the intermediate representation
  (`molde_core::DatabaseModel`) that the language, readers, and diff all share.

## Guides

- **[install.md](install.md)** — install and build per OS (Linux/macOS/Windows),
  prebuilt binaries, and static musl builds.
- **[team-database-workflow.md](team-database-workflow.md)** — running molde in a
  large team where everyone has their own local database: `.model` as the source
  of truth, the snapshot merge driver, and keeping local DBs current.

## Background

- **[competitive-research.md](competitive-research.md)** — where molde sits in
  the schema-migration landscape and the prioritized roadmap. Decision support,
  not a spec.

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for how to set up a dev environment and
the checks CI runs.
