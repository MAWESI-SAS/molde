//! Provider de MySQL / MariaDB.
//!
//! Diferencias frente a Postgres: citado con backticks, `AUTO_INCREMENT`,
//! `MODIFY COLUMN` para alterar, `DROP FOREIGN KEY` / `DROP INDEX ... ON`. No
//! cualifica por esquema (la base de datos la selecciona la conexión).

use efrust_core::diff::Operation;
use efrust_core::model::{Column, ForeignKey, Index, ReferentialAction, Table};

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

pub struct MySqlGenerator;

impl MySqlGenerator {
    pub fn new() -> Self {
        Self
    }

    fn column_def(&self, column: &Column) -> Result<String, ProviderError> {
        let mut parts = vec![self.quote_ident(&column.name), self.store_type_for(column)?];
        // Columna generada (STORED/VIRTUAL). Excluye AUTO_INCREMENT y DEFAULT.
        if let Some(expr) = &column.computed_sql {
            let kind = if column.computed_stored { "STORED" } else { "VIRTUAL" };
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
            // MySQL ignora el nombre de la PK (siempre es PRIMARY).
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
        let pcols: Vec<String> = fk.principal_columns.iter().map(|c| self.quote_ident(c)).collect();
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){};",
            self.quote_ident(table),
            self.quote_ident(&fk.name),
            cols.join(", "),
            self.quote_ident(&fk.principal_table),
            pcols.join(", "),
            on_delete_clause(fk.on_delete),
        )
    }

    fn create_index(&self, table: &str, index: &Index) -> String {
        // FULLTEXT/SPATIAL tienen prioridad sobre UNIQUE (MySQL no combina).
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
            "System.DateTimeOffset" => "datetime(6)", // MySQL no tiene tipo con offset
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
            Operation::AddForeignKey { table, foreign_key, .. } => {
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
            Operation::RawSql { sql } => vec![sql.clone()],
            Operation::EnsureExtension { .. }
            | Operation::CreateFunction { .. }
            | Operation::DropFunction { .. }
            | Operation::CreateTrigger { .. }
            | Operation::DropTrigger { .. } => self.skip_db_object(op),
        };
        Ok(sql)
    }
}
