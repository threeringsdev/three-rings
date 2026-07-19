//! Popover — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/popover.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Native Popover API + CSS anchor positioning kept
//! (the point of the component); behavior rewired. Deviations from upstream:
//! - **deterministic caller-supplied `id`** replaces `use_random_id` (SSR
//!   counter hydration bug); the anchor name derives from it
//! - the inline `<script>` (close-on-CommandItem-click) is gone — feature
//!   compositions close via [`use_popover_open`] in Leptos
//! - optional **`open` signal**: synced to the native popover both ways
//!   (`showPopover`/`hidePopover` on signal change, `toggle` events back
//!   into the signal), so pickers can be driven programmatically
//! - CSS anchor positioning verified on the Android webview (Chrome 145);
//!   webkit rides the boundary tier — fallback decision recorded in
//!   app-ui Findings

use leptos::prelude::*;
use tw_merge::tw_merge;

use super::clx::clx;

mod components {
    use super::*;
    clx! {PopoverTitle, h3, "leading-none font-medium", "mb-3"}
    clx! {PopoverDescription, p, "text-muted-foreground text-sm"}
}

pub use components::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PopoverAlign {
    Start,
    StartOuter,
    End,
    EndOuter,
    #[default]
    Center,
}

#[derive(Clone)]
struct PopoverContext {
    anchor_name: String,
    target_id: String,
    open: RwSignal<bool>,
}

/// The popover's open signal, for closing from composed content (e.g. a
/// destination picker's item click).
pub fn use_popover_open() -> Option<RwSignal<bool>> {
    use_context::<PopoverContext>().map(|c| c.open)
}

#[component]
pub fn Popover(
    /// Deterministic instance id — SSR and hydration must agree on it.
    #[prop(into)]
    id: String,
    /// Share the open state with the caller; omitted = internal state.
    #[prop(optional)]
    open: Option<RwSignal<bool>>,
    #[prop(default = PopoverAlign::default())] align: PopoverAlign,
    children: Children,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let popover_anchor_name = format!("--anchor-{id}");
    let popover_target_id = format!("popover-{id}");

    let (position_styles, transform_origin) = match align {
        PopoverAlign::Start => (
            "left: anchor(left);
                bottom: anchor(top);
                margin-bottom: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                bottom: auto;
                margin-top: 8px;
                margin-bottom: 0;
                }",
            "left top",
        ),
        PopoverAlign::StartOuter => (
            "right: anchor(left);
                top: anchor(top);
                margin-right: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                margin-top: 8px;
                }",
            "right top",
        ),
        PopoverAlign::End => (
            "right: anchor(right);
                bottom: anchor(top);
                margin-bottom: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                bottom: auto;
                margin-top: 8px;
                margin-bottom: 0;
                }",
            "right top",
        ),
        PopoverAlign::EndOuter => (
            "left: anchor(right);
                top: anchor(top);
                margin-left: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                margin-top: 8px;
                }",
            "left top",
        ),
        PopoverAlign::Center => ("position-area: block-start;", "center top"),
    };

    let ctx = PopoverContext {
        anchor_name: popover_anchor_name.clone(),
        target_id: popover_target_id.clone(),
        open,
    };

    view! {
        <leptos::context::Provider value=ctx>
            <style>
                {format!(
                    "
                #{popover_target_id} {{
                position-anchor: {popover_anchor_name};
                inset: auto;
                {position_styles}
                position-try-fallbacks: flip-block;
                position-try-order: most-height;
                position-visibility: anchors-visible;

                /* Open State */
                &:popover-open {{
                opacity: 1;
                transform: scale(1) translateY(0px);

                @starting-style {{
                opacity: 0;
                transform: scale(0.95) translateY(-2px);
                }}
                }}

                /* Closed State */
                & {{
                transition:
                display 0.2s allow-discrete,
                overlay 0.2s allow-discrete,
                transform 0.15s cubic-bezier(0.16, 1, 0.3, 1),
                opacity 0.15s ease-out;
                opacity: 0;
                transform: scale(0.95) translateY(-2px);
                transform-origin: var(--popover-transform-origin, {transform_origin});
                }}
                }}
                ",
                )}
            </style>

            <div data-name="Popover">{children()}</div>
        </leptos::context::Provider>
    }
}

#[component]
pub fn PopoverTrigger(children: Children, #[prop(optional, into)] class: String) -> impl IntoView {
    let ctx = expect_context::<PopoverContext>();
    let button_class = tw_merge!(
        "px-4 py-2 h-9 inline-flex justify-center items-center text-sm font-medium whitespace-nowrap rounded-md transition-colors w-fit focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&_svg:not(:last-child)]:mr-2 [&_svg:not(:first-child)]:ml-2 border bg-background border-input hover:bg-accent hover:text-accent-foreground",
        class
    );

    view! {
        <button
            class=button_class
            style=format!("anchor-name: {}", ctx.anchor_name)
            popovertarget=ctx.target_id
            tabindex="0"
            type="button"
            data-name="PopoverTrigger"
        >
            {children()}
        </button>
    }
}

#[component]
pub fn PopoverContent(children: Children, #[prop(optional, into)] class: String) -> impl IntoView {
    let ctx = expect_context::<PopoverContext>();
    let open = ctx.open;
    let class = tw_merge!(
        "overflow-visible relative z-50 p-4 rounded-md border bg-popover text-popover-foreground shadow-md my-[1ch] w-[250px]",
        class
    );

    let node_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let target_id = ctx.target_id.clone();

    // Two-way sync with the native popover: signal → showPopover/hidePopover
    // (Effects only run client-side, so no cfg gate), native toggle events
    // (light-dismiss, the popovertarget trigger) → signal. DOM types come
    // through leptos's own web_sys re-export — available in every build.
    Effect::new(move |_| {
        let want_open = open.get();
        if let Some(el) = node_ref.get() {
            let is_open = el.matches(":popover-open").unwrap_or(false);
            if want_open && !is_open {
                if el.show_popover().is_err() {
                    // Keep the signal honest if the native call is rejected
                    // (e.g. an ancestor with `display:none`).
                    open.set(el.matches(":popover-open").unwrap_or(false));
                }
            } else if !want_open && is_open {
                let _ = el.hide_popover();
            }
            // JS positioning fallback (spec: "JS fallback if unsupported"):
            // WebKit ships the Popover API but NOT CSS anchor positioning, so
            // the panel would open at the viewport default. When anchors are
            // unsupported and we're open, position manually under the trigger
            // (flipping above if it would overflow the viewport bottom).
            // Hydrate-only: the DOM measurement APIs and the mispositioning
            // it corrects both exist only client-side.
            #[cfg(feature = "hydrate")]
            if want_open && !anchor_positioning_supported() {
                position_below_trigger(&el, &target_id);
            }
            #[cfg(not(feature = "hydrate"))]
            let _ = &target_id;
        }
    });

    view! {
        <div
            class=class
            id=ctx.target_id.clone()
            popover="auto"
            data-name="PopoverContent"
            node_ref=node_ref
            on:toggle=move |_| {
                if let Some(el) = node_ref.get_untracked() {
                    open.set(el.matches(":popover-open").unwrap_or(false));
                }
            }
        >
            {children()}
        </div>
    }
}

/// Whether the engine supports CSS anchor positioning. Chromium (incl. the
/// Android webview) yes; WebKit not yet — there we position manually.
#[cfg(feature = "hydrate")]
fn anchor_positioning_supported() -> bool {
    web_sys::css::supports("position-anchor: --x").unwrap_or(false)
}

/// JS positioning fallback: fixed-position the panel just below its trigger,
/// flipping above when it would overflow the viewport bottom. Only used when
/// CSS anchor positioning is unavailable.
#[cfg(feature = "hydrate")]
fn position_below_trigger(panel: &web_sys::HtmlElement, target_id: &str) {
    use leptos::wasm_bindgen::JsCast;
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(trigger) = doc
        .query_selector(&format!("[popovertarget=\"{target_id}\"]"))
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return;
    };
    let t = trigger.get_bounding_client_rect();
    let p = panel.get_bounding_client_rect();
    let viewport_h = web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let below = t.bottom() + 8.0;
    let top = if below + p.height() > viewport_h && t.top() - 8.0 - p.height() > 0.0 {
        t.top() - 8.0 - p.height()
    } else {
        below
    };
    let style = panel.style();
    let _ = style.set_property("position", "fixed");
    let _ = style.set_property("margin", "0");
    let _ = style.set_property("left", &format!("{}px", t.left()));
    let _ = style.set_property("top", &format!("{top}px"));
}
