//! Formato en disco de las migraciones.
//!
//! Una migración es un JSON que guarda la lista de [`Operation`] (el IR) para
//! `up` y `down`. El SQL **no** se almacena: se renderiza al aplicar, con el
//! `SqlGenerator` del provider elegido. Así una misma migración sirve para
//! distintos motores (Postgres, SQLite, …).
//!
//! Convención de nombre de archivo: `<id>.json`, donde `id` sigue el estilo de
//! EF: `<timestamp UTC yyyyMMddHHmmss>_<Nombre>` (p. ej.
//! `20260607120000_InitialCreate`). El orden lexicográfico del `id` coincide con
//! el orden cronológico de aplicación.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff::Operation;

/// Versión del formato de migración. Se incrementa ante cambios incompatibles.
pub const MIGRATION_FORMAT_VERSION: u32 = 1;

/// Una migración: operaciones para avanzar (`up`) y revertir (`down`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Migration {
    pub format_version: u32,
    /// Identificador único, también la PK en `__EFMigrationsHistory`.
    pub id: String,
    /// Nombre legible (la parte tras el timestamp).
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
    #[error("error de E/S sobre migraciones: {0}")]
    Io(#[from] std::io::Error),
    #[error("error (de)serializando la migración '{path}': {source}")]
    Serde {
        path: String,
        source: serde_json::Error,
    },
    #[error("versión de formato no soportada en '{path}': {found} (esperada <= {MIGRATION_FORMAT_VERSION})")]
    UnsupportedFormat { path: String, found: u32 },
}

/// Carga todas las migraciones (`*.json`) de un directorio, ordenadas por `id`.
/// Si el directorio no existe, devuelve una lista vacía (aún no se ha creado
/// ninguna migración).
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
        // Las migraciones se llaman `<timestamp>_<nombre>.json` (empiezan con
        // dígito). Se ignora cualquier otro `.json` del directorio, como el
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

/// Reconstruye el modelo reproduciendo las operaciones `up` de una secuencia de
/// migraciones (ya ordenadas). Lo usa `migrations remove` para regenerar el
/// snapshot tras eliminar la última migración.
pub fn rebuild_model(migrations: &[Migration]) -> crate::DatabaseModel {
    let mut model = crate::DatabaseModel::empty();
    for migration in migrations {
        for op in &migration.up {
            crate::diff::apply_operation(&mut model, op);
        }
    }
    model
}

/// Persiste una migración como `<dir>/<id>.json`. Devuelve la ruta escrita.
/// (Lo usará `migrations add` en la Fase 4; aquí da simetría y soporte a tests.)
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
