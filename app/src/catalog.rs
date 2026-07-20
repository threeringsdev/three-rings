//! `/catalog` — the public catalog search surface (specs/app-ui.md
//! "`/catalog`", specs/catalog-search.md).
//!
//! The contract this screen implements:
//!
//! - **The query text is the canonical state and it lives in the URL.** Every
//!   edit path — typing, clearing, the view switch — goes through a router
//!   navigation, so the address bar is always a shareable, SSR-able description
//!   of what is on screen. The filter rail (its own task) rewrites terms in this
//!   same string; nothing here may take a second source of truth for the query.
//! - **First page SSRs when the URL carries `q`.** The results `Resource` is
//!   keyed on the URL's query, so a cold load renders markup, not a spinner.
//! - **Live typing: ~250 ms debounce, stale-response discard.** The debounce is
//!   ours (below); the discard is the reactive layer's — see
//!   `SEARCH_DEBOUNCE_MS`. Note what this is *not*: an overtaken request is
//!   discarded on arrival, not aborted in flight.
//! - **Parse errors are results, not failures.** The grammar rejects unknown
//!   terms by design (`ApiError::Validation`, 422) and a half-typed query hits
//!   that constantly, so it renders inline under the bar and leaves the previous
//!   result set alone rather than blanking the page.
//!
//! Anonymous is the default audience here: `/catalog` is public, the search
//! adapter reads the session opportunistically, and the quick actions prompt
//! sign-in rather than disappearing.

pub mod rail;

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};
use leptos_router::NavigateOptions;
use shared::CardSummary;

use crate::cards::CardPreview;
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input_group::{
    InputGroup, InputGroupAddon, InputGroupAddonAlign, InputGroupButton, InputGroupButtonSize,
    InputGroupInput,
};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};
use crate::components::ui::toggle_group::{ToggleGroup, ToggleGroupItem, ToggleGroupVariant};
use crate::shell::CurrentUserResource;

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
const SEARCH_DEBOUNCE_MS: f64 = 250.0;

/// `?view=list` renders the table; anything else (including absent) is the grid.
/// View mode rides the URL alongside `q` so a reload or a shared link keeps the
/// layout, and so it SSRs correctly instead of flipping after hydration. It is
/// *not* search state — it never enters the query text.
const VIEW_PARAM: &str = "view";
const LIST_VIEW: &str = "list";

/// Build `/catalog?q=…&view=…`, omitting empty parts. The single place a
/// catalog URL is constructed, so the canonical form can't drift between the
/// query bar, the clear button, and the view switch.
fn catalog_url(q: &str, list_view: bool) -> String {
    let mut url = String::from("/catalog");
    let mut sep = '?';
    if !q.is_empty() {
        url.push(sep);
        url.push_str("q=");
        url.push_str(&encode_query_value(q));
        sep = '&';
    }
    if list_view {
        url.push(sep);
        url.push_str(VIEW_PARAM);
        url.push('=');
        url.push_str(LIST_VIEW);
    }
    url
}

/// Percent-encode a query *value*. Deliberately conservative: the search
/// grammar is punctuation-heavy (`t:instant c:ur cmc<=2`) and `&`, `#`, `+`
/// and friends would otherwise be read as URL structure or as a space.
fn encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pull the human-facing message out of a server-fn error, and say whether it
/// was the grammar rejecting a term (422) rather than something going wrong.
///
/// The transport only carries `ApiError`'s `Display` string, so the `validation:`
/// prefix is the wire signal. Matching on the prefix is narrow, but the
/// alternative — treating every error as a failure — would flash "search failed"
/// under a user's fingers on every partially-typed term.
fn describe_error(e: &ServerFnError<String>) -> (bool, String) {
    let raw = match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
    };
    match raw.strip_prefix("validation: ") {
        Some(rest) => (true, rest.to_string()),
        None => (false, raw),
    }
}

#[component]
pub fn CatalogPage() -> impl IntoView {
    let query_map = use_query_map();

    // The URL is the source of truth. Both are Memos so that a navigation which
    // changes only `view` doesn't invalidate the results resource (and vice
    // versa) — Memo suppresses the notification when the value is unchanged.
    let url_q = Memo::new(move |_| query_map.read().get("q").unwrap_or_default());
    let list_view =
        Memo::new(move |_| query_map.read().get(VIEW_PARAM).as_deref() == Some(LIST_VIEW));

    // The text in the box. The URL⇄field sync lives in QueryBar, which is the
    // only thing that writes either one.
    let query_text = RwSignal::new(url_q.get_untracked());

    // First page. Keyed on the URL's query alone: the debounce decides *when*
    // the URL moves, this decides what is displayed once it has.
    let results = Resource::new(
        move || url_q.get(),
        |q| async move { crate::search_catalog(q, None).await },
    );

    // The last result set that came back OK. A rejected query must not take
    // the results down with it (specs/catalog-search.md: half-typed queries hit
    // the grammar's term-naming error constantly), so the error renders *over*
    // the last good page rather than replacing it. Effects don't run during
    // SSR, which is correct here — a cold load that errors has no previous
    // page to keep.
    let last_good = RwSignal::new(None::<Vec<CardSummary>>);
    Effect::new(move |_| {
        if let Some(Ok(r)) = results.get() {
            last_good.set(Some(r.cards));
        }
    });

    // Browse-all context line, and the seam-proving anonymous read the shell
    // task parked here (specs/data-access-backends.md). Keyed on "is the query
    // empty" so it costs one request while browsing and none while searching,
    // where the result count is the interesting number instead.
    let count = Resource::new(
        move || url_q.read().is_empty(),
        |browsing| async move {
            match browsing {
                true => crate::catalog_count().await.ok(),
                false => None,
            }
        },
    );

    view! {
        <div class="flex min-w-0 flex-col gap-4 p-4 md:p-6">
            <div>
                <h1 class="text-2xl font-bold">"Catalog"</h1>
                <Transition fallback=|| ()>
                    {move || Suspend::new(async move {
                        count
                            .await
                            .map(|c| {
                                view! {
                                    <p class="text-muted-foreground text-sm">
                                        {format!("{} cards in the catalog.", c.cards)}
                                    </p>
                                }
                            })
                    })}
                </Transition>
            </div>
            <QueryBar query_text url_q list_view />
            <ResultsToolbar results list_view />
            <Results results last_good list_view />
        </div>
    }
}

/// The search box. Owns the debounce: keystrokes move a local signal
/// immediately (so typing never feels laggy) and schedule the navigation that
/// actually searches.
#[component]
fn QueryBar(
    query_text: RwSignal<String>,
    url_q: Memo<String>,
    list_view: Memo<bool>,
) -> impl IntoView {
    let navigate = use_navigate();
    let query_map = use_query_map();
    let pending = StoredValue::new(None::<leptos::leptos_dom::helpers::TimeoutHandle>);

    // The last query *we* put in the URL. Without it, the field-sync effect
    // below cannot tell our own navigation from someone else's, and a keystroke
    // landing in the window between `navigate()` and the effect flushing would
    // be reverted to the older text we just committed — the URL would win an
    // argument it started.
    let self_pushed = StoredValue::new(url_q.get_untracked());

    // History granularity is per *search session*, not per keystroke: refining
    // a query replaces, but starting or ending one pushes. Replacing
    // everything would make Back walk straight off the site from the first
    // search a visitor types; pushing everything would bury the previous page
    // under one entry per character.
    let commit = {
        let navigate = navigate.clone();
        move |q: String| {
            let was_searching = !query_map
                .read_untracked()
                .get("q")
                .unwrap_or_default()
                .is_empty();
            let replace = was_searching && !q.is_empty();
            self_pushed.set_value(q.clone());
            navigate(
                &catalog_url(&q, list_view.get_untracked()),
                NavigateOptions {
                    replace,
                    ..Default::default()
                },
            );
        }
    };

    // Re-seed the field only when the URL moved without us: Back/Forward, a
    // shared link, or (later) a filter-rail edit rewriting a term. Our own
    // commits are already in the box by definition.
    Effect::new(move |_| {
        let from_url = url_q.get();
        if from_url != self_pushed.get_value() {
            self_pushed.set_value(from_url.clone());
            query_text.set(from_url);
        }
    });

    let schedule = {
        let commit = commit.clone();
        move |q: String| {
            // Collapse the burst: only the last keystroke of a run searches.
            pending.update_value(|h| {
                if let Some(h) = h.take() {
                    h.clear();
                }
            });
            let commit = commit.clone();
            let handle = set_timeout_with_handle(
                move || commit(q.clone()),
                std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS as u64),
            );
            pending.set_value(handle.ok());
        }
    };

    // Leaving the page with a timer armed would fire a navigate into a
    // torn-down router.
    on_cleanup(move || {
        pending.update_value(|h| {
            if let Some(h) = h.take() {
                h.clear();
            }
        });
    });

    let on_input = {
        let schedule = schedule.clone();
        move |_| schedule(query_text.get_untracked())
    };
    // Enter searches now rather than waiting out the debounce.
    let on_key = {
        let commit = commit.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            if ev.key() == "Enter" {
                ev.prevent_default();
                pending.update_value(|h| {
                    if let Some(h) = h.take() {
                        h.clear();
                    }
                });
                commit(query_text.get_untracked());
            }
        }
    };
    let on_clear = move |_| {
        pending.update_value(|h| {
            if let Some(h) = h.take() {
                h.clear();
            }
        });
        query_text.set(String::new());
        commit(String::new());
    };

    view! {
        <search>
            <InputGroup class="w-full">
                <InputGroupAddon>
                    <span aria-hidden="true">"🔍"</span>
                </InputGroupAddon>
                // NB: the prop immediately before `{..}` must not end in a bare
                // path — `bind_value=query_text {..}` parses as struct-update
                // syntax and the spread silently becomes part of the value.
                <InputGroupInput
                    id="catalog-query"
                    name="q"
                    bind_value=query_text
                    placeholder="Search the catalog — t:instant c:ur cmc<=2"
                    {..}
                    aria-label="Search the catalog"
                    // `bind:value` is a client-side binding only — it renders
                    // no `value` attribute — so a shared `?q=` link used to
                    // SSR with an empty box that filled in only once wasm
                    // landed. Found while building the rail, which has three
                    // more fields with the same shape. Set once, not
                    // reactively: after hydration the property is what shows,
                    // and a reactive attribute would race the binding.
                    value=url_q.get_untracked()
                    on:input=on_input
                    on:keydown=on_key
                />
                <InputGroupAddon align=InputGroupAddonAlign::InlineEnd>
                    <Show when=move || !query_text.read().is_empty()>
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

/// Result count on the left of the grid/list switch. The destination picker
/// (`Adding to: 📥 Inbox ▾`, wireframe) joins this row in its own task.
#[component]
fn ResultsToolbar(
    results: Resource<Result<shared::SearchResults, ServerFnError<String>>>,
    list_view: Memo<bool>,
) -> impl IntoView {
    // The mobile sheet's "Show N results" footer. Read off the resource
    // directly rather than awaited: the sheet is open while a search is in
    // flight, and a `None` there reads as "Show results" instead of blocking
    // the button behind a suspense boundary.
    let result_count =
        Signal::derive(move || results.get().and_then(|r| r.ok()).map(|r| r.cards.len()));

    view! {
        <div class="flex flex-wrap items-center gap-3">
            <rail::FilterSheet result_count />
            <p class="text-muted-foreground text-sm" data-testid="result-count">
                <Transition fallback=|| {
                    view! { <span>"Searching…"</span> }
                }>
                    {move || Suspend::new(async move {
                        match results.await {
                            Ok(r) => {
                                let n = r.cards.len();
                                let more = if r.next_cursor.is_some() { "+" } else { "" };
                                format!("{n}{more} results")
                            }
                            Err(_) => String::new(),
                        }
                    })}
                </Transition>
            </p>
            <div class="ml-auto">
                <ViewSwitch list_view />
            </div>
        </div>
    }
}

/// Grid / list switch — a `radiogroup`, so it is one tab stop with roving
/// focus and arrow-key selection (the behavior specs/app-ui.md's V1 vendoring
/// findings deferred to this screen).
#[component]
fn ViewSwitch(list_view: Memo<bool>) -> impl IntoView {
    let navigate = use_navigate();
    let query_map = use_query_map();
    let go = {
        let navigate = navigate.clone();
        move |list: bool| {
            let q = query_map.read_untracked().get("q").unwrap_or_default();
            navigate(&catalog_url(&q, list), NavigateOptions::default());
        }
    };

    let on_keydown = {
        let go = go.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            let next = match ev.key().as_str() {
                "ArrowRight" | "ArrowDown" => Some(true),
                "ArrowLeft" | "ArrowUp" => Some(false),
                _ => None,
            };
            if let Some(list) = next {
                ev.prevent_default();
                go(list);
                // Roving focus: selection moved, so the tab stop moved with it
                // and the focus ring has to follow or keyboard users lose their
                // place. `tabindex` is already reactive on `pressed`.
                focus_switch_item(&ev, if list { 1 } else { 0 });
            }
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
                on:click={
                    let go = go.clone();
                    move |_| go(false)
                }
            >
                <span aria-hidden="true">"▦"</span>
                <span class="sr-only">"Grid view"</span>
            </ToggleGroupItem>
            <ToggleGroupItem
                title="List view"
                pressed=Signal::derive(move || list_view.get())
                tabindex=Signal::derive(move || if list_view.get() { 0 } else { -1 })
                {..}
                on:click={
                    let go = go.clone();
                    move |_| go(true)
                }
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

/// The result set: grid of image-led tiles, or the table in list view.
#[component]
fn Results(
    results: Resource<Result<shared::SearchResults, ServerFnError<String>>>,
    last_good: RwSignal<Option<Vec<CardSummary>>>,
    list_view: Memo<bool>,
) -> impl IntoView {
    view! {
        // Transition, not Suspense: re-searching keeps the previous results on
        // screen instead of collapsing the page to skeletons on every keystroke.
        <Transition fallback=|| {
            view! { <ResultsSkeleton /> }
        }>
            {move || Suspend::new(async move {
                match results.await {
                    Ok(r) if r.cards.is_empty() => {
                        view! {
                            <p class="text-muted-foreground py-12 text-center text-sm">
                                "No cards match that search."
                            </p>
                        }
                            .into_any()
                    }
                    Ok(r) => {
                        view! { <ResultCards cards=r.cards list_view stale=false /> }.into_any()
                    }
                    Err(e) => {
                        let (is_query_error, message) = describe_error(&e);
                        let kept = last_good.get_untracked();
                        // The rejected query is a message about the *query*, so
                        // the last page that did parse stays underneath it —
                        // dimmed, because it no longer answers what is in the
                        // box. Blanking here would strobe the results away on
                        // every half-typed term.
                        view! {
                            <p
                                role="alert"
                                data-testid="search-error"
                                class="border-destructive/40 bg-destructive/10 text-destructive rounded-md border px-3 py-2 text-sm"
                            >
                                {if is_query_error {
                                    message
                                } else {
                                    format!("Search failed: {message}")
                                }}
                            </p>
                            {kept
                                .filter(|c| !c.is_empty())
                                .map(|cards| {
                                    view! { <ResultCards cards list_view stale=true /> }
                                })}
                        }
                            .into_any()
                    }
                }
            })}
        </Transition>
    }
}

/// One page of results in whichever layout is selected. `stale` marks a set
/// that no longer matches the query in the box (it is showing under an error).
#[component]
fn ResultCards(cards: Vec<CardSummary>, list_view: Memo<bool>, stale: bool) -> impl IntoView {
    // The layout read must live in a closure, not the component body: a
    // component body runs once, so reading `list_view` there would bake the
    // layout in at construction and the switch would only take effect on the
    // next search.
    let cards = StoredValue::new(cards);
    view! {
        <div
            class=if stale { "pointer-events-none opacity-50" } else { "" }
            data-stale=stale.then_some("true")
            aria-hidden=stale.then_some("true")
        >
            {move || {
                let cards = cards.get_value();
                if list_view.get() {
                    view! { <ResultsList cards /> }.into_any()
                } else {
                    view! { <ResultsGrid cards /> }.into_any()
                }
            }}
        </div>
    }
}

const GRID_CLASS: &str = "grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6";

#[component]
fn ResultsSkeleton() -> impl IntoView {
    view! {
        <div class=GRID_CLASS aria-busy="true" aria-label="Loading results">
            {(0..12).map(|_| view! { <Skeleton class="aspect-[5/7] w-full" /> }).collect_view()}
        </div>
    }
}

#[component]
fn ResultsGrid(cards: Vec<CardSummary>) -> impl IntoView {
    view! {
        <ul class=GRID_CLASS data-testid="results-grid">
            {cards.into_iter().map(|card| view! { <CardTile card /> }).collect_view()}
        </ul>
    }
}

#[component]
fn CardTile(card: CardSummary) -> impl IntoView {
    // The preview renders from this same summary rather than fetching — see
    // `crate::cards::CardPreview`.
    let preview = card.clone();
    let CardSummary {
        oracle_id,
        name,
        image_uri,
        mana_cost,
        type_line,
        owned,
    } = card;
    let href = format!("/cards/{oracle_id}");
    // The whole `<a>` subtree now lives inside CardPreview's children closure,
    // which moves its captures — so the alt text needs its own copy.
    let alt_name = name.clone();
    let subtitle = match (&type_line, &mana_cost) {
        (Some(t), Some(m)) if !m.is_empty() => format!("{t} · {m}"),
        (Some(t), _) => t.clone(),
        (None, Some(m)) => m.clone(),
        (None, None) => String::new(),
    };

    view! {
        <li class="group/tile flex flex-col gap-2">
            // hover=false: the tile is already the card art, so a hover
            // preview would just repeat it smaller. Touch still gets the sheet.
            <CardPreview card=preview hover=false>
            <a
                href=href
                class="focus-visible:ring-ring relative block rounded-lg focus-visible:ring-2 focus-visible:outline-none"
            >
                // The skeleton sits *behind* the image rather than being swapped
                // out on load: no JS, no layout shift, and it is what shows
                // through for a printing with genuinely no art (the multi-face
                // NULLs it used to cover are fixed at the projection now).
                <Skeleton class="aspect-[5/7] w-full" />
                {image_uri
                    .map(|src| {
                        view! {
                            <img
                                src=src
                                alt=alt_name
                                loading="lazy"
                                decoding="async"
                                class="absolute inset-0 size-full rounded-lg object-cover"
                            />
                        }
                    })}
                {(owned.unwrap_or(0) > 0)
                    .then(|| {
                        view! {
                            <span class="absolute right-1.5 top-1.5">
                                <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                                    {format!("{} owned", owned.unwrap_or(0))}
                                </Badge>
                            </span>
                        }
                    })}
            </a>
            </CardPreview>
            <div class="min-w-0">
                <p class="truncate text-sm font-medium" title=name.clone()>
                    {name.clone()}
                </p>
                <p class="text-muted-foreground truncate text-xs">{subtitle}</p>
            </div>
            <QuickActions name />
        </li>
    }
}

#[component]
fn ResultsList(cards: Vec<CardSummary>) -> impl IntoView {
    view! {
        <TableWrapper class="max-h-none">
            <Table {..} data-testid="results-list">
                <TableHeader>
                    <TableRow>
                        <TableHead>"Name"</TableHead>
                        <TableHead class="hidden sm:table-cell">"Type"</TableHead>
                        <TableHead>"Mana"</TableHead>
                        <TableHead class="text-right">"Add"</TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    {cards
                        .into_iter()
                        .map(|card| {
                            let preview = card.clone();
                            let CardSummary { oracle_id, name, mana_cost, type_line, owned, .. } = card;
                            let link_name = name.clone();
                            // The view macro moves captures into per-node
                            // closures, so the link and the quick actions each
                            // need their own copy of the name.
                            view! {
                                <TableRow>
                                    <TableCell class="p-2">
                                        <CardPreview card=preview>
                                            <a
                                                href=format!("/cards/{oracle_id}")
                                                class="font-medium hover:underline"
                                            >
                                                {link_name}
                                            </a>
                                        </CardPreview>
                                        {(owned.unwrap_or(0) > 0)
                                            .then(|| {
                                                view! {
                                                    <Badge
                                                        variant=BadgeVariant::Secondary
                                                        size=BadgeSize::Sm
                                                        class="ml-2"
                                                    >
                                                        {format!("{} owned", owned.unwrap_or(0))}
                                                    </Badge>
                                                }
                                            })}
                                    </TableCell>
                                    <TableCell class="text-muted-foreground hidden p-2 sm:table-cell">
                                        {type_line.unwrap_or_default()}
                                    </TableCell>
                                    <TableCell class="text-muted-foreground p-2">
                                        {mana_cost.unwrap_or_default()}
                                    </TableCell>
                                    <TableCell class="p-2 text-right">
                                        <QuickActions name />
                                    </TableCell>
                                </TableRow>
                            }
                        })
                        .collect_view()}
                </TableBody>
            </Table>
        </TableWrapper>
    }
}

/// `+ Want` / `+ Have` on every result (wireframe). Anonymous visitors get the
/// sign-in prompt this task specifies; the adds themselves are the destination
/// picker's task, so the authed buttons are explicitly inert until then rather
/// than silently doing nothing when clicked.
#[component]
fn QuickActions(name: String) -> impl IntoView {
    let user = expect_context::<CurrentUserResource>().0;
    let location = leptos_router::hooks::use_location();

    view! {
        <div class="flex items-center gap-1.5">
            <Transition fallback=|| ()>
                {move || {
                    let name = name.clone();
                    Suspend::new(async move {
                        let authed = matches!(user.await, Ok(Some(_)));
                        let next = {
                            let path = location.pathname.get_untracked();
                            let search = location.search.get_untracked();
                            let here = if search.is_empty() {
                                path
                            } else {
                                format!("{path}?{search}")
                            };
                            format!("/login?next={}", encode_query_value(&here))
                        };
                        ["Want", "Have"]
                            .into_iter()
                            .map(|kind| {
                                let label = format!("Add {name} to {kind}");
                                if authed {
                                    view! {
                                        <Button
                                            variant=ButtonVariant::Outline
                                            size=ButtonSize::Sm
                                            class="h-7 px-2 text-xs"
                                            {..}
                                            disabled=true
                                            title="Choose a destination — lands with the destination picker"
                                            aria-label=label
                                        >
                                            {format!("+ {kind}")}
                                        </Button>
                                    }
                                        .into_any()
                                } else {
                                    // A link, not a button: the whole point is
                                    // to get an anonymous visitor to sign-in,
                                    // and it must survive with JS disabled.
                                    view! {
                                        <a
                                            href=next.clone()
                                            data-testid="signin-prompt"
                                            aria-label=format!("Sign in to add {name} to {kind}")
                                            class="border-input hover:bg-accent hover:text-accent-foreground inline-flex h-7 items-center rounded-md border px-2 text-xs"
                                        >
                                            {format!("+ {kind}")}
                                        </a>
                                    }
                                        .into_any()
                                }
                            })
                            .collect_view()
                    })
                }}
            </Transition>
        </div>
    }
}
