# Base: imagen devcontainer oficial de .NET 9 (convención MAWESI).
# Trae el SDK de .NET 9 + usuario no-root `vscode`. Encima añadimos la toolchain
# de Rust (stable) y las dependencias nativas que necesita sqlx.
FROM mcr.microsoft.com/devcontainers/dotnet:1-9.0-bookworm

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

# --- Rust toolchain (system-wide, accesible por el usuario `vscode`) ---
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --default-toolchain stable --profile default \
    && rustup component add clippy rustfmt \
    # Hacer el toolchain escribible para que `vscode` pueda compilar/añadir componentes
    && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"

# sqlx-cli es útil para inspeccionar/gestionar la BD durante el desarrollo (Fase 1).
RUN cargo install sqlx-cli --no-default-features --features rustls,postgres,sqlite \
    && chmod -R a+w "$CARGO_HOME"
