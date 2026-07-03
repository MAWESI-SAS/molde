//! # Model IR — Intermediate Representation of the model
//!
//! This is the **core** of molde. Both the .NET sidecar (which loads the
//! `DbContext` and resolves Fluent API + conventions) and the scaffolder
//! (DB → C#) produce this same structure. The `diff` engine operates on
//! it and the `providers` generate SQL from the resulting operations.
//!
//! It models the **relational** side of EF Core (tables/columns), which is what
//! matters for migrations, not the conceptual side (CLR entities). The name
//! of the CLR type is kept only as metadata for scaffolding and diagnostics.

use serde::{Deserialize, Serialize};

/// Version of the IR/snapshot format. It is bumped on incompatible changes
/// so that old snapshots can be migrated.
pub const IR_FORMAT_VERSION: u32 = 1;

/// Root of the relational model. It is what gets serialized as a snapshot and
/// what the sidecar emits as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseModel {
    /// Format version (see [`IR_FORMAT_VERSION`]).
    pub format_version: u32,
    /// EF Core version it was generated with (informational; goes to
    /// `__EFMigrationsHistory.ProductVersion`).
    #[serde(default)]
    pub product_version: Option<String>,
    /// Default schema (e.g. `public` in Postgres, `dbo` in SQL Server).
    #[serde(default)]
    pub default_schema: Option<String>,
    /// Tables of the model, stably ordered by (schema, name).
    pub tables: Vec<Table>,
    /// Schema-level functions (e.g. trigger functions in Postgres).
    /// Objects EF does not model; preserved as raw SQL (`definition`).
    #[serde(default)]
    pub functions: Vec<DbFunction>,
    /// Pre-rendered raw DDL to preserve verbatim (escape hatch for engine-specific
    /// objects that do not fit the IR; e.g. SQL Server full-text:
    /// `CREATE FULLTEXT CATALOG`/`CREATE FULLTEXT INDEX`).
    #[serde(default)]
    pub raw_objects: Vec<String>,
    /// Installed engine extensions (Postgres: `pg_trgm`, `unaccent`,
    /// `vector`…). Ensured with `CREATE EXTENSION IF NOT EXISTS` before the DDL.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Schema views. Preserved as the raw `SELECT` body (`View::definition`)
    /// for a faithful round-trip, like functions and triggers.
    #[serde(default)]
    pub views: Vec<View>,
}

impl DatabaseModel {
    /// Empty model with the current format version.
    pub fn empty() -> Self {
        Self {
            format_version: IR_FORMAT_VERSION,
            product_version: None,
            default_schema: None,
            tables: Vec::new(),
            functions: Vec::new(),
            raw_objects: Vec::new(),
            extensions: Vec::new(),
            views: Vec::new(),
        }
    }

    /// Looks up a table by its qualified name (schema + name).
    pub fn table(&self, schema: Option<&str>, name: &str) -> Option<&Table> {
        self.tables
            .iter()
            .find(|t| t.schema.as_deref() == schema && t.name == name)
    }

    /// Orders tables and columns deterministically. Essential so that diffs and
    /// snapshots are stable (do not produce noise from reorderings).
    pub fn normalize(&mut self) {
        self.tables.sort_by(|a, b| {
            (a.schema.as_deref(), a.name.as_str()).cmp(&(b.schema.as_deref(), b.name.as_str()))
        });
        for t in &mut self.tables {
            t.columns.sort_by(|a, b| a.name.cmp(&b.name));
            t.foreign_keys.sort_by(|a, b| a.name.cmp(&b.name));
            t.indexes.sort_by(|a, b| a.name.cmp(&b.name));
            t.triggers.sort_by(|a, b| a.name.cmp(&b.name));
            t.check_constraints.sort_by(|a, b| a.name.cmp(&b.name));
        }
        self.functions.sort_by(|a, b| {
            (a.schema.as_deref(), a.name.as_str()).cmp(&(b.schema.as_deref(), b.name.as_str()))
        });
        self.raw_objects.sort();
        self.extensions.sort();
        self.views.sort_by(|a, b| {
            (a.schema.as_deref(), a.name.as_str()).cmp(&(b.schema.as_deref(), b.name.as_str()))
        });
    }
}

/// A table of the relational model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Name of the table in the DB.
    pub name: String,
    /// Schema; `None` uses the model's default schema.
    #[serde(default)]
    pub schema: Option<String>,
    /// Name of the source CLR type (e.g. `MyApp.Models.Customer`).
    /// Only metadata for scaffolding/diagnostics.
    #[serde(default)]
    pub clr_type: Option<String>,
    /// Comment/description of the table, if any.
    #[serde(default)]
    pub comment: Option<String>,
    pub columns: Vec<Column>,
    #[serde(default)]
    pub primary_key: Option<PrimaryKey>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    #[serde(default)]
    pub indexes: Vec<Index>,
    /// Triggers associated with the table. EF does not model them; preserved as
    /// raw SQL (`Trigger::definition`) for a faithful round-trip.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// `CHECK` constraints of the table. The expression is kept as the engine
    /// reports it (e.g. `pg_get_constraintdef` without the `CHECK` keyword is
    /// NOT stripped: the full `CHECK (...)` text is stored).
    #[serde(default)]
    pub check_constraints: Vec<CheckConstraint>,
    /// Seed data declared with `HasData` (model-first). Each row maps
    /// column → JSON value. Materialized as `INSERT`/`UPDATE`/`DELETE`.
    #[serde(default)]
    pub seed_data: Vec<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Columns used to match seed rows across migrations (the natural key),
    /// instead of the primary key. Lets a DB-generated PK (e.g. a `guid` with a
    /// `gen_random_uuid()` default) be omitted from seed rows while still
    /// producing incremental `INSERT`/`UPDATE`/`DELETE`. Empty ⇒ match by PK.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seed_key: Vec<String>,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
}

/// A column of a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// Provider-specific storage type (e.g. `nvarchar(200)`,
    /// `integer`, `timestamp with time zone`). If `None`, the provider derives it
    /// from `clr_type` + facets (`max_length`, `precision`, `scale`).
    #[serde(default)]
    pub store_type: Option<String>,
    /// Source CLR type (e.g. `System.String`, `System.Int32`).
    #[serde(default)]
    pub clr_type: Option<String>,
    #[serde(default)]
    pub is_nullable: bool,
    /// Column auto-generated by identity/sequence (auto-increment PK).
    #[serde(default)]
    pub is_identity: bool,
    /// Maximum length for string/binary types.
    #[serde(default)]
    pub max_length: Option<i32>,
    #[serde(default)]
    pub precision: Option<i32>,
    #[serde(default)]
    pub scale: Option<i32>,
    /// Literal default value (raw SQL, e.g. `0`, `'N'`).
    #[serde(default)]
    pub default_value_sql: Option<String>,
    /// Computed column expression, if applicable.
    #[serde(default)]
    pub computed_sql: Option<String>,
    /// Whether the computed column is STORED (persisted) vs virtual.
    #[serde(default)]
    pub computed_stored: bool,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// Primary key of a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimaryKey {
    pub name: String,
    /// Columns that make up the PK, in order.
    pub columns: Vec<String>,
}

/// Referential delete behavior. Maps to the provider's `ON DELETE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferentialAction {
    #[default]
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

/// Foreign key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: String,
    /// Local columns that make up the FK.
    pub columns: Vec<String>,
    /// Referenced principal table.
    pub principal_table: String,
    #[serde(default)]
    pub principal_schema: Option<String>,
    /// Columns referenced in the principal table.
    pub principal_columns: Vec<String>,
    #[serde(default)]
    pub on_delete: ReferentialAction,
    /// Maps to the provider's `ON UPDATE`. `NoAction` (the default) is omitted
    /// from the DDL, like `on_delete`.
    #[serde(default)]
    pub on_update: ReferentialAction,
}

/// Index (unique or not).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub is_unique: bool,
    /// Partial filter (e.g. `[Deleted] = 0`), if the provider supports it.
    #[serde(default)]
    pub filter: Option<String>,
    /// Index access method (e.g. `gin`, `gist`, `hnsw`, `ivfflat`).
    /// `None` = engine's default method (`btree` in Postgres).
    #[serde(default)]
    pub method: Option<String>,
    /// Operator class per column (e.g. `vector_cosine_ops`, `gin_trgm_ops`).
    /// Empty = the type's default operators. Used by pgvector / GIN.
    #[serde(default)]
    pub operators: Vec<String>,
    /// Expression index (e.g. `to_tsvector('english', body)`). When present it
    /// takes precedence over `columns` for the DDL (full-text / functional indexes).
    #[serde(default)]
    pub expression: Option<String>,
}

/// Firing time of a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerTiming {
    Before,
    After,
    InsteadOf,
}

/// Event that fires a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEvent {
    Insert,
    Update,
    Delete,
    Truncate,
}

/// A table trigger. EF Core does not model it; the raw `definition`
/// (`CREATE TRIGGER ...`) is kept as the source of truth to recreate it, and the
/// structured fields serve for diagnostics and ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    /// Table it is associated with.
    pub table: String,
    #[serde(default)]
    pub schema: Option<String>,
    pub timing: TriggerTiming,
    /// Events that fire it (one or more).
    pub events: Vec<TriggerEvent>,
    /// Qualified name of the function it executes (informational).
    #[serde(default)]
    pub function: Option<String>,
    /// Raw DDL `CREATE TRIGGER ...` as returned by the engine.
    pub definition: String,
}

/// A schema function (e.g. a trigger function in Postgres). EF does not
/// model it; the raw `definition` (`CREATE FUNCTION ...`) is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbFunction {
    pub name: String,
    #[serde(default)]
    pub schema: Option<String>,
    /// Raw DDL `CREATE [OR REPLACE] FUNCTION ...` as returned by the engine.
    pub definition: String,
}

/// A `CHECK` constraint on a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckConstraint {
    pub name: String,
    /// Full constraint text as reported by the engine, including the `CHECK`
    /// keyword (e.g. `CHECK ((status)::text = ANY (...))`).
    pub expression: String,
}

/// A schema view. The `definition` is the raw `SELECT` body as reported by the
/// engine (e.g. `pg_get_viewdef`), without the `CREATE VIEW ... AS` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    #[serde(default)]
    pub schema: Option<String>,
    /// Raw `SELECT ...` body as returned by the engine.
    pub definition: String,
}
