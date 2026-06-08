//! # molde-design
//!
//! Model-first migration authoring:
//! - [`author`]: compares the model (parsed from the `.model` files) against the
//!   snapshot, computes the diff, and writes the migration + the updated snapshot.

pub mod author;

pub use author::{add, remove, AddOutcome, AuthorError};
