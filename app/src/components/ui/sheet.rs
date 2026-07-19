//! Sheet — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/sheet.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Markup/CSS vendored; behavior rewired
//! (specs/app-ui.md → "vendor markup + CSS, rewire behavior in Leptos").
//! Deviations from upstream:
//! - **deterministic caller-supplied `id`** replaces `use_random_id_for`
//! - **Leptos-owned open state** replaces the inline vanilla-`<script>`;
//!   the open/closed transform is a reactive class, not classList mutation
//! - **ESC listener cleanup** via `window_event_listener` (upstream leaked
//!   a per-instance `document` listener)
//! - scroll locking calls the vendored Rust [`super::scroll_lock`] directly
//! - `strum` derives on the direction enum hand-written; the `icons` `X`
//!   inlined (Lucide, ISC)
//! - the mobile chrome (grabber, drag-to-dismiss, snap heights) is ours on
//!   top later, per the gap analysis

use leptos::context::Provider;
use leptos::prelude::*;
use tw_merge::tw_merge;

use super::button::{Button, ButtonSize, ButtonVariant};
use super::clx::clx;

mod components {
    use super::*;
    clx! {SheetHeader, div, "flex flex-col gap-0.5 p-4"}
    clx! {SheetTitle, h2, "font-bold text-2xl"}
    clx! {SheetDescription, p, "text-muted-foreground"}
    clx! {SheetBody, div, "flex flex-col gap-4"}
    clx! {SheetFooter, footer, "mt-auto flex flex-col gap-2 p-4"}
}

pub use components::*;

#[derive(Clone)]
struct SheetContext {
    id: String,
    open: RwSignal<bool>,
}

/// The sheet's open signal, for custom triggers/closers inside a `<Sheet>`.
pub fn use_sheet_open() -> Option<RwSignal<bool>> {
    use_context::<SheetContext>().map(|c| c.open)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SheetDirection {
    Right,
    Left,
    Top,
    Bottom,
}

impl SheetDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Right => "Right",
            Self::Left => "Left",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }

    fn closed_class(self) -> &'static str {
        match self {
            Self::Right => "translate-x-full",
            Self::Left => "-translate-x-full",
            Self::Top => "-translate-y-full",
            Self::Bottom => "translate-y-full",
        }
    }

    fn initial_position(self) -> &'static str {
        match self {
            Self::Right => "top-0 right-0 h-full w-[400px]",
            Self::Left => "top-0 left-0 h-full w-[400px]",
            Self::Top => "top-0 left-0 w-full h-[400px]",
            Self::Bottom => "bottom-0 left-0 w-full h-[400px]",
        }
    }
}

#[component]
pub fn Sheet(
    /// Deterministic instance id — SSR and hydration must agree on it.
    #[prop(into)]
    id: String,
    /// Share the open state with the caller; omitted = internal state.
    #[prop(optional)]
    open: Option<RwSignal<bool>>,
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let ctx = SheetContext { id, open };

    view! {
        <Provider value=ctx>
            <div data-name="Sheet" class=class>
                {children()}
            </div>
        </Provider>
    }
}

#[component]
pub fn SheetTrigger(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Default)] size: ButtonSize,
) -> impl IntoView {
    let ctx = expect_context::<SheetContext>();
    let open = ctx.open;

    view! {
        <Button
            class=class
            attr:id=format!("trigger_{}", ctx.id)
            variant=variant
            size=size
            on:click=move |_| open.set(true)
        >
            {children()}
        </Button>
    }
}

#[component]
pub fn SheetClose(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Default)] size: ButtonSize,
) -> impl IntoView {
    let ctx = expect_context::<SheetContext>();
    let open = ctx.open;

    view! {
        <Button
            class=class
            attr:aria-label="Close sheet"
            variant=variant
            size=size
            on:click=move |_| open.set(false)
        >
            {children()}
        </Button>
    }
}

#[component]
pub fn SheetContent(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = SheetDirection::Right)] direction: SheetDirection,
    #[prop(default = true)] show_close_button: bool,
    /// Accessible name for the sheet (announced by AT alongside "dialog").
    #[prop(optional, into)]
    aria_label: Option<String>,
) -> impl IntoView {
    let ctx = expect_context::<SheetContext>();
    let open = ctx.open;

    let base_class = tw_merge!(
        "fixed z-100 bg-card shadow-lg p-6 transition-transform duration-300 overflow-y-auto overscroll-y-contain data-[state=closed]:pointer-events-none",
        direction.initial_position(),
        class
    );
    // The open/closed transform is reactive — upstream mutated classList
    // from its inline script.
    let panel_class = move || {
        if open.get() {
            format!("{base_class} translate-x-0 translate-y-0")
        } else {
            format!("{base_class} {}", direction.closed_class())
        }
    };

    let state = move || if open.get() { "open" } else { "closed" };

    // Scroll lock + overlay-stack registration follow the open state (the
    // lock is reference-counted); unlock waits out the 300 ms exit animation
    // like upstream.
    let stack_id = ctx.id.clone();
    Effect::new(move |prev: Option<bool>| {
        let now = open.get();
        if now {
            super::scroll_lock::lock();
            super::overlay_stack::push(&stack_id);
        } else if prev == Some(true) {
            super::overlay_stack::remove(&stack_id);
            super::scroll_lock::unlock(300);
        }
        now
    });

    // ESC closes only the topmost open overlay (stack gate); handle removed
    // on cleanup, and an unmount-while-open releases stack + lock.
    let esc_id = ctx.id.clone();
    let esc = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && open.get_untracked() && super::overlay_stack::is_top(&esc_id) {
            ev.prevent_default();
            // Consume ESC so sibling overlay listeners on `window` don't also
            // fire (signal-set can flush the stack removal synchronously,
            // which would otherwise let the next-down overlay close too).
            ev.stop_immediate_propagation();
            open.set(false);
        }
    });
    let unmount_id = ctx.id.clone();
    on_cleanup(move || {
        esc.remove();
        if open.get_untracked() {
            super::overlay_stack::remove(&unmount_id);
            super::scroll_lock::unlock(0);
        }
    });

    view! {
        <div
            data-name="SheetBackdrop"
            id=format!("{}_backdrop", ctx.id)
            class="fixed inset-0 transition-opacity duration-200 z-60 bg-black/50 data-[state=closed]:opacity-0 data-[state=closed]:pointer-events-none data-[state=open]:opacity-100"
            data-state=state
            on:click=move |_| open.set(false)
        />

        <div
            data-name="SheetContent"
            class=panel_class
            id=ctx.id.clone()
            role="dialog"
            aria-modal="true"
            aria-label=aria_label
            inert=move || !open.get()
            data-direction=direction.as_str()
            data-state=state
        >
            <button
                type="button"
                class=format!(
                    "absolute top-4 right-4 p-1 rounded-sm focus:ring-2 focus:ring-offset-2 focus:outline-none [&_svg:not([class*='size-'])]:size-4 focus:ring-ring{}",
                    if show_close_button { "" } else { " hidden" },
                )
                aria-label="Close sheet"
                on:click=move |_| open.set(false)
            >
                <span class="hidden">"Close Sheet"</span>
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M18 6 6 18" />
                    <path d="m6 6 12 12" />
                </svg>
            </button>

            {children()}
        </div>
    }
}
