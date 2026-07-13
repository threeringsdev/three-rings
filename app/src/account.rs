//! Account server functions — the path-B cookie session (specs/auth.md).
//!
//! The Leptos pages call these over `/api/*`; on the server they proxy the
//! Neon Auth (Better Auth) REST API ([`crate::auth::upstream`]) and manage
//! our httpOnly cookies ([`crate::auth::cookies`]). The browser (or Tauri
//! webview) never talks to the auth service and never sees a token.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// The signed-in user as the UI needs it, straight from verified JWT claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentUser {
    /// `neon_auth."user".id` (uuid, stringly typed here — the wasm side
    /// doesn't carry the uuid crate).
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// What an auth attempt produced, for the UI to branch on. Business-level
/// failures (wrong password, unknown OTP) are data, not `Err` — the `Err`
/// channel is for transport/config faults only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthOutcome {
    SignedIn(CurrentUser),
    /// The account needs email verification; an OTP was mailed to `email`.
    VerificationRequired {
        email: String,
    },
    Failed {
        message: String,
    },
}

#[cfg(feature = "ssr")]
mod ssr {
    use super::CurrentUser;
    use crate::auth::{cookies, upstream, verify_token, Claims};
    use axum::http::header::SET_COOKIE;
    use axum::http::{HeaderMap, HeaderValue};
    use leptos::prelude::*;
    use leptos_axum::ResponseOptions;

    impl From<Claims> for CurrentUser {
        fn from(claims: Claims) -> Self {
            CurrentUser {
                id: claims.sub,
                email: claims.email,
                name: claims.name,
            }
        }
    }

    pub fn server_err<E: std::fmt::Display>(e: E) -> ServerFnError<String> {
        ServerFnError::ServerError(e.to_string())
    }

    /// The incoming request's headers (cookies, host, forwarded proto).
    pub async fn request_headers() -> Result<HeaderMap, ServerFnError<String>> {
        leptos_axum::extract::<HeaderMap>()
            .await
            .map_err(server_err)
    }

    pub fn append_set_cookie(cookie: &str) -> Result<(), ServerFnError<String>> {
        let response: ResponseOptions = expect_context();
        response.append_header(
            SET_COOKIE,
            HeaderValue::from_str(cookie).map_err(server_err)?,
        );
        Ok(())
    }

    /// Re-host an upstream session on our origin: store the session cookie,
    /// mint + verify a JWT (which also proves the session against the JWKS),
    /// store it too, and describe the user from the verified claims.
    pub async fn establish_session(
        origin: &str,
        secure: bool,
        session: &upstream::Session,
    ) -> Result<CurrentUser, ServerFnError<String>> {
        let jwt = upstream::mint_jwt(origin, &session.cookie_value)
            .await
            .map_err(server_err)?;
        let claims = verify_token(&jwt).await.map_err(server_err)?;
        append_set_cookie(&cookies::set_cookie(
            cookies::SESSION_COOKIE,
            &session.cookie_value,
            cookies::SESSION_MAX_AGE,
            secure,
        ))?;
        append_set_cookie(&cookies::set_cookie(
            cookies::JWT_COOKIE,
            &jwt,
            cookies::JWT_MAX_AGE,
            secure,
        ))?;
        Ok(claims.into())
    }

    pub fn clear_session_cookies(secure: bool) -> Result<(), ServerFnError<String>> {
        for name in [
            cookies::SESSION_COOKIE,
            cookies::JWT_COOKIE,
            cookies::CHALLENGE_COOKIE,
        ] {
            append_set_cookie(&cookies::clear_cookie(name, secure))?;
        }
        Ok(())
    }
}

/// Sign up with email + password. With verify-on-sign-up enabled upstream,
/// the account is created, an OTP is mailed, and the outcome asks for it.
#[server(prefix = "/api", endpoint = "sign_up")]
pub async fn sign_up(
    name: String,
    email: String,
    password: String,
) -> Result<AuthOutcome, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        let secure = cookies::request_is_secure(&headers);
        match upstream::sign_up_email(&origin, &email, &password, &name).await {
            Ok(Some(session)) => Ok(AuthOutcome::SignedIn(
                ssr::establish_session(&origin, secure, &session).await?,
            )),
            // Account created, no session: the upstream mailed a verification
            // OTP as part of sign-up — don't send a duplicate here.
            Ok(None) => Ok(AuthOutcome::VerificationRequired { email }),
            Err(upstream::UpstreamError::Api { code, message }) => Ok(AuthOutcome::Failed {
                // The Google-first dead end (specs/auth.md): this email may
                // exist with only a Google identity — point at the path that
                // adds a password (the reset flow) instead of stranding them.
                message: if code == "USER_ALREADY_EXISTS" {
                    "An account with this email already exists — sign in instead. \
                     If you signed up with Google, use “Forgot password?” on the \
                     sign-in page to add a password."
                        .into()
                } else {
                    message
                },
            }),
            Err(e) => Err(ssr::server_err(e)),
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (name, email, password);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Sign in with email + password.
#[server(prefix = "/api", endpoint = "sign_in")]
pub async fn sign_in(
    email: String,
    password: String,
) -> Result<AuthOutcome, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        let secure = cookies::request_is_secure(&headers);
        match upstream::sign_in_email(&origin, &email, &password).await {
            Ok(session) => Ok(AuthOutcome::SignedIn(
                ssr::establish_session(&origin, secure, &session).await?,
            )),
            Err(upstream::UpstreamError::Api { code, message }) => {
                if code == "EMAIL_NOT_VERIFIED" {
                    // Make sure a fresh code is on its way before asking.
                    let _ = upstream::send_verification_otp(&origin, &email).await;
                    Ok(AuthOutcome::VerificationRequired { email })
                } else {
                    Ok(AuthOutcome::Failed { message })
                }
            }
            Err(e) => Err(ssr::server_err(e)),
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (email, password);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Confirm the emailed verification OTP; signs the user in on success
/// (auto-sign-in-after-verification is enabled upstream).
#[server(prefix = "/api", endpoint = "verify_email")]
pub async fn verify_email(
    email: String,
    otp: String,
) -> Result<AuthOutcome, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        let secure = cookies::request_is_secure(&headers);
        match upstream::verify_email_otp(&origin, &email, &otp).await {
            Ok(Some(session)) => Ok(AuthOutcome::SignedIn(
                ssr::establish_session(&origin, secure, &session).await?,
            )),
            Ok(None) => Ok(AuthOutcome::Failed {
                message: "Email verified — please sign in.".into(),
            }),
            Err(upstream::UpstreamError::Api { message, .. }) => {
                Ok(AuthOutcome::Failed { message })
            }
            Err(e) => Err(ssr::server_err(e)),
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (email, otp);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Mail a fresh verification OTP.
#[server(prefix = "/api", endpoint = "resend_verification")]
pub async fn resend_verification(email: String) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        upstream::send_verification_otp(&origin, &email)
            .await
            .map_err(ssr::server_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = email;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Mail a password-reset code. Also the entry point for **adding a password
/// to a Google-first account** — the upstream reset creates the credential
/// account when none exists (specs/auth.md). Success says nothing about
/// whether the account exists: the upstream anti-enumerates unknown emails.
#[server(prefix = "/api", endpoint = "request_password_reset")]
pub async fn request_password_reset(email: String) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        upstream::request_password_reset(&origin, &email)
            .await
            .map_err(ssr::server_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = email;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Set a new password with the emailed reset code, then sign straight in
/// with it — the upstream reset issues no session of its own, and it marks
/// the email verified, so the sign-in can't bounce into the verification
/// step.
#[server(prefix = "/api", endpoint = "reset_password")]
pub async fn reset_password(
    email: String,
    otp: String,
    password: String,
) -> Result<AuthOutcome, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        let secure = cookies::request_is_secure(&headers);
        match upstream::reset_password(&origin, &email, &otp, &password).await {
            Ok(()) => match upstream::sign_in_email(&origin, &email, &password).await {
                Ok(session) => Ok(AuthOutcome::SignedIn(
                    ssr::establish_session(&origin, secure, &session).await?,
                )),
                // The password *was* reset; only the follow-up sign-in
                // hiccuped. Say so rather than implying the reset failed.
                Err(upstream::UpstreamError::Api { .. }) => Ok(AuthOutcome::Failed {
                    message: "Password updated — sign in with it.".into(),
                }),
                Err(e) => Err(ssr::server_err(e)),
            },
            Err(upstream::UpstreamError::Api { message, .. }) => {
                Ok(AuthOutcome::Failed { message })
            }
            Err(e) => Err(ssr::server_err(e)),
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (email, otp, password);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The signed-in user, if any. Refreshes a missing/expired JWT from the
/// session cookie transparently (this is the path-B refresh: SSR page loads
/// and resources call this, so an idle-but-live session re-arms itself).
#[server(prefix = "/api", endpoint = "current_user")]
pub async fn fetch_current_user() -> Result<Option<CurrentUser>, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream, verify_token};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        let secure = cookies::request_is_secure(&headers);

        if let Some(jwt) = cookies::cookie_value(&headers, cookies::JWT_COOKIE) {
            if let Ok(claims) = verify_token(&jwt).await {
                return Ok(Some(claims.into()));
            }
        }
        let session = match cookies::cookie_value(&headers, cookies::SESSION_COOKIE) {
            Some(session_value) => upstream::Session {
                cookie_value: session_value,
            },
            // Tauri: a system-browser Google flow may have parked a session
            // on the embedded server — claim it and re-host it as ordinary
            // webview cookies (the login page polls this fn for exactly that).
            None => match crate::auth::native::take_session() {
                Some(session) => session,
                None => return Ok(None),
            },
        };
        match ssr::establish_session(&origin, secure, &session).await {
            Ok(user) => Ok(Some(user)),
            // Upstream session revoked/expired: drop our stale cookies.
            Err(_) => {
                ssr::clear_session_cookies(secure)?;
                Ok(None)
            }
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Sign out: revoke the upstream session (best effort) and clear our cookies.
#[server(prefix = "/api", endpoint = "sign_out")]
pub async fn sign_out() -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, upstream};
        let headers = ssr::request_headers().await?;
        let origin = cookies::request_origin(&headers);
        let secure = cookies::request_is_secure(&headers);
        if let Some(session) = cookies::cookie_value(&headers, cookies::SESSION_COOKIE) {
            let _ = upstream::sign_out(&origin, &session).await;
        }
        ssr::clear_session_cookies(secure)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Start the Google sign-in: returns the provider URL for the browser to
/// navigate to, holding the upstream's challenge for the `/auth/callback`
/// exchange — in our httpOnly cookie on the web, and *also* in the embedded
/// server's memory under a Tauri shell (the flow runs in the system browser
/// there, which never carries our webview cookies).
#[server(prefix = "/api", endpoint = "google_sign_in")]
pub async fn google_sign_in() -> Result<String, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::auth::{cookies, native, upstream};
        let headers = ssr::request_headers().await?;
        let secure = cookies::request_is_secure(&headers);
        // Under Tauri, use the shell-exported loopback origin for both the
        // upstream Origin header and the callback: the auth service trusts
        // `localhost` but rejects the `127.0.0.1` spelling the webview uses.
        let native_origin = native::embedded_origin();
        let origin = native_origin
            .clone()
            .unwrap_or_else(|| cookies::request_origin(&headers));
        // Where Google sends the browser back. Web and desktop take this
        // origin directly (a desktop browser can reach the embedded loopback
        // server). Android cannot: the OS freezes the backgrounded app, so
        // its loopback server never answers the redirect (observed live —
        // Chrome timed out on localhost:36265). Route Android through the
        // public web origin, whose /auth/app-return page bounces the
        // verifier into the app via the three-rings:// deep link.
        let (callback, error_url) = if native_origin.is_some() && cfg!(target_os = "android") {
            // Non-secret, like the baked auth URL in src-tauri; env wins.
            let web = std::env::var("TR_WEB_ORIGIN")
                .unwrap_or_else(|_| "https://three-rings-6p5o.onrender.com".into());
            let bounce = format!("{web}/auth/app-return");
            (bounce.clone(), bounce)
        } else {
            (
                format!("{origin}/auth/callback"),
                format!("{origin}/login?error=google"),
            )
        };
        let (url, challenge) = upstream::social_start(&origin, "google", &callback, &error_url)
            .await
            .map_err(ssr::server_err)?;
        if native_origin.is_some() {
            native::stash_challenge(challenge.clone());
        }
        ssr::append_set_cookie(&cookies::set_cookie(
            cookies::CHALLENGE_COOKIE,
            &challenge,
            cookies::CHALLENGE_MAX_AGE,
            secure,
        ))?;
        Ok(url)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}
