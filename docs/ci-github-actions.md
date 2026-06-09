# Gating pull requests with `molde ci`

`molde ci` runs the checks that should block a merge and prints a single Markdown
report (see the [CLI reference](cli.md#molde-ci)):

1. **lint** — static safety analysis of the migrations (no database). Fails on
   destructive changes; with `--strict`, also on warnings.
2. **snapshot** — `snapshot.json` must be up to date with the models.
3. **verify** — *optional*: with `--connection`, apply every migration to an
   ephemeral database from scratch and confirm there's no drift against the
   models. Skipped when no connection is given.

It exits non-zero if any check fails. Run it locally any time:

```bash
molde ci                                            # lint + snapshot (no DB)
molde ci --connection "postgres://…" --report ci.md # + verify, and save the report
```

## A ready-to-copy GitHub Actions workflow

Drop this in your project as `.github/workflows/molde-ci.yml`. It spins up a
throwaway PostgreSQL, runs `molde ci`, posts the report as a sticky PR comment,
and fails the job (blocking the merge) if any check failed.

```yaml
name: molde

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write   # to post the report comment

jobs:
  molde-ci:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: postgres
          POSTGRES_DB: molde_ci
        ports: ["5432:5432"]
        options: >-
          --health-cmd "pg_isready -U postgres"
          --health-interval 5s --health-timeout 5s --health-retries 10
    steps:
      - uses: actions/checkout@v4

      - name: Install molde
        run: |
          curl -L -o molde.tar.gz \
            https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.2-x86_64-unknown-linux-musl.tar.gz
          tar xzf molde.tar.gz
          sudo install molde-*/molde /usr/local/bin/molde
          molde --version || molde --help | head -1

      - name: Run molde ci
        id: molde_ci
        continue-on-error: true
        env:
          DATABASE_URL: postgres://postgres:postgres@localhost:5432/molde_ci
        run: molde ci --connection "$DATABASE_URL" --report molde-ci.md

      - name: Post report as a PR comment
        if: always()
        uses: marocchino/sticky-pull-request-comment@v2
        with:
          path: molde-ci.md

      - name: Enforce the merge gate
        if: steps.molde_ci.outcome == 'failure'
        run: exit 1
```

### How it works

- The `postgres` **service** gives `molde ci` an empty database; `verify` applies
  every migration into it from scratch, so a clean run proves the migrations
  build the schema your models describe.
- `continue-on-error: true` lets the workflow **always post the comment** (even on
  failure); the final step re-fails the job so the check is still required.
- `marocchino/sticky-pull-request-comment` creates the comment once and updates it
  in place on later pushes. Prefer no third-party action? Post `molde-ci.md` with
  `actions/github-script` instead.

### Notes & customization

- **Pin the version.** The example downloads `v0.0.2`; bump it (or build from
  source with `cargo install`) as you upgrade. Other prebuilt targets are on the
  [releases page](https://github.com/MAWESI-SAS/molde/releases/latest).
- **Scope lint to the PR.** By default every migration is linted. To check only
  what the PR adds, pass `--since <base-migration-id>` (the last migration id on
  your target branch).
- **Strict mode.** Add `--strict` to fail on lint *warnings* too (e.g. adding a
  unique index or a foreign key that could fail on dirty data).
- **No database?** Drop the service and the `--connection` flag; `molde ci` then
  runs lint + snapshot only, and the verify check reports as skipped.
- **Other engines.** Point `--connection` at a MySQL or SQL Server service
  instead; set `--provider` if it can't be inferred from the URL.

> `molde init-team --ci github` scaffolds a starter workflow with the snapshot
> merge driver for teams; this page is the richer, PR-commenting version built
> around `molde ci`.
```
