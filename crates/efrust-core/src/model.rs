//! # Model IR — Representación Intermedia del modelo
//!
//! Este es el **núcleo** de efrust. Tanto el sidecar .NET (que carga el
//! `DbContext` y resuelve Fluent API + convenciones) como el scaffolder
//! (BD → C#) producen esta misma estructura. El motor de `diff` opera sobre
//! ella y los `providers` generan SQL a partir de las operaciones resultantes.
//!
//! Modela el lado **relacional** de EF Core (tablas/columnas), que es lo que
//! importa para migraciones, no el lado conceptual (entidades CLR). El nombre
//! del tipo CLR se conserva solo como metadato para scaffolding y diagnóstico.

use serde::{Deserialize, Serialize};

/// Versión del formato del IR/snapshot. Se incrementa ante cambios
/// incompatibles para poder migrar snapshots antiguos.
pub const IR_FORMAT_VERSION: u32 = 1;

/// Raíz del modelo relacional. Es lo que se serializa como snapshot y lo que
/// el sidecar emite como JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseModel {
    /// Versión del formato (ver [`IR_FORMAT_VERSION`]).
    pub format_version: u32,
    /// Versión de EF Core con la que se generó (informativo; va a
    /// `__EFMigrationsHistory.ProductVersion`).
    #[serde(default)]
    pub product_version: Option<String>,
    /// Esquema por defecto (p. ej. `public` en Postgres, `dbo` en SQL Server).
    #[serde(default)]
    pub default_schema: Option<String>,
    /// Tablas del modelo, ordenadas de forma estable por (schema, name).
    pub tables: Vec<Table>,
    /// Funciones a nivel de esquema (p. ej. funciones de trigger en Postgres).
    /// Objetos que EF no modela; se preservan como SQL crudo (`definition`).
    #[serde(default)]
    pub functions: Vec<DbFunction>,
}

impl DatabaseModel {
    /// Modelo vacío con la versión de formato actual.
    pub fn empty() -> Self {
        Self {
            format_version: IR_FORMAT_VERSION,
            product_version: None,
            default_schema: None,
            tables: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Busca una tabla por su nombre cualificado (schema + name).
    pub fn table(&self, schema: Option<&str>, name: &str) -> Option<&Table> {
        self.tables
            .iter()
            .find(|t| t.schema.as_deref() == schema && t.name == name)
    }

    /// Ordena tablas y columnas de forma determinista. Imprescindible para que
    /// los diffs y snapshots sean estables (no produzcan ruido por reordenamientos).
    pub fn normalize(&mut self) {
        self.tables.sort_by(|a, b| {
            (a.schema.as_deref(), a.name.as_str())
                .cmp(&(b.schema.as_deref(), b.name.as_str()))
        });
        for t in &mut self.tables {
            t.columns.sort_by(|a, b| a.name.cmp(&b.name));
            t.foreign_keys.sort_by(|a, b| a.name.cmp(&b.name));
            t.indexes.sort_by(|a, b| a.name.cmp(&b.name));
            t.triggers.sort_by(|a, b| a.name.cmp(&b.name));
        }
        self.functions.sort_by(|a, b| {
            (a.schema.as_deref(), a.name.as_str()).cmp(&(b.schema.as_deref(), b.name.as_str()))
        });
    }
}

/// Una tabla del modelo relacional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Nombre de la tabla en la BD.
    pub name: String,
    /// Esquema; `None` usa el esquema por defecto del modelo.
    #[serde(default)]
    pub schema: Option<String>,
    /// Nombre del tipo CLR de origen (p. ej. `MyApp.Models.Customer`).
    /// Solo metadato para scaffolding/diagnóstico.
    #[serde(default)]
    pub clr_type: Option<String>,
    /// Comentario/descripción de la tabla, si lo hay.
    #[serde(default)]
    pub comment: Option<String>,
    pub columns: Vec<Column>,
    #[serde(default)]
    pub primary_key: Option<PrimaryKey>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    #[serde(default)]
    pub indexes: Vec<Index>,
    /// Triggers asociados a la tabla. EF no los modela; se preservan como SQL
    /// crudo (`Trigger::definition`) para round-trip fiel.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
}

/// Una columna de una tabla.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// Tipo de almacenamiento específico del proveedor (p. ej. `nvarchar(200)`,
    /// `integer`, `timestamp with time zone`). Si `None`, el provider lo deriva
    /// de `clr_type` + facetas (`max_length`, `precision`, `scale`).
    #[serde(default)]
    pub store_type: Option<String>,
    /// Tipo CLR de origen (p. ej. `System.String`, `System.Int32`).
    #[serde(default)]
    pub clr_type: Option<String>,
    #[serde(default)]
    pub is_nullable: bool,
    /// Columna autogenerada por identidad/secuencia (PK autoincremental).
    #[serde(default)]
    pub is_identity: bool,
    /// Longitud máxima para tipos string/binarios.
    #[serde(default)]
    pub max_length: Option<i32>,
    #[serde(default)]
    pub precision: Option<i32>,
    #[serde(default)]
    pub scale: Option<i32>,
    /// Valor por defecto literal (SQL crudo, p. ej. `0`, `'N'`).
    #[serde(default)]
    pub default_value_sql: Option<String>,
    /// Expresión de columna computada, si aplica.
    #[serde(default)]
    pub computed_sql: Option<String>,
    /// Si la computada es STORED (persistida) vs virtual.
    #[serde(default)]
    pub computed_stored: bool,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// Clave primaria de una tabla.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimaryKey {
    pub name: String,
    /// Columnas que componen la PK, en orden.
    pub columns: Vec<String>,
}

/// Comportamiento de borrado referencial. Se mapea al `ON DELETE` del proveedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferentialAction {
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

impl Default for ReferentialAction {
    fn default() -> Self {
        ReferentialAction::NoAction
    }
}

/// Clave foránea.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: String,
    /// Columnas locales que forman la FK.
    pub columns: Vec<String>,
    /// Tabla principal referenciada.
    pub principal_table: String,
    #[serde(default)]
    pub principal_schema: Option<String>,
    /// Columnas referenciadas en la tabla principal.
    pub principal_columns: Vec<String>,
    #[serde(default)]
    pub on_delete: ReferentialAction,
}

/// Índice (único o no).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub is_unique: bool,
    /// Filtro parcial (p. ej. `[Deleted] = 0`), si el proveedor lo soporta.
    #[serde(default)]
    pub filter: Option<String>,
    /// Método de acceso del índice (p. ej. `gin`, `gist`, `hnsw`, `ivfflat`).
    /// `None` = método por defecto del motor (`btree` en Postgres).
    #[serde(default)]
    pub method: Option<String>,
    /// Operator class por columna (p. ej. `vector_cosine_ops`, `gin_trgm_ops`).
    /// Vacío = operadores por defecto del tipo. Usado por pgvector / GIN.
    #[serde(default)]
    pub operators: Vec<String>,
    /// Índice por expresión (p. ej. `to_tsvector('english', body)`). Cuando está
    /// presente manda sobre `columns` para el DDL (full-text / índices funcionales).
    #[serde(default)]
    pub expression: Option<String>,
}

/// Momento de disparo de un trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerTiming {
    Before,
    After,
    InsteadOf,
}

/// Evento que dispara un trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEvent {
    Insert,
    Update,
    Delete,
    Truncate,
}

/// Un trigger de tabla. EF Core no lo modela; se conserva el `definition` crudo
/// (`CREATE TRIGGER ...`) como fuente de verdad para recrearlo, y los campos
/// estructurados sirven para diagnóstico y ordenación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    /// Tabla a la que está asociado.
    pub table: String,
    #[serde(default)]
    pub schema: Option<String>,
    pub timing: TriggerTiming,
    /// Eventos que lo disparan (uno o varios).
    pub events: Vec<TriggerEvent>,
    /// Nombre cualificado de la función que ejecuta (informativo).
    #[serde(default)]
    pub function: Option<String>,
    /// DDL crudo `CREATE TRIGGER ...` tal como lo devuelve el motor.
    pub definition: String,
}

/// Una función de esquema (p. ej. función de trigger en Postgres). EF no la
/// modela; se conserva el `definition` crudo (`CREATE FUNCTION ...`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbFunction {
    pub name: String,
    #[serde(default)]
    pub schema: Option<String>,
    /// DDL crudo `CREATE [OR REPLACE] FUNCTION ...` tal como lo devuelve el motor.
    pub definition: String,
}
