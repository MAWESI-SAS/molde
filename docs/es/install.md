# Instalar molde

`molde` es un binario único autocontenido (`molde`, o `molde.exe` en Windows).
No hay que instalar ningún runtime — solo colócalo en tu `PATH` y listo.

## Instalación (un solo comando) — no requiere Rust

El instalador detecta tu sistema operativo y arquitectura, descarga el binario
prebuilt correspondiente desde el último release, y lo coloca en tu `PATH`.

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/MAWESI-SAS/molde/main/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/MAWESI-SAS/molde/main/install.ps1 | iex
```

Luego verifica:

```bash
molde --version
```

El instalador de Linux usa el build **static musl**, que funciona en cualquier
Linux sin importar su versión de glibc (distros antiguas, contenedores mínimos,
WSL). Instala en `/usr/local/bin` cuando es escribible, o en `~/.local/bin` en
caso contrario.

Perillas (variables de entorno):

| Variable | Propósito |
| --- | --- |
| `MOLDE_INSTALL_DIR` | Instalar en otro lugar, por ejemplo `MOLDE_INSTALL_DIR=~/bin`. |
| `MOLDE_VERSION` | Fijar un release, por ejemplo `MOLDE_VERSION=v0.0.5`. |
| `MOLDE_TLS=nativetls` | Solo Linux — obtiene el build que acepta certificados X.509 v1 legacy (ver la nota más abajo). |

¿Ya lo tienes instalado? `molde update` se actualiza a sí mismo al último
release (ver [Actualización](#updating)).

## Descargar un binario prebuilt (manual)

Cada release publica archivos comprimidos para Linux (glibc y static musl),
macOS (Intel y Apple Silicon) y Windows en la
[página de releases](https://github.com/MAWESI-SAS/molde/releases/latest).
Toma el que corresponda a tu plataforma, extráelo, y coloca `molde` en tu
`PATH`:

```bash
# Linux x86_64 (glibc); see the releases page for other targets
curl -L -o molde.tar.gz \
  https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.5-x86_64-unknown-linux-gnu.tar.gz
tar xzf molde.tar.gz
sudo install molde /usr/local/bin/molde
molde --help
```

El archivo **musl** es totalmente estático y corre en cualquier Linux (sin
requisito de versión de glibc) — útil para distros antiguas, contenedores
mínimos, y **WSL sobre un Ubuntu antiguo**. El archivo `-gnu` se compila sobre
un Ubuntu reciente, así que en un glibc más antiguo (por ejemplo Ubuntu 20.04 /
glibc 2.31, común en WSL) falla con `GLIBC_2.xx not found` — usa el archivo
**musl** en ese caso:

```bash
curl -L -o molde.tar.gz \
  https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.5-x86_64-unknown-linux-musl.tar.gz
tar xzf molde.tar.gz
sudo install molde /usr/local/bin/molde
molde --help
```

En macOS, usa el archivo `aarch64` para Apple Silicon y `x86_64` para Intel.

> **¿Te conectas a un servidor con un certificado legacy (X.509 v1)?** Los
> builds por defecto usan **rustls**, que rechaza los certificados v1 (verías
> `UnsupportedCertVersion` / `invalid peer certificate`). Usa el archivo Linux
> **`-nativetls`** en su lugar —
> `molde-v0.0.5-x86_64-unknown-linux-musl-nativetls.tar.gz` — un build musl
> estático con un OpenSSL vendorizado que acepta esos certificados (siguen
> estando cifrados). Es común que un PostgreSQL autoalojado antiguo presente
> este tipo de certificados.

En **Windows** (`curl` y `tar` vienen incluidos en Windows 10/11), desde `cmd`:

```cmd
curl -L -o molde.zip https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.5-x86_64-pc-windows-msvc.zip
tar -xf molde.zip
mkdir "%USERPROFILE%\bin"
move molde.exe "%USERPROFILE%\bin\molde.exe"
setx PATH "%PATH%;%USERPROFILE%\bin"
:: open a NEW cmd window, then:
molde --help
```

Lo mismo en **PowerShell**:

```powershell
Invoke-WebRequest -Uri https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.5-x86_64-pc-windows-msvc.zip -OutFile molde.zip
Expand-Archive molde.zip -DestinationPath .
New-Item -ItemType Directory -Force "$env:USERPROFILE\bin" | Out-Null
Move-Item molde.exe "$env:USERPROFILE\bin\molde.exe" -Force
# add %USERPROFILE%\bin to PATH (Settings → Environment Variables), then in a new shell:
molde --help
```

> `setx` trunca un PATH más largo que ~1024 caracteres; si el tuyo es largo,
> agrega la carpeta desde *Settings → Environment Variables → Path* en su
> lugar. O evita el PATH por completo y ejecútalo por su ruta completa:
> `"%USERPROFILE%\bin\molde.exe" --help`.

¿Prefieres compilar desde el código fuente? Sigue leyendo.

## Actualización

Una vez que molde está instalado, se actualiza a sí mismo desde el último
release de GitHub:

```bash
molde update          # download the latest release for this platform and replace the binary
molde update --check  # just report whether a newer version exists
```

Elige el archivo correspondiente a tu plataforma y variante de TLS
automáticamente (una instalación native-tls descarga el asset native-tls). Si
el binario vive en una ruta del sistema, ejecútalo con `sudo` para que pueda
reemplazarse a sí mismo.

## Lo que necesita para compilarse

- **Toolchain de Rust** (stable), vía [rustup](https://rustup.rs).
- **Un compilador de C** — molde empaqueta SQLite (a través de `sqlx`), que
  compila una pequeña cantidad de C al momento de la compilación. Esta es la
  única dependencia nativa de compilación; todo lo demás (incluyendo los
  drivers de Postgres, MySQL y SQL Server, y TLS) es Rust puro.
- **git** en el `PATH` en tiempo de ejecución — solo para `molde init-team`
  (merge driver + hooks).

TLS usa **rustls por defecto** (sin OpenSSL, sin librerías de TLS del
sistema). Solo necesitas el backend alternativo para un servidor que presente
un certificado X.509 v1 legacy que rustls rechaza — compílalo con
`--no-default-features --features tls-native-tls` (esa ruta *sí* necesita las
librerías de desarrollo de TLS/OpenSSL del sistema).

## Compilar desde un clon (requiere Rust)

Desde un clon de este repositorio, con rustup instalado:

```bash
cargo install --path crates/molde-cli
```

Esto compila en modo release e instala `molde` en `~/.cargo/bin` (ya está en
el `PATH` después de rustup). Verifica:

```bash
molde --help
```

Para compilar sin instalar, usa `cargo build --release -p molde-cli`; el
binario queda entonces en `target/release/molde` (`target\release\molde.exe`
en Windows) y lo copias adonde quieras.

---

## Por sistema operativo

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

### macOS (Intel y Apple Silicon)

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

> El target por defecto es `x86_64-pc-windows-msvc`. El target GNU
> (`x86_64-pc-windows-gnu`) también funciona si prefieres MinGW.

---

## Sin un toolchain de Rust local

La máquina que aloja este repositorio no tiene `cargo`; las compilaciones
corren en Docker. Lo mismo funciona en cualquier máquina con Docker:

```bash
docker run --rm -v "$PWD":/app -w /app rust:1-bookworm \
  cargo build --release -p molde-cli
# → target/release/molde   (a Linux glibc binary)
```

O abre el repositorio en el **Dev Container** incluido (`.devcontainer/`, VS
Code → *"Reopen in Container"*), que ya tiene Rust listo, y ejecuta `cargo
install --path crates/molde-cli` dentro de él.

---

## Distribuir un binario a un equipo

Cada integrante del equipo puede hacer `cargo install` como se mostró arriba.
Para entregar un binario prebuilt en su lugar:

- **Compilar nativamente en cada sistema operativo** (lo más simple y
  confiable) — una matriz de CI (por ejemplo GitHub Actions sobre
  `ubuntu-latest`, `macos-latest`, `windows-latest`) produce los tres desde un
  solo push.
- **Un binario Linux portable** — un build [musl](https://musl.libc.org)
  estático corre en cualquier distro de Linux sin importar su glibc:

  ```bash
  rustup target add x86_64-unknown-linux-musl
  # The bundled SQLite needs a musl C toolchain (e.g. `musl-tools` on Debian),
  # so the easiest route is the Docker-based `cross`:
  cargo install cross
  cross build --release -p molde-cli --target x86_64-unknown-linux-musl
  # → target/x86_64-unknown-linux-musl/release/molde  (statically linked)
  ```

- **Compilar de forma cruzada entre sistemas operativos** (por ejemplo Windows
  desde Linux) es engorroso a mano; [`cross`](https://github.com/cross-rs/cross)
  o runners de CI por sistema operativo son las opciones prácticas.

Un binario compilado depende únicamente de la libc del sistema del sistema
operativo destino (para los builds glibc por defecto de Linux/macOS/Windows) —
ningún otro runtime. Redúcelo con `strip molde` para achicarlo (~17 MB → menos)
si el tamaño importa.

---

## Ejecutarlo

`molde` se invoca igual en todas partes:

```bash
molde --help                                   # list commands
molde pull   --connection "$DATABASE_URL"      # DB → models/
molde migrate AddInvoices                      # models/ → a migration
molde apply  --connection "$DATABASE_URL"      # apply migrations
molde verify --connection "$DATABASE_URL" --check   # drift check
```

- El **engine** se infiere a partir de la URL de conexión (`postgres://…`,
  `mysql://…`, `sqlite:…`) o se fija con `--provider`. **SQL Server** usa una
  cadena ADO y se detecta automáticamente:
  `Server=host,1433;Database=db;User Id=sa;Password=***;TrustServerCertificate=true`.
- `--connection` toma por defecto la variable de entorno `DATABASE_URL`; los
  comandos preguntan por lo que falte a menos que se indique `--no-input`.
  `--yes` omite las confirmaciones.

Para una configuración de equipo (cada desarrollador con su propia base de
datos local), consulta
[team-database-workflow.md](team-database-workflow.md).
