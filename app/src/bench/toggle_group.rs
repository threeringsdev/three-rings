//! Bench section for the vendored `toggle_group` (the grid/list + mode switch).

use leptos::prelude::*;

use crate::components::ui::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};

pub fn demo() -> AnyView {
    let (mode, set_mode) = signal("grid");

    view! {
        <div class="space-y-3">
            <ToggleGroup variant=ToggleGroupVariant::Outline spacing=0>
                <ToggleGroupItem
                    title="Grid view"
                    pressed=Signal::derive(move || mode.get() == "grid")
                    {..}
                    on:click=move |_| set_mode.set("grid")
                >
                    "Grid"
                </ToggleGroupItem>
                <ToggleGroupItem
                    title="List view"
                    pressed=Signal::derive(move || mode.get() == "list")
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
