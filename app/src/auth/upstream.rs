//! Server-to-server client for the Neon Auth (Better Auth) REST API — the
//! proxy half of path B (specs/auth.md → Integration architecture).
//!
//! Every call sends an `Origin` header carrying *our* origin: the upstream
//! service CSRF-checks it against its trusted-origins config (Render URL on
//! production, `allow_localhost` on dev) and rejects state-changing calls
//! without one (`MISSING_ORIGIN`, observed live). Upstream sessions arrive as
//! a `Set-Cookie` for `__Secure-neon-auth.session_token`; we capture the raw
//! value and replay it verbatim as a `Cookie` header on later calls (the
//! bearer form of the session token is not accepted — verified live).
//!
//! The Google flow is verifier/challenge based (mechanism read out of Neon's
//! own server SDK, `neon-js` `packages/auth/src/server/middleware/oauth.ts`):
//! `POST /sign-in/social` returns the provider URL *and* sets a
//! `__Secure-neon-auth.session_challange` cookie (upstream's typo) that we
//! re-host on our origin; the callback lands on our
//! `/auth/callback?neon_auth_session_verifier=…`, and
//! `GET /get-session?neon_auth_session_verifier=…` with the challenge cookie
//! replayed exchanges the pair for a session.

use serde::Deserialize;
use tokio::sync::OnceCell;

/// Upstream cookie names (`__Secure-` prefixed; the `challange` typo matches
/// the auth server).
const UPSTREAM_SESSION_COOKIE: &str = "__Secure-neon-auth.session_token";
const UPSTREAM_CHALLENGE_COOKIE: &str = "__Secure-neon-auth.session_challange";

/// Query parameter the OAuth callback returns to our origin.
pub const SESSION_VERIFIER_PARAM: &str = "neon_auth_session_verifier";

#[derive(Debug)]
pub enum UpstreamError {
    /// The auth service rejected the request (a user-meaningful failure —
    /// wrong password, unverified email, …). `code` is Better Auth's error
    /// code, e.g. `INVALID_EMAIL_OR_PASSWORD` or `EMAIL_NOT_VERIFIED`.
    Api { code: String, message: String },
    /// Transport / unexpected-shape failures — our problem, not the user's.
    Http(String),
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamError::Api { code, message } => write!(f, "auth api {code}: {message}"),
            UpstreamError::Http(m) => write!(f, "auth http error: {m}"),
        }
    }
}

impl std::error::Error for UpstreamError {}

impl UpstreamError {
    pub fn code(&self) -> Option<&str> {
        match self {
            UpstreamError::Api { code, .. } => Some(code),
            UpstreamError::Http(_) => None,
        }
    }
}

/// An upstream Better Auth session: the raw cookie value we replay on
/// upstream calls (and re-host in [`super::cookies::SESSION_COOKIE`]).
#[derive(Debug, Clone)]
pub struct Session {
    pub cookie_value: String,
}

fn base_url() -> Result<String, UpstreamError> {
    std::env::var("NEON_AUTH_BASE_URL")
        .map(|u| u.trim_end_matches('/').to_string())
        .map_err(|_| UpstreamError::Http("NEON_AUTH_BASE_URL is not set".into()))
}

static CLIENT: OnceCell<reqwest::Client> = OnceCell::const_new();

async fn client() -> &'static reqwest::Client {
    CLIENT
        .get_or_init(|| async {
            reqwest::Client::builder()
                .build()
                .expect("reqwest client construction cannot fail with default TLS")
        })
        .await
}

/// One upstream call. `session` / `challenge` replay captured cookie values.
async fn call(
    method: reqwest::Method,
    path: &str,
    origin: &str,
    body: Option<serde_json::Value>,
    session: Option<&str>,
    challenge: Option<&str>,
) -> Result<reqwest::Response, UpstreamError> {
    let base = base_url()?;
    let mut req = client()
        .await
        .request(method, format!("{base}{path}"))
        .header(reqwest::header::ORIGIN, origin);
    let mut cookies = Vec::new();
    if let Some(v) = session {
        cookies.push(format!("{UPSTREAM_SESSION_COOKIE}={v}"));
    }
    if let Some(v) = challenge {
        cookies.push(format!("{UPSTREAM_CHALLENGE_COOKIE}={v}"));
    }
    if !cookies.is_empty() {
        req = req.header(reqwest::header::COOKIE, cookies.join("; "));
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| UpstreamError::Http(e.to_string()))?;

    if resp.status().is_success() {
        return Ok(resp);
    }
    // Better Auth error bodies: {"code": "...", "message": "..."}.
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let code = body["code"].as_str().unwrap_or("UNKNOWN").to_string();
    let message = body["message"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("auth service returned {status}"));
    Err(UpstreamError::Api { code, message })
}

/// Pull a named cookie's raw value out of a response's `Set-Cookie` headers.
fn response_cookie(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .filter_map(|h| h.split(';').next())
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

fn session_from(resp: &reqwest::Response) -> Result<Session, UpstreamError> {
    response_cookie(resp, UPSTREAM_SESSION_COOKIE)
        .map(|cookie_value| Session { cookie_value })
        .ok_or_else(|| UpstreamError::Http("no session cookie in auth response".into()))
}

pub async fn sign_up_email(
    origin: &str,
    email: &str,
    password: &str,
    name: &str,
) -> Result<Option<Session>, UpstreamError> {
    let resp = call(
        reqwest::Method::POST,
        "/sign-up/email",
        origin,
        Some(serde_json::json!({ "email": email, "password": password, "name": name })),
        None,
        None,
    )
    .await?;
    // With verify-email-on-sign-up enabled the account is created but no
    // session is issued until the OTP is confirmed — that's the None case.
    Ok(session_from(&resp).ok())
}

pub async fn sign_in_email(
    origin: &str,
    email: &str,
    password: &str,
) -> Result<Session, UpstreamError> {
    let resp = call(
        reqwest::Method::POST,
        "/sign-in/email",
        origin,
        Some(serde_json::json!({ "email": email, "password": password })),
        None,
        None,
    )
    .await?;
    session_from(&resp)
}

/// Ask the service to email a fresh verification OTP.
pub async fn send_verification_otp(origin: &str, email: &str) -> Result<(), UpstreamError> {
    call(
        reqwest::Method::POST,
        "/email-otp/send-verification-otp",
        origin,
        Some(serde_json::json!({ "email": email, "type": "email-verification" })),
        None,
        None,
    )
    .await
    .map(|_| ())
}

/// Confirm an emailed OTP. With auto-sign-in-after-verification enabled the
/// response carries a session; `None` means verified but not signed in.
pub async fn verify_email_otp(
    origin: &str,
    email: &str,
    otp: &str,
) -> Result<Option<Session>, UpstreamError> {
    let resp = call(
        reqwest::Method::POST,
        "/email-otp/verify-email",
        origin,
        Some(serde_json::json!({ "email": email, "otp": otp })),
        None,
        None,
    )
    .await?;
    Ok(session_from(&resp).ok())
}

/// Mint a short-lived EdDSA JWT from the session (the JWT plugin's `/token`;
/// only the cookie form of the session is accepted — verified live).
pub async fn mint_jwt(origin: &str, session: &str) -> Result<String, UpstreamError> {
    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }
    let resp = call(
        reqwest::Method::GET,
        "/token",
        origin,
        None,
        Some(session),
        None,
    )
    .await?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| UpstreamError::Http(format!("bad /token response: {e}")))?;
    Ok(body.token)
}

/// Best-effort upstream sign-out (revokes the Better Auth session).
pub async fn sign_out(origin: &str, session: &str) -> Result<(), UpstreamError> {
    call(
        reqwest::Method::POST,
        "/sign-out",
        origin,
        Some(serde_json::json!({})),
        Some(session),
        None,
    )
    .await
    .map(|_| ())
}

/// Start a social sign-in. Returns the provider URL to send the browser to,
/// plus the challenge cookie value to re-host on our origin for the callback.
pub async fn social_start(
    origin: &str,
    provider: &str,
    callback_url: &str,
) -> Result<(String, String), UpstreamError> {
    #[derive(Deserialize)]
    struct SocialResponse {
        url: String,
    }
    let resp = call(
        reqwest::Method::POST,
        "/sign-in/social",
        origin,
        Some(serde_json::json!({ "provider": provider, "callbackURL": callback_url })),
        None,
        None,
    )
    .await?;
    let challenge = response_cookie(&resp, UPSTREAM_CHALLENGE_COOKIE)
        .ok_or_else(|| UpstreamError::Http("no challenge cookie in social response".into()))?;
    let body: SocialResponse = resp
        .json()
        .await
        .map_err(|e| UpstreamError::Http(format!("bad social response: {e}")))?;
    Ok((body.url, challenge))
}

/// Complete a social sign-in: exchange the callback's verifier plus our held
/// challenge for a session.
pub async fn social_complete(
    origin: &str,
    verifier: &str,
    challenge: &str,
) -> Result<Session, UpstreamError> {
    let resp = call(
        reqwest::Method::GET,
        &format!(
            "/get-session?{SESSION_VERIFIER_PARAM}={}",
            urlencode(verifier)
        ),
        origin,
        None,
        None,
        Some(challenge),
    )
    .await?;
    session_from(&resp)
}

/// Minimal percent-encoding for a URL query value (the verifier is UUID-like,
/// so this is belt-and-braces, not a general encoder).
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_passes_unreserved_and_escapes_rest() {
        assert_eq!(urlencode("abc-123_~."), "abc-123_~.");
        assert_eq!(urlencode("a+b/c="), "a%2Bb%2Fc%3D");
    }
}
