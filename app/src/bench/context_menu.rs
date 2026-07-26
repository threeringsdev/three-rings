//! Bench section for the vendored `context_menu` (right-click → native
//! `popover="manual"` panel at the pointer). The tree's own surface is authed
//! and the Android dev proxy strips cookies, so this section is where the
//! primitive gets its real-webview check.
//!
//! **Long-press is not the touch story it was assumed to be.** The header this
//! replaces claimed the Android webview synthesizes `contextmenu` from a
//! long-press; `end2end/android-tree-move-check.mjs` pressed for real
//! (`Input.dispatchTouchEvent`, held 1.2 s) and no `contextmenu` arrived, while
//! a tap on the same page did produce a click. The earlier "verified" run had
//! *dispatched a synthetic `contextmenu` event*, which tests the handler and
//! not the gesture.
//!
//! It also exercises the **keyboard** half added for the tree's `Move to…`:
//! `Shift+F10` / the Menu key on the focused trigger opens the panel, opening
//! moves focus to the first item, ↑↓/Home/End rove, and ESC closes and hands
//! focus back. `data-bench-context-opener` is the focus-return target the
//! bench check asserts against — without a named opener, "focus went back"
//! cannot be told from "focus fell to `<body>`".

//! And the **programmatic** open — [`ContextMenuHandle::open_at`], the API a
//! composite with N rows and one shared panel uses (the collection tree). It
//! had no bench coverage at all, and it is the only trigger a phone can
//! actually work: the tree's real surface is authed, the Android dev proxy
//! strips cookies, so `⋯`-shaped-button → menu → item is checkable on-device
//! *here* or nowhere.

use leptos::prelude::*;

use crate::components::ui::context_menu::{
    use_context_menu, ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuLabel,
    ContextMenuTrigger,
};
use crate::components::ui::separator::Separator;

pub fn demo() -> AnyView {
    let last = RwSignal::new(String::from("nothing yet"));
    let select = move |what: &'static str| Callback::new(move |()| last.set(what.into()));

    view! {
        <div class="space-y-3">
            <ContextMenu id="bench-context-menu">
                <ContextMenuTrigger>
                    // A `<button>`, not a bare div: the keyboard route needs a
                    // focusable opener, and it doubles as the focus-return
                    // target ESC must restore.
                    <button
                        type="button"
                        data-bench-context-target
                        data-bench-context-opener
                        class="border-input text-muted-foreground focus-visible:ring-ring flex h-24 w-full max-w-sm items-center justify-center rounded-md border border-dashed text-sm focus-visible:ring-1 focus-visible:outline-none"
                    >
                        "Right-click, long-press, or focus me and press Shift+F10"
                    </button>
                </ContextMenuTrigger>
                <ContextMenuContent>
                    <ContextMenuLabel>"Collection"</ContextMenuLabel>
                    <ContextMenuItem on_select=select("new-binder")>"New binder inside…"</ContextMenuItem>
                    <ContextMenuItem on_select=select("rename")>"Rename…"</ContextMenuItem>
                    <Separator class="my-1" />
                    <ContextMenuItem
                        on_select=select("delete")
                        class="text-destructive hover:bg-destructive/10 hover:text-destructive"
                    >
                        "Delete…"
                    </ContextMenuItem>
                </ContextMenuContent>
                <TapTrigger />
            </ContextMenu>
            <p class="text-muted-foreground text-xs">
                "Last selected: " <span data-bench-context-last>{move || last.get()}</span>
            </p>
        </div>
    }
    .into_any()
}

/// The tree row's `⋯` in miniature: a plain button that opens the shared panel
/// through [`use_context_menu`], anchored to its own rect.
///
/// A separate component because `use_context_menu()` reads a context the
/// `ContextMenu` provides — a call in `demo()`'s own body sits *above* that
/// provider and resolves to `None` (the same owner rule that forced the tree's
/// menu wrapper inside its `Suspense`).
#[component]
fn TapTrigger() -> impl IntoView {
    let menu = use_context_menu();
    view! {
        <button
            type="button"
            data-bench-context-tap
            aria-haspopup="menu"
            class="border-input hover:bg-accent focus-visible:ring-ring rounded-md border px-2 py-1 text-sm focus-visible:ring-1 focus-visible:outline-none"
            on:click=move |ev| {
                if let Some(menu) = menu {
                    // The button's rect, not `client_x/y`: a keyboard
                    // activation reports 0,0 (and a tap reports the touch
                    // point, which is *inside* the button — anchoring to the
                    // rect puts the panel in the same place either way).
                    let (x, y) = anchor(ev.as_ref()).unwrap_or((0.0, 0.0));
                    menu.open_at(x, y);
                }
            }
        >
            "⋯ Actions"
        </button>
    }
}

#[cfg(feature = "hydrate")]
fn anchor(ev: &leptos::web_sys::Event) -> Option<(f64, f64)> {
    use leptos::wasm_bindgen::JsCast;
    let el = ev
        .current_target()?
        .dyn_into::<leptos::web_sys::HtmlElement>()
        .ok()?;
    let rect = el.get_bounding_client_rect();
    Some((rect.left(), rect.bottom()))
}

#[cfg(not(feature = "hydrate"))]
fn anchor(_ev: &leptos::web_sys::Event) -> Option<(f64, f64)> {
    None
}
