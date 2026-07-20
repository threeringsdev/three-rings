//! Collapsible — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/collapsible.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `aria-expanded` + a caller-supplied `aria-controls` id wired on the
//!   trigger (upstream ships no ARIA)
//! - `CollapsibleContent` takes an optional `id` so the trigger's
//!   `aria-controls` can point at it (deterministic caller IDs, the same
//!   convention as `dialog`/`popover` — no `use_random_id` anywhere)
//! - closed content is `inert` (the grid animation keeps it in the DOM, which
//!   would leave collapsed links tab-reachable; same fix as `dialog`/`sheet`)

use leptos::context::Provider;
use leptos::prelude::*;

#[derive(Clone)]
struct CollapsibleContext {
    open: RwSignal<bool>,
    /// The content panel's element id, for the trigger's `aria-controls`.
    content_id: Option<String>,
}

#[component]
pub fn Collapsible(
    /// Controlled: pass an external RwSignal to drive open/closed from outside.
    #[prop(optional)]
    open: Option<RwSignal<bool>>,
    /// Initial open state when uncontrolled.
    #[prop(default = false)]
    default_open: bool,
    /// Optional id for the content panel (deterministic, caller-supplied);
    /// wires the trigger's `aria-controls`.
    #[prop(optional, into)]
    content_id: Option<String>,
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let open_signal = open.unwrap_or_else(|| RwSignal::new(default_open));
    let ctx = CollapsibleContext {
        open: open_signal,
        content_id,
    };

    let class = tw_merge::tw_merge!("", class);

    view! {
        <Provider value=ctx>
            <div
                data-name="Collapsible"
                data-state=move || if open_signal.get() { "open" } else { "closed" }
                class=class
            >
                {children()}
            </div>
        </Provider>
    }
}

#[component]
pub fn CollapsibleTrigger(
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<CollapsibleContext>();
    let open = ctx.open;

    view! {
        <button
            type="button"
            data-name="CollapsibleTrigger"
            data-state=move || if open.get() { "open" } else { "closed" }
            aria-expanded=move || if open.get() { "true" } else { "false" }
            aria-controls=ctx.content_id
            class=class
            on:click=move |_| open.update(|v| *v = !*v)
        >
            {children()}
        </button>
    }
}

/// Animated show/hide panel using the CSS grid trick.
/// - `class` applies to the inner content div (padding, flex, gap, etc.)
/// - `outer_class` applies to the outer animation div — use for grid item props like `col-span-full`
#[component]
pub fn CollapsibleContent(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] outer_class: String,
) -> impl IntoView {
    let ctx = expect_context::<CollapsibleContext>();
    let open = ctx.open;
    let outer = tw_merge::tw_merge!(
        "grid overflow-hidden transition-all duration-300 data-[state=closed]:grid-rows-[0fr] data-[state=open]:grid-rows-[1fr]",
        outer_class
    );

    view! {
        <div
            data-name="CollapsibleContent"
            id=ctx.content_id
            data-state=move || if open.get() { "open" } else { "closed" }
            inert=move || !open.get()
            class=outer
        >
            <div class=tw_merge::tw_merge!("min-h-0", class)>{children()}</div>
        </div>
    }
}
