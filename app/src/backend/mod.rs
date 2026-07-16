//! The data-access trait seam (specs/data-access-backends.md).
//!
//! Server functions and the hosted JSON routes program against the per-domain
//! [`CatalogStore`] / [`CollectionStore`] traits, never against sqlx or HTTP
//! directly. Two structs implement every trait — one per deployment target:
//!
//! - [`HostedBackend`] (feature `hosted`): in-process sqlx against Neon. It is
//!   the authorization terminus — it holds the `DATABASE_URL` pool and runs
//!   every session-scoped query inside a per-request transaction that
//!   `SET LOCAL app.user_id`, so data-model's RLS policies apply as a backstop.
//! - [`NativeBackend`] (feature `native`): an HTTPS client of the hosted JSON
//!   routes, forwarding the caller's Better Auth JWT as `Authorization: Bearer`.
//!   The Tauri binary contains no sqlx path at all.
//!
//! **Exactly one backend feature must be enabled** alongside `ssr` — enforced by
//! the compile_error below. Callers select the configured backend through the
//! per-request constructors on each struct; the choice is a compile-time cfg,
//! not a runtime branch, so the wrong backend can never be linked.
//!
//! This is the seam-proving slice: `card_count` (anonymous catalog probe) and
//! `list_collections` (session-scoped, exercises the GUC transaction).
//! collection-api extends these traits with the full method surface.

use shared::{ApiResult, CatalogCount, CollectionSummary};

#[cfg(feature = "hosted")]
pub mod hosted;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "hosted")]
pub mod routes;

#[cfg(feature = "hosted")]
pub use hosted::HostedBackend;
#[cfg(feature = "native")]
pub use native::NativeBackend;

// A server build needs a concrete backend. `ssr` alone is the substrate (router
// + auth core); without `hosted` or `native` there is nothing to answer a data
// query, so fail loud at compile time rather than link a backend-less server.
#[cfg(all(feature = "ssr", not(any(feature = "hosted", feature = "native"))))]
compile_error!(
    "enable exactly one data-access backend alongside `ssr`: \
     `hosted` (web server, sqlx) or `native` (Tauri shell, HTTPS client). \
     See specs/data-access-backends.md."
);

/// The hosted route paths — the single source of truth the hosted router
/// (`routes.rs`) mounts and the native client calls, so the two cannot drift on
/// the URL. Operation-named / RPC-ish per specs/collection-api.md.
#[cfg(feature = "ssr")]
pub mod paths {
    pub const CATALOG_COUNT: &str = "/api/catalog/count";
    pub const COLLECTIONS: &str = "/api/collections";
}

/// Catalog reads — anonymous-safe (the public IA routes). No session credential;
/// the backend struct is constructed without one.
#[cfg(feature = "ssr")]
#[allow(async_fn_in_trait)] // internal trait, always awaited on a concrete type
pub trait CatalogStore {
    /// Number of distinct oracle cards in the catalog (0 until ingestion runs).
    async fn card_count(&self) -> ApiResult<CatalogCount>;
}

/// Collection reads/writes — session-scoped. The backend carries the caller's
/// identity (hosted: the verified `user_id`; native: the forwarded JWT), so
/// these methods take no credential argument. A backend built without a session
/// answers with [`shared::ApiError::Unauthorized`].
#[cfg(feature = "ssr")]
#[allow(async_fn_in_trait)]
pub trait CollectionStore {
    /// The caller's collections, flat (the client rebuilds the tree from
    /// `parent_id`). Runs inside the `SET LOCAL app.user_id` transaction on the
    /// hosted side.
    async fn list_collections(&self) -> ApiResult<Vec<CollectionSummary>>;
}
