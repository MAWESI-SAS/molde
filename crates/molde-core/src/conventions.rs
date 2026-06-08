//! Conventional, generated names for primary keys, foreign keys and indexes.
//!
//! Names are **all lowercase**, EF-style in shape: `pk_<table>`,
//! `fk_<table>_<principal>`, `ix_<table>_<columns>`. They are the single source
//! of the naming convention, shared by:
//! - the language parser (fills a name in when omitted) and emitter (hides a name
//!   when it matches one of these, so canonical `.model` files stay free of
//!   boilerplate names), and
//! - the scaffolder, which synthesizes a name when the database does not store
//!   one (e.g. SQLite primary/foreign keys, MySQL's always-`PRIMARY` key).
//!
//! Keeping all of them here guarantees authored and introspected models converge
//! on the same names, so a round-trip stays clean.

/// `pk_<table>` (lowercase).
pub fn pk_name(table: &str) -> String {
    format!("pk_{table}").to_lowercase()
}

/// `fk_<table>_<principal>` (lowercase).
pub fn fk_name(table: &str, principal: &str) -> String {
    format!("fk_{table}_{principal}").to_lowercase()
}

/// `ix_<table>_<col1>_<col2>…` (lowercase). Word boundaries are not split; the
/// name is the pattern lowercased (e.g. `CustomerId` → `customerid`).
pub fn index_name(table: &str, columns: &[String]) -> String {
    format!("ix_{}_{}", table, columns.join("_")).to_lowercase()
}
