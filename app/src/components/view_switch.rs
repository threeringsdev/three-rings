//! Grid / list layout switch — the roving-focus radiogroup first built for
//! `/catalog` (specs/app-ui.md → "Catalog page `/catalog`"), lifted here so
//! the *widget* cannot drift the moment more than one surface mounts it.
//!
//! The maintainer's grid-toggle ruling (2026-08-15, on the task "The My cards
//! page should have a grid display option") was explicit: "the toggle on
//! Catalog applied to /my — same control, same feel", scoped to `/my` (All
//! cards) **and** every `/my/collections/:id` (binder or deck) view. Four
//! mount points now share this one component: `/catalog`, `/my` + `/my/all`
//! (`crate::my::all_cards`) and `/my/collections/:id`
//! (`crate::my::collection`).
//!
//! **What is shared and what is not.** Only the widget — the `radiogroup`
//! markup, the roving `tabindex`, and the arrow-key handling that moves both
//! selection and focus together — lives here. Each caller keeps its own URL
//! convention: `catalog_url` carries `q`/`cursor`/`page` and defaults to
//! *grid* on a bare URL, while `my_url`/`collection_url` carry `q`/`cursor`
//! only and — deliberately, see their own doc comments — default to *list*,
//! so no existing `/my` or collection bookmark, deep link or e2e assertion
//! silently started rendering something else. This component knows nothing
//! about any of that: it is handed "is list mode showing right now" and a
//! callback to run with the newly chosen mode, and it does not touch the
//! router itself.
use leptos::prelude::*;

use crate::components::ui::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};

/// One radiogroup, one tab stop, arrow-key selection — the behavior
/// specs/app-ui.md's V1 vendoring findings deferred to the catalog task and
/// which every later mount point inherits unchanged.
///
/// `list_view` is "is list mode the one showing right now"; `on_change` fires
/// with the newly chosen mode (`true` for list, `false` for grid) on a click
/// or an arrow key. The caller decides what that means — building its own URL
/// and navigating — so this component carries no router state of its own.
#[component]
pub fn ViewSwitch(list_view: Memo<bool>, on_change: Callback<bool>) -> impl IntoView {
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let next = match ev.key().as_str() {
            "ArrowRight" | "ArrowDown" => Some(true),
            "ArrowLeft" | "ArrowUp" => Some(false),
            _ => None,
        };
        if let Some(list) = next {
            ev.prevent_default();
            on_change.run(list);
            // Roving focus: selection moved, so the tab stop moved with it and
            // the focus ring has to follow or keyboard users lose their place.
            // `tabindex` is already reactive on `pressed`.
            focus_switch_item(&ev, if list { 1 } else { 0 });
        }
    };

    view! {
        <ToggleGroup
            variant=ToggleGroupVariant::Outline
            spacing=0
            {..}
            role="radiogroup"
            aria-label="Result layout"
            on:keydown=on_keydown
        >
            <ToggleGroupItem
                title="Grid view"
                pressed=Signal::derive(move || !list_view.get())
                tabindex=Signal::derive(move || if list_view.get() { -1 } else { 0 })
                {..}
                on:click=move |_| on_change.run(false)
            >
                <span aria-hidden="true">"▦"</span>
                <span class="sr-only">"Grid view"</span>
            </ToggleGroupItem>
            <ToggleGroupItem
                title="List view"
                pressed=Signal::derive(move || list_view.get())
                tabindex=Signal::derive(move || if list_view.get() { 0 } else { -1 })
                {..}
                on:click=move |_| on_change.run(true)
            >
                <span aria-hidden="true">"☰"</span>
                <span class="sr-only">"List view"</span>
            </ToggleGroupItem>
        </ToggleGroup>
    }
}

/// Move focus to the nth item of the group the event fired on. Reads the DOM
/// rather than holding node refs: the group is the event's `currentTarget`, so
/// there is nothing to keep in sync. Client-only — event handlers never run
/// during SSR, so the non-hydrate arm is a stub (same shape as
/// `shell::hard_navigate`).
#[allow(unused_variables)]
fn focus_switch_item(ev: &leptos::ev::KeyboardEvent, index: u32) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        let Some(group) = ev
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        if let Ok(items) = group.query_selector_all("button[role='radio']") {
            if let Some(el) = items
                .item(index)
                .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = el.focus();
            }
        }
    }
}
