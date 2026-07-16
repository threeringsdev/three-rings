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

use shared::{ApiError, ApiResult, CatalogCount, CollectionSummary, ErrorEnvelope};
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

    /// GET a hosted JSON route and deserialize the response into `T`, mapping a
    /// non-2xx status onto the shared error via its wire envelope.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        let url = format!("{}{path}", self.base);
        let mut req = client().await.get(&url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ApiError::Upstream(format!("request to {path} failed: {e}")))?;

        let status = resp.status();
        if status.is_success() {
            return resp
                .json::<T>()
                .await
                .map_err(|e| ApiError::Upstream(format!("decoding {path} failed: {e}")));
        }

        // Error path: reconstruct the shared error from status + wire body.
        // TODO(collection-api native client): on 401, silently re-mint the JWT
        // from `tr_session` via auth's refresh path and retry once, per
        // data-access-backends.md. Until that cookie-jar plumbing lands with the
        // session-scoped endpoints, a 401 surfaces as Unauthorized.
        let code = status.as_u16();
        let body = resp.json::<ErrorEnvelope>().await.ok().map(|e| e.error);
        Err(ApiError::from_wire(code, body))
    }
}

impl CatalogStore for NativeBackend {
    async fn card_count(&self) -> ApiResult<CatalogCount> {
        self.get(super::paths::CATALOG_COUNT).await
    }
}

impl CollectionStore for NativeBackend {
    async fn list_collections(&self) -> ApiResult<Vec<CollectionSummary>> {
        if self.token.is_none() {
            return Err(ApiError::Unauthorized("no session token".into()));
        }
        self.get(super::paths::COLLECTIONS).await
    }
}

/// The hosted API origin: `TR_WEB_ORIGIN` if exported, else the baked default.
fn web_origin() -> String {
    std::env::var("TR_WEB_ORIGIN")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_WEB_ORIGIN.to_string())
}
