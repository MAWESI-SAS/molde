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
    /// Specific migration file(s) to lint (e.g. just the ones your PR adds).
    /// When given, the selection flags (`--all`, `--since`) and the directory
    /// scan are bypassed.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Directory of migrations.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Lint every migration, not just the most recent one.
    #[arg(long)]
    pub all: bool,

    /// Lint only migrations newer than this id (exclusive) — e.g. the base your
    /// PR branched from. Takes precedence over `--all`.
    #[arg(long, value_name = "ID")]
    pub since: Option<String>,

    /// Fail on warnings too (data-dependent / locking), not only destructive.
    #[arg(long)]
    pub strict: bool,
}

pub fn run(args: LintArgs) -> Result<()> {
    ui::header("molde lint · migration safety");

    // Explicit files win over any directory-based selection.
    let targets: Vec<Migration> = if !args.files.is_empty() {
        let mut loaded = Vec::with_capacity(args.files.len());
        for path in &args.files {
            loaded.push(
                migration::load_file(path)
                    .with_context(|| format!("reading migration {}", path.display()))?,
            );
        }
        loaded
    } else {
        let migrations = migration::load_dir(&args.migrations_dir).with_context(|| {
            format!("reading migrations from {}", args.migrations_dir.display())
        })?;
        if migrations.is_empty() {
            ui::warn(format!(
                "no migrations in {}",
                args.migrations_dir.display()
            ));
            return Ok(());
        }
        select_targets(migrations, args.all, args.since.as_deref())
    };

    if targets.is_empty() {
        ui::warn("no migrations matched the selection");
        return Ok(());
    }

    let mut destructive = 0usize;
    let mut warnings = 0usize;
    for m in &targets {
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

/// Pick which migrations to lint from the full, id-sorted set.
///
/// Precedence: `--since` (everything strictly newer than the given id) →
/// `--all` (everything) → default (just the latest, the one usually just
/// authored).
fn select_targets(migrations: Vec<Migration>, all: bool, since: Option<&str>) -> Vec<Migration> {
    if let Some(base) = since {
        return migrations
            .into_iter()
            .filter(|m| m.id.as_str() > base)
            .collect();
    }
    if all {
        return migrations;
    }
    migrations.into_iter().last().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::select_targets;
    use molde_core::migration::Migration;

    fn mig(id: &str) -> Migration {
        Migration::new(id, id, vec![], vec![])
    }

    fn ids(ms: Vec<Migration>) -> Vec<String> {
        ms.into_iter().map(|m| m.id).collect()
    }

    #[test]
    fn default_selects_latest_only() {
        let ms = vec![mig("20260101000000_a"), mig("20260102000000_b")];
        assert_eq!(ids(select_targets(ms, false, None)), ["20260102000000_b"]);
    }

    #[test]
    fn all_selects_everything() {
        let ms = vec![mig("20260101000000_a"), mig("20260102000000_b")];
        assert_eq!(
            ids(select_targets(ms, true, None)),
            ["20260101000000_a", "20260102000000_b"]
        );
    }

    #[test]
    fn since_selects_strictly_newer() {
        let ms = vec![
            mig("20260101000000_a"),
            mig("20260102000000_b"),
            mig("20260103000000_c"),
        ];
        assert_eq!(
            ids(select_targets(ms, false, Some("20260101000000_a"))),
            ["20260102000000_b", "20260103000000_c"]
        );
    }

    #[test]
    fn since_takes_precedence_over_all() {
        let ms = vec![mig("20260101000000_a"), mig("20260102000000_b")];
        assert_eq!(
            ids(select_targets(ms, true, Some("20260101000000_a"))),
            ["20260102000000_b"]
        );
    }
}
