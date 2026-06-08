//! # molde-core
//!
//! Provider-agnostic core of molde:
//! - [`model`]: the Intermediate Representation (Model IR) of the relational model.
//! - [`snapshot`]: loading/saving the snapshot (state of the last migration).
//! - [`diff`]: computing migration operations between two models.
//!
//! It does not depend on any DB engine or the .NET runtime. Both the sidecar and
//! the scaffolder produce a [`model::DatabaseModel`]; the rest of the system
//! operates on it.

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
