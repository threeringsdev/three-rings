//! `/my/collections/:id` — the binder / deck view (specs/app-ui.md →
//! `/my/collections/:id`; design/wireframes.pen → "Desktop — Collection view"
//! and "Mobile — Collection view").
//!
//! Five things are worth knowing before editing this file.
//!
//! **The header counts the collection; the table counts the page.**
//! `CollectionView.cards` is one keyset page, but `CollectionView.totals` is
//! whole-collection (specs/collection-api.md computes row aggregates per
//! visible page — a header that did the same would change as you paged and be
//! describing the page, not the binder). The wireframe's
//! `120 here (102 own + 18 rolled up) · 6 wanted` and the needs chip's
//! `6 missing — 4 owned elsewhere · 2 to buy` are exactly those numbers.
//!
//! **Folder rows are rows.** The wireframe puts child collections in the same
//! table as the cards, above them, sharing the three numeric columns — a
//! folder's HERE is its rolled-up count, italic and dimmed because it is
//! elsewhere. Their counts *and, since P6-127, which rows exist at all* come
//! from the shell's collection-tree resource, not from a second read, so a
//! folder row and the sidebar badge for the same node cannot disagree — and a
//! rename or a `New binder inside…` reaches them without refetching the card
//! table. The payload's own `children` remains the fallback for a tree that
//! does not know this node. The breadcrumb (and the mobile back link) walk that
//! same assembled tree.
//!
//! **HERE is editable and the header follows it.** A card cell backed by
//! exactly one `holdings` row carries the stepper
//! ([`crate::components::holding_stepper::HaveStepper`], lifted so
//! `/cards/:id` can reuse the same write semantics); a cell that sums several
//! finish/condition/language grains does not, because a lone number cannot
//! say which grain it meant (`CardRow::holding_id` encodes that). A commit
//! does **not** refetch the view: remounting the row would dispose the
//! stepper the undo toast is about to call back into. The optimistic value is
//! already right, the sidebar tree is refetched for its badges, and the
//! header's own count follows a delta so the two never disagree on screen —
//! a delta that belongs to the rendered payload and is zeroed by the next one,
//! never by the URL (see [`CollectionPage`]).
//!
//! **A committed 0 removes the card, and it is undoable.** It is a *move with no
//! destination* (`remove_holding` → `move_holding(to = None)`), not a delete: the
//! server reads the grain and the board off the named holding inside the write
//! transaction and appends a ledger row, so Undo returns those copies to that
//! board. The stepper's own toast is suppressed for that commit
//! (`caller_reports`) because its undo is "re-commit the old count", which posts
//! a dead id. The floor was 1 for two tasks to make the destructive commit
//! unreachable; the cost was a binder card that could not be removed at all.
//!
//! **A deck is the same page with three differences** (spec): a header card
//! for format + commanders, cards grouped by board and type with slot counts,
//! and Want as the add default. The teardown action is the fourth.
//!
//! **The URL is the whole view state** — `?q=` (in-collection quick search)
//! and `?cursor=` (keyset page), same contract as `/my`.
//!
//! **Nothing in [`CollectionPage`]'s setup body may read `view_res`** — not even
//! from inside an `Effect` (P6-068). A `Resource` read registers the *nearest
//! `SuspenseContext` in the owner chain*, and this route's nearest one is
//! `RequireAuth`'s `<Suspense>` (`crate::shell`), an ancestor of every boundary
//! this page's own view can contain. Every such read therefore made a `?q=`
//! refresh re-suspend the auth guard, which unmounted and re-inserted the whole
//! `/my/*` subtree: the caret left the query bar and any showing native popover
//! was removed without firing `toggle`. The two boundary-crossing consumers —
//! the quick-add panel (which sits *between* the two `Transition`s on purpose)
//! and the `here_delta` reset — read plain `RwSignal`s written from inside the
//! header's `Transition` instead. `RwSignal` reads do no `SuspenseContext`
//! lookup at all, so a query refresh structurally cannot reach the guard.
//!
//! **The header's `⋯` is the tree's menu, aimed at this route** ([`HeaderKebab`]):
//! one `menu_target`, one `TreeMenu`, one set of dialogs, a second `context_menu`
//! instance. Two consequences land in *this* file. Deleting *this* collection
//! navigates up instead of leaving the page on a dead id —
//! `tree_manage::route_after_delete`. Only this one: deleting an ancestor no
//! longer takes the route with it, because the children survive by moving up a
//! level (specs/collection-deletion.md). And a tree mutation has to reach this
//! page's own read somehow, which is the next paragraph.
//!
//! **A tree mutation reaches this page two ways, and which one matters**
//! (P6-127). `view_res` takes `TreeManage::content_revision` as a source — the
//! *subset* of tree mutations that can move copies or move which collection
//! they roll up into (a delete and its undo, a reparent). It deliberately does
//! **not** take `TreeManage::revision`, which every tree mutation bumps: a
//! refetch rebuilds the card table, and rebuilding the card table re-seeds
//! every stepper and disposes the row its undo toast is pointing at — the
//! "Undo silently did nothing" defect recorded below against awaiting the
//! *tree*, resurrected from the other side. Renaming a collection cannot
//! change a card row, so it must not be able to rebuild one.
//!
//! Everything a create or a rename *does* change here is therefore taken from
//! the collection tree, which those mutations already refetch, published out of
//! one nested boundary as [`TreeFacts`] and read as a plain signal: the `<h1>`,
//! the folder rows' identity, the quick-add destination's name, and the kebab's
//! snapshot. Each of them falls back to the payload's own copy when the tree
//! does not know this node (a collection the cached tree predates, or a failed
//! tree read), so a broken tree read still leaves the page exactly as complete
//! as it was before.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use leptos_router::NavigateOptions;
use shared::{Board, CardRow, CardSummary, CollectionKind, CollectionView, Id, QuickAddKind};
use std::collections::{HashMap, HashSet};

use super::tree::{assemble, element_anchor, CollectionTreeResource, TreeNode};
use super::tree_manage::{MenuTarget, TreeManage, TreeMenu};
use crate::cards::CardPreview;
use crate::catalog::destination::Destination;
use crate::catalog::GRID_CLASS;
use crate::components::quick_add::{present_matches, PresentMatch, QuickAddPanel};
use crate::components::states::ErrorNote;
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::breadcrumb::{
    Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator,
};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::context_menu::{use_context_menu, ContextMenu};
use crate::components::ui::count_stepper::StepperCommit;
use crate::components::ui::dialog::{
    Dialog, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader,
    DialogTitle,
};
use crate::components::ui::selection_tray::{
    use_selection, SelectedCard, SelectionCheckbox, SelectionKey,
};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};
use crate::components::view_switch::ViewSwitch;

/// The keyset page cursor, in the URL beside `?q=`.
const CURSOR_PARAM: &str = "cursor";

/// `?view=grid` renders the tile grid; anything else (including absent) is the
/// table — same opposite-of-Catalog polarity as `crate::my::all_cards`'s
/// `VIEW_PARAM`/`GRID_VIEW` (see that doc comment): this route shipped
/// table-only long before the grid task, and a bare `/my/collections/:id`
/// link must keep rendering exactly what it always has.
const VIEW_PARAM: &str = "view";
const GRID_VIEW: &str = "grid";

/// Is `?view=` asking for the grid? Pure so it is unit-testable without a
/// query map.
fn is_grid_view(raw: Option<&str>) -> bool {
    raw == Some(GRID_VIEW)
}

/// Build `/my/collections/{id}?q=…&view=…&cursor=…`, omitting empty parts —
/// the single place such a URL is constructed, so the query bar, the clear
/// button, the view switch and the pager cannot drift on its canonical form.
fn collection_url(id: &str, q: &str, list_view: bool, cursor: Option<&str>) -> String {
    let mut url = format!("/my/collections/{id}");
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

/// Which quick action leads in this collection (specs/app-ui.md: the deck
/// variant is "Want-led"; "binders and Inbox are Have-led"). Exported because
/// the quick-add panel is a separate task that must not re-derive the rule —
/// the Inbox is a binder, so the two sentences are one condition.
pub fn add_default(kind: CollectionKind) -> QuickAddKind {
    match kind {
        CollectionKind::Deck => QuickAddKind::Want,
        CollectionKind::Binder => QuickAddKind::Have,
    }
}

/// What `CollectionPage`'s resource carries.
///
/// A named field, the same pattern as [`crate::catalog::SearchPayload`] and
/// [`crate::cards::CardDetailPayload`], and this is the payload that baited the
/// other two: its serialized `{collection, children, cards, next_cursor, totals,
/// commanders}` is what `SearchResults` cross-decoded (`CardRow` is a structural
/// superset of `CardSummary`), and its sibling quick-add slot is what
/// `AllCardsView` cross-decoded. Wrapping it is what turns "structurally
/// compatible" into "must share a unique field name", which closes the *class*
/// rather than the next instance of it.
///
/// The `Result` stays outside the struct here, unlike the other two payloads: this
/// resource's error arm is a first-class rendering (`LoadError`, with paging-aware
/// escape hatches) rather than something the page merely reports, and the
/// `?`-through-`Ok(...)` shape keeps that arm untouched. A wrapper's job is to
/// make the payload *unmistakable*, not to relocate error handling.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionViewPayload {
    collection_view: CollectionView,
}

/// Everything the quick-add panel needs from this page's payload, as a plain
/// value the page can park in an `RwSignal`.
///
/// It exists to keep `view_res` out of [`CollectionPage`]'s setup body (P6-068,
/// and the module doc on why that matters): the panel is rendered *outside* both
/// of the page's `Transition`s, so whatever feeds it must be readable without
/// touching a `Resource`. Built once per payload, inside the header's boundary.
///
/// `collection_id` is not a fourth thing the panel needs — it is the **stamp**
/// that says which collection the other three describe, so a retained value can
/// be refused the moment the URL names a different one (see [`CollectionPage`]'s
/// `live_facts`).
#[derive(Clone, PartialEq)]
struct QuickAddFacts {
    collection_id: Id,
    destination: Destination,
    kind: QuickAddKind,
    present: Vec<PresentMatch>,
}

/// What the **collection tree** says about the collection this route names —
/// the naming-and-shape half of the page, published as a plain value so the
/// page can read it without a second read of `collection_view` (P6-127).
///
/// It exists because `view_res` no longer refetches on a create or a rename
/// (see the module doc): those two change a name and a child list, both of
/// which the tree already carries and every tree mutation already refetches.
/// Resolved in **one** nested boundary rather than per-consumer, so the `<h1>`,
/// the folder rows and the kebab's snapshot cannot disagree about which name
/// this collection has.
///
/// Every consumer keys on `id` before using it: the tree resolves from cache
/// almost immediately while a payload takes a round trip, so on a navigation
/// these facts describe the *new* collection while the table still shows the
/// old one's rows. Keyed, that window falls back to the payload's own copies —
/// exactly what the page showed before this existed.
#[derive(Clone, PartialEq)]
struct TreeFacts {
    id: Id,
    name: String,
    /// This node's children, in the tree's order — which is the server's
    /// `(position, name)`, the same order `CollectionView::children` uses.
    children: Vec<shared::CollectionSummary>,
}

/// The tree's name for `id`, when it has one. `None` falls the caller back to
/// the payload's own name (see [`TreeFacts`]).
fn tree_name(facts: RwSignal<Option<TreeFacts>>, id: Id) -> Option<String> {
    facts.with(|f| f.as_ref().filter(|f| f.id == id).map(|f| f.name.clone()))
}

/// The folder rows to render under `id`: the tree's children when it knows this
/// node, else `fallback` — the payload's own `children`, which is where this row
/// set came from before P6-127.
///
/// Pure, and split out for it: this is the whole of "a `New binder inside…`
/// still adds its row without refetching the card table", and the fallback is
/// the whole of "a failed tree read leaves the page no worse than before".
fn folder_rows(
    facts: Option<&TreeFacts>,
    id: Id,
    fallback: &[shared::CollectionSummary],
) -> Vec<shared::CollectionSummary> {
    match facts {
        Some(f) if f.id == id => f.children.clone(),
        _ => fallback.to_vec(),
    }
}

/// Fold a rendered payload into what the quick-add panel reads. All three come
/// from the one `collection_view` the header is rendering, so the panel adds
/// *here*, names this collection in its toast, and lists what is already here —
/// none of it re-derived or re-fetched, so none of it can disagree with the
/// header and the table.
fn quick_add_facts(view: &CollectionView) -> QuickAddFacts {
    QuickAddFacts {
        collection_id: view.collection.id,
        destination: Destination {
            id: view.collection.id,
            name: view.collection.name.clone(),
            is_inbox: view.collection.is_inbox,
        },
        kind: add_default(view.collection.kind),
        present: present_matches(&view.cards),
    }
}

#[component]
pub fn CollectionPage() -> impl IntoView {
    let params = use_params_map();
    let query_map = use_query_map();

    // Memos, not plain reads: a navigation that changes only the cursor must
    // not invalidate anything keyed on `q` (and vice versa).
    let url_id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let url_q = Memo::new(move |_| query_map.read().get("q").unwrap_or_default());
    let url_cursor = Memo::new(move |_| query_map.read().get(CURSOR_PARAM).unwrap_or_default());
    // "Is the table showing right now" — table stays the default (see
    // `VIEW_PARAM`'s doc comment on why this route's polarity is the opposite
    // of `/catalog`'s own `list_view`).
    let list_view = Memo::new(move |_| !is_grid_view(query_map.read().get(VIEW_PARAM).as_deref()));

    let query_text = RwSignal::new(url_q.get_untracked());
    let tree = expect_context::<CollectionTreeResource>().0;

    // The selection tray's batch move writes to *this* collection's rows from
    // the shell, where it has no handle on this resource. Taking the revision
    // as a source makes the refetch structural: a move bumps it, the resource
    // re-runs, HERE and the totals move with the database.
    let revision = crate::my::move_selection::holdings_revision();
    // The same trick for the *collection tree's* mutations — but only for the
    // ones that can have moved copies (`content_revision`, not `revision`).
    // A delete relocates the deleted node's holdings, its Undo brings them back,
    // and a reparent moves a whole subtree's copies from one rollup to another;
    // all three change numbers in this table and must refetch it. A create or a
    // rename changes no copy anywhere, and refetching for one would rebuild the
    // card table under a live undo toast — see the module doc (P6-127). What
    // *those* change here comes from `tree_facts` below instead.
    let manage = expect_context::<TreeManage>();

    let view_res = Resource::new(
        move || {
            (
                url_id.get(),
                url_q.get(),
                url_cursor.get(),
                revision.get(),
                manage.content_revision.get(),
            )
        },
        |(id, q, cursor, _revision, _tree_content_revision)| async move {
            let id = Id::parse_str(&id).map_err(|_| {
                // `ApiError::Validation`, typed (P6-083) — a malformed id in the
                // URL is a *request* failure that will never resolve. Read as a
                // transport failure it used to offer a "Try again" that
                // re-parsed the same broken string forever; `ServerFnError::from`
                // (not `crate::api_err`, which is `ssr`-only and this fetcher
                // also runs client-side) puts it on the same typed wire every
                // `collection_view` failure already carries, instead of
                // hand-rolling a `validation:` prefix no consumer has to parse
                // anymore.
                ServerFnError::from(shared::ApiError::Validation(
                    "that is not a collection id".into(),
                ))
            })?;
            let cursor = (!cursor.is_empty()).then_some(cursor);
            Ok(CollectionViewPayload {
                collection_view: crate::collection_view(id, q, cursor).await?,
            })
        },
    );

    // Copies committed through the steppers that the *currently rendered*
    // totals do not yet include. The header adds it so HERE and "N here" cannot
    // disagree without a reload (see the module doc on why a commit does not
    // refetch the view).
    let here_delta = RwSignal::new(0);
    // The same, for the WANTED cells' steppers: copies committed through them
    // that the rendered `totals.desired` does not yet include. Zeroed by the
    // same payload `here_delta` is, for the same reason.
    let want_delta = RwSignal::new(0);
    // `here_delta`'s per-row twin, for the grid — see [`RowDeltas`]'s doc.
    // Reset alongside `here_delta` below, for the identical reason: a fresh
    // payload already contains everything committed before it.
    let row_deltas = RowDeltas::new();

    // ---- the two things that must be readable from outside every boundary ----
    //
    // Both are written by the **header's** `Transition` body, from the payload
    // it is about to render, and read out here as plain signals. Neither may be
    // derived from `view_res`: this is the page's setup body, whose owner is an
    // ancestor of every boundary in the view below, so a `Resource` read here
    // registers on `RequireAuth`'s `<Suspense>` and a `?q=` refresh then
    // unmounts the whole page (P6-068 — see the module doc).
    //
    // **Why the header writes them and not the table.** They are two boundaries
    // awaiting the same resource, so whichever writes decides the ordering, and
    // only one ordering is sound for `here_delta`: the delta exists to keep the
    // header's "N here" agreeing with the HERE cells while a commit is
    // un-refetched, so it must be zeroed *by the same body that puts the fresh
    // totals on screen*. The table writing it would leave a window where the
    // header had already rendered fresh totals with the stale delta still added
    // on top — the teardown double-count noted below, reintroduced. The reverse
    // window is harmless: nothing in the table reads `here_delta` to render, only
    // to update it.
    let facts = RwSignal::new(None::<QuickAddFacts>);

    // The retained payload facts, refused once the URL names a different
    // collection. This gate is the whole safety story of retaining them
    // (P6-068): the panel now keeps the last destination through a re-search
    // instead of collapsing to `None` — which is the point, since `⏎` mid-search
    // used to hit quick-add's "Still loading this collection" guard — but a
    // *navigation to another collection* must never leave a stale destination
    // reachable, or `⏎` in the window before the new payload lands would add to
    // the collection you just left. Keyed on the URL rather than cleared by an
    // `Effect` so there is no such window at all: the read itself is the check.
    let live_facts = Memo::new(move |_| {
        let want = Id::parse_str(&url_id.read()).ok();
        facts.get().filter(|f| Some(f.collection_id) == want)
    });

    // The tree's answer to "what is this collection called, and what is inside
    // it" — the third thing readable from outside every boundary, written by
    // the nested `<Suspense>` below rather than by the header's `Transition`,
    // because it is the *tree* it awaits and nothing else on this page may
    // (P6-127; see [`TreeFacts`] and the module doc).
    let tree_facts = RwSignal::new(None::<TreeFacts>);

    let paged = Memo::new(move |_| !url_cursor.read().is_empty());
    let teardown_open = RwSignal::new(false);

    // The view switch's own navigation: relayouting the page you are on is not
    // a query edit, so the cursor rides along (same rule `/catalog`'s and
    // `/my`'s switches follow).
    let navigate = use_navigate();
    let go = move |list: bool| {
        let id = url_id.get_untracked();
        let q = url_q.get_untracked();
        let cursor = url_cursor.get_untracked();
        navigate(
            &collection_url(
                &id,
                &q,
                list,
                (!cursor.is_empty()).then_some(cursor.as_str()),
            ),
            NavigateOptions::default(),
        );
    };

    // Memos, not raw reads: the panel re-renders on every keystroke.
    // The name comes from the tree when it has one: the toast this destination
    // titles ("Added X to Y") must not still say the old Y after a rename, and
    // a rename no longer refetches the payload `facts` was folded out of.
    let quick_add_destination = Memo::new(move |_| {
        live_facts.with(|f| {
            f.as_ref().map(|f| {
                let mut destination = f.destination.clone();
                if let Some(name) = tree_name(tree_facts, destination.id) {
                    destination.name = name;
                }
                destination
            })
        })
    });
    // Have until the first payload lands. Nothing can be added before then
    // either (the destination is `None` too), so the footer's hint is the only
    // thing briefly generic.
    let quick_add_kind =
        Memo::new(move |_| live_facts.with(|f| f.as_ref().map_or(QuickAddKind::Have, |f| f.kind)));
    let quick_add_present = Memo::new(move |_| {
        live_facts.with(|f| f.as_ref().map(|f| f.present.clone()).unwrap_or_default())
    });

    view! {
        <div class="flex min-w-0 flex-col gap-4 p-4 md:p-6" data-testid="collection-page">
            // Header and body await the same resource in two blocks so the
            // query bar between them is *outside* both: a Transition re-renders
            // its whole child when the new value lands, which would rebuild the
            // input under the caret mid-search.
            //
            // NB neither block awaits the *tree* resource. Everything tree-
            // derived (breadcrumb, folder rows and their counts, teardown
            // destinations, the `TreeFacts` above) awaits it in its own nested
            // boundary instead, so a `tree.refetch()` —
            // which every stepper commit fires, to keep the sidebar badges
            // honest — re-renders those and nothing else. Awaiting it out here
            // remounted every card row on each commit, which re-seeded the
            // stepper's `value` from the stale fetched count and left the undo
            // toast pointing at a signal whose value already matched: Undo
            // silently did nothing and the header kept the old delta. Found as
            // an under-load flake in the e2e; it is a real defect, not a flake.
            <Transition fallback=|| {
                view! { <HeaderSkeleton /> }
            }>
                {move || Suspend::new(async move {
                    let payload = view_res.await.map(|p| p.collection_view);
                    // The delta belongs to a payload, so it is zeroed by a
                    // payload — every new one (a navigation, a re-search, or the
                    // teardown's refetch) already contains everything committed
                    // before it. Zeroed *here*, one statement before the header
                    // below is built from that same payload, so the fresh totals
                    // and the zero land together. Keying it on the *URL* instead
                    // was wrong twice over: a `view_res.refetch()` at the same
                    // URL left the delta applied on top of fresh totals
                    // (teardown emptied a deck to zero and the header read
                    // "1 here"), and a navigation zeroed it while the
                    // `Transition` was still showing the pre-commit totals it
                    // belonged to. The error arm zeroes it too — that arm
                    // *replaces* the header, so no totals are left on screen for
                    // a delta to correct.
                    here_delta.set(0);
                    want_delta.set(0);
                    row_deltas.reset();
                    match payload {
                        Ok(view) => {
                            facts.set(Some(quick_add_facts(&view)));
                            view! {
                                <CollectionHeader
                                    view
                                    here_delta
                                    want_delta
                                    teardown_open
                                    view_res
                                    tree
                                    tree_facts
                                />
                            }
                                .into_any()
                        }
                        Err(e) => {
                            // No destination beats the last good one here: this
                            // collection did not load, so the panel says "still
                            // loading" rather than adding into a payload the
                            // page is showing an error instead of.
                            facts.set(None);
                            view! { <LoadError e view_res paged url_id url_q list_view /> }
                                .into_any()
                        }
                    }
                })}
            </Transition>

            <div class="flex items-center gap-2">
                <div class="min-w-0 flex-1">
                    // The wireframe's one field: it filters this collection
                    // *and* opens the quick-add type-ahead over the catalog
                    // (specs/app-ui.md → Quick-add panel). Both halves answer
                    // the same `?q=`, so the panel's IN THIS COLLECTION rows and
                    // the table behind it can't describe different queries.
                    <QuickAddPanel
                        field_id="collection-query"
                        text=query_text
                        url_q
                        // A new search starts at page one: carrying the old
                        // cursor forward pages into a set that no longer exists.
                        // The current layout rides along untracked, same as
                        // `/catalog`'s and `/my`'s own `QueryBar`/`QuickAddPanel`.
                        to_url=Callback::new(move |q: String| {
                            collection_url(&url_id.get_untracked(), &q, list_view.get_untracked(), None)
                        })
                        placeholder="Add or find cards…"
                        aria_label="Search this collection or add cards"
                        destination=quick_add_destination
                        default_kind=quick_add_kind
                        present=quick_add_present
                        on_undo=Callback::new(move |()| view_res.refetch())
                    />
                </div>
                <SlashHint />
                <ViewSwitch list_view on_change=Callback::new(go) />
            </div>

            <Transition fallback=|| {
                view! { <RowsSkeleton /> }
            }>
                {move || {
                    // `list_view` deliberately not read here — this closure is
                    // the `Transition`'s child, and reading it would re-await
                    // `view_res` on a pure layout toggle. `CollectionBody` and
                    // `Pager` take the `Memo` itself and read it in their own
                    // scope instead (same split as `crate::my::all_cards`'s
                    // `AllCardsBody` — see its comment for the fuller account).
                    let q = url_q.get();
                    let id = url_id.get();
                    Suspend::new(async move {
                        match view_res.await.map(|p| p.collection_view) {
                            Ok(view) => {
                                let next = view.next_cursor.clone();
                                let searching = !q.is_empty();
                                view! {
                                    <CollectionBody
                                        view
                                        searching
                                        paged
                                        here_delta
                                        want_delta
                                        tree
                                        tree_facts
                                        list_view
                                        row_deltas
                                    />
                                    <Pager next paged q id list_view />
                                }
                                    .into_any()
                            }
                            // The header block already rendered the message; a
                            // second copy would just repeat it.
                            Err(_) => ().into_any(),
                        }
                    })
                }}
            </Transition>

            // The tree-derived naming and shape of this collection, resolved in
            // its own boundary and published as a plain signal (P6-127). It
            // renders nothing: the consumers are the `<h1>`, the folder rows,
            // the quick-add destination and the kebab's snapshot, which sit in
            // three different places and must not each grow a tree read of
            // their own. Own boundary for the reason every other tree read on
            // this page has one — a `tree.refetch()` fires on every stepper
            // commit, and it must re-render this and nothing else.
            //
            // **Last in the view on purpose.** This route is `SsrMode::Async`,
            // which renders the whole page in document order once every
            // resource has resolved, so a publisher placed above its consumers
            // would put *tree*-derived names and rows in the server HTML while
            // the client's first pass — where this signal starts at `None` —
            // renders the payload's. Publishing after them makes both passes
            // read the payload, and the correction lands one tick later on the
            // client only.
            <Suspense fallback=|| ()>
                {move || {
                    let id = url_id.get();
                    Suspend::new(async move {
                        let nodes = assembled_roots(tree.await);
                        let next = Id::parse_str(&id)
                            .ok()
                            .and_then(|id| {
                                find_tree_node(&nodes, id)
                                    .map(|node| TreeFacts {
                                        id,
                                        name: node.row.summary.name.clone(),
                                        children: node
                                            .children
                                            .iter()
                                            .map(|c| c.row.summary.clone())
                                            .collect(),
                                    })
                            });
                        // Compare before writing: every stepper commit refetches
                        // the tree, and an unconditional `set` would notify the
                        // folder rows (and the quick-add panel) on each one.
                        if tree_facts.with_untracked(|cur| *cur != next) {
                            tree_facts.set(next);
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// The assembled tree's roots, or an empty forest when the shell has no tree
/// (anonymous, or the read failed). Every consumer here degrades to "no
/// breadcrumb / no folder counts" rather than failing the page.
pub(crate) fn assembled_roots(
    dto: Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>,
) -> Vec<TreeNode> {
    match dto {
        Some(Ok(t)) => assemble(t).roots,
        _ => Vec::new(),
    }
}

/// The human-facing half of a server-fn error (the transport only carries
/// `ApiError`'s `Display` string).
pub(crate) fn message_of(e: &ServerFnError<shared::ApiError>) -> String {
    match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
    }
}

/// The header's failed arm — and, because the header is what carries the
/// breadcrumb and the mobile back link, the *only* thing on the page when the
/// read fails (the body's own error arm renders nothing, deliberately: a second
/// copy of one message).
///
/// That made it a dead end, and this route reaches it two ways that have nothing
/// for the user to fix: a link to a **deleted** collection (`not found`, where a
/// retry re-asks the same dead id), and a shared **`?cursor=`** gone stale — the
/// case `/catalog`'s pager arm was given a way home for. So the way out is
/// unconditional here rather than paged-only, and page one of the same collection
/// is offered on top of it when there is a cursor to drop.
#[component]
fn LoadError(
    e: ServerFnError<shared::ApiError>,
    view_res: Resource<Result<CollectionViewPayload, ServerFnError<shared::ApiError>>>,
    paged: Memo<bool>,
    url_id: Memo<String>,
    url_q: Memo<String>,
    list_view: Memo<bool>,
) -> impl IntoView {
    view! {
        <ErrorNote
            what="Couldn't load this collection"
            e
            testid="collection-error"
            retry=Callback::new(move |()| view_res.refetch())
        >
            <Show when=move || paged.get()>
                <a
                    href=move || {
                        collection_url(&url_id.get(), &url_q.get(), list_view.get(), None)
                    }
                    class="text-destructive text-sm font-medium underline"
                    data-testid="page-first"
                >
                    "← Back to the start"
                </a>
            </Show>
            <a
                href="/my"
                class="text-destructive text-sm font-medium underline"
                data-testid="collection-error-home"
            >
                "My cards"
            </a>
        </ErrorNote>
    }
}

#[component]
fn HeaderSkeleton() -> impl IntoView {
    view! {
        <div class="space-y-2" aria-busy="true" aria-label="Loading this collection">
            <Skeleton class="h-4 w-64" />
            <Skeleton class="h-8 w-72" />
        </div>
    }
}

#[component]
fn RowsSkeleton() -> impl IntoView {
    view! {
        <div class="space-y-2" aria-busy="true" aria-label="Loading these cards">
            {(0..8).map(|_| view! { <Skeleton class="h-10 w-full" /> }).collect_view()}
        </div>
    }
}

/// The wireframe's `/` affordance beside the quick search. Desktop only — a
/// touch keyboard has no such key, and the hint would be a lie there.
#[component]
fn SlashHint() -> impl IntoView {
    // `/` focuses the search unless the caret is already in a field.
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        let handle = window_event_listener(leptos::ev::keydown, move |ev| {
            if ev.key() != "/" || ev.meta_key() || ev.ctrl_key() || ev.alt_key() {
                return;
            }
            // Shared with `back_nav::install_back_shortcut`'s identical
            // guard — see that module's doc, last paragraph, for why this
            // stopped being two independent copies of the same check.
            if crate::components::back_nav::focus_is_editable() {
                return;
            }
            if let Some(el) = leptos::prelude::document().get_element_by_id("collection-query") {
                ev.prevent_default();
                if let Some(input) = el.dyn_ref::<leptos::web_sys::HtmlElement>() {
                    let _ = input.focus();
                }
            }
        });
        on_cleanup(move || handle.remove());
    }
    view! {
        <kbd
            class="text-muted-foreground bg-muted hidden rounded border px-1.5 py-0.5 font-mono text-[11px] md:inline-block"
            data-testid="slash-hint"
            aria-hidden="true"
        >
            "/"
        </kbd>
    }
}

// ---------------------------------------------------------------- header ----

/// One breadcrumb hop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Crumb {
    pub(crate) id: Id,
    pub(crate) name: String,
}

/// The chain of collections from the top level down to `id`, inclusive.
/// `None` when the tree does not contain the node (a fresh collection the
/// cached tree predates, or no tree at all) — callers fall back to the
/// collection's own name rather than rendering half a path.
pub(crate) fn ancestor_path(nodes: &[TreeNode], id: Id) -> Option<Vec<Crumb>> {
    for node in nodes {
        if node.row.summary.id == id {
            return Some(vec![Crumb {
                id,
                name: node.row.summary.name.clone(),
            }]);
        }
        if let Some(mut rest) = ancestor_path(&node.children, id) {
            rest.insert(
                0,
                Crumb {
                    id: node.row.summary.id,
                    name: node.row.summary.name.clone(),
                },
            );
            return Some(rest);
        }
    }
    None
}

/// The assembled node for `id`, anywhere in the forest. `None` when the tree
/// does not contain it (a collection created since the shell's tree was
/// fetched, or a failed tree read) — every caller here has a payload-derived
/// fallback for that case rather than rendering something wrong.
pub(crate) fn find_tree_node(nodes: &[TreeNode], id: Id) -> Option<&TreeNode> {
    for n in nodes {
        if n.row.summary.id == id {
            return Some(n);
        }
        if let Some(hit) = find_tree_node(&n.children, id) {
            return Some(hit);
        }
    }
    None
}

/// A node's own copies plus every descendant's — the number its sidebar badge
/// shows, which is what a folder row must agree with. `None` when the tree does
/// not contain the node; the row then shows no badge rather than a wrong one.
fn rolled_up_of(nodes: &[TreeNode], id: Id) -> Option<i64> {
    find_tree_node(nodes, id).map(|n| n.rolled_up)
}

/// Every collection except `exclude`, depth-first, labelled with its path —
/// the teardown destination list. Path labels rather than bare names because
/// two binders can legitimately share a name under different parents, and a
/// destination list is exactly where that ambiguity would bite.
fn flatten_destinations(nodes: &[TreeNode], exclude: Id) -> Vec<(Id, String)> {
    fn walk(nodes: &[TreeNode], prefix: &str, out: &mut Vec<(Id, String)>) {
        for n in nodes {
            let label = if prefix.is_empty() {
                n.row.summary.name.clone()
            } else {
                format!("{prefix} / {}", n.row.summary.name)
            };
            out.push((n.row.summary.id, label.clone()));
            walk(&n.children, &label, out);
        }
    }
    let mut out = Vec::new();
    walk(nodes, "", &mut out);
    // Emptying a deck into itself is a no-op the API would happily perform.
    out.retain(|(id, _)| *id != exclude);
    out
}

/// The wireframe's counts line: `120 here (102 own + 18 rolled up) · 6 wanted`.
/// The parenthetical appears only when something *is* rolled up, and the
/// wanted clause only when something is wanted — an all-zeroes binder should
/// read `0 here`, not a row of noughts.
///
/// Both deltas are the same trick for the same reason (see [`CollectionPage`]'s
/// module doc: a stepper commit deliberately does not refetch the view):
/// `delta` carries the HERE cells' un-refetched copies, `want_delta` the WANTED
/// cells'. The wanted clause is gated on the *live* number, so zeroing a
/// collection's last want drops the clause rather than leaving a stale `· 1
/// wanted` behind.
fn counts_summary(totals: &shared::CollectionTotals, delta: i32, want_delta: i32) -> String {
    let own = totals.present + delta;
    let here = own + totals.present_rollup;
    let mut out = format!("{here} here");
    if totals.present_rollup > 0 {
        out.push_str(&format!(
            " ({own} own + {} rolled up)",
            totals.present_rollup
        ));
    }
    let wanted = totals.desired + want_delta;
    if wanted > 0 {
        out.push_str(&format!(" · {wanted} wanted"));
    }
    out
}

/// The needs chip, per the storyboard: `7 missing — 4 owned elsewhere · 3 to
/// buy`. `None` when nothing is missing — a deck that is complete has no chip
/// rather than a chip reading zero.
///
/// Shared with `/my/collections/:id/needs` (`super::needs`), which folds the
/// same sentence out of the rows it is showing: one formatter, so the chip and
/// the page it links to cannot word the same numbers differently.
pub(crate) fn needs_chip(totals: &shared::CollectionTotals) -> Option<String> {
    if totals.missing <= 0 {
        return None;
    }
    let mut out = format!("{} missing", totals.missing);
    let mut parts = Vec::new();
    if totals.owned_elsewhere > 0 {
        parts.push(format!("{} owned elsewhere", totals.owned_elsewhere));
    }
    if totals.to_buy > 0 {
        parts.push(format!("{} to buy", totals.to_buy));
    }
    if !parts.is_empty() {
        out.push_str(" — ");
        out.push_str(&parts.join(" · "));
    }
    Some(out)
}

/// What the header actually renders in the chip's slot, past "nothing to
/// say" — [`needs_chip`]'s missing-N sentence, or this task's neutral
/// variant (P6-143). `/my/collections/:id/needs` had a designed empty state
/// ("All set", `app/src/my/needs.rs`) but no navigation path to it: the chip
/// was the page's only link and rendered only when something was missing, so
/// a collection that reached "nothing missing" lost the chip entirely and the
/// empty state became reachable only by hand-typing the URL.
///
/// **Satisfied vs. absent.** `desired > 0 && missing <= 0` — the collection
/// has wants and every one is met — gets the neutral chip, still linking to
/// `/needs`. `desired == 0` — no desires at all — renders nothing, same as
/// before: there is no needs concept in play for a binder nobody has marked
/// wanted, so there is nothing to check off either, and a chip on every
/// binder in the tree would be noise the design never asked for
/// (design/information-architecture.md:41 puts the chip on "a deck or
/// collection header", not on every header unconditionally).
#[derive(Debug, PartialEq, Eq)]
enum ChipState {
    Missing(String),
    Satisfied,
}

fn chip_state(totals: &shared::CollectionTotals) -> Option<ChipState> {
    if let Some(text) = needs_chip(totals) {
        return Some(ChipState::Missing(text));
    }
    (totals.desired > 0).then_some(ChipState::Satisfied)
}

#[component]
fn CollectionHeader(
    view: CollectionView,
    here_delta: RwSignal<i32>,
    /// `here_delta`'s wants twin — see [`counts_summary`]. Read only by the
    /// counts line; the needs chip below is deliberately **not** live (it is
    /// derived from `missing`/`owned_elsewhere`, which no client-side delta can
    /// recompute, and a HERE commit has always left it stale the same way).
    want_delta: RwSignal<i32>,
    teardown_open: RwSignal<bool>,
    view_res: Resource<Result<CollectionViewPayload, ServerFnError<shared::ApiError>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
    /// The tree's naming and shape for this collection, published by the page
    /// (see [`TreeFacts`]). The `<h1>` and the kebab's snapshot read it because
    /// a rename no longer refetches `view`.
    tree_facts: RwSignal<Option<TreeFacts>>,
) -> impl IntoView {
    let id = view.collection.id;
    let name = view.collection.name.clone();
    let kind = view.collection.kind;
    let format = view.collection.format.clone();
    let totals = view.totals;
    let commanders = view.commanders.clone();
    let chip = chip_state(&totals);

    // ---- what the header kebab aims the shared tree menu at ----
    //
    // The subject is *this route's* collection, snapshotted when the menu opens
    // — the same discipline `DeleteReq`/`MoveReq` follow, and for a sharper
    // reason here: `menu_target` is one signal shared with the sidebar's rows, so
    // a snapshot that outlived its aim would act on whatever was right-clicked
    // last.
    let manage = expect_context::<TreeManage>();
    let collection = StoredValue::new(view.collection.clone());
    let payload_name = StoredValue::new(name.clone());
    // Read *untracked at click time* rather than awaited in a nested `Suspense`.
    // Two reasons: a header action that pops into existence when a second read
    // lands is worse than one that is simply always there, and — decisively — a
    // `Provider` above a `Suspense` does not reach a `use_context_menu()` call
    // made inside it (the rule that forced the tree's own menu wrapper inside its
    // boundary), so the kebab could not find its panel from in there.
    // **Own counts, not rolled up** (specs/collection-deletion.md → step 4):
    // `totals.present`/`totals.desired` are this collection alone, unlike
    // `present_total()` which folds in every descendant's copies — a delete
    // relocates only this node's own holdings/desires, so the confirm's
    // count must not overstate that.
    //
    // The **name** and the **immediate children** are the tree's when it knows
    // this node, and the payload's otherwise. P6-111 moved the child count off
    // the sidebar tree and onto `collection_view` to close a stale/failed-tree
    // gap (specs/collection-deletion.md Problem section); the fallback keeps
    // that closed while P6-127 makes the tree the fresher of the two for
    // exactly the two mutations that no longer refetch the payload — a create
    // (which adds a child) and a rename (which is the name the confirm and the
    // rename dialog both prefill from). Both reads happen at click time, off
    // the same `roots` the `forbidden` set is already built from, so they
    // cannot disagree with each other.
    let cards_here = i64::from(totals.present);
    let wants_here = i64::from(totals.desired);
    let payload_children = StoredValue::new(view.children.clone());
    let aim = Callback::new(move |()| {
        let roots = assembled_roots(tree.get_untracked().flatten());
        let node = find_tree_node(&roots, id);
        let subject = node
            .map(|n| n.row.summary.clone())
            .unwrap_or_else(|| collection.get_value());
        let children_here =
            node.map(|n| n.children.len())
                .unwrap_or_else(|| payload_children.with_value(Vec::len)) as i64;
        manage.menu_target.set(Some(MenuTarget::for_collection(
            &subject,
            &roots,
            cards_here + i64::from(here_delta.get_untracked()),
            wants_here,
            children_here,
        )));
    });

    view! {
        <div class="flex flex-col gap-3">
            <CollectionPath id name=name.clone() tree />

            <div class="flex flex-wrap items-start gap-3">
                <div class="min-w-0 flex-1">
                    <div class="flex flex-wrap items-center gap-2">
                        // Reactive, over a plain signal: a rename bumps only
                        // `TreeManage::revision` now, which this page's payload
                        // deliberately does not take as a source (P6-127), so
                        // the tree is what tells the title it changed. The
                        // payload's own name is the fallback and the SSR value,
                        // which is right there — on a fresh load the payload is
                        // as new as the tree.
                        <h1 class="text-2xl font-bold" data-testid="collection-title">
                            {move || {
                                tree_name(tree_facts, id)
                                    .unwrap_or_else(|| payload_name.get_value())
                            }}
                        </h1>
                        <Badge variant=BadgeVariant::Outline attr:data-testid="collection-kind">
                            {match kind {
                                CollectionKind::Deck => "Deck",
                                CollectionKind::Binder => "Binder",
                            }}
                        </Badge>
                        {format
                            .clone()
                            .map(|f| {
                                view! {
                                    <Badge
                                        variant=BadgeVariant::Secondary
                                        attr:data-testid="collection-format"
                                    >
                                        {f}
                                    </Badge>
                                }
                            })}
                    </div>
                    <p class="text-muted-foreground text-sm" data-testid="collection-counts">
                        {move || counts_summary(&totals, here_delta.get(), want_delta.get())}
                    </p>
                    // Which quick action leads here (spec: decks are Want-led,
                    // binders and Inbox Have-led). Stated in the header because
                    // the keystroke it names is a property of *this collection*,
                    // not of the panel the quick-add task will mount below it.
                    <p class="text-muted-foreground mt-1 text-xs" data-testid="add-default">
                        "Adding here: "
                        <kbd class="bg-muted rounded border px-1 py-0.5 font-mono">"⏎"</kbd>
                        {match add_default(kind) {
                            QuickAddKind::Want => " Want",
                            QuickAddKind::Have => " Have",
                        }}
                    </p>
                </div>
                // The frame's `Header Actions` — right end of the `Title Row`,
                // `gap: 8`. The kebab is the only thing in it in the wireframe;
                // `Empty deck…` joins it here because it was already a header
                // action and the frame draws a *binder*, so it says nothing about
                // where a deck's teardown goes. It stays a visible button rather
                // than moving into the menu: see [`HeaderKebab`] on why the
                // kebab's set is collection *lifecycle* only.
                <div class="flex shrink-0 items-center gap-2">
                    <Show when=move || kind == CollectionKind::Deck>
                        <Button
                            variant=ButtonVariant::Outline
                            attr:data-testid="teardown-open"
                            on:click=move |_| teardown_open.set(true)
                        >
                            "Empty deck…"
                        </Button>
                    </Show>
                    // A *second* `context_menu` instance, not a second menu: the
                    // panel is `TreeMenu`, rendered off the very same
                    // `menu_target` the sidebar's rows aim, so the two surfaces
                    // cannot drift on what the actions are. Only the instance id
                    // (and therefore the popover) differs.
                    <ContextMenu id="collection-header">
                        <HeaderKebab aim />
                        <TreeMenu />
                    </ContextMenu>
                </div>
            </div>

            {chip
                .map(|state| match state {
                    ChipState::Missing(text) => {
                        view! {
                            <a
                                href=format!("/my/collections/{id}/needs")
                                class="border-warning/40 bg-warning/10 text-warning-foreground hover:bg-warning/20 inline-flex w-fit items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium"
                                data-testid="needs-chip"
                            >
                                <span aria-hidden="true">"⚠"</span>
                                {text}
                            </a>
                        }
                            .into_any()
                    }
                    // Same shape, `success` tones instead of `warning`: still
                    // a link to `/needs`, which is what makes that page's
                    // "All set" empty state reachable at all (P6-143).
                    ChipState::Satisfied => {
                        view! {
                            <a
                                href=format!("/my/collections/{id}/needs")
                                class="border-success/40 bg-success/10 text-success-foreground hover:bg-success/20 inline-flex w-fit items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium"
                                data-testid="needs-chip-satisfied"
                            >
                                <span aria-hidden="true">"✓"</span>
                                "All needs met"
                            </a>
                        }
                            .into_any()
                    }
                })}

            {commanders
                .filter(|c| !c.commanders.is_empty())
                .map(|c| {
                    let identity = c.color_identity.join("");
                    view! {
                        <div
                            class="bg-card flex flex-wrap items-center gap-3 rounded-md border p-3"
                            data-testid="deck-commanders"
                        >
                            <span class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">
                                {if c.commanders.len() > 1 { "Commanders" } else { "Commander" }}
                            </span>
                            {c
                                .commanders
                                .iter()
                                .map(|card| {
                                    let href = format!("/cards/{}", card.oracle_id);
                                    let art = card.image_uri.clone();
                                    let label = card.name.clone();
                                    view! {
                                        <a
                                            href=href
                                            class="flex items-center gap-2 text-sm font-medium hover:underline"
                                            data-testid="deck-commander"
                                        >
                                            {art
                                                .map(|src| {
                                                    view! {
                                                        <img
                                                            src=src
                                                            alt=""
                                                            loading="lazy"
                                                            class="h-10 w-8 rounded-sm object-cover"
                                                        />
                                                    }
                                                })}
                                            {label}
                                        </a>
                                    }
                                })
                                .collect_view()}
                            {(!identity.is_empty())
                                .then(|| {
                                    view! {
                                        <Badge
                                            variant=BadgeVariant::Muted
                                            size=BadgeSize::Sm
                                            attr:data-testid="deck-color-identity"
                                        >
                                            {identity}
                                        </Badge>
                                    }
                                })}
                        </div>
                    }
                })}

            // Decks only: the dialog behind "Empty deck…".
            {(kind == CollectionKind::Deck)
                .then(|| {
                    view! { <TeardownDialog open=teardown_open collection_id=id view_res tree /> }
                })}
        </div>
    }
}

/// The collection header's `⋯` (`design/wireframes.pen` → `Header Kebab` on
/// *Desktop — Collection view*, `M Header Kebab` on *Mobile — Collection view*).
///
/// **The second designed home for tree management, and on a phone the natural
/// one** — the tree is behind a drawer there, so the header is where you already
/// are. It opens the *same* panel the sidebar's rows open, aimed at the
/// collection the route names, so the offered set is the tree row's set by
/// construction: `New binder inside…` / `New deck inside…`, then (withheld on the
/// Inbox, whose rename, delete **and** reparent the API all refuse with the same
/// `AND NOT is_inbox`) `Move to…` / `Rename…` / `Delete…`. Nothing is added and
/// nothing is dropped — a header kebab offering a different five actions than the
/// row for the same collection would be two contracts for one feature.
///
/// **What is deliberately *not* in it: `Empty deck…`.** The kebab's five actions
/// are all collection-*lifecycle* — they create, name, re-place or destroy the
/// node. Teardown moves the *cards inside* it and belongs with the other
/// content-level affordances on the page; that split is also where the code
/// already lives (`tree_manage` versus this file's `TeardownDialog`). It is a
/// deck's primary destructive action, the wireframe that draws this kebab draws a
/// binder and so cannot be read as relocating it, and burying a visible button in
/// a menu is a discoverability loss no frame asked for.
///
/// One button at both widths, styled by breakpoint: the frames put it in the same
/// structural slot (right end of the title row) and differ only in dress — a
/// 32 px bordered box on desktop, a bare 18 px glyph on the phone. Emitting one
/// element is also the rule this repo already follows for width-switched
/// surfaces, since SSR cannot know the viewport.
///
/// It is a real `<button>`, which is the whole keyboard and touch story: ⏎/space
/// opens the panel (`ContextMenuContent` then puts focus on the first item, ↑↓
/// rove, ESC closes and hands focus back), and a tap is just a click. The tree
/// needed a `⋯` invented for it precisely because a held touch produces no
/// `contextmenu` on the Android webview; here there was never anything else.
/// `pub(crate)` for the bench: `/my/*` is unreachable on the Android emulator
/// (the Tauri dev proxy strips Cookie headers), so a real-touch check of *this*
/// button has to happen on `/dev/components` or nowhere — the same reason the
/// My-cards root list is benched.
#[component]
pub(crate) fn HeaderKebab(
    /// Point the shared `menu_target` at this page's collection. Runs before the
    /// panel opens, so `TreeMenu` renders the right subject on its first pass.
    aim: Callback<()>,
) -> impl IntoView {
    // Called from *under* the `ContextMenu` provider — a call in the enclosing
    // component's body sits above it and resolves to `None`.
    let menu = use_context_menu();

    view! {
        <button
            type="button"
            data-testid="collection-actions"
            aria-haspopup="menu"
            // Not "Actions for {name}", the tree row's label: at `md` and up the
            // rail row for this very collection is on screen carrying exactly
            // that, and two buttons with one accessible name is an ambiguity for
            // whoever is listening. There is only ever one of these per page, and
            // the title it sits beside says which collection.
            aria-label="Collection actions"
            // `size-11` below `md` is the 44 px touch target — the frame's bare
            // 18 px glyph is the *look*, not the hit area. `md:border` +
            // `md:bg-background` is the frame's 32 px bordered box; the app has
            // no third text token, so `$text-3` and `$text-2` both land on
            // `text-muted-foreground`.
            class="text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-ring inline-flex size-11 shrink-0 items-center justify-center rounded-md text-lg leading-none focus-visible:ring-1 focus-visible:outline-none md:size-8 md:border md:bg-background md:text-base"
            on:click=move |ev| {
                ev.stop_propagation();
                aim.run(());
                if let Some(menu) = menu {
                    // The button's own rect, never `client_x/y`: a keyboard
                    // activation fires a click reporting 0,0, which would park
                    // the panel in the viewport corner, and a tap reports a point
                    // *inside* the button — the rect puts the panel in the same
                    // place either way.
                    let (x, y) = element_anchor(ev.as_ref()).unwrap_or((0.0, 0.0));
                    menu.open_at(x, y);
                }
            }
        >
            <span aria-hidden="true">"⋯"</span>
        </button>
    }
}

/// The desktop breadcrumb and the mobile back link — the two tree-derived parts
/// of the header, in their own boundary (see [`CollectionPage`] on why nothing
/// larger awaits the tree). Falls back to the collection's own name when the
/// tree does not know it yet.
#[component]
fn CollectionPath(
    id: Id,
    name: String,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
) -> impl IntoView {
    let name = StoredValue::new(name);
    view! {
        <Suspense fallback=|| {
            view! { <Skeleton class="h-4 w-48" /> }
        }>
            {move || Suspend::new(async move {
                let nodes = assembled_roots(tree.await);
                let crumbs = ancestor_path(&nodes, id)
                    .unwrap_or_else(|| {
                        vec![
                            Crumb {
                                id,
                                name: name.get_value(),
                            },
                        ]
                    });
                // Mobile drill-down: back walks *up the tree* (the IA's My-cards
                // tab is a drill-down, not a history stack), so the target is
                // the parent crumb — `/my` at the top level, where the tree's
                // roots sit. Its label is the *screen* you land on, and below
                // `md` that screen is now the root list titled "My cards", not
                // the All-cards table (which moved to `/my/all` for touch).
                let (back_href, back_label) = match crumbs.len() {
                    0 | 1 => ("/my".to_string(), "My cards".to_string()),
                    n => {
                        let parent = &crumbs[n - 2];
                        (format!("/my/collections/{}", parent.id), parent.name.clone())
                    }
                };
                view! {
                    <a
                        href=back_href
                        class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-sm md:hidden"
                        data-testid="collection-back"
                    >
                        <span aria-hidden="true">"‹"</span>
                        {back_label}
                    </a>
                    <Breadcrumb class="hidden md:block" {..} aria-label="Breadcrumb">
                        <BreadcrumbList>
                            <BreadcrumbItem>
                                <BreadcrumbLink attr:href="/my">"All cards"</BreadcrumbLink>
                            </BreadcrumbItem>
                            {crumbs
                                .iter()
                                .enumerate()
                                .map(|(i, crumb)| {
                                    let last = i + 1 == crumbs.len();
                                    let label = crumb.name.clone();
                                    let href = format!("/my/collections/{}", crumb.id);
                                    view! {
                                        <BreadcrumbSeparator />
                                        <BreadcrumbItem>
                                            {if last {
                                                view! { <BreadcrumbPage>{label}</BreadcrumbPage> }
                                                    .into_any()
                                            } else {
                                                view! {
                                                    <BreadcrumbLink attr:href=href>
                                                        {label}
                                                    </BreadcrumbLink>
                                                }
                                                    .into_any()
                                            }}
                                        </BreadcrumbItem>
                                    }
                                })
                                .collect_view()}
                        </BreadcrumbList>
                    </Breadcrumb>
                }
            })}
        </Suspense>
    }
}

// ------------------------------------------------------------------ rows ----

/// Nothing to show — and which of the three reasons it is matters.
#[component]
fn EmptyState(searching: bool, paged: Memo<bool>) -> impl IntoView {
    view! {
        <div class="text-muted-foreground py-12 text-center text-sm" data-testid="collection-empty">
            <Show
                when=move || paged.get()
                fallback=move || {
                    if searching {
                        view! { <p>"Nothing in here matches that search."</p> }.into_any()
                    } else {
                        view! {
                            <p>
                                "This collection is empty. "
                                <a href="/catalog" class="underline">
                                    "Browse the catalog"
                                </a> " to add cards."
                            </p>
                        }
                            .into_any()
                    }
                }
            >
                <p>"Nothing on this page."</p>
            </Show>
        </div>
    }
}

/// One card row plus the render decisions that need the whole page to make.
#[derive(Clone, PartialEq)]
struct ViewRow {
    row: CardRow,
    /// Whether this row prints the WANTED cell. `desired` is oracle-grained, so
    /// it repeats on every printing row of the same card and board
    /// (specs/collection-api.md: "the UI shows it once").
    show_wanted: bool,
    /// Copies of this card, on this board, held across **every** printing row
    /// in the payload — the same per-`(oracle, board)` sum [`section_slots`]
    /// folds.
    ///
    /// This is the **seed** for [`GroupLive::held`], which is what the two
    /// steppers actually read at commit time; nothing reads this field after
    /// [`group_live`] has folded it. A section's slot count is
    /// `max(held, desired)` per card, so both a HERE and a WANTED change move
    /// the header through [`section_slot_delta`] against the *group's* held
    /// total — the row's own `present` would understate it whenever the card
    /// is held under a second printing, and `max(·, desired)` is steeper the
    /// smaller `held` is, so a row-local stand-in mis-states the delta in both
    /// directions.
    held_in_group: i32,
}

/// Decide, in document order, which rows print WANTED, and fold each card's
/// per-`(oracle, board)` held total onto its rows.
fn view_rows(rows: Vec<CardRow>) -> Vec<ViewRow> {
    let mut held: HashMap<(Id, Board), i32> = HashMap::new();
    for row in &rows {
        *held.entry((row.oracle_id, row.board)).or_default() += row.present;
    }
    let mut seen: HashSet<(Id, Board)> = HashSet::new();
    rows.into_iter()
        .map(|row| {
            let key = (row.oracle_id, row.board);
            let first = seen.insert(key);
            ViewRow {
                show_wanted: first,
                held_in_group: held.get(&key).copied().unwrap_or(row.present),
                row,
            }
        })
        .collect()
}

/// The HERE column's own total for a row: what is here plus what its children
/// hold. Both halves are rendered (the rolled-up part dimmed), so this is the
/// number OWNED is compared against.
fn here_total(row: &CardRow) -> i32 {
    row.present + row.present_rollup
}

/// The WANTED cell, per the spec's "only when set and different".
fn wanted_cell(row: &ViewRow) -> Option<i32> {
    wanted_of(row.show_wanted, row.row.present, row.row.desired)
}

/// [`wanted_cell`]'s rule over an explicit `(present, desired)` pair, so the
/// grid can apply its [`RowDeltas`] overlay to `desired` and still reach the
/// same decision by the same rule rather than a second copy of it.
///
/// `present` stays the caller's business on purpose: `HoldingTile` passes the
/// **snapshot** present here even though its HERE badge shows the live one,
/// because the collapse test ("different") is what the table's own cell
/// evaluated once for this payload — overlaying it on the tile alone would
/// make the two layouts disagree about whether the number prints, which is the
/// one thing that tile's doc promises they cannot do.
fn wanted_of(show_wanted: bool, present: i32, desired: i32) -> Option<i32> {
    (show_wanted && desired > 0 && desired != present).then_some(desired)
}

/// The OWNED cell, per the spec's "collapses when equal to HERE". `owned` is
/// the global per-oracle total; when it is exactly what this collection's
/// subtree holds, printing it again says nothing.
fn owned_cell(row: &CardRow) -> Option<i32> {
    (row.owned != here_total(row)).then_some(row.owned)
}

/// Whether a card row's selection checkbox should be selectable right now
/// (P6-118, app-ui.md → Findings: "Removed rows stay selectable").
///
/// Pure over the two facts that decide it, so the truth table is checkable
/// without a reactive runtime; `CardTableRow` calls this from inside a
/// `style:display` closure reading the live `removed` signal, which is the
/// same one `HereCount`'s `remove`/`undo_removal` flip — a row cannot become
/// unselectable without that signal saying so, and Undo cannot restore it
/// without the same signal saying the opposite. Whether the checkbox mounts
/// **at all** is a separate, non-reactive decision (`CardTableRow`'s own
/// `present > 0`, static) — see that component's doc for why the reactive
/// half is a style toggle rather than a second mount/unmount.
fn row_selectable(removed: bool, present: i32) -> bool {
    !removed && present > 0
}

/// A deck section: one board's slice of one card type.
#[derive(Clone, PartialEq)]
struct DeckSection {
    label: String,
    /// Slots this section fills in the deck: copies present, plus the copies it
    /// wants and does not have. A decklist's `Creatures (28)` counts the
    /// intended 28 whether or not two of them are still on the shopping list.
    slots: i32,
    rows: Vec<ViewRow>,
}

/// The decklist type buckets, in the order decklists print them. `Other` catches
/// anything the catalog's type line doesn't place (tokens, schemes, a NULL).
const TYPE_ORDER: [(&str, &str); 9] = [
    ("Creature", "Creatures"),
    ("Planeswalker", "Planeswalkers"),
    ("Instant", "Instants"),
    ("Sorcery", "Sorceries"),
    ("Artifact", "Artifacts"),
    ("Enchantment", "Enchantments"),
    ("Battle", "Battles"),
    ("Land", "Lands"),
    ("", "Other"),
];

/// Which bucket a type line falls in — its index into [`TYPE_ORDER`].
///
/// First match wins, and the order is why: an *Artifact Creature* is a creature
/// on every decklist ever printed, and a `//`-joined double-faced type line
/// mentions both halves.
fn type_bucket(type_line: Option<&str>) -> usize {
    let line = type_line.unwrap_or("");
    TYPE_ORDER
        .iter()
        .position(|(needle, _)| !needle.is_empty() && line.contains(needle))
        .unwrap_or(TYPE_ORDER.len() - 1)
}

/// Board sections, in the order a deck reads.
const BOARD_ORDER: [(Board, &str); 3] = [
    (Board::Main, ""),
    (Board::Side, "Sideboard"),
    (Board::Maybe, "Maybeboard"),
];

/// The word a board is shown by, or `None` for the mainboard — which is shown
/// by *not* saying anything, the convention [`group_deck`] sets ("the
/// mainboard's sections are bare type names; other boards prefix theirs").
///
/// Shared with `/my/collections/:id/needs` (`super::needs`), which labels its
/// rows the same way now that `NeedRow` carries a board (P6-074): one source
/// for the vocabulary, so the two pages cannot call the same board different
/// things.
pub(crate) fn board_label(board: Board) -> Option<&'static str> {
    BOARD_ORDER
        .iter()
        .find(|(b, _)| *b == board)
        .map(|(_, prefix)| *prefix)
        .filter(|prefix| !prefix.is_empty())
}

/// Group a deck's page into `(board, type)` sections, dropping empties.
///
/// The mainboard's sections are bare type names; other boards prefix theirs, so
/// a sideboard instant is never mistaken for a maindeck one — the DTO carries
/// `board` and nothing else in the row would show it.
fn group_deck(rows: Vec<ViewRow>) -> Vec<DeckSection> {
    let mut out = Vec::new();
    for (board, prefix) in BOARD_ORDER {
        for (bucket, (_, label)) in TYPE_ORDER.iter().enumerate() {
            let picked: Vec<ViewRow> = rows
                .iter()
                .filter(|r| {
                    r.row.board == board && type_bucket(r.row.type_line.as_deref()) == bucket
                })
                .cloned()
                .collect();
            if picked.is_empty() {
                continue;
            }
            out.push(DeckSection {
                label: if prefix.is_empty() {
                    (*label).to_string()
                } else {
                    format!("{prefix} · {label}")
                },
                slots: section_slots(&picked),
                rows: picked,
            });
        }
    }
    out
}

/// Copies present in a section plus the copies it wants and lacks — counted
/// once per card, since `desired` repeats across a card's printing rows.
fn section_slots(rows: &[ViewRow]) -> i32 {
    let mut slots: i32 = rows.iter().map(|r| r.row.present).sum();
    let mut counted: HashSet<(Id, Board)> = HashSet::new();
    for r in rows {
        if r.row.desired <= 0 || !counted.insert((r.row.oracle_id, r.row.board)) {
            continue;
        }
        let held: i32 = rows
            .iter()
            .filter(|o| o.row.oracle_id == r.row.oracle_id && o.row.board == r.row.board)
            .map(|o| o.row.present)
            .sum();
        slots += (r.row.desired - held).max(0);
    }
    slots
}

/// A deck section header's live count (P6-118, app-ui.md → Findings:
/// "Section header contradiction"): `slots` as computed once at payload load
/// by [`section_slots`], adjusted by this section's own running delta. The
/// delta itself must already be in **slots**, not copies — see
/// [`section_slot_delta`], the only thing that should ever be pushed into it.
fn section_slots_live(slots: i32, delta: i32) -> i32 {
    slots + delta
}

/// The section header's slot-count delta when one of the two numbers behind a
/// card's slot contribution moves from `old` to `new`, with the other held at
/// `fixed` (P6-118 review round 1 — the first cut of this fix pushed the raw
/// *copy* delta and was wrong).
///
/// **Symmetric, and used from both steppers.** The contribution is
/// `max(held, desired)`, so HERE passes `(held_before, held_after, desired)`
/// and WANTED passes `(desired_before, desired_after, held)` — one function,
/// the moving side first and the fixed side last, rather than two derivations
/// that could drift.
///
/// `section_slots` is not a copy count: per `(oracle, board)` it is
/// `held + max(desired - held, 0)`, which is exactly `max(held, desired)`
/// (`section_slots_count_a_split_card_once`'s "desire 4, three held → 4
/// slots" pins this). A raw copy delta corrupts that the moment the row is
/// wanted and under-held — the *ordinary* deck row, since decks are
/// Want-led: a 4-held/4-desired row stepped to 2 must leave the header at 4
/// (2 held + 2 still lacking, same total), not push it to 2 — a refetch
/// would snap the header straight back. (The WANTED cell reads 4 only in
/// the under-held variant; at 4-held/4-desired it collapses to "—".) Only
/// when `desired` is 0 does the slot delta reduce to the copy delta this
/// replaces.
///
/// **Both arguments must come from [`GroupLive`], never from a component's
/// construction-time copy.** Slot arithmetic is `(oracle, board)`-grained, and
/// each of the two numbers has a stepper that can move it — so a caller that
/// froze either one computes its delta against a value that may already be
/// stale, and the header then stays wrong until a reload rather than merely
/// approximating. That was the want-stepper round-1 MAJOR; see [`GroupLive`]
/// for the repro and the contract that replaced it. `GroupLive` is
/// group-scoped for the same reason `section_slots` re-sums the whole group:
/// a card held under two printings has two rows whose commits both move the
/// one `held` this function is asking about.
fn section_slot_delta(old: i32, new: i32, fixed: i32) -> i32 {
    new.max(fixed) - old.max(fixed)
}

/// One `(oracle, board)` group's **live** held and desired counts — the two
/// numbers [`section_slot_delta`] needs, each of which one of the row's two
/// steppers can move.
///
/// This exists because both numbers were previously captured as plain `i32`s
/// at each stepper's construction (want-stepper review round 1, MAJOR): each
/// stepper then computed its slot delta against the *other* column's
/// pre-edit snapshot, so two edits in one row composed wrongly and stayed
/// wrong until a reload. The reported repro: held 2 / desired 4, WANTED
/// stepped 4 → 1 pushes `max(2,1) − max(2,4) = −2` (right, the header should
/// read 2); HERE then stepped 2 → 3 pushed `max(3,4) − max(2,4) = 0` against
/// a `desired` that was 4 when the component mounted and is 1 by now, leaving
/// the header at 2 where the truth is 3.
///
/// The contract each stepper follows: **read the other field untracked at
/// commit time, then write your own.** Neither is ever read to render, which
/// is why plain `RwSignal`s are enough and no `Memo` is involved.
///
/// **Group-scoped, not row-scoped.** `desired` is oracle-grained and `held` is
/// the sum over a card's printing rows ([`ViewRow::held_in_group`]), so a card
/// held under two printings in one section has two `CardTableRow`s that must
/// agree about both numbers — per-row copies would drift the moment the second
/// row committed. [`group_live`] builds one of these per `(oracle, board)` and
/// hands the same copy to every row in it, which also makes the HERE path
/// exact where it used to be a documented under-shooting approximation.
#[derive(Clone, Copy)]
struct GroupLive {
    /// Copies of this card held on this board, across every printing row of
    /// the group — seeded from [`ViewRow::held_in_group`].
    held: RwSignal<i32>,
    /// This card's desired count on this board — seeded from
    /// [`shared::CardRow::desired`], which is already this exact grain.
    desired: RwSignal<i32>,
}

/// One [`GroupLive`] per `(oracle, board)`, seeded from the payload the table
/// is about to render. Built once by `CollectionTable` and looked up per row,
/// so every row of a group shares one pair of signals — see [`GroupLive`].
fn group_live(rows: &[ViewRow]) -> HashMap<(Id, Board), GroupLive> {
    let mut out: HashMap<(Id, Board), GroupLive> = HashMap::new();
    for row in rows {
        out.entry((row.row.oracle_id, row.row.board))
            .or_insert_with(|| GroupLive {
                held: RwSignal::new(row.held_in_group),
                desired: RwSignal::new(row.row.desired),
            });
    }
    out
}

/// A row's live HERE overlay on top of the snapshot's own `present` — the
/// grid's counterpart to `here_delta`/`section_delta`, and the fix for a
/// review finding (grid-toggle task round 2): `CollectionBody` freezes `view:
/// CollectionView` into a `StoredValue` the moment it resolves, and a stepper
/// commit deliberately never refetches it (see the module doc, "A commit does
/// not refetch the view"). `CardTableRow`'s stepper tolerates that because it
/// owns its own live displayed value; `HoldingTile` has nothing of the kind
/// (the count stepper is list-only by design), so a HERE edit made in list
/// mode was invisible on the tile after switching to grid — stale at best (a
/// tile reading 2 when the header already says 3) and a **ghost** at worst (a
/// tile still rendering for a holding a commit-to-zero just deleted).
///
/// Pure, so the "no edits yet" and "clamped at zero" edges are checkable
/// without a reactive runtime — see [`RowDeltas`] for the signal this backs.
fn live_present(snapshot: i32, delta: i32) -> i32 {
    (snapshot + delta).max(0)
}

/// Live per-row HERE **and WANTED** deltas since the currently rendered `view`
/// snapshot — keyed by `holding_id` and `desire_id` respectively, the only
/// identifiers a stepper commit and a `HoldingTile` read can agree name the
/// same row without a second read of `view_res` (see [`live_present`]'s doc
/// for why this exists at all).
///
/// **The wants half is the same fix for the same defect** (want-stepper review
/// round 1, MAJOR): once WANTED became steppable in the table, editing a want
/// in list mode and toggling to grid showed the pre-edit count, and zeroing a
/// want-only card's last desire left a **ghost tile** badged `1 wanted` for a
/// `desires` row that no longer exists — exactly the HERE defect this struct
/// was built for, reintroduced in the other column. Two maps rather than one,
/// because the two columns are keyed by different ids and a row can be
/// editable in one, the other, both, or neither.
///
/// **Write side:** every `HereCount` whose row carries a `holding_id` pushes
/// its commit's `(to - from)` here (and every `WantedCount` with a
/// `desire_id` pushes its own), in *addition* to (never instead of) the
/// existing `here_delta`/`section_delta` pushes — this is a third, parallel
/// aggregate, not a replacement. Optimistic writes, server-error rollbacks and
/// Undo all already arrive as a plain `StepperCommit`, so the same
/// accumulate-the-difference trick `here_delta` already relies on nets every
/// one of those out correctly with no special-casing here — including a
/// commit-to-zero (removal): `push`ing its `(0 - present)` and a later Undo's
/// `(copies - 0)` sum back to the original value, and a mid-flight Undo
/// rewires the *stepper's own* `holding_id` signal to the id the server
/// re-inserted under (see `HaveStepper::undo_removal`) but never touches the
/// `holding_id` this component captured as a plain prop at construction — so
/// both halves of a remove/undo pair key against the same value regardless of
/// what the database now calls the row.
///
/// **Read side:** `CollectionGrid` is a fresh component instance every time
/// the switch is clicked into grid (`CollectionBody`'s table/grid branch is
/// itself the reactive boundary — see its own doc), and nothing can edit HERE
/// while grid is showing (no stepper there), so one **untracked** read at
/// `CollectionGrid`'s construction is both correct and sufficient — no
/// `Memo`, no per-tile subscription needed. `CollectionGrid` uses it three
/// ways: [`live_present`] for what a surviving tile's HERE badge shows, the
/// same overlay applied to `desired` for its WANTED badge, and both together
/// to decide whether the row still has a tile at all (the ghost tile case —
/// see [`row_survives`]).
///
/// **Reset:** zeroed alongside `here_delta` the moment a fresh payload lands
/// (`CollectionPage`'s header `Transition`) — a new `view` already contains
/// everything committed before it, the same reasoning `here_delta.set(0)`'s
/// own comment gives.
#[derive(Clone, Copy)]
struct RowDeltas {
    /// Keyed by `holding_id` — HERE.
    present: RwSignal<HashMap<Id, i32>>,
    /// Keyed by `desire_id` — WANTED.
    desired: RwSignal<HashMap<Id, i32>>,
}

impl RowDeltas {
    fn new() -> Self {
        Self {
            present: RwSignal::new(HashMap::new()),
            desired: RwSignal::new(HashMap::new()),
        }
    }

    fn reset(self) {
        self.present.update(|m| m.clear());
        self.desired.update(|m| m.clear());
    }

    fn push_present(self, holding_id: Id, delta: i32) {
        self.present
            .update(|m| accumulate_row_delta(m, holding_id, delta));
    }

    fn push_desired(self, desire_id: Id, delta: i32) {
        self.desired
            .update(|m| accumulate_row_delta(m, desire_id, delta));
    }

    /// The overlaid HERE count for one row. `holding_id: None` (a multi-grain
    /// cell, never edited via the stepper) always reads the snapshot as-is.
    /// **Untracked** — see the struct doc's "Read side".
    fn live_present(self, holding_id: Option<Id>, snapshot: i32) -> i32 {
        Self::overlay(self.present, holding_id, snapshot)
    }

    /// WANTED's counterpart, keyed by `desire_id` — same rule, same clamp: a
    /// cell no stepper could address reads its snapshot unchanged.
    fn live_desired(self, desire_id: Option<Id>, snapshot: i32) -> i32 {
        Self::overlay(self.desired, desire_id, snapshot)
    }

    fn overlay(map: RwSignal<HashMap<Id, i32>>, key: Option<Id>, snapshot: i32) -> i32 {
        match key {
            Some(id) => {
                let delta = map.with_untracked(|m| m.get(&id).copied().unwrap_or(0));
                live_present(snapshot, delta)
            }
            None => snapshot,
        }
    }
}

/// Whether a row still deserves a grid tile, given both live counts and
/// whether either column was ever stepper-addressable.
///
/// The rule the wants overlay forced (want-stepper review round 1): a row that
/// no stepper could reach in *either* column cannot have been zeroed in this
/// session, so it always survives — that was the old
/// `holding_id.is_none() => keep` shortcut, which stopped being true the
/// moment a want-only row (`holding_id: None`, `desire_id: Some`) became
/// editable, leaving a ghost tile behind every zeroed want. An addressable row
/// survives while it still has copies here **or** copies wanted here; a
/// held-and-wanted row whose HERE is stepped to zero therefore stays as a
/// want-only tile, which is what the table's own row does and what the server
/// still holds a `desires` row for.
fn row_survives(editable: bool, live_present: i32, live_desired: i32) -> bool {
    !editable || live_present > 0 || live_desired > 0
}

/// The map mutation `RowDeltas::push` performs, free of the signal so it is
/// testable without a reactive runtime — the same pattern
/// `selection_tray.rs`'s `toggle_in`/`retain_untokened` establish. A repeated
/// commit against the same `holding_id` (an edit, then another edit; a
/// removal, then its Undo) accumulates rather than overwrites, which is what
/// lets the removal/Undo pair net back to zero — see [`RowDeltas`]'s doc.
fn accumulate_row_delta(map: &mut HashMap<Id, i32>, holding_id: Id, delta: i32) {
    *map.entry(holding_id).or_insert(0) += delta;
}

/// The table, or the empty state instead of it — and the one reactive decision
/// between them (P6-127).
///
/// The folder rows are tree-derived now, so a `New binder inside…` can put the
/// **first** row into a collection whose payload still says it is empty. That
/// makes "is there anything to show" a live question rather than a property of
/// the payload, and the answer has to be able to flip without a refetch.
///
/// It is a [`Memo`] and not a raw read on purpose: this closure rebuilds the
/// card table when it re-runs, which is the thing this whole task exists to
/// stop happening on a tree mutation. Deduped to the *boolean*, a rename of a
/// child collection moves the folder list without touching the cards, and only
/// a genuine empty→non-empty flip (whose non-empty branch has no card rows in
/// it anyway, since `cards_empty` is what got us here) rebuilds anything.
#[component]
fn CollectionBody(
    view: CollectionView,
    /// A quick search is running: child collections step aside for it (they are
    /// not what you typed a card name to find), so the empty state's question
    /// is about the cards alone.
    searching: bool,
    paged: Memo<bool>,
    here_delta: RwSignal<i32>,
    /// `here_delta`'s wants twin, threaded to the table alone: the WANTED cell
    /// is steppable in list view only, exactly as HERE is. The grid shows what
    /// those edits did — through `row_deltas`, not through this — because it
    /// renders badges, not controls (see [`CollectionGrid`]).
    want_delta: RwSignal<i32>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
    tree_facts: RwSignal<Option<TreeFacts>>,
    /// Table or grid — read *inside* the closure below, not baked at this
    /// component's construction, so a pure view-switch click flips the layout
    /// without needing `view_res` (the resource that resolved `view`) to
    /// re-await. Same split `crate::my::all_cards::AllCardsBody` uses; see
    /// its comment for the fuller account of why.
    list_view: Memo<bool>,
    /// `here_delta`'s per-row twin, threaded to both branches: `CollectionTable`
    /// writes it (every `HereCount` commit), `CollectionGrid` reads it (once,
    /// on entry — see [`RowDeltas`]'s doc). Owned by `CollectionPage`, not
    /// here, for the same reason `here_delta` is: it must be zeroed by the
    /// same payload that puts fresh totals on screen, not by whichever branch
    /// happens to be showing when a new one lands.
    row_deltas: RowDeltas,
) -> impl IntoView {
    let cards_empty = view.cards.is_empty();
    let folders = folder_list(
        view.collection.id,
        view.children.clone(),
        searching,
        tree_facts,
    );
    let no_folders = Memo::new(move |_| folders.read().is_empty());
    let view = StoredValue::new(view);
    move || {
        if cards_empty && no_folders.get() {
            view! { <EmptyState searching paged /> }.into_any()
        } else if list_view.get() {
            view! {
                <CollectionTable
                    view=view.get_value()
                    folders
                    here_delta
                    want_delta
                    tree
                    row_deltas
                />
            }
            .into_any()
        } else {
            view! { <CollectionGrid view=view.get_value() folders tree row_deltas /> }.into_any()
        }
    }
}

/// The folder rows for a rendered payload: the tree's children of this
/// collection, falling back to the payload's own (see [`folder_rows`]), and
/// empty while a search is running.
///
/// A `Memo` so the rows survive a tree refetch that did not change them —
/// **every stepper commit fires one** to keep the sidebar badges honest, and
/// without the equality gate each would rebuild this row set for nothing.
fn folder_list(
    collection_id: Id,
    payload_children: Vec<shared::CollectionSummary>,
    searching: bool,
    tree_facts: RwSignal<Option<TreeFacts>>,
) -> Memo<Vec<shared::CollectionSummary>> {
    let payload_children = StoredValue::new(payload_children);
    Memo::new(move |_| {
        if searching {
            return Vec::new();
        }
        tree_facts.with(|f| {
            payload_children.with_value(|fallback| folder_rows(f.as_ref(), collection_id, fallback))
        })
    })
}

#[component]
fn CollectionTable(
    view: CollectionView,
    folders: Memo<Vec<shared::CollectionSummary>>,
    here_delta: RwSignal<i32>,
    /// `here_delta`'s wants twin — see [`CollectionBody`].
    want_delta: RwSignal<i32>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
    row_deltas: RowDeltas,
) -> impl IntoView {
    let is_deck = view.collection.kind == CollectionKind::Deck;
    let collection_id = view.collection.id;
    let rows = view_rows(view.cards);
    // One live `(held, desired)` pair per `(oracle, board)`, shared by every
    // row of the group — see [`GroupLive`] for the composition defect this
    // fixes. Built from the whole page's rows before `group_deck` splits them,
    // since a group's rows all land in one section anyway (the bucket is a
    // property of the card).
    let group_live = group_live(&rows);
    let sections = if is_deck {
        group_deck(rows.clone())
    } else {
        Vec::new()
    };
    // The fallback can only fire if a row reached the view without passing
    // through `group_live` above, which it cannot — it is here so a lookup miss
    // degrades to the old per-row behavior instead of panicking.
    let live_for = move |row: &ViewRow| -> GroupLive {
        group_live
            .get(&(row.row.oracle_id, row.row.board))
            .copied()
            .unwrap_or_else(|| GroupLive {
                held: RwSignal::new(row.held_in_group),
                desired: RwSignal::new(row.row.desired),
            })
    };

    view! {
        <TableWrapper class="max-h-none">
            <Table {..} data-testid="collection-table">
                <TableHeader>
                    <TableRow>
                        // `w-11` below `md` is the select control's 44 px touch
                        // target (see `SelectionCheckbox`); the 12 px it costs
                        // the row is paid for by `px-1` on HERE / WANTED /
                        // OWNED, the trade `/my/all` already makes. Both
                        // switch at `md` — pairing the compensation with `sm`
                        // instead left 640–767 px carrying the wide column
                        // with nothing paying for it.
                        <TableHead class="w-11 md:w-8">
                            <span class="sr-only">"Select"</span>
                        </TableHead>
                        <TableHead>"Card"</TableHead>
                        // `lg`, not `md` (P6-001): Type's own text is
                        // untruncated (a truncating `max-w` measurably *forced*
                        // the column to that width under `table-layout: auto`
                        // instead of merely capping it — tried and reverted,
                        // see specs/app-ui.md's "Table overflow re-measured
                        // and fixed"). Its longest word sets the column's
                        // intrinsic min-width, which on decks with long type
                        // lines was enough to overflow the wrapper right at
                        // `md` (768px) — the same width where the select
                        // column's `w-8` shrink and the HERE/WANTED/OWNED
                        // `md:px-2` bump both land, so the freed space from
                        // one and the cost of the other already left no slack
                        // for a seventh column. `lg` (1024px) measured 0
                        // overflow with Type back on every seeded collection;
                        // `md` did not.
                        <TableHead class="hidden lg:table-cell">"Type"</TableHead>
                        <TableHead class="hidden sm:table-cell">"Mana"</TableHead>
                        <TableHead class="px-1 text-right md:px-2">"Here"</TableHead>
                        // Abbreviated below `sm` — see the matching comment
                        // on `AllCardsTablePage`'s header in `my/all_cards.rs`
                        // (P6-001).
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
                    // Child collections first — the wireframe's folder rows, in
                    // the same table and the same columns as the cards. Their
                    // own closure, reading a memo the tree feeds: a rename or a
                    // `New binder inside…` relabels or adds a row here without
                    // touching a single card row below (P6-127).
                    {move || {
                        folders
                            .get()
                            .into_iter()
                            .map(|folder| view! { <FolderTableRow folder tree /> })
                            .collect_view()
                    }}
                    {if is_deck {
                        sections
                            .into_iter()
                            .map(|section| {
                                let label_attr = section.label.clone();
                                let label = section.label.clone();
                                let slots = section.slots;
                                // This section's own present-copy delta — the
                                // section-scoped twin of `here_delta`. Created
                                // fresh here rather than zeroed by an Effect:
                                // this whole branch is rebuilt every time
                                // `view_res` resolves (see `CollectionPage`'s
                                // module doc on why a commit never refetches
                                // it), so a new payload already starts every
                                // section back at zero.
                                let section_delta = RwSignal::new(0);
                                view! {
                                    <TableRow {..} data-testid="deck-section">
                                        <TableCell
                                            class="text-muted-foreground bg-muted/40 p-2 text-xs font-semibold tracking-wide uppercase"
                                            {..}
                                            colspan="7"
                                            data-section=label_attr
                                        >
                                            {label} " · "
                                            {move || {
                                                section_slots_live(slots, section_delta.get())
                                                    .to_string()
                                            }}
                                        </TableCell>
                                    </TableRow>
                                    {section
                                        .rows
                                        .into_iter()
                                        .map(|row| {
                                            let live = live_for(&row);
                                            view! {
                                                <CardTableRow
                                                    row
                                                    here_delta
                                                    want_delta
                                                    collection_id
                                                    section_delta=section_delta
                                                    row_deltas
                                                    live
                                                />
                                            }
                                        })
                                        .collect_view()}
                                }
                            })
                            .collect_view()
                            .into_any()
                    } else {
                        rows.into_iter()
                            .map(|row| {
                                let live = live_for(&row);
                                view! {
                                    <CardTableRow
                                        row
                                        here_delta
                                        want_delta
                                        collection_id
                                        row_deltas
                                        live
                                    />
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </TableBody>
            </Table>
        </TableWrapper>
    }
}

/// A child collection, rendered in the card table above the cards — the
/// wireframe's folder row, sharing the three numeric columns.
///
/// Its count is the *rolled-up* one (own + every descendant's), read from the
/// shell's collection tree so it is the same number the sidebar badge shows.
/// The read is its own `Suspense` rather than the table's: a stepper commit
/// refetches that tree, and re-rendering the whole table would re-seed every
/// stepper (see [`CollectionPage`]). Which rows exist comes from that same tree
/// (via [`TreeFacts`]) rather than the card payload, for the same reason from
/// the other direction — see [`folder_rows`].
#[component]
fn FolderTableRow(
    folder: shared::CollectionSummary,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
) -> impl IntoView {
    let id = folder.id;
    view! {
        <TableRow {..} data-testid="folder-row" data-collection=id.to_string()>
            // No checkbox: the selection tray moves cards, not collections.
            // Padding still tracks the card rows' select cell, or the two row
            // kinds would size the column differently.
            <TableCell class="p-0 md:p-2">""</TableCell>
            // `max-w-0 w-full` (P6-020): a child's `truncate` alone doesn't
            // cap this column's own auto-layout min-content width — see the
            // matching comment on `all_cards.rs`'s WHERE cell and
            // specs/app-ui.md's P6-001 section. Folder names are
            // user-chosen, same risk as a WHERE-cell collection name.
            <TableCell class="max-w-0 w-full p-2">
                <a
                    href=format!("/my/collections/{id}")
                    class="flex items-center gap-2 font-medium hover:underline"
                >
                    <span aria-hidden="true" class="shrink-0">
                        {if folder.kind == CollectionKind::Deck { "🃏" } else { "📁" }}
                    </span>
                    <span class="min-w-0 truncate" title=folder.name.clone()>
                        {folder.name.clone()}
                    </span>
                </a>
            </TableCell>
            <TableCell class="hidden p-2 lg:table-cell">""</TableCell>
            <TableCell class="hidden p-2 sm:table-cell">""</TableCell>
            // Italic + dimmed: these copies are *there*, not here.
            <TableCell
                class="text-muted-foreground px-1 py-2 text-right italic tabular-nums md:px-2"
                {..}
                data-testid="here-count"
            >
                <Suspense fallback=|| {
                    view! { <span></span> }
                }>
                    {move || Suspend::new(async move {
                        rolled_up_of(&assembled_roots(tree.await), id)
                            .map(|n| n.to_string())
                            .unwrap_or_default()
                    })}
                </Suspense>
            </TableCell>
            <TableCell class="px-1 py-2 text-right md:px-2">""</TableCell>
            <TableCell class="px-1 py-2 text-right md:px-2">""</TableCell>
        </TableRow>
    }
}

#[component]
fn CardTableRow(
    row: ViewRow,
    here_delta: RwSignal<i32>,
    /// `here_delta`'s wants twin — threaded straight through to
    /// [`WantedCount`], the only thing that writes it.
    want_delta: RwSignal<i32>,
    collection_id: Id,
    /// This row's deck section, when it has one — threaded straight through to
    /// [`HereCount`] so a section header can react to this row's own commits
    /// and removals the same way the page header already reacts via
    /// `here_delta`. `None` in a binder, which has no sections.
    #[prop(optional)]
    section_delta: Option<RwSignal<i32>>,
    /// `here_delta`'s per-row twin — see [`RowDeltas`]'s doc. Threaded through
    /// to both steppers: `HereCount` writes its `holding_id` half, `WantedCount`
    /// its `desire_id` half.
    row_deltas: RowDeltas,
    /// This row's `(oracle, board)` group's live held/desired pair, shared with
    /// every other row of the same group — see [`GroupLive`]. Both steppers
    /// need it: each reads the *other* field at commit time to compute its
    /// section-slot delta, and writes its own afterward.
    live: GroupLive,
) -> impl IntoView {
    let wanted = wanted_cell(&row);
    let owned = owned_cell(&row.row);
    let CardRow {
        oracle_id,
        printing_id,
        name,
        image_uri,
        mana_cost,
        type_line,
        present,
        owned: owned_total,
        present_rollup,
        board,
        holding_id,
        desire_id,
        faces,
        ..
    } = row.row;

    // Owned here, not inside `HereCount` (which used to create it): the
    // checkbox below has to withdraw the instant a removal flips it, not only
    // once an unrelated refetch remounts the row (P6-118, app-ui.md →
    // Findings, "Removed rows stay selectable").
    let removed = RwSignal::new(false);

    // Selectable only where copies are actually here to move: a desire-only row
    // (`present == 0`) holds nothing, and a rolled-up count belongs to a child
    // collection, not to this one. This is the grain-complete surface — the
    // key names the collection, the printing *and* the board.
    //
    // Whether a checkbox exists **at all** is still decided once, from the
    // payload's own `present` — unchanged from before P6-118. What's new is
    // the *reactive* half below: once mounted, its visibility tracks
    // `removed`. An *existing* selection on a row that gets removed is
    // deliberately left alone here (not force-cleared): the tray already has
    // a name and a toast for a selection that outlived its copies
    // (`SkipReason::NoCopies`, `move_selection.rs`), and "the stepper" is
    // named there explicitly as one of the causes — this is that case
    // reaching the mechanism that was already built for it, not a new one.
    let selection = use_selection();
    let key = SelectionKey::Held {
        collection_id,
        printing_id,
        board,
    };
    let selected = selection.selected(key);
    let selectable_card = (present > 0).then(|| SelectedCard {
        key,
        oracle_id,
        name: name.clone(),
        image_uri: image_uri.clone(),
    });

    // The same preview the catalog and `/my` rows use — hover card, touch
    // sheet, DFC flip — built from this row rather than refetched. The faces
    // are *this printing's*, so a flip shows the copy you hold.
    let preview = CardSummary {
        oracle_id,
        name: name.clone(),
        printing_id: Some(printing_id),
        image_uri,
        mana_cost: mana_cost.clone(),
        type_line: type_line.clone(),
        owned: Some(owned_total),
        faces,
    };
    let link_name = name.clone();
    // Cloned out here rather than at the call site: the HERE cell's own
    // `<div>` closure captures `name`, so a second `name.clone()` further down
    // the same `view!` would be a use-after-move.
    let want_name = name.clone();
    let type_line_text = type_line.unwrap_or_default();

    view! {
        <TableRow
            class="group/row"
            {..}
            data-testid="collection-row"
            data-oracle=oracle_id.to_string()
            data-printing=printing_id.to_string()
            data-board=board.to_pg()
            data-state=move || selected.get().then_some("selected")
        >
            // `p-0` below `md` so the 44 px select target *is* the column
            // rather than 44 px plus 16 px of cell padding (`SelectionCheckbox`).
            <TableCell class="p-0 md:p-2">
                {selectable_card
                    .map(|card| {
                        view! {
                            // A `style:display` toggle, not a mount/unmount:
                            // `HereCount`'s own `<Show>` already disposes the
                            // count stepper's reactive scope on this same
                            // `removed` flip, and `count_stepper.rs` carries a
                            // documented, pre-existing disposal race in its
                            // deferred blur-commit handling (app-ui.md →
                            // Findings, P6-117's "genuine, pre-existing,
                            // unrelated wasm panic"). This task's first cut
                            // used a second structural mount/unmount here
                            // instead and a naive repro script made that race
                            // look far worse under it — but a real A/B test
                            // (the actual e2e test, several runs each way)
                            // measured comparable failure rates with and
                            // without it, so the script was a poor proxy, not
                            // evidence of a regression (app-ui.md →
                            // Findings). Kept as a `style:display` toggle
                            // anyway: patching one style property is the more
                            // defensible choice over tearing down and
                            // rebuilding a component subtree, even though it
                            // does not measurably change the pre-existing
                            // rate. `contents` rather than a base style, so
                            // the wrapper adds no box of its own for
                            // `SelectionCheckbox`'s own sizing to fight.
                            <span style:display=move || {
                                if row_selectable(removed.get(), present) {
                                    "contents"
                                } else {
                                    "none"
                                }
                            }>
                                <SelectionCheckbox selection card />
                            </span>
                        }
                    })}
            </TableCell>
            <TableCell class="p-2">
                <CardPreview card=preview>
                    <a href=format!("/cards/{oracle_id}") class="font-medium hover:underline">
                        {link_name}
                    </a>
                </CardPreview>
            </TableCell>
            <TableCell class="text-muted-foreground hidden p-2 lg:table-cell">
                {type_line_text}
            </TableCell>
            <TableCell class="text-muted-foreground hidden p-2 sm:table-cell">
                {mana_cost.unwrap_or_default()}
            </TableCell>
            <TableCell
                class="px-1 py-2 text-right tabular-nums md:px-2"
                {..}
                data-testid="here-cell"
            >
                <div class="flex items-center justify-end gap-1">
                    <HereCount
                        name=name.clone()
                        present
                        holding_id
                        here_delta
                        section_delta
                        row_deltas
                        live
                        removed
                    />
                    // Italic + dimmed, per the spec: copies a child collection
                    // holds are *here* only in the rolled-up sense.
                    {(present_rollup > 0)
                        .then(|| {
                            view! {
                                <span
                                    class="text-muted-foreground text-xs italic"
                                    data-testid="here-rollup"
                                    title="held in nested collections"
                                >
                                    {format!("+{present_rollup}")}
                                </span>
                            }
                        })}
                </div>
            </TableCell>
            <TableCell
                class="px-1 py-2 text-right tabular-nums md:px-2"
                {..}
                data-testid="wanted-count"
            >
                <WantedCount
                    name=want_name
                    wanted
                    desire_id
                    want_delta
                    section_delta
                    row_deltas
                    live
                />
            </TableCell>
            <TableCell
                class="px-1 py-2 text-right tabular-nums md:px-2"
                {..}
                data-testid="owned-count"
            >
                {owned.map(|n| n.to_string()).unwrap_or_else(|| "—".to_string())}
            </TableCell>
        </TableRow>
    }
}

/// The HERE number: the in-place stepper where the cell is addressable, plain
/// text where it isn't.
///
/// The write semantics (optimistic set, commit-to-zero routed through
/// `remove_holding` so it stays undoable, the multi-grain refusal) live in
/// [`crate::components::holding_stepper::HaveStepper`] now — lifted there so
/// `/cards/:id`'s ownership stepper can reuse them verbatim instead of
/// re-deriving them (the card-quantities-on-detail-page task). This wrapper
/// is what stays page-specific: turning `HaveStepper`'s generic `(from, to)`
/// into *this* page's two aggregates (`here_delta`, and `section_delta` where
/// the row has one), and refetching the sidebar tree — plus, only after an
/// Undo settles, this page's own `HoldingsRevision`-driven refetch (safe only
/// there; see `HaveStepper::on_undo_settled`'s doc for why every other path
/// avoids it).
#[component]
fn HereCount(
    name: String,
    present: i32,
    holding_id: Option<Id>,
    here_delta: RwSignal<i32>,
    /// This row's deck section delta, when it has one (`None` in a binder) —
    /// see [`CardTableRow`]. Not `#[prop(optional)]`: that sugar unwraps a
    /// bare `T` into `Some(T)` for the caller, which is wrong here — the
    /// caller (`CardTableRow`) already holds an `Option<RwSignal<i32>>` of
    /// its own and is forwarding it as-is, not deciding presence itself.
    /// Every push into it must go through [`section_slot_delta`], never a raw
    /// copy delta (P6-118 review round 1) — see that function's doc.
    section_delta: Option<RwSignal<i32>>,
    /// `here_delta`'s per-row twin, keyed by `holding_id` — see
    /// [`RowDeltas`]'s doc for why the grid needs this and the table's own
    /// numbers do not.
    row_deltas: RowDeltas,
    /// This `(oracle, board)` group's live held/desired pair — see
    /// [`GroupLive`]. Read for the group's *current* `desired` (which the
    /// row's own WANTED stepper may already have moved) and written with the
    /// group's new held total.
    live: GroupLive,
    /// Owned by the caller (`CardTableRow`), not here, so the row's selection
    /// checkbox can react to the same flag `HaveStepper` flips — see that
    /// component's doc.
    removed: RwSignal<bool>,
) -> impl IntoView {
    let tree = expect_context::<CollectionTreeResource>().0;
    // The revision a tray batch-move bumps too; only bumped from here on an
    // Undo settling (see the doc above and `HaveStepper::on_undo_settled`).
    let revision = use_context::<crate::my::move_selection::HoldingsRevision>();

    let on_change = Callback::new(move |c: StepperCommit| {
        here_delta.update(|d| *d += c.to - c.from);
        if let Some(d) = section_delta {
            // Against the group's live numbers, not this component's
            // construction-time snapshot of either: this row's own WANTED
            // stepper may already have moved `desired`, and a sibling printing
            // row may already have moved `held` (want-stepper review round 1,
            // MAJOR — see [`GroupLive`]). The group's held moves by exactly
            // this row's copy delta, since the group is a sum over its rows.
            let held_before = live.held.get_untracked();
            let held_after = held_before + (c.to - c.from);
            d.update(|d| {
                *d += section_slot_delta(held_before, held_after, live.desired.get_untracked())
            });
        }
        // Written even without a section (a binder has none): `WantedCount`
        // reads it, and the two steppers must compose in either layout.
        live.held.update(|h| *h += c.to - c.from);
        // Pushed in addition to, never instead of, the two aggregates above —
        // see `RowDeltas`'s doc on why the grid needs a *per-row* aggregate
        // neither of those is.
        if let Some(id) = holding_id {
            row_deltas.push_present(id, c.to - c.from);
        }
    });
    let on_settled = Callback::new(move |()| tree.refetch());
    let on_undo_settled = Callback::new(move |()| {
        if let Some(r) = revision {
            r.bump();
        }
    });

    view! {
        <crate::components::holding_stepper::HaveStepper
            name
            present
            holding_id
            on_change
            on_settled
            on_undo_settled
            removed
        />
    }
}

/// The WANTED number: the in-place stepper where the cell both prints a number
/// and has a single `desires` row to address, plain text everywhere else.
///
/// The write semantics live in
/// [`crate::components::holding_stepper::WantStepper`] — the same component
/// `/cards/:id`'s "Your wants" rows use, so the two surfaces cannot drift on
/// what a committed zero means (desires carry no ledger, so it is a direct,
/// non-undoable delete; see that component's doc). This wrapper is the
/// page-specific half, exactly as [`HereCount`] is for HERE: it turns the
/// stepper's generic `(from, to)` into this page's aggregates and refetches the
/// sidebar tree.
///
/// **When the stepper renders.** Only where [`wanted_cell`] already prints a
/// number — the spec's "WANTED only when set and different" (specs/app-ui.md)
/// decides *visibility*, and making the cell editable does not license
/// printing a count the spec collapses away. Two consequences worth knowing:
/// a card whose want is exactly met (`desired == present`) collapses to `—`
/// and is therefore not steppable *here* (adjust it from `/cards/:id`'s "Your
/// wants", which lists every desire unconditionally), and a want the WANTED
/// column dedupes onto the card's first `(oracle, board)` row is steppable on
/// that row alone — which is right, since `desire_id` is that same grain.
///
/// A cell that prints a number but sums several `desires` rows (a
/// printing-pinned want beside an unpinned one) falls to `WantStepper`'s own
/// refusal span, under this page's `wanted-placeholder` testid rather than the
/// component's default `here-count` — the row's HERE cell may already carry
/// one of those, and a row-scoped locator must not match two.
#[component]
fn WantedCount(
    name: String,
    /// What this cell prints, straight from [`wanted_cell`]. `None` is the
    /// spec's collapsed `—`, and never a stepper.
    wanted: Option<i32>,
    /// The one `desires` row backing this cell — [`shared::CardRow::desire_id`].
    desire_id: Option<Id>,
    /// The page header's live "· N wanted" clause — see [`counts_summary`].
    want_delta: RwSignal<i32>,
    /// This row's deck section, when it has one (`None` in a binder). Not
    /// `#[prop(optional)]`, for the same reason [`HereCount`]'s isn't: the
    /// caller already holds an `Option` and is forwarding it, not deciding
    /// presence here.
    section_delta: Option<RwSignal<i32>>,
    /// `here_delta`'s per-row twin — this component writes its `desire_id`
    /// half, which is what keeps a grid tile's WANTED badge honest and stops a
    /// zeroed want leaving a ghost tile behind (see [`RowDeltas`]).
    row_deltas: RowDeltas,
    /// This `(oracle, board)` group's live held/desired pair — see
    /// [`GroupLive`]. Read for the group's *current* held total (which the
    /// row's own HERE stepper may already have moved) and written with the new
    /// desired count.
    live: GroupLive,
) -> impl IntoView {
    let Some(desired) = wanted else {
        return view! {
            <span class="text-muted-foreground" data-testid="wanted-placeholder">
                "—"
            </span>
        }
        .into_any();
    };
    let tree = expect_context::<CollectionTreeResource>().0;

    let on_change = Callback::new(move |c: StepperCommit| {
        want_delta.update(|d| *d += c.to - c.from);
        if let Some(d) = section_delta {
            // The group's *live* held, not this component's construction-time
            // copy — the row's HERE stepper may already have moved it. See
            // [`GroupLive`] and `HereCount`'s mirror of this comment.
            d.update(|d| *d += section_slot_delta(c.from, c.to, live.held.get_untracked()));
        }
        live.desired.set(c.to);
        if let Some(id) = desire_id {
            row_deltas.push_desired(id, c.to - c.from);
        }
    });
    let on_settled = Callback::new(move |()| tree.refetch());

    view! {
        <crate::components::holding_stepper::WantStepper
            name
            desired
            desire_id
            on_change
            on_settled
            count_testid="wanted-placeholder"
        />
    }
    .into_any()
}

/// The grid layout: folder tiles (when there are any and no search is
/// running — see [`folder_list`]) above the card tiles, capped by
/// `crate::catalog::GRID_CLASS` so this grid's columns and breakpoints cannot
/// drift from Catalog's or `/my`'s.
///
/// **Deck sections carry over.** A deck's cards group by board and type in the
/// table (`group_deck`); the grid keeps the same grouping, one heading plus
/// one tile grid per section, rather than flattening to a single grid — the
/// section headers are load-bearing information (slot counts, sideboard vs
/// main), not a table-only affordance, and losing them on the grid would make
/// the two layouts describe different decks. `group_deck` runs over every row
/// from the snapshot, live-zeroed or not — bucketing and (deliberately, see
/// [`RowDeltas`]'s doc — out of scope for this round) the section's own slot
/// count stay exactly what the table itself would compute from this same
/// snapshot; *which rows get a tile* and *what their HERE and WANTED badges
/// read* are where the live overlay applies.
///
/// **Reads `row_deltas` exactly once, right here, untracked** — see
/// [`RowDeltas`]'s "Read side" for why that is both correct and sufficient:
/// this component is a fresh instance every time the switch is clicked into
/// grid, and neither column can be edited while grid is showing.
#[component]
fn CollectionGrid(
    view: CollectionView,
    folders: Memo<Vec<shared::CollectionSummary>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
    row_deltas: RowDeltas,
) -> impl IntoView {
    let is_deck = view.collection.kind == CollectionKind::Deck;
    let collection_id = view.collection.id;
    let rows = view_rows(view.cards);

    // Pair each row with its live HERE *and* WANTED counts, and drop the ones a
    // same-session edit zeroed out in both — the "ghost tile" finding, in the
    // two columns that can now produce one. A row addressable by neither
    // stepper (a multi-grain HERE cell with no single desires row behind its
    // want either) cannot have been zeroed here, so it survives unchanged; see
    // [`row_survives`].
    let live_rows = |rows: Vec<ViewRow>| -> Vec<(ViewRow, i32, i32)> {
        rows.into_iter()
            .filter_map(|row| {
                let present = row_deltas.live_present(row.row.holding_id, row.row.present);
                let desired = row_deltas.live_desired(row.row.desire_id, row.row.desired);
                let editable = row.row.holding_id.is_some() || row.row.desire_id.is_some();
                row_survives(editable, present, desired).then_some((row, present, desired))
            })
            .collect()
    };

    view! {
        <div class="flex flex-col gap-6" data-testid="collection-grid">
            {move || {
                let folders = folders.get();
                (!folders.is_empty())
                    .then(|| {
                        view! {
                            <ul class=GRID_CLASS data-testid="folder-grid">
                                {folders
                                    .into_iter()
                                    .map(|folder| view! { <FolderTile folder tree /> })
                                    .collect_view()}
                            </ul>
                        }
                    })
            }}
            {if is_deck {
                group_deck(rows)
                    .into_iter()
                    .map(|section| {
                        let label = section.label.clone();
                        view! {
                            <div data-testid="deck-grid-section" data-section=label.clone()>
                                <h3 class="text-muted-foreground mb-2 text-xs font-semibold tracking-wide uppercase">
                                    {label.clone()} " · " {section.slots.to_string()}
                                </h3>
                                <ul class=GRID_CLASS>
                                    {live_rows(section.rows)
                                        .into_iter()
                                        .map(|(row, live_present, live_desired)| {
                                            view! {
                                                <HoldingTile
                                                    row
                                                    collection_id
                                                    live_present
                                                    live_desired
                                                />
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            </div>
                        }
                    })
                    .collect_view()
                    .into_any()
            } else {
                view! {
                    <ul class=GRID_CLASS data-testid="collection-cards-grid">
                        {live_rows(rows)
                            .into_iter()
                            .map(|(row, live_present, live_desired)| {
                                view! {
                                    <HoldingTile row collection_id live_present live_desired />
                                }
                            })
                            .collect_view()}
                    </ul>
                }
                    .into_any()
            }}
        </div>
    }
}

/// A child collection's grid tile — the folder row's tile-shaped counterpart.
/// Its count is the same rolled-up read `FolderTableRow` uses, from the same
/// tree, in its own `Suspense` for the same reason (a stepper commit refetches
/// the tree far more often than it should rebuild this grid).
#[component]
fn FolderTile(
    folder: shared::CollectionSummary,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
) -> impl IntoView {
    let id = folder.id;
    let name = folder.name.clone();
    view! {
        <li
            class="group/tile flex flex-col gap-2"
            data-testid="folder-tile"
            data-collection=id.to_string()
        >
            <a
                href=format!("/my/collections/{id}")
                class="bg-muted/40 hover:bg-muted focus-visible:ring-ring flex aspect-[5/7] w-full flex-col items-center justify-center gap-1 rounded-lg focus-visible:ring-2 focus-visible:outline-none"
            >
                <span aria-hidden="true" class="text-3xl">
                    {if folder.kind == CollectionKind::Deck { "🃏" } else { "📁" }}
                </span>
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        rolled_up_of(&assembled_roots(tree.await), id)
                            .filter(|n| *n > 0)
                            .map(|n| {
                                view! {
                                    <span
                                        class="text-muted-foreground text-xs italic"
                                        data-testid="here-count"
                                    >
                                        {n.to_string()} " here"
                                    </span>
                                }
                            })
                    })}
                </Suspense>
            </a>
            <div class="min-w-0">
                <p class="truncate text-sm font-medium" title=name.clone()>
                    {name.clone()}
                </p>
            </div>
        </li>
    }
}

/// One card's grid tile: image, name, and the same HERE/WANTED/OWNED badges
/// the table's cells show — `wanted_of`/`owned_cell` are the same pure helpers
/// `CardTableRow` uses, so the two layouts cannot disagree about those
/// numbers.
///
/// **HERE and WANTED both arrive live**, resolved by the caller
/// (`CollectionGrid`) through [`RowDeltas`]: `live_present` (plus this row's
/// own `present_rollup`) stands in for `here_total(&row.row)`'s
/// frozen-snapshot present, and `live_desired` for `row.row.desired`. WANTED
/// used to be listed here as deliberately static — true only while the column
/// was read-only, and false the moment the table's WANTED cell became
/// steppable (want-stepper review round 1, MAJOR): a want edited in list mode
/// then viewed in grid showed the pre-edit number, and a zeroed want-only card
/// kept a `1 wanted` badge for a `desires` row that no longer existed.
///
/// OWNED *is* still static, and stays so: it is a global per-oracle aggregate
/// no stepper on this page addresses, exactly as static in the table's own
/// cell, so leaving it introduces no disagreement between the two layouts.
/// The **collapse test** for WANTED is likewise still evaluated against the
/// snapshot `present` rather than the live one — see [`wanted_of`]'s doc for
/// why that is the choice that keeps the two layouts agreeing.
///
/// No stepper here: editing is a table-only surface (the count stepper is a
/// list-only editing surface, not something the grid — a display mode —
/// reproduces) — see specs/app-ui.md's Findings entry for the grid-toggle
/// task.
///
/// Selection stays reachable: `SelectionCheckbox` is a sibling of the `<a>`,
/// never nested inside it, for the same reason `AllCardsTile` keeps them
/// siblings (see that component's doc).
#[component]
fn HoldingTile(
    row: ViewRow,
    collection_id: Id,
    /// This row's live HERE count — `RowDeltas::live_present` already applied
    /// by the caller (`CollectionGrid`), which is also what decided whether
    /// this component gets mounted at all (a row live-zeroed in both columns
    /// never reaches here — see that component's `live_rows`).
    live_present: i32,
    /// This row's live WANTED count — `RowDeltas::live_desired` applied by the
    /// same caller, for the same reason.
    live_desired: i32,
) -> impl IntoView {
    // Snapshot `present` on purpose in the collapse test — see `wanted_of`.
    let wanted = wanted_of(row.show_wanted, row.row.present, live_desired);
    let owned = owned_cell(&row.row);
    let here = live_present + row.row.present_rollup;
    let CardRow {
        oracle_id,
        printing_id,
        name,
        image_uri,
        mana_cost,
        type_line,
        present,
        owned: owned_total,
        board,
        faces,
        ..
    } = row.row;

    let selection = use_selection();
    let key = SelectionKey::Held {
        collection_id,
        printing_id,
        board,
    };
    let selected = selection.selected(key);
    let selectable = (present > 0).then(|| SelectedCard {
        key,
        oracle_id,
        name: name.clone(),
        image_uri: image_uri.clone(),
    });

    let preview = CardSummary {
        oracle_id,
        name: name.clone(),
        printing_id: Some(printing_id),
        image_uri: image_uri.clone(),
        mana_cost,
        type_line,
        owned: Some(owned_total),
        faces,
    };
    let href = format!("/cards/{oracle_id}");
    let link_name = name.clone();

    view! {
        <li
            class="group/tile flex flex-col gap-2"
            data-testid="collection-tile"
            data-oracle=oracle_id.to_string()
            data-printing=printing_id.to_string()
            data-board=board.to_pg()
            data-state=move || selected.get().then_some("selected")
        >
            <div class="relative">
                // hover=false: the tile is already the card art — same call
                // `catalog::CardTile` and `AllCardsTile` make.
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
                                        alt=name.clone()
                                        loading="lazy"
                                        decoding="async"
                                        class="absolute inset-0 size-full rounded-lg object-cover"
                                    />
                                }
                            })}
                        <div class="absolute right-1.5 top-1.5 flex flex-col items-end gap-1">
                            {(here > 0)
                                .then(|| {
                                    view! {
                                        <span data-testid="here-badge">
                                            <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                                                {format!("{here} here")}
                                            </Badge>
                                        </span>
                                    }
                                })}
                            {wanted
                                .map(|n| {
                                    view! {
                                        <span data-testid="wanted-badge">
                                            <Badge variant=BadgeVariant::Default size=BadgeSize::Sm>
                                                {format!("{n} wanted")}
                                            </Badge>
                                        </span>
                                    }
                                })}
                            {owned
                                .map(|n| {
                                    view! {
                                        <span data-testid="owned-badge">
                                            <Badge variant=BadgeVariant::Muted size=BadgeSize::Sm>
                                                {format!("{n} owned")}
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

/// Keyset paging controls — forward-only, for the reason `/my`'s are
/// (a cursor describes "everything after this row").
///
/// **Reactive hrefs, not baked strings** (mirrors `crate::catalog::Pager`'s
/// and `crate::my::all_cards::Pager`'s own finding): this component is
/// mounted once per `(id, q, cursor)` resolution and stays mounted across a
/// pure view-switch click (see `CollectionBody`'s comment on why), so a fixed
/// `href` computed at construction would still point at the layout that was
/// current when the page loaded.
#[component]
fn Pager(
    next: Option<String>,
    paged: Memo<bool>,
    q: String,
    id: String,
    list_view: Memo<bool>,
) -> impl IntoView {
    const LINK: &str =
        "border-input hover:bg-accent hover:text-accent-foreground rounded-md border px-3 py-1.5 text-sm";
    let q = StoredValue::new(q);
    let id = StoredValue::new(id);

    view! {
        <nav aria-label="Pagination" class="flex items-center justify-between gap-2">
            <Show when=move || paged.get() fallback=|| view! { <span></span> }>
                <a
                    href=move || {
                        collection_url(&id.get_value(), &q.get_value(), list_view.get(), None)
                    }
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
                                collection_url(
                                    &id.get_value(),
                                    &q.get_value(),
                                    list_view.get(),
                                    Some(&c.get_value()),
                                )
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

// -------------------------------------------------------------- teardown ----

/// "Empty deck…" (specs/app-ui.md → the deck variant; specs/collection-api.md →
/// Teardown). Two modes, and the second is the one with no picker: **return to
/// previous locations** reads each card's most recent move *into* this deck and
/// sends it back where it came from, falling back to the Inbox. The other mode
/// needs a destination, so the confirm stays disabled until one is chosen.
#[component]
fn TeardownDialog(
    open: RwSignal<bool>,
    collection_id: Id,
    view_res: Resource<Result<CollectionViewPayload, ServerFnError<shared::ApiError>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<shared::ApiError>>>>,
) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    // A teardown is N ledger rows and nothing else records them, so without this
    // ⌘K's `Undo last move` would reach past it and reverse an older, unrelated
    // move — the confirm below promises "every move is in the history", and this
    // is what makes that promise reachable.
    let last_move = use_context::<crate::components::palette::LastMoveState>();
    // Shell-level, like `last_move` above — and for the same reason
    // `undo_teardown` below reaches for it instead of `view_res` directly: the
    // toast this dialog raises can fire its Undo after the page that raised it
    // is gone (P6-117's "toast outlives its row"), and `view_res` is *this
    // page's* resource, disposed with it, while the tree and this revision are
    // shell-level and always live.
    let revision = use_context::<crate::my::move_selection::HoldingsRevision>();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // "" = return to previous locations; otherwise a collection id.
    let destination = RwSignal::new(String::new());

    // Reverse the whole teardown through the same batch undo the tray's own
    // Undo and ⌘K's `Undo last move` both call — `undo_selection_move`, the
    // one-transaction counterpart of `undo_move` (specs/collection-api.md →
    // Undo). Unlike `undo_removal` (`HereCount`, this module) a teardown has no
    // single row to rewire: every card left the deck, so there is nothing here
    // to address back in place — refetching the tree and bumping the revision
    // is the whole of "refresh what the page shows".
    let undo_teardown = move |move_ids: Vec<Id>| {
        let count = move_ids.len();
        // The palette must stop offering the same reversal (`forget`'s doc) —
        // but restored below if the dispatch fails, mirroring ⌘K's own
        // `UndoLastMove` handler (`palette.rs`) exactly. The toast itself
        // cannot be the fallback: `Toaster` dismisses it the instant this
        // button is clicked, before the request below even resolves (sonner.rs
        // `on:click`), so forgetting unconditionally here would leave a failed
        // reversal unreachable from *any* UI — strictly worse than before this
        // task, when a desktop session at least always had ⌘K.
        crate::components::palette::forget_last_move(last_move, &move_ids);
        let restore = move_ids.clone();
        spawn_local(async move {
            match crate::undo_selection_move(move_ids).await {
                Ok(()) => {
                    tree.refetch();
                    if let Some(r) = revision {
                        r.bump();
                    }
                    // The tray's own phrasing (`move_selection::undo`), not
                    // ⌘K's: this is the same batch-undo shape, and this toast
                    // is designed to fire after its page is gone, so it must
                    // say something rather than succeed silently off-page.
                    let cards = if count == 1 { "1 card" } else { "them" };
                    toast.show(ToastOptions::message(format!("Put {cards} back")));
                }
                Err(e) => {
                    // Put the reversal back within ⌘K's reach — only if
                    // nothing newer arrived meanwhile (the same guard ⌘K's
                    // own retry uses), since the toast that started this is
                    // already gone regardless of how this call turns out.
                    if let Some(state) = last_move {
                        if state.0.get_untracked().is_none() {
                            state.note(restore);
                        }
                    }
                    toast.show(
                        ToastOptions::message(format!(
                            "Couldn't undo: {} — try ⌘K → Undo last move",
                            message_of(&e)
                        ))
                        .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let raw = destination.get_untracked();
        let to = if raw.is_empty() {
            None
        } else {
            match Id::parse_str(&raw) {
                Ok(id) => Some(id),
                Err(_) => {
                    error.set(Some("Pick a destination.".into()));
                    return;
                }
            }
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::teardown_collection(collection_id, to).await {
                Ok(receipt) => {
                    busy.set(false);
                    open.set(false);
                    let move_ids = receipt.move_ids;
                    let moved = move_ids.len();
                    crate::components::palette::note_last_move(last_move, move_ids.clone());
                    let mut opts = ToastOptions::message(format!(
                        "Emptied — {moved} card{} moved",
                        if moved == 1 { "" } else { "s" },
                    ));
                    // Nothing moved, nothing to undo — `note_last_move` above
                    // already no-ops on an empty vec for the same reason.
                    if !move_ids.is_empty() {
                        opts = opts.action(
                            "Undo",
                            Callback::new(move |()| undo_teardown(move_ids.clone())),
                        );
                    }
                    toast.show(opts);
                    view_res.refetch();
                    tree.refetch();
                }
                Err(e) => {
                    busy.set(false);
                    error.set(Some(message_of(&e)));
                }
            }
        });
    };

    view! {
        <Dialog id="collection-teardown" open>
            <DialogContent aria_label="Empty deck">
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>"Empty this deck"</DialogTitle>
                        <DialogDescription>
                            "Every card leaves the deck. Nothing is deleted — each copy moves somewhere, and every move is in the history."
                        </DialogDescription>
                    </DialogHeader>
                    <div class="flex flex-col gap-3 text-sm">
                        <label class="flex flex-col gap-1">
                            <span class="font-medium">"Send the cards to"</span>
                            <select
                                class="border-input bg-background rounded-md border px-2 py-1.5 text-sm"
                                data-testid="teardown-destination"
                                on:change=move |ev| {
                                    destination.set(event_target_value(&ev));
                                }
                                prop:value=move || destination.get()
                            >
                                <option value="" data-testid="teardown-previous">
                                    "Their previous locations"
                                </option>
                                // Awaited, not read in render: a resource read
                                // outside a `Suspend` renders one thing during
                                // SSR and another after hydration — the
                                // read-in-render trap the cross-task audit
                                // recorded (specs/app-ui.md Findings).
                                <Suspense fallback=|| {
                                    view! { <option value="" disabled>"…"</option> }
                                }>
                                    {move || Suspend::new(async move {
                                        flatten_destinations(
                                                &assembled_roots(tree.await),
                                                collection_id,
                                            )
                                            .into_iter()
                                            .map(|(id, label)| {
                                                view! { <option value=id.to_string()>{label}</option> }
                                            })
                                            .collect_view()
                                    })}
                                </Suspense>
                            </select>
                        </label>
                        <p class="text-muted-foreground text-xs">
                            "\"Their previous locations\" sends each card back to its most recent live, un-undone source — Inbox where none exists."
                        </p>
                        {move || {
                            error
                                .get()
                                .map(|msg| {
                                    view! {
                                        <p class="text-destructive text-sm" data-testid="teardown-error">
                                            {msg}
                                        </p>
                                    }
                                })
                        }}
                    </div>
                    <DialogFooter>
                        <DialogClose>"Cancel"</DialogClose>
                        <Button
                            variant=ButtonVariant::Destructive
                            attr:data-testid="teardown-confirm"
                            attr:disabled=move || busy.get()
                            on:click=move |_| submit()
                        >
                            "Empty deck"
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </DialogContent>
        </Dialog>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{CollectionSummary, CollectionTotals, CollectionTree, CollectionTreeRow};

    fn card(name: &str, type_line: &str, present: i32, desired: i32, owned: i32) -> CardRow {
        CardRow {
            oracle_id: Id::from_u128(name.len() as u128),
            printing_id: Id::from_u128(1000 + name.len() as u128),
            name: name.into(),
            set_code: None,
            collector_number: "1".into(),
            image_uri: None,
            mana_cost: None,
            type_line: Some(type_line.into()),
            colors: vec![],
            present,
            desired,
            owned,
            present_rollup: 0,
            board: Board::Main,
            holding_id: None,
            desire_id: None,
            faces: vec![],
        }
    }

    #[test]
    fn url_omits_empty_parts_and_encodes() {
        assert_eq!(collection_url("abc", "", true, None), "/my/collections/abc");
        assert_eq!(
            collection_url("abc", "", true, Some("")),
            "/my/collections/abc"
        );
        assert_eq!(
            collection_url("abc", "bolt", true, None),
            "/my/collections/abc?q=bolt"
        );
        assert_eq!(
            collection_url("abc", "", true, Some("cur")),
            "/my/collections/abc?cursor=cur"
        );
        assert_eq!(
            collection_url("abc", "fire // ice", true, Some("c d")),
            "/my/collections/abc?q=fire%20%2F%2F%20ice&cursor=c%20d"
        );
    }

    #[test]
    fn grid_is_the_non_default_view_unlike_catalog() {
        // Same opposite-of-Catalog polarity as `crate::my::all_cards::my_url`
        // — see `VIEW_PARAM`'s doc comment. A bare URL (list_view = true)
        // omits the param; choosing grid (list_view = false) spells it out.
        assert_eq!(
            collection_url("abc", "", false, None),
            "/my/collections/abc?view=grid"
        );
        assert_eq!(
            collection_url("abc", "bolt", false, None),
            "/my/collections/abc?q=bolt&view=grid"
        );
        assert_eq!(
            collection_url("abc", "bolt", false, Some("cur")),
            "/my/collections/abc?q=bolt&view=grid&cursor=cur"
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
    fn add_default_is_want_only_in_decks() {
        // "binders and Inbox are Have-led" is one condition, because the Inbox
        // is a binder.
        assert_eq!(add_default(CollectionKind::Deck), QuickAddKind::Want);
        assert_eq!(add_default(CollectionKind::Binder), QuickAddKind::Have);
    }

    #[test]
    fn wanted_shows_only_when_set_and_different() {
        // The wireframe's own rows: Bolt 3/4/7 prints WANTED, Brainstorm has
        // none set, and a card whose desire is already met prints none either.
        let bolt = view_rows(vec![card("Bolt", "Instant", 3, 4, 7)]);
        assert_eq!(wanted_cell(&bolt[0]), Some(4));
        let brainstorm = view_rows(vec![card("Brainstorm", "Instant", 4, 0, 12)]);
        assert_eq!(wanted_cell(&brainstorm[0]), None);
        let met = view_rows(vec![card("Counterspell", "Instant", 2, 2, 2)]);
        assert_eq!(wanted_cell(&met[0]), None);
    }

    #[test]
    fn wanted_prints_once_per_card_and_board() {
        // `desired` is oracle-grained, so two printings of one card both carry
        // it; printing it twice would read as eight wanted, not four.
        let mut a = card("Bolt", "Instant", 1, 4, 7);
        let mut b = card("Bolt", "Instant", 2, 4, 7);
        b.printing_id = Id::from_u128(9999);
        a.board = Board::Main;
        b.board = Board::Main;
        let rows = view_rows(vec![a, b]);
        assert_eq!(wanted_cell(&rows[0]), Some(4));
        assert_eq!(wanted_cell(&rows[1]), None);
    }

    #[test]
    fn owned_collapses_against_the_here_total() {
        // Equal to what's here → no second copy of the same number…
        assert_eq!(owned_cell(&card("A", "Instant", 2, 0, 2)), None);
        // …including the rolled-up half, which the HERE cell also shows.
        let mut rolled = card("B", "Instant", 3, 0, 5);
        rolled.present_rollup = 2;
        assert_eq!(owned_cell(&rolled), None);
        // Copies elsewhere are the whole point of the column.
        assert_eq!(owned_cell(&card("C", "Instant", 3, 0, 7)), Some(7));
    }

    #[test]
    fn row_selectable_withdraws_once_removed_and_returns_on_undo() {
        // The defect (P6-118): a removed row's checkbox stayed interactive
        // and ticking it earned a `NoCopies` refusal from the tray.
        assert!(row_selectable(false, 3));
        assert!(!row_selectable(true, 3));
        // A desire-only / never-held row (`holding_id: None`) was never
        // selectable to begin with — `removed` alone must not flip it on.
        assert!(!row_selectable(false, 0));
        assert!(!row_selectable(true, 0));
        // Undo flips `removed` back to `false` — selectability returns with
        // it, the same signal, no separate restore path to fall out of sync.
        assert!(row_selectable(false, 3));
    }

    #[test]
    fn section_slots_live_adds_the_pushed_delta() {
        // The combiner itself is just addition — whether what's pushed into
        // it is honest is `section_slot_delta`'s job, tested below.
        assert_eq!(section_slots_live(5, 0), 5);
        assert_eq!(section_slots_live(5, -2), 3);
        assert_eq!(section_slots_live(3, 2), 5);
    }

    #[test]
    fn section_slot_delta_holds_a_wanted_under_held_row_at_its_desired_count() {
        // The bug this review round caught: a raw copy delta corrupts the
        // header the moment the row is wanted and under-held — the ordinary
        // deck row, since decks are Want-led. 4 held, 4 desired: stepping to
        // 2 must leave the section's contribution at 4 (2 held + 2 still
        // lacking — the WANTED cell on this row still reads 4), not push it
        // to 2.
        assert_eq!(section_slot_delta(4, 2, 4), 0);
        // Removing the lot (held 4 → 0) is the same story: the slots are
        // still wanted, so the header must not move at all.
        assert_eq!(section_slot_delta(4, 0, 4), 0);
        // Undo is the exact reverse and must land back at zero too.
        assert_eq!(section_slot_delta(0, 4, 4), 0);
        // Over-held (5 held, 4 desired) stepped down to 3: only the 1 truly
        // surplus copy (5 → 4) counts against the header, not the full 2.
        assert_eq!(section_slot_delta(5, 3, 4), -1);
    }

    /// The want-stepper round-1 MAJOR, as arithmetic: both steppers in one row,
    /// used in sequence, must land the header on the truth — which they only
    /// do if each reads the *other* number's post-edit value (`GroupLive`),
    /// not the copy it captured when it mounted.
    #[test]
    fn both_steppers_in_one_row_compose_against_the_live_other_column() {
        // The reported repro: held 2, desired 4 — so the section contributes
        // `max(2, 4)` = 4 slots at load.
        let (mut held, mut desired) = (2, 4);
        let mut header = 4;

        // WANTED 4 → 1.
        header += section_slot_delta(desired, 1, held);
        desired = 1;
        assert_eq!(header, 2, "2 held, 1 wanted → the 2 copies are the slots");

        // HERE 2 → 3, against the *live* desired (1), not the mounted 4. The
        // stale read is what pushed 0 here and left the header at 2.
        header += section_slot_delta(held, 3, desired);
        held = 3;
        assert_eq!(header, held.max(desired));
        assert_eq!(header, 3);

        // …and the reverse order lands on the same truth, which is the
        // property that makes the two steppers order-independent.
        let (mut held, mut desired) = (2, 4);
        let mut header = 4;
        header += section_slot_delta(held, 3, desired);
        held = 3;
        header += section_slot_delta(desired, 1, held);
        desired = 1;
        assert_eq!(header, held.max(desired));
        assert_eq!(header, 3);
    }

    #[test]
    fn section_slot_delta_serves_a_want_edit_with_its_arguments_swapped() {
        // `max(held, desired)` is symmetric in the two numbers, so the WANTED
        // stepper pushes through the same function — the moving side first,
        // the fixed side last. 3 held, want 4 → 6: two more slots.
        assert_eq!(section_slot_delta(4, 6, 3), 2);
        // Want cut below what is already held: the copies are still in the
        // deck, so the section's count does not move.
        assert_eq!(section_slot_delta(4, 2, 5), 0);
        // Want dropped entirely on a card held nowhere: the slots go with it.
        assert_eq!(section_slot_delta(3, 0, 0), -3);
    }

    #[test]
    fn view_rows_fold_a_cards_held_copies_across_its_printings() {
        // The number the WANTED stepper needs at commit time: `section_slots`
        // sums held per `(oracle, board)`, and the stepper lives on the *first*
        // row of that group, whose own `present` is only one printing's worth.
        let mut a = card("Bolt", "Instant", 1, 4, 7);
        let mut b = card("Bolt", "Instant", 2, 4, 7);
        b.printing_id = Id::from_u128(9999);
        a.board = Board::Main;
        b.board = Board::Main;
        let rows = view_rows(vec![a, b]);
        assert_eq!(rows[0].held_in_group, 3);
        assert_eq!(rows[1].held_in_group, 3);
        // …and it is exactly what `section_slots` folds for the same rows, so
        // the live delta and the static count cannot disagree on `held`.
        assert_eq!(section_slots(&rows), 4);

        // Boards are separate groups: a sideboard copy is not a mainboard one.
        let mut main = card("Bolt", "Instant", 1, 4, 7);
        let mut side = card("Bolt", "Instant", 2, 1, 7);
        side.printing_id = Id::from_u128(9999);
        main.board = Board::Main;
        side.board = Board::Side;
        let split = view_rows(vec![main, side]);
        assert_eq!(split[0].held_in_group, 1);
        assert_eq!(split[1].held_in_group, 2);
    }

    #[test]
    fn section_slot_delta_is_a_plain_copy_delta_when_nothing_is_desired() {
        // `desired == 0` is the case the first (wrong) cut of this fix
        // implicitly assumed applied everywhere: here the slot delta and the
        // copy delta genuinely coincide.
        assert_eq!(section_slot_delta(3, 1, 0), -2);
        assert_eq!(section_slot_delta(0, 3, 0), 3);
        assert_eq!(section_slot_delta(3, 0, 0), -3);
    }

    // ---------------------------------------------- grid HERE overlay ----
    // `RowDeltas`, the fix for the grid-toggle task's round-2 review finding
    // (a HoldingTile reading the frozen `view` snapshot directly went stale
    // after a same-session HERE edit, and kept rendering a tile for a holding
    // a commit-to-zero had deleted).

    #[test]
    fn live_present_applies_the_delta() {
        assert_eq!(live_present(2, 0), 2);
        assert_eq!(live_present(2, 1), 3);
        assert_eq!(live_present(2, -2), 0);
    }

    #[test]
    fn live_present_never_goes_negative() {
        // A defensive floor, not a reachable case in practice — a row's own
        // delta cannot out-negative its snapshot present (the commit that
        // would have to produce that is refused server-side) — but a tile
        // reading a negative HERE count would be a worse failure than a
        // floor at zero if some future caller's bookkeeping ever drifted.
        assert_eq!(live_present(1, -5), 0);
    }

    #[test]
    fn a_grid_row_survives_while_either_column_still_has_something() {
        // The ghost-tile rule, in the two columns that can now produce one.
        // A row no stepper could address was never zeroable here, so it keeps
        // its tile whatever the numbers say — the old
        // `holding_id.is_none() => keep` shortcut, now stated as what it
        // actually meant.
        assert!(row_survives(false, 0, 0));
        // Held only: the HERE stepper decides it.
        assert!(row_survives(true, 2, 0));
        assert!(!row_survives(true, 0, 0));
        // Want-only (the case the shortcut used to wave through): once the
        // want is zeroed there is nothing left, and the tile must go.
        assert!(row_survives(true, 0, 3));
        // Held *and* wanted, with HERE stepped to zero: still a want here, so
        // the tile stays — as a want-only tile, which is exactly what the
        // table's own row becomes and what the server still holds a row for.
        assert!(row_survives(true, 0, 4));
    }

    #[test]
    fn a_tiles_wanted_badge_reads_the_live_desire_by_the_same_rule_as_the_cell() {
        // `wanted_of` is the one rule both layouts evaluate; the grid feeds it
        // an overlaid `desired` and the table its snapshot. Stepping a want
        // 4 → 2 must move the badge, and zeroing it must remove the badge
        // rather than leave `0 wanted` (or the pre-edit 4) standing.
        assert_eq!(wanted_of(true, 0, 4), Some(4));
        assert_eq!(wanted_of(true, 0, 2), Some(2));
        assert_eq!(wanted_of(true, 0, 0), None);
        // The dedupe and collapse halves of the rule are unchanged by the
        // overlay: a repeat row prints nothing, and a met want collapses.
        assert_eq!(wanted_of(false, 0, 4), None);
        assert_eq!(wanted_of(true, 2, 2), None);
    }

    #[test]
    fn accumulate_row_delta_sums_repeated_commits_to_the_same_holding() {
        let mut map = HashMap::new();
        let id = Id::from_u128(1);
        accumulate_row_delta(&mut map, id, 1);
        accumulate_row_delta(&mut map, id, 1);
        assert_eq!(map.get(&id), Some(&2));
    }

    #[test]
    fn accumulate_row_delta_tracks_each_holding_independently() {
        let mut map = HashMap::new();
        let a = Id::from_u128(1);
        let b = Id::from_u128(2);
        accumulate_row_delta(&mut map, a, 3);
        accumulate_row_delta(&mut map, b, -1);
        assert_eq!(map.get(&a), Some(&3));
        assert_eq!(map.get(&b), Some(&-1));
    }

    #[test]
    fn a_removal_and_its_undo_net_back_to_the_snapshot() {
        // The exact sequence `HaveStepper::remove`/`undo_removal` produce: a
        // commit-to-zero pushes `(0 - present)`, and Undo pushes
        // `(present - 0)` back — both keyed by the *same* `holding_id`, even
        // though the server re-inserts the holding under a new one (see
        // `RowDeltas`'s doc on why that rewiring is invisible here).
        let mut map = HashMap::new();
        let id = Id::from_u128(7);
        let present = 3;
        accumulate_row_delta(&mut map, id, 0 - present);
        assert_eq!(
            live_present(present, map[&id]),
            0,
            "zeroed — the ghost-tile case"
        );
        accumulate_row_delta(&mut map, id, present);
        assert_eq!(live_present(present, map[&id]), present, "Undo restores it");
    }

    #[test]
    fn type_buckets_follow_decklist_convention() {
        // An artifact creature is a creature; a DFC's joined line matches on
        // either half; an unknown line falls to Other.
        assert_eq!(
            TYPE_ORDER[type_bucket(Some("Artifact Creature — Golem"))].1,
            "Creatures"
        );
        assert_eq!(
            TYPE_ORDER[type_bucket(Some("Legendary Creature — Elf // Sorcery"))].1,
            "Creatures"
        );
        assert_eq!(TYPE_ORDER[type_bucket(Some("Instant"))].1, "Instants");
        assert_eq!(
            TYPE_ORDER[type_bucket(Some("Basic Land — Island"))].1,
            "Lands"
        );
        assert_eq!(TYPE_ORDER[type_bucket(Some("Scheme"))].1, "Other");
        assert_eq!(TYPE_ORDER[type_bucket(None)].1, "Other");
    }

    #[test]
    fn deck_groups_by_board_then_type_with_slot_counts() {
        let mut side = card("Dispel", "Instant", 1, 0, 1);
        side.board = Board::Side;
        // Wanted but held nowhere: a real deck slot the section must count.
        let missing = card("Force of Will", "Instant", 0, 2, 0);
        let sections = group_deck(view_rows(vec![
            card("Bolt", "Instant", 3, 0, 3),
            card("Bear", "Creature — Bear", 1, 0, 1),
            missing,
            side,
        ]));
        let labels: Vec<&str> = sections.iter().map(|s| s.label.as_str()).collect();
        // Creatures before instants; other boards after the whole mainboard.
        assert_eq!(labels, ["Creatures", "Instants", "Sideboard · Instants"]);
        assert_eq!(sections[0].slots, 1);
        // 3 held + 2 wanted-and-absent.
        assert_eq!(sections[1].slots, 5);
        assert_eq!(sections[2].slots, 1);
    }

    #[test]
    fn section_slots_count_a_split_card_once() {
        // Two printings of one card, desire 4, three held between them: the
        // section is 4 slots, not 4 + 4.
        let mut a = card("Bolt", "Instant", 1, 4, 3);
        let mut b = card("Bolt", "Instant", 2, 4, 3);
        b.printing_id = Id::from_u128(9999);
        a.board = Board::Main;
        b.board = Board::Main;
        assert_eq!(section_slots(&view_rows(vec![a, b])), 4);
    }

    fn totals(
        present: i32,
        rollup: i32,
        desired: i32,
        missing: i32,
        elsewhere: i32,
    ) -> CollectionTotals {
        CollectionTotals {
            present,
            present_rollup: rollup,
            desired,
            missing,
            owned_elsewhere: elsewhere,
            to_buy: missing - elsewhere,
        }
    }

    #[test]
    fn counts_summary_matches_the_wireframe() {
        assert_eq!(
            counts_summary(&totals(102, 18, 6, 0, 0), 0, 0),
            "120 here (102 own + 18 rolled up) · 6 wanted"
        );
        // No children, nothing wanted: just the one number.
        assert_eq!(counts_summary(&totals(7, 0, 0, 0, 0), 0, 0), "7 here");
        // A committed stepper edit moves the header without a refetch.
        assert_eq!(counts_summary(&totals(7, 0, 0, 0, 0), 3, 0), "10 here");
    }

    #[test]
    fn counts_summary_tracks_committed_want_edits() {
        // The WANTED stepper's own un-refetched commits, the wants twin of the
        // HERE case above.
        assert_eq!(
            counts_summary(&totals(0, 0, 6, 0, 0), 0, 2),
            "0 here · 8 wanted"
        );
        // Zeroing the collection's last want drops the clause rather than
        // leaving a stale `· 1 wanted` standing until a reload.
        assert_eq!(counts_summary(&totals(4, 0, 1, 0, 0), 0, -1), "4 here");
    }

    #[test]
    fn needs_chip_matches_the_storyboard_and_vanishes_when_complete() {
        assert_eq!(
            needs_chip(&totals(0, 0, 9, 7, 4)).as_deref(),
            Some("7 missing — 4 owned elsewhere · 3 to buy")
        );
        // One-sided gaps drop the clause they have nothing to say about.
        assert_eq!(
            needs_chip(&totals(0, 0, 9, 2, 2)).as_deref(),
            Some("2 missing — 2 owned elsewhere")
        );
        assert_eq!(
            needs_chip(&totals(0, 0, 9, 2, 0)).as_deref(),
            Some("2 missing — 2 to buy")
        );
        assert_eq!(needs_chip(&totals(0, 0, 9, 0, 0)), None);
    }

    #[test]
    fn chip_state_goes_neutral_when_satisfied_and_vanishes_with_no_desires() {
        // Missing wins whenever there is one — `chip_state` restates
        // `needs_chip` verbatim rather than re-deriving it.
        assert_eq!(
            chip_state(&totals(0, 0, 9, 7, 4)),
            Some(ChipState::Missing(
                "7 missing — 4 owned elsewhere · 3 to buy".into()
            ))
        );
        // Wants exist and nothing is missing: the neutral chip, not silence —
        // this is the P6-143 fix, the reachability path to the needs-empty
        // "All set" state.
        assert_eq!(
            chip_state(&totals(0, 0, 9, 0, 0)),
            Some(ChipState::Satisfied)
        );
        // No desires at all: still nothing, exactly as before this task.
        assert_eq!(chip_state(&totals(0, 0, 0, 0, 0)), None);
    }

    fn tree_row(id: u128, parent: Option<u128>, name: &str, present: i64) -> CollectionTreeRow {
        CollectionTreeRow {
            summary: CollectionSummary {
                id: Id::from_u128(id),
                parent_id: parent.map(Id::from_u128),
                kind: CollectionKind::Binder,
                name: name.into(),
                is_inbox: false,
                position: 0.0,
                format: None,
            },
            present,
            desired: 0,
        }
    }

    fn sample_tree() -> Vec<TreeNode> {
        assemble(CollectionTree {
            collections: vec![
                tree_row(1, None, "Binders", 5),
                tree_row(2, Some(1), "Trade Binder", 120),
                tree_row(3, Some(2), "Foils", 18),
            ],
            shopping_short: 0,
        })
        .roots
    }

    #[test]
    fn breadcrumb_walks_the_assembled_tree() {
        let path = ancestor_path(&sample_tree(), Id::from_u128(3)).expect("node is in the tree");
        let names: Vec<&str> = path.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Binders", "Trade Binder", "Foils"]);
        // A node the cached tree doesn't know is None, not a truncated path.
        assert_eq!(ancestor_path(&sample_tree(), Id::from_u128(99)), None);
    }

    #[test]
    fn a_folder_row_shows_the_sidebar_rollup() {
        // Trade Binder holds 120 itself and 18 under Foils, so its own badge is
        // 138 while the Foils folder row under it reads 18 — the rolled-up
        // number in both cases, which is what the sidebar shows for each node.
        let t = sample_tree();
        assert_eq!(rolled_up_of(&t, Id::from_u128(2)), Some(138));
        assert_eq!(rolled_up_of(&t, Id::from_u128(3)), Some(18));
        // A collection the cached tree predates gets no badge, not a wrong one.
        assert_eq!(rolled_up_of(&t, Id::from_u128(99)), None);
    }

    fn tree_facts_of(nodes: &[TreeNode], id: Id) -> Option<TreeFacts> {
        find_tree_node(nodes, id).map(|node| TreeFacts {
            id,
            name: node.row.summary.name.clone(),
            children: node
                .children
                .iter()
                .map(|c| c.row.summary.clone())
                .collect(),
        })
    }

    #[test]
    fn folder_rows_prefer_the_tree_and_fall_back_to_the_payload() {
        // P6-127. The tree is the fresher of the two reads for the two tree
        // mutations that no longer refetch this page's payload: a rename
        // (relabels a row) and a `New binder inside…` (adds one).
        let t = sample_tree();
        let trade = Id::from_u128(2);
        let payload = vec![tree_row(3, Some(2), "STALE NAME", 18).summary];

        let facts = tree_facts_of(&t, trade);
        let rows = folder_rows(facts.as_ref(), trade, &payload);
        assert_eq!(
            rows.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["Foils"],
            "the tree knows this node, so its children win over the payload's"
        );

        // A collection the cached tree predates, or a tree read that failed:
        // the payload's own `children` is what this row set was before P6-127,
        // so the page is left exactly as complete as it used to be.
        assert_eq!(folder_rows(None, trade, &payload), payload);
        let other = tree_facts_of(&t, Id::from_u128(3));
        assert_eq!(
            folder_rows(other.as_ref(), trade, &payload),
            payload,
            "facts describing a *different* collection are never used — the \
             tree resolves from cache before a navigation's payload lands"
        );
    }

    #[test]
    fn teardown_destinations_are_path_labelled_and_exclude_the_deck() {
        let dests = flatten_destinations(&sample_tree(), Id::from_u128(2));
        assert_eq!(
            dests
                .iter()
                .map(|(_, label)| label.as_str())
                .collect::<Vec<_>>(),
            ["Binders", "Binders / Trade Binder / Foils"]
        );
        // Emptying a deck into itself is a no-op the API would happily perform.
        assert!(!dests.iter().any(|(id, _)| *id == Id::from_u128(2)));
    }
}
