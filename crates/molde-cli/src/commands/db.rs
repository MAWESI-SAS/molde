//! `molde db` — database lifecycle (create / drop / reset). One level above the
//! schema: `migrate`/`apply` work inside an existing database; these manage the
//! database itself (like `createdb`/`dropdb`, or `rails db:create`/`db:reset`).

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use molde_migrate::admin;
use molde_providers::Provider;

use crate::commands::{apply, ui};

#[derive(Args)]
pub struct DbArgs {
    #[command(subcommand)]
    pub action: DbAction,
}

#[derive(Subcommand)]
pub enum DbAction {
    /// Create the database if it doesn't exist.
    Create(ConnArgs),
    /// Drop the database (destructive).
    Drop(ConnArgs),
    /// Drop, recreate, and apply all migrations from scratch.
    Reset(ResetArgs),
}

#[derive(Args)]
pub struct ConnArgs {
    /// Connection string. Defaults to `DATABASE_URL`; prompted if missing.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine: sqlite | postgres | mysql | sqlserver. Inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Do not ask for confirmation before a destructive action.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

#[derive(Args)]
pub struct ResetArgs {
    #[command(flatten)]
    pub conn: ConnArgs,

    /// Directory of migrations to apply after recreating the database.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,
}

pub fn run(args: DbArgs) -> Result<()> {
    match args.action {
        DbAction::Create(c) => create(c),
        DbAction::Drop(c) => drop(c),
        DbAction::Reset(r) => reset(r),
    }
}

fn create(args: ConnArgs) -> Result<()> {
    ui::header("molde db create");
    let (connection, provider) = resolve(&args)?;
    let created = runtime()?.block_on(admin::create_database(&connection, provider))?;
    if created {
        ui::ok(format!("created database '{}'.", ui::db_name(&connection)));
    } else {
        ui::info(format!(
            "database '{}' already exists. Nothing to do.",
            ui::db_name(&connection)
        ));
    }
    Ok(())
}

fn drop(args: ConnArgs) -> Result<()> {
    ui::header("molde db drop");
    let (connection, provider) = resolve(&args)?;
    confirm_destructive(
        &args,
        &format!(
            "Drop database '{}'? This deletes all its data.",
            ui::db_name(&connection)
        ),
    )?;
    let dropped = runtime()?.block_on(admin::drop_database(&connection, provider))?;
    if dropped {
        ui::ok(format!("dropped database '{}'.", ui::db_name(&connection)));
    } else {
        ui::info(format!(
            "database '{}' did not exist. Nothing to do.",
            ui::db_name(&connection)
        ));
    }
    Ok(())
}

fn reset(args: ResetArgs) -> Result<()> {
    ui::header("molde db reset");
    let (connection, provider) = resolve(&args.conn)?;
    confirm_destructive(
        &args.conn,
        &format!(
            "Reset database '{}'? This drops it, recreates it, and re-applies all migrations.",
            ui::db_name(&connection)
        ),
    )?;

    let rt = runtime()?;
    rt.block_on(admin::drop_database(&connection, provider))
        .context("dropping the database")?;
    rt.block_on(admin::create_database(&connection, provider))
        .context("creating the database")?;
    ui::ok(format!(
        "recreated database '{}'.",
        ui::db_name(&connection)
    ));

    // Apply all migrations on the fresh database (already confirmed → --yes).
    apply::run(apply::ApplyArgs {
        connection: Some(connection),
        provider: args.conn.provider,
        migrations_dir: args.migrations_dir,
        to: None,
        yes: true,
        no_input: args.conn.no_input,
    })
}

/// Resolve the connection (flag/env/prompt) and the engine (flag or inferred).
fn resolve(args: &ConnArgs) -> Result<(String, Provider)> {
    let connection = ui::resolve_connection(args.connection.clone(), args.no_input)?;
    let provider = match args.provider.as_deref() {
        Some(p) => Provider::parse(p).with_context(|| {
            format!("unsupported provider: '{p}' (use sqlite | postgres | mysql | sqlserver)")
        })?,
        None => Provider::from_url(&connection)
            .context("could not infer the provider from the URL; use --provider")?,
    };
    Ok((connection, provider))
}

/// Confirm a destructive action, or fail in non-interactive mode without `--yes`.
fn confirm_destructive(args: &ConnArgs, prompt: &str) -> Result<()> {
    let proceed = if args.yes {
        true
    } else if ui::interactive(args.no_input) {
        ui::confirm(prompt, false)?
    } else {
        false
    };
    if !proceed {
        bail!("cancelled — pass --yes to proceed");
    }
    Ok(())
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the async runtime")
}
