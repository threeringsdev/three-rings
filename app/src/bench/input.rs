//! Bench section for the vendored `input` (incl. a bound value).

use leptos::prelude::*;

use crate::components::ui::input::{Input, InputType};

pub fn demo() -> AnyView {
    let bound = RwSignal::new(String::from("Lightning Bolt"));

    view! {
        <div class="max-w-sm space-y-3">
            <Input placeholder="Search cards…" r#type=InputType::Search />
            <Input placeholder="you@example.com" r#type=InputType::Email />
            <Input placeholder="disabled" disabled=true />
            <Input bind_value=bound />
            <p class="text-muted-foreground text-xs">
                "bound value: " {move || bound.get()}
            </p>
        </div>
    }
    .into_any()
}
