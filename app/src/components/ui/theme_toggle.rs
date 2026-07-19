//! ThemeToggle — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/theme_toggle.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - no `icons` crate: the two SVG paths are inlined verbatim
//! - no `use_theme_mode` hook: theme state is app-owned (specs/app-ui.md →
//!   theme persistence): dark is the DEFAULT; the toggle flips the `dark`
//!   class on `<html>` and persists the override in the `tr_theme` cookie
//!   (`light`|`dark`, 1 year, SameSite=Lax) that `shell()` re-applies on SSR
//! - initial state reads `<html>`'s class after hydration (SSR renders the
//!   cookie-derived class, so client and server agree)

use leptos::prelude::*;

/// The theme-override cookie. `light` is the only meaningful override today
/// (absence = the dark default); `dark` is written so an explicit choice
/// survives a future default flip.
pub const THEME_COOKIE: &str = "tr_theme";

/// `tr_theme` parsed from wherever the current side keeps cookies: the
/// request `Parts` in context on the server, `document.cookie` in the wasm.
/// Absent or any non-`light` value = the dark default.
pub fn cookie_theme_is_dark() -> bool {
    fn parse(cookies: &str) -> Option<bool> {
        let prefix = format!("{THEME_COOKIE}=");
        cookies
            .split(';')
            .find_map(|pair| pair.trim().strip_prefix(&prefix).map(|v| v != "light"))
    }
    #[cfg(feature = "ssr")]
    {
        if let Some(parts) = use_context::<http::request::Parts>() {
            for header in parts.headers.get_all(http::header::COOKIE) {
                if let Some(dark) = header.to_str().ok().and_then(parse) {
                    return dark;
                }
            }
        }
    }
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(dark) = leptos::tachys::dom::document()
            .dyn_ref::<web_sys::HtmlDocument>()
            .and_then(|d| d.cookie().ok())
            .as_deref()
            .and_then(parse)
        {
            return dark;
        }
    }
    true
}

#[component]
pub fn ThemeToggle() -> impl IntoView {
    // Initialized from the tr_theme cookie on BOTH sides (Parts on the
    // server, document.cookie in the wasm — the cookie is deliberately not
    // httpOnly), so SSR markup, the hydrating client render, and the <html>
    // class the shell stamped all agree from the first frame: no icon flash,
    // no corrective effect needed.
    let (dark, set_dark) = signal(cookie_theme_is_dark());

    let toggle = move |_| {
        let now = !dark.get_untracked();
        set_dark.set(now);
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::JsCast;
            let doc = leptos::tachys::dom::document();
            if let Some(root) = doc.document_element() {
                let _ = if now {
                    root.class_list().add_1("dark")
                } else {
                    root.class_list().remove_1("dark")
                };
            }
            if let Some(html_doc) = doc.dyn_ref::<web_sys::HtmlDocument>() {
                let value = if now { "dark" } else { "light" };
                let _ = html_doc.set_cookie(&format!(
                    "{THEME_COOKIE}={value}; Path=/; Max-Age=31536000; SameSite=Lax"
                ));
            }
        }
    };

    view! {
        <style>
            {"
            .theme__toggle_transition {
            -webkit-tap-highlight-color: transparent;

            svg path {
            transform-origin: center;
            transition: all .6s ease;
            transform: translate3d(0,0,0);
            backface-visibility: hidden;

            &.sun {
            transform: scale(.4) rotate(60deg);
            opacity: 0;
            }

            &.moon {
            opacity: 1;
            }
            }

            &.switch {
            svg path {
            &.sun {
            transform: scale(1) rotate(0);
            opacity: 1;
            }

            &.moon {
            transform: scale(.4) rotate(-60deg);
            opacity: 0;
            }
            }
            }
            }
            "}
        </style>

        <button
            type="button"
            aria-label="Toggle theme"
            class=move || {
                // "switch" shows the sun (light mode active), per upstream.
                if dark.get() {
                    "theme__toggle_transition".to_string()
                } else {
                    "theme__toggle_transition switch".to_string()
                }
            }
            on:click=toggle
        >
            <svg
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
                stroke-width="1.5"
                stroke="currentColor"
                class="size-4"
            >
                <path
                    class="sun"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M12 1.75V3.25M12 20.75V22.25M1.75 12H3.25M20.75 12H22.25M4.75216 4.75216L5.81282 5.81282M18.1872 18.1872L19.2478 19.2478M4.75216 19.2478L5.81282 18.1872M18.1872 5.81282L19.2478 4.75216M16.25 12C16.25 14.3472 14.3472 16.25 12 16.25C9.65279 16.25 7.75 14.3472 7.75 12C7.75 9.65279 9.65279 7.75 12 7.75C14.3472 7.75 16.25 9.65279 16.25 12Z"
                />
                <path
                    class="moon"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M21.25 15.7499C20.0046 16.2903 18.6462 16.5 17.25 16.5C11.8652 16.5 7.5 12.1348 7.5 6.75C7.5 5.35383 7.70969 3.99536 8.25 2.74991C4.75911 4.24145 2.75 7.68308 2.75 11.5C2.75 16.7467 7.00329 20.9999 12.25 20.9999C16.0669 20.9999 19.7585 19.2408 21.25 15.7499Z"
                />
            </svg>
        </button>
    }
}
