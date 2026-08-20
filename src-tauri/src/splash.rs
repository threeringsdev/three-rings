//! The launch splash the release shell paints while the *first* real page is
//! still being server-rendered.
//!
//! ## Why this exists (measured, 2026-08-20 — specs/architecture-spike.md)
//!
//! Every top-level route is `SsrMode::Async` (app/src/lib.rs — the auth
//! redirects need a real 302, which out-of-order streaming cannot give once
//! headers are flushed). `Async` means the server sends **no byte of HTML**
//! until every resource on the route has resolved, and on the `native`
//! backend those resources are HTTPS round trips: `/` awaits
//! `fetch_current_user`, which mints a JWT against Neon Auth (the 15-minute
//! `tr_jwt` cookie is long gone by the next launch) and fetches the branch
//! JWKS on a cold process, then 302s to `/my`, which awaits the hosted API on
//! Render. A WKWebView paints nothing during a provisional navigation, so all
//! of that is a blank window.
//!
//! So the shell navigates the window *here* first. This page is served by the
//! embedded server off the loopback interface with zero awaits — first byte in
//! well under a millisecond — and then, once it has provably painted a frame,
//! sends the webview on to `/`. The webview keeps painting this document until
//! the real page's response commits, so the remote round trips happen behind a
//! branded loading state instead of behind nothing.
//!
//! Kept in the shell rather than in `app`: the hosted web deployment has no
//! use for it (a browser shows the previous page, not a blank window, while a
//! navigation is in flight), and keeping the diff inside `src-tauri/` leaves
//! the web target byte-for-byte unchanged.

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

/// The path the embedded server serves the splash on. Double-underscored to
/// stay clear of the app's own route space (a Leptos route named `__loading`
/// would collide; nothing in specs/app-ui.md ever will).
pub const PATH: &str = "/__loading";

/// Where the splash sends the webview once it has painted. `/` re-runs the
/// shell's ordinary entry: `RootRedirect` 302s to `/my` or `/login`.
pub const NEXT: &str = "/";

/// How long the webview sits on the splash before it admits things are slow.
const SLOW_AFTER_MS: u32 = 8_000;

/// The hand-off deadline for when `requestAnimationFrame` never runs (see
/// [`JS`]). Long enough that a visible window always paints first, short
/// enough that an invisible one loses nothing worth measuring.
const HANDOFF_FALLBACK_MS: u32 = 300;

/// Add the splash route to the embedded server's router.
pub fn mount(router: Router) -> Router {
    router.route(PATH, get(handler))
}

async fn handler(headers: HeaderMap) -> Response {
    let dark = theme_is_dark(&headers);
    // One line per launch, and the only timestamp that says "the window has
    // something to look at now" — the remote work that follows is all in the
    // outbound reqwest logs after it.
    log::info!("splash: served (dark={dark})");
    response(dark)
}

/// The `tr_theme` override, else the dark default — the same rule
/// `app::shell()` applies to the real pages, so the splash and the page that
/// replaces it agree from the first frame. Absent or any non-`light` value is
/// dark (app/src/components/ui/theme_toggle.rs).
fn theme_is_dark(headers: &HeaderMap) -> bool {
    let prefix = format!("{}=", app::components::ui::theme_toggle::THEME_COOKIE);
    for value in headers.get_all(header::COOKIE) {
        let Ok(value) = value.to_str() else { continue };
        if let Some(dark) = value
            .split(';')
            .find_map(|pair| pair.trim().strip_prefix(&prefix).map(|v| v != "light"))
        {
            return dark;
        }
    }
    true
}

fn response(dark: bool) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        // Never let a webview serve this out of its cache in place of the
        // real page — it is a stepping stone, not content.
        .header(header::CACHE_CONTROL, "no-store")
        .body(axum::body::Body::from(page(dark)))
        .expect("static page construction cannot fail")
}

/// The splash document. Self-contained: no stylesheet, no wasm, no image —
/// nothing that could cost a second request before the first paint.
fn page(dark: bool) -> String {
    // The app's own tokens (style/input.css), resolved to hex so the page
    // needs no color-function support: `--background`, `--foreground`,
    // `--muted-foreground`, and a dim ring track between `--border` and
    // `--muted`.
    let vars = if dark {
        "--bg:#161616;--fg:#fafafa;--muted:#a1a1a1;--dim:#3a3a3a;color-scheme:dark"
    } else {
        "--bg:#ffffff;--fg:#0a0a0a;--muted:#737373;--dim:#d4d4d4;color-scheme:light"
    };
    // The script carries braces of its own, so it is a template with two
    // substitutions rather than another `format!` argument.
    let js = JS
        .replace("NEXT_PATH", &format!("\"{NEXT}\""))
        .replace("HANDOFF_FALLBACK_MS", &HANDOFF_FALLBACK_MS.to_string())
        .replace("SLOW_MS", &SLOW_AFTER_MS.to_string());
    format!(
        "<!DOCTYPE html><html lang=\"en\" style=\"{vars}\"><head>\
         <meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\
         <title>Three Rings</title>\
         <style>{CSS}</style></head>\
         <body><main>{RINGS}\
         <p class=\"wordmark\">Three Rings</p>\
         <p class=\"status\">Signing you in\u{2026}</p>\
         <p class=\"slow\" id=\"slow\">Still working \u{2014} the server may be waking up.</p>\
         </main><script>{js}</script></body></html>"
    )
}

/// Three concentric arcs, one per ring, over their own dim tracks.
const RINGS: &str = "<svg class=\"rings\" viewBox=\"0 0 64 64\" width=\"64\" height=\"64\" \
     fill=\"none\" stroke-width=\"2.5\" stroke-linecap=\"round\" aria-hidden=\"true\">\
     <circle cx=\"32\" cy=\"32\" r=\"28\" stroke=\"var(--dim)\"/>\
     <circle cx=\"32\" cy=\"32\" r=\"20\" stroke=\"var(--dim)\"/>\
     <circle cx=\"32\" cy=\"32\" r=\"12\" stroke=\"var(--dim)\"/>\
     <circle class=\"r r1\" cx=\"32\" cy=\"32\" r=\"28\" stroke=\"var(--fg)\" \
     stroke-dasharray=\"44 132\"/>\
     <circle class=\"r r2\" cx=\"32\" cy=\"32\" r=\"20\" stroke=\"var(--fg)\" \
     stroke-dasharray=\"31 95\"/>\
     <circle class=\"r r3\" cx=\"32\" cy=\"32\" r=\"12\" stroke=\"var(--fg)\" \
     stroke-dasharray=\"19 57\"/></svg>";

const CSS: &str = r#"
html,body{height:100%}
body{margin:0;background:var(--bg);color:var(--fg);
font-family:ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;
display:grid;place-items:center}
/* Held back a beat: a signed-out (or already-warm) launch answers `/` in
   milliseconds, and a spinner that flashes for one frame reads as a glitch.
   A slow launch never notices the delay. */
main{display:grid;justify-items:center;gap:.75rem;padding:2rem;text-align:center;
animation:fade .3s ease .15s both}
.rings{display:block}
.r{transform-box:view-box;transform-origin:50% 50%;animation:spin 1.6s linear infinite}
.r2{animation-duration:1.2s;animation-direction:reverse}
.r3{animation-duration:.9s}
.wordmark{margin:0;font-size:.9375rem;font-weight:600;letter-spacing:-.01em}
.status{margin:0;font-size:.8125rem;color:var(--muted)}
.slow{margin:0;font-size:.8125rem;color:var(--muted);opacity:0;transition:opacity .4s ease}
.slow.on{opacity:1}
@keyframes spin{to{transform:rotate(360deg)}}
@keyframes fade{from{opacity:0}to{opacity:1}}
@media (prefers-reduced-motion:reduce){
.r{animation:none}
main{animation:fade .01s linear .15s both}
.slow{transition:none}}
"#;

/// Hand the webview on to the real page — but only after it has actually put
/// this one on screen. `load` alone is not enough (the document can be parsed
/// and laid out with no frame presented yet); two nested
/// `requestAnimationFrame`s put the navigation strictly after the first paint,
/// which is the whole point of the page.
///
/// **The timer is not belt-and-braces — it is load-bearing.** WebKit stops
/// servicing `requestAnimationFrame` entirely while the view is not on screen,
/// and a launch can easily happen with nothing to paint into: a locked display,
/// another app's full-screen space, a minimized window. Measured on a locked
/// Mac (2026-08-20): the splash was served and the rAF hand-off never fired at
/// all — the app sat on the splash for the full 25s of the run. When rAF is
/// stalled there is no frame to wait for anyway, so the timer takes over; when
/// the window *is* visible rAF wins the race by an order of magnitude and the
/// timer is a no-op behind the `done` latch.
const JS: &str = r#"
(function(){
var done=false;
var go=function(){if(done){return}done=true;location.replace(NEXT_PATH)};
addEventListener('load',function(){requestAnimationFrame(function(){requestAnimationFrame(go)})});
setTimeout(go,HANDOFF_FALLBACK_MS);
setTimeout(function(){var s=document.getElementById('slow');if(s){s.className='slow on'}},SLOW_MS);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn header_map(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        headers
    }

    #[test]
    fn no_cookie_is_the_dark_default() {
        assert!(theme_is_dark(&HeaderMap::new()));
    }

    #[test]
    fn an_explicit_light_override_is_honored() {
        assert!(!theme_is_dark(&header_map("tr_theme=light")));
    }

    #[test]
    fn an_explicit_dark_override_is_dark() {
        assert!(theme_is_dark(&header_map("tr_theme=dark")));
    }

    #[test]
    fn the_theme_cookie_is_found_among_others() {
        assert!(!theme_is_dark(&header_map(
            "tr_jwt=abc; tr_theme=light; tr_session=def"
        )));
    }

    #[test]
    fn unrelated_cookies_leave_the_default_alone() {
        assert!(theme_is_dark(&header_map("tr_jwt=abc; tr_session=def")));
    }

    #[test]
    fn each_theme_paints_its_own_background() {
        assert!(page(true).contains("--bg:#161616"));
        assert!(page(false).contains("--bg:#ffffff"));
    }

    #[test]
    fn the_page_is_self_contained() {
        let html = page(true);
        // Nothing that could cost a second round trip before the first paint.
        assert!(!html.contains("<link"), "no external stylesheet");
        assert!(!html.contains("src="), "no external script or image");
    }

    #[test]
    fn the_hand_off_waits_for_a_painted_frame() {
        let html = page(true);
        assert!(html.contains("location.replace(\"/\")"));
        assert!(
            html.matches("requestAnimationFrame").count() == 2,
            "the redirect must sit behind two nested rAFs, i.e. after first paint"
        );
        assert!(html.contains("addEventListener('load'"));
    }

    #[test]
    fn the_hand_off_still_happens_when_rafs_never_run() {
        // A launch into a locked display or an off-screen space never gets a
        // frame, and WebKit stops servicing rAF there — without this timer the
        // app would sit on the splash forever (observed, 2026-08-20).
        let html = page(true);
        assert!(html.contains(&format!("setTimeout(go,{HANDOFF_FALLBACK_MS})")));
        const { assert!(HANDOFF_FALLBACK_MS < SLOW_AFTER_MS) };
        // …and exactly once, whichever path gets there first.
        assert!(html.contains("if(done){return}done=true"));
        assert_eq!(html.matches("location.replace").count(), 1);
    }

    #[test]
    fn the_slow_notice_is_wired_to_the_timeout() {
        assert!(page(true).contains(&format!("}},{SLOW_AFTER_MS})")));
    }

    #[test]
    fn the_route_mounts_where_the_shell_navigates() {
        // A `Router` has no route introspection, so assert the contract the
        // shell depends on: the path is absolute, namespaced, and answers.
        assert!(PATH.starts_with('/'));
        assert_ne!(PATH, NEXT);
        let _: Router = mount(Router::new());
    }

    #[test]
    fn the_response_is_html_and_uncacheable() {
        let res = response(true);
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }
}
