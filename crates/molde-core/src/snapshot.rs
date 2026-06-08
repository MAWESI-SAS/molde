//! Loading and saving of the model snapshot.
//!
//! The snapshot is the [`DatabaseModel`] of the last migrated state, serialized to
//! JSON. `migrations add` compares the current model against this snapshot to
//! compute the diff. It is the equivalent of EF's `ModelSnapshot.cs`, but in a
//! custom format (Phase 0: stable, readable JSON).

use std::path::Path;

use crate::model::DatabaseModel;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("I/O error reading/writing the snapshot: {0}")]
    Io(#[from] std::io::Error),
    #[error("error (de)serializing the snapshot: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("unsupported format version: {found} (expected <= {supported})")]
    UnsupportedFormat { found: u32, supported: u32 },
}

/// Serializes the model to JSON with stable indentation (normalizes before writing).
pub fn save(model: &DatabaseModel, path: impl AsRef<Path>) -> Result<(), SnapshotError> {
    let mut normalized = model.clone();
    normalized.normalize();
    let json = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Loads a snapshot from disk, validating the format version.
pub fn load(path: impl AsRef<Path>) -> Result<DatabaseModel, SnapshotError> {
    let bytes = std::fs::read(path)?;
    from_slice(&bytes)
}

/// Deserializes a model from JSON bytes (shared by sidecar and snapshot).
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
