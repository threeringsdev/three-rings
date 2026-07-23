//! Badge — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/badge.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `variants!` (leptos_ui, nightly hazard) hand-expanded into plain enums

use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeVariant {
    #[default]
    Default,
    Secondary,
    Accent,
    Muted,
    Destructive,
    Outline,
    Success,
    Warning,
    Info,
}

impl BadgeVariant {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "border-transparent shadow bg-primary text-primary-foreground hover:bg-primary/80",
            Self::Secondary => "border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80",
            Self::Accent => "border-transparent bg-accent text-accent-foreground hover:bg-accent/80",
            Self::Muted => "border-transparent bg-muted text-muted-foreground hover:bg-muted/80",
            Self::Destructive => "border-transparent shadow bg-destructive text-destructive-foreground hover:bg-destructive/80",
            Self::Outline => "text-foreground",
            Self::Success => "border-transparent bg-success-light text-success-dark hover:bg-success-light/80",
            Self::Warning => "border-transparent bg-warning-light text-warning-dark hover:bg-warning-light/80",
            Self::Info => "border-transparent bg-info-light text-info-dark hover:bg-info-light/80",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeSize {
    #[default]
    Default,
    Sm,
    Lg,
}

impl BadgeSize {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "px-2.5 py-0.5 text-xs",
            Self::Sm => "px-1.5 py-0.5 text-[10px]",
            Self::Lg => "px-3 py-1 text-sm",
        }
    }
}

const BADGE_BASE: &str = "inline-flex items-center font-semibold rounded-md border transition-colors focus:outline-hidden focus:ring-2 focus:ring-ring focus:ring-offset-2 w-fit";

#[component]
pub fn Badge(
    #[prop(optional)] variant: BadgeVariant,
    #[prop(optional)] size: BadgeSize,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let merged = tw_merge::tw_merge!(BADGE_BASE, variant.classes(), size.classes(), class);

    view! {
        <span class=merged data-name="Badge">
            {children()}
        </span>
    }
}
