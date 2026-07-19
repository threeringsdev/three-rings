//! Bench section for the vendored `separator`.

use leptos::prelude::*;

use crate::components::ui::separator::{Separator, SeparatorOrientation};

pub fn demo() -> AnyView {
    view! {
        <div class="max-w-sm space-y-4">
            <p class="text-sm">"Above"</p>
            <Separator />
            <p class="text-sm">"Below"</p>
            <div class="flex h-6 items-center gap-3 text-sm">
                <span>"Catalog"</span>
                <Separator orientation=SeparatorOrientation::Vertical />
                <span>"My cards"</span>
            </div>
        </div>
    }
    .into_any()
}
