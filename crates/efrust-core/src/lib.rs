//! # efrust-core
//!
//! Núcleo agnóstico de proveedor de efrust:
//! - [`model`]: la Representación Intermedia (Model IR) del modelo relacional.
//! - [`snapshot`]: carga/guardado del snapshot (estado del último migrado).
//! - [`diff`]: cálculo de operaciones de migración entre dos modelos.
//!
//! No depende de ningún motor de BD ni del runtime .NET. Tanto el sidecar como
//! el scaffolder producen un [`model::DatabaseModel`]; el resto del sistema
//! opera sobre él.

pub mod diff;
pub mod migration;
pub mod model;
pub mod snapshot;

pub use diff::{apply_operation, diff, Operation};
pub use migration::{rebuild_model, Migration, MIGRATION_FORMAT_VERSION};
pub use model::{
    Column, DatabaseModel, DbFunction, ForeignKey, Index, PrimaryKey, ReferentialAction, Table,
    Trigger, TriggerEvent, TriggerTiming, IR_FORMAT_VERSION,
};
