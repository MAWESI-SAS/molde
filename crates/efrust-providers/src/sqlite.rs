//! Provider de SQLite.
//!
//! SQLite tiene un `ALTER TABLE` muy limitado: no soporta alterar el tipo de una
//! columna in-place (EF lo resuelve con el patrón "rebuild": crear tabla nueva,
//! copiar datos, renombrar). En Fase 0 emitimos lo soportado directamente y
//! marcamos `AlterColumn` como no soportado todavía.

use efrust_core::diff::Operation;
use efrust_core::model::{Column, Index, ReferentialAction, Table};

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

pub struct SqliteGenerator;

impl SqliteGenerator {
    pub fn new() -> Self {
        Self
    }

    fn column_def(&self, column: &Column) -> Result<String, ProviderError> {
        let mut parts = vec![self.quote_ident(&column.name), self.store_type_for(column)?];
        if !column.is_nullable {
            parts.push("NOT NULL".into());
        }
        if let Some(def) = &column.default_value_sql {
            parts.push(format!("DEFAULT {def}"));
        }
        Ok(parts.join(" "))
    }

    fn create_table(&self, table: &Table) -> Result<String, ProviderError> {
        let mut lines: Vec<String> = Vec::new();

        // En SQLite, una PK simple de tipo INTEGER suele declararse inline para
        // obtener el comportamiento de autoincremento (alias de ROWID).
        let single_int_pk = table
            .primary_key
            .as_ref()
            .filter(|pk| pk.columns.len() == 1)
            .map(|pk| pk.columns[0].clone());

        for c in &table.columns {
            if single_int_pk.as_deref() == Some(c.name.as_str()) {
                lines.push(format!(
                    "    {} INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT",
                    self.quote_ident(&c.name)
                ));
            } else {
                lines.push(format!("    {}", self.column_def(c)?));
            }
        }

        // PK compuesta → constraint a nivel de tabla.
        if let Some(pk) = &table.primary_key {
            if pk.columns.len() > 1 {
                let cols: Vec<String> = pk.columns.iter().map(|c| self.quote_ident(c)).collect();
                lines.push(format!("    PRIMARY KEY ({})", cols.join(", ")));
            }
        }

        // FKs inline: SQLite no soporta ALTER ADD FK, pero sí permite declararlas
        // dentro de CREATE TABLE (incluso con referencias a tablas aún no creadas).
        for fk in &table.foreign_keys {
            let cols: Vec<String> = fk.columns.iter().map(|c| self.quote_ident(c)).collect();
            let pcols: Vec<String> = fk
                .principal_columns
                .iter()
                .map(|c| self.quote_ident(c))
                .collect();
            lines.push(format!(
                "    CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){}",
                self.quote_ident(&fk.name),
                cols.join(", "),
                self.quote_ident(&fk.principal_table),
                pcols.join(", "),
                on_delete_clause(fk.on_delete),
            ));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            self.quote_ident(&table.name),
            lines.join(",\n")
        ))
    }
}

impl Default for SqliteGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlGenerator for SqliteGenerator {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn store_type_for(&self, column: &Column) -> Result<String, ProviderError> {
        if let Some(st) = &column.store_type {
            return Ok(st.clone());
        }
        let clr = column.clr_type.as_deref().unwrap_or("");
        // SQLite usa afinidades de tipo; mapeo conservador alineado con EF.
        let mapped = match clr {
            "System.Int16" | "System.Int32" | "System.Int64" | "System.Boolean" => "INTEGER",
            "System.Single" | "System.Double" => "REAL",
            "System.Decimal" => "TEXT", // EF guarda decimal como TEXT en SQLite
            "System.DateTime" | "System.DateTimeOffset" | "System.Guid" | "System.String" => "TEXT",
            "System.Byte[]" => "BLOB",
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
            Operation::AlterColumn { table, new, .. } => {
                // SQLite no soporta ALTER COLUMN; el cambio de tipo se materializa
                // mediante la `RebuildTable` que `diff()` emite para esta tabla.
                tracing::debug!(
                    "SQLite: ALTER COLUMN '{}.{}' se aplica vía RebuildTable",
                    table,
                    new.name
                );
                Vec::new()
            }
            Operation::CreateIndex { table, index, .. } => vec![self.create_index(table, index)],
            Operation::DropIndex { name, .. } => {
                vec![format!("DROP INDEX {};", self.quote_ident(name))]
            }
            // SQLite no permite añadir/quitar FKs con ALTER TABLE (requiere
            // reconstruir la tabla). Se omiten con aviso: las tablas e índices
            // se crean igual; la integridad referencial no se aplica.
            Operation::AddForeignKey {
                table, foreign_key, ..
            } => {
                tracing::warn!(
                    "SQLite no soporta ALTER ADD FOREIGN KEY; se omite '{}' en '{}'",
                    foreign_key.name,
                    table
                );
                Vec::new()
            }
            Operation::DropForeignKey { table, name, .. } => {
                tracing::warn!(
                    "SQLite no soporta ALTER DROP FOREIGN KEY; se omite '{}' en '{}'",
                    name,
                    table
                );
                Vec::new()
            }
            Operation::RebuildTable {
                table,
                copy_columns,
            } => self.rebuild_table(table, copy_columns)?,
            Operation::RawSql { sql } => vec![sql.clone()],
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

impl SqliteGenerator {
    fn create_index(&self, table: &str, index: &Index) -> String {
        let unique = if index.is_unique { "UNIQUE " } else { "" };
        let cols: Vec<String> = index.columns.iter().map(|c| self.quote_ident(c)).collect();
        let filter = index
            .filter
            .as_ref()
            .map(|f| format!(" WHERE {f}"))
            .unwrap_or_default();
        format!(
            "CREATE {unique}INDEX {} ON {} ({}){filter};",
            self.quote_ident(&index.name),
            self.quote_ident(table),
            cols.join(", "),
        )
    }

    /// Reconstrucción de tabla estilo EF: crea una tabla temporal con el esquema
    /// final (FKs inline), copia los datos de las columnas comunes, elimina la
    /// original, renombra la temporal y recrea los índices. Es la vía de SQLite
    /// para `ALTER COLUMN` y alta/baja de FK sobre tablas existentes.
    fn rebuild_table(
        &self,
        table: &Table,
        copy_columns: &[String],
    ) -> Result<Vec<String>, ProviderError> {
        let orig = &table.name;
        let tmp_name = format!("{orig}_efrust_new");
        let mut tmp_table = table.clone();
        tmp_table.name = tmp_name.clone();

        let mut out = Vec::new();
        out.push(self.create_table(&tmp_table)?);
        if !copy_columns.is_empty() {
            let cols = copy_columns
                .iter()
                .map(|c| self.quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "INSERT INTO {} ({cols}) SELECT {cols} FROM {};",
                self.quote_ident(&tmp_name),
                self.quote_ident(orig),
            ));
        }
        out.push(format!("DROP TABLE {};", self.quote_ident(orig)));
        out.push(format!(
            "ALTER TABLE {} RENAME TO {};",
            self.quote_ident(&tmp_name),
            self.quote_ident(orig),
        ));
        // Los índices cayeron con la tabla original; se recrean sobre la nueva.
        for idx in &table.indexes {
            out.push(self.create_index(orig, idx));
        }
        Ok(out)
    }
}
