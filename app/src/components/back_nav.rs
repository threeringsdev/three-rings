//! Shared "go back" mechanics behind `/cards/:id`'s Back control
//! (design/information-architecture.md → "Card detail: preview + page") and
//! the desktop-shell `⌘[` / `Alt+←` keyboard shortcut. One mechanism behind
//! both, so the button and the shortcut can never disagree about where "back"
//! goes.
//!
//! **Why a shell-tracked counter and not `history.length` or
//! `document.referrer`.** `/cards/:id` is mode-neutral — reachable from the
//! catalog, from My cards, from a collection view, and from the mobile
//! preview sheet's expand affordance — so there is no fixed "parent" screen
//! the way a collection's drill-down breadcrumb has one; "back" has to mean
//! real browser history, with a fallback for when there isn't any.
//! `history.length` is well known to be unreliable for this in a fresh tab
//! (browsers disagree on whether/how an `about:blank` entry counts, so it
//! cannot tell "opened in a new tab" from "navigated here three clicks deep").
//! A same-origin `document.referrer` check was the other option on the table;
//! it was passed over because it only describes how *this* load was reached,
//! which is a different question from "does `history.back()` have anywhere to
//! go" (a mid-session reload has a real history stack behind it but an empty
//! referrer chain of its own reasoning). Instead: `AppShell` (`shell.rs`)
//! mounts once per document load and stays mounted across every client-side
//! route change under it — the route tree nests `/cards/:id` under it the
//! same as every other page — so counting "how many times has the pathname
//! changed since this shell mounted" answers the one question that matters: a
//! client-side navigation *into* this page leaves a real `history.back()`
//! target; a cold load (fresh tab, direct link, or a same-tab reload) does
//! not. **Accepted false negative:** a reload while deep in history resets
//! the counter to 0 even though the browser's own stack is still intact — the
//! Back control just falls back to a fixed destination in that case instead
//! of being wrong in the dangerous direction (stranding the reader, or
//! walking `history.back()` out of the app entirely), and the browser's own
//! Back button/gesture still works regardless.
//!
//! **The fallback destination.** `/my` once the shared `CurrentUserResource`
//! says signed in, `/catalog` otherwise — including while that resource is
//! still pending, which is the same universal-safe default `cards.rs`'s own
//! read-failure states (`NotFound`, `LoadFailed`) already fall back to. Cheap
//! because it rides the resource the shell already fetches for the user menu;
//! nothing here issues a read of its own.
//!
//! **The keyboard shortcut is app-wide, not card-detail-only.** The task that
//! motivated this is the one page with no way out at all (no browser chrome
//! in the Tauri desktop shell), but "browser back" is a single
//! always-available affordance in every real browser, and a coherent desktop
//! app offers the same reach from anywhere rather than one page being special.
//! **No separate desktop-only gate turned out to be needed, but not for the
//! reason originally assumed.** The working theory going in was that a real
//! web browser reserves both chords for its own chrome (`⌘[`/`⌘]` and
//! `Alt+←`/`Alt+→` are the browser's own back/forward shortcuts on mac and
//! elsewhere respectively) and never delivers the keydown to page JS at all —
//! which would make an explicit web/desktop gate moot. **That is unconfirmed
//! for a real, interactively-driven browser window** (out of reach for an
//! automated suite to check), and it is measurably **false** for headless
//! Chromium under Playwright: an instrumented probe against this exact
//! listener showed the keydown reaching `window` and `event.defaultPrevented`
//! flipping to `true` once this handler ran — i.e. nothing in the browser ate
//! it first. `end2end/tests/card-detail.spec.ts`'s `⌘[` and `Alt+←` tests rely
//! on exactly that: the chord genuinely reaches this code and genuinely walks
//! history, in Chromium, on the web build, no Tauri involved. So the accurate
//! claim is narrower than the original one: no gate is needed because running
//! this handler on the web is harmless (worst case, on an interactive
//! browser that *does* reserve the chord, it is simply never invoked) — not
//! because the keydown is guaranteed to be unreachable there.

use leptos::prelude::*;
use leptos_router::location::Location;

use crate::shell::CurrentUserResource;

/// Shell-provided context — see the module doc for what each field means and
/// why it lives here rather than on the page. `Copy` so it can be captured by
/// both the Back control's click handler and the global keydown listener
/// without cloning.
#[derive(Clone, Copy)]
pub struct BackNavigation {
    /// `true` once this shell has observed at least one client-side route
    /// change — i.e. there is a real `history.back()` target.
    pub has_history: Signal<bool>,
    /// Where "Back" lands when there is no in-app history to return to.
    pub fallback_href: Signal<String>,
}

/// Sets up both fields and provides them as context. Called once from
/// `AppShell`, after `provide_current_user()` has already put
/// [`CurrentUserResource`] in context (the fallback reads it).
pub fn provide_back_navigation(location: Location) -> BackNavigation {
    // Skips exactly the first Effect run (the mount itself, not a
    // navigation) — every run after that is a real pathname change.
    let seen_first = RwSignal::new(false);
    let has_history = RwSignal::new(false);
    let pathname = location.pathname;
    Effect::new(move |_| {
        pathname.track();
        if seen_first.get_untracked() {
            has_history.set(true);
        } else {
            seen_first.set(true);
        }
    });

    let user = expect_context::<CurrentUserResource>().0;
    let fallback_href = Signal::derive(move || {
        if matches!(user.get(), Some(Ok(Some(_)))) {
            "/my".to_string()
        } else {
            "/catalog".to_string()
        }
    });

    let nav = BackNavigation {
        has_history: has_history.into(),
        fallback_href,
    };
    provide_context(nav);
    nav
}

/// Real in-app history back — a client-only DOM call, a no-op under any
/// non-hydrate build (SSR render, or before hydration takes over) exactly
/// like `shell::hard_navigate`.
pub fn browser_back() {
    #[cfg(feature = "hydrate")]
    {
        if let Some(w) = web_sys::window() {
            let _ = w.history().and_then(|h| h.back());
        }
    }
}

/// Is this keystroke the desktop back-shortcut? `⌘[` on mac, `Alt+←`
/// elsewhere — each platform's own conventional "browser back" chord,
/// reimplemented here because the Tauri WebView has no browser chrome to
/// supply it. Split out and pure so the platform split is testable the same
/// way `palette::is_palette_chord` is. Every other modifier is required to be
/// absent, same rule as the palette chord: `⌘⌥[` (say) is someone else's
/// shortcut, not a `Shift`-flavoured spelling of this one.
pub fn is_back_chord(key: &str, meta: bool, ctrl: bool, alt: bool, shift: bool, mac: bool) -> bool {
    if mac {
        key == "[" && meta && !ctrl && !alt && !shift
    } else {
        key == "ArrowLeft" && alt && !ctrl && !meta && !shift
    }
}

/// Whether the keyboard focus is somewhere that should own its own keys — the
/// query bar, a stepper's number field, a dialog's text input. Same guard
/// `SlashHint` (`my/collection.rs`) applies for `/`; kept here as its own
/// function (rather than imported from there) because that one is private to
/// a single page's own affordance and this is shell-level, but the check
/// itself must stay identical, so if one changes the other should too.
#[cfg(feature = "hydrate")]
fn focus_is_editable() -> bool {
    use leptos::wasm_bindgen::JsCast;
    document().active_element().is_some_and(|el| {
        let tag = el.tag_name().to_ascii_lowercase();
        tag == "input"
            || tag == "textarea"
            || tag == "select"
            || el
                .dyn_ref::<web_sys::HtmlElement>()
                .is_some_and(|h| h.is_content_editable())
    })
}

/// Installs the app-wide `⌘[` / `Alt+←` listener. Called once from
/// `AppShell`, alongside `provide_back_navigation` — see the module doc's
/// "keyboard shortcut is app-wide" note for why this is shell-level rather
/// than scoped to the card-detail page.
pub fn install_back_shortcut(nav: BackNavigation) {
    #[cfg(feature = "hydrate")]
    {
        let navigate = leptos_router::hooks::use_navigate();
        let mac = super::palette::is_mac();
        let handle = window_event_listener(leptos::ev::keydown, move |ev| {
            if !is_back_chord(
                &ev.key(),
                ev.meta_key(),
                ev.ctrl_key(),
                ev.alt_key(),
                ev.shift_key(),
                mac,
            ) {
                return;
            }
            if focus_is_editable() {
                return;
            }
            ev.prevent_default();
            if nav.has_history.get_untracked() {
                browser_back();
            } else {
                navigate(&nav.fallback_href.get_untracked(), Default::default());
            }
        });
        on_cleanup(move || handle.remove());
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = nav;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chord_is_cmd_bracket_on_mac_and_alt_left_elsewhere() {
        assert!(is_back_chord("[", true, false, false, false, true));
        assert!(is_back_chord("ArrowLeft", false, false, true, false, false));
        // The wrong platform's spelling is not the chord.
        assert!(!is_back_chord("[", true, false, false, false, false));
        assert!(!is_back_chord("ArrowLeft", false, false, true, false, true));
    }

    #[test]
    fn an_extra_modifier_is_someone_elses_chord() {
        assert!(!is_back_chord("[", true, true, false, false, true));
        assert!(!is_back_chord("[", true, false, true, false, true));
        assert!(!is_back_chord("[", true, false, false, true, true));
        assert!(!is_back_chord("ArrowLeft", false, true, true, false, false));
        assert!(!is_back_chord("ArrowLeft", true, false, true, false, false));
        assert!(!is_back_chord("ArrowLeft", false, false, true, true, false));
    }

    #[test]
    fn bare_navigation_keys_are_never_the_chord() {
        assert!(!is_back_chord("[", false, false, false, false, true));
        assert!(!is_back_chord(
            "ArrowLeft",
            false,
            false,
            false,
            false,
            false
        ));
    }
}
