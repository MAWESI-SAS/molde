# Contributing to molde

Thanks for your interest in improving **molde**! This document explains how to
get a development environment running, the conventions the project follows, and
how to propose changes.

By participating you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

molde is a standard Cargo workspace. You need a recent stable Rust toolchain
(see [`rust-toolchain.toml`](rust-toolchain.toml); MSRV is **1.88**) and a C
compiler (for the bundled SQLite). TLS uses rustls, so OpenSSL is **not**
required for the default build.

```bash
git clone https://github.com/MAWESI-SAS/molde
cd molde
cargo build
cargo test --workspace
```

Prefer containers? The [`.devcontainer/`](.devcontainer/) ships Rust plus a
local PostgreSQL — open the repo in VS Code and choose *"Reopen in Container"*.

## Before you open a pull request

Run the same checks CI runs. All three must pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- **Formatting** is enforced by `rustfmt` defaults — run `cargo fmt --all` to fix.
- **Clippy** runs with `-D warnings`; there should be zero warnings.
- **Tests** must stay green. Add tests for new behavior; pure logic should be
  factored into testable functions (see the existing `lint`/`migrate` modules
  for the pattern).

Engine-specific work (PostgreSQL, MySQL, SQL Server) ideally comes with an
end-to-end check against a throwaway database container. SQLite needs no server
and is the easiest to test against.

## Project layout

| Crate | Responsibility |
|---|---|
| `molde-core` | Model IR, snapshot, diff, migrations, lint (engine-agnostic) |
| `molde-lang` | `.model` language: parser and emitter (text ↔ IR) |
| `molde-providers` | Per-engine SQL generation (`SqlGenerator` trait) |
| `molde-migrate` | Migration apply runtime (sqlx + tiberius) |
| `molde-scaffold` | Database-first introspection (DB → IR → `.model`) |
| `molde-design` | Migration authoring (diff against snapshot) |
| `molde-sync` | Additive live database synchronization |
| `molde-lsp` | Language server for `.model` files |
| `molde-cli` | The `molde` binary (command surface) |

Architecture notes and the language spec live in [`docs/`](docs/).

## Conventions

- **Naming conventions are single-sourced** in `molde_core::conventions`
  (`pk_`/`fk_`/`ix_`, lowercase). Never inline a `format!("ix_…")` elsewhere — the
  parser, emitter, and scaffolder must all go through that module or authored and
  introspected models diverge and the round-trip breaks.
- Code, comments, identifiers, and CLI text are in **English**.
- Keep new code consistent with the surrounding style (naming, comment density,
  error handling with `anyhow`/`thiserror`).
- Conventional-style commit subjects are appreciated (e.g.
  `molde: add --since flag to lint`).

## Reporting bugs and requesting features

Use the [issue templates](https://github.com/MAWESI-SAS/molde/issues/new/choose). A
minimal reproduction (a small `.model` file or a connection-less command) helps a
lot. For anything security-related, see [SECURITY.md](SECURITY.md) — please do
**not** open a public issue.

## License

By contributing, you agree that your contributions will be dual-licensed under
the [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses, the same terms
as the project, without any additional terms or conditions.
