//! # molde-sync — additive live database synchronization
//!
//! Brings into a *target* database the structural changes a *source* database has
//! and it lacks, **strictly additively**: it never drops or alters anything that
//! already exists in the target, so the developer's in-progress local work is
//! preserved. Objects present in both but defined differently are reported as
//! **conflicts** and never applied.
//!
//! It compares the **live structure** of both databases (read from the catalog),
//! not migration files, so it works regardless of `git pull` state or migration
//! order. The result is a portable, atomic, idempotent SQL script that can be
//! applied to the target or carried elsewhere (e.g. production).
//!
//! Engine support is pluggable via [`SyncEngine`]; PostgreSQL is implemented
//! today. Use [`engine_for`] to pick the engine from a connection string.

mod engine;
mod postgres;
mod schema;

pub use engine::{engine_for, SyncEngine};
pub use postgres::PostgresEngine;
pub use schema::{
    diff, ColumnInfo, Conflict, ConstraintInfo, DbSchema, DiffResult, IndexInfo, RoutineInfo,
    TableInfo, TriggerInfo, ViewInfo,
};
