//! Our own session cookies (path B, specs/auth.md → Integration architecture).
//!
//! The browser and Tauri webviews only ever carry cookies on *our* origin;
//! the Better Auth cookies from the upstream service are captured
//! server-to-server ([`super::upstream`]) and re-hosted here as httpOnly
//! cookies. Three cookies:
//!
//! - [`SESSION_COOKIE`]: the upstream Better Auth session token (its full
//!   signed value, replayed verbatim on upstream calls). ~7-day lifetime,
//!   mirroring the upstream session.
//! - [`JWT_COOKIE`]: the current EdDSA JWT minted from that session (15-min
//!   upstream lifetime); verified locally by [`crate::auth`] on every request.
//! - [`CHALLENGE_COOKIE`]: the upstream OAuth session challenge, held for the
//!   few minutes between starting a social sign-in and its callback.

use axum::http::HeaderMap;

pub const SESSION_COOKIE: &str = "tr_session";
pub const JWT_COOKIE: &str = "tr_jwt";
pub const CHALLENGE_COOKIE: &str = "tr_challenge";

/// Upstream lifetimes, mirrored: session 7 days, JWT 15 minutes (measured on
/// the live dev service, 2026-07-13), challenge 10 minutes (upstream Max-Age).
pub const SESSION_MAX_AGE: u32 = 7 * 24 * 60 * 60;
pub const JWT_MAX_AGE: u32 = 15 * 60;
pub const CHALLENGE_MAX_AGE: u32 = 10 * 60;

/// A `Set-Cookie` value for an httpOnly, same-site cookie on our origin.
/// `secure` should come from [`request_is_secure`] — cookies set over the
/// Render deployment are `Secure`, local dev over http skips the attribute.
pub fn set_cookie(name: &str, value: &str, max_age: u32, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{name}={value}; Max-Age={max_age}; Path=/; HttpOnly; SameSite=Lax{secure_attr}")
}

/// A `Set-Cookie` value that expires the named cookie immediately.
pub fn clear_cookie(name: &str, secure: bool) -> String {
    set_cookie(name, "", 0, secure)
}

/// Read one cookie's value from the request `Cookie` header(s).
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// The request's own external origin (`scheme://host`), for the upstream
/// `Origin` header (Better Auth CSRF-checks it against its trusted origins)
/// and for building the OAuth callback URL. Render terminates TLS and sets
/// `x-forwarded-proto`/`host`; plain local serving has only `host`.
///
/// Under a Tauri embedded server the shell-exported loopback origin
/// ([`super::native::embedded_origin`]) is authoritative and wins outright —
/// mirroring the preference `account::google_sign_in` already applied only to
/// itself (specs/auth.md: the release desktop window's `Host` header doesn't
/// reliably match the origin Neon Auth is configured to trust, e.g. the
/// `127.0.0.1` the webview used to navigate to before that was fixed to
/// `localhost` in `src-tauri/src/lib.rs`). Every other Better-Auth-facing
/// caller of this function — `sign_in`/`sign_up`/`verify_email`/etc. in
/// `account.rs`, and the native data-access backend's origin
/// (`NativeBackend::authed`, `user_id_with_session_fallback`'s JWT re-mint) —
/// now gets the same override for free, closing the asymmetry recorded in the
/// 2026-08-15 Findings entry.
pub fn request_origin(headers: &HeaderMap) -> String {
    request_origin_with(super::native::embedded_origin().as_deref(), headers)
}

/// Pure core of [`request_origin`]: given the Tauri embedded origin (if any,
/// taken as an explicit parameter rather than read from the environment here)
/// and the request headers, decide the origin to present. Split out so the
/// decision is unit-testable without racing on process-global env vars across
/// parallel tests — the same split `lib.rs`'s `localhost_redirect_target` uses.
fn request_origin_with(native_origin: Option<&str>, headers: &HeaderMap) -> String {
    if let Some(origin) = native_origin {
        return origin.to_string();
    }
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(axum::http::header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:3000");
    format!("{proto}://{host}")
}

/// Whether cookies we set should carry `Secure` (the request reached us over
/// https, directly or via the proxy).
pub fn request_is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|p| p.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::{COOKIE, HOST};

    #[test]
    fn cookie_value_finds_named_cookie() {
        let mut h = HeaderMap::new();
        h.append(COOKIE, "a=1; tr_jwt=abc.def.ghi; b=2".parse().unwrap());
        assert_eq!(cookie_value(&h, "tr_jwt").as_deref(), Some("abc.def.ghi"));
        assert_eq!(cookie_value(&h, "tr_session"), None);
    }

    #[test]
    fn cookie_value_ignores_name_suffix_collisions() {
        let mut h = HeaderMap::new();
        h.append(COOKIE, "xtr_jwt=nope; tr_jwt=yes".parse().unwrap());
        assert_eq!(cookie_value(&h, "tr_jwt").as_deref(), Some("yes"));
    }

    #[test]
    fn origin_prefers_forwarded_proto_and_host() {
        let mut h = HeaderMap::new();
        h.insert(HOST, "internal:10000".parse().unwrap());
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        h.insert(
            "x-forwarded-host",
            "three-rings-6p5o.onrender.com".parse().unwrap(),
        );
        assert_eq!(
            request_origin_with(None, &h),
            "https://three-rings-6p5o.onrender.com"
        );
        assert!(request_is_secure(&h));
    }

    #[test]
    fn origin_falls_back_to_host_header() {
        let mut h = HeaderMap::new();
        h.insert(HOST, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(request_origin_with(None, &h), "http://127.0.0.1:3000");
        assert!(!request_is_secure(&h));
    }

    /// The regression this task fixes: a release-desktop request whose `Host`
    /// is `127.0.0.1:<port>` (the webview's old navigation target — see
    /// `src-tauri/src/lib.rs`) must still present the trusted embedded origin
    /// upstream, not the untrusted header-derived one.
    #[test]
    fn native_origin_overrides_a_mismatched_host_header() {
        let mut h = HeaderMap::new();
        h.insert(HOST, "127.0.0.1:54321".parse().unwrap());
        assert_eq!(
            request_origin_with(Some("http://localhost:54321"), &h),
            "http://localhost:54321"
        );
    }

    /// Even when the `Host` header would itself compute the "right" answer
    /// (e.g. once the webview navigates to `localhost`, or a proxy sets
    /// `x-forwarded-*`), the native origin still wins outright — matching
    /// `account::google_sign_in`'s existing preference, so the two paths can
    /// never disagree.
    #[test]
    fn native_origin_wins_even_over_forwarded_headers() {
        let mut h = HeaderMap::new();
        h.insert(HOST, "internal:10000".parse().unwrap());
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        h.insert(
            "x-forwarded-host",
            "three-rings-6p5o.onrender.com".parse().unwrap(),
        );
        assert_eq!(
            request_origin_with(Some("http://localhost:3000"), &h),
            "http://localhost:3000"
        );
    }

    /// `request_origin` (the impure wrapper actually used by callers) reads
    /// the real environment. Absent `TR_EMBEDDED_ORIGIN` (never set in the
    /// test process), it must fall back to the header-derived value exactly
    /// like `request_origin_with(None, ..)` — locking the wiring between the
    /// two without depending on a set env var (which would race other tests).
    #[test]
    fn request_origin_matches_the_no_native_core_absent_the_env_var() {
        assert!(
            std::env::var("TR_EMBEDDED_ORIGIN").is_err(),
            "test process must not have TR_EMBEDDED_ORIGIN set"
        );
        let mut h = HeaderMap::new();
        h.insert(HOST, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(request_origin(&h), request_origin_with(None, &h));
    }

    #[test]
    fn set_and_clear_cookie_shapes() {
        assert_eq!(
            set_cookie("tr_jwt", "v", 900, true),
            "tr_jwt=v; Max-Age=900; Path=/; HttpOnly; SameSite=Lax; Secure"
        );
        assert_eq!(
            clear_cookie("tr_jwt", false),
            "tr_jwt=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax"
        );
    }
}
