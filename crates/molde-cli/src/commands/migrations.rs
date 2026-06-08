//! `molde migrations ...` — add / list / remove.
//!
//! `add` toma el modelo de los archivos `.model` (lenguaje molde), calcula el diff
//! contra el snapshot y escribe la migración.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use molde_core::{migration, DatabaseModel};
use molde_design::author::{self, AddOutcome};
use time::OffsetDateTime;

#[derive(Args)]
pub struct AddArgs {
    /// Nombre de la migración (p. ej. `InitialCreate`).
    pub name: String,

    /// Directorio con los archivos `.model` (fuente del modelo).
    #[arg(long, default_value = "models")]
    pub from_models: PathBuf,

    /// Directorio donde se guardan las migraciones.
    #[arg(long, default_value = "migrations")]
    pub output_dir: PathBuf,

    /// Ruta del snapshot del modelo. Por defecto `<output-dir>/snapshot.json`.
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long, default_value = "migrations")]
    pub output_dir: PathBuf,
}

#[derive(Args)]
pub struct RemoveArgs {
    #[arg(long, default_value = "migrations")]
    pub output_dir: PathBuf,

    /// Ruta del snapshot del modelo. Por defecto `<output-dir>/snapshot.json`.
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
}

pub fn add(args: AddArgs) -> anyhow::Result<()> {
    let snapshot_path = args
        .snapshot
        .clone()
        .unwrap_or_else(|| args.output_dir.join("snapshot.json"));

    // 1. Obtener el modelo actual desde los archivos `.model`.
    let model = load_model_dir(&args.from_models)?;

    // 2. Generar el identificador de la migración (timestamp UTC + nombre).
    let id = format!("{}_{}", utc_timestamp(), args.name);

    // 3. Diff contra el snapshot y escritura.
    let outcome = author::add(&args.name, &id, &model, &args.output_dir, &snapshot_path)
        .context("creando la migración")?;

    match outcome {
        AddOutcome::NoChanges => {
            println!(
                "No hay cambios en el modelo respecto al snapshot. No se creó ninguna migración."
            );
        }
        AddOutcome::Created {
            id,
            up_ops,
            down_ops,
            migration_path,
            snapshot_path,
        } => {
            println!("Migración creada: {id}");
            println!(
                "  ✔ {}  ({up_ops} op. up, {down_ops} op. down)",
                migration_path.display()
            );
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
        .unwrap_or_else(|| args.output_dir.join("snapshot.json"));
    let removed = author::remove(&args.output_dir, &snapshot_path)
        .context("eliminando la última migración")?;
    println!("Migración eliminada: {removed}");
    println!("  ✔ snapshot regenerado desde las migraciones restantes.");
    Ok(())
}

/// Lee todos los `.model` de un directorio y los parsea a un modelo IR.
fn load_model_dir(dir: &Path) -> anyhow::Result<DatabaseModel> {
    let mut files: Vec<(String, String)> = Vec::new();
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("leyendo el directorio de modelos {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("model") {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("leyendo {}", path.display()))?;
            files.push((name, contents));
        }
    }
    if files.is_empty() {
        anyhow::bail!("no se encontraron archivos .model en {}", dir.display());
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_str()))
        .collect();
    let mut model = molde_lang::parse_project(&refs).map_err(anyhow::Error::new)?;
    model.normalize();
    Ok(model)
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
