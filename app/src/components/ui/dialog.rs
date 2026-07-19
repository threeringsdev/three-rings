//! Dialog — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/dialog.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Markup/CSS vendored; behavior rewired
//! (specs/app-ui.md → "vendor markup + CSS, rewire behavior in Leptos").
//! Deviations from upstream:
//! - **deterministic caller-supplied `id`** replaces `use_random_id_for`
//!   (the gap analysis's SSR-counter hydration bug)
//! - **Leptos-owned open state**: the inline vanilla-`<script>` is gone;
//!   trigger/close/backdrop/ESC all drive one `RwSignal<bool>`, so the app
//!   can open a dialog programmatically (`m`-key move flow). Pass `open` to
//!   share the signal, or omit it for internal state.
//! - **ESC listener cleanup**: `window_event_listener` unsubscribes on
//!   unmount (upstream leaked a per-instance `document` listener)
//! - scroll locking calls the vendored Rust [`super::scroll_lock`] directly
//! - the `icons` crate's `X` replaced with the inlined Lucide path (ISC)

use leptos::context::Provider;
use leptos::prelude::*;
use tw_merge::tw_merge;

use super::button::{Button, ButtonSize, ButtonVariant};
use super::clx::clx;

mod components {
    use super::*;
    clx! {DialogBody, div, "flex flex-col gap-4"}
    clx! {DialogHeader, div, "flex flex-col gap-2 text-center sm:text-left"}
    clx! {DialogTitle, h3, "text-lg leading-none font-semibold"}
    clx! {DialogDescription, p, "text-muted-foreground text-sm"}
    clx! {DialogFooter, footer, "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end"}
}

pub use components::*;

#[derive(Clone)]
struct DialogContext {
    id: String,
    open: RwSignal<bool>,
}

/// The dialog's open signal, for wiring custom triggers inside a `<Dialog>`.
pub fn use_dialog_open() -> Option<RwSignal<bool>> {
    use_context::<DialogContext>().map(|c| c.open)
}

#[component]
pub fn Dialog(
    /// Deterministic instance id — SSR and hydration must agree on it.
    #[prop(into)]
    id: String,
    /// Share the open state with the caller (programmatic open/close);
    /// omitted = dialog-internal state.
    #[prop(optional)]
    open: Option<RwSignal<bool>>,
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let ctx = DialogContext { id, open };

    let merged_class = tw_merge!("w-fit", class);

    view! {
        <Provider value=ctx>
            <div class=merged_class data-name="Dialog">
                {children()}
            </div>
        </Provider>
    }
}

#[component]
pub fn DialogTrigger(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Default)] size: ButtonSize,
) -> impl IntoView {
    let ctx = expect_context::<DialogContext>();
    let open = ctx.open;

    view! {
        <Button
            class=class
            attr:id=format!("trigger_{}", ctx.id)
            attr:tabindex="0"
            variant=variant
            size=size
            on:click=move |_| open.set(true)
        >
            {children()}
        </Button>
    }
}

#[component]
pub fn DialogContent(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = true)] show_close_button: bool,
    #[prop(default = true)] close_on_backdrop_click: bool,
    /// Accessible name for the dialog (announced by AT alongside "dialog").
    #[prop(optional, into)]
    aria_label: Option<String>,
) -> impl IntoView {
    let ctx = expect_context::<DialogContext>();
    let open = ctx.open;

    let merged_class = tw_merge!(
        "bg-background border rounded-2xl shadow-lg p-6 w-full max-w-[calc(100%-2rem)] sm:max-w-lg max-h-[85vh] fixed top-[50%] left-[50%] translate-x-[-50%] translate-y-[-50%] z-100 transition-all duration-200 data-[state=closed]:opacity-0 data-[state=closed]:scale-95 data-[state=open]:opacity-100 data-[state=open]:scale-100 data-[state=closed]:pointer-events-none",
        class
    );

    let state = move || if open.get() { "open" } else { "closed" };

    // Scroll lock + overlay-stack registration follow the open state; the
    // lock is reference-counted so stacked overlays don't unlock each other,
    // and unlock waits out the exit animation like upstream (200 ms).
    let stack_id = ctx.id.clone();
    Effect::new(move |prev: Option<bool>| {
        let now = open.get();
        if now {
            super::scroll_lock::lock();
            super::overlay_stack::push(&stack_id);
        } else if prev == Some(true) {
            super::overlay_stack::remove(&stack_id);
            super::scroll_lock::unlock(200);
        }
        now
    });

    // ESC closes only the TOPMOST open overlay (the stack gate — one press,
    // one overlay). The listener handle unsubscribes on component cleanup —
    // the upstream document-listener leak this replaces.
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
        // An overlay unmounted while open must release its stack slot and
        // its lock reference.
        if open.get_untracked() {
            super::overlay_stack::remove(&unmount_id);
            super::scroll_lock::unlock(0);
        }
    });

    view! {
        <div
            data-name="DialogBackdrop"
            id=format!("{}_backdrop", ctx.id)
            class="fixed inset-0 transition-opacity duration-200 z-60 bg-black/50 data-[state=closed]:opacity-0 data-[state=closed]:pointer-events-none data-[state=open]:opacity-100"
            data-state=state
            on:click=move |_| {
                if close_on_backdrop_click {
                    open.set(false);
                }
            }
        />

        <div
            data-name="DialogContent"
            class=merged_class
            id=ctx.id.clone()
            role="dialog"
            aria-modal="true"
            aria-label=aria_label
            inert=move || !open.get()
            data-state=state
        >
            <button
                type="button"
                class=format!(
                    "absolute top-4 right-4 p-1 rounded-sm focus:ring-2 focus:ring-offset-2 focus:outline-none [&_svg:not([class*='size-'])]:size-4 focus:ring-ring{}",
                    if show_close_button { "" } else { " hidden" },
                )
                aria-label="Close dialog"
                on:click=move |_| open.set(false)
            >
                <span class="hidden">"Close Dialog"</span>
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

#[component]
pub fn DialogClose(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Default)] size: ButtonSize,
) -> impl IntoView {
    let ctx = expect_context::<DialogContext>();
    let open = ctx.open;

    view! {
        <Button
            class=class
            attr:aria-label="Close dialog"
            variant=variant
            size=size
            on:click=move |_| open.set(false)
        >
            {children()}
        </Button>
    }
}

/// A footer action that also closes the dialog (confirm buttons). The
/// caller's own `on:click` handler runs via normal event bubbling before
/// the close.
#[component]
pub fn DialogAction(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(default = ButtonVariant::Default)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Default)] size: ButtonSize,
) -> impl IntoView {
    let ctx = expect_context::<DialogContext>();
    let open = ctx.open;

    view! {
        <Button
            class=class
            variant=variant
            size=size
            on:click=move |_| open.set(false)
        >
            {children()}
        </Button>
    }
}
