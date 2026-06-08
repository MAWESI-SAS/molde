# Base: official Rust devcontainer image (ships the stable toolchain + clippy/
# rustfmt and the non-root `vscode` user). On top we add the native dependencies
# sqlx needs. molde is 100% Rust: no .NET.
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# --- Native dependencies to build the Rust crates (sqlx, libssl) ---
RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        libsqlite3-dev \
        postgresql-client \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# sqlx-cli is handy to inspect/manage the database during development.
ENV CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN cargo install sqlx-cli --no-default-features --features rustls,postgres,sqlite \
    && chmod -R a+w "$CARGO_HOME"
