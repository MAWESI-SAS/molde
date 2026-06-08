//! `molde sync` — additively sync a target database from a source database.
//!
//! Compares the live structure of two databases and brings into the target what
//! the source has and it lacks, never dropping or altering existing target
//! objects. Conflicts (objects defined differently in both) are reported, not
//! applied. Generates a portable, atomic `.sql` script and optionally applies it.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;
use molde_sync::{diff, engine_for, DiffResult};
use time::OffsetDateTime;

use crate::commands::ui;

#[derive(Args)]
pub struct SyncArgs {
    /// Source database (the one changes are brought FROM, e.g. the shared `test`).
    /// Falls back to the `MOLDE_SYNC_SOURCE` env var; prompted if missing.
    #[arg(long, env = "MOLDE_SYNC_SOURCE")]
    pub source: Option<String>,

    /// Target database (the one that RECEIVES the changes, e.g. your local DB).
    /// Falls back to the `MOLDE_SYNC_TARGET` env var; prompted if missing.
    #[arg(long, env = "MOLDE_SYNC_TARGET")]
    pub target: Option<String>,

    /// Path for the generated `.sql`. Defaults to `./sync-<timestamp>.sql`.
    #[arg(long, short = 'o')]
    pub out: Option<PathBuf>,

    /// Only generate the `.sql` and the report; do not apply anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Apply without asking for confirmation.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run(args: SyncArgs) -> Result<()> {
    ui::header("molde sync · additive database sync");

    let source = resolve(
        args.source,
        "Source database (changes come FROM)",
        args.no_input,
    )?;
    let target = resolve(
        args.target,
        "Target database (receives the changes)",
        args.no_input,
    )?;

    let engine = engine_for(&source)
        .context("sync currently supports PostgreSQL only (source must be postgres://…)")?;
    if engine_for(&target).map(|e| e.name()) != Some(engine.name()) {
        bail!(
            "source and target must be the same engine ({})",
            engine.name()
        );
    }

    ui::info(format!("source = {}", engine.redact(&source)));
    ui::info(format!("target = {}", engine.redact(&target)));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the async runtime")?;

    let sp = ui::spinner("reading both database structures");
    let result = runtime.block_on(async {
        let src = engine
            .read_schema(&source)
            .await
            .context("reading source")?;
        let tgt = engine
            .read_schema(&target)
            .await
            .context("reading target")?;
        Ok::<_, anyhow::Error>(diff(&src, &tgt))
    });
    sp.finish_and_clear();
    let delta = result?;

    print_summary(&delta);

    if !delta.has_changes() && delta.conflicts.is_empty() {
        ui::ok("the target is already up to date with the source. Nothing to do.");
        return Ok(());
    }
    if !delta.has_changes() {
        ui::warn("only conflicts found — no additive script generated (nothing to apply).");
        return Ok(());
    }

    // Build and write the portable .sql script.
    let body = engine.write_ddl(&delta);
    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("sync-{}.sql", timestamp())));
    let file = script_file(
        &body,
        &delta,
        &engine.redact(&source),
        &engine.redact(&target),
    );
    std::fs::write(&out, &file).with_context(|| format!("writing {}", out.display()))?;
    ui::ok(format!("script written: {}", out.display()));

    if args.dry_run {
        ui::info("--dry-run: nothing applied.");
        return Ok(());
    }

    let n = delta.additive_count();
    let proceed = if args.yes {
        true
    } else if ui::interactive(args.no_input) {
        ui::confirm(
            &format!("Apply {n} additive change(s) to the target?"),
            false,
        )?
    } else {
        // Non-interactive without --yes: never apply silently.
        false
    };
    if !proceed {
        ui::info(
            "not applied. The script is saved for review or manual apply (use --yes to apply).",
        );
        return Ok(());
    }

    let sp = ui::spinner("applying to the target");
    let applied = runtime.block_on(engine.apply(&target, &body));
    sp.finish_and_clear();
    applied?;
    ui::ok("applied. The target is synced with the source (local work preserved).");
    Ok(())
}

/// Resolve a connection: flag/env → if missing and interactive, prompt → else error.
fn resolve(value: Option<String>, prompt: &str, no_input: bool) -> Result<String> {
    if let Some(v) = value.filter(|s| !s.trim().is_empty()) {
        return Ok(v);
    }
    if ui::interactive(no_input) {
        let v = ui::ask(prompt)?;
        if v.trim().is_empty() {
            bail!("the connection cannot be empty");
        }
        Ok(v)
    } else {
        bail!("missing connection: pass --source/--target or set MOLDE_SYNC_SOURCE/TARGET")
    }
}

fn print_summary(d: &DiffResult) {
    ui::header("Changes the source has and the target lacks");
    let rows = [
        ("New extensions", d.new_extensions.len()),
        ("New tables", d.new_tables.len()),
        ("New columns", d.new_columns.len()),
        ("New constraints", d.new_constraints.len()),
        ("New indexes", d.new_indexes.len()),
        ("New functions", d.new_functions.len()),
        ("New triggers", d.new_triggers.len()),
        ("New views", d.new_views.len()),
        ("New migrations", d.new_history_rows.len()),
    ];
    for (label, n) in rows {
        ui::info(format!("{label:.<22} {n}"));
    }
    if !d.conflicts.is_empty() {
        ui::warn(format!(
            "conflicts (NOT applied — may be your local changes): {}",
            d.conflicts.len()
        ));
        for c in &d.conflicts {
            ui::warn(format!("  [{}] {}", c.category, c.name));
        }
    }
}

/// Wrap the additive body in a portable, atomic script file with a header.
fn script_file(body: &str, d: &DiffResult, source: &str, target: &str) -> String {
    let mut s = String::new();
    s.push_str("-- Sync script generated by molde\n");
    s.push_str(&format!("-- Source: {source}\n"));
    s.push_str(&format!("-- Target: {target}\n"));
    s.push_str("-- Additive: does not drop or alter anything existing in the target.\n");
    if !d.conflicts.is_empty() {
        s.push_str("--\n");
        s.push_str(&format!(
            "-- CONFLICTS detected ({}) — NOT included in this script:\n",
            d.conflicts.len()
        ));
        for c in &d.conflicts {
            s.push_str(&format!("--   [{}] {}\n", c.category, c.name));
        }
    }
    s.push_str("\nBEGIN;\n\n");
    s.push_str(body);
    s.push_str("\nCOMMIT;\n");
    s
}

/// Local timestamp `yyyyMMdd-HHmmss` for the default output file name.
fn timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}
