//! ToggleGroup — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/toggle_group.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `leptos_ui`'s `clx!` swapped for the vendored clx.rs (nightly hazard)
//! - the unused `ToggleGroupAction` anchor variant dropped (upstream ships
//!   an `<a>`-based row nothing here composes; the button item is the API)

use leptos::prelude::*;
use tw_merge::tw_merge;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ToggleGroupVariant {
    #[default]
    Default,
    Outline,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ToggleGroupOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Default)]
struct ToggleGroupCtx {
    variant: ToggleGroupVariant,
    orientation: ToggleGroupOrientation,
    spacing: i32,
}

#[component]
pub fn ToggleGroup(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(optional)] variant: ToggleGroupVariant,
    #[prop(optional)] orientation: ToggleGroupOrientation,
    #[prop(optional, default = 1i32)] spacing: i32,
) -> impl IntoView {
    provide_context(ToggleGroupCtx {
        variant,
        orientation,
        spacing,
    });

    let is_vertical = orientation == ToggleGroupOrientation::Vertical;

    let gap_style = if spacing == 0 {
        "gap: 0px".to_string()
    } else {
        format!("gap: {}rem", spacing as f64 * 0.25)
    };

    let class = tw_merge!(
        "flex items-center rounded-md group/toggle-group w-fit",
        if is_vertical { "flex-col" } else { "" },
        if variant == ToggleGroupVariant::Outline {
            "shadow-xs"
        } else {
            ""
        },
        class
    );

    view! {
        <div
            class=class
            style=gap_style
            data-variant=if variant == ToggleGroupVariant::Outline { "Outline" } else { "Default" }
            data-orientation=if is_vertical { "Vertical" } else { "Horizontal" }
            data-spacing=spacing.to_string()
        >
            {children()}
        </div>
    }
}

#[component]
pub fn ToggleGroupItem(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(into)] title: String,
    #[prop(optional, into)] pressed: Signal<bool>,
    /// Roving-focus tab stop. A `radiogroup` is one tab stop: the selected item
    /// carries `0`, the rest `-1` (the upstream default, kept when unset).
    #[prop(into, default = Signal::from(-1))]
    tabindex: Signal<i32>,
) -> impl IntoView {
    let ctx = use_context::<ToggleGroupCtx>().unwrap_or_default();

    let is_vertical = ctx.orientation == ToggleGroupOrientation::Vertical;
    let is_grouped = ctx.spacing == 0;
    let is_outline = ctx.variant == ToggleGroupVariant::Outline;

    let rounded = match (is_grouped, is_vertical) {
        (true, true) => "rounded-none first:rounded-t-md last:rounded-b-md",
        (true, false) => "rounded-none first:rounded-l-md last:rounded-r-md",
        (false, _) => "rounded-md",
    };

    let border = if is_outline && is_grouped {
        if is_vertical {
            "border border-t-0 first:border-t"
        } else {
            "border border-l-0 first:border-l"
        }
    } else if is_outline {
        "border"
    } else {
        ""
    };

    let width = if is_vertical { "w-full" } else { "" };

    let merged_class = tw_merge!(
        "inline-flex flex-1 gap-2 justify-center items-center px-2 min-w-0 h-9 text-sm font-medium whitespace-nowrap bg-transparent shadow-none outline-none focus:z-10 focus-visible:z-10 disabled:opacity-50 disabled:pointer-events-none data-[state=on]:bg-accent data-[state=on]:text-accent-foreground [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 [&_svg]:shrink-0 transition-[color,box-shadow] aria-invalid:ring-destructive/20 aria-invalid:border-destructive shrink-0 dark:aria-invalid:ring-destructive/40 hover:bg-muted hover:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]",
        rounded,
        border,
        width,
        class
    );

    view! {
        // Deviations from upstream: `aria-checked` added (upstream ships
        // role="radio" with no checked state exposed), and `tabindex` opened
        // as a prop so callers can drive roving focus. The arrow-key handling
        // itself stays feature-side (the group owns which item is selected) —
        // see catalog.rs's view switch for the reference wiring.
        <button
            type="button"
            data-name="ToggleGroupItem"
            class=merged_class
            role="radio"
            tabindex=move || tabindex.get()
            title=title
            data-state=move || if pressed.get() { "on" } else { "off" }
            aria-checked=move || pressed.get().to_string()
        >
            {children()}
        </button>
    }
}
