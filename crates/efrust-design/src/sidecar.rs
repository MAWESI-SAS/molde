//! Invocación del sidecar .NET: ejecuta el proceso, captura su JSON de stdout y
//! lo deserializa en [`DatabaseModel`].
//!
//! Tres modos de invocación, en orden de prioridad:
//! 1. Variable `EFRUST_SIDECAR_CMD` (p. ej. `"efrust-sidecar"` o
//!    `"dotnet efrust-sidecar"`): comando explícito, útil con el `dotnet tool`.
//! 2. `dotnet <ruta-al-dll>` (modo por defecto, usando [`SidecarOptions`]).

use std::path::Path;
use std::process::Command;

use efrust_core::snapshot::{self, SnapshotError};
use efrust_core::DatabaseModel;

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("no se pudo ejecutar el sidecar ('{cmd}'): {source}")]
    Spawn { cmd: String, source: std::io::Error },
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

/// Construye el comando base (programa + args fijos) según el modo activo.
/// Devuelve también una representación textual para diagnósticos.
fn base_command(opts: &SidecarOptions<'_>) -> (Command, String) {
    // Modo `dotnet tool` / comando personalizado.
    if let Ok(custom) = std::env::var("EFRUST_SIDECAR_CMD") {
        let custom = custom.trim();
        if !custom.is_empty() {
            let mut parts = custom.split_whitespace();
            let prog = parts.next().unwrap_or("dotnet");
            let mut cmd = Command::new(prog);
            for a in parts {
                cmd.arg(a);
            }
            return (cmd, custom.to_string());
        }
    }
    // Modo por defecto: `dotnet <dll>`.
    let mut cmd = Command::new(opts.dotnet);
    cmd.arg(opts.sidecar_dll);
    (
        cmd,
        format!("{} {}", opts.dotnet, opts.sidecar_dll.display()),
    )
}

/// Ejecuta el sidecar y devuelve el modelo del DbContext del usuario.
pub fn fetch_model(opts: &SidecarOptions<'_>) -> Result<DatabaseModel, SidecarError> {
    let (mut cmd, label) = base_command(opts);
    cmd.arg("--assembly").arg(opts.assembly);
    if let Some(ctx) = opts.context {
        cmd.arg("--context").arg(ctx);
    }

    tracing::debug!("invocando sidecar: {:?}", cmd);
    let output = cmd
        .output()
        .map_err(|source| SidecarError::Spawn { cmd: label, source })?;

    if !output.status.success() {
        return Err(SidecarError::Failed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(snapshot::from_slice(&output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_define_el_programa() {
        let opts = SidecarOptions {
            dotnet: "dotnet",
            sidecar_dll: Path::new("/x/efrust-sidecar.dll"),
            assembly: Path::new("/x/app.dll"),
            context: None,
        };
        // Sin override: usa `dotnet`.
        std::env::remove_var("EFRUST_SIDECAR_CMD");
        let (cmd, _) = base_command(&opts);
        assert_eq!(cmd.get_program(), "dotnet");

        // Con override de tool: usa el comando indicado.
        std::env::set_var("EFRUST_SIDECAR_CMD", "efrust-sidecar");
        let (cmd, label) = base_command(&opts);
        assert_eq!(cmd.get_program(), "efrust-sidecar");
        assert_eq!(label, "efrust-sidecar");
        std::env::remove_var("EFRUST_SIDECAR_CMD");
    }
}
