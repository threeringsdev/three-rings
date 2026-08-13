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
//! 5. **The armed debounce is public to the page's other writer of `?q=`.**
//!    While a timer is armed, the newest query the user has expressed is *in
//!    that timer*, not in the URL — so any other surface that rewrites the same
//!    string (the catalog's filter rail) must build its edit on the pending text
//!    and cancel the timer, or its edit gets overwritten ~250 ms later by the
//!    pre-click text. [`PendingQuery`] is that seam; see its docs for the
//!    semantics (P6-086).
//!
//! What differs per page is only the URL to navigate to, which is the
//! `to_url` prop.
//!
//! Two optional props exist for one caller, the quick-add panel
//! (`crate::components::quick_add`), which wraps this box in a type-ahead
//! surface: `on_key` lets it see keys *first* (its `⏎`/`⇧⏎`/`⌥⏎` contract
//! outranks the Enter-commits-now shortcut), and `reset` lets it clear the box
//! after an add — through here rather than by writing `text`, because a
//! keystroke's armed debounce would otherwise fire afterwards and put the query
//! straight back.

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

/// The query bar's armed debounce, shared with the page's *other* writer of the
/// same `?q=` — today only the catalog's filter rail (P6-086).
///
/// **Why this has to be shared at all.** A keystroke arms a timer holding the
/// box text *captured at that moment*. For the next ~250 ms the URL is stale by
/// construction, and the two surfaces disagree about what the query is. The rail
/// commits synchronously on click, so a facet clicked inside that window
/// navigated with the URL's (pre-typing) text, and then the bar's timer fired
/// and navigated again with the box's (pre-click) text — silently undoing the
/// facet edit. The rail's own commits do not have the mirror problem: they
/// re-read the URL when they fire rather than capturing it, so they rebase.
///
/// **The semantics: reconcile, don't cancel.** A rail edit reads [`peek`] and
/// rewrites *that* string, so the single navigation it already performs carries
/// both intents — what you typed plus what you clicked — and then [`cancel`]s
/// the timer whose text it has just absorbed. Cancelling alone would have
/// thrown the typed text away; letting the timer fire would have thrown the
/// click away.
///
/// [`peek`]: PendingQuery::peek
/// [`cancel`]: PendingQuery::cancel
///
/// **One bar at a time.** The slot is a single value provided by the shell
/// ([`provide_pending_query`]), and exactly one [`QueryBar`] is mounted on any
/// route (`/catalog`, `/my`, `/my/all`, and the quick-add panel's on
/// `/my/collections/:id`). A `QueryBar` rendered with no such context in scope
/// falls back to a private slot and behaves exactly as it did before.
#[derive(Clone, Copy)]
pub struct PendingQuery(StoredValue<Option<ArmedCommit>>);

#[derive(Clone)]
struct ArmedCommit {
    handle: leptos::leptos_dom::helpers::TimeoutHandle,
    /// The box text captured when the timer was armed — what it will put in the
    /// URL when it fires.
    text: String,
}

impl PendingQuery {
    fn new() -> Self {
        Self(StoredValue::new(None))
    }

    /// The query the armed debounce is about to commit, if one is armed. Does
    /// not disarm it: a caller that reads this and then declines to act (an
    /// unparseable base, say) must leave the user's keystrokes on their way to
    /// the URL.
    pub fn peek(&self) -> Option<String> {
        self.0.with_value(|a| a.as_ref().map(|a| a.text.clone()))
    }

    /// Cancel the armed debounce. Call this **only** once a commit that already
    /// folds in [`peek`](Self::peek)'s text is certain to happen — cancelling
    /// without absorbing it deletes what the user typed.
    pub fn cancel(&self) {
        self.0.update_value(|armed| {
            if let Some(armed) = armed.take() {
                armed.handle.clear();
            }
        });
    }

    /// Arm (replacing any previous timer, which is cancelled).
    fn arm(&self, handle: Option<leptos::leptos_dom::helpers::TimeoutHandle>, text: String) {
        self.cancel();
        self.0
            .set_value(handle.map(|handle| ArmedCommit { handle, text }));
    }

    /// The timer fired: empty the slot without clearing an already-elapsed
    /// handle, so a rail edit arriving after it reads the URL rather than a
    /// pending text that is no longer pending.
    fn fired(&self) {
        self.0.set_value(None);
    }
}

/// Provide the shared pending-debounce slot. Called once by the shell, above
/// both the `<Outlet/>` (where the bar lives) and the sidebar rail (where the
/// facets live) — context flows down the owner tree, so nothing the page
/// provides could reach the rail.
pub fn provide_pending_query() -> PendingQuery {
    let pending = PendingQuery::new();
    provide_context(pending);
    pending
}

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
    /// A composed surface that must see keydowns before this box does.
    /// Returning `true` means "handled": the box does nothing further,
    /// including its own Enter-commits-now shortcut.
    #[prop(optional)]
    on_key: Option<Callback<leptos::ev::KeyboardEvent, bool>>,
    /// Bump this to clear the box *and* commit the empty query, cancelling any
    /// armed debounce. Only the change matters, not the value — see the module
    /// doc for why writing `text` directly is not enough. Always **replaces**
    /// the history entry rather than pushing, unlike a manual clear — see
    /// where this is wired up, below.
    #[prop(optional)]
    reset: Option<RwSignal<u32>>,
) -> impl IntoView {
    let navigate = use_navigate();
    // The armed debounce lives in the shell's shared slot when there is one, so
    // the other writer of this `?q=` can see it (behavior 5 in the module doc);
    // a private slot otherwise, which is the pre-P6-086 behavior exactly.
    let pending = use_context::<PendingQuery>().unwrap_or_else(PendingQuery::new);

    // The last query *we* put in the URL — see behavior 3 in the module doc.
    let self_pushed = StoredValue::new(url_q.get_untracked());

    // Navigate to `q` with an explicit `replace`. `commit` (below) derives
    // `replace` from the URL; the `reset` effect (further below) pins it to
    // `true` instead — see that effect's comment and P6-148.
    let navigate_to = {
        let navigate = navigate.clone();
        move |q: String, replace: bool| {
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

    let commit = {
        let navigate_to = navigate_to.clone();
        move |q: String| {
            let was_searching = !url_q.get_untracked().is_empty();
            let replace = was_searching && !q.is_empty();
            navigate_to(q, replace);
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

    let clear_pending = move || pending.cancel();

    let schedule = {
        let commit = commit.clone();
        move |q: String| {
            let commit = commit.clone();
            let captured = q.clone();
            let handle = set_timeout_with_handle(
                move || {
                    // Empty the slot first: from here the URL, not the timer,
                    // holds the newest query (see `PendingQuery::fired`).
                    pending.fired();
                    commit(q.clone());
                },
                std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
            );
            // Arming collapses the burst: it cancels the previous timer, so only
            // the last keystroke of a run searches.
            pending.arm(handle.ok(), captured);
        }
    };

    // Leaving the page with a timer armed would fire a navigate into a
    // torn-down router.
    on_cleanup(clear_pending);

    let on_input = {
        let schedule = schedule.clone();
        move |_| schedule(text.get_untracked())
    };
    // Enter searches now rather than waiting out the debounce — unless a
    // composed surface claimed the key first (see the module doc).
    let on_keydown = {
        let commit = commit.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            if on_key.is_some_and(|cb| cb.run(ev.clone())) {
                return;
            }
            if ev.key() == "Enter" {
                ev.prevent_default();
                clear_pending();
                commit(text.get_untracked());
            }
        }
    };
    let clear = {
        let commit = commit.clone();
        move || {
            clear_pending();
            text.set(String::new());
            commit(String::new());
        }
    };
    // The caller's imperative reset — quick-add's own field, after every add.
    // This is *not* routed through `clear`/`commit`: `commit` prices going
    // empty as "ending a search" and pushes (P6-086's rule 4), which is right
    // for the ✕ button but wrong here — an add's clear-and-refetch is
    // housekeeping, not the user ending anything, and pushing on every add
    // turned Back into a walk through every add ever made (P6-148). So this
    // path always replaces instead. `prev.is_some()` skips the mount run, so
    // rendering the box never clears the query a deep link put in the URL.
    if let Some(reset) = reset {
        let navigate_to = navigate_to.clone();
        Effect::new(move |prev: Option<u32>| {
            let now = reset.get();
            if prev.is_some_and(|p| p != now) {
                clear_pending();
                text.set(String::new());
                navigate_to(String::new(), true);
            }
            now
        });
    }
    let on_clear = move |_| clear();

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
                    on:keydown=on_keydown
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
