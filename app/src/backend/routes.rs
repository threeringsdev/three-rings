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
    AddHave, AddLine, AddWant, ApiResult, BatchMove, Id, MoveRequest, NewCollection, NewTag, Page,
    Rename, RenameTag, Reorder, Reparent, SearchQuery, SetBoard, SetQuantity, TagAssignment,
    Teardown,
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
        .route(paths::CATALOG_SEARCH, get(search))
        .route(paths::CARD_DETAIL_ROUTE, get(card_detail))
        .route(paths::CARD_SUMMARY_ROUTE, get(card_summary))
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
        .route(&paths::collection_op_route(op::TEARDOWN), post(teardown))
        .route(paths::HOLDING_QUANTITY_ROUTE, post(set_holding_quantity))
        .route(paths::MOVES, post(move_cards))
        .route(paths::MOVES_BATCH, post(move_batch))
        .route(paths::MOVES_UNDO_LAST, post(undo_last_move))
        .route(paths::MOVE_UNDO_ROUTE, post(undo_move))
        .route(paths::CARD_DESTINATIONS_ROUTE, get(suggested_destinations))
        .route(paths::ALL_CARDS, get(all_cards))
        .route(paths::SHOPPING_LIST, get(shopping_list))
        .route(&paths::collection_op_route(op::NEEDS), get(needs))
        // Tags & boards (specs/card-tagging.md).
        .route(paths::TAGS, post(create_tag))
        .route(&paths::tag_op_route(op::RENAME), post(rename_tag))
        .route(&paths::tag_op_route(op::DELETE), post(delete_tag))
        .route(paths::TAGS_ASSIGN, post(assign_tag))
        .route(paths::TAGS_UNASSIGN, post(unassign_tag))
        .route(&paths::collection_op_route(op::TAGS), get(list_tags))
        .route(
            &paths::collection_op_route(op::COMMANDERS),
            get(deck_commanders),
        )
        .route(paths::CARD_TAGS_ROUTE, get(card_tags))
        .route(paths::TAG_CARDS_ROUTE, get(cards_with_tag))
        .route(paths::HOLDING_BOARD_ROUTE, post(set_holding_board))
        .route(paths::DESIRE_BOARD_ROUTE, post(set_desire_board))
}

/// `GET /api/catalog/count` — anonymous catalog size.
async fn catalog_count() -> Response {
    json_result(async { HostedBackend::anonymous().await?.card_count().await }.await)
}

/// Catalog endpoints read the session **opportunistically**: a valid JWT (bearer
/// or cookie) yields the ownership block / owned counts, otherwise the anonymous
/// public data. An absent/invalid token simply degrades to anonymous.
async fn catalog_backend(headers: &http::HeaderMap) -> ApiResult<HostedBackend> {
    match crate::auth::user_id_from_headers(headers).await {
        Ok(user_id) => HostedBackend::for_user(user_id).await,
        Err(_) => HostedBackend::anonymous().await,
    }
}

/// `GET /api/cards/{id}` — the full card page (ownership block when authed).
async fn card_detail(headers: http::HeaderMap, Path(id): Path<Id>) -> Response {
    json_result(async { catalog_backend(&headers).await?.card_detail(id).await }.await)
}

/// `GET /api/cards/{id}/summary` — the hover subset.
async fn card_summary(headers: http::HeaderMap, Path(id): Path<Id>) -> Response {
    json_result(async { catalog_backend(&headers).await?.card_summary(id).await }.await)
}

/// Combined query params for `GET /api/catalog/search`.
#[derive(serde::Deserialize)]
struct SearchParams {
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

/// `GET /api/catalog/search?q=&cursor=&limit=` — one keyset page of results.
async fn search(headers: http::HeaderMap, Query(p): Query<SearchParams>) -> Response {
    let query = SearchQuery { q: p.q };
    let page = Page {
        cursor: p.cursor,
        limit: p.limit,
    };
    json_result(async { catalog_backend(&headers).await?.search(query, page).await }.await)
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

/// `POST /api/moves` — move copies between collections.
async fn move_cards(user: AuthUser, Json(req): Json<MoveRequest>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .move_cards(req)
                .await
        }
        .await,
    )
}

/// `POST /api/moves/batch` — many items to one destination, one transaction.
async fn move_batch(user: AuthUser, Json(req): Json<BatchMove>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .move_batch(req)
                .await
        }
        .await,
    )
}

/// `POST /api/moves/{id}/undo` — reverse a move (idempotent).
async fn undo_move(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .undo_move(id)
                .await
        }
        .await,
    )
}

/// `POST /api/moves/undo-last` — undo the caller's most recent move.
async fn undo_last_move(user: AuthUser) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .undo_last_move()
                .await
        }
        .await,
    )
}

/// `GET /api/cards/{id}/destinations` — collections wanting this oracle card.
async fn suggested_destinations(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .suggested_destinations(id)
                .await
        }
        .await,
    )
}

/// `POST /api/collections/{id}/teardown` — empty a collection.
async fn teardown(user: AuthUser, Path(id): Path<Id>, Json(mode): Json<Teardown>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .teardown(id, mode)
                .await
        }
        .await,
    )
}

/// `GET /api/all-cards?cursor=&limit=` — the virtual everything-view.
async fn all_cards(user: AuthUser, Query(page): Query<Page>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .all_cards(page)
                .await
        }
        .await,
    )
}

/// `GET /api/collections/{id}/needs` — a collection's needs.
async fn needs(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(async { HostedBackend::for_user(user.user_id).await?.needs(id).await }.await)
}

/// `GET /api/shopping-list` — the global shopping list.
async fn shopping_list(user: AuthUser) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .shopping_list()
                .await
        }
        .await,
    )
}

// --- Tags & boards (specs/card-tagging.md) ---------------------------------

/// `POST /api/tags` — create an account- or deck-scoped tag.
async fn create_tag(user: AuthUser, Json(req): Json<NewTag>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .create_tag(req)
                .await
        }
        .await,
    )
}

/// `POST /api/tags/{id}/rename`.
async fn rename_tag(user: AuthUser, Path(id): Path<Id>, Json(req): Json<RenameTag>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .rename_tag(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/tags/{id}/delete` — cascades the tag's assignments.
async fn delete_tag(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .delete_tag(id)
                .await
        }
        .await,
    )
}

/// `POST /api/tags/assign` — add a tag to a card in a collection.
async fn assign_tag(user: AuthUser, Json(req): Json<TagAssignment>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .assign_tag(req)
                .await
        }
        .await,
    )
}

/// `POST /api/tags/unassign` — remove a tag from a card in a collection.
async fn unassign_tag(user: AuthUser, Json(req): Json<TagAssignment>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .unassign_tag(req)
                .await
        }
        .await,
    )
}

/// `GET /api/collections/{id}/tags` — the tags in scope for a collection.
async fn list_tags(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .list_tags(id)
                .await
        }
        .await,
    )
}

/// `GET /api/collections/{id}/commanders` — a deck's commanders + color identity.
async fn deck_commanders(user: AuthUser, Path(id): Path<Id>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .deck_commanders(id)
                .await
        }
        .await,
    )
}

/// `GET /api/collections/{id}/cards/{oracle}/tags` — a card's tags.
async fn card_tags(user: AuthUser, Path((id, oracle)): Path<(Id, Id)>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .card_tags(id, oracle)
                .await
        }
        .await,
    )
}

/// `GET /api/collections/{id}/tags/{tag}/cards` — cards carrying a tag.
async fn cards_with_tag(user: AuthUser, Path((id, tag)): Path<(Id, Id)>) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .cards_with_tag(id, tag)
                .await
        }
        .await,
    )
}

/// `POST /api/holdings/{id}/board` — re-label a holding stack onto another board.
async fn set_holding_board(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(req): Json<SetBoard>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .set_holding_board(id, req)
                .await
        }
        .await,
    )
}

/// `POST /api/desires/{id}/board` — re-label a desire stack onto another board.
async fn set_desire_board(
    user: AuthUser,
    Path(id): Path<Id>,
    Json(req): Json<SetBoard>,
) -> Response {
    json_result(
        async {
            HostedBackend::for_user(user.user_id)
                .await?
                .set_desire_board(id, req)
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

#[cfg(test)]
mod tests {
    /// `mount` builds the router without a matchit conflict — guards the mixed
    /// static/param paths added for tags (`/api/tags/assign` alongside
    /// `/api/tags/{id}/rename`, and the nested `/api/collections/{id}/tags…`
    /// forms). matchit conflicts panic at mount time, so merely constructing the
    /// router is the assertion.
    #[test]
    fn mount_has_no_route_conflicts() {
        let _router = super::mount(axum::Router::<()>::new());
    }
}
