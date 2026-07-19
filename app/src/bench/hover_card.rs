//! Bench section for the vendored `hover_card` (Leptos hover-intent timers).

use leptos::prelude::*;

use crate::components::ui::hover_card::{HoverCard, HoverCardContent, HoverCardTrigger};

pub fn demo() -> AnyView {
    view! {
        <div class="flex items-center gap-2 text-sm">
            "Hover "
            <HoverCard id="bench-hovercard">
                <HoverCardTrigger class="underline decoration-dotted cursor-help">
                    "Lightning Bolt"
                </HoverCardTrigger>
                <HoverCardContent>
                    <p class="font-medium">"Lightning Bolt"</p>
                    <p class="text-muted-foreground">"Instant — {R}"</p>
                    <p class="mt-2">"Lightning Bolt deals 3 damage to any target."</p>
                </HoverCardContent>
            </HoverCard>
            " for the preview (150 ms intent delay)."
        </div>
    }
    .into_any()
}
