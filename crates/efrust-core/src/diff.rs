//! Diff entre dos [`DatabaseModel`] → lista de [`Operation`], y la evolución
//! inversa [`apply_operation`] (aplicar una operación a un modelo).
//!
//! El orden de las operaciones respeta dependencias: al crear, primero las
//! tablas, luego sus FKs e índices; al eliminar, primero FKs e índices y al
//! final las tablas. Esto evita violar restricciones referenciales.

use serde::{Deserialize, Serialize};

use crate::model::{Column, DatabaseModel, ForeignKey, Index, Table};

/// Una operación atómica de migración. Provider-agnóstica; cada provider la
/// traduce a SQL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    CreateTable {
        table: Table,
    },
    DropTable {
        schema: Option<String>,
        name: String,
    },
    AddColumn {
        schema: Option<String>,
        table: String,
        column: Column,
    },
    DropColumn {
        schema: Option<String>,
        table: String,
        name: String,
    },
    AlterColumn {
        schema: Option<String>,
        table: String,
        /// Estado deseado de la columna.
        new: Column,
        /// Estado previo (para generar el `Down`).
        old: Column,
    },
    AddForeignKey {
        schema: Option<String>,
        table: String,
        foreign_key: ForeignKey,
    },
    DropForeignKey {
        schema: Option<String>,
        table: String,
        name: String,
    },
    CreateIndex {
        schema: Option<String>,
        table: String,
        index: Index,
    },
    DropIndex {
        schema: Option<String>,
        table: String,
        name: String,
    },
}

/// Calcula las operaciones necesarias para transformar `from` en `to`.
///
/// Orden (seguro frente a dependencias):
/// 1. `DropForeignKey`, `DropIndex` de tablas que cambian o desaparecen.
/// 2. `CreateTable` de tablas nuevas.
/// 3. Alteraciones de columnas en tablas existentes.
/// 4. `AddForeignKey`, `CreateIndex` (las tablas referenciadas ya existen).
/// 5. `DropTable` de tablas obsoletas.
///
/// > Nota: aún no se detectan renombrados (se ven como drop+add).
pub fn diff(from: &DatabaseModel, to: &DatabaseModel) -> Vec<Operation> {
    let mut drop_fks = Vec::new();
    let mut drop_indexes = Vec::new();
    let mut create_tables = Vec::new();
    let mut column_ops = Vec::new();
    let mut add_fks = Vec::new();
    let mut create_indexes = Vec::new();
    let mut drop_tables = Vec::new();

    // Tablas nuevas (y sus FKs/índices).
    for t in &to.tables {
        if from.table(t.schema.as_deref(), &t.name).is_none() {
            create_tables.push(Operation::CreateTable { table: t.clone() });
            for fk in &t.foreign_keys {
                add_fks.push(Operation::AddForeignKey {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    foreign_key: fk.clone(),
                });
            }
            for ix in &t.indexes {
                create_indexes.push(Operation::CreateIndex {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    index: ix.clone(),
                });
            }
        }
    }

    // Tablas presentes en ambos → diff de columnas, FKs e índices.
    for new_t in &to.tables {
        let Some(old_t) = from.table(new_t.schema.as_deref(), &new_t.name) else {
            continue;
        };
        diff_columns(old_t, new_t, &mut column_ops);
        diff_foreign_keys(old_t, new_t, &mut add_fks, &mut drop_fks);
        diff_indexes(old_t, new_t, &mut create_indexes, &mut drop_indexes);
    }

    // Tablas eliminadas (FKs e índices primero, luego la tabla).
    for t in &from.tables {
        if to.table(t.schema.as_deref(), &t.name).is_none() {
            for ix in &t.indexes {
                drop_indexes.push(Operation::DropIndex {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    name: ix.name.clone(),
                });
            }
            for fk in &t.foreign_keys {
                drop_fks.push(Operation::DropForeignKey {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    name: fk.name.clone(),
                });
            }
            drop_tables.push(Operation::DropTable {
                schema: t.schema.clone(),
                name: t.name.clone(),
            });
        }
    }

    let mut ops = Vec::new();
    ops.extend(drop_fks);
    ops.extend(drop_indexes);
    ops.extend(create_tables);
    ops.extend(column_ops);
    ops.extend(add_fks);
    ops.extend(create_indexes);
    ops.extend(drop_tables);
    ops
}

fn diff_columns(old_t: &Table, new_t: &Table, ops: &mut Vec<Operation>) {
    for new_c in &new_t.columns {
        match old_t.column(&new_c.name) {
            None => ops.push(Operation::AddColumn {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                column: new_c.clone(),
            }),
            Some(old_c) if old_c != new_c => ops.push(Operation::AlterColumn {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                new: new_c.clone(),
                old: old_c.clone(),
            }),
            Some(_) => {}
        }
    }
    for old_c in &old_t.columns {
        if new_t.column(&old_c.name).is_none() {
            ops.push(Operation::DropColumn {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                name: old_c.name.clone(),
            });
        }
    }
}

fn diff_foreign_keys(old_t: &Table, new_t: &Table, add: &mut Vec<Operation>, drop: &mut Vec<Operation>) {
    for fk in &new_t.foreign_keys {
        if !old_t.foreign_keys.iter().any(|f| f.name == fk.name) {
            add.push(Operation::AddForeignKey {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                foreign_key: fk.clone(),
            });
        }
    }
    for fk in &old_t.foreign_keys {
        if !new_t.foreign_keys.iter().any(|f| f.name == fk.name) {
            drop.push(Operation::DropForeignKey {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                name: fk.name.clone(),
            });
        }
    }
}

fn diff_indexes(old_t: &Table, new_t: &Table, create: &mut Vec<Operation>, drop: &mut Vec<Operation>) {
    for ix in &new_t.indexes {
        if !old_t.indexes.iter().any(|i| i.name == ix.name) {
            create.push(Operation::CreateIndex {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                index: ix.clone(),
            });
        }
    }
    for ix in &old_t.indexes {
        if !new_t.indexes.iter().any(|i| i.name == ix.name) {
            drop.push(Operation::DropIndex {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                name: ix.name.clone(),
            });
        }
    }
}

/// Aplica una operación a un modelo en memoria. Permite reconstruir el snapshot
/// reproduciendo las operaciones `up` de una secuencia de migraciones.
pub fn apply_operation(model: &mut DatabaseModel, op: &Operation) {
    match op {
        Operation::CreateTable { table } => model.tables.push(table.clone()),
        Operation::DropTable { schema, name } => model
            .tables
            .retain(|t| !(t.schema.as_deref() == schema.as_deref() && &t.name == name)),
        Operation::AddColumn { schema, table, column } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.columns.push(column.clone());
            }
        }
        Operation::DropColumn { schema, table, name } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.columns.retain(|c| &c.name != name);
            }
        }
        Operation::AlterColumn { schema, table, new, .. } => {
            if let Some(t) = find_mut(model, schema, table) {
                if let Some(c) = t.columns.iter_mut().find(|c| c.name == new.name) {
                    *c = new.clone();
                }
            }
        }
        Operation::AddForeignKey { schema, table, foreign_key } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.foreign_keys.push(foreign_key.clone());
            }
        }
        Operation::DropForeignKey { schema, table, name } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.foreign_keys.retain(|f| &f.name != name);
            }
        }
        Operation::CreateIndex { schema, table, index } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.indexes.push(index.clone());
            }
        }
        Operation::DropIndex { schema, table, name } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.indexes.retain(|i| &i.name != name);
            }
        }
    }
}

fn find_mut<'a>(
    model: &'a mut DatabaseModel,
    schema: &Option<String>,
    name: &str,
) -> Option<&'a mut Table> {
    model
        .tables
        .iter_mut()
        .find(|t| t.schema.as_deref() == schema.as_deref() && t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ForeignKey, Index, PrimaryKey};

    fn col(name: &str) -> Column {
        Column {
            name: name.into(),
            store_type: Some("integer".into()),
            clr_type: Some("System.Int32".into()),
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

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns: cols.iter().map(|c| col(c)).collect(),
            primary_key: None,
            foreign_keys: vec![],
            indexes: vec![],
        }
    }

    #[test]
    fn detecta_tabla_nueva() {
        let from = DatabaseModel::empty();
        let mut to = DatabaseModel::empty();
        to.tables.push(table("Customer", &["Id", "Name"]));
        let ops = diff(&from, &to);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], Operation::CreateTable { .. }));
    }

    #[test]
    fn tabla_nueva_con_fk_e_indice_ordena_create_antes_de_fk() {
        let from = DatabaseModel::empty();
        let mut to = DatabaseModel::empty();
        let mut order = table("Order", &["Id", "CustomerId"]);
        order.foreign_keys.push(ForeignKey {
            name: "FK_Order_Customer".into(),
            columns: vec!["CustomerId".into()],
            principal_table: "Customer".into(),
            principal_schema: None,
            principal_columns: vec!["Id".into()],
            on_delete: crate::model::ReferentialAction::Cascade,
        });
        order.indexes.push(Index {
            name: "IX_Order_CustomerId".into(),
            columns: vec!["CustomerId".into()],
            is_unique: false,
            filter: None,
        });
        to.tables.push(order);

        let ops = diff(&from, &to);
        // create_table, add_foreign_key, create_index — en ese orden.
        assert!(matches!(ops[0], Operation::CreateTable { .. }));
        assert!(matches!(ops[1], Operation::AddForeignKey { .. }));
        assert!(matches!(ops[2], Operation::CreateIndex { .. }));
    }

    #[test]
    fn detecta_columna_añadida_y_eliminada() {
        let mut from = DatabaseModel::empty();
        from.tables.push(table("Customer", &["Id", "Name"]));
        let mut to = DatabaseModel::empty();
        to.tables.push(table("Customer", &["Id", "Email"]));
        let ops = diff(&from, &to);
        assert_eq!(ops.iter().filter(|o| matches!(o, Operation::AddColumn { .. })).count(), 1);
        assert_eq!(ops.iter().filter(|o| matches!(o, Operation::DropColumn { .. })).count(), 1);
    }

    #[test]
    fn modelo_identico_sin_cambios() {
        let mut m = DatabaseModel::empty();
        m.tables.push(table("Customer", &["Id"]));
        assert!(diff(&m, &m).is_empty());
    }

    #[test]
    fn apply_operation_reconstruye_el_modelo() {
        let mut target = DatabaseModel::empty();
        let mut t = table("Customer", &["Id", "Name"]);
        t.primary_key = Some(PrimaryKey { name: "PK_Customer".into(), columns: vec!["Id".into()] });
        target.tables.push(t);

        // Reproducir las ops `up` sobre un modelo vacío debe dar el mismo modelo.
        let ops = diff(&DatabaseModel::empty(), &target);
        let mut rebuilt = DatabaseModel::empty();
        for op in &ops {
            apply_operation(&mut rebuilt, op);
        }
        assert_eq!(rebuilt.table(None, "Customer").unwrap().columns.len(), 2);
        assert!(diff(&rebuilt, &target).is_empty(), "reconstrucción equivalente");
    }
}
