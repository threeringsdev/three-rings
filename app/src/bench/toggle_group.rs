//! Bench section for the vendored `toggle_group` (the grid/list + mode switch).

use leptos::prelude::*;

use crate::components::ui::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};

pub fn demo() -> AnyView {
    let (mode, set_mode) = signal("grid");

    view! {
        <div class="space-y-3">
            // `tabindex` drives roving focus: the group is one tab stop, and
            // the selected item is the one that stops there. The arrow-key
            // handling that moves the selection is feature-side — catalog.rs's
            // ViewSwitch is the reference wiring.
            <ToggleGroup variant=ToggleGroupVariant::Outline spacing=0>
                <ToggleGroupItem
                    title="Grid view"
                    pressed=Signal::derive(move || mode.get() == "grid")
                    tabindex=Signal::derive(move || if mode.get() == "grid" { 0 } else { -1 })
                    {..}
                    on:click=move |_| set_mode.set("grid")
                >
                    "Grid"
                </ToggleGroupItem>
                <ToggleGroupItem
                    title="List view"
                    pressed=Signal::derive(move || mode.get() == "list")
                    tabindex=Signal::derive(move || if mode.get() == "list" { 0 } else { -1 })
                    {..}
                    on:click=move |_| set_mode.set("list")
                >
                    "List"
                </ToggleGroupItem>
            </ToggleGroup>
            <p class="text-muted-foreground text-xs">"mode: " {move || mode.get()}</p>
        </div>
    }
    .into_any()
}
