//! `molde verify` — check whether a live database matches the model.
//!
//! Answers "is this database in sync with the `.model` files?" It reads the live
//! structure, renders it through the **same** `.model` pipeline the authored
//! models use (scaffold → `.model` text → parse → normalize), then compares both
//! column types by what the **target engine actually stores** (`store_type_for`)
//! before diffing the two IR models. That second step is what keeps it quiet:
//! engines round-trip types lossily (SQLite stores both `int` and `long` as
//! `INTEGER` and drops a `varchar`'s length), so judging drift on the stored form
//! cancels those non-differences while still catching genuine type changes. The
//! diff is then empty exactly when the database matches the model.
//!
//! Drift is classified by direction:
//! - **missing in the database** — the model has it, the DB doesn't (you forgot
//!   to apply a migration, or the model is ahead).
//! - **extra in the database** — the DB has it, the model doesn't (a manual or
//!   stale change).
//! - **differs** — present in both but defined differently.
//!
//! `--check` exits non-zero on any drift (CI gate §9.3 of the team workflow).

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use molde_core::{diff, DatabaseModel, Operation};
use molde_providers::{Provider, SqlGenerator};

use crate::commands::migrate::load_model_dir;
use crate::commands::ui;

#[derive(Args)]
pub struct VerifyArgs {
    /// Connection string to the database to check.
    /// Defaults to the `DATABASE_URL` env var; if missing, you are prompted.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine: inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Schema to read (Postgres only). Defaults to `public`.
    #[arg(long)]
    pub schema: Option<String>,

    /// Directory with the `.model` files (the desired state).
    #[arg(long, default_value = "models")]
    pub from_models: PathBuf,

    /// Exit non-zero if the database drifts from the model (CI gate).
    #[arg(long)]
    pub check: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run(args: VerifyArgs) -> anyhow::Result<()> {
    ui::header("molde verify · database ⇄ model");

    let desired = load_model_dir(&args.from_models)?;

    let connection = ui::resolve_connection(args.connection, args.no_input)?;
    let provider = match args.provider.as_deref() {
        Some(p) => Provider::parse(p).with_context(|| {
            format!("unsupported provider: '{p}' (use sqlite | postgres | mysql | sqlserver)")
        })?,
        None => Provider::from_url(&connection)
            .context("could not infer the provider from the URL; use --provider")?,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the async runtime")?;

    let sp = ui::spinner(format!("reading the schema of {}", ui::redact(&connection)));
    let live = runtime.block_on(read_live_model(
        &connection,
        provider,
        args.schema.as_deref(),
    ));
    sp.finish_and_clear();
    let mut live = live?;
    let mut desired = desired;

    // Compare column types by what the *target engine* actually stores, so
    // round-trip-lossy distinctions don't read as drift. On SQLite, e.g., both
    // `int` and `long` store as `INTEGER` and `varchar(n)` loses its length, so
    // an in-sync database would otherwise show spurious `AlterColumn`s. Mapping
    // both sides through the engine's `store_type_for` cancels that while still
    // catching genuine type changes (`text` vs `integer`).
    let generator = provider.generator();
    canonicalize_types(&mut desired, generator.as_ref());
    canonicalize_types(&mut live, generator.as_ref());

    // diff(live → desired): operations that would bring the DB up to the model.
    // Empty ⇒ the database matches the model.
    let operations = diff(&live, &desired);
    let drift: Vec<&Operation> = operations
        .iter()
        // RebuildTable is a SQLite-only re-expression of the granular ALTERs, not
        // a distinct drift — skip it so changes aren't double-counted.
        .filter(|op| !matches!(op, Operation::RebuildTable { .. }))
        .collect();

    if drift.is_empty() {
        ui::ok("the database is in sync with the model. No drift.");
        return Ok(());
    }

    report(&drift);

    if args.check {
        anyhow::bail!(
            "{} drift item(s) between the database and the model",
            drift.len()
        );
    }
    Ok(())
}

/// Reads the live database and renders it through the authored-`.model` pipeline
/// so it is directly comparable to the desired model.
async fn read_live_model(
    connection: &str,
    provider: Provider,
    schema: Option<&str>,
) -> anyhow::Result<DatabaseModel> {
    let files = molde_scaffold::build_model_files(connection, provider, schema)
        .await
        .context("reading the database schema")?;
    if files.is_empty() {
        return Ok(DatabaseModel::empty());
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f.name.as_str(), f.contents.as_str()))
        .collect();
    let mut model = molde_lang::parse_project(&refs)
        .map_err(anyhow::Error::new)
        .context("parsing the introspected schema")?;
    model.normalize();
    Ok(model)
}

/// Rewrite every column's type to the target engine's stored form, so drift is
/// judged on what the database can actually persist (not on engine-lossy clr/type
/// nuance). Also drops `comment`, which is not schema drift. Applied to both the
/// desired and the live model so genuine type changes still surface.
fn canonicalize_types(model: &mut DatabaseModel, generator: &dyn SqlGenerator) {
    for table in &mut model.tables {
        for column in &mut table.columns {
            // Force the canonical clr → store mapping (ignore any carried store
            // type so both sides are computed identically).
            column.store_type = None;
            let store = generator.store_type_for(column).ok();
            column.clr_type = None;
            column.max_length = None;
            column.precision = None;
            column.scale = None;
            column.comment = None;
            column.store_type = store;
        }
        table.comment = None;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Direction {
    MissingInDb,
    ExtraInDb,
    Differs,
}

/// Print the drift grouped by direction.
fn report(drift: &[&Operation]) {
    ui::warn(format!("{} drift item(s) found:", drift.len()));
    for (dir, label) in [
        (
            Direction::MissingInDb,
            "missing in database (model is ahead — apply migrations?)",
        ),
        (Direction::ExtraInDb, "extra in database (not in the model)"),
        (Direction::Differs, "differs between database and model"),
    ] {
        let items: Vec<String> = drift
            .iter()
            .filter(|op| classify(op).0 == dir)
            .map(|op| classify(op).1)
            .collect();
        if items.is_empty() {
            continue;
        }
        ui::info(format!("{label}:"));
        for item in items {
            ui::info(format!("  - {item}"));
        }
    }
}

/// Map an operation to its drift direction and a human label.
fn classify(op: &Operation) -> (Direction, String) {
    use Direction::*;
    match op {
        Operation::CreateTable { table } => (MissingInDb, format!("table {}", table.name)),
        Operation::AddColumn { table, column, .. } => {
            (MissingInDb, format!("column {table}.{}", column.name))
        }
        Operation::AddForeignKey {
            table, foreign_key, ..
        } => (
            MissingInDb,
            format!("foreign key {table}.{}", foreign_key.name),
        ),
        Operation::CreateIndex { table, index, .. } => {
            (MissingInDb, format!("index {table}.{}", index.name))
        }
        Operation::EnsureExtension { name } => (MissingInDb, format!("extension {name}")),
        Operation::CreateFunction { function } => {
            (MissingInDb, format!("function {}", function.name))
        }
        Operation::CreateTrigger { table, trigger, .. } => {
            (MissingInDb, format!("trigger {table}.{}", trigger.name))
        }
        Operation::InsertData { table, .. } => (MissingInDb, format!("seed row in {table}")),

        Operation::DropTable { name, .. } => (ExtraInDb, format!("table {name}")),
        Operation::DropColumn { table, name, .. } => (ExtraInDb, format!("column {table}.{name}")),
        Operation::DropForeignKey { table, name, .. } => {
            (ExtraInDb, format!("foreign key {table}.{name}"))
        }
        Operation::DropIndex { table, name, .. } => (ExtraInDb, format!("index {table}.{name}")),
        Operation::DropFunction { name, .. } => (ExtraInDb, format!("function {name}")),
        Operation::DropTrigger { table, name, .. } => {
            (ExtraInDb, format!("trigger {table}.{name}"))
        }
        Operation::DeleteData { table, .. } => (ExtraInDb, format!("seed row in {table}")),

        Operation::AlterColumn { table, new, .. } => {
            (Differs, format!("column {table}.{}", new.name))
        }
        Operation::UpdateData { table, .. } => (Differs, format!("seed row in {table}")),
        Operation::RawSql { .. } => (Differs, "raw SQL object".to_string()),
        // Filtered out before classify; keep exhaustive.
        Operation::RebuildTable { table, .. } => (Differs, format!("table {}", table.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, Direction};
    use molde_core::Operation;

    #[test]
    fn classify_maps_direction_and_label() {
        let (dir, label) = classify(&Operation::DropTable {
            schema: None,
            name: "Stray".into(),
        });
        assert_eq!(dir, Direction::ExtraInDb);
        assert_eq!(label, "table Stray");

        let (dir, label) = classify(&Operation::EnsureExtension {
            name: "vector".into(),
        });
        assert_eq!(dir, Direction::MissingInDb);
        assert_eq!(label, "extension vector");

        let (dir, _) = classify(&Operation::DropColumn {
            schema: None,
            table: "T".into(),
            name: "c".into(),
        });
        assert_eq!(dir, Direction::ExtraInDb);
    }
}
