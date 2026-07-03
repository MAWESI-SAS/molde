//! MySQL / MariaDB provider.
//!
//! Differences from Postgres: quoting with backticks, `AUTO_INCREMENT`,
//! `MODIFY COLUMN` to alter, `DROP FOREIGN KEY` / `DROP INDEX ... ON`. It does
//! not qualify by schema (the database is selected by the connection).

use molde_core::diff::Operation;
use molde_core::model::{Column, ForeignKey, Index, ReferentialAction, Table};

use crate::generator::{ProviderError, SqlGenerator};

fn on_delete_clause(action: ReferentialAction) -> &'static str {
    match action {
        ReferentialAction::Cascade => " ON DELETE CASCADE",
        ReferentialAction::SetNull => " ON DELETE SET NULL",
        ReferentialAction::SetDefault => " ON DELETE SET DEFAULT",
        ReferentialAction::Restrict => " ON DELETE RESTRICT",
        ReferentialAction::NoAction => "",
    }
}

/// `ON UPDATE` clause for a referential action (empty for `NoAction`).
fn on_update_clause(action: ReferentialAction) -> &'static str {
    match action {
        ReferentialAction::Cascade => " ON UPDATE CASCADE",
        ReferentialAction::SetNull => " ON UPDATE SET NULL",
        ReferentialAction::SetDefault => " ON UPDATE SET DEFAULT",
        ReferentialAction::Restrict => " ON UPDATE RESTRICT",
        ReferentialAction::NoAction => "",
    }
}

pub struct MySqlGenerator;

impl MySqlGenerator {
    pub fn new() -> Self {
        Self
    }

    fn column_def(&self, column: &Column) -> Result<String, ProviderError> {
        let mut parts = vec![self.quote_ident(&column.name), self.store_type_for(column)?];
        // Generated column (STORED/VIRTUAL). Excludes AUTO_INCREMENT and DEFAULT.
        if let Some(expr) = &column.computed_sql {
            let kind = if column.computed_stored {
                "STORED"
            } else {
                "VIRTUAL"
            };
            parts.push(format!("GENERATED ALWAYS AS ({expr}) {kind}"));
            if !column.is_nullable {
                parts.push("NOT NULL".into());
            }
            return Ok(parts.join(" "));
        }
        if !column.is_nullable {
            parts.push("NOT NULL".into());
        }
        if column.is_identity {
            parts.push("AUTO_INCREMENT".into());
        }
        if let Some(def) = &column.default_value_sql {
            parts.push(format!("DEFAULT {def}"));
        }
        Ok(parts.join(" "))
    }

    fn create_table(&self, table: &Table) -> Result<String, ProviderError> {
        let mut lines: Vec<String> = Vec::new();
        for c in &table.columns {
            lines.push(format!("    {}", self.column_def(c)?));
        }
        if let Some(pk) = &table.primary_key {
            // MySQL ignores the PK name (it is always PRIMARY).
            let cols: Vec<String> = pk.columns.iter().map(|c| self.quote_ident(c)).collect();
            lines.push(format!("    PRIMARY KEY ({})", cols.join(", ")));
        }
        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            self.quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn add_foreign_key(&self, table: &str, fk: &ForeignKey) -> String {
        let cols: Vec<String> = fk.columns.iter().map(|c| self.quote_ident(c)).collect();
        let pcols: Vec<String> = fk
            .principal_columns
            .iter()
            .map(|c| self.quote_ident(c))
            .collect();
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){}{};",
            self.quote_ident(table),
            self.quote_ident(&fk.name),
            cols.join(", "),
            self.quote_ident(&fk.principal_table),
            pcols.join(", "),
            on_delete_clause(fk.on_delete),
            on_update_clause(fk.on_update),
        )
    }

    fn create_index(&self, table: &str, index: &Index) -> String {
        // FULLTEXT/SPATIAL take priority over UNIQUE (MySQL does not combine them).
        let kind = match index.method.as_deref() {
            Some("fulltext") => "FULLTEXT ",
            Some("spatial") => "SPATIAL ",
            _ if index.is_unique => "UNIQUE ",
            _ => "",
        };
        let cols: Vec<String> = index.columns.iter().map(|c| self.quote_ident(c)).collect();
        format!(
            "CREATE {kind}INDEX {} ON {} ({});",
            self.quote_ident(&index.name),
            self.quote_ident(table),
            cols.join(", "),
        )
    }
}

impl Default for MySqlGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlGenerator for MySqlGenerator {
    fn name(&self) -> &'static str {
        "mysql"
    }

    fn quote_ident(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }

    fn store_type_for(&self, column: &Column) -> Result<String, ProviderError> {
        if let Some(st) = &column.store_type {
            return Ok(st.clone());
        }
        let clr = column.clr_type.as_deref().unwrap_or("");
        let mapped = match clr {
            "System.Int16" => "smallint",
            "System.Int32" => "int",
            "System.Int64" => "bigint",
            "System.Boolean" => "tinyint(1)",
            "System.Single" => "float",
            "System.Double" => "double",
            "System.Decimal" => match (column.precision, column.scale) {
                (Some(p), Some(s)) => return Ok(format!("decimal({p},{s})")),
                _ => "decimal(18,2)",
            },
            "System.DateTime" => "datetime(6)",
            "System.DateOnly" => "date",
            "System.TimeOnly" => "time",
            "System.DateTimeOffset" => "datetime(6)", // MySQL has no type with offset
            "System.Guid" => "char(36)",
            "System.Byte[]" => "longblob",
            "System.String" => {
                return Ok(match column.max_length {
                    Some(n) if n > 0 => format!("varchar({n})"),
                    _ => "longtext".to_string(),
                });
            }
            _ => {
                return Err(ProviderError::UnmappedType {
                    clr: clr.to_string(),
                    column: column.name.clone(),
                })
            }
        };
        Ok(mapped.to_string())
    }

    fn emit(&self, op: &Operation) -> Result<Vec<String>, ProviderError> {
        let sql = match op {
            Operation::CreateTable { table } => vec![self.create_table(table)?],
            Operation::DropTable { name, .. } => {
                vec![format!("DROP TABLE {};", self.quote_ident(name))]
            }
            Operation::AddColumn { table, column, .. } => vec![format!(
                "ALTER TABLE {} ADD COLUMN {};",
                self.quote_ident(table),
                self.column_def(column)?
            )],
            Operation::DropColumn { table, name, .. } => vec![format!(
                "ALTER TABLE {} DROP COLUMN {};",
                self.quote_ident(table),
                self.quote_ident(name)
            )],
            Operation::AlterColumn { table, new, .. } => vec![format!(
                "ALTER TABLE {} MODIFY COLUMN {};",
                self.quote_ident(table),
                self.column_def(new)?
            )],
            Operation::AddForeignKey {
                table, foreign_key, ..
            } => {
                vec![self.add_foreign_key(table, foreign_key)]
            }
            Operation::DropForeignKey { table, name, .. } => vec![format!(
                "ALTER TABLE {} DROP FOREIGN KEY {};",
                self.quote_ident(table),
                self.quote_ident(name)
            )],
            Operation::CreateIndex { table, index, .. } => vec![self.create_index(table, index)],
            Operation::DropIndex { table, name, .. } => vec![format!(
                "DROP INDEX {} ON {};",
                self.quote_ident(name),
                self.quote_ident(table)
            )],
            Operation::AddCheckConstraint {
                schema: _,
                table,
                check,
            } => vec![format!(
                "ALTER TABLE {} ADD CONSTRAINT {} {};",
                self.quote_ident(table),
                self.quote_ident(&check.name),
                check.expression
            )],
            Operation::DropCheckConstraint {
                schema: _,
                table,
                name,
            } => vec![format!(
                "ALTER TABLE {} DROP CHECK {};",
                self.quote_ident(table),
                self.quote_ident(name)
            )],
            Operation::CreateView { view } => {
                let body = view.definition.trim().trim_end_matches(';');
                vec![format!(
                    "CREATE VIEW {} AS\n{};",
                    self.quote_ident(&view.name),
                    body
                )]
            }
            Operation::DropView { schema: _, name } => {
                vec![format!("DROP VIEW IF EXISTS {};", self.quote_ident(name))]
            }
            Operation::RawSql { sql } => vec![sql.clone()],
            Operation::RebuildTable { .. } => Vec::new(),
            Operation::InsertData { schema, table, row } => {
                self.emit_insert_data(schema.as_deref(), table, row)
            }
            Operation::DeleteData { schema, table, key } => {
                self.emit_delete_data(schema.as_deref(), table, key)
            }
            Operation::UpdateData {
                schema,
                table,
                key,
                values,
            } => self.emit_update_data(schema.as_deref(), table, key, values),
            Operation::EnsureExtension { .. }
            | Operation::CreateFunction { .. }
            | Operation::DropFunction { .. }
            | Operation::CreateTrigger { .. }
            | Operation::DropTrigger { .. } => self.skip_db_object(op),
        };
        Ok(sql)
    }
}
