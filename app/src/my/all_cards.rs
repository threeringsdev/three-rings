//! `/my` and `/my/all` — the All-cards everything-view (specs/app-ui.md →
//! `/my`).
//!
//! The My-cards landing page: every card the caller owns *or* wants, across
//! every collection including Inbox, one keyset page at a time.
//!
//! **Two routes, one body, because a phone's `/my` is not this table.**
//! `design/wireframes.pen` → *Mobile — My cards root* makes `/my` a drill-down
//! list of collections below `md` (built in [`super::root`]), so the table needs
//! a home a phone can reach: [`ALL_CARDS_PATH`] renders it at every width and is
//! what the list's `All cards` row drills into. `/my` still shows the table at
//! `md` and up — it is the shipped desktop landing and the target of every
//! existing `All cards` link, breadcrumb root and `?q=`/`?cursor=` deep link.
//! Both routes share [`AllCardsBody`], which takes its own base path so the
//! query bar and the pager build URLs for the route they are actually on.
//!
//! **`/my/all` is the SSR-complete one; `/my` mounts its table on the client.**
//! `/my` used to emit both markups and let CSS pick, which meant a phone paid
//! for the aggregate read and downloaded fifty rows it never displayed. It now
//! ships the list plus a constant-size skeleton and defers the table to
//! hydration — see [`AllCardsPage`] for the mechanism, the measurements and
//! what it costs a desktop document load. Nothing about [`AllCardsBody`] itself
//! changed.
//!
//! Three more things are worth knowing before editing this file.
//!
//! **The row is a catalog row.** The spec says "same row treatment as
//! collection view", and the cheapest way to guarantee that is to render the
//! *same DTO*: [`shared::AllCardsRow`] carries a whole `CardSummary`, so the
//! name cell reuses [`CardPreview`] (hover card on desktop, bottom sheet on
//! touch, DFC flip control in both) instead of re-deriving a second, drifting
//! notion of what a card row shows.
//!
//! **HERE becomes WHERE.** The collection view's three right-aligned numeric
//! columns are HERE / WANTED / OWNED; here the first is meaningless (there is
//! no "here") and the spec replaces it with an expandable location summary —
//! `7 across 3 collections`, opening to the per-collection breakdown.
//!
//! **The URL is the whole view state.** `?q=` (quick search) and `?cursor=`
//! (keyset page) are the resource's only inputs, so the page SSRs complete
//! markup, is shareable, and Back walks the pages you visited. Editing the
//! search drops the cursor — a new filter has no page two yet.

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};
use leptos_router::NavigateOptions;
use shared::{AllCardsRow, CardLocation};

use super::tree_manage::TreeManage;
use crate::cards::CardPreview;
use crate::catalog::GRID_CLASS;
use crate::components::query_bar::QueryBar;
use crate::components::states::ErrorNote;
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger};
use crate::components::ui::selection_tray::{
    use_selection, SelectedCard, SelectionCheckbox, SelectionKey,
};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};
use crate::components::view_switch::ViewSwitch;
use crate::components::viewport::{media_signal, MD_UP};

/// The keyset page cursor, in the URL beside `?q=`.
const CURSOR_PARAM: &str = "cursor";

/// `?view=grid` renders the tile grid; anything else (including absent) is
/// the table — the **opposite** default from `/catalog` (`?view=list` opts
/// into the table there, grid is what a bare URL shows). `/my` and
/// `/my/all` shipped table-only for a long time before the grid task
/// (WB-01M031Z4MN401FTKNKPE1RZE2E) added the toggle; flipping the *bare*-URL
/// default to match Catalog would have silently changed what every existing
/// bookmark, deep link, and e2e assertion against these two routes renders.
/// The rule that *is* shared with Catalog: the URL only ever spells out the
/// **non-default** view for that surface — see `catalog_url`'s own doc.
pub(crate) const VIEW_PARAM: &str = "view";
const GRID_VIEW: &str = "grid";

pub use super::root::ALL_CARDS_PATH;

/// Is `?view=` asking for the grid? Pure so it is unit-testable without a
/// query map. `pub(crate)`: `super::root::MyRootNav`'s "All cards" row link
/// forwards the bare `/my`'s own `?view=` down into `/my/all` alongside
/// `?q=`/`?cursor=`, so a grid-mode link opened on a phone lands the reader on
/// the same layout once the drill-down target renders it.
pub(crate) fn is_grid_view(raw: Option<&str>) -> bool {
    raw == Some(GRID_VIEW)
}

/// Build `<base>?q=…&view=…&cursor=…`, omitting empty parts — the single place
/// an All-cards URL is constructed, so the query bar, the clear button, the
/// view switch, the pager and the mobile root list's `All cards` row cannot
/// drift on its canonical form. `base` is `/my` or [`ALL_CARDS_PATH`]; the two
/// routes render the same body and must each keep their own URLs.
pub(crate) fn my_url(base: &str, q: &str, list_view: bool, cursor: Option<&str>) -> String {
    let mut url = String::from(base);
    let mut sep = '?';
    if !q.is_empty() {
        url.push(sep);
        url.push_str("q=");
        url.push_str(&crate::catalog::encode_query_value(q));
        sep = '&';
    }
    if !list_view {
        url.push(sep);
        url.push_str(VIEW_PARAM);
        url.push('=');
        url.push_str(GRID_VIEW);
        sep = '&';
    }
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        url.push(sep);
        url.push_str(CURSOR_PARAM);
        url.push('=');
        url.push_str(&crate::catalog::encode_query_value(c));
    }
    url
}

/// What [`AllCardsBody`]'s resource resolves to — the read's own result, behind a
/// **named field**, so its serialized form cannot be mistaken for another
/// resource's.
///
/// This wrapper carries no information. It exists to close a real, shipping
/// wrong-data bug, and the mechanism is worth stating because the next resource
/// added to this app is exposed to it too.
///
/// `leptos_server`'s `initial_value()` reads `__RESOLVED_RESOURCES[<next
/// monotonic id>]` for **every** `Resource::new`, at any time — it never consults
/// the `during_hydration()` flag that `hydration_context` maintains and that
/// `leptos::mount` dutifully flips (`hydrate.rs:145`, `mount.rs:97`). So a
/// resource created during a **client-side navigation** still reads a slot, and
/// that slot belongs to the page you just left. If it decodes, `is_ready` is true
/// and **the fetcher never runs**.
///
/// It decoded. An `SsrMode::Async` route serializes its resources three times at
/// three disjoint id ranges and the client consumes only the first, so the rest
/// are unclaimed slots. `/my/collections/:id` leaves
/// `{"Ok":{"cards":[],"next_cursor":null}}` — the quick-add panel's closed-state
/// search — at ids 8, 12 and 16 (measured), and `shared::AllCardsView` and
/// `shared::SearchResults` are byte-identical when `cards` is empty. Navigating
/// from a collection into `/my` put this resource on **id 12** (measured by
/// removal: dropping slot 12 fixes it, dropping 8 or 16 does not), so `/my`
/// rendered "You haven't added any cards yet." on an account with 100 cards,
/// having issued **zero** requests.
///
/// A named field is the whole fix: `{"Ok":{…}}` cannot decode into
/// `{"all_cards":…}`, so `initial_value` returns `None` and the fetcher runs.
///
/// **What this does not fix**, stated so nobody assumes otherwise: two resources
/// of the *same* type can still cross-decode a correctly-shaped but wrong-query
/// payload (`/my` ↔ `/my/all` with different `?q=`). Only echoing the request
/// back in the payload and rejecting a mismatch would close that, and it is
/// currently latent — measured not to occur at today's id layout.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AllCardsPayload {
    all_cards: Result<shared::AllCardsView, ServerFnError<shared::ApiError>>,
}

/// `/my` — the My-cards landing. Below `md` the drill-down root list the
/// wireframe puts here; at `md` and up the All-cards table.
///
/// **The server ships only the list, and the table mounts client-side**
/// (P6-166). SSR still cannot know the viewport, so it renders the one markup
/// that is correct at every width — the root list plus this table's own chrome
/// — and the aggregate read waits for the client, which *can* know. The
/// mechanism is [`media_signal`]: `false` during SSR and during the hydration
/// render, corrected in an `Effect` afterwards, so nothing about the width is
/// resolved on the server and the hydration render still matches the markup it
/// hydrates. It is the same gate the ⌘K palette already uses, and CSS is still
/// the display authority — [`AllCardsBody`] keeps its `hidden md:flex` and the
/// query names the same 768 px line.
///
/// What this buys, measured on the dev seed (specs/app-ui.md → P6-166): bare
/// `/my` went from 576,473 bytes carrying 50 hidden `<tr>`s to 143,580 with
/// none — 75% of the document, all of it markup a phone never displayed — and
/// stopped blocking SSR on the aggregate read (~820 ms → ~480 ms warm).
///
/// What it costs, stated as plainly as the old comment stated its own cost: a
/// **full document load** of `/my` at desktop width no longer arrives with rows
/// in the HTML — it paints this page's heading and row skeleton and fills in one
/// round trip after hydration. Every *in-app* arrival at `/my` is unaffected,
/// because a client-side navigation always mounted and fetched this table
/// anyway. The SSR-complete table is [`ALL_CARDS_PATH`] (`/my/all`), which
/// renders it at every width and is where the "the table SSRs every row"
/// contract now lives.
#[component]
pub fn AllCardsPage() -> impl IntoView {
    let wide = media_signal(MD_UP);
    view! {
        <super::root::MyRootNav />
        <Show when=move || wide.get() fallback=|| view! { <AllCardsPending /> }>
            <AllCardsBody base="/my" class="hidden md:flex" back=false />
        </Show>
    }
}

/// What `/my` renders where the table will go until the client has told us the
/// viewport is wide enough to want one.
///
/// Constant-size, and that is the whole point: a phone ships this instead of
/// O(50 rows) it will never display, and a desktop sees the page's own chrome
/// rather than an empty column. It carries [`AllCardsBody`]'s wrapper classes
/// and heading verbatim (one [`AllCardsHeading`], not a second copy) so the
/// swap when the real body mounts is the skeleton turning into rows and nothing
/// else moving.
///
/// No [`QueryBar`] here on purpose: a second, throwaway instance of it would
/// accept keystrokes it is about to be unmounted with. The skeleton says
/// "loading", which is true.
#[component]
fn AllCardsPending() -> impl IntoView {
    view! {
        <div class="hidden min-w-0 flex-col gap-4 p-4 md:flex md:p-6">
            <AllCardsHeading />
            <RowsSkeleton />
        </div>
    }
}

/// The page's title block, shared by [`AllCardsBody`] and [`AllCardsPending`]
/// so the pending state cannot drift from the state it becomes.
#[component]
fn AllCardsHeading() -> impl IntoView {
    view! {
        <div>
            <h1 class="text-2xl font-bold">"All cards"</h1>
            <p class="text-muted-foreground text-sm">
                "Every card across your collections, Inbox included."
            </p>
        </div>
    }
}

/// `/my/all` — the same table at *every* width: the drill-down target of the
/// root list's `All cards` row, and the only place a phone can reach it.
#[component]
pub fn AllCardsTablePage() -> impl IntoView {
    view! { <AllCardsBody base=ALL_CARDS_PATH class="flex" back=true /> }
}

/// The table itself. `base` is the route it is mounted at (every URL it builds
/// is relative to it); `back` adds the mobile drill-down's up-link, which only
/// the sub-route needs.
#[component]
fn AllCardsBody(base: &'static str, class: &'static str, back: bool) -> impl IntoView {
    let query_map = use_query_map();

    // Memos, not plain reads: a navigation that changes only the cursor must not
    // invalidate anything keyed on `q` (and vice versa) — Memo suppresses the
    // notification when the value is unchanged.
    let url_q = Memo::new(move |_| query_map.read().get("q").unwrap_or_default());
    let url_cursor = Memo::new(move |_| query_map.read().get(CURSOR_PARAM).unwrap_or_default());
    // "Is the table the one showing right now" — the same name and the same
    // sense `/catalog` uses, even though the two surfaces default in opposite
    // directions (see `VIEW_PARAM`'s doc comment).
    let list_view = Memo::new(move |_| !is_grid_view(query_map.read().get(VIEW_PARAM).as_deref()));

    // The text in the box; the URL⇄field sync lives inside `QueryBar`.
    let query_text = RwSignal::new(url_q.get_untracked());

    // See `collection.rs`: the tray's batch move lives in the shell and cannot
    // refetch this resource directly, so the revision it bumps is one of the
    // resource's sources.
    let revision = crate::my::move_selection::holdings_revision();
    // The same trick for the *collection tree's* mutations: the WHERE column
    // names each row's collection(s) straight out of `all_cards`, which no
    // `tree.refetch()` can update. A sidebar rename or delete left this
    // column naming the old collection until an unrelated search or page turn
    // refetched it. See `TreeManage::revision`.
    let manage = expect_context::<TreeManage>();

    let rows = Resource::new(
        move || {
            (
                url_q.get(),
                url_cursor.get(),
                revision.get(),
                manage.revision.get(),
            )
        },
        |(q, cursor, _revision, _tree_revision)| async move {
            let cursor = (!cursor.is_empty()).then_some(cursor);
            AllCardsPayload {
                all_cards: crate::all_cards(q, cursor).await,
            }
        },
    );

    // Whether we are on a page other than the first — the pager's "Back to the
    // start" affordance, and the reason an empty page is not necessarily an
    // empty collection.
    let paged = Memo::new(move |_| !url_cursor.read().is_empty());

    // The view switch's own navigation: relayouting the page you are on is not
    // a query edit, so the cursor rides along (same rule `/catalog`'s switch
    // follows — see `catalog::ResultsToolbar`'s `go`).
    let navigate = use_navigate();
    let go = move |list: bool| {
        let q = url_q.get_untracked();
        let cursor = url_cursor.get_untracked();
        navigate(
            &my_url(
                base,
                &q,
                list,
                (!cursor.is_empty()).then_some(cursor.as_str()),
            ),
            NavigateOptions::default(),
        );
    };

    view! {
        <div class=format!("min-w-0 flex-col gap-4 p-4 md:p-6 {class}")>
            // The mobile drill-down's up-link: back walks *up*, to the root
            // screen the list is, not to wherever history happens to be — the
            // same rule (and the same idiom) as the collection view's.
            {back
                .then(|| {
                    view! {
                        <a
                            href="/my"
                            class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-sm md:hidden"
                            data-testid="all-cards-back"
                        >
                            <span aria-hidden="true">"‹"</span>
                            "My cards"
                        </a>
                    }
                })}
            <AllCardsHeading />
            <div class="flex flex-wrap items-center gap-2">
                <div class="min-w-0 flex-1">
                    <QueryBar
                        text=query_text
                        url_q
                        // A new search starts at page one: carrying the old cursor
                        // forward would page into a result set that no longer
                        // exists. The current layout rides along untracked, same
                        // reasoning as `/catalog`'s own `QueryBar`.
                        to_url=Callback::new(move |q: String| {
                            my_url(base, &q, list_view.get_untracked(), None)
                        })
                        id="my-query"
                        placeholder="Search your cards by name"
                        aria_label="Search your cards"
                    />
                </div>
                <ViewSwitch list_view on_change=Callback::new(go) />
            </div>
            // Transition, not Suspense: re-searching keeps the previous rows on
            // screen instead of collapsing the table on every keystroke.
            <Transition fallback=|| {
                view! { <RowsSkeleton /> }
            }>
                {move || {
                    // Read the URL state *here*, in the tracked render scope —
                    // not inside the async block, where the read lands after
                    // the await and outside this effect's dependency set.
                    //
                    // `list_view` is deliberately *not* read here: this closure
                    // is the `Transition`'s child, and reading it here would
                    // re-run this whole match (and re-await `rows`) on a pure
                    // layout toggle. `CardsView`/`Pager`/`EmptyState` below each
                    // take the `Memo` itself and read it in their own reactive
                    // scope instead — the same split `crate::catalog::ResultCards`
                    // uses, and its own doc explains why (a fixed href/branch
                    // baked at this level would need the next search to notice a
                    // switch made in between).
                    let q = url_q.get();
                    let searching = !q.is_empty();
                    Suspend::new(async move {
                    match rows.await.all_cards {
                        Ok(view) if view.cards.is_empty() => {
                            view! { <EmptyState searching paged base q list_view /> }.into_any()
                        }
                        Ok(view) => {
                            let next = view.next_cursor.clone();
                            view! {
                                <CardsView rows=view.cards list_view />
                                <Pager next paged q base list_view />
                            }
                                .into_any()
                        }
                        // **The arm the pager does not reach.** `Pager` renders
                        // only under `Ok`, so past a cursor this page used to be
                        // a message and nothing else — and the failure that puts
                        // you here most often is a *shared or bookmarked*
                        // `?cursor=` gone stale, where there is nothing in the box
                        // to fix and no way back. That is the case
                        // `/catalog`'s own error arm was given a way home for; it
                        // is the same defect and this is the same fix.
                        Err(e) => {
                            let q = StoredValue::new(q);
                            view! {
                                <ErrorNote
                                    what="Couldn't load your cards"
                                    e
                                    testid="all-cards-error"
                                    retry=Callback::new(move |()| rows.refetch())
                                >
                                    <Show when=move || paged.get()>
                                        <a
                                            href=move || {
                                                my_url(base, &q.get_value(), list_view.get(), None)
                                            }
                                            class="text-destructive text-sm font-medium underline"
                                            data-testid="page-first"
                                        >
                                            "← Back to the start"
                                        </a>
                                    </Show>
                                </ErrorNote>
                            }
                                .into_any()
                        }
                    }
                    })
                }}
            </Transition>
        </div>
    }
}

#[component]
fn RowsSkeleton() -> impl IntoView {
    view! {
        <div class="space-y-2" aria-busy="true" aria-label="Loading your cards">
            {(0..8).map(|_| view! { <Skeleton class="h-10 w-full" /> }).collect_view()}
        </div>
    }
}

/// Nothing to show. Which of the three reasons it is matters: an empty
/// collection wants a pointer at the catalog, a filtered-out search wants its
/// term blamed, and a walked-past-the-end page wants a way back.
///
/// `q` is the current search and `base` the route (`/my` or [`ALL_CARDS_PATH`])
/// — together with `list_view` they build page one *of the current search, in
/// the current layout*: dropping the query along with the cursor made the way
/// out cost the user their search, and baking the layout at construction made
/// it cost them their grid/list choice the moment they toggled it after this
/// state was already on screen (`list_view` is read reactively inside the
/// `href` closure for exactly that reason — see `AllCardsBody`'s comment on
/// why the layout Memo is threaded down rather than read at the call site).
#[component]
fn EmptyState(
    searching: bool,
    paged: Memo<bool>,
    base: &'static str,
    q: String,
    list_view: Memo<bool>,
) -> impl IntoView {
    let q = StoredValue::new(q);
    view! {
        <div class="text-muted-foreground py-12 text-center text-sm" data-testid="all-cards-empty">
            <Show
                when=move || paged.get()
                fallback=move || {
                    if searching {
                        view! { <p>"No cards of yours match that search."</p> }.into_any()
                    } else {
                        view! {
                            <p>
                                "You haven't added any cards yet. "
                                <a href="/catalog" class="underline">
                                    "Browse the catalog"
                                </a> " to start."
                            </p>
                        }
                            .into_any()
                    }
                }
            >
                <p>
                    "Nothing on this page. "
                    <a
                        href=move || my_url(base, &q.get_value(), list_view.get(), None)
                        class="underline"
                    >
                        "Back to the start"
                    </a> "."
                </p>
            </Show>
        </div>
    }
}

/// One resolved keyset page in whichever layout is selected — the table or the
/// tile grid. Mirrors `crate::catalog::ResultCards`: the layout read lives in
/// this closure, not in the caller that resolved `rows`, so a pure view-switch
/// click does not need to re-await the `all_cards` resource (see
/// `AllCardsBody`'s comment on the same split).
#[component]
fn CardsView(rows: Vec<AllCardsRow>, list_view: Memo<bool>) -> impl IntoView {
    let rows = StoredValue::new(rows);
    view! {
        {move || {
            let rows = rows.get_value();
            if list_view.get() {
                view! { <CardsTable rows /> }.into_any()
            } else {
                view! { <AllCardsGrid rows /> }.into_any()
            }
        }}
    }
}

/// The grid layout's tiles, capped the same way `/catalog`'s own grid is
/// (`crate::catalog::GRID_CLASS`) so the two column/breakpoint schemes cannot
/// drift apart.
#[component]
fn AllCardsGrid(rows: Vec<AllCardsRow>) -> impl IntoView {
    view! {
        <ul class=GRID_CLASS data-testid="all-cards-grid">
            {rows.into_iter().map(|row| view! { <AllCardsTile row /> }).collect_view()}
        </ul>
    }
}

/// One tile: image, name, and the ownership badges the table's OWNED/WANTED
/// cells carry — the essentials the task called for. The WHERE column's
/// per-collection breakdown is deliberately left off the tile (it is the one
/// column with no fixed-width home on a card-sized tile — see the module doc
/// on `LocationSummary`'s three shapes); a reader who wants it switches to
/// list, which is one click away in the same control.
///
/// Selection stays reachable here: `SelectionCheckbox` is a sibling of the
/// `<a>`, not nested inside it (nesting it would let a tap both toggle the
/// selection *and* follow the card link, since a click on a descendant
/// bubbles to the anchor's own click). Whether that reach is the right call is
/// a real design decision, not an oversight — see specs/app-ui.md's Findings
/// entry for the grid-toggle task for the alternative considered and why this
/// one was picked.
#[component]
fn AllCardsTile(row: AllCardsRow) -> impl IntoView {
    let owned = row.owned();
    let AllCardsRow { card, wanted, .. } = row;
    let preview = card.clone();
    let oracle_id = card.oracle_id;
    let href = format!("/cards/{oracle_id}");
    let alt_name = card.name.clone();
    let link_name = card.name.clone();
    let image_uri = card.image_uri.clone();

    let selection = use_selection();
    let key = SelectionKey::Card { oracle_id };
    let selected = selection.selected(key);
    let selectable = (owned > 0).then(|| SelectedCard {
        key,
        oracle_id,
        name: card.name.clone(),
        image_uri: card.image_uri.clone(),
    });

    view! {
        <li
            class="group/tile flex flex-col gap-2"
            data-testid="all-cards-tile"
            data-oracle=oracle_id.to_string()
            data-state=move || selected.get().then_some("selected")
        >
            <div class="relative">
                // hover=false: the tile is already the card art, same call
                // `catalog::CardTile` makes — a hover card over it would be a
                // smaller copy of the image already on screen. The touch sheet
                // (and its DFC flip) stays on regardless.
                <CardPreview card=preview hover=false>
                    <a
                        href=href
                        class="focus-visible:ring-ring relative block rounded-lg focus-visible:ring-2 focus-visible:outline-none"
                    >
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
                        <div class="absolute right-1.5 top-1.5 flex flex-col items-end gap-1">
                            {(owned > 0)
                                .then(|| {
                                    view! {
                                        <span data-testid="owned-badge">
                                            <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                                                {format!("{owned} owned")}
                                            </Badge>
                                        </span>
                                    }
                                })}
                            {(wanted > 0)
                                .then(|| {
                                    view! {
                                        <span data-testid="wanted-badge">
                                            <Badge variant=BadgeVariant::Default size=BadgeSize::Sm>
                                                {format!("{wanted} wanted")}
                                            </Badge>
                                        </span>
                                    }
                                })}
                        </div>
                    </a>
                </CardPreview>
                {selectable
                    .map(|card| {
                        view! {
                            <div
                                class="bg-background/90 absolute left-1.5 top-1.5 z-10 rounded-full shadow"
                                data-testid="tile-select"
                            >
                                <SelectionCheckbox selection card />
                            </div>
                        }
                    })}
            </div>
            <div class="min-w-0">
                <p class="truncate text-sm font-medium" title=link_name.clone()>
                    {link_name.clone()}
                </p>
            </div>
        </li>
    }
}

/// One keyset page. Seven columns: the select checkbox, the card, its type and
/// mana (both progressive — they drop out on narrow screens rather than
/// squeezing the numbers), then the three the spec calls for.
///
/// The two numeric columns also tighten their padding below `sm`. Dropping Type
/// and Mana was not quite enough: `WANTED` and `OWNED` are sized by their own
/// header words, and the table's intrinsic width came out 3 px over a 390 px
/// screen — a wrapper-local sideways scroll, invisible to a document-level
/// assertion (specs/app-ui.md:1198). It went unnoticed while this table was
/// desktop-only in practice; `/my/all` makes it a phone surface.
#[component]
fn CardsTable(rows: Vec<AllCardsRow>) -> impl IntoView {
    view! {
        <TableWrapper class="max-h-none">
            <Table {..} data-testid="all-cards-table">
                <TableHeader>
                    <TableRow>
                        // `w-11` below `md` is the select control's 44 px touch
                        // target (`SelectionCheckbox`); WHERE joins WANTED and
                        // OWNED on `px-1` at phone width to pay for it.
                        <TableHead class="w-11 md:w-8">
                            <span class="sr-only">"Select"</span>
                        </TableHead>
                        <TableHead>"Card"</TableHead>
                        // `lg`, not `md` — see the matching comment in
                        // `my/collection.rs`'s `CollectionTable` header (P6-001).
                        <TableHead class="hidden lg:table-cell">"Type"</TableHead>
                        <TableHead class="hidden sm:table-cell">"Mana"</TableHead>
                        <TableHead class="px-1 md:px-2">"Where"</TableHead>
                        // Abbreviated below `sm` (P6-001): a `TableHead`'s own
                        // word sets its column's intrinsic min-width under
                        // `table-layout: auto`, and at 320 px "Wanted" +
                        // "Owned" alone were most of the 40 px the wrapper
                        // scrolled by. Full words return at `sm`, where
                        // there's room.
                        <TableHead class="px-1 text-right md:px-2">
                            <span class="sm:hidden">"Want"</span>
                            <span class="hidden sm:inline">"Wanted"</span>
                        </TableHead>
                        <TableHead class="px-1 text-right md:px-2">
                            <span class="sm:hidden">"Own"</span>
                            <span class="hidden sm:inline">"Owned"</span>
                        </TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    {rows.into_iter().map(|row| view! { <CardsRow row /> }).collect_view()}
                </TableBody>
            </Table>
        </TableWrapper>
    }
}

#[component]
fn CardsRow(row: AllCardsRow) -> impl IntoView {
    let owned = row.owned();
    let AllCardsRow {
        card,
        wanted,
        locations,
    } = row;
    // The preview renders from this same summary rather than fetching — see
    // `crate::cards::CardPreview`.
    let preview = card.clone();
    let oracle_id = card.oracle_id;
    let link_name = card.name.clone();
    let type_line = card.type_line.clone().unwrap_or_default();
    let mana_cost = card.mana_cost.clone().unwrap_or_default();

    // Selectable only where there is something to move. A row here can be
    // desire-only (`owned == 0`, held nowhere) — the FULL OUTER JOIN correction
    // this view needed — and a selection tray that offered to move copies that
    // do not exist would be lying about what the checkbox does.
    //
    // The key is `Card { oracle }`, not a printing: this view aggregates every
    // collection per oracle card and `card.printing_id` is only the
    // *representative* printing (the has-art-first pick), so neither the source
    // collection nor the held printing is answerable from the row. See
    // `SelectionKey`.
    let selection = use_selection();
    let key = SelectionKey::Card { oracle_id };
    let selected = selection.selected(key);
    let selectable = (owned > 0).then(|| SelectedCard {
        key,
        oracle_id,
        name: card.name.clone(),
        image_uri: card.image_uri.clone(),
    });

    view! {
        <TableRow
            {..}
            data-testid="all-cards-row"
            data-oracle=oracle_id.to_string()
            data-state=move || selected.get().then_some("selected")
        >
            // `p-0` below `md` so the 44 px select target *is* the column
            // rather than 44 px plus 16 px of cell padding.
            <TableCell class="p-0 md:p-2">
                {selectable.map(|card| view! { <SelectionCheckbox selection card /> })}
            </TableCell>
            // `px-1` below `sm` (P6-001): the last 7 px of a 40 px overflow
            // at 320 px, after WHERE/WANTED/OWNED were already at their
            // floor — this card-name column was the remaining slack.
            <TableCell class="px-1 py-2 sm:p-2">
                <CardPreview card=preview>
                    <a href=format!("/cards/{oracle_id}") class="font-medium hover:underline">
                        {link_name}
                    </a>
                </CardPreview>
            </TableCell>
            <TableCell class="text-muted-foreground hidden p-2 lg:table-cell">{type_line}</TableCell>
            <TableCell class="text-muted-foreground hidden p-2 sm:table-cell">{mana_cost}</TableCell>
            // `max-w-0 w-full` (P6-020): under `table-layout: auto` a plain
            // `truncate` on the content does not stop a long, unbreakable
            // collection name from setting this column's min-content width —
            // the column's own auto-computed width is content-driven, and
            // `white-space: nowrap` (which `truncate` sets) removes every
            // wrap opportunity, so the browser's shrink-to-fit pass uses the
            // *whole* name (see specs/app-ui.md's P6-001 section, "Type's own
            // text is untruncated" — the same mechanism, worse here because
            // the name isn't from a bounded vocabulary). `max-w-0` on the
            // cell itself caps this column's own contribution to that pass
            // at zero regardless of content, so the column's width comes
            // from a number instead of from the data, and
            // `LocationSummary`'s `truncate` ellipsizes within it.
            //
            // **A percentage, not `w-full` (WB-01M0AWAM8Z).** P6-020 paired
            // `max-w-0` with `w-full`, which is the "this column takes the
            // table's *whole* leftover width" idiom — and it did: measured at
            // 1440×900, WHERE was 726px of a 1150px table (63%) while every
            // other column collapsed to its own min-content, i.e. its longest
            // word. Card came out 118px (10%) with names on four lines, Type
            // one word per line, Mana one symbol per line — the alpha
            // feedback's complaint exactly. A bounded percentage keeps the
            // data-independence `max-w-0` buys while leaving the rest of the
            // table to be shared content-proportionally the way `/catalog`'s
            // list already is. Two steps because Type is `hidden lg:table-cell`
            // just below: at `lg` there is a seventh column to pay for, so
            // WHERE gives some back. Measured after, at 1440: Card 320 / Type
            // 294 / Mana 99 / WHERE 276, every name on one line, and 0px of
            // wrapper overflow from 390px up (`all-cards.spec.ts`, "the name
            // column is allocated like the catalog's" + the 390px arm).
            <TableCell class="max-w-0 w-[30%] px-1 py-2 md:px-2 lg:w-[24%]">
                <LocationSummary oracle_id owned locations />
            </TableCell>
            <TableCell
                class="px-1 py-2 text-right tabular-nums md:px-2"
                {..}
                data-testid="wanted-count"
            >
                {count_or_dash(wanted)}
            </TableCell>
            <TableCell
                class="px-1 py-2 text-right tabular-nums md:px-2"
                {..}
                data-testid="owned-count"
            >
                {count_or_dash(owned)}
            </TableCell>
        </TableRow>
    }
}

/// A zero count reads as absence, not as a number worth aligning against — the
/// collection view's "OWNED collapses when equal to HERE" instinct, applied to
/// the one case `/my` has.
fn count_or_dash(n: i32) -> String {
    if n > 0 {
        n.to_string()
    } else {
        "—".to_string()
    }
}

/// The column that replaces HERE: where the copies actually are.
///
/// Three shapes, because the spec's `7 across 3 collections` phrasing only
/// works for the plural case:
/// - **nowhere** (a card you want but hold nowhere) — a dash, no control;
/// - **one collection** — `3 in Trade Binder`, linked. A disclosure here would
///   expand to the sentence it is already showing;
/// - **several** — the spec's summary, expandable to the per-collection list.
#[component]
fn LocationSummary(
    oracle_id: shared::Id,
    owned: i32,
    locations: Vec<CardLocation>,
) -> impl IntoView {
    match locations.len() {
        0 => view! {
            <span class="text-muted-foreground" data-testid="location-summary">
                "—"
            </span>
        }
        .into_any(),
        1 => {
            let loc = &locations[0];
            // One line, one truncation: the count is always first, so an
            // end-ellipsis on the whole string cuts into the name, never the
            // count — no need to split them into separate spans. `title`
            // carries the untruncated text for hover/a11y.
            let text = format!("{} in {}", loc.quantity, loc.collection_name);
            view! {
                <a
                    href=format!("/my/collections/{}", loc.collection_id)
                    class="block truncate hover:underline"
                    data-testid="location-summary"
                    title=text.clone()
                >
                    {text.clone()}
                </a>
            }
            .into_any()
        }
        n => {
            // Deterministic, caller-supplied id — the convention every vendored
            // overlay here follows (no `use_random_id`). Unique per row because
            // an oracle id appears at most once in a page.
            let content_id = format!("locations-{oracle_id}");
            view! {
                <Collapsible content_id=content_id.clone()>
                    <CollapsibleTrigger class="group hover:text-foreground text-muted-foreground flex items-center gap-1 text-left">
                        <span
                            aria-hidden="true"
                            class="transition-transform group-data-[state=open]:rotate-90"
                        >
                            "▸"
                        </span>
                        <span data-testid="location-summary">
                            {format!("{owned} across {n} collections")}
                        </span>
                    </CollapsibleTrigger>
                    <CollapsibleContent class="pt-1">
                        <ul class="space-y-0.5" data-testid="location-list">
                            {locations
                                .into_iter()
                                .map(|loc| {
                                    let text = format!("{} · {}", loc.quantity, loc.collection_name);
                                    view! {
                                        <li class="text-muted-foreground text-xs">
                                            <a
                                                href=format!("/my/collections/{}", loc.collection_id)
                                                class="block truncate hover:underline"
                                                title=text.clone()
                                            >
                                                {text.clone()}
                                            </a>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    </CollapsibleContent>
                </Collapsible>
            }
            .into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{count_or_dash, is_grid_view, my_url};

    #[test]
    fn url_omits_empty_parts() {
        assert_eq!(my_url("/my", "", true, None), "/my");
        assert_eq!(my_url("/my", "", true, Some("")), "/my");
        assert_eq!(my_url("/my", "bolt", true, None), "/my?q=bolt");
        assert_eq!(my_url("/my", "", true, Some("abc")), "/my?cursor=abc");
        assert_eq!(
            my_url("/my", "bolt", true, Some("abc")),
            "/my?q=bolt&cursor=abc"
        );
    }

    #[test]
    fn url_percent_encodes_the_query() {
        // A card name can carry `&`, `+`, `/` and spaces (`Fire // Ice`,
        // `Borrowing 100,000 Arrows`); none may be read as URL structure.
        assert_eq!(
            my_url("/my", "fire // ice", true, None),
            "/my?q=fire%20%2F%2F%20ice"
        );
        assert_eq!(
            my_url("/my", "a&b", true, Some("c d")),
            "/my?q=a%26b&cursor=c%20d"
        );
    }

    #[test]
    fn url_stays_on_the_route_it_was_built_for() {
        // The mobile drill-down target keeps its own base: a search or a page
        // taken there must not bounce the reader back to `/my`, which on a
        // phone is the collection list rather than this table.
        assert_eq!(my_url(super::ALL_CARDS_PATH, "", true, None), "/my/all");
        assert_eq!(
            my_url(super::ALL_CARDS_PATH, "bolt", true, None),
            "/my/all?q=bolt"
        );
        assert_eq!(
            my_url(super::ALL_CARDS_PATH, "bolt", true, Some("abc")),
            "/my/all?q=bolt&cursor=abc"
        );
    }

    #[test]
    fn grid_is_the_non_default_view_unlike_catalog() {
        // The opposite polarity from `/catalog`'s `?view=list`, on purpose —
        // see `VIEW_PARAM`'s doc comment. A bare URL (list_view = true) omits
        // the param entirely; choosing grid (list_view = false) is what gets
        // spelled out.
        assert_eq!(my_url("/my", "", false, None), "/my?view=grid");
        assert_eq!(my_url("/my", "bolt", false, None), "/my?q=bolt&view=grid");
        assert_eq!(
            my_url("/my", "bolt", false, Some("abc")),
            "/my?q=bolt&view=grid&cursor=abc"
        );
        assert_eq!(
            my_url(super::ALL_CARDS_PATH, "", false, None),
            "/my/all?view=grid"
        );
    }

    #[test]
    fn is_grid_view_reads_only_the_exact_value() {
        assert!(!is_grid_view(None));
        assert!(!is_grid_view(Some("")));
        assert!(!is_grid_view(Some("list")));
        assert!(!is_grid_view(Some("Grid")));
        assert!(is_grid_view(Some("grid")));
    }

    #[test]
    fn zero_counts_render_as_absence() {
        assert_eq!(count_or_dash(0), "—");
        assert_eq!(count_or_dash(-1), "—");
        assert_eq!(count_or_dash(7), "7");
    }
}

/// Keyset paging controls.
///
/// Forward-only by design: a keyset cursor describes "everything after this
/// row", so a Previous link would need a second, reverse-ordered query and a
/// `before` cursor. Browser Back already walks the pages you came through
/// (each Next is a real history entry), so the only thing missing was a way to
/// jump home — which is what "Back to the start" is.
///
/// **Reactive hrefs, not baked strings** (mirrors `crate::catalog::Pager`'s
/// own finding, specs/app-ui.md → "Catalog paging via `?cursor=`"): this
/// component is mounted once per `(q, cursor)` resolution and stays mounted
/// across a pure view-switch click (see `AllCardsBody`'s comment on why), so a
/// fixed `href` computed at construction would still point at the layout that
/// was current when the page loaded — paging a grid reader back into the
/// table.
#[component]
fn Pager(
    next: Option<String>,
    paged: Memo<bool>,
    q: String,
    base: &'static str,
    list_view: Memo<bool>,
) -> impl IntoView {
    const LINK: &str =
        "border-input hover:bg-accent hover:text-accent-foreground rounded-md border px-3 py-1.5 text-sm";
    let q = StoredValue::new(q);

    view! {
        <nav aria-label="Pagination" class="flex items-center justify-between gap-2">
            <Show when=move || paged.get() fallback=|| view! { <span></span> }>
                <a
                    href=move || my_url(base, &q.get_value(), list_view.get(), None)
                    class=LINK
                    data-testid="page-first"
                >
                    "← Back to the start"
                </a>
            </Show>
            {next
                .map(|c| {
                    let c = StoredValue::new(c);
                    view! {
                        <a
                            href=move || {
                                my_url(base, &q.get_value(), list_view.get(), Some(&c.get_value()))
                            }
                            class=format!("{LINK} ml-auto")
                            data-testid="page-next"
                        >
                            "Next page →"
                        </a>
                    }
                })}
        </nav>
    }
}
