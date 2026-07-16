//! The hosted JSON API — the routes the [`NativeBackend`](super::native) calls
//! (specs/collection-api.md: "the native impl is the HTTP client of those same
//! routes"). Hosted-only: these run in the web deployment, the authorization
//! terminus. The web UI does NOT go through them — its Leptos server functions
//! call [`HostedBackend`] in-process (data-access-backends' "one terminus" rule).
//!
//! Each route is one trait method projected to HTTP, returning the shared DTO on
//! success and the shared error envelope (`{ "error": { code, message } }`) with
//! the mapped status on failure — the exact shape the native client decodes.

use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use http::StatusCode;
use shared::ApiResult;

use super::paths;
use super::{CatalogStore, CollectionStore, HostedBackend};

/// Add the hosted data-access routes to `router`. Generic over the router state
/// so it composes with the Leptos-options-stated app router.
pub fn mount<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route(paths::CATALOG_COUNT, get(catalog_count))
        .route(paths::COLLECTIONS, get(list_collections))
}

/// `GET /api/catalog/count` — anonymous catalog size.
async fn catalog_count() -> Response {
    let result = async { HostedBackend::anonymous().await?.card_count().await }.await;
    json_result(result)
}

/// `GET /api/collections` — the caller's collections. Requires a valid session:
/// the [`AuthUser`](crate::auth::AuthUser) extractor 401s a missing/invalid JWT
/// before this runs, and its verified `user_id` scopes the RLS transaction.
async fn list_collections(user: crate::auth::AuthUser) -> Response {
    let result = async {
        HostedBackend::for_user(user.user_id)
            .await?
            .list_collections()
            .await
    }
    .await;
    json_result(result)
}

/// Project an `ApiResult` onto an HTTP response: the DTO as JSON on success, the
/// shared error envelope with the mapped status on failure.
fn json_result<T: serde::Serialize>(result: ApiResult<T>) -> Response {
    match result {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => {
            let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::BAD_GATEWAY);
            (status, Json(err.to_wire())).into_response()
        }
    }
}
