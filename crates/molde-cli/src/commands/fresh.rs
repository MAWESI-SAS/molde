//! `molde fresh` — rebuild the local database from migrations.
//!
//! Rolls every migration back (dropping the migration-managed schema and its
//! data) and re-applies them all, so the database is a clean projection of the
//! committed migrations. "Rebuilding is cheap" is a convention that prevents
//! drift (see `docs/team-database-workflow.md` §10) — this is the cheap way.
//!
//! It is **destructive** (data in migration-managed tables is lost), so it
//! confirms first unless `--yes`.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Args;

use crate::commands::{apply, ui};

#[derive(Args)]
#[command(
    after_long_help = r#"PURPOSE: rebuild the local database from migrations — roll ALL migrations back, then re-apply them.
Leaves the DB as a clean projection of the committed migrations.

DESTRUCTIVE: data in migration-managed tables is lost. Confirms first unless -y/--yes.

PRECONDITIONS: migrations/ exists; a reachable -c/--connection or $DATABASE_URL.

EXAMPLE:
  molde fresh -c "$DATABASE_URL" --yes"#
)]
pub struct FreshArgs {
    /// Local database connection. Defaults to `DATABASE_URL`; prompted if missing.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine: sqlite | postgres | mysql | sqlserver. Inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Directory of migrations to rebuild from.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Rebuild without asking for confirmation.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run(args: FreshArgs) -> Result<()> {
    ui::header("molde fresh · rebuild the local database");

    let connection = ui::resolve_connection(args.connection, args.no_input)?;
    ui::warn(format!(
        "this rolls back ALL migrations (dropping schema + data) and re-applies \
         them on '{}'.",
        ui::db_name(&connection)
    ));

    let proceed = if args.yes {
        true
    } else if ui::interactive(args.no_input) {
        ui::confirm("Rebuild the database from scratch?", false)?
    } else {
        // Non-interactive without --yes never destroys data.
        false
    };
    if !proceed {
        bail!("cancelled — pass --yes to rebuild");
    }

    // Roll everything back, then re-apply from scratch. Inner steps are already
    // confirmed, so they run with `yes`.
    apply::run(apply::ApplyArgs {
        connection: Some(connection.clone()),
        provider: args.provider.clone(),
        migrations_dir: args.migrations_dir.clone(),
        to: Some("0".to_string()),
        yes: true,
        no_input: args.no_input,
    })?;
    apply::run(apply::ApplyArgs {
        connection: Some(connection),
        provider: args.provider,
        migrations_dir: args.migrations_dir,
        to: None,
        yes: true,
        no_input: args.no_input,
    })?;

    ui::ok("local database rebuilt from migrations.");
    Ok(())
}
