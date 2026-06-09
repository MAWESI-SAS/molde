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

/// A substring unique to THIS build's asset within its OS/ARCH group, passed to
/// `self_update` as the asset *identifier*.
///
/// `Release::asset_for` matches `name.contains(target) || (name.contains(OS) &&
/// name.contains(ARCH))`, then ANDs an optional `identifier`. On **Linux** the
/// `OS && ARCH` fallback (`linux` && `x86_64`) matches all three linux assets
/// (gnu, musl, musl-nativetls), and `x86_64-unknown-linux-musl` is itself a
/// substring of the `…-musl-nativetls` name — so a bare triple can resolve to
/// the wrong archive (even the glibc one). A unique identifier is the only lever
/// that defeats that fallback, so every linux variant supplies one:
///   • native-tls → `nativetls`  (only the `…-musl-nativetls.…` asset)
///   • rustls musl → `-musl.`     (the trailing dot excludes `…-musl-nativetls`)
///   • rustls gnu  → `-gnu`       (absent from both musl assets)
///
/// macOS and Windows asset names are already unambiguous (the two darwin triples
/// aren't substrings of each other; the `OS` const `macos` doesn't appear in
/// `apple-darwin`; Windows ships a single asset), so they need no identifier.
fn asset_identifier() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        if cfg!(feature = "tls-native-tls") {
            Some("nativetls")
        } else if cfg!(target_env = "musl") {
            Some("-musl.")
        } else {
            Some("-gnu")
        }
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

#[cfg(test)]
mod tests {
    /// Mirror of `self_update::Release::asset_for`'s predicate (0.42): an asset
    /// matches when its name contains the target triple OR both the OS and ARCH
    /// constants, and — if given — the identifier. Kept in lockstep with the
    /// real crate so these tests actually prove our selection is unambiguous.
    fn asset_matches(
        name: &str,
        target: &str,
        identifier: Option<&str>,
        os: &str,
        arch: &str,
    ) -> bool {
        (name.contains(target) || (name.contains(os) && name.contains(arch)))
            && identifier.is_none_or(|i| name.contains(i))
    }

    /// Every release asset published by `.github/workflows/release.yml`.
    const ASSETS: &[&str] = &[
        "molde-v9.9.9-x86_64-unknown-linux-gnu.tar.gz",
        "molde-v9.9.9-x86_64-unknown-linux-musl.tar.gz",
        "molde-v9.9.9-x86_64-unknown-linux-musl-nativetls.tar.gz",
        "molde-v9.9.9-x86_64-apple-darwin.tar.gz",
        "molde-v9.9.9-aarch64-apple-darwin.tar.gz",
        "molde-v9.9.9-x86_64-pc-windows-msvc.zip",
    ];

    /// One build variant: the `(target, identifier)` it computes (mirroring
    /// `asset_triple`/`asset_identifier`), the `std::env::consts` OS/ARCH on that
    /// platform, and the single asset it must resolve to.
    struct Case {
        label: &'static str,
        target: &'static str,
        identifier: Option<&'static str>,
        os: &'static str,
        arch: &'static str,
        expected: &'static str,
    }

    const CASES: &[Case] = &[
        Case {
            label: "linux-gnu",
            target: "x86_64-unknown-linux-gnu",
            identifier: Some("-gnu"),
            os: "linux",
            arch: "x86_64",
            expected: "molde-v9.9.9-x86_64-unknown-linux-gnu.tar.gz",
        },
        Case {
            label: "linux-musl-rustls",
            target: "x86_64-unknown-linux-musl",
            identifier: Some("-musl."),
            os: "linux",
            arch: "x86_64",
            expected: "molde-v9.9.9-x86_64-unknown-linux-musl.tar.gz",
        },
        Case {
            label: "linux-musl-nativetls",
            target: "x86_64-unknown-linux-musl",
            identifier: Some("nativetls"),
            os: "linux",
            arch: "x86_64",
            expected: "molde-v9.9.9-x86_64-unknown-linux-musl-nativetls.tar.gz",
        },
        Case {
            label: "macos-x86_64",
            target: "x86_64-apple-darwin",
            identifier: None,
            os: "macos",
            arch: "x86_64",
            expected: "molde-v9.9.9-x86_64-apple-darwin.tar.gz",
        },
        Case {
            label: "macos-aarch64",
            target: "aarch64-apple-darwin",
            identifier: None,
            os: "macos",
            arch: "aarch64",
            expected: "molde-v9.9.9-aarch64-apple-darwin.tar.gz",
        },
        Case {
            label: "windows-x86_64",
            target: "x86_64-pc-windows-msvc",
            identifier: None,
            os: "windows",
            arch: "x86_64",
            expected: "molde-v9.9.9-x86_64-pc-windows-msvc.zip",
        },
    ];

    #[test]
    fn each_build_variant_selects_exactly_one_asset() {
        for case in CASES {
            let matched: Vec<&str> = ASSETS
                .iter()
                .copied()
                .filter(|name| {
                    asset_matches(name, case.target, case.identifier, case.os, case.arch)
                })
                .collect();
            assert_eq!(
                matched,
                vec![case.expected],
                "variant `{}` must resolve to exactly one asset",
                case.label
            );
        }
    }
}
