//! `molde apply` — apply (or roll back) migrations against the database.

use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::Args;
use molde_core::migration;
use molde_migrate::Migrator;
use molde_providers::Provider;

use crate::commands::ui;

#[derive(Args)]
#[command(
    after_long_help = r#"PURPOSE: apply pending migrations to a database (or roll back with --to).
Records each applied migration in the __EFMigrationsHistory table.

ORDER / PRECONDITIONS:
  • migrations/ must exist — run `molde migrate <Name>` first.
  • a reachable database via -c/--connection or $DATABASE_URL.

VALIDATIONS:
  • Confirms before touching the database unless -y/--yes (or --no-input).
  • --to <id|name> moves up OR down to that migration; --to 0 rolls everything back.
  • Each migration runs in one transaction (its DDL + the history row together).

EXAMPLES:
  molde apply -c "$DATABASE_URL"                 # apply all pending
  molde apply -c "$DATABASE_URL" --to 0          # roll back everything
  molde apply -c "$DATABASE_URL" --to InitialCreate"#
)]
pub struct ApplyArgs {
    /// Connection string. Defaults to `DATABASE_URL`; if missing, you are prompted.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine: sqlite | postgres | mysql | sqlserver. Inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Directory of migrations to apply.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Bring the database up to this migration (id or name). `0` rolls back all.
    /// By default, applies every pending migration.
    #[arg(long)]
    pub to: Option<String>,

    /// Do not ask for confirmation before touching the database.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run(args: ApplyArgs) -> anyhow::Result<()> {
    ui::header("molde apply · apply migrations");
    let connection = ui::resolve_connection(args.connection, args.no_input)?;
    let provider = resolve_provider(args.provider.as_deref(), &connection)?;

    let migrations = migration::load_dir(&args.migrations_dir)
        .with_context(|| format!("reading migrations from {}", args.migrations_dir.display()))?;
    if migrations.is_empty() {
        ui::warn(format!(
            "no migrations found in {}",
            args.migrations_dir.display()
        ));
        return Ok(());
    }

    // Confirm before touching the database (unless --yes or --no-input).
    let target = args
        .to
        .as_deref()
        .map(|t| format!(" up to '{t}'"))
        .unwrap_or_default();
    ui::info(format!(
        "{} migration(s) in {} → database '{}'{target}",
        migrations.len(),
        args.migrations_dir.display(),
        ui::db_name(&connection),
    ));
    if !args.yes && ui::interactive(args.no_input) && !ui::confirm("Apply to the database?", false)?
    {
        bail!("cancelled by the user");
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the async runtime")?;

    let sp = ui::spinner(format!("applying to {}", ui::redact(&connection)));
    let report = runtime.block_on(async {
        let migrator = Migrator::connect(&connection, provider)
            .await
            .context("connecting to the database")?;
        migrator
            .update(&migrations, args.to.as_deref())
            .await
            .context("applying migrations")
    });
    sp.finish_and_clear();
    let report = report?;

    if report.is_empty() {
        ui::ok("the database was already up to date. Nothing to apply.");
    } else {
        for id in &report.reverted {
            ui::info(format!("⤺ reverted  {id}"));
        }
        for id in &report.applied {
            ui::ok(format!("applied   {id}"));
        }
        ui::ok(format!(
            "{} applied, {} reverted.",
            report.applied.len(),
            report.reverted.len()
        ));
    }
    Ok(())
}

/// Resolve the provider: explicit (`--provider`) or inferred from the URL.
fn resolve_provider(explicit: Option<&str>, url: &str) -> anyhow::Result<Provider> {
    match explicit {
        Some(p) => Provider::parse(p).with_context(|| {
            format!("unsupported provider: '{p}' (use sqlite | postgres | mysql | sqlserver)")
        }),
        None => Provider::from_url(url)
            .context("could not infer the provider from the URL; specify it with --provider"),
    }
}
