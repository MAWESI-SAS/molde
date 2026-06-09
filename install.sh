#!/bin/sh
# molde installer — downloads the latest prebuilt binary for this OS/architecture
# and puts it on your PATH. No Rust toolchain required.
#
#   curl -fsSL https://raw.githubusercontent.com/MAWESI-SAS/molde/main/install.sh | sh
#
# Environment overrides:
#   MOLDE_INSTALL_DIR   target directory (default: /usr/local/bin if writable,
#                       otherwise $HOME/.local/bin)
#   MOLDE_VERSION       a specific release tag, e.g. v0.0.5 (default: latest)
#   MOLDE_TLS           set to "nativetls" for the Linux build that accepts
#                       legacy X.509 v1 certificates (default: rustls)

set -eu

REPO="MAWESI-SAS/molde"
BIN="molde"

# --- tiny output helpers -----------------------------------------------------
if [ -t 1 ]; then BOLD="$(printf '\033[1m')"; RED="$(printf '\033[31m')"; YEL="$(printf '\033[33m')"; GRN="$(printf '\033[32m')"; RST="$(printf '\033[0m')"; else BOLD=""; RED=""; YEL=""; GRN=""; RST=""; fi
info() { printf '%s==>%s %s\n' "$GRN" "$RST" "$1"; }
warn() { printf '%swarning:%s %s\n' "$YEL" "$RST" "$1" >&2; }
err()  { printf '%serror:%s %s\n' "$RED" "$RST" "$1" >&2; exit 1; }

# --- pick a downloader -------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
  dl() { curl -fSL "$1" -o "$2"; }
  dl_stdout() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
  dl() { wget -qO "$2" "$1"; }
  dl_stdout() { wget -qO- "$1"; }
else
  err "neither curl nor wget found; install one and re-run."
fi

# --- detect platform ---------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)  os_part="unknown-linux-musl"; ext="tar.gz" ;;  # static musl: runs on any glibc
  Darwin) os_part="apple-darwin";       ext="tar.gz" ;;
  *) err "unsupported OS '$os'. This installer covers Linux and macOS; on Windows run install.ps1 (see docs/install.md)." ;;
esac

case "$arch" in
  x86_64|amd64)  arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *) err "unsupported architecture '$arch'." ;;
esac

# Linux releases are x86_64 only; macOS ships both.
if [ "$os" = "Linux" ] && [ "$arch_part" != "x86_64" ]; then
  err "no prebuilt $arch Linux binary yet. Build from source: see docs/install.md."
fi

suffix=""
if [ "$os" = "Linux" ] && [ "${MOLDE_TLS:-}" = "nativetls" ]; then
  suffix="-nativetls"
fi

triple="${arch_part}-${os_part}"

# --- resolve the release tag -------------------------------------------------
tag="${MOLDE_VERSION:-}"
if [ -z "$tag" ]; then
  info "looking up the latest release..."
  tag="$(dl_stdout "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -n1 | cut -d'"' -f4)"
  [ -n "$tag" ] || err "could not determine the latest version (GitHub API rate limit? set MOLDE_VERSION)."
fi

asset="${BIN}-${tag}-${triple}${suffix}.${ext}"
url="https://github.com/$REPO/releases/download/$tag/$asset"

# --- download & extract ------------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
info "downloading ${BOLD}${asset}${RST}"
dl "$url" "$tmp/$asset" || err "download failed: $url"
tar xzf "$tmp/$asset" -C "$tmp" || err "could not extract $asset"
[ -f "$tmp/$BIN" ] || err "archive did not contain a '$BIN' binary."
chmod +x "$tmp/$BIN"

# --- choose an install dir ---------------------------------------------------
if [ -n "${MOLDE_INSTALL_DIR:-}" ]; then
  dir="$MOLDE_INSTALL_DIR"
elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  dir="/usr/local/bin"
else
  dir="$HOME/.local/bin"
fi
mkdir -p "$dir" || err "cannot create install dir '$dir'."
mv "$tmp/$BIN" "$dir/$BIN" || err "cannot write to '$dir' (set MOLDE_INSTALL_DIR, or re-run with the right permissions)."

# --- report ------------------------------------------------------------------
ver="$("$dir/$BIN" --version 2>/dev/null || echo "$tag")"
info "installed ${BOLD}${ver}${RST} to ${BOLD}${dir}/${BIN}${RST}"

case ":$PATH:" in
  *":$dir:"*) ;;
  *) warn "$dir is not on your PATH. Add it, e.g.:"
     printf '       export PATH="%s:$PATH"\n' "$dir" >&2 ;;
esac

printf '%sRun `molde --help` to get started.%s\n' "$BOLD" "$RST"
