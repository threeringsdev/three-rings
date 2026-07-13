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
pub fn request_origin(headers: &HeaderMap) -> String {
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
        assert_eq!(request_origin(&h), "https://three-rings-6p5o.onrender.com");
        assert!(request_is_secure(&h));
    }

    #[test]
    fn origin_falls_back_to_host_header() {
        let mut h = HeaderMap::new();
        h.insert(HOST, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(request_origin(&h), "http://127.0.0.1:3000");
        assert!(!request_is_secure(&h));
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
