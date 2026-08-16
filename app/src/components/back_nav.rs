//! Shared "go back" mechanics behind `/cards/:id`'s Back control
//! (design/information-architecture.md → "Card detail: preview + page") and
//! the desktop-shell `⌘[` / `Alt+←` keyboard shortcut. One mechanism behind
//! both, so the button and the shortcut can never disagree about where "back"
//! goes.
//!
//! **Round 2 rewrite.** The first version counted client-side route changes
//! since `AppShell` mounted, treating "seen at least one navigation" as
//! "there is a `history.back()` target". Adversarial review caught the
//! blocker this hides: the counter re-arms on the navigation `browser_back()`
//! itself causes (a popstate is still a navigation), so after
//! `/catalog → /cards/1 → back` the counter is `true` again on `/catalog`,
//! and a **second** back press calls `history.back()` with nothing real
//! behind it — on the web that leaves the search results one entry short (an
//! external referrer, say); on desktop, with no address bar, it can walk the
//! reader out of the app entirely. What follows is the fix: history-entry
//! stamping, not a shell-lifetime count.
//!
//! **Why `history.state`, not a Leptos `Effect` on the router's URL.** The
//! obvious fix looked like: track `location.pathname`/`.search` in an
//! `Effect`, and stamp `history.state` from inside it. That does not work
//! with `leptos_router` 0.8's real navigation timing, and this was checked
//! against the crate source (`leptos_router-0.8.10/src/location/history.rs`,
//! `flat_router.rs`), not assumed. `BrowserUrl::init`'s `navigate` closure
//! updates the reactive `url` signal *before* it decides whether to call the
//! browser's `pushState`/`replaceState` — for a same-pathname change (a
//! query-only edit) it does so synchronously right after, but for a real
//! pathname change (the common case: catalog → card) the actual
//! `history.pushState` call is deferred behind `ready_to_complete()`, called
//! from `flat_router.rs` only *after* `view.choose().await` resolves the new
//! route. An `Effect` reacting to the signal update fires on the *first* half
//! of that (the signal write), which can run before the *second* half (the
//! real browser call) — so a stamp attempted from the Effect can land on the
//! entry being left, or race the entry being created, and the marker never
//! reliably reaches the right one. This is the "if the router's state
//! handling makes stamping unworkable, say so" case: it does, for an
//! Effect-driven stamp specifically.
//!
//! **What is shipped instead: `history.pushState`/`replaceState` themselves
//! are wrapped**, once, in [`install_history_stamping`]. This sidesteps the
//! timing question entirely — the stamp happens *inside* the exact call that
//! creates or overwrites the entry, synchronously, with zero gap for
//! anything to race. web-sys's `History::push_state_with_url` compiles to a
//! plain `history.pushState(...)` JS call on the one shared `window.history`
//! object; reassigning that object's own `pushState`/`replaceState`
//! properties is seen by every caller of them, including `leptos_router`'s —
//! JS resolves a method call against the object at call time, not at
//! wasm-bindgen's codegen time. Concretely:
//!
//! - **A push always means "there is now a prior entry"** — even the very
//!   first push in a tab happens *from* an already-sealed entry (see below).
//!   So the wrapped `pushState` always stamps the new entry `true`.
//! - **A replace keeps the current entry's own marker.** `history.state` at
//!   the moment the wrapped `replaceState` runs is still the pre-replace
//!   entry's (the browser hasn't overwritten it yet), so reading it there and
//!   carrying it forward is exactly "this position in history didn't change
//!   depth" — which is the correct read of a replace, not a fresh policy
//!   invented for this file. It is the same rule `catalog/rail.rs` and
//!   `components/query_bar.rs` already apply to *when* they push vs. replace
//!   ("History granularity is per search session": the first filter on a
//!   bare page pushes, refining an existing query replaces) — this module
//!   just has to agree with it, not decide it.
//! - **The entry already current when this file first installs the wrapper**
//!   (a fresh tab's first load, or a reload) is sealed `false` if it carries
//!   no marker yet, and left untouched if it already does — a reload
//!   mid-history lands back on an entry a *previous* document already
//!   stamped, and `history.state` is preserved across a reload by the
//!   browser itself, so this is no longer the false negative the first
//!   version had to accept. (**Correction, not a new finding**: the original
//!   "accepted false negative" for a same-tab reload no longer applies under
//!   this design — recorded in specs/app-ui.md so nobody re-reads the old
//!   note as still true.)
//!
//! **The upshot for query-only navigations, decided rather than left open**
//! (the fold this round's review asked for): a `?q=` edit that *pushes* (the
//! first filter on a bare page) is a real new back-target and now gets its
//! own marker like any other push: `has_history` correctly reads `true` once
//! the reader has performed one. A `?q=` edit that *replaces* (refining an
//! existing search) is not a new position and does not advance the marker —
//! matching the granularity the app already committed to for those two
//! surfaces. Tracking pathname alone (what the first version actually did,
//! silently) would have missed the first case; this version does not,
//! because it stamps at the real DOM call, which fires for both.
//!
//! **`has_history` is a plain function, not a reactive signal**, on purpose:
//! it is read exactly twice, once inside a click handler and once inside a
//! keydown handler — both already event-driven, never rendered — so there is
//! nothing for reactivity to buy here, and a signal would invite exactly the
//! "when did this last update" question this rewrite exists to answer
//! honestly. It reads `history.state` fresh, synchronously, every call.
//!
//! **The fallback destination.** Unchanged from the first version: `/my`
//! once the shared `CurrentUserResource` says signed in, `/catalog`
//! otherwise — including while that resource is still pending, the same
//! universal-safe default `cards.rs`'s own read-failure states (`NotFound`,
//! `LoadFailed`) already fall back to.
//!
//! **The keyboard shortcut is app-wide, not card-detail-only** — unchanged
//! from round 1: the task that motivated this is the one page with no way
//! out at all (no browser chrome in the Tauri desktop shell), but "browser
//! back" is a single always-available affordance in every real browser, and a
//! coherent desktop app offers the same reach from anywhere rather than one
//! page being special. **No separate desktop-only gate turned out to be
//! needed, but not for the reason originally assumed** — also unchanged, and
//! worth restating since it is easy to misremember as "browsers never
//! deliver this keydown to page JS": the working theory going in was that a
//! real web browser reserves both chords for its own chrome (`⌘[`/`⌘]` and
//! `Alt+←`/`Alt+→` are the browser's own back/forward shortcuts on mac and
//! elsewhere respectively) and never delivers the keydown to page JS at all.
//! **That is unconfirmed for a real, interactively-driven browser window**
//! (out of reach for an automated suite to check), and it is measurably
//! **false** for headless Chromium under Playwright: an instrumented probe
//! against this exact listener showed the keydown reaching `window` with
//! `event.defaultPrevented` flipping `true` once this handler ran — nothing
//! in the browser ate it first. `end2end/tests/card-detail.spec.ts`'s `⌘[`
//! and `Alt+←` tests rely on exactly that: the chord genuinely reaches this
//! code and genuinely walks history, in Chromium, on the web build, no Tauri
//! involved. So the accurate claim stays narrower than the original one: no
//! gate is needed because running the handler on the web is harmless (worst
//! case, on an interactive browser that does reserve the chord, it is simply
//! never invoked) — not because the keydown is guaranteed unreachable there.
//!
//! Two guards were added this round, both from adversarial review:
//!
//! - **It defers to a handler that already claimed the key.**
//!   `components::view_switch::ViewSwitch`'s roving-focus arrows matched
//!   `ev.key()` alone (no modifier check) and called `prevent_default()` but
//!   not `stop_propagation()` — so a focused view switch on non-mac turned
//!   one `Alt+←` into *both* "flip to grid" and "navigate back". Fixed at
//!   both ends: `ViewSwitch` now ignores any arrow carrying
//!   Alt/Meta/Ctrl (a modified arrow is never a roving-focus move — see its
//!   own module for the reasoning), and this listener separately checks
//!   `ev.default_prevented()` before its own chord match, so it stays out of
//!   the way of *any* component that claims a key this way, not just the one
//!   bug that was actually found.
//! - **It defers to an open overlay**, the same way `⌘K` does
//!   (`palette::palette_chord_target`'s "swallow the chord, change nothing"
//!   arm): gated on `components::ui::overlay_stack::is_empty()`, so the
//!   shortcut over an open `Dialog`/`Sheet`/`Popover` does nothing at all —
//!   it does not close the overlay (that is Escape's job) and it does not
//!   navigate the page underneath it. Same known gap the palette's own
//!   module doc records and this one inherits rather than re-solves: the
//!   quick-add panel is deliberately not built on `Dialog`/`Popover` (its own
//!   module doc explains why) and is invisible to this stack, so the
//!   shortcut is not gated against it specifically — a narrower, pre-existing
//!   gap, not one this file introduces.
//!
//! **`focus_is_editable` and `my/collection.rs`'s `SlashHint`.** These used
//! to be two independent copies of the same check. They are now one function
//! (`pub(crate)` here), because unlike `is_back_chord` vs.
//! `palette::is_palette_chord` — which share a *shape* (a pure, platform-
//! aware chord predicate) but not a line of actual logic, since the chords
//! themselves differ — `focus_is_editable` and `SlashHint`'s old inline check
//! were byte-for-byte the same predicate for a genuinely shared question ("is
//! the keyboard focus somewhere that owns its own keys"), which is exactly
//! the case where leaving two copies to drift was not worth it.

use leptos::prelude::*;

use crate::shell::CurrentUserResource;

/// Shell-provided context — see the module doc for why `fallback_href` is the
/// only field left (the first version's `has_history: Signal<bool>` is now
/// the free function [`has_history`]). `Copy` so it can be captured by both
/// the Back control's click handler and the global keydown listener without
/// cloning.
#[derive(Clone, Copy)]
pub struct BackNavigation {
    /// Where "Back" lands when there is no in-app history to return to.
    pub fallback_href: Signal<String>,
}

/// Installs the history-entry stamping (see the module doc) and provides
/// [`BackNavigation`]. Called once from `AppShell`, after
/// `provide_current_user()` has already put [`CurrentUserResource`] in
/// context (the fallback reads it).
pub fn provide_back_navigation() -> BackNavigation {
    #[cfg(feature = "hydrate")]
    install_history_stamping();

    let user = expect_context::<CurrentUserResource>().0;
    let fallback_href = Signal::derive(move || {
        if matches!(user.get(), Some(Ok(Some(_)))) {
            "/my".to_string()
        } else {
            "/catalog".to_string()
        }
    });

    let nav = BackNavigation { fallback_href };
    provide_context(nav);
    nav
}

/// Is the *current* history entry marked as having a real predecessor? Reads
/// `history.state` fresh, synchronously — see the module doc's "plain
/// function, not a reactive signal" note. `false` (never strand, never walk
/// out) whenever hydration hasn't taken over yet, or in any non-hydrate
/// build.
pub fn has_history() -> bool {
    #[cfg(feature = "hydrate")]
    {
        current_marker().unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        false
    }
}

#[cfg(feature = "hydrate")]
const HISTORY_MARKER_KEY: &str = "trHasHistory";

/// A second key, on `window` itself rather than on any one history entry —
/// guards [`install_history_stamping`] against installing twice (a dev-mode
/// hot-reload remount, most plausibly; a real page load only ever calls this
/// once regardless, since SSR never runs `hydrate`-gated code). Re-wrapping
/// an already-wrapped `pushState` would still behave correctly (the outer
/// wrapper's "orig" is the inner wrapper, which itself delegates truthfully
/// to the real native call) — this just avoids the pointless extra
/// indirection.
#[cfg(feature = "hydrate")]
const PATCH_GUARD_KEY: &str = "__trBackNavPatched";

#[cfg(feature = "hydrate")]
fn current_marker() -> Option<bool> {
    let state = web_sys::window()?.history().ok()?.state().ok()?;
    marker_of(&state)
}

/// `None` when `state` carries no marker at all (never stamped — the
/// question `install_history_stamping`'s initial seal exists to answer once
/// and for all for the entry a document lands on); `Some(_)` is the stamped
/// value, whatever it is.
#[cfg(feature = "hydrate")]
fn marker_of(state: &wasm_bindgen::JsValue) -> Option<bool> {
    if !state.is_object() {
        return None;
    }
    let key = wasm_bindgen::JsValue::from_str(HISTORY_MARKER_KEY);
    let present = js_sys::Reflect::has(state, &key).unwrap_or(false);
    if !present {
        return None;
    }
    js_sys::Reflect::get(state, &key)
        .ok()
        .map(|v| v.is_truthy())
}

/// `state` with the marker set, merged onto (not replacing) whatever `state`
/// already carried — `leptos_router` never sets a custom `state` on a plain
/// `navigate()` call today (every `NavigateOptions` in this app leaves it at
/// its `State::default()`, which serializes to `undefined`), but this stays
/// merge-not-clobber regardless, so a future caller that *does* pass state
/// through doesn't lose it to this file.
#[cfg(feature = "hydrate")]
fn stamped(state: &wasm_bindgen::JsValue, marker: bool) -> wasm_bindgen::JsValue {
    use wasm_bindgen::JsCast;
    let target = js_sys::Object::new();
    if state.is_object() {
        js_sys::Object::assign(&target, state.unchecked_ref());
    }
    let _ = js_sys::Reflect::set(
        &target,
        &wasm_bindgen::JsValue::from_str(HISTORY_MARKER_KEY),
        &wasm_bindgen::JsValue::from_bool(marker),
    );
    target.into()
}

#[cfg(feature = "hydrate")]
fn get_function(obj: &wasm_bindgen::JsValue, name: &str) -> Option<js_sys::Function> {
    use wasm_bindgen::JsCast;
    js_sys::Reflect::get(obj, &wasm_bindgen::JsValue::from_str(name))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
}

/// Wraps `window.history.pushState`/`replaceState` and seals whatever entry
/// is already current — see the module doc for the full mechanism and why an
/// `Effect` on the router's own URL signal cannot do this reliably instead.
#[cfg(feature = "hydrate")]
fn install_history_stamping() {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsValue;

    let Some(window) = web_sys::window() else {
        return;
    };
    let guard_key = JsValue::from_str(PATCH_GUARD_KEY);
    if js_sys::Reflect::has(&window, &guard_key).unwrap_or(false) {
        return;
    }
    let _ = js_sys::Reflect::set(&window, &guard_key, &JsValue::TRUE);

    let Ok(history) = window.history() else {
        return;
    };

    // Seal the entry already current *before* patching anything, so this
    // call goes through the real, unwrapped `replaceState` — not the one
    // installed a few lines down.
    if let Ok(current) = history.state() {
        if marker_of(&current).is_none() {
            let sealed = stamped(&current, false);
            let _ = history.replace_state_with_url(&sealed, "", None);
        }
    }

    let history_obj: JsValue = history.clone().into();

    if let Some(orig) = get_function(&history_obj, "pushState") {
        let this = history_obj.clone();
        let wrapped = Closure::wrap(
            Box::new(move |state: JsValue, title: JsValue, url: JsValue| {
                let merged = stamped(&state, true);
                let _ = orig.call3(&this, &merged, &title, &url);
            }) as Box<dyn FnMut(JsValue, JsValue, JsValue)>,
        );
        let _ = js_sys::Reflect::set(
            &history_obj,
            &JsValue::from_str("pushState"),
            wrapped.as_ref(),
        );
        wrapped.forget();
    }

    if let Some(orig) = get_function(&history_obj, "replaceState") {
        let this = history_obj.clone();
        let wrapped = Closure::wrap(
            Box::new(move |state: JsValue, title: JsValue, url: JsValue| {
                let carried = current_marker().unwrap_or(false);
                let merged = stamped(&state, carried);
                let _ = orig.call3(&this, &merged, &title, &url);
            }) as Box<dyn FnMut(JsValue, JsValue, JsValue)>,
        );
        let _ = js_sys::Reflect::set(
            &history_obj,
            &JsValue::from_str("replaceState"),
            wrapped.as_ref(),
        );
        wrapped.forget();
    }
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
/// query bar, a stepper's number field, a dialog's text input. Shared with
/// `my/collection.rs`'s `SlashHint` (see the module doc's last paragraph for
/// why this one stopped being two copies).
#[cfg(feature = "hydrate")]
pub(crate) fn focus_is_editable() -> bool {
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
            // Defer to whatever already claimed this keystroke — see the
            // module doc's `ViewSwitch` finding. Checked before our own
            // chord match so this stays correct for *any* component that
            // claims a key this way, not only the one bug found this round.
            if ev.default_prevented() {
                return;
            }
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
            // Same semantics as ⌘K over an already-open overlay
            // (`palette::palette_chord_target`'s "swallow the chord, change
            // nothing" arm): claim the keystroke, but do not act on it while
            // a Dialog/Sheet/Popover is up — it does not close the overlay
            // (Escape's job) and it does not navigate the page underneath.
            if !super::ui::overlay_stack::is_empty() {
                return;
            }
            if has_history() {
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
