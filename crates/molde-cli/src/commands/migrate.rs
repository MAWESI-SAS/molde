//! `molde migrate` (create a migration), `molde status` (list) and `molde undo`
//! (remove the latest). `migrate` takes the model from the `.model` files,
//! diffs it against the snapshot, and writes the migration.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use molde_core::{migration, DatabaseModel};
use molde_design::author::{self, AddOutcome};
use time::OffsetDateTime;

use crate::commands::ui;

#[derive(Args)]
pub struct MigrateArgs {
    /// Migration name (e.g. `AddInvoices`). If omitted, you are prompted.
    pub name: Option<String>,

    /// Directory with the `.model` files (the model source).
    #[arg(long, default_value = "models")]
    pub from_models: PathBuf,

    /// Directory where migrations are stored.
    #[arg(long, default_value = "migrations")]
    pub output_dir: PathBuf,

    /// Snapshot path. Defaults to `<output-dir>/snapshot.json`.
    #[arg(long)]
    pub snapshot: Option<PathBuf>,

    /// Do not ask interactive questions (for CI). Fails if the name is missing.
    #[arg(long)]
    pub no_input: bool,
}

#[derive(Args)]
pub struct StatusArgs {
    #[arg(long, default_value = "migrations")]
    pub output_dir: PathBuf,
}

#[derive(Args)]
pub struct UndoArgs {
    #[arg(long, default_value = "migrations")]
    pub output_dir: PathBuf,

    /// Snapshot path. Defaults to `<output-dir>/snapshot.json`.
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
}

pub fn migrate(args: MigrateArgs) -> anyhow::Result<()> {
    ui::header("molde migrate · create migration");
    let name = resolve_name(args.name, args.no_input)?;

    let snapshot_path = args
        .snapshot
        .clone()
        .unwrap_or_else(|| args.output_dir.join("snapshot.json"));

    let sp = ui::spinner("reading models and computing the diff");
    let result = (|| {
        let model = load_model_dir(&args.from_models)?;
        let id = format!("{}_{}", utc_timestamp(), name);
        author::add(&name, &id, &model, &args.output_dir, &snapshot_path)
            .context("creating the migration")
    })();
    sp.finish_and_clear();

    match result? {
        AddOutcome::NoChanges => {
            ui::info("no changes against the snapshot. No migration was created.");
        }
        AddOutcome::Created {
            id,
            up_ops,
            down_ops,
            migration_path,
            snapshot_path,
        } => {
            ui::ok(format!("migration created: {id}"));
            ui::info(format!(
                "{}  ({up_ops} up op(s) · {down_ops} down op(s))",
                migration_path.display()
            ));
            ui::info(format!("snapshot updated: {}", snapshot_path.display()));
        }
    }
    Ok(())
}

pub fn status(args: StatusArgs) -> anyhow::Result<()> {
    let migrations = migration::load_dir(&args.output_dir)
        .with_context(|| format!("reading migrations from {}", args.output_dir.display()))?;
    if migrations.is_empty() {
        ui::info(format!("no migrations in {}.", args.output_dir.display()));
        return Ok(());
    }
    ui::header(format!("Migrations ({})", migrations.len()));
    for m in &migrations {
        ui::info(&m.id);
    }
    Ok(())
}

pub fn undo(args: UndoArgs) -> anyhow::Result<()> {
    let snapshot_path = args
        .snapshot
        .unwrap_or_else(|| args.output_dir.join("snapshot.json"));
    let removed = author::remove(&args.output_dir, &snapshot_path)
        .context("removing the latest migration")?;
    ui::ok(format!("migration removed: {removed}"));
    ui::info("snapshot regenerated from the remaining migrations.");
    Ok(())
}

/// Resolve the migration name: argument → if missing and interactive, ask for
/// it → otherwise an error.
fn resolve_name(name: Option<String>, no_input: bool) -> anyhow::Result<String> {
    if let Some(n) = name.filter(|s| !s.trim().is_empty()) {
        return Ok(n);
    }
    if ui::interactive(no_input) {
        let n = ui::ask("Migration name")?;
        if n.trim().is_empty() {
            anyhow::bail!("the name cannot be empty");
        }
        Ok(n)
    } else {
        anyhow::bail!("missing migration name (pass it as an argument)")
    }
}

/// Read every `.model` file in a directory and parse them into an IR model.
pub(crate) fn load_model_dir(dir: &Path) -> anyhow::Result<DatabaseModel> {
    let mut files: Vec<(String, String)> = Vec::new();
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading the models directory {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("model") {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            files.push((name, contents));
        }
    }
    if files.is_empty() {
        anyhow::bail!("no .model files found in {}", dir.display());
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_str()))
        .collect();
    let mut model = molde_lang::parse_project(&refs).map_err(anyhow::Error::new)?;
    model.normalize();
    Ok(model)
}

/// UTC timestamp in `yyyyMMddHHmmss` format.
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
