#!/usr/bin/env bash
# Runs once after the container is created. Leaves the environment ready to
# build and test the Rust workspace.
set -euo pipefail

echo "==> Toolchain versions"
rustc --version
cargo --version

echo "==> Downloading Rust dependencies (cargo fetch)"
cargo fetch

echo "==> Ready. Useful commands:"
echo "    cargo build && cargo test"
echo "    molde pull --connection \"\$DATABASE_URL\"     # database -> models/*.model"
echo "    molde migrate InitialCreate                    # models/ -> migrations/"
echo "    molde apply --connection \"\$DATABASE_URL\"     # apply migrations"
echo "    psql \"\$DATABASE_URL\"   # local Postgres (db service)"
