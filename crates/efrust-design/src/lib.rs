//! # efrust-design
//!
//! Orquestación model-first (lo que hace `dotnet ef migrations add`):
//! - [`sidecar`]: invoca el sidecar .NET para obtener el modelo del DbContext.
//! - [`author`]: compara contra el snapshot, calcula el diff y escribe la
//!   migración + el snapshot actualizado.

pub mod author;
pub mod sidecar;

pub use author::{add, remove, AddOutcome, AuthorError};
pub use sidecar::{fetch_model, SidecarError, SidecarOptions};
