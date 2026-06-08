//! # molde-lang — molde, the molde model language
//!
//! Parser and emitter between `.model` text (indented TOON/YAML style, one
//! entity per file) and the IR [`molde_core::model::DatabaseModel`].
//!
//! - [`parse_project`] / [`emit_project`]: full project (multiple files).
//! - [`parse_entity`] / [`emit_entity`]: a single table.
//! - [`parse_database`] / [`emit_database`]: the global `database.model` file.
//!
//! Round-trip guarantee: `parse_project(emit_project(ir)) == ir` for every
//! normalized IR. The authoring sugar (`owns`, `subtypes`, `enum[…]`) is expanded
//! when parsing; the emitter always produces the canonical (flat) form.
//!
//! Specification: `docs/molde-language-spec.md`.

mod emit;
mod error;
mod fk_index;
mod fmt;
mod outline;
mod parse;
mod tree;
mod types;
mod value;

pub use emit::{emit_database, emit_entity, emit_project};
pub use error::{MoldeError, Result};
pub use fmt::{format_model, DATABASE_FILE};
pub use outline::{outline, OutlineItem};
pub use parse::{parse_database, parse_entity, parse_project, DbGlobals};

/// A file of the models project: relative name + contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub name: String,
    pub contents: String,
}
