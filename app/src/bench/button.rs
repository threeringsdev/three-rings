//! Bench section for the vendored `button`.

use leptos::prelude::*;

use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};

pub fn demo() -> AnyView {
    view! {
        <div class="space-y-4">
            <div class="flex flex-wrap items-center gap-2">
                <Button>"Default"</Button>
                <Button variant=ButtonVariant::Secondary>"Secondary"</Button>
                <Button variant=ButtonVariant::Outline>"Outline"</Button>
                <Button variant=ButtonVariant::Ghost>"Ghost"</Button>
                <Button variant=ButtonVariant::Accent>"Accent"</Button>
                <Button variant=ButtonVariant::Destructive>"Destructive"</Button>
                <Button variant=ButtonVariant::Link>"Link"</Button>
            </div>
            <div class="flex flex-wrap items-center gap-2">
                <Button size=ButtonSize::Lg>"Large"</Button>
                <Button size=ButtonSize::Sm>"Small"</Button>
                <Button size=ButtonSize::Icon attr:aria-label="icon button">
                    "+"
                </Button>
                <Button size=ButtonSize::IconSm attr:aria-label="small icon button">
                    "+"
                </Button>
                <Button attr:disabled=true>
                    "Disabled"
                </Button>
            </div>
        </div>
    }
    .into_any()
}
