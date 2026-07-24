//! `/my` — the All-cards everything-view (specs/app-ui.md → `/my`).
//!
//! The My-cards landing page: every card the caller owns *or* wants, across
//! every collection including Inbox, one keyset page at a time.
//!
//! Three things are worth knowing before editing this file.
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
use leptos_router::hooks::use_query_map;
use shared::{AllCardsRow, CardLocation};

use crate::cards::CardPreview;
use crate::components::query_bar::QueryBar;
use crate::components::ui::collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

/// The keyset page cursor, in the URL beside `?q=`.
const CURSOR_PARAM: &str = "cursor";

/// Build `/my?q=…&cursor=…`, omitting empty parts — the single place a `/my`
/// URL is constructed, so the query bar, the clear button and the pager cannot
/// drift on its canonical form.
fn my_url(q: &str, cursor: Option<&str>) -> String {
    let mut url = String::from("/my");
    let mut sep = '?';
    if !q.is_empty() {
        url.push(sep);
        url.push_str("q=");
        url.push_str(&crate::catalog::encode_query_value(q));
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

#[component]
pub fn AllCardsPage() -> impl IntoView {
    let query_map = use_query_map();

    // Memos, not plain reads: a navigation that changes only the cursor must not
    // invalidate anything keyed on `q` (and vice versa) — Memo suppresses the
    // notification when the value is unchanged.
    let url_q = Memo::new(move |_| query_map.read().get("q").unwrap_or_default());
    let url_cursor = Memo::new(move |_| query_map.read().get(CURSOR_PARAM).unwrap_or_default());

    // The text in the box; the URL⇄field sync lives inside `QueryBar`.
    let query_text = RwSignal::new(url_q.get_untracked());

    let rows = Resource::new(
        move || (url_q.get(), url_cursor.get()),
        |(q, cursor)| async move {
            let cursor = (!cursor.is_empty()).then_some(cursor);
            crate::all_cards(q, cursor).await
        },
    );

    // Whether we are on a page other than the first — the pager's "Back to the
    // start" affordance, and the reason an empty page is not necessarily an
    // empty collection.
    let paged = Memo::new(move |_| !url_cursor.read().is_empty());

    view! {
        <div class="flex min-w-0 flex-col gap-4 p-4 md:p-6">
            <div>
                <h1 class="text-2xl font-bold">"All cards"</h1>
                <p class="text-muted-foreground text-sm">
                    "Every card across your collections, Inbox included."
                </p>
            </div>
            <QueryBar
                text=query_text
                url_q
                // A new search starts at page one: carrying the old cursor
                // forward would page into a result set that no longer exists.
                to_url=Callback::new(|q: String| my_url(&q, None))
                id="my-query"
                placeholder="Search your cards by name"
                aria_label="Search your cards"
            />
            // Transition, not Suspense: re-searching keeps the previous rows on
            // screen instead of collapsing the table on every keystroke.
            <Transition fallback=|| {
                view! { <RowsSkeleton /> }
            }>
                {move || {
                    // Read the URL state *here*, in the tracked render scope —
                    // not inside the async block, where the read lands after
                    // the await and outside this effect's dependency set.
                    let q = url_q.get();
                    let searching = !q.is_empty();
                    Suspend::new(async move {
                    match rows.await {
                        Ok(view) if view.cards.is_empty() => {
                            view! { <EmptyState searching paged /> }.into_any()
                        }
                        Ok(view) => {
                            let next = view.next_cursor.clone();
                            view! {
                                <CardsTable rows=view.cards />
                                <Pager next paged q />
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! {
                                <p
                                    role="alert"
                                    data-testid="all-cards-error"
                                    class="border-destructive/40 bg-destructive/10 text-destructive rounded-md border px-3 py-2 text-sm"
                                >
                                    {format!("Couldn't load your cards: {}", message_of(&e))}
                                </p>
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

/// The human-facing half of a server-fn error (the transport only carries
/// `ApiError`'s `Display` string).
fn message_of(e: &ServerFnError<String>) -> String {
    match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
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
#[component]
fn EmptyState(searching: bool, paged: Memo<bool>) -> impl IntoView {
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
                    "Nothing on this page. " <a href="/my" class="underline">"Back to the start"</a>
                    "."
                </p>
            </Show>
        </div>
    }
}

/// One keyset page. Six columns: the card, its type and mana (both progressive
/// — they drop out on narrow screens rather than squeezing the numbers), then
/// the three the spec calls for.
#[component]
fn CardsTable(rows: Vec<AllCardsRow>) -> impl IntoView {
    view! {
        <TableWrapper class="max-h-none">
            <Table {..} data-testid="all-cards-table">
                <TableHeader>
                    <TableRow>
                        <TableHead>"Card"</TableHead>
                        <TableHead class="hidden md:table-cell">"Type"</TableHead>
                        <TableHead class="hidden sm:table-cell">"Mana"</TableHead>
                        <TableHead>"Where"</TableHead>
                        <TableHead class="text-right">"Wanted"</TableHead>
                        <TableHead class="text-right">"Owned"</TableHead>
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

    view! {
        <TableRow {..} data-testid="all-cards-row" data-oracle=oracle_id.to_string()>
            <TableCell class="p-2">
                <CardPreview card=preview>
                    <a href=format!("/cards/{oracle_id}") class="font-medium hover:underline">
                        {link_name}
                    </a>
                </CardPreview>
            </TableCell>
            <TableCell class="text-muted-foreground hidden p-2 md:table-cell">{type_line}</TableCell>
            <TableCell class="text-muted-foreground hidden p-2 sm:table-cell">{mana_cost}</TableCell>
            <TableCell class="p-2">
                <LocationSummary oracle_id owned locations />
            </TableCell>
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="wanted-count">
                {count_or_dash(wanted)}
            </TableCell>
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="owned-count">
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
            view! {
                <a
                    href=format!("/my/collections/{}", loc.collection_id)
                    class="hover:underline"
                    data-testid="location-summary"
                >
                    {format!("{} in {}", loc.quantity, loc.collection_name)}
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
                    // No padding on the content wrapper: `class` lands on the
                    // *inner* div, whose padding survives the closed state's
                    // `grid-rows-[0fr]` (`min-h-0` zeroes the content box, not
                    // the padding box) and leaves a sliver of height under
                    // every collapsed row. Spacing goes on the list instead.
                    <CollapsibleContent>
                        <ul class="space-y-0.5 pt-1" data-testid="location-list">
                            {locations
                                .into_iter()
                                .map(|loc| {
                                    view! {
                                        <li class="text-muted-foreground text-xs">
                                            <a
                                                href=format!("/my/collections/{}", loc.collection_id)
                                                class="hover:underline"
                                            >
                                                {format!("{} · {}", loc.quantity, loc.collection_name)}
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
    use super::{count_or_dash, my_url};

    #[test]
    fn url_omits_empty_parts() {
        assert_eq!(my_url("", None), "/my");
        assert_eq!(my_url("", Some("")), "/my");
        assert_eq!(my_url("bolt", None), "/my?q=bolt");
        assert_eq!(my_url("", Some("abc")), "/my?cursor=abc");
        assert_eq!(my_url("bolt", Some("abc")), "/my?q=bolt&cursor=abc");
    }

    #[test]
    fn url_percent_encodes_the_query() {
        // A card name can carry `&`, `+`, `/` and spaces (`Fire // Ice`,
        // `Borrowing 100,000 Arrows`); none may be read as URL structure.
        assert_eq!(my_url("fire // ice", None), "/my?q=fire%20%2F%2F%20ice");
        assert_eq!(my_url("a&b", Some("c d")), "/my?q=a%26b&cursor=c%20d");
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
#[component]
fn Pager(next: Option<String>, paged: Memo<bool>, q: String) -> impl IntoView {
    const LINK: &str =
        "border-input hover:bg-accent hover:text-accent-foreground rounded-md border px-3 py-1.5 text-sm";
    let start_url = my_url(&q, None);
    let next_url = next.as_deref().map(|c| my_url(&q, Some(c)));

    view! {
        <nav aria-label="Pagination" class="flex items-center justify-between gap-2">
            <Show when=move || paged.get() fallback=|| view! { <span></span> }>
                <a href=start_url.clone() class=LINK data-testid="page-first">
                    "← Back to the start"
                </a>
            </Show>
            {next_url
                .map(|url| {
                    view! {
                        <a href=url class=format!("{LINK} ml-auto") data-testid="page-next">
                            "Next page →"
                        </a>
                    }
                })}
        </nav>
    }
}
