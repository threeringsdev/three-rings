//! Catalog search — the SQL half of the query engine (specs/catalog-search.md).
//!
//! The grammar itself moved to [`shared::search`] with the filter-rail task —
//! the rail edits the same query text in the browser, so the parser had to
//! become wasm-safe (the move this module's doc comment predicted). What
//! stayed here is [`sql`], which emits the WHERE clause onto the hosted
//! backend's QueryBuilder and is the only half that needs sqlx.

pub mod sql;
