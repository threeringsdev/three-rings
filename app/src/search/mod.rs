//! Catalog search — the query engine (specs/catalog-search.md).
//!
//! [`parse`] is the pure v1 grammar (TDD core, dependency-free — could move
//! to `shared/` if the rail's term↔widget mapping ever needs it client-side);
//! [`sql`] emits the WHERE clause onto the hosted backend's QueryBuilder.

pub mod parse;
pub mod sql;
