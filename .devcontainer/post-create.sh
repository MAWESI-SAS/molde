#!/usr/bin/env bash
# Se ejecuta una vez tras crear el contenedor. Deja el entorno listo para
# compilar y probar el workspace de Rust.
set -euo pipefail

echo "==> Versiones de toolchain"
rustc --version
cargo --version

echo "==> Descargando dependencias de Rust (cargo fetch)"
cargo fetch

echo "==> Listo. Comandos útiles:"
echo "    cargo build && cargo test"
echo "    molde scaffold --connection \"\$DATABASE_URL\"   # BD -> models/*.model"
echo "    molde migrations add InitialCreate              # models/ -> migrations/"
echo "    molde database update --connection \"\$DATABASE_URL\""
echo "    psql \"\$DATABASE_URL\"   # Postgres local (servicio db)"
