//! Item — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/item.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `variants!` (leptos_ui, nightly hazard) hand-expanded into plain enums
//! - `support_href` expanded by hand: an `href` prop renders an `<a>`, and
//!   upstream's `[a]:`-arbitrary-variant hover classes (`[a]:hover:bg-accent/50`,
//!   `[a]:transition-colors`) become plain utilities on that arm — the `[a]:`
//!   form resolves to no usable CSS under our Tailwind v4 setup
//! - `ItemMedia`'s single-value `size` axis dropped

use leptos::prelude::*;

use super::clx::clx;

clx! {ItemGroup, div, "group/item-group flex flex-col"}
clx! {ItemContent, div, "flex flex-1 flex-col gap-1 [&+[data-slot=item-content]]:flex-none"}
clx! {ItemTitle, div, "flex w-fit items-center gap-2 text-sm leading-snug font-medium"}
clx! {ItemDescription, p, "text-muted-foreground line-clamp-2 text-sm leading-normal font-normal text-balance [&>a:hover]:text-primary [&>a]:underline [&>a]:underline-offset-4"}
clx! {ItemActions, div, "flex items-center gap-2"}
clx! {ItemHeader, div, "flex basis-full items-center justify-between gap-2"}
clx! {ItemFooter, div, "flex basis-full items-center justify-between gap-2"}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemVariant {
    #[default]
    Default,
    Outline,
    Muted,
}

impl ItemVariant {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "bg-transparent",
            Self::Outline => "border-border",
            Self::Muted => "bg-muted/50",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemSize {
    #[default]
    Default,
    Sm,
    Xs,
}

impl ItemSize {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "p-4 gap-4",
            Self::Sm => "py-3 px-4 gap-2.5",
            Self::Xs => "py-2 px-3 gap-2",
        }
    }
}

const ITEM_BASE: &str = "group/item flex items-center border border-transparent text-sm rounded-md transition-colors duration-100 flex-wrap outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]";

#[component]
pub fn Item(
    #[prop(optional)] variant: ItemVariant,
    #[prop(optional)] size: ItemSize,
    /// Render as a link; hover feedback rides this arm (see header deviation).
    #[prop(into, optional)]
    href: Option<String>,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let merged = tw_merge::tw_merge!(ITEM_BASE, variant.classes(), size.classes(), class);
    match href {
        Some(href) => view! {
            <a
                href=href
                class=tw_merge::tw_merge!(&merged, "hover:bg-accent/50")
                data-name="Item"
            >
                {children()}
            </a>
        }
        .into_any(),
        None => view! {
            <div class=merged data-name="Item">
                {children()}
            </div>
        }
        .into_any(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemMediaVariant {
    #[default]
    Default,
    Icon,
    Image,
}

impl ItemMediaVariant {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "bg-transparent",
            Self::Icon => "size-8 border rounded-sm bg-muted [&_svg:not([class*='size-'])]:size-4",
            Self::Image => {
                "size-10 rounded-sm overflow-hidden [&_img]:size-full [&_img]:object-cover"
            }
        }
    }
}

const ITEM_MEDIA_BASE: &str = "flex shrink-0 items-center justify-center gap-2 group-has-[[data-slot=item-description]]/item:self-start [&_svg]:pointer-events-none group-has-[[data-slot=item-description]]/item:translate-y-0.5";

#[component]
pub fn ItemMedia(
    #[prop(optional)] variant: ItemMediaVariant,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let merged = tw_merge::tw_merge!(ITEM_MEDIA_BASE, variant.classes(), class);
    view! {
        <div class=merged data-name="ItemMedia">
            {children()}
        </div>
    }
}

#[component]
pub fn ItemSeparator(#[prop(into, optional)] class: String) -> impl IntoView {
    let merged_class = tw_merge::tw_merge!("my-0", class);

    view! { <super::separator::Separator attr:data-name="ItemSeparator" class=merged_class /> }
}
