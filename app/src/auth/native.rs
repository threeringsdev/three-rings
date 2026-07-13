//! Single-user embedded-server support for the system-browser OAuth flow
//! (specs/auth.md → Tauri desktop spike / desktop Google plan).
//!
//! In the Tauri shells the webview talks to an in-process Axum server bound
//! to a loopback port, and Google sign-in must run in the *system browser*
//! (Google blocks OAuth in embedded webviews). The external browser shares no
//! cookies with the webview, so the handoff state lives here, in process
//! memory — safe precisely because the embedded server serves exactly one
//! user. The Tauri shell opts in by exporting `TR_EMBEDDED_ORIGIN`
//! (`http://localhost:<port>`, the loopback origin the auth service trusts —
//! it rejects the `127.0.0.1` spelling in callback URLs).
//!
//! Flow: `google_sign_in` stashes the upstream challenge here (no cookie —
//! the external browser never had one) → the browser completes Google → the
//! callback lands on the embedded server, exchanges verifier + challenge, and
//! stashes the session here → the webview's `current_user` polling claims it
//! and re-hosts it as ordinary webview cookies.

use std::sync::Mutex;

use super::upstream::Session;

/// The embedded server's loopback origin, when running inside a Tauri shell.
pub fn embedded_origin() -> Option<String> {
    std::env::var("TR_EMBEDDED_ORIGIN").ok()
}

static PENDING_CHALLENGE: Mutex<Option<String>> = Mutex::new(None);
static PENDING_SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn stash_challenge(challenge: String) {
    *PENDING_CHALLENGE.lock().expect("challenge lock poisoned") = Some(challenge);
}

pub fn take_challenge() -> Option<String> {
    PENDING_CHALLENGE
        .lock()
        .expect("challenge lock poisoned")
        .take()
}

pub fn stash_session(session: Session) {
    *PENDING_SESSION.lock().expect("session lock poisoned") = Some(session);
}

pub fn take_session() -> Option<Session> {
    PENDING_SESSION
        .lock()
        .expect("session lock poisoned")
        .take()
}

/// Complete a Google sign-in whose callback returned through the deep link
/// (Android: the OS freezes the backgrounded app, so the system browser
/// can't reach the embedded server the way desktop browsers can; the public
/// web origin's `/auth/app-return` page hands the verifier back via
/// `three-rings://auth/callback` instead — specs/auth.md). Exchanges the
/// verifier + parked challenge for a session and parks that for the
/// webview's `current_user` poll, exactly like the loopback callback path.
/// A stolen verifier is useless without the challenge held here, so a rogue
/// app claiming the scheme gains nothing.
pub async fn complete_google_return(verifier: &str) -> Result<(), String> {
    let origin = embedded_origin().ok_or("not running with an embedded server")?;
    let challenge = take_challenge()
        .ok_or("no pending sign-in challenge (app restarted mid-flow? — start over)")?;
    let session = super::upstream::social_complete(&origin, verifier, &challenge)
        .await
        .map_err(|e| e.to_string())?;
    stash_session(session);
    Ok(())
}
