//! On-disk format of migrations.
//!
//! A migration is a JSON that stores the list of [`Operation`] (the IR) for
//! `up` and `down`. The SQL is **not** stored: it is rendered when applied, with the
//! `SqlGenerator` of the chosen provider. This way a single migration works for
//! different engines (Postgres, SQLite, …).
//!
//! File naming convention: `<id>.json`, where `id` follows EF's style:
//! `<UTC timestamp yyyyMMddHHmmss>_<Name>` (e.g.
//! `20260607120000_InitialCreate`). The lexicographic order of `id` matches the
//! chronological order of application.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff::Operation;

/// Migration format version. It is bumped on incompatible changes.
pub const MIGRATION_FORMAT_VERSION: u32 = 1;

/// A migration: operations to move forward (`up`) and to revert (`down`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Migration {
    pub format_version: u32,
    /// Unique identifier, also the PK in `__EFMigrationsHistory`.
    pub id: String,
    /// Readable name (the part after the timestamp).
    pub name: String,
    #[serde(default)]
    pub up: Vec<Operation>,
    #[serde(default)]
    pub down: Vec<Operation>,
}

impl Migration {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        up: Vec<Operation>,
        down: Vec<Operation>,
    ) -> Self {
        Self {
            format_version: MIGRATION_FORMAT_VERSION,
            id: id.into(),
            name: name.into(),
            up,
            down,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("I/O error on migrations: {0}")]
    Io(#[from] std::io::Error),
    #[error("error (de)serializing the migration '{path}': {source}")]
    Serde {
        path: String,
        source: serde_json::Error,
    },
    #[error(
        "unsupported format version in '{path}': {found} (expected <= {MIGRATION_FORMAT_VERSION})"
    )]
    UnsupportedFormat { path: String, found: u32 },
}

/// Loads all migrations (`*.json`) from a directory, ordered by `id`.
/// If the directory does not exist, returns an empty list (no migration has been
/// created yet).
pub fn load_dir(dir: impl AsRef<Path>) -> Result<Vec<Migration>, MigrationError> {
    let dir = dir.as_ref();
    let mut migrations = Vec::new();
    if !dir.exists() {
        return Ok(migrations);
    }

    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        // Migrations are named `<timestamp>_<name>.json` (they start with a
        // digit). Any other `.json` in the directory is ignored, such as the
        // `model-snapshot.json`.
        let starts_with_digit = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.chars().next())
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        if !starts_with_digit {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let migration: Migration =
            serde_json::from_slice(&bytes).map_err(|source| MigrationError::Serde {
                path: path.display().to_string(),
                source,
            })?;
        if migration.format_version > MIGRATION_FORMAT_VERSION {
            return Err(MigrationError::UnsupportedFormat {
                path: path.display().to_string(),
                found: migration.format_version,
            });
        }
        migrations.push(migration);
    }

    migrations.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(migrations)
}

/// Rebuilds the model by replaying the `up` operations of a sequence of
/// migrations (already ordered). Used by `migrations remove` to regenerate the
/// snapshot after removing the last migration.
pub fn rebuild_model(migrations: &[Migration]) -> crate::DatabaseModel {
    let mut model = crate::DatabaseModel::empty();
    for migration in migrations {
        for op in &migration.up {
            crate::diff::apply_operation(&mut model, op);
        }
    }
    model
}

/// Persists a migration as `<dir>/<id>.json`. Returns the written path.
/// (`migrations add` will use it in Phase 4; here it provides symmetry and test support.)
pub fn save(migration: &Migration, dir: impl AsRef<Path>) -> Result<PathBuf, MigrationError> {
    let dir = dir.as_ref();
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", migration.id));
    let json = serde_json::to_string_pretty(migration).map_err(|source| MigrationError::Serde {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::write(&path, json)?;
    Ok(path)
}
