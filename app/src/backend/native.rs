//! Native (Tauri desktop/mobile) backend: an HTTPS client of the hosted JSON
//! routes (specs/data-access-backends.md). The Tauri binary holds no Postgres
//! credentials and contains no sqlx path; it delegates every data query to the
//! hosted deployment, which is the authorization terminus.
//!
//! Identity rides the **same** Better Auth JWT the hosted `AuthUser` extractor
//! already verifies: the embedded server forwards the caller's `tr_jwt` as
//! `Authorization: Bearer`. There is no bespoke native↔hosted token.
//!
//! This path is compiled (CI lints `-p app --features native`) and shipped in
//! the APK/`.dmg`, but never exercised in the web-dev container — it needs a
//! running hosted deployment to talk to.

use shared::{
    AddHave, AddLine, AddWant, AllCardsView, ApiError, ApiResult, BatchMove, CardDetail,
    CardSummary, CatalogCount, CollectionSummary, CollectionView, DesireLine, ErrorEnvelope,
    HoldingLine, Id, LineResult, MoveReceipt, MoveRequest, NeedsView, NewCollection, Page, Rename,
    Reorder, Reparent, SearchQuery, SearchResults, SetQuantity, ShoppingList, SuggestedDestination,
    Teardown, TeardownReceipt,
};
use tokio::sync::OnceCell;

use super::{CatalogStore, CollectionStore};

/// The hosted API origin release builds fall back to (non-secret, matches
/// auth's baked default). An exported `TR_WEB_ORIGIN` wins, so a dev build can
/// point the native client at a local/dev deployment.
const DEFAULT_WEB_ORIGIN: &str = "https://three-rings-6p5o.onrender.com";

/// Process-wide reqwest client (connection pooling / keep-alive). rustls, no
/// OpenSSL — same stack as the auth core.
static CLIENT: OnceCell<reqwest::Client> = OnceCell::const_new();

async fn client() -> &'static reqwest::Client {
    CLIENT
        .get_or_init(|| async { reqwest::Client::new() })
        .await
}

/// A per-request client of the hosted API. `token` is the caller's `tr_jwt`,
/// forwarded on session-scoped calls; `None` for anonymous catalog reads.
pub struct NativeBackend {
    base: String,
    token: Option<String>,
}

impl NativeBackend {
    /// Anonymous client — catalog reads only.
    pub fn anonymous() -> Self {
        Self {
            base: web_origin(),
            token: None,
        }
    }

    /// Session client forwarding `token` (the caller's `tr_jwt`).
    pub fn authed(token: String) -> Self {
        Self {
            base: web_origin(),
            token: Some(token),
        }
    }

    /// Send a request to a hosted JSON route, attaching the bearer token and an
    /// optional JSON body, and return the raw response on 2xx — mapping a non-2xx
    /// status onto the shared error via its wire envelope.
    async fn send<B: serde::Serialize>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> ApiResult<reqwest::Response> {
        let url = format!("{}{path}", self.base);
        let mut req = client().await.request(method, &url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ApiError::Upstream(format!("request to {path} failed: {e}")))?;

        if resp.status().is_success() {
            return Ok(resp);
        }
        // Error path: reconstruct the shared error from status + wire body.
        // TODO(collection-api native client): on 401, silently re-mint the JWT
        // from `tr_session` via auth's refresh path and retry once, per
        // data-access-backends.md. Until that cookie-jar plumbing lands with the
        // session-scoped endpoints, a 401 surfaces as Unauthorized.
        let code = resp.status().as_u16();
        let body = resp.json::<ErrorEnvelope>().await.ok().map(|e| e.error);
        Err(ApiError::from_wire(code, body))
    }

    /// GET a route and decode the JSON response into `T`.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        let resp = self.send(reqwest::Method::GET, path, None::<&()>).await?;
        decode(path, resp).await
    }

    /// POST a JSON body to a route and decode the JSON response into `T`.
    async fn post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let resp = self.send(reqwest::Method::POST, path, Some(body)).await?;
        decode(path, resp).await
    }

    /// POST a JSON body to a route that returns no content.
    async fn post_unit<B: serde::Serialize>(&self, path: &str, body: &B) -> ApiResult<()> {
        self.send(reqwest::Method::POST, path, Some(body)).await?;
        Ok(())
    }

    /// Require a session token before a session-scoped call; the hosted route
    /// would 401 anyway, but this skips a round trip.
    fn require_session(&self) -> ApiResult<()> {
        if self.token.is_none() {
            return Err(ApiError::Unauthorized("no session token".into()));
        }
        Ok(())
    }
}

/// Decode a successful response body into `T`.
async fn decode<T: serde::de::DeserializeOwned>(
    path: &str,
    resp: reqwest::Response,
) -> ApiResult<T> {
    resp.json::<T>()
        .await
        .map_err(|e| ApiError::Upstream(format!("decoding {path} failed: {e}")))
}

impl CatalogStore for NativeBackend {
    async fn card_count(&self) -> ApiResult<CatalogCount> {
        self.get(super::paths::CATALOG_COUNT).await
    }

    async fn card_detail(&self, oracle_id: Id) -> ApiResult<CardDetail> {
        self.get(&super::paths::card_detail(oracle_id)).await
    }

    async fn card_summary(&self, oracle_id: Id) -> ApiResult<CardSummary> {
        self.get(&super::paths::card_summary(oracle_id)).await
    }

    async fn search(&self, query: SearchQuery, page: Page) -> ApiResult<SearchResults> {
        let mut path = super::paths::CATALOG_SEARCH.to_string();
        let mut qs = Vec::new();
        if let Some(q) = &query.q {
            qs.push(format!("q={}", urlencode(q)));
        }
        if let Some(cursor) = &page.cursor {
            qs.push(format!("cursor={cursor}"));
        }
        if let Some(limit) = page.limit {
            qs.push(format!("limit={limit}"));
        }
        if !qs.is_empty() {
            path.push('?');
            path.push_str(&qs.join("&"));
        }
        self.get(&path).await
    }
}

/// Minimal percent-encoding for a query-string value (the search term may carry
/// spaces / punctuation). Only the native client needs it.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl CollectionStore for NativeBackend {
    async fn list_collections(&self) -> ApiResult<Vec<CollectionSummary>> {
        self.require_session()?;
        self.get(super::paths::COLLECTIONS).await
    }

    async fn create_collection(&self, req: NewCollection) -> ApiResult<CollectionSummary> {
        self.require_session()?;
        self.post(super::paths::COLLECTIONS, &req).await
    }

    async fn rename_collection(&self, id: Id, req: Rename) -> ApiResult<CollectionSummary> {
        self.require_session()?;
        self.post(
            &super::paths::collection_op(id, super::paths::op::RENAME),
            &req,
        )
        .await
    }

    async fn delete_collection(&self, id: Id) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(
            &super::paths::collection_op(id, super::paths::op::DELETE),
            &(),
        )
        .await
    }

    async fn reparent_collection(&self, id: Id, req: Reparent) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(
            &super::paths::collection_op(id, super::paths::op::REPARENT),
            &req,
        )
        .await
    }

    async fn reorder_collection(&self, id: Id, req: Reorder) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(
            &super::paths::collection_op(id, super::paths::op::REORDER),
            &req,
        )
        .await
    }

    async fn add_holding(&self, collection_id: Id, req: AddHave) -> ApiResult<HoldingLine> {
        self.require_session()?;
        self.post(
            &super::paths::collection_op(collection_id, super::paths::op::HAVE),
            &req,
        )
        .await
    }

    async fn add_desire(&self, collection_id: Id, req: AddWant) -> ApiResult<DesireLine> {
        self.require_session()?;
        self.post(
            &super::paths::collection_op(collection_id, super::paths::op::WANT),
            &req,
        )
        .await
    }

    async fn set_holding_quantity(
        &self,
        holding_id: Id,
        req: SetQuantity,
    ) -> ApiResult<Option<HoldingLine>> {
        self.require_session()?;
        self.post(&super::paths::holding_quantity(holding_id), &req)
            .await
    }

    async fn batch_add(
        &self,
        collection_id: Id,
        lines: Vec<AddLine>,
    ) -> ApiResult<Vec<LineResult>> {
        self.require_session()?;
        self.post(
            &super::paths::collection_op(collection_id, super::paths::op::BATCH),
            &lines,
        )
        .await
    }

    async fn collection_view(&self, id: Id, page: Page) -> ApiResult<CollectionView> {
        self.require_session()?;
        // A read: GET with the keyset params in the query string. The cursor is
        // base64url (already URL-safe), so no escaping is needed.
        let mut path = super::paths::collection_op(id, super::paths::op::VIEW);
        let mut qs = Vec::new();
        if let Some(cursor) = &page.cursor {
            qs.push(format!("cursor={cursor}"));
        }
        if let Some(limit) = page.limit {
            qs.push(format!("limit={limit}"));
        }
        if !qs.is_empty() {
            path.push('?');
            path.push_str(&qs.join("&"));
        }
        self.get(&path).await
    }

    async fn move_cards(&self, req: MoveRequest) -> ApiResult<MoveReceipt> {
        self.require_session()?;
        self.post(super::paths::MOVES, &req).await
    }

    async fn move_batch(&self, req: BatchMove) -> ApiResult<Vec<MoveReceipt>> {
        self.require_session()?;
        self.post(super::paths::MOVES_BATCH, &req).await
    }

    async fn undo_move(&self, move_id: Id) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(&super::paths::move_undo(move_id), &()).await
    }

    async fn undo_last_move(&self) -> ApiResult<Option<MoveReceipt>> {
        self.require_session()?;
        self.post(super::paths::MOVES_UNDO_LAST, &()).await
    }

    async fn suggested_destinations(&self, oracle_id: Id) -> ApiResult<Vec<SuggestedDestination>> {
        self.require_session()?;
        self.get(&super::paths::card_destinations(oracle_id)).await
    }

    async fn teardown(&self, collection_id: Id, mode: Teardown) -> ApiResult<TeardownReceipt> {
        self.require_session()?;
        self.post(
            &super::paths::collection_op(collection_id, super::paths::op::TEARDOWN),
            &mode,
        )
        .await
    }

    async fn all_cards(&self, page: Page) -> ApiResult<AllCardsView> {
        self.require_session()?;
        let mut path = super::paths::ALL_CARDS.to_string();
        let mut qs = Vec::new();
        if let Some(cursor) = &page.cursor {
            qs.push(format!("cursor={cursor}"));
        }
        if let Some(limit) = page.limit {
            qs.push(format!("limit={limit}"));
        }
        if !qs.is_empty() {
            path.push('?');
            path.push_str(&qs.join("&"));
        }
        self.get(&path).await
    }

    async fn needs(&self, collection_id: Id) -> ApiResult<NeedsView> {
        self.require_session()?;
        self.get(&super::paths::collection_op(
            collection_id,
            super::paths::op::NEEDS,
        ))
        .await
    }

    async fn shopping_list(&self) -> ApiResult<ShoppingList> {
        self.require_session()?;
        self.get(super::paths::SHOPPING_LIST).await
    }
}

/// The hosted API origin: `TR_WEB_ORIGIN` if exported, else the baked default.
fn web_origin() -> String {
    std::env::var("TR_WEB_ORIGIN")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_WEB_ORIGIN.to_string())
}
