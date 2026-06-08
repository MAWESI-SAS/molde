# Base: imagen devcontainer oficial de Rust (trae la toolchain stable + clippy/
# rustfmt y el usuario no-root `vscode`). Encima añadimos las dependencias
# nativas que necesita sqlx. molde es 100% Rust: no hay .NET.
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# --- Dependencias nativas para compilar las crates de Rust (sqlx, libssl) ---
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

# sqlx-cli es útil para inspeccionar/gestionar la BD durante el desarrollo.
ENV CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN cargo install sqlx-cli --no-default-features --features rustls,postgres,sqlite \
    && chmod -R a+w "$CARGO_HOME"
