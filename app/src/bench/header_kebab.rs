//! Bench section for the **collection-header kebab** (`app/src/my/collection.rs`
//! → `HeaderKebab`) — `design/wireframes.pen`'s `Header Kebab` and
//! `M Header Kebab`.
//!
//! It is an app composite over `context_menu`, not a vendored registry
//! component, and it is here for the one reason `my_root` is: **`/my/*` is
//! unreachable on the Android emulator.** The Tauri dev proxy strips Cookie
//! headers, so every authed surface redirects to `/login?next=…` (recorded four
//! times over in specs/app-ui.md Findings). The kebab's whole point is that a
//! phone can reach tree management without the rail drawer, so "a real touch on
//! this button opens the panel" has to be checkable on a real webview — and this
//! page is the only anonymous place it can be.
//!
//! The `context_menu` section above already covers the *primitive's*
//! programmatic open (`TapTrigger`). What this adds is the real button: its
//! 44 px hit area below `md`, its bordered 32 px box at `md` and up, and the fact
//! that the element the wireframe specifies is the element that opens the menu.
//!
//! The menu here is a stand-in, not `TreeMenu`: that reads `TreeManage` and the
//! collection-tree resource, both provided by `AppShell`, which the bench page
//! does not mount. The item labels are copied from the real panel so the section
//! reads as the surface it stands for; what is under test is the button.

use leptos::prelude::*;

use crate::components::ui::context_menu::{
    ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuLabel,
};
use crate::components::ui::separator::Separator;
use crate::my::collection::HeaderKebab;

pub fn demo() -> AnyView {
    let last = RwSignal::new(String::from("nothing yet"));
    let aimed = RwSignal::new(0u32);
    let select = move |what: &'static str| Callback::new(move |()| last.set(what.into()));

    view! {
        <div class="space-y-3">
            <p class="text-muted-foreground text-sm">
                "The collection header's " <code>"⋯"</code>
                ": a real button that opens the shared context menu at its own rect. Bordered 32 px box at "
                <code>"md"</code>
                " and up, a bare glyph in a 44 px tap target below it. Focus it and press ⏎ — the panel takes focus, ↑↓ rove, ESC hands it back."
            </p>
            // The wireframe's `Title Row`: title group left, `Header Actions`
            // right, framed at the phone width so the 44 px target and the bare
            // glyph are readable without resizing the window.
            <div class="bg-background w-[390px] max-w-full overflow-hidden rounded-xl border p-4">
                <div class="flex items-start gap-3">
                    <div class="min-w-0 flex-1">
                        <h3 class="text-xl font-semibold">"Trade Binder"</h3>
                        <p class="text-muted-foreground text-sm">
                            "120 here (102 own + 18 rolled up) · 6 wanted"
                        </p>
                    </div>
                    <div class="flex shrink-0 items-center gap-2">
                        <ContextMenu id="bench-header-kebab">
                            <HeaderKebab aim=Callback::new(move |()| {
                                aimed.update(|n| *n += 1);
                            }) />
                            <ContextMenuContent class="w-56">
                                <ContextMenuLabel>"Trade Binder"</ContextMenuLabel>
                                <ContextMenuItem on_select=select(
                                    "new-binder",
                                )>"New binder inside…"</ContextMenuItem>
                                <ContextMenuItem on_select=select(
                                    "new-deck",
                                )>"New deck inside…"</ContextMenuItem>
                                <Separator class="my-1" />
                                <ContextMenuItem on_select=select("move")>"Move to…"</ContextMenuItem>
                                <ContextMenuItem on_select=select("rename")>"Rename…"</ContextMenuItem>
                                <ContextMenuItem
                                    class="text-destructive hover:bg-destructive/10 hover:text-destructive"
                                    on_select=select("delete")
                                >
                                    "Delete…"
                                </ContextMenuItem>
                            </ContextMenuContent>
                        </ContextMenu>
                    </div>
                </div>
            </div>
            <p class="text-muted-foreground text-xs">
                // `aim` runs before the panel opens, so a probe can tell "the
                // button was pressed" from "the panel happened to be up".
                "Aimed " <span data-bench-kebab-aimed>{move || aimed.get()}</span>
                " time(s) · last selected: "
                <span data-bench-kebab-last>{move || last.get()}</span>
            </p>
        </div>
    }
    .into_any()
}
