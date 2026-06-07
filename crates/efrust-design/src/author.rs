//! Autoría de migraciones: compara el modelo actual contra el snapshot, calcula
//! el diff y escribe la migración + el snapshot actualizado. Es el equivalente de
//! `dotnet ef migrations add`.

use std::path::{Path, PathBuf};

use efrust_core::diff::diff;
use efrust_core::migration::{self, Migration};
use efrust_core::snapshot::{self, SnapshotError};
use efrust_core::DatabaseModel;

#[derive(Debug, thiserror::Error)]
pub enum AuthorError {
    #[error("error con el snapshot: {0}")]
    Snapshot(#[from] SnapshotError),
    #[error("error escribiendo la migración: {0}")]
    Migration(#[from] migration::MigrationError),
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("no hay migraciones que eliminar")]
    NothingToRemove,
}

/// Resultado de intentar crear una migración.
#[derive(Debug)]
pub enum AddOutcome {
    /// El modelo no difiere del snapshot: no se creó nada.
    NoChanges,
    /// Migración creada.
    Created {
        id: String,
        up_ops: usize,
        down_ops: usize,
        migration_path: PathBuf,
        snapshot_path: PathBuf,
    },
}

/// Crea una migración llamada `name` (con identificador `id` ya formado, p. ej.
/// `20260607120000_InitialCreate`) a partir del diff entre el snapshot previo y
/// `current`. Actualiza el snapshot si hay cambios.
pub fn add(
    name: &str,
    id: &str,
    current: &DatabaseModel,
    migrations_dir: &Path,
    snapshot_path: &Path,
) -> Result<AddOutcome, AuthorError> {
    let previous = if snapshot_path.exists() {
        snapshot::load(snapshot_path)?
    } else {
        DatabaseModel::empty()
    };

    let up = diff(&previous, current);
    if up.is_empty() {
        return Ok(AddOutcome::NoChanges);
    }
    // El `down` es el diff inverso: revierte el modelo actual al previo.
    let down = diff(current, &previous);

    let migration = Migration::new(id, name, up.clone(), down.clone());
    let migration_path = migration::save(&migration, migrations_dir)?;
    snapshot::save(current, snapshot_path)?;

    Ok(AddOutcome::Created {
        id: id.to_string(),
        up_ops: up.len(),
        down_ops: down.len(),
        migration_path,
        snapshot_path: snapshot_path.to_path_buf(),
    })
}

/// Elimina la última migración y regenera el snapshot reproduciendo las
/// migraciones restantes. Si no queda ninguna, borra el snapshot.
pub fn remove(migrations_dir: &Path, snapshot_path: &Path) -> Result<String, AuthorError> {
    let mut migrations = migration::load_dir(migrations_dir)?;
    let last = migrations.pop().ok_or(AuthorError::NothingToRemove)?;

    std::fs::remove_file(migrations_dir.join(format!("{}.json", last.id)))?;

    if migrations.is_empty() {
        if snapshot_path.exists() {
            std::fs::remove_file(snapshot_path)?;
        }
    } else {
        let model = migration::rebuild_model(&migrations);
        snapshot::save(&model, snapshot_path)?;
    }

    Ok(last.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use efrust_core::diff::Operation;
    use efrust_core::model::{Column, PrimaryKey, Table};
    use efrust_providers::SqlGenerator;

    fn col(name: &str, clr: &str) -> Column {
        Column {
            name: name.into(),
            store_type: None,
            clr_type: Some(clr.into()),
            is_nullable: false,
            is_identity: false,
            max_length: None,
            precision: None,
            scale: None,
            default_value_sql: None,
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        }
    }

    fn model_with(columns: Vec<Column>) -> DatabaseModel {
        let mut m = DatabaseModel::empty();
        m.tables.push(Table {
            name: "Customer".into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns,
            primary_key: Some(PrimaryKey {
                name: "PK_Customer".into(),
                columns: vec!["Id".into()],
            }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
        });
        m
    }

    #[test]
    fn primera_migracion_crea_tabla_y_snapshot() {
        let dir = std::env::temp_dir().join(format!("efrust_author_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");
        let model = model_with(vec![col("Id", "System.Int32"), col("Name", "System.String")]);

        let outcome = add("InitialCreate", "20260607120000_InitialCreate", &model, &dir, &snap).unwrap();
        let AddOutcome::Created { up_ops, migration_path, .. } = outcome else {
            panic!("se esperaba Created");
        };
        assert_eq!(up_ops, 1); // un CreateTable
        assert!(migration_path.exists());
        assert!(snap.exists());

        // La migración renderiza a un CREATE TABLE válido.
        let migration = efrust_core::migration::load_dir(&dir).unwrap().remove(0);
        let sql = efrust_providers::SqliteGenerator::new()
            .emit_all(&migration.up)
            .unwrap()
            .join("\n");
        assert!(sql.contains("CREATE TABLE \"Customer\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sin_cambios_no_crea_migracion() {
        let dir = std::env::temp_dir().join(format!("efrust_author_nc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");
        let model = model_with(vec![col("Id", "System.Int32")]);

        add("Init", "20260607120000_Init", &model, &dir, &snap).unwrap();
        let again = add("NoOp", "20260607120100_NoOp", &model, &dir, &snap).unwrap();
        assert!(matches!(again, AddOutcome::NoChanges));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_regenera_snapshot_desde_las_restantes() {
        let dir = std::env::temp_dir().join(format!("efrust_author_rm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");

        let v1 = model_with(vec![col("Id", "System.Int32")]);
        add("Init", "20260607120000_Init", &v1, &dir, &snap).unwrap();
        let v2 = model_with(vec![col("Id", "System.Int32"), col("Email", "System.String")]);
        add("AddEmail", "20260607120100_AddEmail", &v2, &dir, &snap).unwrap();

        // Quitar la última debe dejar el snapshot como v1 (solo columna Id).
        let removed = remove(&dir, &snap).unwrap();
        assert_eq!(removed, "20260607120100_AddEmail");
        assert_eq!(efrust_core::migration::load_dir(&dir).unwrap().len(), 1);
        let snapshot = efrust_core::snapshot::load(&snap).unwrap();
        assert_eq!(snapshot.table(None, "Customer").unwrap().columns.len(), 1);

        // Quitar la última restante borra el snapshot.
        remove(&dir, &snap).unwrap();
        assert!(!snap.exists());
        assert!(remove(&dir, &snap).is_err()); // ya no hay nada

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn segunda_migracion_detecta_columna_nueva() {
        let dir = std::env::temp_dir().join(format!("efrust_author_col_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");

        let v1 = model_with(vec![col("Id", "System.Int32")]);
        add("Init", "20260607120000_Init", &v1, &dir, &snap).unwrap();

        let v2 = model_with(vec![col("Id", "System.Int32"), col("Email", "System.String")]);
        let outcome = add("AddEmail", "20260607120100_AddEmail", &v2, &dir, &snap).unwrap();
        let AddOutcome::Created { up_ops, .. } = outcome else {
            panic!("se esperaba Created");
        };
        assert_eq!(up_ops, 1); // un AddColumn
        let migration = efrust_core::migration::load_dir(&dir).unwrap().pop().unwrap();
        assert!(matches!(migration.up[0], Operation::AddColumn { .. }));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
