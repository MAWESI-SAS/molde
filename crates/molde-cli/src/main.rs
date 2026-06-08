//! molde — gestión de esquema en Rust sobre el lenguaje de modelos molde.
//!
//! Comandos:
//! - `scaffold`        BD → archivos `.model`
//! - `fmt`             formatea archivos `.model` a su forma canónica
//! - `migrations add`  `.model` → migración (diff contra el snapshot)
//! - `database update` aplica/revierte migraciones contra la BD

use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "molde",
    version,
    about = "Scaffolding, migraciones y aplicación de migraciones para EF Core, en Rust",
    long_about = None
)]
struct Cli {
    /// Aumenta el nivel de detalle de los logs (-v, -vv).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Genera archivos `.model` (molde) desde una base de datos existente (database-first).
    Scaffold(commands::scaffold::ScaffoldArgs),

    /// Formatea archivos `.model` a su forma canónica.
    Fmt(commands::fmt::FmtArgs),

    /// Operaciones sobre migraciones.
    #[command(subcommand)]
    Migrations(MigrationsCommands),

    /// Operaciones sobre la base de datos.
    #[command(subcommand)]
    Database(DatabaseCommands),
}

#[derive(Subcommand)]
enum MigrationsCommands {
    /// Crea una nueva migración a partir del diff modelo-actual vs snapshot.
    Add(commands::migrations::AddArgs),
    /// Lista las migraciones conocidas.
    List(commands::migrations::ListArgs),
    /// Elimina la última migración (si no está aplicada).
    Remove(commands::migrations::RemoveArgs),
}

#[derive(Subcommand)]
enum DatabaseCommands {
    /// Aplica las migraciones pendientes a la base de datos.
    Update(commands::database::UpdateArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Commands::Scaffold(args) => commands::scaffold::run(args),
        Commands::Fmt(args) => commands::fmt::run(args),
        Commands::Migrations(cmd) => match cmd {
            MigrationsCommands::Add(args) => commands::migrations::add(args),
            MigrationsCommands::List(args) => commands::migrations::list(args),
            MigrationsCommands::Remove(args) => commands::migrations::remove(args),
        },
        Commands::Database(cmd) => match cmd {
            DatabaseCommands::Update(args) => commands::database::update(args),
        },
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
