# Installing molde

`molde` is a single self-contained binary (`molde`, or `molde.exe` on Windows).
There is no runtime to install — drop it on your `PATH` and go.

## Download a prebuilt binary (fastest)

Each release ships archives for Linux (glibc and static musl), macOS (Intel and
Apple Silicon) and Windows on the
[releases page](https://github.com/MAWESI-SAS/molde/releases/latest). Grab the
one for your platform, extract it, and put `molde` on your `PATH`:

```bash
# Linux x86_64 (glibc); see the releases page for other targets
curl -L -o molde.tar.gz \
  https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.1-x86_64-unknown-linux-gnu.tar.gz
tar xzf molde.tar.gz
sudo mv molde-v0.0.1-x86_64-unknown-linux-gnu/molde /usr/local/bin/
molde --help
```

The **musl** archive is fully static and runs on any Linux (no glibc version
requirement) — handy for old distros and minimal containers. On macOS, use the
`aarch64` archive for Apple Silicon and `x86_64` for Intel. Prefer to build from
source instead? Read on.

## What it needs to build

- **Rust toolchain** (stable), via [rustup](https://rustup.rs).
- **A C compiler** — molde bundles SQLite (through `sqlx`), which compiles a small
  amount of C at build time. This is the only native build dependency; everything
  else (including the Postgres, MySQL and SQL Server drivers and TLS) is pure Rust.
- **git** on `PATH` at runtime — only for `molde init-team` (merge driver + hooks).

TLS uses **rustls by default** (no OpenSSL, no system TLS libraries). You only need
the alternate backend for a server presenting a legacy X.509 v1 certificate that
rustls rejects — build with `--no-default-features --features tls-native-tls`
(that path *does* need the system TLS/OpenSSL dev libraries).

## The one-liner (any OS)

From a clone of this repo, with rustup installed:

```bash
cargo install --path crates/molde-cli
```

This builds in release mode and installs `molde` into `~/.cargo/bin` (already on
`PATH` after rustup). Verify:

```bash
molde --help
```

To build without installing, use `cargo build --release -p molde-cli`; the binary
is then at `target/release/molde` (`target\release\molde.exe` on Windows) and you
copy it wherever you want.

---

## Per-operating-system

### Linux

```bash
# 1. Toolchain + C compiler
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # rustup
sudo apt-get install -y build-essential                          # gcc (Debian/Ubuntu)
#   Fedora/RHEL:  sudo dnf install gcc
#   Arch:         sudo pacman -S base-devel

# 2. Build + install
cargo install --path crates/molde-cli
#   …or copy the binary to a system location:
cargo build --release -p molde-cli
sudo install -m 0755 target/release/molde /usr/local/bin/molde
```

### macOS (Intel and Apple Silicon)

```bash
# 1. Toolchain + C compiler (Xcode Command Line Tools)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
xcode-select --install        # provides clang; skip if Xcode is installed

# 2. Build + install (rustup picks aarch64-apple-darwin or x86_64-apple-darwin)
cargo install --path crates/molde-cli
#   …or:
cargo build --release -p molde-cli
sudo install -m 0755 target/release/molde /usr/local/bin/molde
```

### Windows

```powershell
# 1. Install rustup from https://rustup.rs (the installer offers to install the
#    Microsoft C++ Build Tools — accept it; they provide the MSVC C compiler).

# 2. Build + install (produces molde.exe)
cargo install --path crates/molde-cli
#   `molde.exe` lands in %USERPROFILE%\.cargo\bin, which rustup adds to PATH.

# Or build without installing:
cargo build --release -p molde-cli
#   binary at target\release\molde.exe — copy it to a folder on your PATH.
```

> The default target is `x86_64-pc-windows-msvc`. The GNU target
> (`x86_64-pc-windows-gnu`) also works if you prefer MinGW.

---

## Without a local Rust toolchain

This repo's host machine has no `cargo`; builds run in Docker. The same works on
any machine with Docker:

```bash
docker run --rm -v "$PWD":/app -w /app rust:1-bookworm \
  cargo build --release -p molde-cli
# → target/release/molde   (a Linux glibc binary)
```

Or open the repo in the bundled **Dev Container** (`.devcontainer/`, VS Code →
*"Reopen in Container"*) which has Rust ready, and run `cargo install --path
crates/molde-cli` inside it.

---

## Distributing one binary to a team

Each teammate can `cargo install` as above. To hand out a prebuilt binary instead:

- **Build natively on each OS** (simplest, most reliable) — a CI matrix
  (e.g. GitHub Actions on `ubuntu-latest`, `macos-latest`, `windows-latest`)
  produces all three from one push.
- **One portable Linux binary** — a static [musl](https://musl.libc.org) build runs
  on any Linux distro regardless of its glibc:

  ```bash
  rustup target add x86_64-unknown-linux-musl
  # The bundled SQLite needs a musl C toolchain (e.g. `musl-tools` on Debian),
  # so the easiest route is the Docker-based `cross`:
  cargo install cross
  cross build --release -p molde-cli --target x86_64-unknown-linux-musl
  # → target/x86_64-unknown-linux-musl/release/molde  (statically linked)
  ```

- **Cross-compiling between OSes** (e.g. Windows from Linux) is fiddly by hand;
  [`cross`](https://github.com/cross-rs/cross) or per-OS CI runners are the
  practical options.

A built binary depends only on the target OS's system libc (for the default glibc
Linux/macOS/Windows builds) — no other runtime. Strip it with `strip molde` to
shrink it (~17 MB → smaller) if size matters.

---

## Running it

`molde` is invoked the same way everywhere:

```bash
molde --help                                   # list commands
molde pull   --connection "$DATABASE_URL"      # DB → models/
molde migrate AddInvoices                      # models/ → a migration
molde apply  --connection "$DATABASE_URL"      # apply migrations
molde verify --connection "$DATABASE_URL" --check   # drift check
```

- The **engine** is inferred from the connection URL (`postgres://…`, `mysql://…`,
  `sqlite:…`) or set with `--provider`. **SQL Server** uses an ADO string and is
  detected automatically:
  `Server=host,1433;Database=db;User Id=sa;Password=***;TrustServerCertificate=true`.
- `--connection` defaults to the `DATABASE_URL` environment variable; commands
  prompt for anything missing unless `--no-input` is given. `--yes` skips
  confirmations.

For a team setup (each developer with their own local database), see
[team-database-workflow.md](team-database-workflow.md).
