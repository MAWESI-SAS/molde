//! `molde snapshot` — regenerate (or verify) the migration snapshot from the
//! `.model` files.
//!
//! The snapshot (`migrations/snapshot.json`) is the normalized serialization of
//! the model, so it is fully *derivable* from `models/`. That makes it safe to
//! regenerate instead of hand-merging: when two branches each authored a
//! migration they both rewrite the snapshot and git reports a conflict on that
//! one file. Pointing a git merge driver at `molde snapshot --output %A`
//! resolves it automatically by re-deriving the snapshot from the merged models.
//!
//! `--check` turns this into a CI gate: it verifies the committed snapshot
//! matches the models and exits non-zero if it is stale.

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use molde_core::snapshot;

use crate::commands::migrate::load_model_dir;
use crate::commands::ui;

#[derive(Args)]
pub struct SnapshotArgs {
    /// Directory with the `.model` files (the model source).
    #[arg(long, default_value = "models")]
    pub from_models: PathBuf,

    /// Where to write the snapshot. Defaults to `migrations/snapshot.json`.
    /// The git merge driver passes the conflicted file here (`%A`).
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Do not write; exit non-zero if the on-disk snapshot is stale (CI gate).
    #[arg(long)]
    pub check: bool,
}

pub fn run(args: SnapshotArgs) -> anyhow::Result<()> {
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from("migrations").join("snapshot.json"));

    let model = load_model_dir(&args.from_models)?;
    let canonical = snapshot::to_json(&model).context("serializing the snapshot")?;

    if args.check {
        ui::header("molde snapshot · check");
        let current = std::fs::read_to_string(&output).ok();
        match status(&canonical, current.as_deref()) {
            Status::UpToDate => {
                ui::ok(format!(
                    "{} is up to date with the models.",
                    output.display()
                ));
                Ok(())
            }
            Status::Stale => {
                ui::warn(format!(
                    "{} is STALE — regenerate it with `molde snapshot`.",
                    output.display()
                ));
                anyhow::bail!("snapshot is out of date with the models");
            }
            Status::Missing => {
                ui::warn(format!("{} does not exist.", output.display()));
                anyhow::bail!("snapshot file not found: {}", output.display());
            }
        }
    } else {
        ui::header("molde snapshot · regenerate");
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
        }
        std::fs::write(&output, &canonical)
            .with_context(|| format!("writing {}", output.display()))?;
        ui::ok(format!(
            "snapshot regenerated from models: {}",
            output.display()
        ));
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
enum Status {
    UpToDate,
    Stale,
    Missing,
}

/// Compare the freshly derived snapshot to what is on disk. A trailing-newline
/// difference is tolerated (the file is a generated artifact).
fn status(canonical: &str, current: Option<&str>) -> Status {
    match current {
        Some(c) if c.trim_end() == canonical.trim_end() => Status::UpToDate,
        Some(_) => Status::Stale,
        None => Status::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::{status, Status};

    #[test]
    fn status_detects_up_to_date_stale_and_missing() {
        let canon = "{\n  \"a\": 1\n}";
        assert_eq!(status(canon, Some(canon)), Status::UpToDate);
        // Trailing newline difference is tolerated.
        assert_eq!(status(canon, Some(&format!("{canon}\n"))), Status::UpToDate);
        assert_eq!(status(canon, Some("{\n  \"a\": 2\n}")), Status::Stale);
        assert_eq!(status(canon, None), Status::Missing);
    }
}
