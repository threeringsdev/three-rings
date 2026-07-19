//! Breadcrumb — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/breadcrumb.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `leptos_ui`'s `clx!` swapped for the vendored clx.rs (nightly hazard)
//! - the `icons` crate's `ChevronRight`/`Ellipsis` replaced with inlined
//!   Lucide paths (ISC) — no icon-crate dependency

use leptos::prelude::*;

use super::clx::clx;

mod components {
    use super::*;
    clx! {Breadcrumb, nav, ""}
    clx! {BreadcrumbList, ol, "flex flex-wrap gap-1 items-center text-sm break-words sm:gap-2 text-muted-foreground"}
    clx! {BreadcrumbItem, li, "inline-flex gap-1 items-center [&_svg:not([class*='size-'])]:size-4"}
    clx! {BreadcrumbLink, a, "transition-colors hover:text-foreground"}
    clx! {RootSeparator, li, "[&>svg]:size-3.5 [&_svg:not([class*='size-'])]:size-4"}
    clx! {RootPage, span, "font-normal text-foreground"}
    clx! {RootEllipsisBtn, button, "flex items-center gap-1"}
    clx! {RootEllipsis, span, "flex items-center justify-center size-4"}
}

pub use components::*;

fn chevron_right() -> impl IntoView {
    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="m9 18 6-6-6-6" />
        </svg>
    }
}

#[component]
pub fn BreadcrumbSeparator(
    #[prop(into, optional)] class: String,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <RootSeparator class=class attr:role="presentation" attr:aria-hidden="true">
            {match children {
                Some(c) => c().into_any(),
                None => chevron_right().into_any(),
            }}
        </RootSeparator>
    }
}

#[component]
pub fn BreadcrumbPage(#[prop(into, optional)] class: String, children: Children) -> impl IntoView {
    view! {
        <RootPage class=class attr:role="link" attr:aria-disabled="true" attr:aria-current="page">
            {children()}
        </RootPage>
    }
}

#[component]
pub fn BreadcrumbEllipsis(#[prop(into, optional)] class: String) -> impl IntoView {
    view! {
        <RootEllipsisBtn attr:aria-haspopup="menu" attr:aria-expanded="false" attr:data-state="closed">
            <RootEllipsis attr:role="presentation" attr:aria-hidden="true">
                <svg
                    class=class
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <circle cx="12" cy="12" r="1" />
                    <circle cx="19" cy="12" r="1" />
                    <circle cx="5" cy="12" r="1" />
                </svg>
                <span class="hidden">More</span>
            </RootEllipsis>
            <span class="hidden">Toggle menu</span>
        </RootEllipsisBtn>
    }
}
