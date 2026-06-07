//! efrust — CLI estilo `dotnet ef` en Rust.
//!
//! Fase 0: estructura de comandos y cableado del pipeline. La lógica de cada
//! comando está como esqueleto (`todo!`-ish con mensajes) y se completa por fases:
//! - Fase 1: `database update`
//! - Fase 2: `dbcontext scaffold`
//! - Fase 4: `migrations add` (requiere el sidecar de Fase 3)

use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "efrust",
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
    /// Genera modelos C# + DbContext desde una base de datos existente (database-first).
    Scaffold(commands::scaffold::ScaffoldArgs),

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
    let filter = std::env::var("EFRUST_LOG").unwrap_or_else(|_| format!("efrust={level}"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
