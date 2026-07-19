//! Button — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/button.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `variants!` (leptos_ui, nightly hazard) hand-expanded into plain enums +
//!   match — same class strings, no `paste`/derive machinery
//! - `Warning`/`Success`/`Bordered` variants dropped (they reference
//!   `warning`/`success` tokens style/input.css doesn't define — Tailwind
//!   would emit no CSS) along with the unused `Mobile`/`Badge` sizes
//! - `support_href` dropped: this renders a real `<button>`; link-styled
//!   navigation uses an `<a>` with `ButtonVariant::Link` classes instead

use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Default,
    Destructive,
    Outline,
    Secondary,
    Ghost,
    Accent,
    Link,
}

impl ButtonVariant {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "bg-primary text-primary-foreground shadow-xs hover:bg-primary/90",
            Self::Destructive => "bg-destructive text-white shadow-xs hover:bg-destructive/90 focus-visible:ring-destructive/20 dark:focus-visible:ring-destructive/40 dark:bg-destructive/60",
            Self::Outline => "border bg-background shadow-xs hover:bg-accent hover:text-accent-foreground dark:bg-input/30 dark:border-input dark:hover:bg-input/5",
            Self::Secondary => "bg-secondary text-secondary-foreground shadow-xs hover:bg-secondary/80",
            Self::Ghost => "hover:bg-accent hover:text-accent-foreground dark:hover:bg-accent/50",
            Self::Accent => "bg-accent text-accent-foreground hover:bg-accent/80",
            Self::Link => "text-primary underline-offset-4 hover:underline",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    #[default]
    Default,
    Sm,
    Lg,
    Icon,
    IconSm,
    IconXs,
}

impl ButtonSize {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "h-9 px-4 py-2 has-[>svg]:px-3",
            Self::Sm => "h-8 rounded-md gap-1.5 px-3 has-[>svg]:px-2.5",
            Self::Lg => "h-10 rounded-md px-6 has-[>svg]:px-4",
            Self::Icon => "size-9",
            Self::IconSm => "size-8 rounded-md",
            Self::IconXs => "size-6 rounded-md",
        }
    }
}

const BUTTON_BASE: &str = "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive w-fit hover:cursor-pointer active:scale-[0.98] active:opacity-100 touch-manipulation [-webkit-tap-highlight-color:transparent] select-none [-webkit-touch-callout:none]";

#[component]
pub fn Button(
    #[prop(optional)] variant: ButtonVariant,
    #[prop(optional)] size: ButtonSize,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let merged = tw_merge::tw_merge!(BUTTON_BASE, variant.classes(), size.classes(), class);

    view! {
        <button type="button" class=merged data-name="Button">
            {children()}
        </button>
    }
}
