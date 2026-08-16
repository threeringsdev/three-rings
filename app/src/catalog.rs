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
//!   keyed on the URL's query *and* its `?cursor=`, so a cold load — page one or
//!   a shared page-three link — renders markup, not a spinner.
//! - **Paging lives in the URL too, two ways.** `?cursor=` is the same
//!   forward-only keyset shape `/my` uses — a position in the result set the
//!   query produced, so **every path that edits the query drops it**; see
//!   [`catalog_url`]. `?page=` is a *second*, independent primitive
//!   (maintainer ruling, 2026-08-15, specs/catalog-search.md "Numbered page
//!   links, round 2"): an explicit page-N jump, turned server-side into an
//!   `OFFSET` under the same sort the cursor uses — see [`PAGE_PARAM`]. When a
//!   URL carries both, the *client* forwards only the cursor (the fetch
//!   closure drops `page`; a legacy shared link carries only a cursor, never
//!   a page) — at the [`CatalogStore::search`] level itself a `page_number`,
//!   when sent, wins over any cursor.
//! - **What is *displayed* comes from the payload, not the URL.** The two
//!   disagree for the whole of every search — `<Transition>` holds the previous
//!   page on screen while the next resolves — so which page this is, how many
//!   rows it holds and where its pager points all read [`SearchPayload`]'s
//!   echoed `q`/`cursor`/`page`. Reading the URL for any of them is how the
//!   pager grew a premature "Back to the start" and how its Next reverted
//!   typed text (specs/app-ui.md "Catalog paging honesty").
//! - **Live typing: ~250 ms debounce, stale-response discard.** Both live in
//!   the shared [`QueryBar`](crate::components::query_bar) — the debounce is
//!   ours, the discard is the reactive layer's. Note what this is *not*: an
//!   overtaken request is discarded on arrival, not aborted in flight.
//! - **Parse errors are results, not failures.** The grammar rejects unknown
//!   terms by design (`ApiError::Validation`, 422) and a half-typed query hits
//!   that constantly, so it renders inline under the bar and leaves the previous
//!   result set alone rather than blanking the page.
//!
//! Anonymous is the default audience here: `/catalog` is public, the search
//! adapter reads the session opportunistically, and the quick actions prompt
//! sign-in rather than disappearing.

pub mod destination;
pub mod rail;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_query_map};
use leptos_router::NavigateOptions;
use shared::CardSummary;

use crate::cards::CardPreview;
use crate::components::query_bar::QueryBar;
use crate::components::states::{StateBadge, Tone};
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};
use crate::components::view_switch::ViewSwitch;
use crate::shell::CurrentUserResource;

/// `?view=list` renders the table; anything else (including absent) is the grid.
/// View mode rides the URL alongside `q` so a reload or a shared link keeps the
/// layout, and so it SSRs correctly instead of flipping after hydration. It is
/// *not* search state — it never enters the query text.
const VIEW_PARAM: &str = "view";
const LIST_VIEW: &str = "list";

/// The catalog's own default page size (WB-01M033AFA0VSCGB8Z3HTYPFZVD,
/// maintainer report): 60, not [`shared::Page`]'s generic default of 50.
/// 60 has far more useful divisors than 50 across the grid's column
/// breakpoints (2, 3, 4, 5, 6 all divide it evenly, vs. only 2, 5, 10 for 50),
/// so a full page tiles cleanly no matter which of [`GRID_CLASS`]'s tiers is
/// showing — the maintainer's own complaint was that 50 "doesn't divide
/// evenly unless you happen to have rows of 5, which the breakpoints never
/// give you."
///
/// Wired in at the two SSR call sites that used to hand the generic
/// [`shared::Page`] default straight through with `limit: None`:
/// `search_catalog` (`app/src/lib.rs`, both the hosted in-process call and the
/// native backend's HTTP forward, since [`crate::backend::native::NativeBackend::search`]
/// echoes whatever `limit` it was given as `?limit=`) and the hosted
/// `/api/catalog/search` route's own fallback (`app/src/backend/routes.rs`)
/// for a caller that hits the HTTP route directly without stating one.
///
/// **Catalog-only, deliberately** — unlike [`GRID_CLASS`], this does not flow
/// to `/my`. `all_cards` and `collection_view` (`app/src/lib.rs`) still pass
/// `limit: None` and get the shared `Page::limit()` default of 50: the
/// maintainer's report named only the catalog page's *count*, and a page-size
/// change is a data-fetch behaviour change (offsets, cursor pages, request
/// volume) for a screen nobody asked to touch — a materially bigger blast
/// radius than reusing a CSS class, so it does not get the same "let it flow"
/// treatment `GRID_CLASS` does.
///
/// `#[cfg(feature = "ssr")]`: both call sites are themselves `ssr`-only (the
/// `search_catalog` server fn's body, the hosted-only `routes.rs`), so a
/// `hydrate`-only build (the wasm client) never references this and would
/// otherwise fail its `-D warnings` gate on dead code.
#[cfg(feature = "ssr")]
pub(crate) const CATALOG_PAGE_SIZE: u32 = 60;

/// The keyset page cursor, in the URL beside `?q=` (specs/catalog-search.md:
/// `/catalog?q=…&cursor=…`, shareable/restorable/SSR-able).
const CURSOR_PARAM: &str = "cursor";

/// The page number, in the URL beside `?q=`/`?cursor=` (WB-01M032Q6BX8BM7NPK8H3AQKGWF,
/// specs/catalog-search.md "Numbered page links"). **Real fetch input as of
/// the maintainer's round-2 ruling (2026-08-15)** — `results` keys on it and,
/// when there is no `?cursor=` riding along, sends it to `search_catalog` as
/// an explicit page-N jump, turned server-side into an `OFFSET` under the same
/// sort the keyset cursor uses. When both are present the client sends only
/// the cursor — the guard lives in `results`' fetch closure, not the store,
/// where a sent `page_number` wins over any cursor (legacy/shared links from
/// before this ruling carry only a cursor, never a page). Omitted for page
/// one, same as `cursor`.
const PAGE_PARAM: &str = "page";

/// A hard ceiling on the page number this screen will ever parse out of the
/// URL or hand to the server — nowhere close to a claim about the catalog's
/// real size (at [`CATALOG_PAGE_SIZE`] rows/page that's 60 million rows), just
/// large enough to need no per-request knowledge of the true last page while
/// still bounding
/// every downstream `usize`/`u32` computation (`page_strip`, `page_offset`)
/// against an adversarial `?page=` (WB-01M032Q6BX8BM7NPK8H3AQKGWF round 2's
/// adversarial-review blocker: `GET /catalog?page=18446744073709551615`
/// reaching unguarded `page + 1` arithmetic panicked an anonymous SSR request
/// in debug builds). `page_offset` (hosted.rs) is `saturating` on its own
/// terms too — this ceiling is defense in depth, not the only guard, so a
/// direct hosted-API caller bypassing this parse entirely still cannot
/// overflow anything server-side.
const MAX_PAGE: usize = 1_000_000;

/// Parse `?page=`, clamped to `1..=MAX_PAGE`. Absent, zero, unparsable, or a
/// number so large it can't even fit a `usize` (`usize::MAX` on a 64-bit
/// build parses fine as a `usize` — the crafted case this function exists to
/// defuse) all land on a sane, bounded value; nothing here can panic or wrap.
fn parse_page(raw: Option<&str>) -> usize {
    raw.and_then(|p| p.parse::<usize>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(1)
        .min(MAX_PAGE)
}

/// Build `/catalog?q=…&view=…&cursor=…&page=…`, omitting empty parts. The
/// single place a catalog URL is constructed, so the canonical form can't
/// drift between the query bar, the clear button, the view switch, the
/// filter rail and the pager.
///
/// **`cursor` is named at every call site on purpose.** A cursor describes a
/// position in the result set *the previous query* produced — the rows after
/// `(name, oracle_id)` (specs/catalog-search.md "Result order and keyset") — so
/// carrying one into a different query pages into rows that need not exist
/// there, and lands on someone else's page or on nothing at all. Every caller
/// that changes `q` therefore passes `None` for both `cursor` and `page`, and
/// there are exactly two of them:
///
/// - [`QueryBar`]'s `to_url` — typing, Enter, and the ✕ clear button;
/// - [`rail::use_navigate_query`] — every facet checkbox, every rail text
///   field, the mana-value row, and Reset.
///
/// The only callers that carry a cursor (and its page label) forward are
/// [`ViewSwitch`] and [`Pager`], neither of which touches the query.
///
/// **`page` and `cursor` name the same thing two ways, and `cursor` wins when
/// both are present** (`CatalogStore::search`'s doc comment). `Pager` never
/// generates both together — every link it builds carries `page` alone — but
/// a **legacy shared link from before round 2** (`?cursor=…`, no `?page=`)
/// still works, unchanged, for fetching: `results` uses the cursor and
/// ignores the absent page. What it does *not* get right is the *label*:
/// `page` there parses to 1 (nothing else to read), so `SearchPayload.page`
/// mislabels a genuinely later page as page one. Callers that key behavior on
/// "is this really page one" (`last_good`) check `cursor.is_empty() &&
/// page <= 1` together for exactly this reason — `page` alone is not enough.
fn catalog_url(q: &str, list_view: bool, cursor: Option<&str>, page: Option<usize>) -> String {
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
        sep = '&';
    }
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        url.push(sep);
        url.push_str(CURSOR_PARAM);
        url.push('=');
        url.push_str(&encode_query_value(c));
        sep = '&';
    }
    if let Some(p) = page.filter(|p| *p > 1) {
        url.push(sep);
        url.push_str(PAGE_PARAM);
        url.push('=');
        url.push_str(&p.to_string());
    }
    url
}

/// Percent-encode a query *value*. Deliberately conservative: the search
/// grammar is punctuation-heavy (`t:instant c:ur cmc<=2`) and `&`, `#`, `+`
/// and friends would otherwise be read as URL structure or as a space.
///
/// `pub(crate)` because every URL-canonical search surface needs the same rule
/// (`/my`'s quick search builds its `?q=` with it).
pub(crate) fn encode_query_value(s: &str) -> String {
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

/// How a catalog search error should be blamed, for display.
///
/// **P6-043.** Used to be a bare `bool` ("is this a query error"), which is
/// exactly what conflated a rejected search term with a corrupt `?cursor=` —
/// both were `ApiError::Validation`, so both got the same treatment. Now that
/// a bad cursor carries its own `ApiError::BadCursor` variant across the
/// wire, this can say which of the two it actually is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryErrorKind {
    /// The grammar rejected a term in `q` — rendered inline verbatim, no
    /// "Search failed" prefix; the message names the offending term.
    Grammar,
    /// The query parsed fine. The `?cursor=` naming a page in its results is
    /// stale, corrupt, or foreign — the query is not at fault, only the page
    /// reference is.
    Cursor,
    /// Anything else — genuine breakage, prefixed "Search failed: ".
    Other,
}

/// Pull the human-facing message out of a server-fn error, and classify it
/// ([`QueryErrorKind`]) so the caller can tell a rejected query from a bad
/// page reference from something actually going wrong.
///
/// **P6-083.** The server-fn wire now carries the typed `ApiError` variant
/// (`crate::api_err`), so a `WrappedServerError(ApiError::Validation(_) |
/// ApiError::BadCursor(_))` is matched directly rather than parsed off a
/// `validation:`/`bad cursor:` prefix. The prefix parse survives as the
/// fallback for `ServerFnError` variants that carry no typed `ApiError` at
/// all (a dropped fetch, e.g.) — treating every one of those as
/// [`QueryErrorKind::Other`] is deliberate, so a partially-typed term doesn't
/// flash "search failed" for a transport hiccup it isn't.
pub(crate) fn describe_error(e: &ServerFnError<shared::ApiError>) -> (QueryErrorKind, String) {
    // `WrappedServerError` is soft-deprecated (server_fn 0.8.8) in favor of
    // authoring a wholly custom `FromServerFnError` type instead of
    // `ServerFnError<CustErr>` — but the generic remains fully supported
    // (`server_fn`'s own test suite asserts `ServerFnError: FromServerFnError`),
    // and matching this variant is the only way to read the typed `ApiError`
    // back out of it.
    #[allow(deprecated)]
    if let ServerFnError::WrappedServerError(api_err) = e {
        return match api_err {
            shared::ApiError::Validation(msg) => (QueryErrorKind::Grammar, msg.clone()),
            shared::ApiError::BadCursor(msg) => (QueryErrorKind::Cursor, msg.clone()),
            other => (QueryErrorKind::Other, other.message().to_string()),
        };
    }
    let raw = match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
    };
    if let Some(rest) = raw.strip_prefix("validation: ") {
        return (QueryErrorKind::Grammar, rest.to_string());
    }
    if let Some(rest) = raw.strip_prefix("bad cursor: ") {
        return (QueryErrorKind::Cursor, rest.to_string());
    }
    (QueryErrorKind::Other, raw)
}

/// The catalog search resource's payload — a **named field**, for exactly the
/// reason [`crate::my::all_cards`]'s `AllCardsPayload` is one, and against a
/// second live instance of the same bug.
///
/// `leptos_server`'s `initial_value()` reads `__RESOLVED_RESOURCES[<next
/// monotonic id>]` for **every** `Resource::new` without consulting
/// `during_hydration()`, so a resource created during a client-side navigation
/// reads a slot belonging to the page you just left. If it decodes, the fetcher
/// never runs.
///
/// It decoded, and this pair is worse than the `/my` one because the wrong data
/// is *plausible*. `shared::SearchResults { cards, next_cursor }` and
/// `shared::CollectionView { collection, children, cards, next_cursor, totals,
/// commanders }` cross-decode: serde ignored the four extra keys, and
/// `shared::CardRow` is a structural superset of `shared::CardSummary`
/// (`printing_id: Id` widens into `Option<Id>`, `owned: i32` into `Option<i32>`,
/// `faces` is `serde(default)`). So clicking **Catalog** from a collection page
/// re-rendered *that collection's cards* as catalog results, under a confident
/// "11 results", having issued **zero** requests.
///
/// Measured before the fix (responsive audit, 2026-07-26): Commander Deck → 11
/// tiles / "11 results", Depth Box → 3 / "3", Depth Shelf → 1 / "1", Shoebox →
/// 1 / "1", each reproducing on both attempts and via both the desktop mode
/// switch and the mobile bottom tab; Inbox, Rares, Bulk Box, Trade Binder and
/// Depth Drawer were correct. Collection-dependent for the same reason `/my`'s
/// was — the client id counter sits somewhere different after each page builds
/// its own resources — which is why a single spot check cleared it. Pinned by
/// removal: deleting serialized slot 6 or 14, both carrying the `collection_view`
/// payload, made the search fetch correctly.
///
/// `{"Ok":{"collection":…}}` cannot decode into `{"search":…}`, so
/// `initial_value` returns `None` and the fetcher runs.
///
/// **What this still does not fix**, restated because it is now the *only*
/// remaining hole rather than one of two: two resources of the **same** type can
/// cross-decode a correctly-shaped but wrong-query payload. Closing that needs
/// the payload to echo the request it answered and the consumer to reject a
/// mismatch. Measured not to occur at today's id layout — latent, not active.
///
/// **The payload echoes its request** (`q`, `cursor`) — added 2026-08-12 with
/// the paging-honesty batch. It is not (yet) the mismatch *rejection* the
/// paragraph above asks for, but it is the half that everything downstream
/// needed anyway: `<Transition>` keeps a previously-resolved page on screen
/// while a newer search runs, so the URL and the rendered results routinely
/// disagree. Anything that describes what is *on screen* — which page it is,
/// how many rows it holds, where its pager points — has to read the payload
/// that produced it, not the URL that has already moved on.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchPayload {
    /// The query these results answer.
    pub(crate) q: String,
    /// The cursor they were fetched with; empty means page one.
    pub(crate) cursor: String,
    /// The page number this request was made under (1 when absent/unparsable
    /// — see [`PAGE_PARAM`]/[`parse_page`]); when `cursor` is empty, this is
    /// what actually drove the fetch (an explicit `OFFSET` jump), not just a
    /// label. Echoed for the same reason `q`/`cursor` are: the pager describes
    /// the payload on screen, not whatever the URL has moved on to under a
    /// `<Transition>`.
    pub(crate) page: usize,
    pub(crate) search: Result<shared::SearchResults, ServerFnError<shared::ApiError>>,
}

/// `search_count`'s answer, tagged with the query it answers — the same
/// "echo the query back" shape [`SearchPayload`] uses, and for the same
/// reason: `results` and `search_count` are two independent round trips of
/// similar latency, so either can resolve first. Without the echo, nothing
/// downstream could tell "this is the count for what's on screen right now"
/// from "this is the count for a query the reader has since edited past" —
/// `Pager` only ever needed the number, but the header's count line needs to
/// know when its own number has fallen behind (WB-01M0324HQ12B590CZ0YXJPB5T6
/// round 2, specs/catalog-search.md).
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CountPayload {
    /// The query this count answers.
    pub(crate) q: String,
    /// `None` only when `search_catalog_count` itself errored.
    pub(crate) count: Option<i64>,
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
    let url_cursor = Memo::new(move |_| query_map.read().get(CURSOR_PARAM).unwrap_or_default());
    // `parse_page` clamps and never panics — see its own doc comment
    // (WB-01M032Q6BX8BM7NPK8H3AQKGWF round 2's adversarial-review blocker).
    let url_page = Memo::new(move |_| parse_page(query_map.read().get(PAGE_PARAM).as_deref()));

    // The text in the box. The URL⇄field sync lives in QueryBar, which is the
    // only thing that writes either one.
    let query_text = RwSignal::new(url_q.get_untracked());

    // One page of results. Keyed on the URL's query, cursor *and* page: the
    // debounce decides *when* the URL moves, this decides what is displayed
    // once it has. A navigation that changes only `view` moves none of the
    // three, so switching layout does not re-search.
    //
    // **`page` is real fetch input now** (maintainer ruling, 2026-08-15 —
    // specs/catalog-search.md "Numbered page links"), not just a label: when
    // there is no `cursor`, a `page > 1` becomes an explicit jump —
    // `search_catalog`'s `page` argument, turned server-side into an `OFFSET`
    // under the same sort the keyset cursor uses. A `cursor` still wins when
    // both are present (a legacy/shared link from before this ruling carries
    // only a cursor, never a page) — see `catalog_url`'s doc comment.
    let results = Resource::new(
        move || (url_q.get(), url_cursor.get(), url_page.get()),
        |(q, cursor, page)| async move {
            let cursor_arg = (!cursor.is_empty()).then(|| cursor.clone());
            let page_arg = (cursor.is_empty() && page > 1).then_some(page as u32);
            let search = crate::search_catalog(q.clone(), cursor_arg, page_arg).await;
            SearchPayload {
                q,
                cursor,
                page,
                search,
            }
        },
    );

    // The last result set that came back OK, and the query it answered. A
    // rejected query must not take the results down with it
    // (specs/catalog-search.md: half-typed queries hit the grammar's
    // term-naming error constantly), so the error renders *over* the last good
    // page rather than replacing it. Effects don't run during SSR, which is
    // correct here — a cold load that errors has no previous page to keep.
    //
    // **Only page one is ever kept, and paging away forgets it** (P6-131). The
    // set exists to survive a *typing* error, and every edit to the query drops
    // the cursor (and the page), so the page an error lands on is always page
    // one. Retaining a paged result instead put "rows 51–73 of a search you
    // have since left" under an error about a different one — dimmed and
    // labelled "Previous results", which made it look deliberate.
    //
    // Checked on **both** `p.cursor` and `p.page` now: an explicit page-N
    // jump also fetches with an empty cursor (round 2's offset path), so
    // cursor-emptiness alone no longer means "page one" — and a legacy
    // `?cursor=`-only link (no `page`) still defaults `page` to 1 despite
    // answering a later page (`catalog_url`'s doc comment has the caveat).
    // Only a fetch that is unambiguously page one on *both* counts is kept.
    let last_good = RwSignal::new(None::<(String, Vec<CardSummary>)>);
    Effect::new(move |_| {
        let Some(p) = results.get() else { return };
        match p.search {
            Ok(r) if p.cursor.is_empty() && p.page <= 1 => last_good.set(Some((p.q, r.cards))),
            Ok(_) => last_good.set(None),
            // An error keeps whatever is there — that *is* the feature.
            Err(_) => {}
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

    // The row count *for this query* — filtered or browse-all, either way —
    // powering the pager's true-last-page number (maintainer ruling,
    // 2026-08-15). A second, independent request from `results`/`count`
    // itself, keyed the same way `count` is (fires once per settled query,
    // same debounce protection) but never awaited alongside `results`: `Pager`
    // reads it with a plain, non-blocking `.get()`, so the strip renders with
    // `results` immediately and *upgrades* once this resolves — never the
    // reverse. Kept off the per-keystroke path only by never being on it:
    // nothing here runs any more often than `results` itself already does.
    //
    // Resolves to [`CountPayload`], not a bare `Option<i64>` — the `q` it
    // echoes back is how the header's count line tells its own number apart
    // from a still-in-flight query's leftover answer (round 2's staleness
    // fix; see `CountPayload`'s doc comment).
    let search_count = Resource::new(
        move || url_q.get(),
        |q| async move {
            let count = crate::search_catalog_count(q.clone())
                .await
                .ok()
                .map(|c| c.cards);
            CountPayload { q, count }
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
                // The filtered counterpart to the line above — same data source
                // `Pager` already reads for its true-last-page number
                // (`search_count`, specs/catalog-search.md "Numbered page
                // links, round 2"), no second request.
                //
                // **The outer `move ||` closure reads nothing synchronously —
                // not even `url_q`, on purpose.** An earlier revision read
                // `url_q.get()` here to skip `search_count` entirely for an
                // empty (browse-all) query, which seemed like the obvious
                // place to gate it. That made this closure itself a tracked
                // reactive computation, rebuilding a *brand new* `Suspend`
                // cycle the instant `url_q` changed — independent of, and
                // ahead of, whether `search_count`'s own fetch for the
                // *previous* cycle had resolved. That premature rebuild
                // disposes the previous cycle's reactive `Owner` — including
                // `displaced_by`'s `Signal::derive`, below — while its content
                // is still the one `<Transition>` is displaying, and reading
                // that now-disposed signal panics (`unreachable`, taking the
                // whole wasm module — and `results` alongside it, not just
                // this line — down with it; reproduced live,
                // WB-01M0324HQ12B590CZ0YXJPB5T6 round 2, adversarial review).
                // `Results`' own outer closure (above, and again below) reads
                // nothing synchronously either, for the identical reason —
                // its only reactive dependency is the `.await` inside, so a
                // new cycle begins only when `Suspend`'s own machinery decides
                // a fresh value is ready, never on a raw signal tick. This
                // mirrors that: `search_count` is unconditionally awaited, and
                // "was this query empty" is read *after*, off the payload's
                // own echoed `q` — a fact about the resolved cycle, not a
                // trigger for starting a new one.
                <Transition fallback=|| ()>
                    {move || Suspend::new(async move {
                        // Deliberately the same non-blocking relationship
                        // `Pager` has with this resource: this `Transition`
                        // is its own boundary, entirely separate from
                        // `Results`' — so a slow count never holds up the
                        // cards rendering, only this line's own text.
                        let CountPayload { q, count } = search_count.await;
                        if q.is_empty() {
                            return None;
                        }
                        let label = count.and_then(filtered_count_label)?;
                        // `results` and `search_count` are two independent
                        // round trips of similar latency — either can
                        // resolve first. When `results` wins the race, the
                        // body can already show `NoResults` for the new
                        // (zero-hit) query while this line is still showing
                        // the *previous* query's number: real, not corrupted
                        // (the reactive graph's version stamp guarantees that
                        // much), but not a fact about what's on screen
                        // either, and presenting it unqualified reads as a
                        // contradiction, not a lag.
                        //
                        // `stale`'s comparison is `Pager`'s own signal, reused
                        // verbatim (round 2, WB-01M0324HQ12B590CZ0YXJPB5T6)
                        // rather than reinvented: `displaced_by` compares
                        // *this number's own* echoed query (`q`, just
                        // unpacked above) against the live URL/box, the same
                        // comparison `Pager` already makes against `results`'
                        // echoed query.
                        //
                        // Rendered through a genuine child `#[component]`
                        // (`FilteredCount`, below) rather than inline here —
                        // deliberate belt-and-suspenders, not the fix itself:
                        // removing the synchronous `url_q.get()` above is what
                        // empirically stops the panic (confirmed by reverting
                        // *only* this component split, markup inlined right
                        // here with the same no-sync-read outer closure — no
                        // crash, same as with the split). The split earns its
                        // keep anyway as a tripwire against a *future* regression
                        // that reintroduces a synchronous read here: `Pager`
                        // hit its own "already disposed" panic in round 1 for
                        // an adjacent reason (its own doc comment) and settled
                        // on the identical shape — `stale.get()` read inside a
                        // child component's own plain view output, not a raw
                        // `class:x=signal` binding built directly as a
                        // `Suspend` block's tail value.
                        let stale = displaced_by(q, url_q, query_text);
                        Some(view! { <FilteredCount label stale /> })
                    })}
                </Transition>
            </div>
            <QueryBar
                text=query_text
                url_q
                // Reads `list_view` untracked at commit time: the layout is URL
                // state this bar must preserve, not search state it owns. The
                // cursor is the opposite case — a new query has no page two yet
                // (see `catalog_url`), so every edit here starts at the top.
                to_url=Callback::new(move |q: String| {
                    catalog_url(&q, list_view.get_untracked(), None, None)
                })
                id="catalog-query"
                placeholder="Search the catalog — t:instant c:ur cmc<=2"
                aria_label="Search the catalog"
            />
            <ResultsToolbar results list_view />
            <Results results last_good list_view url_q query_text search_count />
        </div>
    }
}

/// The filtered header count's own body — a genuine child `#[component]`
/// rather than markup built inline as the tail value of the `Suspend::new`
/// async block that resolves `label`/`stale` (see the call site's own
/// comment). Not the fix itself for the round-2 crash this task's
/// adversarial review reproduced (WB-01M0324HQ12B590CZ0YXJPB5T6) — the
/// confirmed, necessary fix was removing a synchronous `url_q.get()` read
/// from that block's *outer* closure, which was rebuilding a fresh `Suspend`
/// cycle on every keystroke-settled query change, independent of whether
/// `search_count`'s own fetch had resolved, and disposing the *previous*
/// cycle's `displaced_by` signal while its content was still what
/// `<Transition>` had on screen. Reading that disposed signal from a live
/// `class:x=`/attribute binding panicked (`unreachable`, wasm-fatal — took
/// the whole page down, `results` included, not just this line) — reverting
/// only *this* split (same markup, inlined at the call site, same no-sync-read
/// outer closure) does not reproduce it, so the split is not independently
/// load-bearing here.
///
/// Kept anyway as a deliberate tripwire against a *future* regression that
/// reintroduces a synchronous read at that call site: `Pager` hit its own
/// "already disposed" panic in round 1 (its own doc comment, an adjacent
/// cause — N sibling `Signal::derive`-holding elements, not this task's
/// premature-rebuild mechanism) and settled on the same shape — a live
/// signal read inside a child component's own plain, non-`Suspend` view
/// output, disposed only when that component itself unmounts, never as a
/// side effect of a sibling `Suspend` cycle merely starting.
#[component]
fn FilteredCount(label: String, stale: Signal<bool>) -> impl IntoView {
    view! {
        <p
            class="text-muted-foreground text-sm"
            class:opacity-50=stale
            data-stale=move || stale.get().then_some("true")
            data-testid="catalog-count"
        >
            {label}
        </p>
    }
}

/// The result-count phrase — and the whole honest claim keyset paging supports.
///
/// There is no offset in a keyset cursor and the endpoint deliberately runs no
/// `COUNT` (specs/catalog-search.md), so this screen cannot say "51–73 of 73".
/// What it *can* say is exact, and it differs by page:
///
/// - **page one, complete** — `23 results`: the page is the result set.
/// - **page one, with more** — `50+ results`: "at least 50", the reading
///   specs/app-ui.md settled on when the wireframe's "128 results" turned out
///   not to be obtainable.
/// - **past a cursor** — `50 results on this page`: the row count is a fact
///   about this page only. Saying it unqualified is what made the last page of
///   a 73-row search read "23 results" and every middle page read "50+"
///   (P6-132). No `+` here: the page holds exactly what it holds, and the
///   qualifier already refuses the claim about the total.
///
/// A page ordinal (`Page 2 · 50 results`) was the other candidate and is the
/// bigger change: it needs a new URL parameter threaded through every writer of
/// a catalog URL, kept in sync with a cursor that can also arrive from a shared
/// link with no ordinal beside it. Deferred in favour of the qualifier, which
/// needs no new state to be true.
pub(crate) fn count_label(n: usize, has_more: bool, paged: bool) -> String {
    match (paged, has_more) {
        (true, _) => format!("{n} results on this page"),
        (false, true) => format!("{n}+ results"),
        (false, false) => format!("{n} results"),
    }
}

/// The header's count line for a **filtered** query — `search_count`'s
/// unqualified counterpart to the unfiltered "N cards in the catalog." line
/// above it. Unlike [`count_label`], no `+` qualifier is ever needed here:
/// `search_count` runs a real `count(*)` (specs/catalog-search.md "Numbered
/// page links, round 2"), so the number is exact regardless of how many pages
/// the search runs to — `count_label`'s qualifier exists only because a page
/// of *results* can't see past its own `next_cursor`, a limitation this
/// number doesn't have.
///
/// **`None` for zero, on purpose — not `"0 cards match."`.** `NoResults`
/// already renders "No cards match that search." in the body for exactly
/// this case; the header repeating the same verdict a second time above the
/// grid would be noise stacked on the one true message, not a second fact.
///
/// **No singular/plural agreement** (`"1 cards match."`), matching
/// `count_label`'s own precedent (`"1 results"`) — this app's tone has never
/// special-cased `n == 1` for a count line, so this doesn't start now.
pub(crate) fn filtered_count_label(n: i64) -> Option<String> {
    (n > 0).then(|| format!("{n} cards match."))
}

/// Is the query now in the box an edit of the one `kept` answered — the same
/// search being refined — or a different search entirely?
///
/// This gates the dimmed "Previous results" block. Keeping the last good page
/// under a grammar error is *for* refinement: `bolt` → `bolt pow>3` errors by
/// design and blanking the page there would strobe the results away under the
/// user's fingers. But results from a search the user has abandoned are not
/// "previous results", they are somebody else's, and the badge does not say
/// which query they answer.
///
/// Prefix in either direction is the honest test for "still editing this":
/// appending a term or backspacing one keeps the pair related, replacing the
/// whole string does not. It is a heuristic and it fails safe — a mid-string
/// edit simply drops the kept page, which is the pre-`last_good` behavior for
/// that one keystroke, not a wrong page.
fn same_search(kept: &str, now: &str) -> bool {
    kept.starts_with(now) || now.starts_with(kept)
}

/// Result count on the left of the grid/list switch. The destination picker
/// (`Adding to: 📥 Inbox ▾`, wireframe) joins this row in its own task.
#[component]
fn ResultsToolbar(results: Resource<SearchPayload>, list_view: Memo<bool>) -> impl IntoView {
    // The mobile sheet's "Show N results" footer. The goal is to stay *outside*
    // a suspense boundary — the sheet is open while a search is in flight, and
    // a `None` here reads as "Show results" rather than blocking the button.
    //
    // But it must not read the resource in render to get that. Doing so warned
    // ("reading a resource in hydrate mode outside a Suspense/Transition/
    // effect") and the warning was right: SSR ran the closure before the
    // resource resolved and emitted "Show results", hydration then *claimed*
    // that text node without rewriting it, and the label stayed wrong until the
    // next query change. Verified on `/catalog?q=bolt` — one result, label stuck
    // at "Show results".
    //
    // An Effect-written signal keeps the non-blocking property and fixes the
    // staleness: Effects don't run during SSR (so SSR still renders the `None`
    // branch, deterministically), and the post-hydration write is a real signal
    // change, which does update the DOM.
    //
    // The phrase, not the bare number: the footer makes the same claim the
    // count beside the view switch does, so it has to be qualified the same way
    // past a cursor (P6-132). Both read the *payload's* cursor rather than the
    // URL's — during a search the URL has already moved and these still
    // describe the page on screen.
    let count_after_hydrate = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        let Some(p) = results.get() else { return };
        if let Ok(r) = p.search {
            count_after_hydrate.set(Some(count_label(
                r.cards.len(),
                r.next_cursor.is_some(),
                !p.cursor.is_empty(),
            )));
        }
    });
    let result_label: Signal<Option<String>> = count_after_hydrate.into();

    // The view switch's own navigation: relayouting the page you are on is not
    // a query edit, so the cursor (and its page label) ride along — bouncing a
    // reader back to page one for choosing list view would lose their place.
    // Built here, not inside `ViewSwitch` itself (P6-… grid-toggle task): the
    // widget is now shared with `/my` and every collection view
    // (`crate::components::view_switch`), each of which builds its own URL
    // its own way, so the router wiring lives at each call site instead.
    let navigate = use_navigate();
    let query_map = use_query_map();
    let go = move |list: bool| {
        let params = query_map.read_untracked();
        let q = params.get("q").unwrap_or_default();
        let cursor = params.get(CURSOR_PARAM).unwrap_or_default();
        let page = params
            .get(PAGE_PARAM)
            .and_then(|p| p.parse::<usize>().ok())
            .filter(|p| *p > 0);
        drop(params);
        navigate(
            &catalog_url(
                &q,
                list,
                (!cursor.is_empty()).then_some(cursor.as_str()),
                page,
            ),
            NavigateOptions::default(),
        );
    };

    view! {
        <div class="flex flex-wrap items-center gap-3">
            <rail::FilterSheet result_label />
            <p class="text-muted-foreground text-sm" data-testid="result-count">
                <Transition fallback=|| {
                    view! { <span>"Searching…"</span> }
                }>
                    {move || Suspend::new(async move {
                        let p = results.await;
                        match p.search {
                            Ok(r) => {
                                count_label(
                                    r.cards.len(),
                                    r.next_cursor.is_some(),
                                    !p.cursor.is_empty(),
                                )
                            }
                            Err(_) => String::new(),
                        }
                    })}
                </Transition>
            </p>
            <div class="ml-auto flex items-center gap-2">
                <destination::DestinationPicker />
                <ViewSwitch list_view on_change=Callback::new(go) />
            </div>
        </div>
    }
}

/// The result set: grid of image-led tiles, or the table in list view, plus the
/// pager underneath it.
#[component]
fn Results(
    results: Resource<SearchPayload>,
    last_good: RwSignal<Option<(String, Vec<CardSummary>)>>,
    list_view: Memo<bool>,
    url_q: Memo<String>,
    query_text: RwSignal<String>,
    search_count: Resource<CountPayload>,
) -> impl IntoView {
    view! {
        // Transition, not Suspense: re-searching keeps the previous results on
        // screen instead of collapsing the page to skeletons on every keystroke.
        <Transition fallback=|| {
            view! { <ResultsSkeleton /> }
        }>
            {move || {
                Suspend::new(async move {
                // Everything about *which page this is* comes out of the
                // payload, never out of the URL. Under a Transition this block
                // stays on screen while a newer search runs, so the URL has
                // routinely moved past what is rendered here (P6-133a: reading
                // `?cursor=` grew a "Back to the start" on a page that was
                // still page one).
                //
                // `paged`: not `page`'s own request `p.page` alone — a legacy
                // `?cursor=`-only link (no `page`) still has to read as "not
                // page one" for `NoResults`/the error banner's recovery link,
                // even though `page` there defaults to 1 (see `catalog_url`'s
                // doc comment on that caveat).
                let SearchPayload { q, cursor, page, search } = results.await;
                let paged = !cursor.is_empty() || page > 1;
                let stale = displaced_by(q.clone(), url_q, query_text);
                match search {
                    Ok(r) if r.cards.is_empty() => {
                        view! { <NoResults q paged list_view stale /> }.into_any()
                    }
                    Ok(r) => {
                        let shared::SearchResults { cards, next_cursor } = r;
                        let has_more = next_cursor.is_some();
                        // `page_size` read off this page rather than hardcoded
                        // 50, so a future page-size change (queued) needs no
                        // edit here — only meaningful (and only used) when
                        // `has_more`, since the server always returns exactly
                        // `limit` rows in that case.
                        let page_size = cards.len();
                        view! {
                            <ResultCards cards list_view stale=false />
                            <Pager page has_more page_size q list_view stale search_count />
                        }
                            .into_any()
                    }
                    Err(e) => {
                        let (kind, message) = describe_error(&e);
                        // The kept page has to answer the query this error is
                        // about; see `same_search`.
                        let kept = last_good
                            .get_untracked()
                            .filter(|(kept_q, _)| same_search(kept_q, &q))
                            .map(|(_, cards)| cards);
                        // Kept for the escape hatch below, which keeps the
                        // query and drops only the cursor: a bad cursor must
                        // not cost the user the search they typed.
                        let q = StoredValue::new(q);
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
                                {match kind {
                                    // The grammar's own words about the term it
                                    // rejected — no prefix, so it reads as an
                                    // answer about the box, not the app.
                                    QueryErrorKind::Grammar => message,
                                    // P6-043: this used to fall into the branch
                                    // above and print the raw "invalid cursor"
                                    // as if `q` were the problem — the one
                                    // thing it demonstrably is not, since the
                                    // query underneath is what the "Back to the
                                    // start" link below re-runs successfully.
                                    // The page reference is what is wrong, so
                                    // the banner says that instead of echoing a
                                    // decode-failure message the reader has no
                                    // way to act on.
                                    QueryErrorKind::Cursor => {
                                        "This page link is no longer valid.".to_string()
                                    }
                                    QueryErrorKind::Other => format!("Search failed: {message}"),
                                }}
                            </p>
                            // Paging is what makes an error reachable with no
                            // way out: a *shared* `?cursor=` link can be stale
                            // or corrupt, and unlike a mid-typing error there is
                            // no last-good page under it and nothing wrong with
                            // the box to fix. `/my` leaves that a dead end;
                            // here the pager's own affordance covers it —
                            // keeping `q` and dropping only the cursor, exactly
                            // what a `QueryErrorKind::Cursor` banner promises.
                            {paged
                                .then(|| {
                                    view! {
                                        <p class="text-muted-foreground pt-3 text-sm">
                                            <PageLink
                                                href=Signal::derive(move || {
                                                    catalog_url(&q.get_value(), list_view.get(), None, None)
                                                })
                                                class="underline"
                                                testid="page-first"
                                                stale
                                                label="← Back to the start"
                                            />
                                        </p>
                                    }
                                })}
                            {kept
                                .filter(|c| !c.is_empty())
                                .map(|cards| {
                                    view! {
                                        // The cards below are dimmed and inert,
                                        // which says "not clickable" but not
                                        // *why*. Unlabeled, a reader sees results
                                        // sitting under an error and has no way to
                                        // tell they answer the previous query —
                                        // and the `aria-hidden` on the block means
                                        // a screen reader is told nothing at all,
                                        // so the label has to live out here. The
                                        // `info` tone is the honest one: nothing
                                        // failed and nothing is resolved, the
                                        // content is simply not current.
                                        <p class="pt-3">
                                            <StateBadge
                                                tone=Tone::Stale
                                                label="Previous results"
                                            />
                                        </p>
                                        <ResultCards cards list_view stale=true />
                                    }
                                })}
                        }
                            .into_any()
                    }
                }
                })
            }}
        </Transition>
    }
}

/// Nothing to show. On page one that means the search matched nothing; past a
/// cursor it may only mean the reader walked off the end, which needs a way
/// home rather than a verdict on the query (`/my`'s `EmptyState`, same reason).
#[component]
fn NoResults(q: String, paged: bool, list_view: Memo<bool>, stale: Signal<bool>) -> impl IntoView {
    let q = StoredValue::new(q);
    // `paged` is a fact about the payload that produced this empty page, not a
    // signal: it cannot change without a new render (P6-133a).
    let body = if paged {
        view! {
            <p>
                "Nothing on this page. "
                <PageLink
                    href=Signal::derive(move || {
                        catalog_url(&q.get_value(), list_view.get(), None, None)
                    })
                    class="underline"
                    testid="page-first"
                    stale
                    label="Back to the start"
                /> "."
            </p>
        }
        .into_any()
    } else {
        view! { <p>"No cards match that search."</p> }.into_any()
    };
    view! {
        <div class="text-muted-foreground py-12 text-center text-sm" data-testid="no-results">
            {body}
        </div>
    }
}

/// One element of the numbered page strip [`page_strip`] computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageSlot {
    /// A page number — may or may not be a real link; `Pager` decides that
    /// from data this function never sees (see its doc comment).
    Number(usize),
    /// The current page, always rendered as plain text, never a link.
    Current(usize),
    /// A gap of un-rendered page numbers between two shown ones.
    Ellipsis,
}

/// Compute the numbered page strip's shape (WB-01M032Q6BX8BM7NPK8H3AQKGWF): up
/// to 6 numbers, always page 1 and `last` (when `last` is known), a band of 4
/// counting up from `current` -- or down, once counting up would run past
/// `last`, so 6 numbers still fit. `...` fills any gap wider than one between
/// shown numbers. `current` always renders as [`PageSlot::Current`], never a
/// link. The three task examples are this function's unit tests verbatim.
///
/// **`last = None` means the total page count is not known.** There is no
/// `COUNT` behind a catalog search -- deliberately, so the box can search on
/// every keystroke (specs/catalog-search.md "What a keyset page may claim") --
/// so a filtered search only ever learns its true last page by *reaching* it
/// (`next_cursor` comes back empty). Until then, fabricating a "last" this
/// screen cannot back up would be exactly the false claim that file's
/// `count_label`/P6-130..133 batch exists to refuse. This mode degrades to the
/// only pages nameable without one: 1, the current page, and current + 1 (the
/// same cursor "Next" already has in hand).
pub(crate) fn page_strip(current: usize, last: Option<usize>) -> Vec<PageSlot> {
    let current = current.max(1);
    let Some(last) = last else {
        let mut out = Vec::new();
        if current > 1 {
            out.push(PageSlot::Number(1));
            if current > 2 {
                out.push(PageSlot::Ellipsis);
            }
        }
        out.push(PageSlot::Current(current));
        // `saturating_add`: `current` is clamped well below `usize::MAX` by
        // every real caller (`MAX_PAGE`), but this function is unit-tested
        // directly against the crafted `usize::MAX` case (adversarial-review
        // blocker, WB-01M032Q6BX8BM7NPK8H3AQKGWF round 2) and must not panic
        // or wrap on its own terms, independent of what a caller clamped.
        out.push(PageSlot::Number(current.saturating_add(1)));
        return out;
    };
    let last = last.max(current);
    let slot = |p: usize| {
        if p == current {
            PageSlot::Current(p)
        } else {
            PageSlot::Number(p)
        }
    };
    if last <= 6 {
        return (1..=last).map(slot).collect();
    }

    // A band of 4 around `current`, counting up unless that would run past
    // the `last` boundary reserved below -- then counting down instead, ending
    // *at* `current` (examples 1 vs. 3). `saturating_add` throughout: same
    // no-panic-on-its-own-terms contract as the `last = None` branch above.
    let counting_up = current.saturating_add(3) < last;
    let (mut lo, mut hi) = if counting_up {
        (current, current.saturating_add(3))
    } else {
        (current.saturating_sub(3).max(1), current)
    };
    hi = hi.min(last);

    let mut nums = std::collections::BTreeSet::new();
    nums.insert(1);
    nums.insert(last);
    nums.extend(lo..=hi);

    // 1 and/or `last` may already be inside the band (examples 2 and its
    // mirror, `current == last`) -- the slot that frees grows the band by one
    // more in the same direction, so 6 numbers still show rather than 5.
    let target = 6.min(last);
    while nums.len() < target {
        let grew = if counting_up {
            if hi < last {
                hi += 1;
                nums.insert(hi);
                true
            } else if lo > 1 {
                lo -= 1;
                nums.insert(lo);
                true
            } else {
                false
            }
        } else if lo > 1 {
            lo -= 1;
            nums.insert(lo);
            true
        } else if hi < last {
            hi += 1;
            nums.insert(hi);
            true
        } else {
            false
        };
        if !grew {
            break;
        }
    }

    let mut out = Vec::with_capacity(nums.len() + 2);
    let mut prev = None;
    for p in nums {
        if let Some(pv) = prev {
            if p > pv + 1 {
                out.push(PageSlot::Ellipsis);
            }
        }
        out.push(slot(p));
        prev = Some(p);
    }
    out
}

/// Numbered paging controls (specs/catalog-search.md `/catalog?q=...&page=...`).
///
/// **Left-justified, Prev/Next always rendered, wrapping up to 6 page-number
/// links** (WB-01M032Q6BX8BM7NPK8H3AQKGWF), replacing the old right-aligned
/// "Next page (arrow)" / "(arrow) Back to the start" pair.
///
/// **Every rendered number is a real link, from round 2 on** (maintainer
/// ruling, 2026-08-15, superseding round 1's "some numbers render inert"
/// compromise): an explicit page-N jump no longer needs a cursor this browser
/// happens to have already fetched — `href_for` hands every number straight to
/// `catalog_url`, which the server turns into an `OFFSET` under the same sort
/// the keyset cursor uses (`CatalogStore::search`'s doc comment). This retired
/// the client-side `trail` `CatalogPage` used to keep (one entry per page a
/// reader had actually stood on) — with every page directly addressable,
/// remembering *which* pages had been visited no longer earns its keep.
///
/// **`last` (the true last page) is two-tier honest**, same principle as
/// before, cheaper to reach now: `has_more == false` means this *is* the last
/// page, exactly, no query needed. Otherwise `search_count` (a second,
/// independent request `CatalogPage` fires alongside `results`, not blocking
/// it) supplies the row count once it resolves — read here with a plain,
/// non-blocking `.get()`, so the strip renders immediately with `results` and
/// upgrades in place the moment the count lands, never the reverse. Until
/// then (or for the true edge case a `search_count` fetch itself errors),
/// `last = None` and `page_strip` degrades to naming only 1, current, and
/// current + 1 — the only pages nameable without a total, same as always.
///
/// **`paged`/`last` are the rendered page's own facts, not the URL's**
/// (P6-133a), and there is **no `<nav>` at all when there is nothing to page**
/// (P6-133b): a named landmark wrapping an empty `<span>` promises navigation
/// it does not have, and a single-page result set is the common case here --
/// unchanged from before this task, so a lone page still renders no pager
/// rather than a disabled Prev/[1]/Next strip.
#[component]
fn Pager(
    page: usize,
    has_more: bool,
    page_size: usize,
    q: String,
    list_view: Memo<bool>,
    stale: Signal<bool>,
    search_count: Resource<CountPayload>,
) -> impl IntoView {
    // Nothing before this page and nothing after it: the whole landmark is
    // noise, so it is not rendered.
    if !pager_is_needed(page > 1, has_more) {
        return ().into_any();
    }
    let q = StoredValue::new(q);

    view! {
        <nav aria-label="Pagination" class="flex flex-wrap items-center gap-1">
            {move || {
                // The whole strip is *one* dynamic child, rebuilt whenever
                // `list_view`, `stale`, or `search_count` changes -- the same
                // "reactive dependencies read inside a `move ||` block that
                // rebuilds the subtree" shape `ResultCards` already uses for
                // `list_view`, not N sibling components each holding their own
                // `Signal::derive` prop (round 1 built it that way for the
                // numbered links and hit a live "already disposed" panic the
                // instant a sibling signal changed while `<Transition>` still
                // held the previous `Pager` mounted — see git history on this
                // function for the fuller account).
                let list = list_view.get();
                let is_stale = stale.get();
                // Two-tier honesty -- see the doc comment above.
                let last = if !has_more {
                    Some(page)
                } else {
                    search_count
                        .get()
                        .and_then(|p| p.count)
                        .filter(|_| page_size > 0)
                        .map(|total| (total.max(0) as usize).div_ceil(page_size))
                };
                let href_for = |n: usize| -> String {
                    catalog_url(&q.get_value(), list, None, Some(n))
                };
                let link = move |href: String, testid: String, label: String, boundary: bool| {
                    let disabled = is_stale || boundary;
                    let class = if disabled {
                        "border-input hover:bg-accent hover:text-accent-foreground rounded-md border px-3 py-1.5 text-sm pointer-events-none opacity-50"
                            .to_string()
                    } else {
                        "border-input hover:bg-accent hover:text-accent-foreground rounded-md border px-3 py-1.5 text-sm"
                            .to_string()
                    };
                    view! {
                        <a
                            href=href
                            class=class
                            aria-disabled=disabled.then_some("true")
                            data-testid=testid
                            on:click=move |ev: leptos::ev::MouseEvent| {
                                if disabled {
                                    ev.prevent_default();
                                }
                            }
                        >
                            {label}
                        </a>
                    }
                        .into_any()
                };

                let slots = page_strip(page, last);
                let mut children: Vec<leptos::prelude::AnyView> = Vec::with_capacity(slots.len() + 2);
                children.push(link(
                    href_for(page.saturating_sub(1)),
                    "pager-prev".to_string(),
                    "\u{2190} Prev".to_string(),
                    page <= 1,
                ));
                for slot in slots {
                    children.push(match slot {
                        PageSlot::Ellipsis => {
                            view! {
                                <span
                                    class="text-muted-foreground px-2 text-sm"
                                    aria-hidden="true"
                                >
                                    "\u{2026}"
                                </span>
                            }
                                .into_any()
                        }
                        PageSlot::Current(n) => {
                            view! {
                                <span
                                    class="px-3 py-1.5 text-sm font-medium"
                                    aria-current="page"
                                    data-testid="pager-current"
                                >
                                    {n.to_string()}
                                </span>
                            }
                                .into_any()
                        }
                        PageSlot::Number(n) => {
                            link(href_for(n), format!("pager-page-{n}"), n.to_string(), false)
                        }
                    });
                }
                children.push(link(
                    href_for(page.saturating_add(1)),
                    "pager-next".to_string(),
                    "Next \u{2192}".to_string(),
                    !has_more,
                ));
                children.into_iter().collect_view()
            }}
        </nav>
    }
    .into_any()
}

/// Does this page have anywhere to go? A single-page result set has neither a
/// page before it nor one after, and `<nav aria-label="Pagination">` around an
/// empty `<span>` is a named landmark with no content in it -- announced to a
/// screen reader as navigation that then contains nothing (P6-133b).
fn pager_is_needed(paged: bool, has_next: bool) -> bool {
    paged || has_next
}

/// Is the page these results describe no longer the page the reader is asking
/// for — because a newer search is in flight, or because the box has been typed
/// into and the debounce has not fired yet?
///
/// `<Transition>` deliberately keeps the old result set on screen through a
/// search (no strobing), and the pager that came with it points at
/// `(old_q, old_cursor)`. Clicking it navigates *around* `QueryBar::commit`
/// straight to the old query, whose re-seed effect then sees the URL move
/// without it and rewrites the box — silently reverting what the user just
/// typed (P6-130). A cursor is only ever valid for its own query, so the honest
/// state for that control is "not actionable yet", not "actionable, wrongly".
///
/// Both halves are needed: `url_q` catches the in-flight window after the
/// debounce fires, `query_text` catches the ~250 ms before it.
fn displaced_by(
    rendered_q: String,
    url_q: Memo<String>,
    query_text: RwSignal<String>,
) -> Signal<bool> {
    Signal::derive(move || {
        url_q.with(|q| q != &rendered_q) || query_text.with(|t| t != &rendered_q)
    })
}

/// One pager link, inert while the page it belongs to is stale.
///
/// **Inert, not gone.** The results stay on screen during a search, so removing
/// the control — or its `href`, which is the same thing to the tab order —
/// would make the pager flicker under the reader and drop keyboard focus
/// mid-navigation. `aria-disabled` plus a click that does not navigate keeps
/// the tab stop, tells assistive tech the truth, and is the only combination
/// that also survives `Enter` (a keyboard activation dispatches a real click,
/// and `leptos_router`'s *window* bubble listener bails on `defaultPrevented`).
///
/// **Load-bearing build assumption:** this works because `tachys/delegation`
/// is OFF in this build, so `on:click` attaches directly to the anchor and
/// fires (target phase) before the router's window listener. Enabling
/// `leptos/delegation` would move our handler after the router's and silently
/// re-arm stale-pager navigation — if that feature is ever turned on, switch
/// this to `on:click:undelegated`.
#[component]
fn PageLink(
    #[prop(into)] href: Signal<String>,
    #[prop(into)] class: String,
    testid: &'static str,
    stale: Signal<bool>,
    label: &'static str,
) -> impl IntoView {
    view! {
        <a
            href=move || href.get()
            class=move || {
                if stale.get() {
                    format!("{class} pointer-events-none opacity-50")
                } else {
                    class.clone()
                }
            }
            aria-disabled=move || stale.get().then_some("true")
            data-testid=testid
            on:click=move |ev: leptos::ev::MouseEvent| {
                if stale.get() {
                    ev.prevent_default();
                }
            }
        >
            {label}
        </a>
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

// `max-w-7xl` used to cap the grid the same way `Table`'s own `max-w-7xl`
// (`components/ui/table.rs`) caps the list view: neither centered (no
// `mx-auto`), both just stopped stretching past 1280px. Before P6-098 the grid
// had no cap at all and `xl:grid-cols-6` was the last breakpoint, so a card
// tile kept growing with the window past it — comically large at ultrawide;
// `max-w-7xl` fixed that by freezing the container at 1280px for any viewport
// xl and up.
//
// **The cap is gone (WB-01M033AFA0VSCGB8Z3HTYPFZVD, maintainer report from a
// 2560px monitor):** 1280px is exactly half of 2560px, so the frozen container
// read as "the grid only fills about half the available width" there — the
// literal bug. Removing the cap without another fix would just resurrect
// P6-098 (six columns stretching wider and wider past 1280px), so the fix
// pairs the removal with one more breakpoint: `3xl:grid-cols-10` (custom
// screen, `style/input.css`, 2200px) takes over from six columns before a
// viewport gets wide enough for them to look comically large, the same
// problem P6-098 solved, solved the same way — a denser column count instead
// of a width ceiling. Deliberately no 5- or 7-9-column tier (maintainer
// ruling): 2, 3, 4, 6 and 10 are the divisors 60-per-page (`CATALOG_PAGE_SIZE`)
// wants, so every tile row is either full or the grid has moved to a fresh
// row cleanly, never a lonely 1-2 tile remainder.
//
// `pub(crate)`: the My-cards grid views (`crate::my::all_cards`,
// `crate::my::collection`) reuse this literally rather than re-deriving their
// own breakpoints, so the two-column-at-390px / now-uncapped shape stays one
// decision instead of three that can drift — including this fix, which
// therefore also widens those two grids on a wide viewport. The maintainer's
// report named only `/catalog`, but the width-waste symptom and its root
// cause (this shared constant) are identical there; splitting the constant to
// hold `/my` back at the old capped behaviour would be the one that drifts.
// (`CATALOG_PAGE_SIZE`, by contrast, stays catalog-only — see its own doc
// comment for why a fetch-size change doesn't get the same "let it flow"
// treatment as a layout class.)
pub(crate) const GRID_CLASS: &str =
    "grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6 3xl:grid-cols-10";

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
        printing_id,
        image_uri,
        mana_cost,
        type_line,
        owned,
        ..
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
                // Authed-only: `owned` is `None` for an anonymous caller
                // (unknown, not zero), so the badge never renders there.
                {owned
                    .filter(|n| *n > 0)
                    .map(|n| {
                        view! {
                            // The testid rides the wrapper, not the `Badge`: a
                            // component prop ending in a bare path (`size=…::Sm`)
                            // immediately before `{..}` is parsed as
                            // struct-update syntax (see cards.rs).
                            <span
                                class="absolute right-1.5 top-1.5"
                                data-testid="owned-badge"
                            >
                                <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                                    {format!("{n} owned")}
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
            <QuickActions name oracle_id printing_id />
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
                            let CardSummary {
                                oracle_id,
                                name,
                                printing_id,
                                mana_cost,
                                type_line,
                                owned,
                                ..
                            } = card;
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
                                        // Authed-only, and the testid rides the
                                        // wrapper — same two notes as the tile.
                                        {owned
                                            .filter(|n| *n > 0)
                                            .map(|n| {
                                                view! {
                                                    <span class="ml-2" data-testid="owned-badge">
                                                        <Badge
                                                            variant=BadgeVariant::Secondary
                                                            size=BadgeSize::Sm
                                                        >
                                                            {format!("{n} owned")}
                                                        </Badge>
                                                    </span>
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
                                        <QuickActions name oracle_id printing_id />
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

/// `+ Want` / `+ Have` on every result (wireframe). Anonymous visitors get a
/// sign-in prompt; a signed-in caller adds to whatever the sticky
/// [`destination::DestinationPicker`] currently points at, and gets a
/// confirmation toast — with Undo for a Have.
#[component]
fn QuickActions(
    name: String,
    oracle_id: shared::Id,
    printing_id: Option<shared::Id>,
) -> impl IntoView {
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
                        [AddKind::Want, AddKind::Have]
                            .into_iter()
                            .map(|kind| {
                                if authed {
                                    view! {
                                        <QuickAddButton
                                            name=name.clone()
                                            oracle_id
                                            printing_id
                                            kind
                                        />
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
                                            aria-label=format!(
                                                "Sign in to add {name} to {}",
                                                kind.noun(),
                                            )
                                            class="border-input hover:bg-accent hover:text-accent-foreground inline-flex h-7 items-center rounded-md border px-2 text-xs"
                                        >
                                            {format!("+ {}", kind.noun())}
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

/// Which of the two quick actions a button is. The pair differ in more than a
/// label — grain (printing vs oracle), undoability, and toast wording — so they
/// share one component parameterized by this rather than two near-copies.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AddKind {
    Want,
    Have,
}

impl AddKind {
    fn noun(self) -> &'static str {
        match self {
            AddKind::Want => "Want",
            AddKind::Have => "Have",
        }
    }

    fn wire(self) -> shared::QuickAddKind {
        match self {
            AddKind::Want => shared::QuickAddKind::Want,
            AddKind::Have => shared::QuickAddKind::Have,
        }
    }
}

/// One quick-add button: fires the add, then raises the toast.
#[component]
fn QuickAddButton(
    name: String,
    oracle_id: shared::Id,
    printing_id: Option<shared::Id>,
    kind: AddKind,
) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    let destination = destination::current_destination();
    // Captured at setup, not looked up in the async block (no owner there).
    // The sidebar tree's badges count what this button changes.
    let tree = use_context::<crate::my::tree::CollectionTreeResource>();
    let last_move = use_context::<crate::components::palette::LastMoveState>();
    let pending = RwSignal::new(false);

    // A Have is stored per *printing*, so a card whose oracle row resolved no
    // representative printing can be Wanted but not Had. Disabling beats
    // firing an add that the server can only reject.
    let addable = matches!(kind, AddKind::Want) || printing_id.is_some();
    let disabled = Signal::derive(move || pending.get() || destination.get().is_none() || !addable);

    let on_click = {
        let name = name.clone();
        move |_| {
            // Re-read at click time, not render time: the picker may have moved
            // since this row rendered, and the wireframe's sticky picker means
            // the *current* choice is the one that counts.
            let Some(dest) = destination.get_untracked() else {
                return;
            };
            if pending.get_untracked() {
                return;
            }
            pending.set(true);
            let name = name.clone();
            spawn_local(async move {
                let result =
                    crate::quick_add(dest.id, kind.wire(), oracle_id, printing_id, 1).await;
                pending.set(false);
                match result {
                    Ok(receipt) => {
                        if let Some(t) = tree {
                            t.0.refetch();
                        }
                        raise_add_toast(AddToast {
                            toast,
                            tree,
                            name,
                            dest,
                            kind: kind.wire(),
                            quantity: 1,
                            undo_move_id: receipt.undo_move_id,
                            after_undo: None,
                            last_move,
                        })
                    }
                    Err(e) => {
                        toast.show(
                            ToastOptions::message(format!(
                                "Couldn't add {name}: {}",
                                describe_error(&e).1
                            ))
                            .kind(ToastKind::Error),
                        );
                    }
                }
            });
        }
    };

    let aria = format!("Add {name} to {}", kind.noun());
    view! {
        <Button
            variant=ButtonVariant::Outline
            size=ButtonSize::Sm
            class="h-7 px-2 text-xs"
            {..}
            disabled=disabled
            aria-label=aria
            data-testid=match kind {
                AddKind::Want => "quick-add-want",
                AddKind::Have => "quick-add-have",
            }
            on:click=on_click
        >
            {format!("+ {}", kind.noun())}
        </Button>
    }
}

/// Everything the confirmation toast reports. A struct rather than eight
/// positional arguments because two surfaces raise it now — `/catalog`'s quick
/// actions and the quick-add panel — and the toast is the one place either one
/// tells the user *what* went *where*.
pub(crate) struct AddToast {
    pub toast: ToastHandle,
    /// The shared sidebar resource, refetched after an undo lands (the caller
    /// already refetched for the add itself).
    pub tree: Option<crate::my::tree::CollectionTreeResource>,
    pub name: String,
    pub dest: destination::Destination,
    pub kind: shared::QuickAddKind,
    pub quantity: u32,
    pub undo_move_id: Option<shared::Id>,
    /// Run after a successful undo, for a surface whose own read has to move
    /// with it (the collection view). The add-side refetch is the caller's.
    pub after_undo: Option<Callback<()>>,
    /// Where ⌘K's `Undo last move` remembers this add, so the palette command
    /// and this toast's button reverse the same thing. Passed in rather than
    /// read from context here: this is a free function, and its callers reach it
    /// from inside a `spawn_local` where the reactive owner is long gone.
    pub last_move: Option<crate::components::palette::LastMoveState>,
}

/// The confirmation toast, and the Undo action when there is one to offer.
pub(crate) fn raise_add_toast(t: AddToast) {
    let AddToast {
        toast,
        tree,
        name,
        dest,
        kind,
        quantity,
        undo_move_id,
        after_undo,
        last_move,
    } = t;
    let verb = match kind {
        shared::QuickAddKind::Want => "Wanted",
        shared::QuickAddKind::Have => "Added",
    };
    // The count is always stated (the storyboard's "Added 1 Lightning Strike to
    // Trade Binder"): with ⇧⏎ able to add a playset, "Added Lightning Bolt"
    // would leave the user unsure whether the digits landed.
    let message = format!("{verb} {quantity} {name} → {}", dest.label());
    let options = ToastOptions::message(message).kind(ToastKind::Success);

    // Undo exists only for a Have — a Want writes no move row, so there is
    // nothing to reverse (specs/app-ui.md Findings). Offering a dead button
    // would be worse than offering none.
    let options = match undo_move_id {
        Some(move_id) => {
            crate::components::palette::note_last_move(last_move, vec![move_id]);
            let name = name.clone();
            options.action(
                "Undo",
                Callback::new(move |()| {
                    let name = name.clone();
                    // ⌘K must stop offering this one: undo is idempotent, so a
                    // second reversal would succeed over a no-op and claim it
                    // undid something (see `LastMoveState::forget`).
                    crate::components::palette::forget_last_move(last_move, &[move_id]);
                    spawn_local(async move {
                        match crate::undo_quick_add(move_id).await {
                            Ok(()) => {
                                if let Some(t) = tree {
                                    t.0.refetch();
                                }
                                if let Some(after) = after_undo {
                                    after.run(());
                                }
                                toast.show(ToastOptions::message(format!("Removed {name} again")))
                            }
                            Err(e) => toast.show(
                                ToastOptions::message(format!(
                                    "Couldn't undo: {}",
                                    describe_error(&e).1
                                ))
                                .kind(ToastKind::Error),
                            ),
                        };
                    });
                }),
            )
        }
        None => options,
    };
    toast.show(options);
}

#[cfg(test)]
mod tests {
    use super::{
        catalog_url, count_label, encode_query_value, filtered_count_label, page_strip,
        pager_is_needed, parse_page, same_search, PageSlot, MAX_PAGE,
    };

    #[test]
    fn a_single_page_result_set_renders_no_pagination_landmark() {
        assert!(!pager_is_needed(false, false));
        // Page one of many, a middle page, and the last page all have a control
        // worth wrapping.
        assert!(pager_is_needed(false, true));
        assert!(pager_is_needed(true, true));
        assert!(pager_is_needed(true, false));
    }

    #[test]
    fn the_count_qualifies_itself_past_page_one() {
        // Page one is allowed to speak for the whole result set, because it is
        // the whole result set when nothing follows it.
        assert_eq!(count_label(23, false, false), "23 results");
        assert_eq!(count_label(50, true, false), "50+ results");
        // Past a cursor there is no offset to add, so the only true statement
        // left is about this page. The old form said "23 results" on the last
        // page of a 73-row search (P6-132).
        assert_eq!(count_label(23, false, true), "23 results on this page");
        // ...and no `+` there: 50 is exactly what the page holds; the "more"
        // claim is already refused by the qualifier.
        assert_eq!(count_label(50, true, true), "50 results on this page");
    }

    #[test]
    fn the_filtered_header_line_is_exact_no_plus_and_silent_at_zero() {
        // Unlike `count_label`, this is a real `count(*)` — no "+" qualifier,
        // ever, regardless of how many pages the search runs to.
        assert_eq!(filtered_count_label(1), Some("1 cards match.".to_string()));
        assert_eq!(
            filtered_count_label(38_623),
            Some("38623 cards match.".to_string())
        );
        // Zero is `None`, not `"0 cards match."` — `NoResults` already owns
        // that verdict in the body, and the header repeating it would be
        // stacked noise, not a second fact.
        assert_eq!(filtered_count_label(0), None);
        // Defensive: a `count(*)` cannot go negative, but the check is `n >
        // 0`, not `n != 0`, so a negative value degrades the same way zero
        // does rather than rendering a nonsense sentence.
        assert_eq!(filtered_count_label(-1), None);
        // No arithmetic here (unlike `page_strip`/`page_offset`'s `OFFSET`
        // math), so there is nothing to overflow — but the boundary and an
        // absurdly large value both still just format, no panic or wrap.
        assert_eq!(
            filtered_count_label(i64::MAX),
            Some(format!("{} cards match.", i64::MAX))
        );
        assert_eq!(
            filtered_count_label(999_999_999_999),
            Some("999999999999 cards match.".to_string())
        );
    }

    #[test]
    fn a_refinement_is_the_same_search_and_a_replacement_is_not() {
        // Appending a term (the case `last_good` exists for: `bolt pow>3`
        // errors by design) and backspacing one both stay related.
        assert!(same_search("bolt", "bolt pow>3"));
        assert!(same_search("bolt pow>3", "bolt"));
        assert!(same_search("bolt", "bolt"));
        // Browse-all is a prefix of everything, which is right: it is what was
        // on screen before the first character was typed.
        assert!(same_search("", "pow>3"));
        // A different search entirely. Its page is not "previous results",
        // it answers a question nobody asked (P6-131).
        assert!(!same_search("bolt", "counter pow>3"));
        assert!(!same_search("t:instant", "t:creature"));
    }

    #[test]
    fn url_omits_empty_parts() {
        assert_eq!(catalog_url("", false, None, None), "/catalog");
        assert_eq!(catalog_url("", false, Some(""), None), "/catalog");
        assert_eq!(catalog_url("bolt", false, None, None), "/catalog?q=bolt");
        assert_eq!(catalog_url("", true, None, None), "/catalog?view=list");
        assert_eq!(
            catalog_url("", false, Some("abc"), None),
            "/catalog?cursor=abc"
        );
        assert_eq!(
            catalog_url("bolt", true, Some("abc"), None),
            "/catalog?q=bolt&view=list&cursor=abc"
        );
        // Page 1 is implicit, same as an empty cursor: never written, even if
        // explicitly asked for.
        assert_eq!(
            catalog_url("", false, Some("abc"), Some(1)),
            "/catalog?cursor=abc"
        );
        assert_eq!(
            catalog_url("bolt", true, Some("abc"), Some(9)),
            "/catalog?q=bolt&view=list&cursor=abc&page=9"
        );
        // `page` with no `cursor` is not a state this screen ever generates,
        // but it isn't rejected either — the label is purely cosmetic (see
        // `PAGE_PARAM`), so a hand-edited URL just gets a (harmless) label.
        assert_eq!(catalog_url("", false, None, Some(3)), "/catalog?page=3");
    }

    #[test]
    fn url_percent_encodes_the_query_and_the_cursor() {
        // The grammar is punctuation-heavy and a keyset cursor is an opaque
        // string that may carry `&`, `+` or a space (it encodes a card *name*).
        // Either read as URL structure is a different page than the one linked.
        assert_eq!(
            catalog_url("t:instant c:ur", false, None, None),
            "/catalog?q=t%3Ainstant%20c%3Aur"
        );
        assert_eq!(
            catalog_url("", false, Some("Fire // Ice|1"), None),
            "/catalog?cursor=Fire%20%2F%2F%20Ice%7C1"
        );
        assert_eq!(encode_query_value("a&b+c"), "a%26b%2Bc");
    }

    // `page_strip` — WB-01M032Q6BX8BM7NPK8H3AQKGWF's three worked examples,
    // verbatim, plus the edges the task called out by name.

    #[test]
    fn the_28_page_example_counts_up_from_a_middle_current() {
        // [Prev] [1] ... 9 [10] [11] [12] ... [28] [Next]
        use PageSlot::*;
        assert_eq!(
            page_strip(9, Some(28)),
            vec![
                Number(1),
                Ellipsis,
                Current(9),
                Number(10),
                Number(11),
                Number(12),
                Ellipsis,
                Number(28),
            ]
        );
    }

    #[test]
    fn the_28_page_example_on_page_one_has_no_leading_ellipsis() {
        // Prev 1 [2] [3] [4] [5] ... [28] [Next] — current *is* the "1"
        // boundary, so the freed slot grows the band to 5 wide instead of 4.
        use PageSlot::*;
        assert_eq!(
            page_strip(1, Some(28)),
            vec![
                Current(1),
                Number(2),
                Number(3),
                Number(4),
                Number(5),
                Ellipsis,
                Number(28),
            ]
        );
    }

    #[test]
    fn the_10_page_example_counts_down_near_the_end() {
        // [Prev] [1] ... [6] [7] [8] 9 [10] [Next] — the band's top touches
        // `last` with no ellipsis between them, Next still shows regardless.
        use PageSlot::*;
        assert_eq!(
            page_strip(9, Some(10)),
            vec![
                Number(1),
                Ellipsis,
                Number(6),
                Number(7),
                Number(8),
                Current(9),
                Number(10),
            ]
        );
    }

    #[test]
    fn one_page_is_just_the_current_page() {
        use PageSlot::*;
        assert_eq!(page_strip(1, Some(1)), vec![Current(1)]);
    }

    #[test]
    fn two_pages_shows_both_with_no_ellipsis() {
        use PageSlot::*;
        assert_eq!(page_strip(1, Some(2)), vec![Current(1), Number(2)]);
        assert_eq!(page_strip(2, Some(2)), vec![Number(1), Current(2)]);
    }

    #[test]
    fn exactly_six_pages_shows_every_number_no_ellipsis() {
        use PageSlot::*;
        assert_eq!(
            page_strip(3, Some(6)),
            vec![
                Number(1),
                Number(2),
                Current(3),
                Number(4),
                Number(5),
                Number(6)
            ]
        );
    }

    #[test]
    fn seven_pages_still_fits_six_numbers_plus_one_ellipsis() {
        // One page over the small-`last` shortcut: still exactly 6 numbers,
        // and the algorithm self-corrects into a sane shape even though the
        // near-the-end trigger fires early at this size.
        use PageSlot::*;
        let strip = page_strip(4, Some(7));
        assert_eq!(strip.iter().filter(|s| matches!(s, Ellipsis)).count(), 1);
        let shown: Vec<usize> = strip
            .iter()
            .filter_map(|s| match s {
                Number(n) | Current(n) => Some(*n),
                Ellipsis => None,
            })
            .collect();
        assert_eq!(shown.len(), 6);
        assert!(shown.contains(&1));
        assert!(shown.contains(&7));
        assert!(shown.contains(&4));
        assert!(strip.contains(&Current(4)));
    }

    #[test]
    fn current_equals_last_mirrors_the_current_equals_one_case() {
        // The symmetric case to `the_28_page_example_on_page_one...`: current
        // *is* the `last` boundary, so the band grows downward by one instead.
        use PageSlot::*;
        assert_eq!(
            page_strip(28, Some(28)),
            vec![
                Number(1),
                Ellipsis,
                Number(24),
                Number(25),
                Number(26),
                Number(27),
                Current(28),
            ]
        );
    }

    #[test]
    fn unknown_total_only_names_current_and_its_next_hop() {
        // No `COUNT` behind a filtered search still mid-walk: the strip must
        // not fabricate a "last" this screen cannot back up (see the doc
        // comment). Only 1, current, and current + 1 are honestly nameable.
        use PageSlot::*;
        assert_eq!(page_strip(1, None), vec![Current(1), Number(2)]);
        assert_eq!(page_strip(2, None), vec![Number(1), Current(2), Number(3)]);
        assert_eq!(
            page_strip(9, None),
            vec![Number(1), Ellipsis, Current(9), Number(10)]
        );
    }

    // Overflow safety (WB-01M032Q6BX8BM7NPK8H3AQKGWF round 2's adversarial-
    // review blocker): `GET /catalog?page=18446744073709551615` (usize::MAX
    // on a 64-bit build) reaching unguarded `page + 1` / `current + 1` /
    // `current + 3` arithmetic panicked an anonymous SSR request in debug
    // builds. The crafted string, verbatim.

    #[test]
    fn the_crafted_overflow_url_clamps_instead_of_parsing_raw() {
        assert_eq!(parse_page(Some("18446744073709551615")), MAX_PAGE);
        // Every other edge `parse_page` exists to defuse, alongside it.
        assert_eq!(parse_page(None), 1);
        assert_eq!(parse_page(Some("")), 1);
        assert_eq!(parse_page(Some("0")), 1);
        assert_eq!(parse_page(Some("-1")), 1);
        assert_eq!(parse_page(Some("not a number")), 1);
        assert_eq!(parse_page(Some("9")), 9);
        // A merely-large-but-parseable number still clamps to the ceiling —
        // not just the one value that happens to equal `usize::MAX`.
        assert_eq!(parse_page(Some("999999999999")), MAX_PAGE);
    }

    #[test]
    fn page_strip_does_not_panic_or_wrap_on_the_largest_possible_current() {
        // `page_strip` is unit-tested directly against the raw, unclamped
        // value too — defense in depth, independent of whatever a caller
        // (`parse_page`, or a direct hosted-API caller bypassing the UI
        // entirely) already clamped. Must not panic; the exact shape is not
        // the point (nothing sane can be shown for this input) — only that a
        // huge `current` alone, with no `last`, produces a small, bounded
        // strip rather than climbing toward `usize::MAX`.
        let strip = page_strip(usize::MAX, None);
        assert_eq!(strip.len(), 4); // [1] ... current [current+1], saturated
        assert!(strip.contains(&PageSlot::Current(usize::MAX)));
        // `current + 1` saturates rather than wrapping to 0.
        assert!(strip.contains(&PageSlot::Number(usize::MAX)));

        // The same input with a `last` that also happens to be huge (the
        // `last.max(current)` defensive clamp reaching for `usize::MAX` too)
        // — still no panic, still a bounded result: at most 6 *numbers*
        // (`strip.len()` itself can run a couple over that, `...` fillers
        // between them).
        let strip = page_strip(usize::MAX, Some(773));
        assert!(!strip.is_empty());
        let numbered = strip
            .iter()
            .filter(|s| !matches!(s, PageSlot::Ellipsis))
            .count();
        assert!(numbered <= 6, "{numbered} numbered slots: {strip:?}");
    }

    /// WB-01M033AFA0VSCGB8Z3HTYPFZVD: the whole point of picking 60 over 50 was
    /// that it divides evenly by every column count [`GRID_CLASS`] actually
    /// uses (2, 3, 4, 6 — 5 and the 7-9 tier are deliberately skipped, so they
    /// are not asserted here), so a full page always tiles into whole rows
    /// with no partial remainder. This pins that property directly against
    /// the constant rather than trusting the arithmetic never regresses back
    /// toward 50 (which only divides evenly by 2 and 5 of that set).
    #[cfg(feature = "ssr")]
    #[test]
    fn the_catalog_page_size_divides_evenly_by_every_grid_column_count() {
        use super::CATALOG_PAGE_SIZE;

        assert_eq!(CATALOG_PAGE_SIZE, 60);
        for columns in [2, 3, 4, 6] {
            assert_eq!(
                CATALOG_PAGE_SIZE % columns,
                0,
                "{CATALOG_PAGE_SIZE} does not divide evenly by {columns} columns"
            );
        }
        // The wide-viewport tier: 60 / 10 = 6 whole rows, not a partial one.
        assert_eq!(CATALOG_PAGE_SIZE % 10, 0);
    }

    /// [`GRID_CLASS`] must name exactly the column tiers the maintainer asked
    /// for (2, 3, 4, 6, 10) and nothing in the deliberately-skipped 5/7-9
    /// range — a typo here (e.g. `grid-cols-5`) would silently reintroduce
    /// the uneven-row problem this whole task exists to fix.
    #[test]
    fn grid_class_names_only_the_approved_column_tiers() {
        for present in [
            "grid-cols-2",
            "sm:grid-cols-3",
            "lg:grid-cols-4",
            "xl:grid-cols-6",
            "3xl:grid-cols-10",
        ] {
            assert!(
                super::GRID_CLASS.contains(present),
                "GRID_CLASS is missing {present}: {}",
                super::GRID_CLASS
            );
        }
        for skipped in ["grid-cols-5", "grid-cols-7", "grid-cols-8", "grid-cols-9"] {
            assert!(
                !super::GRID_CLASS.contains(skipped),
                "GRID_CLASS names a deliberately-skipped tier {skipped}: {}",
                super::GRID_CLASS
            );
        }
        // The old cap that froze the container at 1280px — the literal bug
        // report ("only fills about half the available width" on a 2560px
        // monitor) — must not come back.
        assert!(!super::GRID_CLASS.contains("max-w-7xl"));
    }
}
