//! Bench section for the vendored `input_group`.

use leptos::prelude::*;

use crate::components::ui::input_group::{
    InputGroup, InputGroupAddon, InputGroupAddonAlign, InputGroupButton, InputGroupInput,
    InputGroupText,
};
use crate::components::ui::kbd::Kbd;

pub fn demo() -> AnyView {
    view! {
        <div class="max-w-sm space-y-3">
            <InputGroup>
                <InputGroupAddon>
                    <InputGroupText>"🔍"</InputGroupText>
                </InputGroupAddon>
                <InputGroupInput placeholder="Quick search…" />
                <InputGroupAddon align=InputGroupAddonAlign::InlineEnd>
                    <Kbd>"/"</Kbd>
                </InputGroupAddon>
            </InputGroup>
            <InputGroup>
                <InputGroupInput placeholder="Add a card…" />
                <InputGroupAddon align=InputGroupAddonAlign::InlineEnd>
                    <InputGroupButton>"Add"</InputGroupButton>
                </InputGroupAddon>
            </InputGroup>
        </div>
    }
    .into_any()
}
