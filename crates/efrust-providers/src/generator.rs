//! El trait [`SqlGenerator`] y utilidades compartidas.
//!
//! Cada motor implementa este trait. El núcleo (`efrust-core`) produce
//! [`Operation`]s agnósticas; el provider las traduce al dialecto SQL concreto,
//! incluyendo el mapeo de tipo CLR → tipo de almacenamiento.

use efrust_core::diff::Operation;
use efrust_core::model::Column;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("operación aún no soportada por el provider '{provider}': {detail}")]
    Unsupported { provider: &'static str, detail: String },
    #[error("no se pudo mapear el tipo CLR '{clr}' (columna '{column}')")]
    UnmappedType { clr: String, column: String },
}

/// Genera SQL para un motor concreto a partir de operaciones de migración.
pub trait SqlGenerator {
    /// Nombre del proveedor (para diagnósticos y `ProductVersion`).
    fn name(&self) -> &'static str;

    /// Carácter(es) de citado de identificadores. SQLite/Postgres usan `"`,
    /// SQL Server `[ ]`, MySQL backticks.
    fn quote_ident(&self, ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }

    /// Mapea una columna a su tipo de almacenamiento. Si la columna ya trae
    /// `store_type` explícito se respeta; si no, el provider lo deriva del
    /// tipo CLR + facetas.
    fn store_type_for(&self, column: &Column) -> Result<String, ProviderError>;

    /// SQL para crear la tabla de historial `__EFMigrationsHistory` si no existe.
    /// El default sirve a Postgres/SQLite/MySQL (`CREATE TABLE IF NOT EXISTS`);
    /// SQL Server lo sobreescribe (no soporta esa sintaxis).
    fn create_history_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {t} ({id} varchar(150) NOT NULL, \
             {ver} varchar(32) NOT NULL, PRIMARY KEY ({id}));",
            t = self.quote_ident("__EFMigrationsHistory"),
            id = self.quote_ident("MigrationId"),
            ver = self.quote_ident("ProductVersion"),
        )
    }

    /// Traduce una única operación a una o más sentencias SQL.
    fn emit(&self, op: &Operation) -> Result<Vec<String>, ProviderError>;

    /// Traduce una lista de operaciones (atajo conveniente).
    fn emit_all(&self, ops: &[Operation]) -> Result<Vec<String>, ProviderError> {
        let mut out = Vec::new();
        for op in ops {
            out.extend(self.emit(op)?);
        }
        Ok(out)
    }
}
