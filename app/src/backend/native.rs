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
    CardSummary, CatalogCount, CollectionSummary, CollectionView, DeckCommanders, DesireLine,
    ErrorEnvelope, HoldingLine, Id, LineResult, MoveReceipt, MoveRequest, NeedsView, NewCollection,
    NewTag, Page, Rename, RenameTag, Reorder, Reparent, SearchQuery, SearchResults, SetBoard,
    SetQuantity, ShoppingList, SuggestedDestination, Tag, TagAssignment, TaggedCard, Teardown,
    TeardownReceipt,
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

/// Auth's path-B silent-refresh material (specs/auth.md): the long-lived
/// upstream `tr_session` value plus the origin to mint against. Carried on
/// session calls so an expired 15-min `tr_jwt` can re-mint a fresh JWT and
/// retry once, transparently, instead of bouncing the user to sign-in.
struct Refresh {
    session: String,
    origin: String,
}

/// A per-request client of the hosted API. `token` is the caller's current
/// `tr_jwt` (forwarded on session-scoped calls; `None` for anonymous catalog
/// reads *or* when the 15-min JWT has already expired and the webview dropped
/// the cookie — in which case `refresh` re-mints one on demand).
pub struct NativeBackend {
    base: String,
    token: Option<String>,
    refresh: Option<Refresh>,
}

impl NativeBackend {
    /// Anonymous client — catalog reads only.
    pub fn anonymous() -> Self {
        Self {
            base: web_origin(),
            token: None,
            refresh: None,
        }
    }

    /// Session client. `token` is the caller's current `tr_jwt` cookie (`None`
    /// once the 15-min JWT expires and the webview drops it); `session` is the
    /// `tr_session` used to silently re-mint a fresh JWT on a `401`; `origin` is
    /// the caller's own origin to mint against (auth CSRF-checks it against its
    /// trusted origins, and `allow_localhost` covers the embedded server). With
    /// `session = None` there is no refresh material, so an expired token is
    /// terminal (`Unauthorized`).
    pub fn authed(token: Option<String>, session: Option<String>, origin: String) -> Self {
        Self {
            base: web_origin(),
            token,
            refresh: session.map(|session| Refresh { session, origin }),
        }
    }

    /// Build + send one request to a hosted JSON route with an explicit bearer
    /// token (so the caller can retry with a freshly-minted JWT). A transport
    /// failure — the hosted API unreachable (offline) — maps to
    /// [`ApiError::Upstream`], the defined offline behavior (OQ#3): distinct
    /// from an auth error, so callers/UI can tell "can't reach the server" from
    /// "signed out".
    async fn dispatch<B: serde::Serialize>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
        token: Option<&str>,
    ) -> ApiResult<reqwest::Response> {
        let url = format!("{}{path}", self.base);
        let mut req = client().await.request(method, &url);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        req.send()
            .await
            .map_err(|e| ApiError::Upstream(format!("request to {path} failed: {e}")))
    }

    /// Send a request to a hosted JSON route, attaching the bearer token and an
    /// optional JSON body, and return the raw response on 2xx — mapping a non-2xx
    /// status onto the shared error via its wire envelope.
    ///
    /// On a `401` this performs auth's **path-B silent refresh**: re-mint the JWT
    /// from `tr_session` (auth's `mint_jwt`) and retry the request exactly once.
    /// The re-minted JWT is used only for this call — the webview's `tr_jwt`
    /// cookie is re-hosted separately by the `current_user` poll (account.rs), so
    /// this stays a pure transport concern and touches no Leptos response state.
    async fn send<B: serde::Serialize>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> ApiResult<reqwest::Response> {
        let resp = self
            .dispatch(method.clone(), path, body, self.token.as_deref())
            .await?;
        if resp.status().as_u16() != 401 {
            return Self::into_result(resp).await;
        }
        // 401 → silent re-mint + one retry. If there is no refresh material, or
        // the re-mint fails (session revoked, or the *auth* service unreachable),
        // fall through and surface the original 401 as `Unauthorized` — the
        // hosted API answered, so this is an auth error, not an offline one.
        if let Some(refresh) = &self.refresh {
            if let Ok(fresh) =
                crate::auth::upstream::mint_jwt(&refresh.origin, &refresh.session).await
            {
                let retry = self.dispatch(method, path, body, Some(&fresh)).await?;
                return Self::into_result(retry).await;
            }
        }
        Self::into_result(resp).await
    }

    /// Map a response onto the shared result: the raw response on 2xx, else the
    /// shared error reconstructed from status + the `{error:{…}}` wire envelope.
    async fn into_result(resp: reqwest::Response) -> ApiResult<reqwest::Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
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

    /// Require session credentials before a session-scoped call; the hosted route
    /// would 401 anyway, but this skips a round trip. A live `tr_session` (in
    /// `refresh`) is sufficient even when the `tr_jwt` has expired — `send`
    /// re-mints from it — so only the fully-anonymous case short-circuits.
    fn require_session(&self) -> ApiResult<()> {
        if self.token.is_none() && self.refresh.is_none() {
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

    // --- Tags & boards (specs/card-tagging.md) ------------------------------

    async fn create_tag(&self, req: NewTag) -> ApiResult<Tag> {
        self.require_session()?;
        self.post(super::paths::TAGS, &req).await
    }

    async fn rename_tag(&self, tag_id: Id, req: RenameTag) -> ApiResult<Tag> {
        self.require_session()?;
        self.post(
            &super::paths::tag_op(tag_id, super::paths::op::RENAME),
            &req,
        )
        .await
    }

    async fn delete_tag(&self, tag_id: Id) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(&super::paths::tag_op(tag_id, super::paths::op::DELETE), &())
            .await
    }

    async fn list_tags(&self, collection_id: Id) -> ApiResult<Vec<Tag>> {
        self.require_session()?;
        self.get(&super::paths::collection_op(
            collection_id,
            super::paths::op::TAGS,
        ))
        .await
    }

    async fn assign_tag(&self, req: TagAssignment) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(super::paths::TAGS_ASSIGN, &req).await
    }

    async fn unassign_tag(&self, req: TagAssignment) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(super::paths::TAGS_UNASSIGN, &req).await
    }

    async fn card_tags(&self, collection_id: Id, oracle_id: Id) -> ApiResult<Vec<Tag>> {
        self.require_session()?;
        self.get(&super::paths::card_tags(collection_id, oracle_id))
            .await
    }

    async fn cards_with_tag(&self, collection_id: Id, tag_id: Id) -> ApiResult<Vec<TaggedCard>> {
        self.require_session()?;
        self.get(&super::paths::tag_cards(collection_id, tag_id))
            .await
    }

    async fn deck_commanders(&self, collection_id: Id) -> ApiResult<DeckCommanders> {
        self.require_session()?;
        self.get(&super::paths::collection_op(
            collection_id,
            super::paths::op::COMMANDERS,
        ))
        .await
    }

    async fn set_holding_board(&self, holding_id: Id, req: SetBoard) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(&super::paths::holding_board(holding_id), &req)
            .await
    }

    async fn set_desire_board(&self, desire_id: Id, req: SetBoard) -> ApiResult<()> {
        self.require_session()?;
        self.post_unit(&super::paths::desire_board(desire_id), &req)
            .await
    }
}

/// The hosted API origin: `TR_WEB_ORIGIN` if exported, else the baked default.
fn web_origin() -> String {
    std::env::var("TR_WEB_ORIGIN")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_WEB_ORIGIN.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_session_accepts_a_live_session_even_without_a_jwt() {
        // An expired 15-min JWT (dropped by the webview) with a live `tr_session`
        // is still a session: `require_session` must not short-circuit — `send`
        // re-mints from the session on the ensuing 401.
        let refresh_only =
            NativeBackend::authed(None, Some("sess".into()), "http://localhost:1420".into());
        assert!(refresh_only.require_session().is_ok());

        let with_jwt = NativeBackend::authed(
            Some("jwt".into()),
            Some("sess".into()),
            "http://localhost:1420".into(),
        );
        assert!(with_jwt.require_session().is_ok());
    }

    #[test]
    fn require_session_rejects_the_fully_anonymous_case() {
        // No JWT and no session → nothing to mint from; terminal Unauthorized
        // without a wasted round trip.
        let none = NativeBackend::authed(None, None, "http://localhost:1420".into());
        assert!(matches!(
            none.require_session(),
            Err(ApiError::Unauthorized(_))
        ));
        assert!(matches!(
            NativeBackend::anonymous().require_session(),
            Err(ApiError::Unauthorized(_))
        ));
    }
}
