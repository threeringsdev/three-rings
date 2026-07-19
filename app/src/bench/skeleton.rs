//! Bench section for the vendored `skeleton`.

use leptos::prelude::*;

use crate::components::ui::skeleton::Skeleton;

pub fn demo() -> AnyView {
    view! {
        <div class="flex max-w-sm items-start gap-4">
            <Skeleton class="h-24 w-16" />
            <div class="flex-1 space-y-2">
                <Skeleton class="h-4 w-3/4" />
                <Skeleton class="h-3 w-1/2" />
                <Skeleton class="h-3 w-2/3" />
            </div>
        </div>
    }
    .into_any()
}
