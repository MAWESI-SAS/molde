//! Diff between two [`DatabaseModel`] → list of [`Operation`], and the inverse
//! evolution [`apply_operation`] (apply an operation to a model).
//!
//! The order of operations respects dependencies: when creating, first the
//! tables, then their FKs and indexes; when dropping, first FKs and indexes and
//! the tables last. This avoids violating referential constraints.

use serde::{Deserialize, Serialize};

use crate::model::{Column, DatabaseModel, DbFunction, ForeignKey, Index, Table, Trigger};

/// An atomic migration operation. Provider-agnostic; each provider translates
/// it to SQL.
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
        /// Desired state of the column.
        new: Column,
        /// Previous state (to generate the `Down`).
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
    /// Ensures an engine extension is installed (e.g. pgvector's `vector` in
    /// Postgres). It is prepended when the model uses types/indexes that
    /// require it. Engines without extensions skip it.
    EnsureExtension {
        name: String,
    },
    /// Creates a schema function (e.g. a trigger function in Postgres).
    CreateFunction {
        function: DbFunction,
    },
    DropFunction {
        schema: Option<String>,
        name: String,
    },
    /// Creates a trigger on a table.
    CreateTrigger {
        schema: Option<String>,
        table: String,
        trigger: Trigger,
    },
    DropTrigger {
        schema: Option<String>,
        table: String,
        name: String,
    },
    /// Raw DDL to run verbatim (escape hatch; see [`DatabaseModel::raw_objects`]).
    RawSql {
        sql: String,
    },
    /// Full rebuild of an existing table. Emitted by `diff()` (in addition to
    /// the granular ALTERs) when a table changes in a way that SQLite cannot
    /// apply with `ALTER` (column type change, FK add/drop).
    /// SQLite materializes it (create-new/copy/drop/rename); the rest of the engines
    /// ignore it and use the granular ALTERs.
    RebuildTable {
        /// Desired final schema of the table.
        table: Table,
        /// Columns to copy from the old table (those present in both).
        copy_columns: Vec<String>,
    },
    /// Inserts a seed data row (`HasData`).
    InsertData {
        schema: Option<String>,
        table: String,
        row: std::collections::BTreeMap<String, serde_json::Value>,
    },
    /// Deletes a seed row by its primary key.
    DeleteData {
        schema: Option<String>,
        table: String,
        key: std::collections::BTreeMap<String, serde_json::Value>,
    },
    /// Updates the non-key values of a seed row.
    UpdateData {
        schema: Option<String>,
        table: String,
        key: std::collections::BTreeMap<String, serde_json::Value>,
        values: std::collections::BTreeMap<String, serde_json::Value>,
    },
}

/// Computes the operations needed to transform `from` into `to`.
///
/// Order (safe with respect to dependencies):
/// 1. `DropForeignKey`, `DropIndex` of tables that change or disappear.
/// 2. `CreateTable` of new tables.
/// 3. Column alterations on existing tables.
/// 4. `AddForeignKey`, `CreateIndex` (the referenced tables already exist).
/// 5. `DropTable` of obsolete tables.
///
/// > Note: renames are not detected yet (they look like drop+add).
pub fn diff(from: &DatabaseModel, to: &DatabaseModel) -> Vec<Operation> {
    let mut ensure_extensions = Vec::new();
    let mut drop_triggers = Vec::new();
    let mut drop_fks = Vec::new();
    let mut drop_indexes = Vec::new();
    let mut drop_functions = Vec::new();
    let mut create_functions = Vec::new();
    let mut create_tables = Vec::new();
    let mut column_ops = Vec::new();
    let mut add_fks = Vec::new();
    let mut create_indexes = Vec::new();
    let mut create_triggers = Vec::new();
    let mut rebuilds = Vec::new();
    let mut data_ops = Vec::new();
    let mut raw_sql = Vec::new();
    let mut drop_tables = Vec::new();

    // New raw DDL objects (SQL Server full-text, etc.). Verbatim, at the
    // end (they depend on already-created tables/indexes).
    for obj in &to.raw_objects {
        if !from.raw_objects.contains(obj) {
            raw_sql.push(Operation::RawSql { sql: obj.clone() });
        }
    }

    // Extensions to ensure: those declared explicitly in the model
    // (read from the DB) plus `vector` if inferred from type/index usage
    // (model-first flow, which does not list extensions). They are prepended to all the DDL.
    let needed = wanted_extensions(to);
    let already = wanted_extensions(from);
    for ext in &needed {
        if !already.contains(ext) {
            ensure_extensions.push(Operation::EnsureExtension { name: ext.clone() });
        }
    }

    // Schema functions: create new or redefined ones (CREATE OR REPLACE),
    // drop those no longer present. Created before the triggers that use them.
    for f in &to.functions {
        match from
            .functions
            .iter()
            .find(|x| x.name == f.name && x.schema == f.schema)
        {
            Some(old) if old.definition == f.definition => {}
            _ => create_functions.push(Operation::CreateFunction {
                function: f.clone(),
            }),
        }
    }
    for f in &from.functions {
        if !to
            .functions
            .iter()
            .any(|x| x.name == f.name && x.schema == f.schema)
        {
            drop_functions.push(Operation::DropFunction {
                schema: f.schema.clone(),
                name: f.name.clone(),
            });
        }
    }

    // New tables (and their FKs/indexes/triggers).
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
            for tg in &t.triggers {
                create_triggers.push(Operation::CreateTrigger {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    trigger: tg.clone(),
                });
            }
            for row in &t.seed_data {
                data_ops.push(Operation::InsertData {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    row: row.clone(),
                });
            }
        }
    }

    // Tables present in both → diff of columns, FKs, indexes and triggers.
    for new_t in &to.tables {
        let Some(old_t) = from.table(new_t.schema.as_deref(), &new_t.name) else {
            continue;
        };
        diff_columns(old_t, new_t, &mut column_ops);
        diff_foreign_keys(old_t, new_t, &mut add_fks, &mut drop_fks);
        diff_indexes(old_t, new_t, &mut create_indexes, &mut drop_indexes);
        diff_triggers(old_t, new_t, &mut create_triggers, &mut drop_triggers);

        // Changes that SQLite cannot do with ALTER (column type or FK):
        // a full rebuild is also emitted, which SQLite materializes.
        let column_type_changed = new_t
            .columns
            .iter()
            .any(|nc| matches!(old_t.column(&nc.name), Some(oc) if oc != nc));
        let fk_changed = new_t
            .foreign_keys
            .iter()
            .any(|f| !old_t.foreign_keys.iter().any(|o| o.name == f.name))
            || old_t
                .foreign_keys
                .iter()
                .any(|f| !new_t.foreign_keys.iter().any(|o| o.name == f.name));
        if column_type_changed || fk_changed {
            let copy_columns = new_t
                .columns
                .iter()
                .filter(|c| old_t.column(&c.name).is_some())
                .map(|c| c.name.clone())
                .collect();
            rebuilds.push(Operation::RebuildTable {
                table: new_t.clone(),
                copy_columns,
            });
        }

        diff_seed_data(old_t, new_t, &mut data_ops);
    }

    // Removed tables (triggers/FKs/indexes first, then the table).
    // Note: in the DB triggers drop with the table; here we only emit the
    // drops for tables that change, not for those that disappear.
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
    ops.extend(ensure_extensions);
    ops.extend(drop_triggers);
    ops.extend(drop_fks);
    ops.extend(drop_indexes);
    ops.extend(drop_functions);
    ops.extend(create_tables);
    ops.extend(column_ops);
    ops.extend(add_fks);
    // Functions after the tables (they may reference them) and before indexes and
    // triggers (which may use them, e.g. functional expression indexes).
    ops.extend(create_functions);
    ops.extend(create_indexes);
    ops.extend(create_triggers);
    ops.extend(rebuilds);
    ops.extend(data_ops);
    ops.extend(raw_sql);
    ops.extend(drop_tables);
    ops
}

/// Key of a seed row: the subset of PK columns (or the whole row if there is no
/// PK).
fn seed_key(
    row: &std::collections::BTreeMap<String, serde_json::Value>,
    pk_cols: &[String],
) -> std::collections::BTreeMap<String, serde_json::Value> {
    if pk_cols.is_empty() {
        return row.clone();
    }
    pk_cols
        .iter()
        .filter_map(|c| row.get(c).map(|v| (c.clone(), v.clone())))
        .collect()
}

/// Diff of seed data (`HasData`): generates Insert/Update/Delete per row,
/// matching by the seed match key — the table's explicit `seed_key` (a natural
/// key, so a DB-generated PK can be omitted from rows) when set, otherwise the
/// primary key.
fn diff_seed_data(old_t: &Table, new_t: &Table, ops: &mut Vec<Operation>) {
    let key_cols = if !new_t.seed_key.is_empty() {
        new_t.seed_key.clone()
    } else {
        new_t
            .primary_key
            .as_ref()
            .map(|p| p.columns.clone())
            .unwrap_or_default()
    };

    for new_row in &new_t.seed_data {
        let key = seed_key(new_row, &key_cols);
        match old_t
            .seed_data
            .iter()
            .find(|r| seed_key(r, &key_cols) == key)
        {
            None => ops.push(Operation::InsertData {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                row: new_row.clone(),
            }),
            Some(old_row) if old_row != new_row => {
                let values = new_row
                    .iter()
                    .filter(|(c, _)| !key_cols.contains(c))
                    .map(|(c, v)| (c.clone(), v.clone()))
                    .collect();
                ops.push(Operation::UpdateData {
                    schema: new_t.schema.clone(),
                    table: new_t.name.clone(),
                    key,
                    values,
                });
            }
            Some(_) => {}
        }
    }
    for old_row in &old_t.seed_data {
        let key = seed_key(old_row, &key_cols);
        if !new_t
            .seed_data
            .iter()
            .any(|r| seed_key(r, &key_cols) == key)
        {
            ops.push(Operation::DeleteData {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                key,
            });
        }
    }
}

/// Does the model use the `vector` extension (pgvector)? True if any column is
/// of type `vector(...)` / `Pgvector.Vector`, or some index uses `hnsw`/`ivfflat`.
fn requires_vector_extension(model: &DatabaseModel) -> bool {
    model.tables.iter().any(|t| {
        t.columns.iter().any(|c| {
            c.store_type
                .as_deref()
                .map(|s| s.starts_with("vector"))
                .unwrap_or(false)
                || c.clr_type.as_deref() == Some("Pgvector.Vector")
        }) || t
            .indexes
            .iter()
            .any(|i| matches!(i.method.as_deref(), Some("hnsw") | Some("ivfflat")))
    })
}

/// Set of extensions the model needs: those declared explicitly
/// (read from the DB) plus `vector` if inferred from type/index usage.
fn wanted_extensions(model: &DatabaseModel) -> std::collections::BTreeSet<String> {
    let mut set: std::collections::BTreeSet<String> = model.extensions.iter().cloned().collect();
    if requires_vector_extension(model) {
        set.insert("vector".to_string());
    }
    set
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

fn diff_foreign_keys(
    old_t: &Table,
    new_t: &Table,
    add: &mut Vec<Operation>,
    drop: &mut Vec<Operation>,
) {
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

fn diff_indexes(
    old_t: &Table,
    new_t: &Table,
    create: &mut Vec<Operation>,
    drop: &mut Vec<Operation>,
) {
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

fn diff_triggers(
    old_t: &Table,
    new_t: &Table,
    create: &mut Vec<Operation>,
    drop: &mut Vec<Operation>,
) {
    // Postgres does not guarantee CREATE OR REPLACE TRIGGER in all versions, so
    // a definition change is treated as drop + create.
    for tg in &new_t.triggers {
        match old_t.triggers.iter().find(|t| t.name == tg.name) {
            Some(old) if old.definition == tg.definition => {}
            Some(_) => {
                drop.push(Operation::DropTrigger {
                    schema: new_t.schema.clone(),
                    table: new_t.name.clone(),
                    name: tg.name.clone(),
                });
                create.push(Operation::CreateTrigger {
                    schema: new_t.schema.clone(),
                    table: new_t.name.clone(),
                    trigger: tg.clone(),
                });
            }
            None => create.push(Operation::CreateTrigger {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                trigger: tg.clone(),
            }),
        }
    }
    for tg in &old_t.triggers {
        if !new_t.triggers.iter().any(|t| t.name == tg.name) {
            drop.push(Operation::DropTrigger {
                schema: new_t.schema.clone(),
                table: new_t.name.clone(),
                name: tg.name.clone(),
            });
        }
    }
}

/// Applies an operation to an in-memory model. Allows rebuilding the snapshot
/// by replaying the `up` operations of a sequence of migrations.
pub fn apply_operation(model: &mut DatabaseModel, op: &Operation) {
    match op {
        Operation::CreateTable { table } => model.tables.push(table.clone()),
        Operation::DropTable { schema, name } => model
            .tables
            .retain(|t| !(t.schema.as_deref() == schema.as_deref() && &t.name == name)),
        Operation::AddColumn {
            schema,
            table,
            column,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.columns.push(column.clone());
            }
        }
        Operation::DropColumn {
            schema,
            table,
            name,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.columns.retain(|c| &c.name != name);
            }
        }
        Operation::AlterColumn {
            schema, table, new, ..
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                if let Some(c) = t.columns.iter_mut().find(|c| c.name == new.name) {
                    *c = new.clone();
                }
            }
        }
        Operation::AddForeignKey {
            schema,
            table,
            foreign_key,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.foreign_keys.push(foreign_key.clone());
            }
        }
        Operation::DropForeignKey {
            schema,
            table,
            name,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.foreign_keys.retain(|f| &f.name != name);
            }
        }
        Operation::CreateIndex {
            schema,
            table,
            index,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.indexes.push(index.clone());
            }
        }
        Operation::DropIndex {
            schema,
            table,
            name,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.indexes.retain(|i| &i.name != name);
            }
        }
        Operation::EnsureExtension { name } => {
            if !model.extensions.contains(name) {
                model.extensions.push(name.clone());
            }
        }
        Operation::CreateFunction { function } => {
            match model
                .functions
                .iter_mut()
                .find(|f| f.name == function.name && f.schema == function.schema)
            {
                Some(existing) => *existing = function.clone(),
                None => model.functions.push(function.clone()),
            }
        }
        Operation::DropFunction { schema, name } => {
            model
                .functions
                .retain(|f| !(f.schema.as_deref() == schema.as_deref() && &f.name == name));
        }
        Operation::CreateTrigger {
            schema,
            table,
            trigger,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.triggers.retain(|x| x.name != trigger.name);
                t.triggers.push(trigger.clone());
            }
        }
        Operation::DropTrigger {
            schema,
            table,
            name,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                t.triggers.retain(|x| &x.name != name);
            }
        }
        Operation::RawSql { sql } => {
            if !model.raw_objects.contains(sql) {
                model.raw_objects.push(sql.clone());
            }
        }
        Operation::RebuildTable { table, .. } => {
            if let Some(t) = model
                .tables
                .iter_mut()
                .find(|t| t.schema.as_deref() == table.schema.as_deref() && t.name == table.name)
            {
                *t = table.clone();
            }
        }
        Operation::InsertData { schema, table, row } => {
            if let Some(t) = find_mut(model, schema, table) {
                let pk = t
                    .primary_key
                    .as_ref()
                    .map(|p| p.columns.clone())
                    .unwrap_or_default();
                let key = seed_key(row, &pk);
                if !t.seed_data.iter().any(|r| seed_key(r, &pk) == key) {
                    t.seed_data.push(row.clone());
                }
            }
        }
        Operation::DeleteData { schema, table, key } => {
            if let Some(t) = find_mut(model, schema, table) {
                let pk = t
                    .primary_key
                    .as_ref()
                    .map(|p| p.columns.clone())
                    .unwrap_or_default();
                t.seed_data.retain(|r| &seed_key(r, &pk) != key);
            }
        }
        Operation::UpdateData {
            schema,
            table,
            key,
            values,
        } => {
            if let Some(t) = find_mut(model, schema, table) {
                let pk = t
                    .primary_key
                    .as_ref()
                    .map(|p| p.columns.clone())
                    .unwrap_or_default();
                if let Some(r) = t.seed_data.iter_mut().find(|r| &seed_key(r, &pk) == key) {
                    for (c, v) in values {
                        r.insert(c.clone(), v.clone());
                    }
                }
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
            triggers: vec![],
            seed_data: vec![],
            seed_key: vec![],
        }
    }

    fn seed_row(code: &str, name: &str) -> std::collections::BTreeMap<String, serde_json::Value> {
        let mut m = std::collections::BTreeMap::new();
        m.insert("Code".to_string(), serde_json::json!(code));
        m.insert("Name".to_string(), serde_json::json!(name));
        m
    }

    fn tenant_with_seed(rows: Vec<std::collections::BTreeMap<String, serde_json::Value>>) -> Table {
        // Note: rows carry no `Id` — the DB generates it; seeds match by `Code`.
        let mut t = table("Tenant", &["Id", "Code", "Name"]);
        t.seed_key = vec!["Code".into()];
        t.seed_data = rows;
        t
    }

    fn model_with(t: Table) -> DatabaseModel {
        let mut m = DatabaseModel::empty();
        m.tables.push(t);
        m
    }

    #[test]
    fn seed_key_inserts_only_the_new_row_by_natural_key() {
        let prev = model_with(tenant_with_seed(vec![seed_row("ACME", "ACME")]));
        let cur = model_with(tenant_with_seed(vec![
            seed_row("ACME", "ACME"),
            seed_row("GLX", "Globex"),
        ]));

        let inserts: Vec<_> = diff(&prev, &cur)
            .into_iter()
            .filter(|o| matches!(o, Operation::InsertData { .. }))
            .collect();
        assert_eq!(inserts.len(), 1, "only the new row should be inserted");
        match &inserts[0] {
            Operation::InsertData { row, .. } => {
                assert_eq!(row.get("Code").unwrap(), &serde_json::json!("GLX"));
                assert!(!row.contains_key("Id"), "Id is left to the DB default");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn seed_key_updates_by_natural_key_excluding_the_key_columns() {
        let prev = model_with(tenant_with_seed(vec![seed_row("ACME", "ACME")]));
        let cur = model_with(tenant_with_seed(vec![seed_row("ACME", "ACME Inc")]));

        let updates: Vec<_> = diff(&prev, &cur)
            .into_iter()
            .filter(|o| matches!(o, Operation::UpdateData { .. }))
            .collect();
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            Operation::UpdateData { key, values, .. } => {
                assert_eq!(key.get("Code").unwrap(), &serde_json::json!("ACME"));
                assert_eq!(values.get("Name").unwrap(), &serde_json::json!("ACME Inc"));
                assert!(!values.contains_key("Code"), "the match key is not updated");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn detects_new_table() {
        let from = DatabaseModel::empty();
        let mut to = DatabaseModel::empty();
        to.tables.push(table("Customer", &["Id", "Name"]));
        let ops = diff(&from, &to);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], Operation::CreateTable { .. }));
    }

    #[test]
    fn new_table_with_fk_and_index_orders_create_before_fk() {
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
            method: None,
            operators: vec![],
            expression: None,
        });
        to.tables.push(order);

        let ops = diff(&from, &to);
        // create_table, add_foreign_key, create_index — in that order.
        assert!(matches!(ops[0], Operation::CreateTable { .. }));
        assert!(matches!(ops[1], Operation::AddForeignKey { .. }));
        assert!(matches!(ops[2], Operation::CreateIndex { .. }));
    }

    #[test]
    fn detects_added_and_removed_column() {
        let mut from = DatabaseModel::empty();
        from.tables.push(table("Customer", &["Id", "Name"]));
        let mut to = DatabaseModel::empty();
        to.tables.push(table("Customer", &["Id", "Email"]));
        let ops = diff(&from, &to);
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o, Operation::AddColumn { .. }))
                .count(),
            1
        );
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o, Operation::DropColumn { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn identical_model_no_changes() {
        let mut m = DatabaseModel::empty();
        m.tables.push(table("Customer", &["Id"]));
        assert!(diff(&m, &m).is_empty());
    }

    fn trig(name: &str, def: &str) -> crate::model::Trigger {
        use crate::model::{TriggerEvent, TriggerTiming};
        crate::model::Trigger {
            name: name.into(),
            table: "documents".into(),
            schema: None,
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert],
            function: Some("normalize_body".into()),
            definition: def.into(),
        }
    }

    fn func(name: &str, def: &str) -> crate::model::DbFunction {
        crate::model::DbFunction {
            name: name.into(),
            schema: None,
            definition: def.into(),
        }
    }

    #[test]
    fn function_before_trigger_when_creating() {
        let from = DatabaseModel::empty();
        let mut to = DatabaseModel::empty();
        to.functions.push(func(
            "normalize_body",
            "CREATE FUNCTION normalize_body() ...",
        ));
        let mut t = table("documents", &["Id"]);
        t.triggers
            .push(trig("trg_norm", "CREATE TRIGGER trg_norm ..."));
        to.tables.push(t);

        let ops = diff(&from, &to);
        let fpos = ops
            .iter()
            .position(|o| matches!(o, Operation::CreateFunction { .. }))
            .unwrap();
        let tpos = ops
            .iter()
            .position(|o| matches!(o, Operation::CreateTrigger { .. }))
            .unwrap();
        assert!(
            fpos < tpos,
            "the function must be created before the trigger"
        );
    }

    #[test]
    fn redefined_trigger_is_drop_plus_create() {
        let mut from = DatabaseModel::empty();
        let mut t0 = table("documents", &["Id"]);
        t0.triggers.push(trig("trg_norm", "DEF v1"));
        from.tables.push(t0);

        let mut to = DatabaseModel::empty();
        let mut t1 = table("documents", &["Id"]);
        t1.triggers.push(trig("trg_norm", "DEF v2"));
        to.tables.push(t1);

        let ops = diff(&from, &to);
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o, Operation::DropTrigger { .. }))
                .count(),
            1
        );
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o, Operation::CreateTrigger { .. }))
                .count(),
            1
        );
        // The drop goes before the create.
        let dpos = ops
            .iter()
            .position(|o| matches!(o, Operation::DropTrigger { .. }))
            .unwrap();
        let cpos = ops
            .iter()
            .position(|o| matches!(o, Operation::CreateTrigger { .. }))
            .unwrap();
        assert!(dpos < cpos);
    }

    fn vector_col(name: &str) -> Column {
        let mut c = col(name);
        c.store_type = Some("vector(384)".into());
        c.clr_type = Some("Pgvector.Vector".into());
        c
    }

    #[test]
    fn ensure_extension_vector_is_prepended_and_is_idempotent() {
        let from = DatabaseModel::empty();
        let mut to = DatabaseModel::empty();
        let mut t = table("docs", &["id"]);
        t.columns.push(vector_col("embedding"));
        to.tables.push(t);

        let ops = diff(&from, &to);
        assert!(
            matches!(ops.first(), Some(Operation::EnsureExtension { name }) if name == "vector"),
            "EnsureExtension(vector) must come first: {ops:?}"
        );
        // Idempotency: if both models already use vector, it is not re-emitted.
        assert!(!diff(&to, &to)
            .iter()
            .any(|o| matches!(o, Operation::EnsureExtension { .. })));
    }

    #[test]
    fn type_change_on_existing_table_emits_rebuild() {
        let mut from = DatabaseModel::empty();
        let mut t0 = table("docs", &["id", "title"]);
        // title goes from integer (col() default) to string in `to`.
        from.tables.push(t0.clone());

        let mut to = DatabaseModel::empty();
        let mut title = col("title");
        title.store_type = Some("text".into());
        title.clr_type = Some("System.String".into());
        t0.columns = vec![col("id"), title];
        to.tables.push(t0);

        let ops = diff(&from, &to);
        let rebuild = ops.iter().find_map(|o| match o {
            Operation::RebuildTable {
                table,
                copy_columns,
            } => Some((table, copy_columns)),
            _ => None,
        });
        let (rt, copy) = rebuild.expect("must emit RebuildTable");
        assert_eq!(rt.name, "docs");
        assert_eq!(copy, &vec!["id".to_string(), "title".to_string()]);
        // Idempotency: no changes means no rebuild.
        assert!(!diff(&to, &to)
            .iter()
            .any(|o| matches!(o, Operation::RebuildTable { .. })));
    }

    #[test]
    fn seed_data_generates_insert_update_delete() {
        use serde_json::json;
        let row = |id: i64, name: &str| {
            let mut m = std::collections::BTreeMap::new();
            m.insert("Id".to_string(), json!(id));
            m.insert("Name".to_string(), json!(name));
            m
        };
        let mk = |rows: Vec<_>| {
            let mut m = DatabaseModel::empty();
            let mut t = table("Category", &["Id", "Name"]);
            t.primary_key = Some(PrimaryKey {
                name: "PK".into(),
                columns: vec!["Id".into()],
            });
            t.seed_data = rows;
            m.tables.push(t);
            m
        };
        // from: {1:A, 2:B}; to: {1:A, 2:B2, 3:C} → update(2), insert(3)
        let from = mk(vec![row(1, "A"), row(2, "B")]);
        let to = mk(vec![row(1, "A"), row(2, "B2"), row(3, "C")]);
        let ops = diff(&from, &to);
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o, Operation::InsertData { .. }))
                .count(),
            1
        );
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o, Operation::UpdateData { .. }))
                .count(),
            1
        );
        // and in reverse there is a delete.
        let back = diff(&to, &from);
        assert_eq!(
            back.iter()
                .filter(|o| matches!(o, Operation::DeleteData { .. }))
                .count(),
            1
        );

        // Idempotency + rebuild.
        let mut rebuilt = DatabaseModel::empty();
        for op in &diff(&DatabaseModel::empty(), &to) {
            apply_operation(&mut rebuilt, op);
        }
        assert!(diff(&rebuilt, &to).is_empty(), "rebuilt seed is equivalent");
    }

    #[test]
    fn apply_rebuilds_functions_and_triggers() {
        let mut target = DatabaseModel::empty();
        target.functions.push(func("f", "DEF"));
        let mut t = table("documents", &["Id"]);
        t.triggers.push(trig("trg", "TRGDEF"));
        target.tables.push(t);

        let ops = diff(&DatabaseModel::empty(), &target);
        let mut rebuilt = DatabaseModel::empty();
        for op in &ops {
            apply_operation(&mut rebuilt, op);
        }
        assert_eq!(rebuilt.functions.len(), 1);
        assert_eq!(rebuilt.table(None, "documents").unwrap().triggers.len(), 1);
        assert!(diff(&rebuilt, &target).is_empty(), "rebuild is equivalent");
    }

    #[test]
    fn apply_operation_rebuilds_the_model() {
        let mut target = DatabaseModel::empty();
        let mut t = table("Customer", &["Id", "Name"]);
        t.primary_key = Some(PrimaryKey {
            name: "PK_Customer".into(),
            columns: vec!["Id".into()],
        });
        target.tables.push(t);

        // Replaying the `up` ops on an empty model must give the same model.
        let ops = diff(&DatabaseModel::empty(), &target);
        let mut rebuilt = DatabaseModel::empty();
        for op in &ops {
            apply_operation(&mut rebuilt, op);
        }
        assert_eq!(rebuilt.table(None, "Customer").unwrap().columns.len(), 2);
        assert!(diff(&rebuilt, &target).is_empty(), "rebuild is equivalent");
    }
}
