//! Context menu — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/context_menu.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Markup + classes kept; behavior fully rewired in
//! Leptos (upstream drives open/close/position from an inline vanilla
//! `<script>` per instance). Deviations from upstream:
//! - **deterministic caller-supplied `id`** replaces `use_random_id_for`
//!   (SSR counter hydration bug — same convention as `dialog`/`popover`)
//! - the inline `<script>` is gone: the panel is a native **`popover="manual"`**
//!   (top layer, no *automatic* light-dismiss), shown at the pointer via Leptos
//!   state. Viewport clamping (upstream's `updatePosition`) is an Effect after
//!   `showPopover`. **Not `popover="auto"`**: an auto popover light-dismisses
//!   on any outside pointerdown, and a right-click's own trailing pointerup
//!   races that dismissal, closing the menu the instant it opens (engine-
//!   dependent — some engines close it, some don't). We dismiss ourselves, on
//!   the first *subsequent* outside pointerdown and on ESC, and defer the open
//!   one macrotask so the opening gesture can't self-close.
//! - `close_context_menu()`'s global DOM query is replaced by the context's
//!   open signal; composites open programmatically via [`use_context_menu`]
//!   (so a tree with N rows can share **one** menu instead of N panels)
//! - `window.ScrollLock` dropped — a manual popover doesn't lock scroll;
//!   scrolling light-dismisses on the next outside pointerdown
//! - the hover-only CSS submenu (`ContextMenuSub*` + its `icons` import) is
//!   dropped: no keyboard/touch path, no consumer; revisit if a surface
//!   needs nesting
//! - `ContextMenuGroup` (a `ul` expecting `li` items) is dropped — items
//!   here are `role="menuitem"` buttons under one `role="menu"` panel
//!   (upstream ships no ARIA at all); use `Separator` between clusters
//! - ESC and outside-pointerdown dismissal are our own `window` listeners
//!   (a manual popover gets neither for free), removed on `on_cleanup`. ESC
//!   is not overlay-stack-coordinated — same known caveat as `popover`.

use leptos::prelude::*;
use tw_merge::tw_merge;

use super::clx::clx;

mod components {
    use super::*;
    clx! {ContextMenuLabel, span, "px-2 py-1.5 text-sm font-medium block", "mb-1"}
}

pub use components::*;

#[derive(Clone)]
struct ContextMenuContext {
    target_id: String,
    open: RwSignal<bool>,
    pos: RwSignal<(f64, f64)>,
    /// Bumped on every open/close so a *pending* deferred open (see `open_at`)
    /// that a `close` raced can tell it is stale and skip.
    generation: RwSignal<u32>,
}

/// Programmatic handle for composites that open one shared menu from many
/// rows (the collection tree): position it at the pointer and open.
#[derive(Clone, Copy)]
pub struct ContextMenuHandle {
    open: RwSignal<bool>,
    pos: RwSignal<(f64, f64)>,
    generation: RwSignal<u32>,
}

impl ContextMenuHandle {
    pub fn open_at(&self, x: f64, y: f64) {
        self.pos.set((x, y));
        // Defer the actual open to the next macrotask. A right-click's own
        // pointer sequence (mousedown → contextmenu → mouseup/click) is still
        // in flight when this handler runs; showing the popover now lets a
        // trailing pointerup be read as an outside interaction and dismiss it
        // the instant it appears. Letting the gesture finish first avoids the
        // race; the first *subsequent* outside pointerdown still dismisses.
        //
        // The deferral is guarded by a generation stamp so a `close` (or a
        // second `open_at`) that lands before the macrotask cancels this open
        // rather than reviving a menu the caller already dismissed.
        let open = self.open;
        let generation = self.generation;
        let stamp = generation.get_untracked().wrapping_add(1);
        generation.set(stamp);
        set_timeout(
            move || {
                if generation.get_untracked() == stamp {
                    open.set(true);
                }
            },
            std::time::Duration::from_millis(0),
        );
    }

    pub fn close(&self) {
        self.generation.update(|g| *g = g.wrapping_add(1));
        self.open.set(false);
    }
}

/// The enclosing menu's handle, for rows that open it themselves.
pub fn use_context_menu() -> Option<ContextMenuHandle> {
    use_context::<ContextMenuContext>().map(|c| ContextMenuHandle {
        open: c.open,
        pos: c.pos,
        generation: c.generation,
    })
}

#[component]
pub fn ContextMenu(
    /// Deterministic instance id — SSR and hydration must agree on it.
    #[prop(into)]
    id: String,
    children: Children,
) -> impl IntoView {
    let ctx = ContextMenuContext {
        target_id: format!("context-menu-{id}"),
        open: RwSignal::new(false),
        pos: RwSignal::new((0.0, 0.0)),
        generation: RwSignal::new(0),
    };

    view! {
        <leptos::context::Provider value=ctx>
            <div data-name="ContextMenu" class="contents">
                {children()}
            </div>
        </leptos::context::Provider>
    }
}

/// Wrapper that opens the menu on right-click (or long-press where the
/// platform synthesizes `contextmenu`, e.g. the Android webview).
#[component]
pub fn ContextMenuTrigger(
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<ContextMenuContext>();
    let trigger_class = tw_merge!("contents", class);
    let handle = ContextMenuHandle {
        open: ctx.open,
        pos: ctx.pos,
        generation: ctx.generation,
    };

    view! {
        <div
            class=trigger_class
            data-name="ContextMenuTrigger"
            on:contextmenu=move |ev| {
                ev.prevent_default();
                handle.open_at(f64::from(ev.client_x()), f64::from(ev.client_y()));
            }
        >
            {children()}
        </div>
    }
}

#[component]
pub fn ContextMenuContent(
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<ContextMenuContext>();
    let open = ctx.open;
    let pos = ctx.pos;

    let class = tw_merge!(
        "z-50 p-1 rounded-md border bg-popover text-popover-foreground shadow-md w-[200px] m-0",
        class
    );

    let node_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    // Signal → native popover, then clamp to the viewport (upstream's
    // `updatePosition`: flip to the other side of the pointer rather than
    // overflow an edge). Effects only run client-side; the measurement APIs
    // are hydrate-gated like popover's positioning fallback.
    //
    // **`popover="manual"`, not `"auto"`** — an auto popover light-dismisses
    // on any outside pointerdown, and a right-click's own pointer sequence
    // races that dismissal, closing the menu the instant it opens (engine-
    // dependent: some close it, some don't). Manual gives the top layer with
    // no automatic dismissal; we close it ourselves below, on the first
    // *subsequent* outside pointerdown and on ESC.
    Effect::new(move |_| {
        let want_open = open.get();
        let (x, y) = pos.get();
        if let Some(el) = node_ref.get() {
            let is_open = el.matches(":popover-open").unwrap_or(false);
            if want_open {
                if !is_open && el.show_popover().is_err() {
                    open.set(el.matches(":popover-open").unwrap_or(false));
                    return;
                }
                #[cfg(feature = "hydrate")]
                position_at_pointer(&el, x, y);
                #[cfg(not(feature = "hydrate"))]
                let _ = (x, y);
            } else if is_open {
                let _ = el.hide_popover();
            }
        }
    });

    // Our own light-dismiss, gated on `open`. Because `open_at` defers the
    // open to the next macrotask, the opening pointerdown fires while `open`
    // is still false and this listener ignores it; the first pointerdown after
    // the menu is up closes it — unless it landed inside the panel (a menu
    // item, which closes itself via its own click).
    let dismiss = window_event_listener(leptos::ev::pointerdown, move |ev| {
        if !open.get_untracked() {
            return;
        }
        #[cfg(feature = "hydrate")]
        if pointer_outside(&node_ref, &ev) {
            open.set(false);
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = &ev;
    });
    // ESC closes it (manual popovers get no built-in ESC).
    let esc = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && open.get_untracked() {
            ev.prevent_default();
            open.set(false);
        }
    });
    on_cleanup(move || {
        dismiss.remove();
        esc.remove();
    });

    view! {
        <div
            class=class
            id=ctx.target_id.clone()
            popover="manual"
            role="menu"
            data-name="ContextMenuContent"
            style="position: fixed; inset: auto; left: 0; top: 0;"
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

/// Whether a pointerdown landed outside the panel (so it should dismiss).
/// Hydrate-only: the DOM containment API exists client-side.
#[cfg(feature = "hydrate")]
fn pointer_outside(node_ref: &NodeRef<leptos::html::Div>, ev: &leptos::ev::PointerEvent) -> bool {
    use leptos::wasm_bindgen::JsCast;
    let Some(el) = node_ref.get_untracked() else {
        return false;
    };
    match ev.target() {
        Some(t) => !el.contains(Some(t.unchecked_ref::<web_sys::Node>())),
        None => false,
    }
}

/// Place the shown panel at the pointer, flipping to the other side of the
/// cursor rather than overflowing a viewport edge (upstream's
/// `updatePosition`). Hydrate-only: the measurement APIs exist client-side.
#[cfg(feature = "hydrate")]
fn position_at_pointer(el: &web_sys::HtmlDivElement, x: f64, y: f64) {
    let rect = el.get_bounding_client_rect();
    let w = web_sys::window();
    let vw = w
        .as_ref()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::MAX);
    let vh = w
        .as_ref()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::MAX);
    let left = if x + rect.width() > vw {
        x - rect.width()
    } else {
        x
    };
    let top = if y + rect.height() > vh {
        y - rect.height()
    } else {
        y
    };
    let style = web_sys::HtmlElement::style(el);
    let _ = style.set_property("left", &format!("{}px", left.max(0.0)));
    let _ = style.set_property("top", &format!("{}px", top.max(0.0)));
}

/// One action row. Runs `on_select`, then closes the menu.
#[component]
pub fn ContextMenuItem(
    on_select: Callback<()>,
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<ContextMenuContext>();
    let open = ctx.open;

    let class = tw_merge!(
        "inline-flex gap-2 items-center w-full rounded-sm px-2 py-1.5 text-sm text-left no-underline transition-colors duration-200 text-popover-foreground hover:bg-accent hover:text-accent-foreground focus:outline-none focus-visible:bg-accent focus-visible:text-accent-foreground [&_svg:not([class*='size-'])]:size-4",
        class
    );

    view! {
        <button
            type="button"
            role="menuitem"
            data-name="ContextMenuItem"
            class=class
            on:click=move |_| {
                on_select.run(());
                open.set(false);
            }
        >
            {children()}
        </button>
    }
}
