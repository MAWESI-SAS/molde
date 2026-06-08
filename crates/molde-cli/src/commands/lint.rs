//! `molde lint` — static safety analysis of migrations before they ship.
//!
//! Runs the [`molde_core::lint`] rules over a migration's `up` operations and
//! reports risky changes (destructive data loss, or changes that may fail on a
//! populated table). No database access — meant for CI on a pull request.
//!
//! Exit code: non-zero if any **destructive** finding (data loss); with
//! `--strict`, also non-zero on warnings.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;
use molde_core::lint::{lint_operations, Finding, Severity};
use molde_core::migration::{self, Migration};

use crate::commands::ui;

#[derive(Args)]
pub struct LintArgs {
    /// Directory of migrations.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Lint every migration, not just the most recent one.
    #[arg(long)]
    pub all: bool,

    /// Fail on warnings too (data-dependent / locking), not only destructive.
    #[arg(long)]
    pub strict: bool,
}

pub fn run(args: LintArgs) -> Result<()> {
    ui::header("molde lint · migration safety");

    let migrations = migration::load_dir(&args.migrations_dir)
        .with_context(|| format!("reading migrations from {}", args.migrations_dir.display()))?;
    if migrations.is_empty() {
        ui::warn(format!(
            "no migrations in {}",
            args.migrations_dir.display()
        ));
        return Ok(());
    }

    // Default: only the latest migration (the one just authored); `--all`: every one.
    let targets: &[Migration] = if args.all {
        &migrations
    } else {
        &migrations[migrations.len() - 1..]
    };

    let mut destructive = 0usize;
    let mut warnings = 0usize;
    for m in targets {
        let findings = lint_operations(&m.up);
        if findings.is_empty() {
            ui::ok(format!("{} — clean", m.id));
            continue;
        }
        ui::info(format!("{}:", m.id));
        for f in &findings {
            match f.severity {
                Severity::Destructive => destructive += 1,
                Severity::Warning => warnings += 1,
            }
            ui::info(format!("  {}", render(f)));
        }
    }

    ui::info(format!(
        "{destructive} destructive, {warnings} warning(s) across {} migration(s).",
        targets.len()
    ));

    if destructive > 0 {
        bail!("{destructive} destructive change(s) — review before applying");
    }
    if args.strict && warnings > 0 {
        bail!("{warnings} warning(s) and --strict is set");
    }
    Ok(())
}

fn render(f: &Finding) -> String {
    let tag = match f.severity {
        Severity::Destructive => "destructive",
        Severity::Warning => "warning",
    };
    format!("[{tag}] {} ({}): {}", f.object, f.code, f.message)
}
