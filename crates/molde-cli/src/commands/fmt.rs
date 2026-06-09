//! `molde fmt` — format `.model` files to their canonical form.
//!
//! Three modes:
//! - paths (files or directories): format in place, or only check with `--check`.
//! - `--stdin`: read from stdin and write the result to stdout (for editors).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

use crate::commands::ui;

#[derive(Args)]
#[command(
    after_long_help = r#"PURPOSE: format .model files to their canonical form (a formatter for the model language).
Structure only; touches no database. Note: canonicalizing expands authoring sugar (owns/enum/subtypes).

MODES:
  • paths (files/dirs; default models/) — reformat in place.
  • --check — do not write; exit non-zero if any file is not already formatted (CI gate).
  • --stdin — read from stdin, write the formatted result to stdout (for editors).

EXAMPLES:
  molde fmt
  molde fmt --check"#
)]
pub struct FmtArgs {
    /// `.model` files or directories to format. Defaults to `models/`.
    pub paths: Vec<PathBuf>,

    /// Do not write; exit non-zero if any file is not formatted.
    #[arg(long)]
    pub check: bool,

    /// Read content from stdin and write the formatted result to stdout.
    #[arg(long)]
    pub stdin: bool,

    /// File name used to infer the kind with `--stdin`
    /// (`database.model` = globals; anything else = entity).
    #[arg(long, default_value = "entity.model")]
    pub stdin_name: String,
}

pub fn run(args: FmtArgs) -> Result<()> {
    if args.stdin {
        return run_stdin(&args.stdin_name);
    }

    let paths = if args.paths.is_empty() {
        vec![PathBuf::from("models")]
    } else {
        args.paths.clone()
    };

    let mut files = Vec::new();
    for p in &paths {
        collect_models(p, &mut files)
            .with_context(|| format!("collecting .model files in {}", p.display()))?;
    }
    if files.is_empty() {
        ui::info("no .model files found.");
        return Ok(());
    }

    let mut changed = 0usize;
    let mut errors = 0usize;
    for path in &files {
        match format_one(path, args.check) {
            Ok(true) => {
                changed += 1;
                if args.check {
                    ui::warn(format!("not formatted: {}", path.display()));
                } else {
                    ui::ok(format!("formatted: {}", path.display()));
                }
            }
            Ok(false) => {}
            Err(e) => {
                errors += 1;
                ui::warn(format!("{}: {e}", path.display()));
            }
        }
    }

    if errors > 0 {
        anyhow::bail!("{errors} file(s) with parse errors");
    }
    if args.check {
        if changed > 0 {
            anyhow::bail!("{changed} file(s) not formatted");
        }
        ui::ok("all files are already formatted.");
    } else {
        ui::ok(format!(
            "{changed} file(s) reformatted out of {}.",
            files.len()
        ));
    }
    Ok(())
}

/// Format a single file. Returns `true` if the content changed (or would change
/// with `--check`).
fn format_one(path: &Path, check: bool) -> Result<bool> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("entity.model");
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let formatted = molde_lang::format_model(name, &src).map_err(anyhow::Error::new)?;
    if formatted == src {
        return Ok(false);
    }
    if !check {
        std::fs::write(path, &formatted).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(true)
}

fn run_stdin(name: &str) -> Result<()> {
    let mut src = String::new();
    std::io::stdin()
        .read_to_string(&mut src)
        .context("reading stdin")?;
    let formatted = molde_lang::format_model(name, &src).map_err(anyhow::Error::new)?;
    std::io::stdout()
        .write_all(formatted.as_bytes())
        .context("writing stdout")?;
    Ok(())
}

/// Gather the `.model` files from a path (single file or directory, non-recursive).
fn collect_models(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let p = entry?.path();
            if p.extension().and_then(|s| s.to_str()) == Some("model") {
                out.push(p);
            }
        }
    } else if path.extension().and_then(|s| s.to_str()) == Some("model") {
        out.push(path.to_path_buf());
    } else {
        anyhow::bail!("not a .model file or directory: {}", path.display());
    }
    Ok(())
}
