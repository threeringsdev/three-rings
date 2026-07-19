//! HoverCard — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/hover_card.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Markup/CSS + native Popover API + CSS anchor
//! positioning kept; behavior rewired. Deviations from upstream:
//! - **deterministic caller-supplied `id`** replaces `use_random_id`
//! - **hover-intent timers are Leptos** (`set_timeout`/`clear` in the
//!   component), not the inline `<script>` upstream shipped — open on
//!   mouseenter/focus after 150 ms, close on leave/blur after 150 ms, and
//!   cancel the close while the pointer is over the content
//! - the anchor-positioning platform caveat is the same as `popover`; the
//!   card is a hover preview (never the sole affordance), so no JS fallback
//!   is needed — a mispositioned preview on a non-anchor engine is cosmetic

use leptos::prelude::*;
use tw_merge::tw_merge;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum HoverCardSide {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

#[derive(Clone)]
struct HoverCardContext {
    anchor_name: String,
    content_id: String,
    open: RwSignal<bool>,
    /// ONE timer shared by trigger and content, so moving the pointer from
    /// the trigger onto the content cancels the trigger's pending close
    /// (separate timers was the bug: the handoff closed the card).
    timer: HoverTimer,
}

#[component]
pub fn HoverCard(
    /// Deterministic instance id — SSR and hydration must agree on it.
    #[prop(into)]
    id: String,
    #[prop(default = HoverCardSide::default())] side: HoverCardSide,
    children: Children,
) -> impl IntoView {
    let anchor_name = format!("--hc-anchor-{id}");
    let content_id = format!("hc-content-{id}");
    let open = RwSignal::new(false);

    let (position_styles, transform_origin) = match side {
        HoverCardSide::Bottom => ("position-area: block-end; margin-top: 8px;", "center top"),
        HoverCardSide::Top => (
            "position-area: block-start; margin-bottom: 8px;",
            "center bottom",
        ),
        HoverCardSide::Left => (
            "position-area: inline-start; margin-right: 8px;",
            "right center",
        ),
        HoverCardSide::Right => (
            "position-area: inline-end; margin-left: 8px;",
            "left center",
        ),
    };

    let ctx = HoverCardContext {
        anchor_name: anchor_name.clone(),
        content_id: content_id.clone(),
        open,
        timer: HoverTimer::new(),
    };

    view! {
        <leptos::context::Provider value=ctx>
            <style>
                {format!(
                    "
                    #{content_id} {{
                        position-anchor: {anchor_name};
                        inset: auto;
                        {position_styles}
                        position-try-fallbacks: flip-block;
                        position-try-order: most-height;
                        position-visibility: anchors-visible;

                        &:popover-open {{
                            opacity: 1;
                            transform: scale(1) translateY(0px);

                            @starting-style {{
                                opacity: 0;
                                transform: scale(0.95) translateY(-4px);
                            }}
                        }}

                        & {{
                            transition:
                                display 0.2s allow-discrete,
                                overlay 0.2s allow-discrete,
                                transform 0.2s cubic-bezier(0.16, 1, 0.3, 1),
                                opacity 0.15s ease-out;
                            opacity: 0;
                            transform: scale(0.95) translateY(-4px);
                            transform-origin: {transform_origin};
                        }}
                    }}
                    ",
                )}
            </style>
            {children()}
        </leptos::context::Provider>
    }
}

/// Hover-intent scheduling: a per-HoverCard cancelable timer. Client-only.
#[derive(Clone, Copy)]
struct HoverTimer {
    handle: StoredValue<Option<leptos::leptos_dom::helpers::TimeoutHandle>>,
}

impl HoverTimer {
    fn new() -> Self {
        Self {
            handle: StoredValue::new(None),
        }
    }

    fn cancel(&self) {
        if let Some(h) = self.handle.get_value() {
            h.clear();
            self.handle.set_value(None);
        }
    }

    fn schedule(&self, delay_ms: u64, f: impl FnOnce() + 'static) {
        self.cancel();
        let handle = self.handle;
        let h = leptos::prelude::set_timeout_with_handle(
            move || {
                handle.set_value(None);
                f();
            },
            std::time::Duration::from_millis(delay_ms),
        )
        .ok();
        self.handle.set_value(h);
    }
}

#[component]
pub fn HoverCardTrigger(
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<HoverCardContext>();
    let open = ctx.open;
    let timer = ctx.timer;
    on_cleanup(move || timer.cancel());

    let show = move || timer.schedule(150, move || open.set(true));
    let hide = move || timer.schedule(150, move || open.set(false));

    view! {
        <span
            class=tw_merge!("inline-block", class)
            style=format!("anchor-name: {}", ctx.anchor_name)
            data-name="HoverCardTrigger"
            on:mouseenter=move |_| show()
            on:mouseleave=move |_| hide()
            on:focusin=move |_| show()
            on:focusout=move |_| hide()
        >
            {children()}
        </span>
    }
}

#[component]
pub fn HoverCardContent(
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let ctx = expect_context::<HoverCardContext>();
    let open = ctx.open;
    let timer = ctx.timer; // shared with the trigger — see HoverCardContext
    let class = tw_merge!(
        "overflow-visible relative z-50 p-4 rounded-lg border bg-card text-card-foreground shadow-md w-64",
        class
    );
    let node_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    // Drive the native popover from the shared open signal.
    Effect::new(move |_| {
        let want = open.get();
        if let Some(el) = node_ref.get() {
            let is_open = el.matches(":popover-open").unwrap_or(false);
            if want && !is_open {
                if el.show_popover().is_err() {
                    open.set(el.matches(":popover-open").unwrap_or(false));
                }
            } else if !want && is_open {
                let _ = el.hide_popover();
            }
        }
    });

    view! {
        <div
            class=class
            id=ctx.content_id
            popover="manual"
            data-name="HoverCardContent"
            node_ref=node_ref
            on:mouseenter=move |_| timer.cancel()
            on:mouseleave=move |_| timer.schedule(150, move || open.set(false))
        >
            {children()}
        </div>
    }
}
