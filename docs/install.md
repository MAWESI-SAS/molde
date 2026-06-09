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
  https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.2-x86_64-unknown-linux-gnu.tar.gz
tar xzf molde.tar.gz
sudo mv molde-v0.0.2-x86_64-unknown-linux-gnu/molde /usr/local/bin/
molde --help
```

The **musl** archive is fully static and runs on any Linux (no glibc version
requirement) — handy for old distros, minimal containers, and **WSL on an older
Ubuntu**. The `-gnu` archive is built on a recent Ubuntu, so on an older glibc
(e.g. Ubuntu 20.04 / glibc 2.31, common in WSL) it fails with `GLIBC_2.xx not
found` — use the **musl** archive there:

```bash
curl -L -o molde.tar.gz \
  https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.2-x86_64-unknown-linux-musl.tar.gz
tar xzf molde.tar.gz
sudo install molde-v0.0.2-x86_64-unknown-linux-musl/molde /usr/local/bin/molde
molde --help
```

On macOS, use the `aarch64` archive for Apple Silicon and `x86_64` for Intel.

> **Connecting to a server with a legacy (X.509 v1) certificate?** The default
> builds use **rustls**, which rejects v1 certs (you'd see `UnsupportedCertVersion`
> /`invalid peer certificate`). Use the **`-nativetls`** Linux archive instead —
> `molde-v0.0.2-x86_64-unknown-linux-musl-nativetls.tar.gz` — a static musl build
> with a vendored OpenSSL that accepts those certs (still encrypted). Older
> self-hosted PostgreSQL often presents such certificates.

On **Windows** (`curl` and `tar` ship with Windows 10/11), from `cmd`:

```cmd
curl -L -o molde.zip https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.2-x86_64-pc-windows-msvc.zip
tar -xf molde.zip
mkdir "%USERPROFILE%\bin"
move molde-v0.0.2-x86_64-pc-windows-msvc\molde.exe "%USERPROFILE%\bin\molde.exe"
setx PATH "%PATH%;%USERPROFILE%\bin"
:: open a NEW cmd window, then:
molde --help
```

The same in **PowerShell**:

```powershell
Invoke-WebRequest -Uri https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.2-x86_64-pc-windows-msvc.zip -OutFile molde.zip
Expand-Archive molde.zip -DestinationPath .
New-Item -ItemType Directory -Force "$env:USERPROFILE\bin" | Out-Null
Move-Item molde-v0.0.2-x86_64-pc-windows-msvc\molde.exe "$env:USERPROFILE\bin\molde.exe" -Force
# add %USERPROFILE%\bin to PATH (Settings → Environment Variables), then in a new shell:
molde --help
```

> `setx` truncates a PATH longer than ~1024 chars; if yours is long, add the
> folder via *Settings → Environment Variables → Path* instead. Or skip PATH
> entirely and run it by full path: `"%USERPROFILE%\bin\molde.exe" --help`.

Prefer to build from source instead? Read on.

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
