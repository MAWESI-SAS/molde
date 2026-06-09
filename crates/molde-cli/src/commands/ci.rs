//! `molde ci` — one command for pull-request gating.
//!
//! Runs the checks that should block a merge and emits a single Markdown report
//! suitable for posting as a PR comment:
//!
//! 1. **lint** — static safety analysis of the migrations (no database). Fails on
//!    destructive changes; with `--strict`, also on warnings.
//! 2. **snapshot** — `snapshot.json` must be up to date with the models.
//! 3. **verify** — *optional*: when `--connection` is given, apply every migration
//!    to that (ephemeral) database from scratch and check the result has no drift
//!    against the models. Skipped when no connection is provided.
//!
//! Exit code is non-zero if any check fails — that's the merge gate. The Markdown
//! report goes to stdout, and to `--report <path>` if given (for the workflow to
//! post).

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use molde_core::lint::{lint_operations, Severity};
use molde_core::migration::{self, Migration};
use molde_core::snapshot;
use molde_migrate::Migrator;
use molde_providers::Provider;

use crate::commands::migrate::load_model_dir;
use crate::commands::ui;
use crate::commands::verify::{drift_items, read_live_model};

#[derive(Args)]
#[command(
    after_long_help = r#"PURPOSE: run the merge-gating checks together and print a Markdown report (for a PR comment).
Exits non-zero if any check fails.

CHECKS:
  • lint     — static analysis of the migrations (no DB).
  • snapshot — migrations/snapshot.json must be up to date with models/.
  • verify   — ONLY when -c/--connection is given: apply every migration to that
               (ephemeral) database from scratch, then check it has no drift.
               Skipped when no connection is provided.

EXAMPLES:
  molde ci                                   # lint + snapshot (no DB; verify skipped)
  molde ci -c "$DATABASE_URL" --report ci.md # + from-scratch verify, save the report"#
)]
pub struct CiArgs {
    /// Directory with the `.model` files (the model source).
    #[arg(long, default_value = "models")]
    pub from_models: PathBuf,

    /// Directory of migrations.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Lint only migrations newer than this id (e.g. the base your PR branched
    /// from). By default every migration is linted.
    #[arg(long, value_name = "ID")]
    pub since: Option<String>,

    /// Treat lint warnings as failures too, not only destructive changes.
    #[arg(long)]
    pub strict: bool,

    /// Connection to an (ephemeral) database to apply-from-scratch and verify for
    /// drift. When omitted, the verify check is skipped. Defaults to `DATABASE_URL`.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine for `--connection`: inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Schema to read for the verify check (Postgres only). Defaults to `public`.
    #[arg(long)]
    pub schema: Option<String>,

    /// Also write the Markdown report to this file (for posting as a PR comment).
    #[arg(long, value_name = "PATH")]
    pub report: Option<PathBuf>,
}

/// Outcome of one check.
#[derive(Clone, Copy, PartialEq)]
enum Status {
    Pass,
    Fail,
    Skip,
}

impl Status {
    fn emoji(self) -> &'static str {
        match self {
            Status::Pass => "✅",
            Status::Fail => "❌",
            Status::Skip => "⏭️",
        }
    }
}

struct Check {
    name: &'static str,
    status: Status,
    /// One-line result shown in the summary table.
    summary: String,
    /// Optional bullet details (findings, drift items) shown in a collapsible block.
    details: Vec<String>,
}

pub fn run(args: CiArgs) -> anyhow::Result<()> {
    ui::header("molde ci · pull-request checks");

    let model = load_model_dir(&args.from_models)?;
    let migrations = migration::load_dir(&args.migrations_dir)
        .with_context(|| format!("reading migrations from {}", args.migrations_dir.display()))?;

    let checks = vec![
        check_lint(&migrations, args.since.as_deref(), args.strict),
        check_snapshot(&model, &args.migrations_dir),
        check_verify(&args, &model, &migrations),
    ];

    let report = render_report(&checks);
    println!("{report}");
    if let Some(path) = &args.report {
        std::fs::write(path, &report)
            .with_context(|| format!("writing the report to {}", path.display()))?;
        ui::info(format!("report written to {}", path.display()));
    }

    let failed: Vec<&str> = checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .map(|c| c.name)
        .collect();
    if !failed.is_empty() {
        anyhow::bail!("CI checks failed: {}", failed.join(", "));
    }
    ui::ok("all checks passed.");
    Ok(())
}

/// Lint check: select the migrations (all, or newer than `since`) and run the
/// static analyzer over their `up` operations.
fn check_lint(migrations: &[Migration], since: Option<&str>, strict: bool) -> Check {
    let targets: Vec<&Migration> = match since {
        Some(base) => migrations.iter().filter(|m| m.id.as_str() > base).collect(),
        None => migrations.iter().collect(),
    };

    let mut destructive = 0usize;
    let mut warnings = 0usize;
    let mut details = Vec::new();
    for m in &targets {
        for f in lint_operations(&m.up) {
            match f.severity {
                Severity::Destructive => destructive += 1,
                Severity::Warning => warnings += 1,
            }
            let tag = match f.severity {
                Severity::Destructive => "destructive",
                Severity::Warning => "warning",
            };
            details.push(format!(
                "`{}` [{tag}] {} ({}): {}",
                m.id, f.object, f.code, f.message
            ));
        }
    }

    let fail = destructive > 0 || (strict && warnings > 0);
    let status = if fail { Status::Fail } else { Status::Pass };
    let summary = format!(
        "{destructive} destructive, {warnings} warning(s) across {} migration(s)",
        targets.len()
    );
    Check {
        name: "lint",
        status,
        summary,
        details,
    }
}

/// Snapshot check: the committed `snapshot.json` must match the models.
fn check_snapshot(model: &molde_core::DatabaseModel, migrations_dir: &std::path::Path) -> Check {
    let path = migrations_dir.join("snapshot.json");
    let canonical = match snapshot::to_json(model) {
        Ok(c) => c,
        Err(e) => {
            return Check {
                name: "snapshot",
                status: Status::Fail,
                summary: format!("could not serialize the model: {e}"),
                details: Vec::new(),
            }
        }
    };
    let current = std::fs::read_to_string(&path).ok();
    let (status, summary) = match current {
        Some(c) if c.trim_end() == canonical.trim_end() => {
            (Status::Pass, format!("{} is up to date", path.display()))
        }
        Some(_) => (
            Status::Fail,
            format!("{} is stale — run `molde snapshot`", path.display()),
        ),
        None => (
            Status::Fail,
            format!("{} not found — run `molde snapshot`", path.display()),
        ),
    };
    Check {
        name: "snapshot",
        status,
        summary,
        details: Vec::new(),
    }
}

/// Verify check: apply every migration to a fresh database and confirm the result
/// has no drift against the models. Skipped when no connection is configured.
fn check_verify(
    args: &CiArgs,
    model: &molde_core::DatabaseModel,
    migrations: &[Migration],
) -> Check {
    let Some(connection) = args.connection.clone() else {
        return Check {
            name: "verify",
            status: Status::Skip,
            summary: "skipped (no --connection / DATABASE_URL)".to_string(),
            details: Vec::new(),
        };
    };

    match run_verify(&connection, args, model, migrations) {
        Ok(drift) if drift.is_empty() => Check {
            name: "verify",
            status: Status::Pass,
            summary: "no drift after applying every migration".to_string(),
            details: Vec::new(),
        },
        Ok(drift) => {
            let details = drift
                .iter()
                .map(|d| format!("{}: {}", d.direction.heading(), d.label))
                .collect();
            Check {
                name: "verify",
                status: Status::Fail,
                summary: format!("{} drift item(s) after applying migrations", drift.len()),
                details,
            }
        }
        Err(e) => Check {
            name: "verify",
            status: Status::Fail,
            summary: format!("error: {e:#}"),
            details: Vec::new(),
        },
    }
}

/// Apply all migrations to the database from scratch, then compute drift.
fn run_verify(
    connection: &str,
    args: &CiArgs,
    model: &molde_core::DatabaseModel,
    migrations: &[Migration],
) -> anyhow::Result<Vec<crate::commands::verify::DriftItem>> {
    let provider = match args.provider.as_deref() {
        Some(p) => Provider::parse(p).with_context(|| format!("unsupported provider: '{p}'"))?,
        None => Provider::from_url(connection)
            .context("could not infer the provider from the URL; use --provider")?,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the async runtime")?;

    runtime.block_on(async {
        let migrator = Migrator::connect(connection, provider)
            .await
            .context("connecting to the database")?;
        migrator
            .update(migrations, None)
            .await
            .context("applying migrations")?;
        let live = read_live_model(connection, provider, args.schema.as_deref()).await?;
        let generator = provider.generator();
        Ok(drift_items(&live, model, generator.as_ref()))
    })
}

/// Render the checks as a Markdown report for a PR comment.
fn render_report(checks: &[Check]) -> String {
    let mut out = String::new();
    out.push_str("## molde CI\n\n");
    out.push_str("| Check | Result |\n| --- | --- |\n");
    for c in checks {
        out.push_str(&format!(
            "| {} | {} {} |\n",
            c.name,
            c.status.emoji(),
            c.summary
        ));
    }

    let any_fail = checks.iter().any(|c| c.status == Status::Fail);
    out.push('\n');
    if any_fail {
        out.push_str("**Result: ❌ failed** — fix the items above before merging.\n");
    } else {
        out.push_str("**Result: ✅ passed**\n");
    }

    for c in checks {
        if c.details.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n<details><summary>{} {} details</summary>\n\n",
            c.status.emoji(),
            c.name
        ));
        for d in &c.details {
            out.push_str(&format!("- {d}\n"));
        }
        out.push_str("\n</details>\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &'static str, status: Status, summary: &str, details: Vec<String>) -> Check {
        Check {
            name,
            status,
            summary: summary.to_string(),
            details,
        }
    }

    #[test]
    fn report_marks_overall_pass_when_all_pass() {
        let checks = vec![
            check("lint", Status::Pass, "0 destructive", vec![]),
            check("snapshot", Status::Pass, "up to date", vec![]),
            check("verify", Status::Skip, "skipped", vec![]),
        ];
        let md = render_report(&checks);
        assert!(md.contains("Result: ✅ passed"));
        assert!(md.contains("| verify | ⏭️ skipped |"));
    }

    #[test]
    fn report_marks_overall_fail_and_lists_details() {
        let checks = vec![
            check(
                "lint",
                Status::Fail,
                "1 destructive",
                vec!["`m1` [destructive] Order (drop-table): drops table".to_string()],
            ),
            check("snapshot", Status::Pass, "up to date", vec![]),
        ];
        let md = render_report(&checks);
        assert!(md.contains("Result: ❌ failed"));
        assert!(md.contains("<details><summary>❌ lint details</summary>"));
        assert!(md.contains("drop-table"));
    }
}
