//! Neon Auth (Better Auth) JWT verification.
//!
//! Better Auth issues standard **EdDSA / Ed25519** JWTs and serves a JWKS per
//! Neon branch; we verify tokens locally against that JWKS — no shared secret,
//! no call back to the auth service on the hot path. See
//! [`specs/auth.md`](../../specs/auth.md) → *Integration architecture*.
//!
//! This module is the flow-agnostic verification core: fetch + cache the JWKS,
//! verify a bearer token's signature/issuer/expiry, and extract the user id
//! (`sub` = `neon_auth."user".id`, a uuid). Wiring the id into the per-request
//! `SET LOCAL app.user_id` transaction arrives with the data-model migrations;
//! the httpOnly-cookie proxy (path B) layers a cookie source on top of the same
//! core.

pub mod cookies;
pub mod upstream;

use std::collections::HashMap;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::{OnceCell, RwLock};
use uuid::Uuid;

/// Claims we use from a Better Auth JWT. `sub` is the user id
/// (`neon_auth."user".id`, a uuid); `exp` is validated by `jsonwebtoken`.
/// The profile fields ride along in the token (observed live, 2026-07-13)
/// and save an upstream round-trip when describing the signed-in user.
#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub email: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "emailVerified")]
    pub email_verified: Option<bool>,
}

/// Why authentication failed. Missing/invalid → 401; config/JWKS problems are
/// our fault → 500 (a client can't fix them by re-authenticating).
#[derive(Debug)]
pub enum AuthError {
    /// No bearer token on the request.
    MissingToken,
    /// Token present but malformed, unverifiable, expired, or wrong issuer.
    InvalidToken,
    /// Server misconfiguration (e.g. `NEON_AUTH_BASE_URL` unset).
    Configuration(String),
    /// Could not fetch or parse the branch JWKS.
    Jwks(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::MissingToken => write!(f, "missing bearer token"),
            AuthError::InvalidToken => write!(f, "invalid token"),
            AuthError::Configuration(m) => write!(f, "auth configuration error: {m}"),
            AuthError::Jwks(m) => write!(f, "jwks error: {m}"),
        }
    }
}

impl std::error::Error for AuthError {}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let code = match self {
            AuthError::MissingToken | AuthError::InvalidToken => StatusCode::UNAUTHORIZED,
            AuthError::Configuration(_) | AuthError::Jwks(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        code.into_response()
    }
}

/// One key from the branch JWKS. Better Auth publishes a single `OKP`/Ed25519
/// key; we ignore anything else (`x` is the base64url raw public key).
#[derive(Debug, Deserialize)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    x: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

/// Verifies EdDSA JWTs against a cached, lazily-refreshed branch JWKS.
pub struct Verifier {
    jwks_url: String,
    /// Both `iss` and `aud` are the base URL's *origin* (no `/neondb/auth`
    /// path) — confirmed against a live token 2026-07-13. `aud` must be
    /// validated explicitly: `jsonwebtoken` v9 rejects any token carrying an
    /// `aud` claim unless the expected audience is configured (this was the
    /// step-5 middleware's silent 401 against real tokens).
    origin: String,
    keys: RwLock<HashMap<String, DecodingKey>>,
}

impl Verifier {
    /// Build from `NEON_AUTH_BASE_URL` (the auth base URL). The JWKS lives at
    /// `<base_url>/.well-known/jwks.json`.
    pub fn from_env() -> Result<Self, AuthError> {
        let base_url = std::env::var("NEON_AUTH_BASE_URL")
            .map_err(|_| AuthError::Configuration("NEON_AUTH_BASE_URL is not set".into()))?;
        let base_url = base_url.trim_end_matches('/').to_string();
        let jwks_url = format!("{base_url}/.well-known/jwks.json");
        Ok(Self {
            jwks_url,
            origin: origin_of(&base_url),
            keys: RwLock::new(HashMap::new()),
        })
    }

    /// Fetch the JWKS and replace the key cache. Called lazily on an unknown
    /// `kid` (covers key rotation without a background refresh loop).
    async fn refresh(&self) -> Result<(), AuthError> {
        let set: JwkSet = reqwest::get(&self.jwks_url)
            .await
            .map_err(|e| AuthError::Jwks(e.to_string()))?
            .error_for_status()
            .map_err(|e| AuthError::Jwks(e.to_string()))?
            .json()
            .await
            .map_err(|e| AuthError::Jwks(e.to_string()))?;

        let mut map = HashMap::new();
        for jwk in set.keys {
            if jwk.kty != "OKP" {
                continue;
            }
            let (Some(kid), Some(x)) = (jwk.kid, jwk.x) else {
                continue;
            };
            let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&x)
                .map_err(|e| AuthError::Jwks(format!("bad JWK x: {e}")))?;
            map.insert(kid, DecodingKey::from_ed_der(&raw));
        }
        if map.is_empty() {
            return Err(AuthError::Jwks("no usable OKP keys in JWKS".into()));
        }
        *self.keys.write().await = map;
        Ok(())
    }

    async fn key_for(&self, kid: &str) -> Option<DecodingKey> {
        self.keys.read().await.get(kid).cloned()
    }

    /// Verify a JWT and return its claims. Refreshes the JWKS once on an unknown
    /// `kid` before giving up.
    pub async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
        let kid = header.kid.ok_or(AuthError::InvalidToken)?;

        let key = match self.key_for(&kid).await {
            Some(k) => k,
            None => {
                self.refresh().await?;
                self.key_for(&kid).await.ok_or(AuthError::InvalidToken)?
            }
        };

        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[&self.origin]);
        validation.set_audience(&[&self.origin]);
        validation.set_required_spec_claims(&["exp", "sub"]);
        let data =
            decode::<Claims>(token, &key, &validation).map_err(|_| AuthError::InvalidToken)?;
        Ok(data.claims)
    }
}

/// The process-wide verifier, initialized from env on first use.
static VERIFIER: OnceCell<Verifier> = OnceCell::const_new();

async fn verifier() -> Result<&'static Verifier, AuthError> {
    VERIFIER
        .get_or_try_init(|| async { Verifier::from_env() })
        .await
}

/// Verify a JWT with the process-wide verifier — the entry point for code
/// outside the extractor (the cookie-session server fns).
pub async fn verify_token(token: &str) -> Result<Claims, AuthError> {
    verifier().await?.verify(token).await
}

/// The `scheme://host[:port]` origin of a URL, for issuer matching. Falls back
/// to the whole string if it doesn't parse as expected.
fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{authority}")
}

/// An authenticated user, extracted from a verified bearer JWT. Use it as an
/// Axum handler argument to require (and identify) a signed-in caller; a
/// missing/invalid token short-circuits with 401 before the handler runs.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Native/API clients send `Authorization: Bearer`; browser and Tauri
        // webview sessions carry the JWT in our httpOnly cookie (path B).
        let token = bearer(parts)
            .or_else(|| cookies::cookie_value(&parts.headers, cookies::JWT_COOKIE))
            .ok_or(AuthError::MissingToken)?;
        let claims = verifier().await?.verify(&token).await?;
        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AuthError::InvalidToken)?;
        Ok(AuthUser { user_id })
    }
}

/// Pull the token out of an `Authorization: Bearer <token>` header.
fn bearer(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(axum::http::header::AUTHORIZATION)?;
    let text = value.to_str().ok()?;
    text.strip_prefix("Bearer ")
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_strips_path() {
        assert_eq!(
            origin_of("https://ep-x.neonauth.neon.tech/neondb/auth"),
            "https://ep-x.neonauth.neon.tech"
        );
        assert_eq!(origin_of("https://host:8443/a/b"), "https://host:8443");
        assert_eq!(origin_of("not-a-url"), "not-a-url");
    }
}
