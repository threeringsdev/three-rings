//! The debounced, URL-canonical search box shared by `/catalog` and `/my`.
//!
//! Not a vendored registry component and not one of specs/app-ui.md's three
//! custom gap components — it is an app-level composite over the vendored
//! `input_group`, so it carries no bench section. It exists because two pages
//! now need the *same* four behaviors, and each one is a trap that was already
//! paid for once on `/catalog`:
//!
//! 1. **The URL is the query.** Typing schedules a router navigation; the box
//!    never becomes a second source of truth. So a reload, a share, or Back
//!    reproduces exactly what is on screen, and the first page SSRs.
//! 2. **Debounce, with an Enter escape hatch.** Only the last keystroke of a
//!    burst navigates. Enter commits immediately rather than waiting it out.
//! 3. **The URL only re-seeds the field when it moved without us.** Without the
//!    `self_pushed` guard below, a keystroke landing between `navigate()` and
//!    the effect flushing gets reverted to the text we just committed — the URL
//!    winning an argument it started.
//! 4. **History granularity is per search *session*.** Refining replaces;
//!    starting or ending a search pushes. Replacing everything walks Back
//!    straight off the site; pushing everything buries the previous page under
//!    one entry per character.
//!
//! What differs per page is only the URL to navigate to, which is the
//! `to_url` prop.

use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;

use super::ui::input_group::{
    InputGroup, InputGroupAddon, InputGroupAddonAlign, InputGroupButton, InputGroupButtonSize,
    InputGroupInput,
};

/// Idle time after the last keystroke before the URL (and so the search) moves.
///
/// 250 ms is specs/catalog-search.md's proposal, left "tunable at execution";
/// kept as proposed — see that spec's Findings for the measurement.
///
/// The number is the *comfort* knob only. The correctness guarantee — "no stale
/// results ever render over newer input" — does not depend on it: `Resource` is
/// an `ArcAsyncDerived`, which stamps every run with a monotonic version and
/// drops a resolved future whose version is no longer the latest
/// (reactive_graph 0.2.14, `arc_async_derived.rs`: `if latest_version ==
/// this_version`). Overlapping searches therefore cannot land out of order
/// however short the debounce gets.
///
/// What the debounce *does* buy is request volume: a request already in flight
/// is discarded on arrival, never aborted, so shortening this trades server
/// work for responsiveness rather than trading away correctness.
pub const SEARCH_DEBOUNCE_MS: u64 = 250;

#[component]
pub fn QueryBar(
    /// The text in the box. Owned by the caller so the page can read it (the
    /// clear button's visibility, an empty-state message) without a second copy.
    text: RwSignal<String>,
    /// The URL's canonical query — the value this box must agree with.
    url_q: Memo<String>,
    /// Build the URL a given query text should navigate to. Called with the
    /// committed text; may read other URL state (the catalog's `?view=`)
    /// untracked.
    to_url: Callback<String, String>,
    /// The `<input>`'s DOM id — caller-supplied and deterministic, the same
    /// convention the vendored overlays follow.
    #[prop(into)]
    id: String,
    #[prop(into)] placeholder: String,
    #[prop(into)] aria_label: String,
) -> impl IntoView {
    let navigate = use_navigate();
    let pending = StoredValue::new(None::<leptos::leptos_dom::helpers::TimeoutHandle>);

    // The last query *we* put in the URL — see behavior 3 in the module doc.
    let self_pushed = StoredValue::new(url_q.get_untracked());

    let commit = {
        let navigate = navigate.clone();
        move |q: String| {
            let was_searching = !url_q.get_untracked().is_empty();
            let replace = was_searching && !q.is_empty();
            self_pushed.set_value(q.clone());
            navigate(
                &to_url.run(q),
                NavigateOptions {
                    replace,
                    ..Default::default()
                },
            );
        }
    };

    // Re-seed the field only when the URL moved without us: Back/Forward, a
    // shared link, or a filter-rail edit rewriting a term. Our own commits are
    // already in the box by definition.
    Effect::new(move |_| {
        let from_url = url_q.get();
        if from_url != self_pushed.get_value() {
            self_pushed.set_value(from_url.clone());
            text.set(from_url);
        }
    });

    let clear_pending = move || {
        pending.update_value(|h| {
            if let Some(h) = h.take() {
                h.clear();
            }
        });
    };

    let schedule = {
        let commit = commit.clone();
        move |q: String| {
            // Collapse the burst: only the last keystroke of a run searches.
            clear_pending();
            let commit = commit.clone();
            let handle = set_timeout_with_handle(
                move || commit(q.clone()),
                std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
            );
            pending.set_value(handle.ok());
        }
    };

    // Leaving the page with a timer armed would fire a navigate into a
    // torn-down router.
    on_cleanup(clear_pending);

    let on_input = {
        let schedule = schedule.clone();
        move |_| schedule(text.get_untracked())
    };
    // Enter searches now rather than waiting out the debounce.
    let on_key = {
        let commit = commit.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            if ev.key() == "Enter" {
                ev.prevent_default();
                clear_pending();
                commit(text.get_untracked());
            }
        }
    };
    let on_clear = move |_| {
        clear_pending();
        text.set(String::new());
        commit(String::new());
    };

    view! {
        <search>
            <InputGroup class="w-full">
                <InputGroupAddon>
                    <span aria-hidden="true">"🔍"</span>
                </InputGroupAddon>
                // NB: the prop immediately before `{..}` must not end in a bare
                // path — `placeholder=placeholder {..}` parses as struct-update
                // syntax and the spread silently becomes part of the value,
                // which then reports as "no field `aria`" on the props builder.
                // Hence the `.clone()` (also what lets the prop stay a String).
                <InputGroupInput
                    id=id
                    name="q"
                    bind_value=text
                    placeholder=placeholder.clone()
                    {..}
                    aria-label=aria_label
                    // No manual `value` seed: `Input` emits the SSR attribute
                    // from `bind_value` itself (see its bind_value arm).
                    on:input=on_input
                    on:keydown=on_key
                />
                <InputGroupAddon align=InputGroupAddonAlign::InlineEnd>
                    <Show when=move || !text.read().is_empty()>
                        <InputGroupButton
                            size=InputGroupButtonSize::IconXs
                            class=""
                            {..}
                            aria-label="Clear search"
                            on:click=on_clear.clone()
                        >
                            "✕"
                        </InputGroupButton>
                    </Show>
                </InputGroupAddon>
            </InputGroup>
        </search>
    }
}
