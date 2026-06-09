//! `molde update` — self-update to the latest GitHub release.
//!
//! Fetches the latest release of MAWESI-SAS/molde, downloads the archive that
//! matches THIS build's platform and TLS variant, and atomically replaces the
//! running binary. The download always talks to GitHub over rustls (GitHub has a
//! modern certificate); molde's own `tls-native-tls` build affects only which
//! release asset is selected, not how the update is fetched.

use anyhow::{Context, Result};
use clap::Args;

use crate::commands::ui;

#[derive(Args)]
#[command(
    after_long_help = r#"PURPOSE: replace the installed `molde` with the latest GitHub release for this exact
platform and build variant. Atomic self-replace; no package manager needed.

BEHAVIOR:
  • Compares the running version against the latest release; if already current, does nothing.
  • Picks the asset matching this target triple and TLS variant (a native-tls build pulls the
    native-tls asset; otherwise the default rustls asset).
  • The download itself uses rustls against GitHub (independent of molde's DB TLS).

NOTES:
  • Needs write permission to the installed binary's location (e.g. run with sudo if it
    lives in /usr/local/bin).
  • `--check` only reports whether a newer version exists; it does not modify anything.

EXAMPLES:
  molde update
  molde update --check"#
)]
pub struct UpdateArgs {
    /// Only report whether a newer version is available; do not modify anything.
    #[arg(long)]
    pub check: bool,
}

/// The release-asset target triple for THIS build, computed from `cfg!`
/// (`self_update::get_target()` doesn't detect musl and returns the gnu triple).
fn asset_triple() -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let os_part = if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_env = "musl") {
        "unknown-linux-musl"
    } else {
        "unknown-linux-gnu"
    };
    format!("{arch}-{os_part}")
}

/// `Some("nativetls")` for native-tls builds — used as the asset *identifier* so
/// `Release::asset_for` (which checks the identifier in every match branch)
/// selects the `…-nativetls.…` archive instead of the default one.
fn asset_identifier() -> Option<&'static str> {
    if cfg!(feature = "tls-native-tls") {
        Some("nativetls")
    } else {
        None
    }
}

pub fn run(args: UpdateArgs) -> Result<()> {
    ui::header("molde update");
    let current = self_update::cargo_crate_version!();
    let target = asset_triple();

    if args.check {
        let releases = self_update::backends::github::ReleaseList::configure()
            .repo_owner("MAWESI-SAS")
            .repo_name("molde")
            .build()
            .context("configuring the release lookup")?
            .fetch()
            .context("fetching the release list from GitHub")?;
        match releases.first() {
            Some(latest) if latest.version != current => {
                ui::info(format!(
                    "a newer version is available: {current} -> {}",
                    latest.version
                ));
                ui::info("run `molde update` to install it.");
            }
            Some(_) => ui::ok(format!("already on the latest version ({current}).")),
            None => ui::warn("no releases found."),
        }
        return Ok(());
    }

    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner("MAWESI-SAS")
        .repo_name("molde")
        .bin_name("molde")
        .target(&target)
        .current_version(current)
        .show_download_progress(true)
        .no_confirm(true);
    if let Some(id) = asset_identifier() {
        builder.identifier(id);
    }
    let status = builder
        .build()
        .context("configuring the updater")?
        .update()
        .context("updating molde (need write access to the binary? try sudo)")?;

    if status.updated() {
        ui::ok(format!("updated molde {current} -> {}.", status.version()));
    } else {
        ui::ok(format!("already on the latest version ({current})."));
    }
    Ok(())
}
