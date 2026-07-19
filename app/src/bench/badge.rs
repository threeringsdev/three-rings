//! Bench section for the vendored `badge`.

use leptos::prelude::*;

use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};

pub fn demo() -> AnyView {
    view! {
        <div class="flex flex-wrap items-center gap-2">
            <Badge>"12"</Badge>
            <Badge variant=BadgeVariant::Secondary>"Secondary"</Badge>
            <Badge variant=BadgeVariant::Accent>"Accent"</Badge>
            <Badge variant=BadgeVariant::Muted>"Muted"</Badge>
            <Badge variant=BadgeVariant::Destructive>"2 to buy"</Badge>
            <Badge variant=BadgeVariant::Outline>"Outline"</Badge>
            <Badge size=BadgeSize::Sm variant=BadgeVariant::Muted>"×1"</Badge>
            <Badge size=BadgeSize::Lg>"6 missing"</Badge>
        </div>
    }
    .into_any()
}
