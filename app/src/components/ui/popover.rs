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
//! - **the JS fallback now mirrors the CSS anchor path's semantics instead
//!   of its own fixed below-only, left-only, unclamped placement**
//!   (WB-01M05K42G96ZSGHEBHKQ47CBDV): `#148`'s adversarial review flagged
//!   those three as consciously-dropped flaws on the assumption the fallback
//!   was dead code on every real engine — wrong once a maintainer's desktop
//!   `.app` screenshots showed the tray's `End`-aligned "Move to…" panel
//!   rendering below the window and the catalog's `Center`-aligned "Adding
//!   to" panel reduced to a sliver, proving the system WKWebView the Tauri
//!   shell embeds has the Popover API but not CSS anchor positioning.
//!   [`fallback_position`] is now pure rect math (no DOM, unit-tested on
//!   every host): it honors [`PopoverAlign`] the same way the CSS path's
//!   per-align block does (`Start`/`End` align an edge, `Center` centers,
//!   `*Outer` sit beside the trigger), prefers opening ABOVE the trigger by
//!   default — mirroring `position-area: block-start` / `bottom:
//!   anchor(top)`, the CSS path's own default — and flips below only when
//!   there isn't room above (the CSS path's `@position-try(flip-block)`),
//!   then clamps the result inside the viewport with a small margin on every
//!   edge. The DOM shim (`apply_fallback_position`) and the `ResizeObserver`
//!   wiring are otherwise unchanged in mechanism — they now just pass
//!   `align` and the panel's width through, alongside the height they
//!   already measured.

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
    /// Threaded through to the JS fallback (`fallback_position`) so it can
    /// mirror the same per-align rule the CSS path's `position_styles` match
    /// above encodes — the fallback has no other way to learn which align
    /// the caller chose.
    align: PopoverAlign,
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
        align,
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
    let align = ctx.align;

    // Re-runs `apply_fallback_position` whenever the panel's own size changes
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
            // WebKit — including, per WB-01M05K42G96ZSGHEBHKQ47CBDV, the
            // system WKWebView the desktop `.app` embeds — ships the Popover
            // API but not always CSS anchor positioning, so the panel would
            // open at the viewport default. When anchors are unsupported and
            // we're open, position manually with the same semantics the CSS
            // path would have used (`fallback_position`'s doc comment).
            // Hydrate-only: the DOM measurement APIs and the mispositioning
            // it corrects both exist only client-side.
            //
            // **Also kept in sync with a `ResizeObserver`, not just this one
            // call.** `apply_fallback_position` measures the panel's *current*
            // size to decide where it fits — but this Effect fires once per
            // `open` toggle, right when `show_popover()` runs, which on a
            // picker whose rows come from a `Resource` (every
            // `DestinationList` consumer: the catalog toolbar, the tray's own
            // "Move to…") is *before* the real rows have arrived. The first
            // measurement catches the much shorter "Loading collections…"
            // placeholder, pins the position to a decision made for that
            // size, and then never revisits it as the real rows land and the
            // panel grows underneath that stale position — parking the
            // now-taller edge off the target device's screen with no way to
            // scroll to it. A fixed delay cannot promise the fetch has landed
            // by then; a `ResizeObserver` reacts to the panel's real size
            // settling, however long that takes.
            #[cfg(feature = "hydrate")]
            {
                if want_open && !anchor_positioning_supported() {
                    apply_fallback_position(&el, &target_id, align);
                    watch_panel_resize(&el, &target_id, align, resize_watch);
                } else {
                    stop_watching_resize(resize_watch);
                }
            }
            #[cfg(not(feature = "hydrate"))]
            let _ = (&target_id, align);
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
/// Android webview) yes; WebKit not always — there (and, per
/// WB-01M05K42G96ZSGHEBHKQ47CBDV, the desktop `.app`'s system WKWebView) we
/// position manually.
#[cfg(feature = "hydrate")]
fn anchor_positioning_supported() -> bool {
    web_sys::css::supports("position-anchor: --x").unwrap_or(false)
}

/// A plain rectangle — mirrors the handful of [`web_sys::DomRect`] fields
/// [`fallback_position`] needs, without depending on `web_sys`. Keeping the
/// geometry free of DOM types is what lets [`fallback_position`] (and its
/// tests) compile and run on every host, including a bare `cargo test` with
/// no `hydrate` feature and no wasm target. `cfg`-gated on `test` as well as
/// `hydrate`: the only non-test caller (`apply_fallback_position`, the DOM
/// shim) lives behind `hydrate`, so without this the whole module is
/// legitimately dead code — and therefore clippy-denied — in a build that
/// carries neither.
#[cfg(any(test, feature = "hydrate"))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[cfg(any(test, feature = "hydrate"))]
impl Rect {
    fn right(&self) -> f64 {
        self.left + self.width
    }

    fn bottom(&self) -> f64 {
        self.top + self.height
    }
}

/// A width/height pair — used for the panel (whose position isn't known yet,
/// only its current size) and the viewport (whose origin is always `0, 0`).
#[cfg(any(test, feature = "hydrate"))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Size {
    width: f64,
    height: f64,
}

/// Gap kept between the trigger and the panel — mirrors the CSS path's own
/// `margin-bottom: 8px` / `margin-top: 8px` (declared per-align in
/// `Popover`'s `position_styles` match).
#[cfg(any(test, feature = "hydrate"))]
const FALLBACK_GAP: f64 = 8.0;

/// Minimum breathing room kept against every viewport edge once the panel is
/// clamped inside it. Independent of `FALLBACK_GAP`: this is the edge the CSS
/// path never had to think about because `position-visibility:
/// anchors-visible` plus its `@position-try` fallbacks were doing the
/// clamping natively — the JS path has to do it by hand.
#[cfg(any(test, feature = "hydrate"))]
const VIEWPORT_MARGIN: f64 = 8.0;

/// Pure geometry for the JS positioning fallback: given the trigger's rect,
/// the panel's own size, the viewport's size, and the popover's
/// [`PopoverAlign`], returns the `(left, top)` fixed-position coordinates
/// that mirror the CSS anchor path's own semantics.
///
/// - **Vertical placement** defaults to ABOVE the trigger — the CSS path's
///   own default (`position-area: block-start` for `Center`, `bottom:
///   anchor(top)` for `Start`/`End`) — and flips below only when there isn't
///   room above but there is below, mirroring the CSS path's
///   `@position-try(flip-block)` fallback. When *neither* side has room (a
///   short viewport under a tall panel), it picks whichever side has more
///   space, so the clamp below has the least work left to do. This is the
///   piece that matters for a bottom-docked trigger (the selection tray's
///   "Move to…"): space below is ~0, so it always resolves to ABOVE.
/// - **Horizontal placement** honors `align` the same way the CSS path's
///   per-align block does: `Start`/`End` align the panel's left/right edge
///   with the trigger's, `Center` centers it over the trigger, and
///   `StartOuter`/`EndOuter` sit the panel beside the trigger (used for
///   flyout-style menus; no current caller uses them, but the fallback
///   should not silently ignore them either).
/// - **Clamping**: the result is then clamped inside the viewport with
///   [`VIEWPORT_MARGIN`] of breathing room on every side — the flaw #148's
///   review flagged as dropped ("never clamps to viewport edges"). A panel
///   larger than the viewport minus its margins clamps to the margin itself
///   rather than overflowing, which is the best any positioning can do for a
///   panel that structurally cannot fit.
#[cfg(any(test, feature = "hydrate"))]
fn fallback_position(
    trigger: Rect,
    panel: Size,
    viewport: Size,
    align: PopoverAlign,
) -> (f64, f64) {
    let space_above = trigger.top;
    let space_below = viewport.height - trigger.bottom();
    let needed = panel.height + FALLBACK_GAP;
    let fits_above = space_above >= needed;
    let fits_below = space_below >= needed;
    let top = if fits_above || (!fits_below && space_above >= space_below) {
        trigger.top - FALLBACK_GAP - panel.height
    } else {
        trigger.bottom() + FALLBACK_GAP
    };

    let left = match align {
        PopoverAlign::Start => trigger.left,
        PopoverAlign::End => trigger.right() - panel.width,
        PopoverAlign::Center => trigger.left + trigger.width / 2.0 - panel.width / 2.0,
        PopoverAlign::StartOuter => trigger.left - FALLBACK_GAP - panel.width,
        PopoverAlign::EndOuter => trigger.right() + FALLBACK_GAP,
    };

    (
        clamp_within(left, panel.width, viewport.width),
        clamp_within(top, panel.height, viewport.height),
    )
}

/// Clamps a single axis (`pos`, sized `size`) inside `[0, viewport]` with
/// [`VIEWPORT_MARGIN`] of breathing room on both ends. When `size` alone
/// (plus both margins) exceeds `viewport`, the upper bound would fall below
/// the lower one — `.max(VIEWPORT_MARGIN)` on the upper bound keeps the
/// panel pinned to the margin instead of the clamp inverting and producing a
/// position further off-screen than the unclamped one was.
#[cfg(any(test, feature = "hydrate"))]
fn clamp_within(pos: f64, size: f64, viewport: f64) -> f64 {
    let max = (viewport - size - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
    pos.max(VIEWPORT_MARGIN).min(max)
}

/// DOM shim over [`fallback_position`]: measures the trigger's position via
/// `getBoundingClientRect`, the panel's size via `offsetWidth`/`offsetHeight`
/// (see the note below on why, not `getBoundingClientRect` there too), the
/// viewport via `window.inner{Width,Height}`, and writes the resulting
/// `position: fixed; left; top` onto the panel. Only used when CSS anchor
/// positioning is unavailable.
///
/// **`offsetWidth`/`offsetHeight` for the panel, not `getBoundingClientRect`
/// — found empirically, not by inspection.** This function's caller (the
/// `Effect` in `PopoverContent`) runs synchronously inside the SAME tick as
/// `show_popover()`, right as the panel's `@starting-style` transition
/// (`transform: scale(0.95) translateY(...)`) is (or may still be) in
/// effect. `getBoundingClientRect()` returns the *painted, transformed* box
/// — a `scale(0.95)` on a 280px-wide panel reads as a ~266px-wide box at
/// that instant — and unlike a genuine layout-affecting size change (new
/// `DestinationList` rows arriving, or the internal scrollbar their
/// overflow introduces), a pure CSS `transform` never fires
/// [`watch_panel_resize`]'s `ResizeObserver` (transforms are paint-only, not
/// layout), so a `Center`-aligned panel caught mid-transition this way had
/// no second chance to correct itself: verified live (a forced-fallback
/// chromium probe against a real dev server, `end2end/
/// force-fallback-probe.mjs`) — the catalog's `Center`-aligned "Adding to"
/// picker centered ~5px off depending on exactly when the effect ran,
/// while the tray's `End`-aligned picker (this bug's original report)
/// stayed exact, because an edge-alignment error from the same width slip
/// is smaller and further diluted by the clamp. `offsetWidth`/`offsetHeight`
/// report the element's *layout* border-box size — untouched by `transform`
/// — so they read the panel's true, settled 280px width even mid-animation,
/// making the very first synchronous placement correct instead of relying
/// on an incidental later `ResizeObserver` firing to paper over it.
#[cfg(feature = "hydrate")]
fn apply_fallback_position(panel: &web_sys::HtmlElement, target_id: &str, align: PopoverAlign) {
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
    let Some(window) = web_sys::window() else {
        return;
    };
    let viewport = Size {
        width: window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        height: window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    };

    let (left, top) = fallback_position(
        Rect {
            left: t.left(),
            top: t.top(),
            width: t.width(),
            height: t.height(),
        },
        Size {
            width: f64::from(panel.offset_width()),
            height: f64::from(panel.offset_height()),
        },
        viewport,
        align,
    );

    let style = panel.style();
    let _ = style.set_property("position", "fixed");
    let _ = style.set_property("margin", "0");
    let _ = style.set_property("left", &format!("{left}px"));
    let _ = style.set_property("top", &format!("{top}px"));
}

/// Start (or resume) watching `panel` for size changes, re-running
/// [`apply_fallback_position`] on every one — the fix for the stale-height
/// race documented at this function's one call site. Idempotent: a panel
/// already being watched just gets `observe`d again (a no-op per the
/// `ResizeObserver` spec for a target already under observation) rather than
/// growing a second observer, so re-opening the same popover cannot leak one
/// per open.
#[cfg(feature = "hydrate")]
fn watch_panel_resize(
    panel: &web_sys::HtmlElement,
    target_id: &str,
    align: PopoverAlign,
    watch: ResizeWatch,
) {
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
    // never on `apply_fallback_position`'s own writes below, which only
    // touch `position`/`left`/`top` and never the panel's size — so this
    // cannot re-trigger itself.
    let watched_panel = panel.clone();
    let watched_target = target_id.to_string();
    let handler = Closure::wrap(Box::new(move || {
        apply_fallback_position(&watched_panel, &watched_target, align);
    }) as Box<dyn FnMut()>);
    let Ok(observer) = web_sys::ResizeObserver::new(handler.as_ref().unchecked_ref()) else {
        return;
    };
    observer.observe(panel);
    watch.set_value(Some((observer, handler)));
}

#[cfg(test)]
mod fallback_position_tests {
    use super::*;

    /// A generously-sized viewport used by tests that aren't exercising the
    /// clamp itself.
    const ROOMY_VIEWPORT: Size = Size {
        width: 1200.0,
        height: 900.0,
    };

    fn trigger(left: f64, top: f64, width: f64, height: f64) -> Rect {
        Rect {
            left,
            top,
            width,
            height,
        }
    }

    fn panel(width: f64, height: f64) -> Size {
        Size { width, height }
    }

    // The reported bug, reproduced directly: the selection tray's "Move
    // to…" trigger docks at the very bottom of the window (`align =
    // PopoverAlign::End`, per `move_selection.rs`'s own "opening upward"
    // comment). With almost no room below and plenty above, the fallback
    // MUST resolve above the trigger — the flaw (WB-01M05K42G96ZSGHEBHKQ47CBDV)
    // was that the old fallback always went below regardless.
    #[test]
    fn end_aligned_bottom_docked_trigger_opens_above() {
        let t = trigger(700.0, 860.0, 80.0, 32.0); // bottom = 892, 8px from the 900px viewport floor
        let p = panel(280.0, 240.0);
        let (left, top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::End);

        assert!(
            top + p.height <= t.top,
            "panel bottom ({}) must clear the trigger's top ({}) — it opened above",
            top + p.height,
            t.top,
        );
        // Trailing edges align: the panel's right edge meets the trigger's.
        assert_eq!(left + p.width, t.right());
        // Comfortably inside the viewport (this scenario doesn't need the clamp).
        assert!(top >= VIEWPORT_MARGIN);
        assert!(left >= VIEWPORT_MARGIN);
        assert!(left + p.width <= ROOMY_VIEWPORT.width - VIEWPORT_MARGIN);
    }

    // The catalog's "Adding to" picker has no explicit `align`, so it uses
    // the `Center` default. Placed near the right edge, the naive centered
    // left would run the panel off the right of the viewport — the clamp
    // must pull it back in.
    #[test]
    fn center_align_near_right_edge_clamps_left() {
        let t = trigger(1150.0, 400.0, 60.0, 32.0); // center x = 1180, near the 1200px right edge
        let p = panel(280.0, 200.0);
        let (left, top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::Center);

        assert!(
            left + p.width <= ROOMY_VIEWPORT.width - VIEWPORT_MARGIN,
            "panel right ({}) must not run past the viewport",
            left + p.width
        );
        assert!(left >= VIEWPORT_MARGIN);
        // Still opens above by default (plenty of room at top=400 on a 900px
        // viewport).
        assert!(top + p.height <= t.top);
    }

    // Neither above nor below has room for a panel taller than the whole
    // viewport. The clamp must still produce an in-bounds (if imperfect)
    // position rather than a negative or off-screen one.
    #[test]
    fn tall_panel_short_viewport_clamps_top() {
        let viewport = Size {
            width: 400.0,
            height: 300.0,
        };
        let t = trigger(150.0, 140.0, 60.0, 20.0);
        let p = panel(200.0, 500.0); // taller than the entire viewport
        let (_left, top) = fallback_position(t, p, viewport, PopoverAlign::Center);

        assert!(top >= VIEWPORT_MARGIN, "top ({top}) must not go negative");
        // Can't fully fit — but must not compute a position further off the
        // bottom than the margin-pinned one the clamp settles on.
        assert_eq!(top, VIEWPORT_MARGIN);
    }

    // All four overflow directions, each forced independently.
    #[test]
    fn overflow_left_is_clamped() {
        // End-aligned against a narrow, near-left trigger: right-edge align
        // pushes the wide panel's left past 0.
        let t = trigger(0.0, 400.0, 20.0, 20.0);
        let p = panel(300.0, 100.0);
        let (left, _top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::End);
        assert_eq!(left, VIEWPORT_MARGIN);
    }

    #[test]
    fn overflow_right_is_clamped() {
        // Start-aligned against a trigger near the right edge: left-edge
        // align pushes the wide panel's right past the viewport.
        let t = trigger(ROOMY_VIEWPORT.width - 20.0, 400.0, 20.0, 20.0);
        let p = panel(300.0, 100.0);
        let (left, _top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::Start);
        assert_eq!(left, ROOMY_VIEWPORT.width - p.width - VIEWPORT_MARGIN);
    }

    #[test]
    fn overflow_top_is_clamped() {
        // Both sides are short of the panel's height, but `space_above`
        // (50) still ties-or-beats `space_below` (40 on this short
        // viewport), so "above" is picked — landing the RAW top at -58 (50 -
        // 8 - 100). The clamp must pull it back to the margin, not leave it
        // negative.
        let viewport = Size {
            width: 800.0,
            height: 100.0,
        };
        let t = trigger(300.0, 50.0, 60.0, 10.0); // space_above=50, space_below=40
        let p = panel(200.0, 100.0); // needed = 108, neither side fits
        let (_left, top) = fallback_position(t, p, viewport, PopoverAlign::Center);
        assert_eq!(
            top, VIEWPORT_MARGIN,
            "unclamped top would be negative (50 - 8 - 100); clamp must pin it to the margin"
        );
    }

    #[test]
    fn overflow_bottom_is_clamped() {
        // Both sides are short of the panel's height, but `space_below`
        // (180) beats `space_above` (100), so "below" is picked — landing
        // the RAW bottom at 128 + 175 = 303 on a 300px-tall viewport. The
        // clamp must pull the top back up so the panel's bottom stops at the
        // floor's margin instead of running past it.
        let viewport = Size {
            width: 800.0,
            height: 300.0,
        };
        let t = trigger(300.0, 100.0, 60.0, 20.0); // space_above=100, space_below=180
        let p = panel(200.0, 175.0); // needed = 183, neither side fits
        let (_left, top) = fallback_position(t, p, viewport, PopoverAlign::Center);
        assert!(
            top + p.height <= viewport.height - VIEWPORT_MARGIN,
            "panel bottom ({}) must not run past the viewport floor",
            top + p.height
        );
        assert!(
            top < t.bottom() + FALLBACK_GAP,
            "clamp must have pulled the naive below-placement back up"
        );
    }

    #[test]
    fn prefers_above_when_both_sides_fit() {
        let t = trigger(400.0, 400.0, 80.0, 32.0);
        let p = panel(200.0, 120.0);
        let (_left, top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::Start);
        assert_eq!(top, t.top - FALLBACK_GAP - p.height);
    }

    #[test]
    fn flips_below_when_above_is_insufficient() {
        // Only 20px above the trigger — nowhere near enough for a 120px panel
        // — but the viewport below has plenty of room.
        let t = trigger(400.0, 20.0, 80.0, 32.0);
        let p = panel(200.0, 120.0);
        let (_left, top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::Start);
        assert_eq!(top, t.bottom() + FALLBACK_GAP);
    }

    #[test]
    fn start_align_sets_left_edges_equal() {
        let t = trigger(400.0, 400.0, 80.0, 32.0);
        let p = panel(200.0, 120.0);
        let (left, _top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::Start);
        assert_eq!(left, t.left);
    }

    #[test]
    fn center_align_centers_over_trigger_when_unclamped() {
        let t = trigger(500.0, 400.0, 100.0, 32.0);
        let p = panel(200.0, 120.0);
        let (left, _top) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::Center);
        assert_eq!(left, t.left + t.width / 2.0 - p.width / 2.0);
    }

    #[test]
    fn outer_aligns_sit_beside_the_trigger() {
        let t = trigger(500.0, 400.0, 100.0, 32.0);
        let p = panel(200.0, 120.0);

        let (left_start_outer, _) =
            fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::StartOuter);
        assert_eq!(left_start_outer, t.left - FALLBACK_GAP - p.width);

        let (left_end_outer, _) = fallback_position(t, p, ROOMY_VIEWPORT, PopoverAlign::EndOuter);
        assert_eq!(left_end_outer, t.right() + FALLBACK_GAP);
    }
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
