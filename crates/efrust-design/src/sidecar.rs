//! Invocación del sidecar .NET: ejecuta el proceso, captura su JSON de stdout y
//! lo deserializa en [`DatabaseModel`].

use std::path::Path;
use std::process::Command;

use efrust_core::snapshot::{self, SnapshotError};
use efrust_core::DatabaseModel;

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("no se pudo ejecutar el sidecar ('{dotnet} {dll}'): {source}")]
    Spawn {
        dotnet: String,
        dll: String,
        source: std::io::Error,
    },
    #[error("el sidecar terminó con código {code:?}:\n{stderr}")]
    Failed { code: Option<i32>, stderr: String },
    #[error("no se pudo interpretar la salida del sidecar: {0}")]
    Parse(#[from] SnapshotError),
}

/// Parámetros para invocar el sidecar.
pub struct SidecarOptions<'a> {
    /// Ejecutable de .NET (normalmente `dotnet`).
    pub dotnet: &'a str,
    /// Ruta al `efrust-sidecar.dll`.
    pub sidecar_dll: &'a Path,
    /// Ruta al assembly compilado del proyecto del usuario.
    pub assembly: &'a Path,
    /// Nombre del DbContext (opcional si solo hay uno).
    pub context: Option<&'a str>,
}

/// Ejecuta el sidecar y devuelve el modelo del DbContext del usuario.
pub fn fetch_model(opts: &SidecarOptions<'_>) -> Result<DatabaseModel, SidecarError> {
    let mut cmd = Command::new(opts.dotnet);
    cmd.arg(opts.sidecar_dll)
        .arg("--assembly")
        .arg(opts.assembly);
    if let Some(ctx) = opts.context {
        cmd.arg("--context").arg(ctx);
    }

    tracing::debug!("invocando sidecar: {:?}", cmd);
    let output = cmd.output().map_err(|source| SidecarError::Spawn {
        dotnet: opts.dotnet.to_string(),
        dll: opts.sidecar_dll.display().to_string(),
        source,
    })?;

    if !output.status.success() {
        return Err(SidecarError::Failed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(snapshot::from_slice(&output.stdout)?)
}
