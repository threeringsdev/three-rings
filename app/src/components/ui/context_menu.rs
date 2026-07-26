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
//! - **the panel is keyboard-operable** (added for the tree's `Move to…`, which
//!   exists to be reachable without a mouse): opening it moves focus to the
//!   first `role="menuitem"`, ↑↓/Home/End rove between items, and closing puts
//!   focus back where it came from. Upstream had none of this — the panel was
//!   right-click-only, so a keyboard could at best *open* a menu it could never
//!   reach a row in (Tab from the opener walks the document, not the panel).

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
    /// Should the next close hand focus back to whatever opened the menu?
    ///
    /// True for a dismissal (ESC, an outside click) — the user is going back to
    /// where they were. **False when an item was activated**, because the item's
    /// action decides where focus goes next: the tree's `Move to…` opens a
    /// dialog and focuses its search field, and a restore racing that in the
    /// same effect flush would yank focus back to the row and dead-end the
    /// keyboard path this whole feature exists for.
    restore_focus: RwSignal<bool>,
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
        restore_focus: RwSignal::new(true),
    };

    view! {
        <leptos::context::Provider value=ctx>
            <div data-name="ContextMenu" class="contents">
                {children()}
            </div>
        </leptos::context::Provider>
    }
}

/// Wrapper that opens the menu on right-click, or on the keyboard's menu chord
/// (`ContextMenu` / `Shift+F10`) from anything focused inside it.
///
/// **Not on long-press.** That was assumed for the Android webview and does not
/// hold: a real held touch produced no `contextmenu` there
/// (`end2end/android-tree-move-check.mjs`). A touch surface needs an explicit
/// trigger calling [`ContextMenuHandle::open_at`] — what the tree's row `⋯`
/// button does.
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
            on:keydown=move |ev| {
                // The platform keyboard route into a context menu. Engines do
                // synthesize `contextmenu` from these keys, but at 0,0 — and
                // not at all through some automation transports — so the chord
                // is handled here and the synthesized event prevented, which
                // also lets the panel be anchored to the focused element rather
                // than the viewport corner.
                let key = ev.key();
                if key != "ContextMenu" && !(key == "F10" && ev.shift_key()) {
                    return;
                }
                ev.prevent_default();
                let (x, y) = focused_anchor(&ev).unwrap_or((0.0, 0.0));
                handle.open_at(x, y);
            }
        >
            {children()}
        </div>
    }
}

/// Bottom-left of the element the key event landed on, in viewport
/// coordinates. `target`, not `current_target`: [`ContextMenuTrigger`] is a
/// `display: contents` wrapper, which has no box to measure. Hydrate-only —
/// the measurement API and the gesture both exist only client-side.
#[cfg(feature = "hydrate")]
fn focused_anchor(ev: &leptos::ev::KeyboardEvent) -> Option<(f64, f64)> {
    use leptos::wasm_bindgen::JsCast;
    let el = ev
        .target()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
        .or_else(active_element)?;
    let rect = el.get_bounding_client_rect();
    Some((rect.left(), rect.bottom()))
}

#[cfg(not(feature = "hydrate"))]
fn focused_anchor(_ev: &leptos::ev::KeyboardEvent) -> Option<(f64, f64)> {
    None
}

#[component]
pub fn ContextMenuContent(
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<ContextMenuContext>();
    let open = ctx.open;
    let pos = ctx.pos;
    #[cfg_attr(not(feature = "hydrate"), allow(unused_variables))]
    let restore_focus = ctx.restore_focus;

    let class = tw_merge!(
        "z-50 p-1 rounded-md border bg-popover text-popover-foreground shadow-md w-[200px] m-0",
        class
    );

    let node_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    // What had focus when the menu opened, so closing can hand it back. A menu
    // that takes focus and then drops it on `<body>` costs a keyboard user
    // their place in the list they opened it from — the exact user this
    // component's keyboard support exists for. `new_local` because a DOM node
    // is neither `Send` nor `Sync`.
    #[cfg(feature = "hydrate")]
    let opener: StoredValue<Option<web_sys::HtmlElement>, leptos::prelude::LocalStorage> =
        StoredValue::new_local(None);

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
                let opening = !is_open;
                #[cfg(feature = "hydrate")]
                if opening {
                    opener.set_value(active_element());
                }
                if opening && el.show_popover().is_err() {
                    open.set(el.matches(":popover-open").unwrap_or(false));
                    return;
                }
                #[cfg(feature = "hydrate")]
                position_at_pointer(&el, x, y);
                #[cfg(not(feature = "hydrate"))]
                let _ = (x, y);
                // Focus goes in on the *opening* transition only — the effect
                // also re-runs on a reposition, and stealing focus back to the
                // first item then would undo the user's own ↑↓.
                #[cfg(feature = "hydrate")]
                if opening {
                    focus_menu_item(&el, MenuStep::First);
                }
            } else if is_open {
                let _ = el.hide_popover();
                #[cfg(feature = "hydrate")]
                {
                    let prev = opener.try_update_value(Option::take).flatten();
                    if restore_focus.get_untracked() {
                        if let Some(prev) = prev {
                            let _ = prev.focus();
                        }
                    }
                    restore_focus.set(true);
                }
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
            on:keydown=move |ev| {
                // Roving focus between the items. ⏎/space need nothing here —
                // the items are real `<button>`s and activate themselves — and
                // ESC is handled on `window` above, where it also serves a menu
                // whose focus never made it in.
                let step = match ev.key().as_str() {
                    "ArrowDown" => MenuStep::Next,
                    "ArrowUp" => MenuStep::Prev,
                    "Home" => MenuStep::First,
                    "End" => MenuStep::Last,
                    _ => return,
                };
                ev.prevent_default();
                #[cfg(feature = "hydrate")]
                if let Some(el) = node_ref.get_untracked() {
                    focus_menu_item(&el, step);
                }
                #[cfg(not(feature = "hydrate"))]
                let _ = step;
            }
        >
            {children()}
        </div>
    }
}

/// Which item [`focus_menu_item`] should land on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuStep {
    First,
    Last,
    Next,
    Prev,
}

/// The document's focused element, if it is one.
#[cfg(feature = "hydrate")]
fn active_element() -> Option<web_sys::HtmlElement> {
    use leptos::wasm_bindgen::JsCast;
    document()
        .active_element()
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
}

/// Move focus among the panel's `role="menuitem"` buttons, wrapping at both
/// ends the way a menu is expected to. Hydrate-only: focus and the DOM query
/// are client-side, and so is every gesture that reaches this.
#[cfg(feature = "hydrate")]
fn focus_menu_item(panel: &web_sys::HtmlDivElement, step: MenuStep) {
    use leptos::wasm_bindgen::JsCast;
    let Ok(list) = panel.query_selector_all("[role=menuitem]") else {
        return;
    };
    let items: Vec<web_sys::HtmlElement> = (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .collect();
    if items.is_empty() {
        return;
    }
    let current =
        active_element().and_then(|a| items.iter().position(|i| i.is_same_node(Some(a.as_ref()))));
    let next = match (step, current) {
        (MenuStep::First, _) => 0,
        (MenuStep::Last, _) => items.len() - 1,
        // No item focused yet (the menu was opened by pointer, which leaves
        // focus outside): the first arrow press enters at the near end.
        (MenuStep::Next, None) => 0,
        (MenuStep::Prev, None) => items.len() - 1,
        (MenuStep::Next, Some(i)) => (i + 1) % items.len(),
        (MenuStep::Prev, Some(i)) => (i + items.len() - 1) % items.len(),
    };
    let _ = items[next].focus();
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
    let restore_focus = ctx.restore_focus;

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
                // The action owns focus from here (see `restore_focus`), so
                // this close must not put it back on the opener.
                restore_focus.set(false);
                on_select.run(());
                open.set(false);
            }
        >
            {children()}
        </button>
    }
}
