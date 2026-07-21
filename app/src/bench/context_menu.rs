//! Bench section for the vendored `context_menu` (right-click → native
//! `popover="auto"` panel at the pointer; long-press on the Android webview
//! synthesizes `contextmenu`, so this section is the on-device check).

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
                    <div
                        data-bench-context-target
                        class="border-input text-muted-foreground flex h-24 w-full max-w-sm items-center justify-center rounded-md border border-dashed text-sm"
                    >
                        "Right-click (or long-press) here"
                    </div>
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
