//! The [`SqlGenerator`] trait and shared utilities.
//!
//! Each engine implements this trait. The core (`molde-core`) produces
//! engine-agnostic [`Operation`]s; the provider translates them to the concrete
//! SQL dialect, including the CLR type → store type mapping.

use std::collections::BTreeMap;

use molde_core::diff::Operation;
use molde_core::model::Column;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("operation not yet supported by provider '{provider}': {detail}")]
    Unsupported {
        provider: &'static str,
        detail: String,
    },
    #[error("could not map CLR type '{clr}' (column '{column}')")]
    UnmappedType { clr: String, column: String },
}

/// Generates SQL for a concrete engine from migration operations.
pub trait SqlGenerator {
    /// Provider name (for diagnostics and `ProductVersion`).
    fn name(&self) -> &'static str;

    /// Identifier quoting character(s). SQLite/Postgres use `"`,
    /// SQL Server `[ ]`, MySQL backticks.
    fn quote_ident(&self, ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }

    /// Maps a column to its store type. If the column already carries an
    /// explicit `store_type` it is respected; otherwise the provider derives it
    /// from the CLR type + facets.
    fn store_type_for(&self, column: &Column) -> Result<String, ProviderError>;

    /// SQL to create the `__EFMigrationsHistory` history table if it does not exist.
    /// The default works for Postgres/SQLite/MySQL (`CREATE TABLE IF NOT EXISTS`);
    /// SQL Server overrides it (it does not support that syntax).
    fn create_history_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {t} ({id} varchar(150) NOT NULL, \
             {ver} varchar(32) NOT NULL, PRIMARY KEY ({id}));",
            t = self.quote_ident("__EFMigrationsHistory"),
            id = self.quote_ident("MigrationId"),
            ver = self.quote_ident("ProductVersion"),
        )
    }

    /// Translates a single operation into one or more SQL statements.
    fn emit(&self, op: &Operation) -> Result<Vec<String>, ProviderError>;

    /// Warn-skip for non-modelable DB objects (functions/triggers) on engines
    /// that do not support them yet. Returns empty SQL with a warning, just like
    /// the handling of FKs in SQLite. Postgres implements them for real.
    fn skip_db_object(&self, op: &Operation) -> Vec<String> {
        let (kind, name) = match op {
            Operation::EnsureExtension { name } => ("CREATE EXTENSION", name.as_str()),
            Operation::CreateFunction { function } => ("CREATE FUNCTION", function.name.as_str()),
            Operation::DropFunction { name, .. } => ("DROP FUNCTION", name.as_str()),
            Operation::CreateTrigger { trigger, .. } => ("CREATE TRIGGER", trigger.name.as_str()),
            Operation::DropTrigger { name, .. } => ("DROP TRIGGER", name.as_str()),
            _ => ("operation", ""),
        };
        tracing::warn!(
            "provider '{}' does not support {kind}; skipping '{name}'",
            self.name()
        );
        Vec::new()
    }

    /// SQL literal for `true`/`false`. Defaults to `1`/`0` (SQLite/MySQL/SQL
    /// Server); Postgres overrides it with `TRUE`/`FALSE`.
    fn bool_literal(&self, b: bool) -> &'static str {
        if b {
            "1"
        } else {
            "0"
        }
    }

    /// Renders a JSON value as a SQL literal (for seed data).
    fn sql_value(&self, v: &Value) -> String {
        match v {
            Value::Null => "NULL".to_string(),
            Value::Bool(b) => self.bool_literal(*b).to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => format!("'{}'", s.replace('\'', "''")),
            // Arrays/objects: as quoted JSON text.
            other => format!("'{}'", other.to_string().replace('\'', "''")),
        }
    }

    /// Qualified name `schema.table` (or just `table` if there is no schema).
    fn qualify(&self, schema: Option<&str>, name: &str) -> String {
        match schema {
            Some(s) => format!("{}.{}", self.quote_ident(s), self.quote_ident(name)),
            None => self.quote_ident(name),
        }
    }

    /// `INSERT` of a seed row.
    fn emit_insert_data(
        &self,
        schema: Option<&str>,
        table: &str,
        row: &BTreeMap<String, Value>,
    ) -> Vec<String> {
        let cols = row
            .keys()
            .map(|c| self.quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let vals = row
            .values()
            .map(|v| self.sql_value(v))
            .collect::<Vec<_>>()
            .join(", ");
        vec![format!(
            "INSERT INTO {} ({cols}) VALUES ({vals});",
            self.qualify(schema, table)
        )]
    }

    /// `DELETE` of a seed row by its key.
    fn emit_delete_data(
        &self,
        schema: Option<&str>,
        table: &str,
        key: &BTreeMap<String, Value>,
    ) -> Vec<String> {
        let pred = self.key_predicate(key);
        vec![format!(
            "DELETE FROM {} WHERE {pred};",
            self.qualify(schema, table)
        )]
    }

    /// `UPDATE` of the non-key values of a seed row.
    fn emit_update_data(
        &self,
        schema: Option<&str>,
        table: &str,
        key: &BTreeMap<String, Value>,
        values: &BTreeMap<String, Value>,
    ) -> Vec<String> {
        let set = values
            .iter()
            .map(|(c, v)| format!("{} = {}", self.quote_ident(c), self.sql_value(v)))
            .collect::<Vec<_>>()
            .join(", ");
        let pred = self.key_predicate(key);
        vec![format!(
            "UPDATE {} SET {set} WHERE {pred};",
            self.qualify(schema, table)
        )]
    }

    /// `col = val AND …` predicate built from a key.
    fn key_predicate(&self, key: &BTreeMap<String, Value>) -> String {
        key.iter()
            .map(|(c, v)| format!("{} = {}", self.quote_ident(c), self.sql_value(v)))
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// Translates a list of operations (convenience shortcut).
    fn emit_all(&self, ops: &[Operation]) -> Result<Vec<String>, ProviderError> {
        let mut out = Vec::new();
        for op in ops {
            out.extend(self.emit(op)?);
        }
        Ok(out)
    }
}
