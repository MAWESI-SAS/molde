//! Provider de SQL Server (generación de T-SQL).
//!
//! Genera DDL en dialecto T-SQL: citado con `[ ]`, `IDENTITY(1,1)`, tipos
//! `nvarchar`/`datetime2`/`uniqueidentifier`, `ALTER TABLE ... ADD` (sin COLUMN),
//! `DROP INDEX ... ON`.
//!
//! Nota: la **aplicación** a SQL Server requiere un driver TDS (p. ej. `tiberius`),
//! fuera del driver `Any` de sqlx; aún no está cableada en el CLI. La generación
//! de T-SQL sí está disponible y testeada.

use efrust_core::diff::Operation;
use efrust_core::model::{Column, ForeignKey, Index, ReferentialAction, Table};

use crate::generator::{ProviderError, SqlGenerator};

fn on_delete_clause(action: ReferentialAction) -> &'static str {
    match action {
        ReferentialAction::Cascade => " ON DELETE CASCADE",
        ReferentialAction::SetNull => " ON DELETE SET NULL",
        ReferentialAction::SetDefault => " ON DELETE SET DEFAULT",
        // SQL Server no tiene RESTRICT; NO ACTION es el equivalente.
        ReferentialAction::Restrict | ReferentialAction::NoAction => "",
    }
}

pub struct SqlServerGenerator;

impl SqlServerGenerator {
    pub fn new() -> Self {
        Self
    }

    fn qualified(&self, schema: Option<&str>, name: &str) -> String {
        match schema {
            Some(s) => format!("{}.{}", self.quote_ident(s), self.quote_ident(name)),
            None => self.quote_ident(name),
        }
    }

    fn column_def(&self, column: &Column) -> Result<String, ProviderError> {
        let mut parts = vec![self.quote_ident(&column.name)];
        // Columna computada: `[col] AS (expr) [PERSISTED [NOT NULL]]`. No lleva
        // tipo, IDENTITY ni DEFAULT. La expresión ya viene parentizada por
        // sys.computed_columns.definition.
        if let Some(expr) = &column.computed_sql {
            parts.push(format!("AS {expr}"));
            if column.computed_stored {
                parts.push("PERSISTED".into());
                if !column.is_nullable {
                    parts.push("NOT NULL".into());
                }
            }
            return Ok(parts.join(" "));
        }
        if column.is_identity {
            parts.push("int NOT NULL IDENTITY(1,1)".into());
        } else {
            parts.push(self.store_type_for(column)?);
            if !column.is_nullable {
                parts.push("NOT NULL".into());
            } else {
                parts.push("NULL".into());
            }
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
            let cols: Vec<String> = pk.columns.iter().map(|c| self.quote_ident(c)).collect();
            lines.push(format!(
                "    CONSTRAINT {} PRIMARY KEY ({})",
                self.quote_ident(&pk.name),
                cols.join(", ")
            ));
        }
        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            self.qualified(table.schema.as_deref(), &table.name),
            lines.join(",\n")
        ))
    }

    fn add_foreign_key(&self, schema: Option<&str>, table: &str, fk: &ForeignKey) -> String {
        let cols: Vec<String> = fk.columns.iter().map(|c| self.quote_ident(c)).collect();
        let pcols: Vec<String> = fk.principal_columns.iter().map(|c| self.quote_ident(c)).collect();
        let principal = self.qualified(fk.principal_schema.as_deref().or(schema), &fk.principal_table);
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){};",
            self.qualified(schema, table),
            self.quote_ident(&fk.name),
            cols.join(", "),
            principal,
            pcols.join(", "),
            on_delete_clause(fk.on_delete),
        )
    }

    fn create_index(&self, schema: Option<&str>, table: &str, index: &Index) -> String {
        let unique = if index.is_unique { "UNIQUE " } else { "" };
        let cols: Vec<String> = index.columns.iter().map(|c| self.quote_ident(c)).collect();
        let filter = index.filter.as_ref().map(|f| format!(" WHERE {f}")).unwrap_or_default();
        format!(
            "CREATE {unique}INDEX {} ON {} ({}){filter};",
            self.quote_ident(&index.name),
            self.qualified(schema, table),
            cols.join(", "),
        )
    }
}

impl Default for SqlServerGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlGenerator for SqlServerGenerator {
    fn name(&self) -> &'static str {
        "sqlserver"
    }

    fn quote_ident(&self, ident: &str) -> String {
        format!("[{}]", ident.replace(']', "]]"))
    }

    fn create_history_table_sql(&self) -> String {
        // SQL Server no soporta CREATE TABLE IF NOT EXISTS.
        "IF OBJECT_ID(N'__EFMigrationsHistory') IS NULL \
         CREATE TABLE [__EFMigrationsHistory] (\
         [MigrationId] varchar(150) NOT NULL \
         CONSTRAINT [PK___EFMigrationsHistory] PRIMARY KEY, \
         [ProductVersion] varchar(32) NOT NULL);"
            .to_string()
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
            "System.Boolean" => "bit",
            "System.Single" => "real",
            "System.Double" => "float",
            "System.Decimal" => match (column.precision, column.scale) {
                (Some(p), Some(s)) => return Ok(format!("decimal({p},{s})")),
                _ => "decimal(18,2)",
            },
            "System.DateTime" => "datetime2",
            "System.DateTimeOffset" => "datetimeoffset",
            "System.Guid" => "uniqueidentifier",
            "System.Byte[]" => "varbinary(max)",
            "System.String" => {
                return Ok(match column.max_length {
                    Some(n) if n > 0 => format!("nvarchar({n})"),
                    _ => "nvarchar(max)".to_string(),
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
            Operation::DropTable { schema, name } => {
                vec![format!("DROP TABLE {};", self.qualified(schema.as_deref(), name))]
            }
            Operation::AddColumn { schema, table, column } => vec![format!(
                "ALTER TABLE {} ADD {};",
                self.qualified(schema.as_deref(), table),
                self.column_def(column)?
            )],
            Operation::DropColumn { schema, table, name } => vec![format!(
                "ALTER TABLE {} DROP COLUMN {};",
                self.qualified(schema.as_deref(), table),
                self.quote_ident(name)
            )],
            Operation::AlterColumn { schema, table, new, .. } => vec![format!(
                "ALTER TABLE {} ALTER COLUMN {};",
                self.qualified(schema.as_deref(), table),
                self.column_def(new)?
            )],
            Operation::AddForeignKey { schema, table, foreign_key } => {
                vec![self.add_foreign_key(schema.as_deref(), table, foreign_key)]
            }
            Operation::DropForeignKey { schema, table, name } => vec![format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                self.qualified(schema.as_deref(), table),
                self.quote_ident(name)
            )],
            Operation::CreateIndex { schema, table, index } => {
                vec![self.create_index(schema.as_deref(), table, index)]
            }
            Operation::DropIndex { schema, table, name } => vec![format!(
                "DROP INDEX {} ON {};",
                self.quote_ident(name),
                self.qualified(schema.as_deref(), table)
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
