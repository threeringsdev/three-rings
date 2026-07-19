//! Bench section for the vendored `popover` (native Popover API + CSS
//! anchor positioning — the section to eyeball on native webviews).

use leptos::prelude::*;

use crate::components::ui::popover::{
    Popover, PopoverContent, PopoverDescription, PopoverTitle, PopoverTrigger,
};

pub fn demo() -> AnyView {
    view! {
        <div class="flex flex-wrap items-center gap-4">
            <Popover id="bench-popover">
                <PopoverTrigger>"Adding to: 📥 Inbox ▾"</PopoverTrigger>
                <PopoverContent>
                    <PopoverTitle>"Destination"</PopoverTitle>
                    <PopoverDescription>
                        "Anchor-positioned above/below the trigger; light-dismiss on outside click."
                    </PopoverDescription>
                </PopoverContent>
            </Popover>
        </div>
    }
    .into_any()
}
