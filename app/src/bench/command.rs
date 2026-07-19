//! Bench section for the vendored `command` (reactive filter + ↑↓/⏎ nav).

use leptos::prelude::*;

use crate::components::ui::command::{
    Command, CommandEmpty, CommandGroup, CommandGroupLabel, CommandInput, CommandItem, CommandList,
};

pub fn demo() -> AnyView {
    let (picked, set_picked) = signal(String::new());
    let places = [
        "Inbox",
        "Trade Binder",
        "Shoebox",
        "Rares",
        "Commander Deck",
    ];

    view! {
        <div class="max-w-sm space-y-2">
            // Command provides the context CommandInput/Item/Empty read.
            <Command class="min-h-0 rounded-md border">
                <div class="border-b px-2">
                    <CommandInput placeholder="Filter places… (↑↓ then ⏎)" />
                </div>
                <CommandList class="min-h-0 max-h-60 p-1">
                    <CommandGroup>
                        <CommandGroupLabel>"Places"</CommandGroupLabel>
                        {places
                            .into_iter()
                            .map(|p| {
                                view! {
                                    <CommandItem
                                        value=p
                                        on_select=Callback::new(move |_| set_picked.set(p.to_string()))
                                    >
                                        {p}
                                    </CommandItem>
                                }
                            })
                            .collect_view()}
                    </CommandGroup>
                    <CommandEmpty>"No places found."</CommandEmpty>
                </CommandList>
            </Command>
            <p class="text-muted-foreground text-xs">"picked: " {move || picked.get()}</p>
        </div>
    }
    .into_any()
}
