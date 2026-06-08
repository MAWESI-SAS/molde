//! Engine-agnostic, in-memory snapshot of a live database's structure, plus the
//! additive diff between two snapshots. Definitions are stored as text exactly as
//! the engine reports them, so comparison is faithful to the live database (it
//! does not go through molde's IR). Engine-specific readers (one per database)
//! produce a `DbSchema`; the diff and the orchestration are shared.

use std::collections::{BTreeMap, BTreeSet};

/// A table and its columns (in physical/ordinal order).
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    /// Full `CREATE TABLE` text when the engine provides one (SQLite/MySQL). The
    /// Postgres engine builds the DDL from `columns` and leaves this `None`.
    pub create_sql: Option<String>,
}

/// A column with the exact type text and its defining attributes.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    /// Exact engine type text (e.g. `character varying(50)`).
    pub type_: String,
    pub not_null: bool,
    /// Default expression, or the generation expression when `is_generated`.
    pub default: Option<String>,
    /// STORED generated column.
    pub is_generated: bool,
}

impl ColumnInfo {
    /// Identity used to detect a definition change on a column present in both DBs.
    pub fn signature(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.type_,
            self.not_null,
            self.default.as_deref().unwrap_or(""),
            self.is_generated
        )
    }
}

/// A table constraint (PK / unique / check / FK).
#[derive(Debug, Clone)]
pub struct ConstraintInfo {
    pub table: String,
    pub name: String,
    /// `p`=PK, `u`=unique, `c`=check, `f`=FK.
    pub kind: char,
    /// Full definition text (e.g. from `pg_get_constraintdef`).
    pub definition: String,
}

impl ConstraintInfo {
    pub fn key(&self) -> String {
        format!("{}|{}", self.table, self.name)
    }
}

/// An index, with its full `CREATE INDEX` definition.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub table: String,
    pub definition: String,
}

/// A routine (function/procedure), with its full `CREATE … FUNCTION` definition.
#[derive(Debug, Clone)]
pub struct RoutineInfo {
    pub name: String,
    pub arguments: String,
    pub definition: String,
}

impl RoutineInfo {
    pub fn key(&self) -> String {
        format!("{}({})", self.name, self.arguments)
    }
}

/// A trigger, with its full `CREATE TRIGGER` definition.
#[derive(Debug, Clone)]
pub struct TriggerInfo {
    pub table: String,
    pub name: String,
    pub definition: String,
}

impl TriggerInfo {
    pub fn key(&self) -> String {
        format!("{}|{}", self.table, self.name)
    }
}

/// A view, with its `SELECT` body.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    pub name: String,
    pub definition: String,
}

/// In-memory snapshot of a database's structure. Keys give a deterministic order.
#[derive(Debug, Default)]
pub struct DbSchema {
    /// Installed extensions (excluding the default `plpgsql`).
    pub extensions: BTreeSet<String>,
    /// key: table name
    pub tables: BTreeMap<String, TableInfo>,
    /// key: `table|name`
    pub constraints: BTreeMap<String, ConstraintInfo>,
    /// key: index name
    pub indexes: BTreeMap<String, IndexInfo>,
    /// key: `name(args)`
    pub functions: BTreeMap<String, RoutineInfo>,
    /// key: `table|name`
    pub triggers: BTreeMap<String, TriggerInfo>,
    /// key: view name
    pub views: BTreeMap<String, ViewInfo>,
    /// MigrationId -> ProductVersion (from `__EFMigrationsHistory`)
    pub migration_history: BTreeMap<String, String>,
}

/// An object present in both databases but defined differently. Never applied —
/// only reported, since the difference may be the developer's own local change.
#[derive(Debug, Clone)]
pub struct Conflict {
    pub category: &'static str,
    pub name: String,
    pub source_definition: String,
    pub target_definition: String,
}

/// The additive delta: everything the source has that the target lacks.
#[derive(Debug, Default)]
pub struct DiffResult {
    pub new_extensions: Vec<String>,
    pub new_tables: Vec<TableInfo>,
    pub new_columns: Vec<(String, ColumnInfo)>,
    pub new_constraints: Vec<ConstraintInfo>,
    pub new_indexes: Vec<IndexInfo>,
    pub new_functions: Vec<RoutineInfo>,
    pub new_triggers: Vec<TriggerInfo>,
    pub new_views: Vec<ViewInfo>,
    pub new_history_rows: Vec<(String, String)>,
    pub conflicts: Vec<Conflict>,
}

impl DiffResult {
    pub fn additive_count(&self) -> usize {
        self.new_extensions.len()
            + self.new_tables.len()
            + self.new_columns.len()
            + self.new_constraints.len()
            + self.new_indexes.len()
            + self.new_functions.len()
            + self.new_triggers.len()
            + self.new_views.len()
            + self.new_history_rows.len()
    }

    pub fn has_changes(&self) -> bool {
        self.additive_count() > 0
    }
}

/// Computes the additive delta `source` → `target`. Strictly additive: anything
/// that exists only in the target is left untouched (it is the developer's own
/// work). Objects present in both but defined differently become conflicts.
pub fn diff(source: &DbSchema, target: &DbSchema) -> DiffResult {
    let mut d = DiffResult::default();

    for ext in &source.extensions {
        if !target.extensions.contains(ext) {
            d.new_extensions.push(ext.clone());
        }
    }

    for (name, table) in &source.tables {
        let Some(target_table) = target.tables.get(name) else {
            d.new_tables.push(table.clone());
            continue;
        };
        let target_columns: BTreeMap<&str, &ColumnInfo> = target_table
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();
        for column in &table.columns {
            match target_columns.get(column.name.as_str()) {
                None => d.new_columns.push((name.clone(), column.clone())),
                Some(tc) if column.signature() != tc.signature() => d.conflicts.push(Conflict {
                    category: "column",
                    name: format!("{name}.{}", column.name),
                    source_definition: column.signature(),
                    target_definition: tc.signature(),
                }),
                Some(_) => {}
            }
        }
    }

    compare(
        &source.constraints,
        &target.constraints,
        "constraint",
        &mut d,
        |d, c| d.new_constraints.push(c.clone()),
    );
    compare(&source.indexes, &target.indexes, "index", &mut d, |d, i| {
        d.new_indexes.push(i.clone())
    });
    compare(
        &source.functions,
        &target.functions,
        "function",
        &mut d,
        |d, f| d.new_functions.push(f.clone()),
    );
    compare(
        &source.triggers,
        &target.triggers,
        "trigger",
        &mut d,
        |d, t| d.new_triggers.push(t.clone()),
    );
    compare(&source.views, &target.views, "view", &mut d, |d, v| {
        d.new_views.push(v.clone())
    });

    for (migration_id, product_version) in &source.migration_history {
        if !target.migration_history.contains_key(migration_id) {
            d.new_history_rows
                .push((migration_id.clone(), product_version.clone()));
        }
    }

    d
}

/// Generic additive comparison over a keyed object map with a `definition`.
fn compare<T: Defined + Clone>(
    source: &BTreeMap<String, T>,
    target: &BTreeMap<String, T>,
    category: &'static str,
    d: &mut DiffResult,
    add_new: impl Fn(&mut DiffResult, &T),
) {
    for (key, s) in source {
        match target.get(key) {
            None => add_new(d, s),
            Some(t) if s.definition() != t.definition() => d.conflicts.push(Conflict {
                category,
                name: key.clone(),
                source_definition: s.definition().to_string(),
                target_definition: t.definition().to_string(),
            }),
            Some(_) => {}
        }
    }
}

/// Objects whose change is detected by comparing a single `definition` text.
trait Defined {
    fn definition(&self) -> &str;
}
impl Defined for ConstraintInfo {
    fn definition(&self) -> &str {
        &self.definition
    }
}
impl Defined for IndexInfo {
    fn definition(&self) -> &str {
        &self.definition
    }
}
impl Defined for RoutineInfo {
    fn definition(&self) -> &str {
        &self.definition
    }
}
impl Defined for TriggerInfo {
    fn definition(&self) -> &str {
        &self.definition
    }
}
impl Defined for ViewInfo {
    fn definition(&self) -> &str {
        &self.definition
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, type_: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_: type_.into(),
            not_null: false,
            default: None,
            is_generated: false,
        }
    }

    #[test]
    fn additive_diff_adds_new_and_ignores_target_only() {
        let mut source = DbSchema::default();
        source.tables.insert(
            "A".into(),
            TableInfo {
                name: "A".into(),
                columns: vec![col("id", "integer"), col("extra", "text")],
                create_sql: None,
            },
        );
        let mut target = DbSchema::default();
        // Target has table A with only `id`, plus its own table B (must be ignored).
        target.tables.insert(
            "A".into(),
            TableInfo {
                name: "A".into(),
                columns: vec![col("id", "integer")],
                create_sql: None,
            },
        );
        target.tables.insert(
            "B".into(),
            TableInfo {
                name: "B".into(),
                columns: vec![col("id", "integer")],
                create_sql: None,
            },
        );

        let d = diff(&source, &target);
        assert!(d.new_tables.is_empty(), "A exists in target, not new");
        assert_eq!(d.new_columns.len(), 1, "extra is the only additive change");
        assert_eq!(d.new_columns[0].1.name, "extra");
        assert!(d.conflicts.is_empty());
    }

    #[test]
    fn changed_column_signature_is_a_conflict_not_applied() {
        let mut source = DbSchema::default();
        source.tables.insert(
            "A".into(),
            TableInfo {
                name: "A".into(),
                columns: vec![col("name", "character varying(50)")],
                create_sql: None,
            },
        );
        let mut target = DbSchema::default();
        target.tables.insert(
            "A".into(),
            TableInfo {
                name: "A".into(),
                columns: vec![col("name", "character varying(30)")],
                create_sql: None,
            },
        );

        let d = diff(&source, &target);
        assert!(!d.has_changes(), "a conflict is not an additive change");
        assert_eq!(d.conflicts.len(), 1);
        assert_eq!(d.conflicts[0].category, "column");
        assert_eq!(d.conflicts[0].name, "A.name");
    }

    #[test]
    fn new_function_and_history_row_are_additive() {
        let mut source = DbSchema::default();
        source.functions.insert(
            "f()".into(),
            RoutineInfo {
                name: "f".into(),
                arguments: String::new(),
                definition: "CREATE FUNCTION f() ...".into(),
            },
        );
        source
            .migration_history
            .insert("2026_Init".into(), "9.0".into());
        let target = DbSchema::default();

        let d = diff(&source, &target);
        assert_eq!(d.new_functions.len(), 1);
        assert_eq!(d.new_history_rows.len(), 1);
        assert!(d.has_changes());
    }
}
