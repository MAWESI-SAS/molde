#!/usr/bin/env bash
# Se ejecuta una vez tras crear el contenedor. Deja el entorno listo para
# compilar el workspace de Rust y el sidecar de .NET.
set -euo pipefail

echo "==> Versiones de toolchain"
rustc --version
cargo --version
dotnet --version

echo "==> dotnet-ef (CLI oficial, para comparar paridad en fases posteriores)"
dotnet tool install --global dotnet-ef --version "9.*" || dotnet tool update --global dotnet-ef --version "9.*"
# Asegura que las herramientas globales de dotnet estén en el PATH de futuras shells.
if ! grep -q 'DOTNET_ROOT/tools\|.dotnet/tools' "$HOME/.bashrc" 2>/dev/null; then
    echo 'export PATH="$PATH:$HOME/.dotnet/tools"' >> "$HOME/.bashrc"
fi

echo "==> Descargando dependencias de Rust (cargo fetch)"
cargo fetch

echo "==> Restaurando el sidecar .NET"
dotnet restore sidecar/EfRust.Sidecar/EfRust.Sidecar.csproj

echo "==> Listo. Comandos útiles:"
echo "    cargo build && cargo test"
echo "    dotnet run --project sidecar/EfRust.Sidecar"
echo "    psql \"\$DATABASE_URL\"   # Postgres local (servicio db)"
