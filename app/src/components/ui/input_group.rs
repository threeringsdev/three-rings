//! InputGroup — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/input_group.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `variants!`/`TwClass`/`TwVariant` (leptos_ui + derive machinery)
//!   hand-expanded into plain enums + match, same class strings
//! - `InputGroupTextarea` dropped (textarea is not vendored; nothing in the
//!   wireframes needs a grouped textarea)

use leptos::prelude::*;
use tw_merge::tw_merge;

use super::clx::clx;
use super::input::{Input, InputType};

mod components {
    use super::*;
    clx! {InputGroupText, span, "text-muted-foreground flex items-center gap-2 text-sm [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4"}
}

pub use components::*;

#[component]
pub fn InputGroup(#[prop(into, optional)] class: String, children: Children) -> impl IntoView {
    let merged_class = tw_merge!(
        "group/input-group border-input dark:bg-input/30 relative flex w-full items-center rounded-md border shadow-xs transition-[color,box-shadow] outline-none h-9 min-w-0 has-[>textarea]:h-auto has-[>[data-align=inline-start]]:[&>input]:pl-2 has-[>[data-align=inline-end]]:[&>input]:pr-2 has-[>[data-align=block-start]]:h-auto has-[>[data-align=block-start]]:flex-col has-[>[data-align=block-start]]:[&>input]:pb-3 has-[>[data-align=block-end]]:h-auto has-[>[data-align=block-end]]:flex-col has-[>[data-align=block-end]]:[&>input]:pt-3 has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-ring/50 has-[[data-slot=input-group-control]:focus-visible]:ring-[3px] has-[[data-slot][aria-invalid=true]]:ring-destructive/20 has-[[data-slot][aria-invalid=true]]:border-destructive dark:has-[[data-slot][aria-invalid=true]]:ring-destructive/40",
        class
    );

    view! {
        <div data-name="InputGroup" data-slot="input-group" role="group" class=merged_class>
            {children()}
        </div>
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum InputGroupAddonAlign {
    #[default]
    InlineStart,
    InlineEnd,
    BlockStart,
    BlockEnd,
}

impl InputGroupAddonAlign {
    fn classes(self) -> &'static str {
        match self {
            Self::InlineStart => "order-first pl-3 has-[>button]:ml-[-0.45rem] has-[>kbd]:ml-[-0.35rem]",
            Self::InlineEnd => "order-last pr-3 has-[>button]:mr-[-0.45rem] has-[>kbd]:mr-[-0.35rem]",
            Self::BlockStart => "order-first w-full justify-start px-3 pt-3 [.border-b]:pb-3 group-has-[>input]/input-group:pt-2.5",
            Self::BlockEnd => "order-last w-full justify-start px-3 pb-3 [.border-t]:pt-3 group-has-[>input]/input-group:pb-2.5",
        }
    }

    fn attr(self) -> &'static str {
        match self {
            Self::InlineStart => "inline-start",
            Self::InlineEnd => "inline-end",
            Self::BlockStart => "block-start",
            Self::BlockEnd => "block-end",
        }
    }
}

const ADDON_BASE: &str = "text-muted-foreground flex h-auto cursor-text items-center justify-center gap-2 py-1.5 text-sm font-medium select-none [&>svg:not([class*='size-'])]:size-4 [&>kbd]:rounded-[calc(var(--radius)-5px)] group-data-[disabled=true]/input-group:opacity-50";

#[component]
pub fn InputGroupAddon(
    #[prop(optional)] align: InputGroupAddonAlign,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let merged_class = tw_merge!(ADDON_BASE, align.classes(), class);

    view! {
        <div
            data-name="InputGroupAddon"
            data-slot="input-group-addon"
            data-align=align.attr()
            role="group"
            class=merged_class
        >
            {children()}
        </div>
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum InputGroupButtonSize {
    #[default]
    Xs,
    Sm,
    IconXs,
    IconSm,
}

impl InputGroupButtonSize {
    fn classes(self) -> &'static str {
        match self {
            Self::Xs => "h-6 gap-1 px-2 rounded-[calc(var(--radius)-5px)] [&>svg:not([class*='size-'])]:size-3.5 has-[>svg]:px-2",
            Self::Sm => "h-8 px-2.5 gap-1.5 rounded-md has-[>svg]:px-2.5",
            Self::IconXs => "size-6 rounded-[calc(var(--radius)-5px)] p-0 has-[>svg]:p-0",
            Self::IconSm => "size-8 p-0 has-[>svg]:p-0",
        }
    }
}

/// Upstream's `InputGroupButton` has a single `Ghost` variant; the ghost
/// hover/active treatment comes from the button base below.
#[component]
pub fn InputGroupButton(
    #[prop(optional)] size: InputGroupButtonSize,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let merged = tw_merge!(
        "text-sm shadow-none flex gap-2 items-center hover:bg-accent hover:text-accent-foreground rounded-md transition-all outline-none disabled:pointer-events-none disabled:opacity-50 hover:cursor-pointer",
        size.classes(),
        class
    );

    view! {
        <button type="button" data-name="InputGroupButton" class=merged>
            {children()}
        </button>
    }
}

/// The group's input control — our [`Input`] restyled flat for the group box.
#[component]
pub fn InputGroupInput(
    #[prop(into, optional)] class: String,
    #[prop(default = InputType::default())] r#type: InputType,
    #[prop(into, optional)] placeholder: String,
    #[prop(into, optional)] name: String,
    #[prop(into, optional)] id: String,
    #[prop(optional)] disabled: bool,
    #[prop(optional)] readonly: bool,
    #[prop(optional)] required: bool,
    #[prop(into, optional)] bind_value: Option<RwSignal<String>>,
) -> impl IntoView {
    let merged_class = tw_merge!(
        "flex-1 rounded-none border-0 bg-transparent shadow-none focus-visible:ring-0 dark:bg-transparent",
        class
    );

    match bind_value {
        Some(signal) => view! {
            <Input
                attr:data-slot="input-group-control"
                class=merged_class
                r#type=r#type
                placeholder=placeholder
                name=name
                id=id
                disabled=disabled
                readonly=readonly
                required=required
                bind_value=signal
            />
        }
        .into_any(),
        None => view! {
            <Input
                attr:data-slot="input-group-control"
                class=merged_class
                r#type=r#type
                placeholder=placeholder
                name=name
                id=id
                disabled=disabled
                readonly=readonly
                required=required
            />
        }
        .into_any(),
    }
}
