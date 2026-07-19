//! Bench section for the vendored `checkbox` (+ `label`, its constant pair).

use leptos::prelude::*;

use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::label::Label;

pub fn demo() -> AnyView {
    let (checked, set_checked) = signal(true);

    view! {
        <div class="space-y-3">
            <div class="flex items-center gap-2">
                <Checkbox
                    checked=checked
                    on_checked_change=Callback::new(move |v| set_checked.set(v))
                    aria_label="Rare"
                    attr:id="bench-rare-checkbox"
                />
                <Label html_for="bench-rare-checkbox">"Rare"</Label>
                <span class="text-muted-foreground text-xs">
                    {move || if checked.get() { "(checked)" } else { "(unchecked)" }}
                </span>
            </div>
            <div class="flex items-center gap-2">
                <Checkbox disabled=Signal::from(true) aria_label="Disabled option" />
                <Label class="opacity-50">"Disabled option"</Label>
            </div>
        </div>
    }
    .into_any()
}
