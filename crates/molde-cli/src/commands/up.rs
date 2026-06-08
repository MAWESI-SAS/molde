//! `molde up` — bring your local database up to date, then report drift.
//!
//! The daily catch-up after a `git pull`, in one command. By default it applies
//! pending migrations (prod-accurate). With `--from-trunk`, it instead
//! additively syncs from a canonical "trunk" database (instant, preserves local
//! experiments — see `docs/team-database-workflow.md` §6). Either way it finishes
//! with a `verify` drift report so you know your database matches the model.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::{apply, sync, ui, verify};

#[derive(Args)]
pub struct UpArgs {
    /// Local database connection. Defaults to `DATABASE_URL`; prompted if missing.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine: sqlite | postgres | mysql | sqlserver. Inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Fast-forward from this trunk database (additive sync) instead of replaying
    /// migrations. Falls back to `MOLDE_SYNC_SOURCE`.
    #[arg(long, env = "MOLDE_SYNC_SOURCE")]
    pub from_trunk: Option<String>,

    /// Directory of migrations to apply (replay mode).
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Directory with the `.model` files (for the drift report).
    #[arg(long, default_value = "models")]
    pub from_models: PathBuf,

    /// Schema to read (Postgres only). Defaults to `public`.
    #[arg(long)]
    pub schema: Option<String>,

    /// Apply/sync without asking for confirmation.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run(args: UpArgs) -> Result<()> {
    ui::header("molde up · catch up the local database");

    // Resolve the local connection once so the steps below don't re-prompt.
    let connection = ui::resolve_connection(args.connection, args.no_input)?;

    if let Some(trunk) = args.from_trunk {
        // Fast-forward mode: additive sync trunk → local.
        sync::run(sync::SyncArgs {
            source: Some(trunk),
            target: Some(connection.clone()),
            out: None,
            dry_run: false,
            yes: args.yes,
            no_input: args.no_input,
        })?;
    } else {
        // Replay mode: apply pending migrations.
        apply::run(apply::ApplyArgs {
            connection: Some(connection.clone()),
            provider: args.provider.clone(),
            migrations_dir: args.migrations_dir,
            to: None,
            yes: args.yes,
            no_input: args.no_input,
        })?;
    }

    // Always finish with a (non-failing) drift report.
    verify::run(verify::VerifyArgs {
        connection: Some(connection),
        provider: args.provider,
        schema: args.schema,
        from_models: args.from_models,
        check: false,
        no_input: args.no_input,
    })
}
