//! Bench section for the vendored `hover_card` (Leptos hover-intent timers).

use leptos::prelude::*;

use crate::components::ui::hover_card::{HoverCard, HoverCardContent, HoverCardTrigger};

pub fn demo() -> AnyView {
    let disabled = RwSignal::new(false);
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

        // The `disabled` deviation (card-detail task): callers that offer a
        // different affordance on touch suppress the hover preview rather than
        // showing both. Toggling it while the card is open must close it.
        <div class="mt-4 flex items-center gap-2 text-sm">
            <button
                class="rounded border px-2 py-1"
                data-testid="bench-hovercard-disable"
                on:click=move |_| disabled.update(|d| *d = !*d)
            >
                {move || if disabled.get() { "Enable" } else { "Disable" }}
            </button>
            "Hover "
            <HoverCard id="bench-hovercard-disabled" disabled=disabled>
                <HoverCardTrigger class="underline decoration-dotted cursor-help">
                    <span id="bench-hovercard-disabled-anchor">"Counterspell"</span>
                </HoverCardTrigger>
                <HoverCardContent {..} data-testid="bench-hovercard-disabled-content">
                    <p class="font-medium">"Counterspell"</p>
                </HoverCardContent>
            </HoverCard>
            <span data-testid="bench-hovercard-disabled-state">
                {move || if disabled.get() { "disabled" } else { "enabled" }}
            </span>
        </div>
    }
    .into_any()
}
