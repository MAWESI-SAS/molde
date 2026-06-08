//! molde — schema management in Rust over the `.model` language.
//!
//! Commands:
//! - `pull`     database → `.model` files (introspection)
//! - `migrate`  `.model` → migration (diff against the snapshot)
//! - `apply`    apply/roll back migrations against the database
//! - `status`   list known migrations
//! - `undo`     remove the latest migration
//! - `fmt`      format `.model` files to their canonical form

use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "molde",
    version,
    about = "molde — a Rust schema tool: introspect, migrate, apply",
    long_about = None
)]
struct Cli {
    /// Increase log verbosity (-v, -vv).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Introspect an existing database into `.model` files (database-first).
    Pull(commands::pull::PullArgs),

    /// Create a migration from the diff between the models and the snapshot.
    Migrate(commands::migrate::MigrateArgs),

    /// Regenerate (or verify with --check) the migration snapshot from the models.
    Snapshot(commands::snapshot::SnapshotArgs),

    /// Apply pending migrations to the database (or roll back with `--to`).
    Apply(commands::apply::ApplyArgs),

    /// Additively sync a target database from a source (live DB → live DB).
    Sync(commands::sync::SyncArgs),

    /// Check whether a live database matches the model (drift check).
    Verify(commands::verify::VerifyArgs),

    /// List known migrations.
    Status(commands::migrate::StatusArgs),

    /// Remove the latest migration.
    Undo(commands::migrate::UndoArgs),

    /// Format `.model` files to their canonical form.
    Fmt(commands::fmt::FmtArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Commands::Pull(args) => commands::pull::run(args),
        Commands::Migrate(args) => commands::migrate::migrate(args),
        Commands::Snapshot(args) => commands::snapshot::run(args),
        Commands::Apply(args) => commands::apply::run(args),
        Commands::Sync(args) => commands::sync::run(args),
        Commands::Verify(args) => commands::verify::run(args),
        Commands::Status(args) => commands::migrate::status(args),
        Commands::Undo(args) => commands::migrate::undo(args),
        Commands::Fmt(args) => commands::fmt::run(args),
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter = std::env::var("MOLDE_LOG").unwrap_or_else(|_| format!("molde={level}"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
