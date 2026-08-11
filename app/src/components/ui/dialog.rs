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
//! - **Tab/Shift+Tab focus trap** (P6-125; upstream has none — a vanilla
//!   dialog's Tab walks straight through to the rest of the page):
//!   `DialogContent` owns a second `window` keydown listener, gated exactly
//!   like Escape's (`overlay_stack::is_top`, so a `Popover` opened on top of
//!   this dialog keeps its own Tab order until it closes). While topmost, Tab
//!   from the last tabbable descendant wraps to the first, Shift+Tab from the
//!   first wraps to the last, and — the palette's own cited symptom, a field
//!   focused on open with nothing installed to hold it — a Tab pressed while
//!   focus is not on a tracked tabbable at all (the trigger, still focused by
//!   the click that opened the dialog; *or*, routinely, the container's own
//!   `tabindex="-1"`, the click-focus target for any non-interactive chrome —
//!   title, description, padding) is redirected in rather than left to walk
//!   the page behind the scrim, forward or backward. The tabbable set is
//!   re-queried fresh on every keypress, never cached (dialogs re-render),
//!   and filtered to elements with real layout (`offsetWidth`/`offsetHeight`)
//!   so a `display:none` close button (`show_close_button=false`, e.g.
//!   `CommandDialog`) or a closed sibling `Popover`'s native-hidden content
//!   never counts as a stop. Zero tabbable descendants keeps focus on the
//!   container itself, now `tabindex="-1"` so `.focus()` has somewhere to
//!   land.

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

    // Tab/Shift+Tab focus trap — gated exactly like Escape above, so a
    // `Popover` opened on top of this dialog (the delete confirm's
    // disposition pickers) keeps its own Tab order until it closes. See the
    // module doc for the boundary rules; `trap_tab` re-queries the tabbable
    // set fresh on every press.
    let tab_id = ctx.id.clone();
    let tab = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Tab" && open.get_untracked() && super::overlay_stack::is_top(&tab_id) {
            trap_tab(&tab_id, ev.shift_key(), &ev);
        }
    });

    let unmount_id = ctx.id.clone();
    on_cleanup(move || {
        esc.remove();
        tab.remove();
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
            // Not in the page's own Tab order (no positive tabindex), but
            // programmatically focusable — the zero-tabbables trap fallback
            // below needs somewhere to put focus.
            tabindex="-1"
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

// ------------------------------------------------------- Tab focus trap --

/// Natively tabbable elements — mirrors the browser's own default Tab order.
/// `[tabindex="-1"]` and `[disabled]` both match `querySelectorAll` but never
/// receive focus from a real Tab press (a `CommandItem`'s roving-focus rows,
/// a busy dialog's disabled confirm button), so both are excluded here too.
#[cfg(feature = "hydrate")]
const TABBABLE_SELECTOR: &str = "a[href]:not([disabled]), button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1']):not([disabled])";

/// The document's focused element, if it is one.
#[cfg(feature = "hydrate")]
fn active_element() -> Option<web_sys::HtmlElement> {
    use leptos::wasm_bindgen::JsCast;
    document()
        .active_element()
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
}

/// `container`'s tabbable descendants, in DOM order, re-queried fresh on
/// every call (never cached — dialogs re-render) and filtered to elements
/// that actually have layout. `offsetWidth`/`offsetHeight` catches both a
/// `display:none` close button (`show_close_button=false`, e.g.
/// `CommandDialog`) and a closed sibling `Popover`'s native-hidden content —
/// neither of which the selector alone excludes, since the native Popover API
/// only sets `display:none` via the UA stylesheet, not a `disabled`/tabindex
/// attribute.
#[cfg(feature = "hydrate")]
fn tabbable_within(container: &web_sys::Element) -> Vec<web_sys::HtmlElement> {
    use leptos::wasm_bindgen::JsCast;
    let Ok(list) = container.query_selector_all(TABBABLE_SELECTOR) else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .filter(|el| el.offset_width() > 0 || el.offset_height() > 0)
        .collect()
}

/// The trap itself, run from `DialogContent`'s Tab/Shift+Tab listener above.
/// - Tab from the last tabbable → wraps to the first
/// - Shift+Tab from the first tabbable → wraps to the last
/// - focus is not on any tracked tabbable at all → Tab enters at the first,
///   Shift+Tab at the last. Two situations land here, treated identically:
///   focus outside the container entirely (the trigger, still focused from
///   the click that opened the dialog — the symptom this was written for),
///   and focus *inside* the container but on something that isn't itself
///   tabbable — routinely the container's own `tabindex="-1"` (added for the
///   zero-tabbables case below): clicking non-interactive chrome (a title, a
///   description, plain padding) has no focusable target of its own, so the
///   browser's focus algorithm walks up to the nearest focusable ancestor —
///   the container — and lands there instead. Left unhandled, that state
///   free-rides `is_within == true, idx == None` straight through to native
///   Tab, which is a live escape hatch out of the dialog, not a rare edge
///   case (any click on dialog chrome reaches it).
/// - a Tab that lands on a *tracked* tabbable, on neither boundary, is left
///   alone — the browser's own DOM-order walk already stays inside the
///   container between two non-boundary elements
/// - no tabbable descendants at all → focus stays on the container itself
#[cfg(feature = "hydrate")]
fn trap_tab(container_id: &str, shift: bool, ev: &leptos::ev::KeyboardEvent) {
    use leptos::wasm_bindgen::JsCast;
    let Some(container) = document().get_element_by_id(container_id) else {
        return;
    };
    let tabbables = tabbable_within(&container);
    if tabbables.is_empty() {
        ev.prevent_default();
        ev.stop_immediate_propagation();
        if let Ok(html) = container.dyn_into::<web_sys::HtmlElement>() {
            let _ = html.focus();
        }
        return;
    }

    // Position of the focused element within the tabbable set, if it has
    // one. Every element found here is, by construction, a descendant of
    // `container` (queried from it via `tabbable_within`), so `Some(_)`
    // already implies "focus is within the container" — no separate
    // containment check is needed, and `None` correctly covers both "outside
    // the container" and "inside it, but not on a tabbable" the same way.
    let idx = active_element().and_then(|a| {
        tabbables
            .iter()
            .position(|t| t.is_same_node(Some(a.as_ref())))
    });

    match (shift, idx) {
        (true, Some(0)) | (true, None) => {
            if let Some(last) = tabbables.last() {
                ev.prevent_default();
                ev.stop_immediate_propagation();
                let _ = last.focus();
            }
        }
        (false, None) => {
            if let Some(first) = tabbables.first() {
                ev.prevent_default();
                ev.stop_immediate_propagation();
                let _ = first.focus();
            }
        }
        (false, Some(i)) if i == tabbables.len() - 1 => {
            if let Some(first) = tabbables.first() {
                ev.prevent_default();
                ev.stop_immediate_propagation();
                let _ = first.focus();
            }
        }
        _ => {}
    }
}

#[cfg(not(feature = "hydrate"))]
fn trap_tab(_container_id: &str, _shift: bool, _ev: &leptos::ev::KeyboardEvent) {}
