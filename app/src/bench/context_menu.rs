//! Bench section for the vendored `context_menu` (right-click → native
//! `popover="manual"` panel at the pointer; long-press on the Android webview
//! synthesizes `contextmenu`, so this section is the on-device check).
//!
//! It also exercises the **keyboard** half added for the tree's `Move to…`:
//! `Shift+F10` / the Menu key on the focused trigger opens the panel, opening
//! moves focus to the first item, ↑↓/Home/End rove, and ESC closes and hands
//! focus back. `data-bench-context-opener` is the focus-return target the
//! bench check asserts against — without a named opener, "focus went back"
//! cannot be told from "focus fell to `<body>`".

use leptos::prelude::*;

use crate::components::ui::context_menu::{
    ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuLabel, ContextMenuTrigger,
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
            </ContextMenu>
            <p class="text-muted-foreground text-xs">
                "Last selected: " <span data-bench-context-last>{move || last.get()}</span>
            </p>
        </div>
    }
    .into_any()
}
