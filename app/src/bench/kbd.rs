//! Bench section for the vendored `kbd`.

use leptos::prelude::*;

use crate::components::ui::kbd::{Kbd, KbdGroup};

pub fn demo() -> AnyView {
    view! {
        <div class="flex flex-wrap items-center gap-4 text-sm">
            <span class="text-muted-foreground">
                "Focus search " <Kbd>"/"</Kbd>
            </span>
            <KbdGroup>
                <Kbd>"↑"</Kbd>
                <Kbd>"↓"</Kbd>
                <Kbd>"⏎"</Kbd>
                <Kbd>"⇧⏎"</Kbd>
                <Kbd>"⌥⏎"</Kbd>
            </KbdGroup>
            <span class="text-muted-foreground">
                "Palette " <KbdGroup><Kbd>"⌘"</Kbd><Kbd>"K"</Kbd></KbdGroup>
            </span>
        </div>
    }
    .into_any()
}
