//! Popover — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/popover.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Native Popover API + CSS anchor positioning kept
//! (the point of the component); behavior rewired. Deviations from upstream:
//! - **deterministic caller-supplied `id`** replaces `use_random_id` (SSR
//!   counter hydration bug); the anchor name derives from it
//! - the inline `<script>` (close-on-CommandItem-click) is gone — feature
//!   compositions close via [`use_popover_open`] in Leptos
//! - optional **`open` signal**: synced to the native popover both ways
//!   (`showPopover`/`hidePopover` on signal change, `toggle` events back
//!   into the signal), so pickers can be driven programmatically
//! - CSS anchor positioning verified on the Android webview (Chrome 145);
//!   webkit rides the boundary tier — fallback decision recorded in
//!   app-ui Findings
//! - **registers with [`super::overlay_stack`]** (Adversarial review,
//!   `P6-189`): without this, a popover opened *inside* a `Dialog` (the
//!   delete confirm's two disposition pickers) was invisible to the app's own
//!   overlay bookkeeping, so `Dialog`'s window-level Escape listener still
//!   believed itself topmost and closed the whole dialog underneath the
//!   popover on the same keypress. `PopoverContent` now pushes/removes its
//!   own id the same way `DialogContent` does, and owns its own Escape
//!   listener gated on `overlay_stack::is_top` — consuming the keypress
//!   (`stop_immediate_propagation`) so a `Dialog` further down the stack
//!   never sees it, exactly `DialogContent`'s own multi-overlay reasoning.
//!   A **second** Escape press then closes the dialog, since the popover has
//!   dropped off the stack by then.
//! - **no `relative` on the panel** (the selection tray's "Move to…" opening
//!   off the bottom of the window bug): a top-layer element's used `position`
//!   is `absolute`, not the UA-default `fixed`, whenever the author sets any
//!   non-`static` `position` — so the leftover `relative` this component
//!   copied from the upstream (non-native-popover) registry source silently
//!   swapped the panel's containing block from the viewport to the page,
//!   corrupting every `anchor()` offset on a page taller than the viewport.
//!   See `PopoverContent`'s own comment for the mechanism.
//! - **the JS positioning fallback now watches the panel's own size**
//!   (`watch_panel_resize`/`ResizeObserver`), not just the trigger's, so a
//!   panel whose rows arrive after a `Resource` resolves (every
//!   `DestinationList` consumer) does not stay pinned to the flip decision
//!   made for its much-shorter "Loading…" placeholder as it grows underneath
//!   that stale position — the second contributor to the same bug, on an
//!   engine without CSS anchor positioning.

use leptos::prelude::*;
use tw_merge::tw_merge;

use super::clx::clx;

mod components {
    use super::*;
    clx! {PopoverTitle, h3, "leading-none font-medium", "mb-3"}
    clx! {PopoverDescription, p, "text-muted-foreground text-sm"}
}

pub use components::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PopoverAlign {
    Start,
    StartOuter,
    End,
    EndOuter,
    #[default]
    Center,
}

#[derive(Clone)]
struct PopoverContext {
    anchor_name: String,
    target_id: String,
    open: RwSignal<bool>,
}

/// The popover's open signal, for closing from composed content (e.g. a
/// destination picker's item click).
pub fn use_popover_open() -> Option<RwSignal<bool>> {
    use_context::<PopoverContext>().map(|c| c.open)
}

#[component]
pub fn Popover(
    /// Deterministic instance id — SSR and hydration must agree on it.
    #[prop(into)]
    id: String,
    /// Share the open state with the caller; omitted = internal state.
    #[prop(optional)]
    open: Option<RwSignal<bool>>,
    #[prop(default = PopoverAlign::default())] align: PopoverAlign,
    children: Children,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let popover_anchor_name = format!("--anchor-{id}");
    let popover_target_id = format!("popover-{id}");

    let (position_styles, transform_origin) = match align {
        PopoverAlign::Start => (
            "left: anchor(left);
                bottom: anchor(top);
                margin-bottom: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                bottom: auto;
                margin-top: 8px;
                margin-bottom: 0;
                }",
            "left top",
        ),
        PopoverAlign::StartOuter => (
            "right: anchor(left);
                top: anchor(top);
                margin-right: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                margin-top: 8px;
                }",
            "right top",
        ),
        PopoverAlign::End => (
            "right: anchor(right);
                bottom: anchor(top);
                margin-bottom: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                bottom: auto;
                margin-top: 8px;
                margin-bottom: 0;
                }",
            "right top",
        ),
        PopoverAlign::EndOuter => (
            "left: anchor(right);
                top: anchor(top);
                margin-left: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                margin-top: 8px;
                }",
            "left top",
        ),
        PopoverAlign::Center => ("position-area: block-start;", "center top"),
    };

    let ctx = PopoverContext {
        anchor_name: popover_anchor_name.clone(),
        target_id: popover_target_id.clone(),
        open,
    };

    view! {
        <leptos::context::Provider value=ctx>
            <style>
                {format!(
                    "
                #{popover_target_id} {{
                position-anchor: {popover_anchor_name};
                inset: auto;
                {position_styles}
                position-try-fallbacks: flip-block;
                position-try-order: most-height;
                position-visibility: anchors-visible;

                /* Open State */
                &:popover-open {{
                opacity: 1;
                transform: scale(1) translateY(0px);

                @starting-style {{
                opacity: 0;
                transform: scale(0.95) translateY(-2px);
                }}
                }}

                /* Closed State */
                & {{
                transition:
                display 0.2s allow-discrete,
                overlay 0.2s allow-discrete,
                transform 0.15s cubic-bezier(0.16, 1, 0.3, 1),
                opacity 0.15s ease-out;
                opacity: 0;
                transform: scale(0.95) translateY(-2px);
                transform-origin: var(--popover-transform-origin, {transform_origin});
                }}
                }}
                ",
                )}
            </style>

            <div data-name="Popover">{children()}</div>
        </leptos::context::Provider>
    }
}

#[component]
pub fn PopoverTrigger(children: Children, #[prop(optional, into)] class: String) -> impl IntoView {
    let ctx = expect_context::<PopoverContext>();
    let button_class = tw_merge!(
        "px-4 py-2 h-9 inline-flex justify-center items-center text-sm font-medium whitespace-nowrap rounded-md transition-colors w-fit focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&_svg:not(:last-child)]:mr-2 [&_svg:not(:first-child)]:ml-2 border bg-background border-input hover:bg-accent hover:text-accent-foreground",
        class
    );

    view! {
        <button
            class=button_class
            style=format!("anchor-name: {}", ctx.anchor_name)
            popovertarget=ctx.target_id
            tabindex="0"
            type="button"
            data-name="PopoverTrigger"
        >
            {children()}
        </button>
    }
}

/// The [`ResizeObserver`](web_sys::ResizeObserver) + its callback `Closure`,
/// kept alive across opens — see [`watch_panel_resize`] and
/// [`stop_watching_resize`], the two functions that hold one of these.
#[cfg(feature = "hydrate")]
type ResizeWatch = StoredValue<
    Option<(
        web_sys::ResizeObserver,
        leptos::wasm_bindgen::closure::Closure<dyn FnMut()>,
    )>,
    leptos::prelude::LocalStorage,
>;

#[component]
pub fn PopoverContent(children: Children, #[prop(optional, into)] class: String) -> impl IntoView {
    let ctx = expect_context::<PopoverContext>();
    let open = ctx.open;
    // **No `relative` here — that's a bug fix, not a stylistic call.** The
    // upstream registry source positions its panel with a JS library (portal +
    // `position: absolute`), where a `relative` ancestor made sense. This panel
    // is a native `popover="auto"` element promoted to the top layer, where
    // `position: fixed` is the UA default and the mechanism CSS anchor
    // positioning assumes. Per the CSS Position spec, a top-layer element's used
    // `position` is `fixed` only when its *computed* value is `static` —
    // anything else (author-set `relative`, as here) resolves to `absolute`
    // instead. `absolute`'s containing block is the nearest positioned ancestor
    // (or the ICB sized to the whole *document*, not the viewport, when there is
    // none) rather than the viewport `fixed` gets — so every `anchor()` offset
    // above was computed against document height instead of viewport height.
    // On a long page (the `/my` list behind the selection tray) that is
    // thousands of pixels taller than the viewport, which is exactly what
    // parked the tray's "Move to…" panel off the bottom of the window with no
    // way to scroll to it (verified: forcing `position: fixed` here by deleting
    // this class in devtools puts the panel back in-viewport; specs/app-ui.md
    // Findings).
    let class = tw_merge!(
        "overflow-visible z-50 p-4 rounded-md border bg-popover text-popover-foreground shadow-md my-[1ch] w-[250px]",
        class
    );

    let node_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let target_id = ctx.target_id.clone();

    // Re-runs `position_below_trigger` whenever the panel's own size changes
    // while it is open on a no-anchor-positioning engine — see that function's
    // caller below for why this exists. `new_local`, exactly `viewport.rs`'s
    // `media_signal` watch: neither a `ResizeObserver` nor a `Closure` is
    // `Send`, and both need to outlive one Effect run to be reused across
    // opens instead of leaking a new observer per open.
    #[cfg(feature = "hydrate")]
    let resize_watch: ResizeWatch = StoredValue::new_local(None);

    // Two-way sync with the native popover: signal → showPopover/hidePopover
    // (Effects only run client-side, so no cfg gate), native toggle events
    // (light-dismiss, the popovertarget trigger) → signal. DOM types come
    // through leptos's own web_sys re-export — available in every build.
    Effect::new(move |_| {
        let want_open = open.get();
        if let Some(el) = node_ref.get() {
            let is_open = el.matches(":popover-open").unwrap_or(false);
            if want_open && !is_open {
                if el.show_popover().is_err() {
                    // Keep the signal honest if the native call is rejected
                    // (e.g. an ancestor with `display:none`).
                    open.set(el.matches(":popover-open").unwrap_or(false));
                }
            } else if !want_open && is_open {
                let _ = el.hide_popover();
            }
            // JS positioning fallback (spec: "JS fallback if unsupported"):
            // WebKit ships the Popover API but NOT CSS anchor positioning, so
            // the panel would open at the viewport default. When anchors are
            // unsupported and we're open, position manually under the trigger
            // (flipping above if it would overflow the viewport bottom).
            // Hydrate-only: the DOM measurement APIs and the mispositioning
            // it corrects both exist only client-side.
            //
            // **Also kept in sync with a `ResizeObserver`, not just this one
            // call.** `position_below_trigger` measures the panel's *current*
            // height to decide whether to flip above the trigger — but this
            // Effect fires once per `open` toggle, right when `show_popover()`
            // runs, which on a picker whose rows come from a `Resource` (every
            // `DestinationList` consumer: the catalog toolbar, the tray's own
            // "Move to…") is *before* the real rows have arrived. The first
            // measurement catches the much shorter "Loading collections…"
            // placeholder, pins `top` to a flip decision made for that height,
            // and then never revisits it as the real rows land and the panel
            // grows underneath that stale `top` — parking the now-taller
            // bottom edge off the target device's screen with no way to
            // scroll to it. A fixed delay cannot promise the fetch has landed
            // by then; a `ResizeObserver` reacts to the panel's real size
            // settling, however long that takes.
            #[cfg(feature = "hydrate")]
            {
                if want_open && !anchor_positioning_supported() {
                    position_below_trigger(&el, &target_id);
                    watch_panel_resize(&el, &target_id, resize_watch);
                } else {
                    stop_watching_resize(resize_watch);
                }
            }
            #[cfg(not(feature = "hydrate"))]
            let _ = &target_id;
        }
    });

    // Overlay-stack bookkeeping (see the module doc): push while open, so a
    // `Dialog` further down the visual stack can tell it is no longer
    // topmost. `target_id` is already the deterministic per-instance id
    // (`popover-{id}`), so it doubles as the stack key.
    let stack_id = ctx.target_id.clone();
    Effect::new(move |prev: Option<bool>| {
        let now = open.get();
        if now {
            super::overlay_stack::push(&stack_id);
        } else if prev == Some(true) {
            super::overlay_stack::remove(&stack_id);
        }
        now
    });

    // Own Escape listener, gated the same way `DialogContent`'s is: only the
    // topmost overlay reacts, and it consumes the keypress so nothing further
    // down the stack (a `Dialog` wrapping this popover, most notably) also
    // sees it. Native `popover="auto"` closes on Escape by default too, but
    // relying on that alone raced this listener's stack bookkeeping and a
    // `Dialog`'s separate window listener with no defined order between them
    // — `prevent_default` here suppresses the native default action, making
    // this the sole authority, exactly `DialogContent`'s own model.
    let esc_id = ctx.target_id.clone();
    let esc = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && open.get_untracked() && super::overlay_stack::is_top(&esc_id) {
            ev.prevent_default();
            ev.stop_immediate_propagation();
            open.set(false);
        }
    });
    let unmount_id = ctx.target_id.clone();
    on_cleanup(move || {
        esc.remove();
        if open.get_untracked() {
            super::overlay_stack::remove(&unmount_id);
        }
        // Disconnects (does not need to drop) the `ResizeObserver` set up
        // above, same as the two lines above it clean up their own watches —
        // a no-op if anchor positioning was supported and none was ever
        // created.
        #[cfg(feature = "hydrate")]
        stop_watching_resize(resize_watch);
    });

    view! {
        <div
            class=class
            id=ctx.target_id.clone()
            popover="auto"
            data-name="PopoverContent"
            node_ref=node_ref
            on:toggle=move |_| {
                if let Some(el) = node_ref.get_untracked() {
                    open.set(el.matches(":popover-open").unwrap_or(false));
                }
            }
        >
            {children()}
        </div>
    }
}

/// Whether the engine supports CSS anchor positioning. Chromium (incl. the
/// Android webview) yes; WebKit not yet — there we position manually.
#[cfg(feature = "hydrate")]
fn anchor_positioning_supported() -> bool {
    web_sys::css::supports("position-anchor: --x").unwrap_or(false)
}

/// JS positioning fallback: fixed-position the panel just below its trigger,
/// flipping above when it would overflow the viewport bottom. Only used when
/// CSS anchor positioning is unavailable.
#[cfg(feature = "hydrate")]
fn position_below_trigger(panel: &web_sys::HtmlElement, target_id: &str) {
    use leptos::wasm_bindgen::JsCast;
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(trigger) = doc
        .query_selector(&format!("[popovertarget=\"{target_id}\"]"))
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return;
    };
    let t = trigger.get_bounding_client_rect();
    let p = panel.get_bounding_client_rect();
    let viewport_h = web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let below = t.bottom() + 8.0;
    let top = if below + p.height() > viewport_h && t.top() - 8.0 - p.height() > 0.0 {
        t.top() - 8.0 - p.height()
    } else {
        below
    };
    let style = panel.style();
    let _ = style.set_property("position", "fixed");
    let _ = style.set_property("margin", "0");
    let _ = style.set_property("left", &format!("{}px", t.left()));
    let _ = style.set_property("top", &format!("{top}px"));
}

/// Start (or resume) watching `panel` for size changes, re-running
/// [`position_below_trigger`] on every one — the fix for the stale-height race
/// documented at this function's one call site. Idempotent: a panel already
/// being watched just gets `observe`d again (a no-op per the
/// `ResizeObserver` spec for a target already under observation) rather than
/// growing a second observer, so re-opening the same popover cannot leak one
/// per open.
#[cfg(feature = "hydrate")]
fn watch_panel_resize(panel: &web_sys::HtmlElement, target_id: &str, watch: ResizeWatch) {
    use leptos::wasm_bindgen::closure::Closure;
    use leptos::wasm_bindgen::JsCast;

    let already_watching = watch.with_value(|v| {
        let Some((observer, _)) = v else {
            return false;
        };
        observer.observe(panel);
        true
    });
    if already_watching {
        return;
    }

    // The observer fires on the panel's own content-box size changing —
    // never on `position_below_trigger`'s own writes below, which only touch
    // `position`/`left`/`top` and never the panel's size — so this cannot
    // re-trigger itself.
    let watched_panel = panel.clone();
    let watched_target = target_id.to_string();
    let handler = Closure::wrap(Box::new(move || {
        position_below_trigger(&watched_panel, &watched_target);
    }) as Box<dyn FnMut()>);
    let Ok(observer) = web_sys::ResizeObserver::new(handler.as_ref().unchecked_ref()) else {
        return;
    };
    observer.observe(panel);
    watch.set_value(Some((observer, handler)));
}

/// Stop the [`ResizeObserver`](web_sys::ResizeObserver) [`watch_panel_resize`]
/// started, if one was. Disconnects rather than dropping it: the `StoredValue`
/// keeps the observer and its closure alive across the toggle (the same
/// non-`Send`-JS-object-outliving-one-`Effect`-run reason `media_signal`'s own
/// watch in `components/viewport.rs` is a `StoredValue`), so the next open's
/// [`watch_panel_resize`] can `observe` the same panel again instead of
/// constructing a fresh observer and closure.
#[cfg(feature = "hydrate")]
fn stop_watching_resize(watch: ResizeWatch) {
    watch.with_value(|v| {
        if let Some((observer, _)) = v {
            observer.disconnect();
        }
    });
}
