//! # efrust-lang — EFM, el lenguaje de modelos de efrust
//!
//! Parser y emitter entre el texto `.model` (estilo TOON/YAML indentado, una
//! entidad por archivo) y el IR [`efrust_core::model::DatabaseModel`].
//!
//! - [`parse_project`] / [`emit_project`]: proyecto completo (varios archivos).
//! - [`parse_entity`] / [`emit_entity`]: una tabla.
//! - [`parse_database`] / [`emit_database`]: el archivo global `database.model`.
//!
//! Garantía de round-trip: `parse_project(emit_project(ir)) == ir` para todo IR
//! normalizado. El azúcar de escritura (`owns`, `subtypes`, `enum[…]`) se expande
//! al parsear; el emitter produce siempre la forma canónica (plana).
//!
//! Especificación: `docs/efm-language-spec.md`.

mod emit;
mod error;
mod fmt;
mod outline;
mod parse;
mod tree;
mod types;
mod value;

pub use emit::{emit_database, emit_entity, emit_project};
pub use error::{EfmError, Result};
pub use fmt::{format_model, DATABASE_FILE};
pub use outline::{outline, OutlineItem};
pub use parse::{parse_database, parse_entity, parse_project, DbGlobals};

/// Un archivo del proyecto de modelos: nombre relativo + contenido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub name: String,
    pub contents: String,
}
