//! Bench section for the vendored `command` (reactive filter + ↑↓/⏎ nav), plus
//! the `use_command_nav` deviation quick-add needs: an input the *feature* owns
//! driving the same item registry, with modifiers of its own.

use leptos::prelude::*;

use crate::components::ui::command::{
    use_command_nav, Command, CommandEmpty, CommandGroup, CommandGroupLabel, CommandInput,
    CommandItem, CommandList,
};

pub fn demo() -> AnyView {
    // Two wrappers with ids, because both halves mount `CommandItem`s and the
    // bench probe has to scope its assertions to one list at a time.
    view! {
        <div class="flex flex-wrap items-start gap-8">
            <div id="bench-command-classic">{input_demo()}</div>
            <div id="bench-command-nav">{foreign_input_demo()}</div>
        </div>
    }
    .into_any()
}

fn input_demo() -> AnyView {
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
            <p class="text-muted-foreground text-xs" data-testid="bench-command-picked">
                "picked: "
                {move || picked.get()}
            </p>
        </div>
    }
    .into_any()
}

/// The foreign-input path: no `CommandInput` at all — a plain `<input>` outside
/// the list drives ↑↓/⏎ through `use_command_nav`, and reads a modifier the
/// primitive's own handler never sees (⌥⏎ here picks the row in caps).
fn foreign_input_demo() -> AnyView {
    view! {
        <Command should_filter=false class="h-auto max-w-sm space-y-2 overflow-visible bg-transparent">
            <ForeignNavDemo />
        </Command>
    }
    .into_any()
}

#[component]
fn ForeignNavDemo() -> impl IntoView {
    let (picked, set_picked) = signal(String::new());
    let nav = use_command_nav().expect("inside a Command");
    let highlighted = nav.highlighted();
    let places = ["Inbox", "Trade Binder", "Shoebox"];
    // Which modifier the *last* activation carried — a caller-owned key contract
    // layered over the registry, which is the whole point of the deviation.
    let shout = RwSignal::new(false);

    view! {
        <div class="space-y-2">
            <input
                id="bench-command-foreign"
                class="border-input bg-background h-9 w-full rounded-md border px-3 text-sm"
                placeholder="↑↓ then ⏎ (⌥⏎ shouts)"
                autocomplete="off"
                on:keydown=move |ev| {
                    match ev.key().as_str() {
                        "ArrowDown" => {
                            ev.prevent_default();
                            nav.next();
                        }
                        "ArrowUp" => {
                            ev.prevent_default();
                            nav.prev();
                        }
                        "Enter" => {
                            ev.prevent_default();
                            shout.set(ev.alt_key());
                            nav.activate();
                        }
                        _ => {}
                    }
                }
            />
            <CommandList class="min-h-0 rounded-md border p-1">
                {places
                    .into_iter()
                    .map(|p| {
                        view! {
                            <CommandItem
                                value=p
                                on_select=Callback::new(move |_| {
                                    set_picked
                                        .set(
                                            if shout.get_untracked() {
                                                p.to_uppercase()
                                            } else {
                                                p.to_string()
                                            },
                                        )
                                })
                            >
                                <span data-testid="bench-command-foreign-item">{p}</span>
                            </CommandItem>
                        }
                    })
                    .collect_view()}
            </CommandList>
            <p class="text-muted-foreground text-xs" data-testid="bench-command-foreign-state">
                {move || format!("row {} · picked: {}", highlighted.get(), picked.get())}
            </p>
        </div>
    }
}
