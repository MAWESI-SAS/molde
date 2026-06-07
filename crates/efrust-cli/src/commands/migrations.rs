//! `efrust migrations ...` — add / list / remove. Fase 4.

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use efrust_core::migration;
use efrust_design::author::{self, AddOutcome};
use efrust_design::sidecar::{self, SidecarOptions};
use time::OffsetDateTime;

#[derive(Args)]
pub struct AddArgs {
    /// Nombre de la migración (p. ej. `InitialCreate`).
    pub name: String,

    /// Assembly compilado del proyecto del usuario (.dll con el DbContext).
    #[arg(long)]
    pub assembly: PathBuf,

    /// Ruta al `efrust-sidecar.dll`. Por defecto, la variable `EFRUST_SIDECAR`.
    #[arg(long, env = "EFRUST_SIDECAR")]
    pub sidecar: PathBuf,

    /// Ejecutable de .NET.
    #[arg(long, default_value = "dotnet")]
    pub dotnet: String,

    /// Nombre del DbContext (si el proyecto tiene varios).
    #[arg(long)]
    pub context: Option<String>,

    /// Directorio donde se guardan las migraciones.
    #[arg(long, default_value = "Migrations")]
    pub output_dir: PathBuf,

    /// Ruta del snapshot del modelo. Por defecto `<output-dir>/model-snapshot.json`.
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long, default_value = "Migrations")]
    pub output_dir: PathBuf,
}

#[derive(Args)]
pub struct RemoveArgs {
    #[arg(long, default_value = "Migrations")]
    pub output_dir: PathBuf,

    /// Ruta del snapshot del modelo. Por defecto `<output-dir>/model-snapshot.json`.
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
}

pub fn add(args: AddArgs) -> anyhow::Result<()> {
    let snapshot_path = args
        .snapshot
        .unwrap_or_else(|| args.output_dir.join("model-snapshot.json"));

    // 1. Obtener el modelo actual desde el sidecar .NET.
    tracing::info!("obteniendo el modelo desde el sidecar…");
    let model = sidecar::fetch_model(&SidecarOptions {
        dotnet: &args.dotnet,
        sidecar_dll: &args.sidecar,
        assembly: &args.assembly,
        context: args.context.as_deref(),
    })
    .context("ejecutando el sidecar")?;

    // 2. Generar el identificador de la migración (timestamp UTC + nombre).
    let id = format!("{}_{}", utc_timestamp(), args.name);

    // 3. Diff contra el snapshot y escritura.
    let outcome = author::add(&args.name, &id, &model, &args.output_dir, &snapshot_path)
        .context("creando la migración")?;

    match outcome {
        AddOutcome::NoChanges => {
            println!("No hay cambios en el modelo respecto al snapshot. No se creó ninguna migración.");
        }
        AddOutcome::Created {
            id,
            up_ops,
            down_ops,
            migration_path,
            snapshot_path,
        } => {
            println!("Migración creada: {id}");
            println!("  ✔ {}  ({up_ops} op. up, {down_ops} op. down)", migration_path.display());
            println!("  ✔ snapshot actualizado: {}", snapshot_path.display());
        }
    }
    Ok(())
}

pub fn list(args: ListArgs) -> anyhow::Result<()> {
    let migrations = migration::load_dir(&args.output_dir)
        .with_context(|| format!("leyendo migraciones de {}", args.output_dir.display()))?;
    if migrations.is_empty() {
        println!("No hay migraciones en {}.", args.output_dir.display());
        return Ok(());
    }
    for m in &migrations {
        println!("  {}", m.id);
    }
    println!("{} migración(es).", migrations.len());
    Ok(())
}

pub fn remove(args: RemoveArgs) -> anyhow::Result<()> {
    let snapshot_path = args
        .snapshot
        .unwrap_or_else(|| args.output_dir.join("model-snapshot.json"));
    let removed = author::remove(&args.output_dir, &snapshot_path)
        .context("eliminando la última migración")?;
    println!("Migración eliminada: {removed}");
    println!("  ✔ snapshot regenerado desde las migraciones restantes.");
    Ok(())
}

/// Timestamp UTC en formato `yyyyMMddHHmmss` (estilo EF).
fn utc_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}
