//! The hosted JSON API — the routes the [`NativeBackend`](super::native) calls
//! (specs/collection-api.md: "the native impl is the HTTP client of those same
//! routes"). Hosted-only: these run in the web deployment, the authorization
//! terminus. The web UI does NOT go through them — its Leptos server functions
//! call [`HostedBackend`] in-process (data-access-backends' "one terminus" rule).
//!
//! Each route is one trait method projected to HTTP, returning the shared DTO on
//! success and the shared error envelope (`{ "error": { code, message } }`) with
//! the mapped status on failure — the exact shape the native client decodes.

use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use http::StatusCode;
use shared::{
    AddHave, AddLine, AddWant, ApiResult, Id, NewCollection, Page, Rename, Reorder, Reparent,
    SetQuantity,
};

use super::paths;
use super::paths::op;
use super::{CatalogStore, CollectionStore, HostedBackend};
use crate::auth::AuthUser;

/// Add the hosted data-access routes to `router`. Generic over the router state
/// so it composes with the Leptos-options-stated app router.
pub fn mount<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route(paths::CATALOG_COUNT, get(catalog_count))
        .route(
            paths::COLLECTIONS,
            get(list_collections).post(create_collection),
        )
        .route(
            &paths::collection_op_route(op::RENAME),
            post(rename_collection),
        )
        .route(
            &paths::collection_op_route(op::DELETE),
            post(delete_collection),
        )
        .route(
            &paths::collection_op_route(op::REPARENT),
            post(reparent_collection),
        )
        .route(
            &paths::collection_op_route(op::REORDER),
            post(reorder_collection),
        )
        .route(&paths::collection_op_route(op::HAVE), post(add_holding))
        .route(&paths::collection_op_route(op::WANT), post(add_desire))
        .route(&paths::collection_op_route(op::BATCH), post(batch_add))
        .route(&paths::collection_op_route(op::VIEW), get(collection_view))
        .route(paths::HOLDING_QUANTITY_ROUTE, post(set_holding_quantity))
}

/// `GET /api/catalog/count` — anonymous catalog size.
async fn catalog_count() -> Response {
    json_result(async { HostedBackend::anonymous().await?.card_count().await }.await)
}

/// `GET /api/collections` — the caller's collections (Inbox provisioned lazily).
/// The [`AuthUser`] extractor 401s a missing/invalid JWT before this runs, and
/// its verified `user_id` scopes the RLS transaction.
async fn list_collections(user: AuthUser) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .list_collections()
                .await
        }
        .await,
    )
}

/// `POST /api/collections` — create a binder or deck.
async fn create_collection(user: AuthUser, Json(req): Json<NewCollection>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .create_collection(req)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/rename`.
async fn rename_collection(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(req): Json<Rename>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .rename_collection(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/delete`.
async fn delete_collection(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .delete_collection(id)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/reparent`.
async fn reparent_collection(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(req): Json<Reparent>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .reparent_collection(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/reorder`.
async fn reorder_collection(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(req): Json<Reorder>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .reorder_collection(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/have` — add present copies.
async fn add_holding(user: AuthUser, Path(id): Path<Id>, Json(req): Json<AddHave>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .add_holding(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/want` — add a desired count.
async fn add_desire(user: AuthUser, Path(id): Path<Id>, Json(req): Json<AddWant>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .add_desire(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/batch` — add many lines, per-line results.
async fn batch_add(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(lines): Json<Vec<AddLine>>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .batch_add(id, lines)
                .await
        }
        .await,
    )
}

/// `POST /api/holdings/{id}/quantity` — set a holding's quantity (0 deletes).
async fn set_holding_quantity(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(req): Json<SetQuantity>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .set_holding_quantity(id, req)
                .await
        }
        .await,
    )
}

/// `GET /api/collections/{id}/view?cursor=&limit=` — one keyset page of the
/// collection's card rows plus metadata and children.
async fn collection_view(user: AuthUser, Path(id): Path<Id>, Query(page): Query<Page>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .collection_view(id, page)
                .await
        }
        .await,
    )
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
