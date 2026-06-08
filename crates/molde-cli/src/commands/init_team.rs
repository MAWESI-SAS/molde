//! `molde init-team` — wire a project for the team database workflow.
//!
//! Installs the snapshot **merge driver** so concurrent migrations that both
//! rewrite `migrations/snapshot.json` auto-resolve by regenerating it from the
//! merged models (see `docs/team-database-workflow.md` §7.2), instead of a manual
//! merge. This is two pieces:
//!
//! - a committed `.gitattributes` line that routes the snapshot through a named
//!   merge driver, and
//! - a local `.git/config` registration of that driver (`molde snapshot --output
//!   %A`). The registration is per-clone — git does not share `.git/config` — so
//!   every teammate runs `molde init-team` once after cloning.
//!
//! Optionally writes a CI template (`--ci github`) that runs the workflow's gates.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::commands::ui;

const GITATTRIBUTES_LINE: &str = "migrations/snapshot.json merge=molde-snapshot";
const DRIVER_KEY: &str = "merge.molde-snapshot";
const HOOK_MARKER: &str = "molde-init-team";

const POST_MERGE_HOOK: &str = r#"#!/bin/sh
# molde-init-team: keep migrations/snapshot.json in sync after a merge.
# The merge driver runs mid-merge (other-side files may not be in the tree yet),
# so re-derive the snapshot now that the working tree has settled.
command -v molde >/dev/null 2>&1 || exit 0
[ -d models ] && [ -f migrations/snapshot.json ] || exit 0
molde snapshot >/dev/null 2>&1 || exit 0
if ! git diff --quiet -- migrations/snapshot.json 2>/dev/null; then
  git add -- migrations/snapshot.json 2>/dev/null || true
  echo "molde: regenerated migrations/snapshot.json from the merged models (staged — commit it)."
fi
"#;

#[derive(Args)]
pub struct InitTeamArgs {
    /// Repository root (defaults to the current directory).
    #[arg(long, default_value = ".")]
    pub path: PathBuf,

    /// Also write a CI template for this provider (currently: `github`).
    #[arg(long)]
    pub ci: Option<String>,

    /// Overwrite an existing CI template if it differs.
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: InitTeamArgs) -> Result<()> {
    ui::header("molde init-team · snapshot merge driver");
    let root = &args.path;

    let git_dir = ensure_git_repo(root)?;
    ensure_gitattributes(root)?;
    register_merge_driver(root)?;
    install_post_merge_hook(&git_dir, args.force)?;

    if let Some(provider) = args.ci.as_deref() {
        match provider {
            "github" => write_github_ci(root, args.force)?,
            other => bail!("unsupported --ci provider: '{other}' (supported: github)"),
        }
    }

    ui::ok("set up. Snapshot conflicts now resolve without a manual merge.");
    ui::info("Each teammate runs `molde init-team` once per clone (the merge driver and");
    ui::info("hook live under .git, which git does not share). `molde` must be on PATH.");
    Ok(())
}

/// Confirm `root` is a work tree and return its `.git` directory (resolving
/// `--git-dir`, so it works in worktrees/submodules too).
fn ensure_git_repo(root: &Path) -> Result<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .context("running git (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!(
            "{} is not a git repository — run this from your project root",
            root.display()
        );
    }
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(dir))
}

/// Append the `.gitattributes` line if it is not already present.
fn ensure_gitattributes(root: &Path) -> Result<()> {
    let path = root.join(".gitattributes");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == GITATTRIBUTES_LINE) {
        ui::info(".gitattributes already routes the snapshot — left as is.");
        return Ok(());
    }
    let mut contents = existing;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(GITATTRIBUTES_LINE);
    contents.push('\n');
    std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    ui::ok(format!(
        "{}: routed the snapshot to the merge driver.",
        path.display()
    ));
    Ok(())
}

/// Register the merge driver in the repo's local git config.
fn register_merge_driver(root: &Path) -> Result<()> {
    git_config(
        root,
        &format!("{DRIVER_KEY}.name"),
        "regenerate molde snapshot from models",
    )?;
    git_config(
        root,
        &format!("{DRIVER_KEY}.driver"),
        "molde snapshot --output %A",
    )?;
    ui::ok("registered the `molde-snapshot` merge driver in .git/config.");
    Ok(())
}

fn git_config(root: &Path, key: &str, value: &str) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--local", key, value])
        .status()
        .context("running git config")?;
    if !status.success() {
        bail!("git config {key} failed");
    }
    Ok(())
}

/// Install a `post-merge` hook that regenerates the snapshot from the fully
/// merged tree. The merge driver runs *during* the merge, when files from the
/// other side may not be in the working tree yet, so its snapshot can be stale;
/// this hook re-derives it once the merge has settled and stages the fix.
fn install_post_merge_hook(git_dir: &Path, force: bool) -> Result<()> {
    let hooks_dir = git_dir.join("hooks");
    let path = hooks_dir.join("post-merge");
    if path.exists() && !force {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if existing.contains(HOOK_MARKER) {
            ui::info("post-merge hook already installed — left as is.");
        } else {
            ui::warn(format!(
                "{} exists (not molde's) — left as is. Add `molde snapshot` to it, \
                 or re-run with --force.",
                path.display()
            ));
        }
        return Ok(());
    }
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating {}", hooks_dir.display()))?;
    std::fs::write(&path, POST_MERGE_HOOK)
        .with_context(|| format!("writing {}", path.display()))?;
    set_executable(&path)?;
    ui::ok("installed a post-merge hook that keeps the snapshot in sync.");
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("chmod +x {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn write_github_ci(root: &Path, force: bool) -> Result<()> {
    let dir = root.join(".github").join("workflows");
    let path = dir.join("molde-schema.yml");
    if path.exists() && !force {
        ui::warn(format!(
            "{} already exists — left as is (use --force to overwrite).",
            path.display()
        ));
        return Ok(());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&path, GITHUB_CI_TEMPLATE)
        .with_context(|| format!("writing {}", path.display()))?;
    ui::ok(format!(
        "{}: wrote a CI template (adjust the DB + molde install).",
        path.display()
    ));
    Ok(())
}

const GITHUB_CI_TEMPLATE: &str = r#"# molde schema checks — generated by `molde init-team` (a template; adjust to your setup).
name: molde schema
on:
  pull_request:
    paths:
      - "models/**"
      - "migrations/**"

jobs:
  schema:
    runs-on: ubuntu-latest
    # TODO: add a database service for the fresh-apply + verify steps, e.g.:
    # services:
    #   postgres:
    #     image: postgres:16
    #     env:
    #       POSTGRES_PASSWORD: postgres
    #     ports: ["5432:5432"]
    env:
      DATABASE_URL: "postgres://postgres:postgres@localhost:5432/molde_ci"
    steps:
      - uses: actions/checkout@v4

      # TODO: install the `molde` binary (build from source or download a release)
      #       and make sure it is on PATH for the steps below.

      - name: Models are canonical
        run: molde fmt --check models

      - name: Snapshot matches the models
        run: molde snapshot --check

      - name: Migrations apply cleanly on a fresh database
        run: molde apply --yes --no-input

      - name: Database has no drift from the model
        run: molde verify --check --no-input
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_stay_consistent() {
        // The hook must be detectable (idempotency) and a valid shell script.
        assert!(POST_MERGE_HOOK.starts_with("#!/bin/sh"));
        assert!(POST_MERGE_HOOK.contains(HOOK_MARKER));
        assert!(POST_MERGE_HOOK.contains("molde snapshot"));
        // The .gitattributes line and the driver key name the same driver.
        assert!(GITATTRIBUTES_LINE.contains("merge=molde-snapshot"));
        assert_eq!(DRIVER_KEY, "merge.molde-snapshot");
        // The CI template runs every gate from the workflow doc.
        for gate in [
            "fmt --check",
            "snapshot --check",
            "molde apply",
            "verify --check",
        ] {
            assert!(
                GITHUB_CI_TEMPLATE.contains(gate),
                "CI template missing: {gate}"
            );
        }
    }
}
