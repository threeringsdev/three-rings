//! One way to ask the client what its viewport is — a **listened** media query
//! resolved after hydration, never on the server.
//!
//! SSR cannot know the viewport, so the repo's standing rule is that markup is
//! width-agnostic and CSS picks. [`media_signal`] is the one sanctioned
//! exception, and it is not an exception to the rule so much as a statement of
//! it: the signal is `false` during SSR **and** during the hydration render, so
//! the server's markup and the client's first render are identical by
//! construction. Only afterwards, in an `Effect`, does the real width arrive.
//!
//! That makes it safe for exactly one job — deciding whether to **mount** a
//! subtree the server had no business rendering (the ⌘K palette, `/my`'s
//! desktop All-cards table). It is *not* a way to decide what something looks
//! like: `display` stays CSS's, so a subtree gated here still carries its own
//! `md:` classes and the two agree at the same 768 px line.
//!
//! **Listened, not sampled.** One `MediaQueryList` per caller with a `change`
//! handler, so resizing across the breakpoint (or docking a laptop, or rotating
//! a tablet into landscape) takes effect. Sampling once — which
//! `crate::cards::CardPreview` does for `(pointer: coarse)`, a filed discovery
//! — leaves a resized window wrong until the next navigation.

use leptos::prelude::*;

/// The `md:` line the shell's chrome switches on — Tailwind's `md` breakpoint,
/// written out because a media query is a string and Tailwind's is a config
/// value. Anything gated on this must also carry `md:` classes: this decides
/// whether the subtree *exists*, the classes decide whether it *shows*, and
/// they have to name the same width.
pub const MD_UP: &str = "(min-width: 768px)";

/// `true` while the viewport matches `query`. Starts `false` and is corrected in
/// an `Effect` (client-only), then **kept** correct by a `change` listener on
/// the same `MediaQueryList`; the listener is removed on cleanup.
///
/// Returns a plain `false` on the server and in any client without
/// `matchMedia` — the honest answer for "I cannot know", and the one that keeps
/// a `Show` gated on it from rendering server-side.
pub fn media_signal(query: &'static str) -> Signal<bool> {
    let matches = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    {
        use leptos::prelude::LocalStorage;
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::JsCast;

        // Kept for the lifetime of the calling component so the listener can be
        // removed again. `new_local` because neither a `MediaQueryList` nor a
        // `Closure` is `Send` (nor needs to be — this whole block is wasm-only).
        type Watch =
            StoredValue<Option<(web_sys::MediaQueryList, Closure<dyn FnMut()>)>, LocalStorage>;
        let registration: Watch = StoredValue::new_local(None);

        // In an Effect, not the body: setting this synchronously during the
        // hydration render would mount the gated subtree against SSR markup
        // that does not contain it.
        Effect::new(move |_| {
            let Some(mql) = window().match_media(query).ok().flatten() else {
                return;
            };
            matches.set(mql.matches());
            let watched = mql.clone();
            // A `FnMut()` re-reading `matches()`, rather than a handler taking a
            // `MediaQueryListEvent` — that type is not in the crate's web-sys
            // feature set, and the query is the source of truth anyway.
            let handler = Closure::wrap(Box::new(move || {
                matches.set(watched.matches());
            }) as Box<dyn FnMut()>);
            if mql
                .add_event_listener_with_callback("change", handler.as_ref().unchecked_ref())
                .is_ok()
            {
                registration.set_value(Some((mql, handler)));
            }
        });

        on_cleanup(move || {
            if let Some((mql, handler)) = registration.try_update_value(Option::take).flatten() {
                let _ = mql.remove_event_listener_with_callback(
                    "change",
                    handler.as_ref().unchecked_ref(),
                );
            }
        });
    }

    #[cfg(not(feature = "hydrate"))]
    let _ = query;

    matches.into()
}
