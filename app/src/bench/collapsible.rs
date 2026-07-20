//! Bench section for the vendored `collapsible`.

use leptos::prelude::*;

use crate::components::ui::collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger};

pub fn demo() -> AnyView {
    view! {
        <div class="flex max-w-xs flex-col gap-4">
            <Collapsible default_open=true content_id="bench-collapsible-open">
                <CollapsibleTrigger class="flex w-full items-center gap-2 text-sm font-medium">
                    <span class="transition-transform duration-200 [[data-state=open]>&]:rotate-90">
                        "›"
                    </span>
                    "Default open"
                </CollapsibleTrigger>
                <CollapsibleContent class="text-muted-foreground pl-6 text-sm">
                    <p>"Visible until collapsed — the tree's per-node body."</p>
                </CollapsibleContent>
            </Collapsible>
            <Collapsible content_id="bench-collapsible-closed">
                <CollapsibleTrigger class="flex w-full items-center gap-2 text-sm font-medium">
                    <span class="transition-transform duration-200 [[data-state=open]>&]:rotate-90">
                        "›"
                    </span>
                    "Default closed"
                </CollapsibleTrigger>
                <CollapsibleContent class="pl-6 text-sm">
                    <a href="#collapsible" class="underline">
                        "A link that must be unreachable (inert) while closed"
                    </a>
                </CollapsibleContent>
            </Collapsible>
        </div>
    }
    .into_any()
}
