//! # molde-design
//!
//! Autoría de migraciones model-first:
//! - [`author`]: compara el modelo (parseado de los `.model`) contra el snapshot,
//!   calcula el diff y escribe la migración + el snapshot actualizado.

pub mod author;

pub use author::{add, remove, AddOutcome, AuthorError};
