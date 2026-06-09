//! Migration authoring: compares the current model against the snapshot, computes
//! the diff, and writes the migration + the updated snapshot. It is the equivalent
//! of `dotnet ef migrations add`.

use std::path::{Path, PathBuf};

use molde_core::diff::diff;
use molde_core::migration::{self, Migration};
use molde_core::snapshot::{self, SnapshotError};
use molde_core::DatabaseModel;

#[derive(Debug, thiserror::Error)]
pub enum AuthorError {
    #[error("snapshot error: {0}")]
    Snapshot(#[from] SnapshotError),
    #[error("error writing the migration: {0}")]
    Migration(#[from] migration::MigrationError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("there are no migrations to remove")]
    NothingToRemove,
}

/// Result of attempting to create a migration.
#[derive(Debug)]
pub enum AddOutcome {
    /// The model does not differ from the snapshot: nothing was created.
    NoChanges,
    /// Migration created.
    Created {
        id: String,
        up_ops: usize,
        down_ops: usize,
        migration_path: PathBuf,
        snapshot_path: PathBuf,
    },
}

/// Creates a migration named `name` (with an already-formed identifier `id`, e.g.
/// `20260607120000_InitialCreate`) from the diff between the previous snapshot and
/// `current`. Updates the snapshot if there are changes.
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
    // The `down` is the inverse diff: it reverts the current model to the previous one.
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

/// Removes the last migration and regenerates the snapshot by replaying the
/// remaining migrations. If none are left, it deletes the snapshot.
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
    use molde_core::diff::Operation;
    use molde_core::model::{Column, PrimaryKey, Table};
    use molde_providers::SqlGenerator;

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
            seed_data: vec![],
            seed_key: vec![],
        });
        m
    }

    #[test]
    fn first_migration_creates_table_and_snapshot() {
        let dir = std::env::temp_dir().join(format!("molde_author_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");
        let model = model_with(vec![
            col("Id", "System.Int32"),
            col("Name", "System.String"),
        ]);

        let outcome = add(
            "InitialCreate",
            "20260607120000_InitialCreate",
            &model,
            &dir,
            &snap,
        )
        .unwrap();
        let AddOutcome::Created {
            up_ops,
            migration_path,
            ..
        } = outcome
        else {
            panic!("expected Created");
        };
        assert_eq!(up_ops, 1); // one CreateTable
        assert!(migration_path.exists());
        assert!(snap.exists());

        // The migration renders to a valid CREATE TABLE.
        let migration = molde_core::migration::load_dir(&dir).unwrap().remove(0);
        let sql = molde_providers::SqliteGenerator::new()
            .emit_all(&migration.up)
            .unwrap()
            .join("\n");
        assert!(sql.contains("CREATE TABLE \"Customer\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_changes_does_not_create_migration() {
        let dir = std::env::temp_dir().join(format!("molde_author_nc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");
        let model = model_with(vec![col("Id", "System.Int32")]);

        add("Init", "20260607120000_Init", &model, &dir, &snap).unwrap();
        let again = add("NoOp", "20260607120100_NoOp", &model, &dir, &snap).unwrap();
        assert!(matches!(again, AddOutcome::NoChanges));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_regenerates_snapshot_from_the_remaining() {
        let dir = std::env::temp_dir().join(format!("molde_author_rm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");

        let v1 = model_with(vec![col("Id", "System.Int32")]);
        add("Init", "20260607120000_Init", &v1, &dir, &snap).unwrap();
        let v2 = model_with(vec![
            col("Id", "System.Int32"),
            col("Email", "System.String"),
        ]);
        add("AddEmail", "20260607120100_AddEmail", &v2, &dir, &snap).unwrap();

        // Removing the last one should leave the snapshot as v1 (only the Id column).
        let removed = remove(&dir, &snap).unwrap();
        assert_eq!(removed, "20260607120100_AddEmail");
        assert_eq!(molde_core::migration::load_dir(&dir).unwrap().len(), 1);
        let snapshot = molde_core::snapshot::load(&snap).unwrap();
        assert_eq!(snapshot.table(None, "Customer").unwrap().columns.len(), 1);

        // Removing the last remaining one deletes the snapshot.
        remove(&dir, &snap).unwrap();
        assert!(!snap.exists());
        assert!(remove(&dir, &snap).is_err()); // nothing left

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_migration_detects_new_column() {
        let dir = std::env::temp_dir().join(format!("molde_author_col_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = dir.join("model-snapshot.json");

        let v1 = model_with(vec![col("Id", "System.Int32")]);
        add("Init", "20260607120000_Init", &v1, &dir, &snap).unwrap();

        let v2 = model_with(vec![
            col("Id", "System.Int32"),
            col("Email", "System.String"),
        ]);
        let outcome = add("AddEmail", "20260607120100_AddEmail", &v2, &dir, &snap).unwrap();
        let AddOutcome::Created { up_ops, .. } = outcome else {
            panic!("expected Created");
        };
        assert_eq!(up_ops, 1); // one AddColumn
        let migration = molde_core::migration::load_dir(&dir)
            .unwrap()
            .pop()
            .unwrap();
        assert!(matches!(migration.up[0], Operation::AddColumn { .. }));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
