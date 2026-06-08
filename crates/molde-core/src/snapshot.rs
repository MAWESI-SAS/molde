//! Carga y guardado del snapshot del modelo.
//!
//! El snapshot es el [`DatabaseModel`] del último estado migrado, serializado a
//! JSON. `migrations add` compara el modelo actual contra este snapshot para
//! calcular el diff. Es el equivalente de `ModelSnapshot.cs` de EF, pero en un
//! formato propio (Fase 0: JSON estable y legible).

use std::path::Path;

use crate::model::DatabaseModel;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("error de E/S leyendo/escribiendo el snapshot: {0}")]
    Io(#[from] std::io::Error),
    #[error("error (de)serializando el snapshot: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("versión de formato no soportada: {found} (esperada <= {supported})")]
    UnsupportedFormat { found: u32, supported: u32 },
}

/// Serializa el modelo a JSON con indentación estable (normaliza antes de escribir).
pub fn save(model: &DatabaseModel, path: impl AsRef<Path>) -> Result<(), SnapshotError> {
    let mut normalized = model.clone();
    normalized.normalize();
    let json = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Carga un snapshot desde disco, validando la versión de formato.
pub fn load(path: impl AsRef<Path>) -> Result<DatabaseModel, SnapshotError> {
    let bytes = std::fs::read(path)?;
    from_slice(&bytes)
}

/// Deserializa un modelo desde bytes JSON (compartido por sidecar y snapshot).
pub fn from_slice(bytes: &[u8]) -> Result<DatabaseModel, SnapshotError> {
    let model: DatabaseModel = serde_json::from_slice(bytes)?;
    if model.format_version > crate::model::IR_FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedFormat {
            found: model.format_version,
            supported: crate::model::IR_FORMAT_VERSION,
        });
    }
    Ok(model)
}
