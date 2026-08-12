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
//! elsewhere. Their counts come from the shell's collection-tree resource, not
//! from a second read, so a folder row and the sidebar badge for the same node
//! cannot disagree. The breadcrumb (and the mobile back link) walk that same
//! assembled tree.
//!
//! **HERE is editable and the header follows it.** A card cell backed by
//! exactly one `holdings` row carries the [`CountStepper`]; a cell that sums
//! several finish/condition/language grains does not, because a lone number
//! cannot say which grain it meant (`CardRow::holding_id` encodes that). A
//! commit does **not** refetch the view: remounting the row would dispose the
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
//! instance. Two consequences land in *this* file. The page takes
//! `TreeManage::revision` as a resource source, because a rename/create/move
//! changes what `collection_view` says and no tree refetch can tell it. And
//! deleting *this* collection navigates up instead of leaving the page on a
//! dead id — `tree_manage::route_after_delete`. Only this one: deleting an
//! ancestor no longer takes the route with it, because the children survive by
//! moving up a level (specs/collection-deletion.md).

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_params_map, use_query_map};
use shared::{Board, CardRow, CardSummary, CollectionKind, CollectionView, Id, QuickAddKind};
use std::collections::HashSet;

use super::tree::{assemble, element_anchor, CollectionTreeResource, TreeNode};
use super::tree_manage::{MenuTarget, TreeManage, TreeMenu};
use crate::cards::CardPreview;
use crate::catalog::destination::Destination;
use crate::components::quick_add::{present_matches, PresentMatch, QuickAddPanel};
use crate::components::states::ErrorNote;
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::breadcrumb::{
    Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator,
};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::context_menu::{use_context_menu, ContextMenu};
use crate::components::ui::count_stepper::{CountStepper, StepperCommit};
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

/// The keyset page cursor, in the URL beside `?q=`.
const CURSOR_PARAM: &str = "cursor";

/// Build `/my/collections/{id}?q=…&cursor=…`, omitting empty parts — the single
/// place such a URL is constructed, so the query bar, the clear button and the
/// pager cannot drift on its canonical form.
fn collection_url(id: &str, q: &str, cursor: Option<&str>) -> String {
    let mut url = format!("/my/collections/{id}");
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

    let query_text = RwSignal::new(url_q.get_untracked());
    let tree = expect_context::<CollectionTreeResource>().0;

    // The selection tray's batch move writes to *this* collection's rows from
    // the shell, where it has no handle on this resource. Taking the revision
    // as a source makes the refetch structural: a move bumps it, the resource
    // re-runs, HERE and the totals move with the database.
    let revision = crate::my::move_selection::holdings_revision();
    // The same trick for the *collection tree's* mutations, and the header kebab
    // is what made it necessary: this page's title, counts and folder rows all
    // come from `collection_view`, which no `tree.refetch()` can update. A rename
    // from the header left the `<h1>` stale beside a breadcrumb that had already
    // caught up; a `New binder inside…` added a folder row that never appeared.
    // See `TreeManage::revision`.
    let manage = expect_context::<TreeManage>();

    let view_res = Resource::new(
        move || {
            (
                url_id.get(),
                url_q.get(),
                url_cursor.get(),
                revision.get(),
                manage.revision.get(),
            )
        },
        |(id, q, cursor, _revision, _tree_revision)| async move {
            let id = Id::parse_str(&id).map_err(|_| {
                // `validation:` deliberately — the wire vocabulary is what the
                // UI classifies on, and a malformed id in the URL is a *request*
                // failure that will never resolve. Unprefixed it read as a
                // transport failure and the error arm offered a "Try again" that
                // re-parsed the same broken string forever.
                ServerFnError::<String>::ServerError(
                    "validation: that is not a collection id".into(),
                )
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

    let paged = Memo::new(move |_| !url_cursor.read().is_empty());
    let teardown_open = RwSignal::new(false);

    // Memos, not raw reads: the panel re-renders on every keystroke.
    let quick_add_destination =
        Memo::new(move |_| live_facts.with(|f| f.as_ref().map(|f| f.destination.clone())));
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
            // derived (breadcrumb, folder counts, teardown destinations) awaits
            // it in its own nested boundary instead, so a `tree.refetch()` —
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
                    match payload {
                        Ok(view) => {
                            facts.set(Some(quick_add_facts(&view)));
                            view! {
                                <CollectionHeader
                                    view
                                    here_delta
                                    teardown_open
                                    view_res
                                    tree
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
                            view! { <LoadError e view_res paged url_id url_q /> }.into_any()
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
                        to_url=Callback::new(move |q: String| {
                            collection_url(&url_id.get_untracked(), &q, None)
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
            </div>

            <Transition fallback=|| {
                view! { <RowsSkeleton /> }
            }>
                {move || {
                    let q = url_q.get();
                    let id = url_id.get();
                    Suspend::new(async move {
                        match view_res.await.map(|p| p.collection_view) {
                            Ok(view) => {
                                let next = view.next_cursor.clone();
                                let searching = !q.is_empty();
                                // A search filters *cards*; child collections
                                // are not what you typed a card name to find,
                                // so they step aside while one is running. Their
                                // identity comes from the view's own `children`
                                // (ordered by position, name, like the tree);
                                // only the rolled-up badge needs the tree.
                                let folders = if searching {
                                    Vec::new()
                                } else {
                                    view.children.clone()
                                };
                                let body = if view.cards.is_empty() && folders.is_empty() {
                                    view! { <EmptyState searching paged /> }.into_any()
                                } else {
                                    view! { <CollectionTable view folders here_delta tree /> }
                                        .into_any()
                                };
                                view! {
                                    {body}
                                    <Pager next paged q id />
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
        </div>
    }
}

/// The assembled tree's roots, or an empty forest when the shell has no tree
/// (anonymous, or the read failed). Every consumer here degrades to "no
/// breadcrumb / no folder counts" rather than failing the page.
pub(crate) fn assembled_roots(
    dto: Option<Result<shared::CollectionTree, ServerFnError<String>>>,
) -> Vec<TreeNode> {
    match dto {
        Some(Ok(t)) => assemble(t).roots,
        _ => Vec::new(),
    }
}

/// The human-facing half of a server-fn error (the transport only carries
/// `ApiError`'s `Display` string).
pub(crate) fn message_of(e: &ServerFnError<String>) -> String {
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
    e: ServerFnError<String>,
    view_res: Resource<Result<CollectionViewPayload, ServerFnError<String>>>,
    paged: Memo<bool>,
    url_id: Memo<String>,
    url_q: Memo<String>,
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
                    href=move || collection_url(&url_id.get(), &url_q.get(), None)
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
            let typing = leptos::prelude::document()
                .active_element()
                .is_some_and(|el| {
                    let tag = el.tag_name().to_ascii_lowercase();
                    tag == "input"
                        || tag == "textarea"
                        || tag == "select"
                        || el
                            .dyn_ref::<leptos::web_sys::HtmlElement>()
                            .is_some_and(|h| h.is_content_editable())
                });
            if typing {
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

/// A node's own copies plus every descendant's — the number its sidebar badge
/// shows, which is what a folder row must agree with. `None` when the tree does
/// not contain the node (a collection created since the shell's tree was
/// fetched); the row then shows no badge rather than a wrong one.
fn rolled_up_of(nodes: &[TreeNode], id: Id) -> Option<i64> {
    for n in nodes {
        if n.row.summary.id == id {
            return Some(n.rolled_up);
        }
        if let Some(hit) = rolled_up_of(&n.children, id) {
            return Some(hit);
        }
    }
    None
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
fn counts_summary(totals: &shared::CollectionTotals, delta: i32) -> String {
    let own = totals.present + delta;
    let here = own + totals.present_rollup;
    let mut out = format!("{here} here");
    if totals.present_rollup > 0 {
        out.push_str(&format!(
            " ({own} own + {} rolled up)",
            totals.present_rollup
        ));
    }
    if totals.desired > 0 {
        out.push_str(&format!(" · {} wanted", totals.desired));
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

#[component]
fn CollectionHeader(
    view: CollectionView,
    here_delta: RwSignal<i32>,
    teardown_open: RwSignal<bool>,
    view_res: Resource<Result<CollectionViewPayload, ServerFnError<String>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    let id = view.collection.id;
    let name = view.collection.name.clone();
    let kind = view.collection.kind;
    let format = view.collection.format.clone();
    let totals = view.totals;
    let commanders = view.commanders.clone();
    let chip = needs_chip(&totals);

    // ---- what the header kebab aims the shared tree menu at ----
    //
    // The subject is *this route's* collection, snapshotted when the menu opens
    // — the same discipline `DeleteReq`/`MoveReq` follow, and for a sharper
    // reason here: `menu_target` is one signal shared with the sidebar's rows, so
    // a snapshot that outlived its aim would act on whatever was right-clicked
    // last.
    let manage = expect_context::<TreeManage>();
    let collection = StoredValue::new(view.collection.clone());
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
    // count must not overstate that. `children.len()` is the same read's
    // **immediate** children, which is what actually re-parents; sourcing it
    // from `collection_view` rather than the sidebar tree is the fix for the
    // stale/failed-tree-read gap (specs/collection-deletion.md Problem
    // section, absorbed P6-111): this is the very payload that already
    // renders the header, so it cannot disagree with what's on screen the
    // way a second, independently-fetched tree read could.
    let cards_here = i64::from(totals.present);
    let wants_here = i64::from(totals.desired);
    let children_here = view.children.len() as i64;
    let aim = Callback::new(move |()| {
        let roots = assembled_roots(tree.get_untracked().flatten());
        manage.menu_target.set(Some(MenuTarget::for_collection(
            &collection.get_value(),
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
                        <h1 class="text-2xl font-bold" data-testid="collection-title">
                            {name.clone()}
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
                        {move || counts_summary(&totals, here_delta.get())}
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
                .map(|text| {
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
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
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
}

/// Decide, in document order, which rows print WANTED.
fn view_rows(rows: Vec<CardRow>) -> Vec<ViewRow> {
    let mut seen: HashSet<(Id, Board)> = HashSet::new();
    rows.into_iter()
        .map(|row| {
            let first = seen.insert((row.oracle_id, row.board));
            ViewRow {
                show_wanted: first,
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
    let d = row.row.desired;
    (row.show_wanted && d > 0 && d != row.row.present).then_some(d)
}

/// The OWNED cell, per the spec's "collapses when equal to HERE". `owned` is
/// the global per-oracle total; when it is exactly what this collection's
/// subtree holds, printing it again says nothing.
fn owned_cell(row: &CardRow) -> Option<i32> {
    (row.owned != here_total(row)).then_some(row.owned)
}

/// Whether `HereCount::on_commit` should drop a commit rather than act on it.
///
/// `CountStepper`'s own built-in commit-toast carries an Undo that re-fires
/// `on_commit` later, and its only guard is "is the row's `value` signal still
/// live" — true here, because removal deliberately does **not** dispose the
/// row (that is what keeps the *removal's own* Undo toast reachable). So a
/// "3 → 1" toast raised before a removal can still fire after it, and without
/// this check `on_commit` would post the reversed count to the holding
/// `remove_holding` just deleted — a write to a dead id, surfaced as a bogus
/// "Couldn't save: not found: holding" error toast (app-ui.md → Findings).
///
/// Read at the moment a commit arrives, not once: while the row is live this
/// must return `false` for the *real* first commit too, or nothing could ever
/// be typed into the stepper.
fn stale_commit_should_be_dropped(row_removed: bool) -> bool {
    row_removed
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

/// The section header's slot-count delta for one row's present-copy change,
/// from `old` to `new`, given the row's own `desired` (P6-118 review round 1
/// — the first cut of this fix pushed the raw *copy* delta and was wrong).
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
/// `desired` is read at commit time from the row itself (`CardRow::desired`
/// is oracle-grained, so every row already carries the group's true value —
/// no cross-row aggregation needed). This is deliberately per-row, not
/// per-`(oracle, board)`-group: a row's own commit only ever changes its own
/// `present`, so treating this row's held count as standing in for the
/// group's is exact whenever the row is the group's only holding (the
/// overwhelmingly common case). For a wanted card held under two printings
/// in one section it is an approximation introduced *here* (`section_slots`
/// re-sums the whole group and is exact): the delta under-shoots — never
/// overshoots, since `max(·, desired)` grows at most 1:1 with a row-local
/// change — so the header is never *worse* than the static value it
/// replaces, and the next refetch makes it exact.
fn section_slot_delta(old: i32, new: i32, desired: i32) -> i32 {
    new.max(desired) - old.max(desired)
}

#[component]
fn CollectionTable(
    view: CollectionView,
    folders: Vec<shared::CollectionSummary>,
    here_delta: RwSignal<i32>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    let is_deck = view.collection.kind == CollectionKind::Deck;
    let collection_id = view.collection.id;
    let rows = view_rows(view.cards);
    let sections = if is_deck {
        group_deck(rows.clone())
    } else {
        Vec::new()
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
                        <TableHead class="hidden md:table-cell">"Type"</TableHead>
                        <TableHead class="hidden sm:table-cell">"Mana"</TableHead>
                        <TableHead class="px-1 text-right md:px-2">"Here"</TableHead>
                        <TableHead class="px-1 text-right md:px-2">"Wanted"</TableHead>
                        <TableHead class="px-1 text-right md:px-2">"Owned"</TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    // Child collections first — the wireframe's folder rows, in
                    // the same table and the same columns as the cards.
                    {folders
                        .into_iter()
                        .map(|folder| view! { <FolderTableRow folder tree /> })
                        .collect_view()}
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
                                            view! {
                                                <CardTableRow
                                                    row
                                                    here_delta
                                                    collection_id
                                                    section_delta=section_delta
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
                            .map(|row| view! { <CardTableRow row here_delta collection_id /> })
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
/// stepper (see [`CollectionPage`]).
#[component]
fn FolderTableRow(
    folder: shared::CollectionSummary,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    let id = folder.id;
    view! {
        <TableRow {..} data-testid="folder-row" data-collection=id.to_string()>
            // No checkbox: the selection tray moves cards, not collections.
            // Padding still tracks the card rows' select cell, or the two row
            // kinds would size the column differently.
            <TableCell class="p-0 md:p-2">""</TableCell>
            <TableCell class="p-2">
                <a
                    href=format!("/my/collections/{id}")
                    class="flex items-center gap-2 font-medium hover:underline"
                >
                    <span aria-hidden="true">
                        {if folder.kind == CollectionKind::Deck { "🃏" } else { "📁" }}
                    </span>
                    {folder.name}
                </a>
            </TableCell>
            <TableCell class="hidden p-2 md:table-cell">""</TableCell>
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

/// A zero reads as absence, not as a number worth aligning against.
fn count_or_dash(n: i32) -> String {
    if n > 0 {
        n.to_string()
    } else {
        "—".to_string()
    }
}

#[component]
fn CardTableRow(
    row: ViewRow,
    here_delta: RwSignal<i32>,
    collection_id: Id,
    /// This row's deck section, when it has one — threaded straight through to
    /// [`HereCount`] so a section header can react to this row's own commits
    /// and removals the same way the page header already reacts via
    /// `here_delta`. `None` in a binder, which has no sections.
    #[prop(optional)]
    section_delta: Option<RwSignal<i32>>,
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
        desired,
        owned: owned_total,
        present_rollup,
        board,
        holding_id,
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
            <TableCell class="text-muted-foreground hidden p-2 md:table-cell">
                {type_line.unwrap_or_default()}
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
                        desired
                        holding_id
                        here_delta
                        section_delta
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
                {wanted.map(|n| n.to_string()).unwrap_or_else(|| "—".to_string())}
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
#[component]
fn HereCount(
    name: String,
    present: i32,
    /// The row's own oracle-grained desire count — needed only to compute
    /// [`section_slot_delta`] correctly (a wanted, under-held row's slot
    /// count does not move copy-for-copy with its present count).
    desired: i32,
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
    /// Owned by the caller (`CardTableRow`), not here, so the row's selection
    /// checkbox can react to the same flag `remove`/`undo_removal` below
    /// flip — see that component's doc.
    removed: RwSignal<bool>,
) -> impl IntoView {
    let Some(holding_id) = holding_id else {
        // Either a desire-only row (nothing here to step) or a cell summing
        // several finish/condition/language grains, which one number cannot
        // address — `title` says which to anyone who wonders.
        let title = if present > 0 {
            "several finishes or conditions here — edit them individually"
        } else {
            "wanted here, not held"
        };
        return view! {
            <span class="text-muted-foreground" data-testid="here-count" title=title>
                {count_or_dash(present)}
            </span>
        }
        .into_any();
    };

    // A signal, not a plain `Id`: undoing a removal re-inserts the holding
    // under a *new* id, and `remove`/`on_commit` below read this at call time
    // rather than capturing it once, so `undo_removal` can rewire it in place.
    // Before this the row kept posting to the dead pre-removal id until an
    // unrelated refetch remounted it — a real window where a +/- during it
    // failed with "not found: holding" (app-ui.md → Findings).
    let holding_id = RwSignal::new(holding_id);
    let value = RwSignal::new(present);
    // `removed` arrives as a prop now (owned by `CardTableRow`, which also
    // reads it for the selection checkbox — P6-118, app-ui.md → Findings).
    // What it's *for* here is unchanged: the stepper is withdrawn rather than
    // left showing 0, because every further write it could issue is
    // addressed at a row that no longer exists.
    let toast = expect_context::<ToastHandle>();
    let tree = expect_context::<CollectionTreeResource>().0;
    // Bumping this refetches the page's view (it is one of the resource's
    // sources), refreshing the row's other cells (WANTED/OWNED) and totals
    // from the database. It no longer supplies the row's *id* — see
    // `undo_removal`, which rewires `holding_id` directly from the server's
    // receipt so the stepper is never blocked on this refetch landing.
    let revision = use_context::<crate::my::move_selection::HoldingsRevision>();
    // A removal is a move with no destination, so ⌘K's `Undo last move`
    // reverses it too (see `crate::components::palette`).
    let last_move = use_context::<crate::components::palette::LastMoveState>();

    let label = StoredValue::new(name.clone());

    // Reverse the removal through the move ledger — the copies come back at the
    // grain and on the board they left, which is the whole reason the removal is
    // a move rather than a delete.
    let undo_removal = move |move_id: Id, copies: i32| {
        // The palette must stop offering the same reversal (`forget`'s doc).
        crate::components::palette::forget_last_move(last_move, &[move_id]);
        spawn_local(async move {
            match crate::undo_move(move_id).await {
                Ok(receipt) => {
                    // `try_*` throughout: a toast outlives its row, so this can
                    // run after a navigation disposed these signals. The
                    // refetch below is what actually restores the truth on
                    // screen; these are the optimistic half.
                    let _ = here_delta.try_update(|d| *d += copies);
                    if let Some(d) = section_delta {
                        let _ = d.try_update(|d| *d += section_slot_delta(0, copies, desired));
                    }
                    let _ = removed.try_set(false);
                    let _ = value.try_set(copies);
                    // Rewire to the *live* id immediately — undoing a removal
                    // re-inserts the holding under a new one, and the server
                    // just told us which. This closes the race the `revision`
                    // bump below used to leave open: a +/- landing before that
                    // refetch's remount no longer addresses a dead id.
                    if let Some(new_id) = receipt.restored_holding_id {
                        let _ = holding_id.try_set(new_id);
                    }
                    tree.refetch();
                    // Still bumped for the row's *other* cells and the header:
                    // WANTED/OWNED and the totals come from `collection_view`,
                    // which this refetches. The id no longer depends on it.
                    if let Some(r) = revision {
                        r.bump();
                    }
                }
                Err(e) => {
                    toast.show(
                        ToastOptions::message(format!("Couldn't undo: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    // A committed 0. The optimistic write has already happened (the stepper
    // wrote `value`), and the stepper raised nothing — `caller_reports` claimed
    // this commit — so the message and the undo are both this callback's.
    let remove = move |copies: i32| {
        here_delta.update(|d| *d -= copies);
        if let Some(d) = section_delta {
            d.update(|d| *d += section_slot_delta(copies, 0, desired));
        }
        removed.set(true);
        spawn_local(async move {
            // `try_*`, like everywhere else in this file: a toast (or now, a
            // signal read at the top of a spawned future) can outlive its row.
            let Some(id) = holding_id.try_get_untracked() else {
                return;
            };
            match crate::remove_holding(id).await {
                Ok(move_id) => {
                    // The view is deliberately *not* refetched here: it would
                    // unmount this row and take the Undo below with it. The
                    // sidebar badges are a different read, so they refresh.
                    tree.refetch();
                    crate::components::palette::note_last_move(last_move, vec![move_id]);
                    let copies_label = if copies == 1 {
                        "1 copy".to_string()
                    } else {
                        format!("{copies} copies")
                    };
                    toast.show(
                        ToastOptions::message(format!(
                            "Removed {} ({copies_label})",
                            label.get_value()
                        ))
                        .kind(ToastKind::Success)
                        .action(
                            "Undo",
                            Callback::new(move |()| undo_removal(move_id, copies)),
                        ),
                    );
                }
                Err(e) => {
                    removed.set(false);
                    value.set(copies);
                    here_delta.update(|d| *d += copies);
                    if let Some(d) = section_delta {
                        d.update(|d| *d += section_slot_delta(0, copies, desired));
                    }
                    toast.show(
                        ToastOptions::message(format!("Couldn't remove: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    let on_commit = Callback::new(move |c: StepperCommit| {
        // See `stale_commit_should_be_dropped`: a stale count-change toast's
        // own Undo can still fire after this row is removed, and the only
        // legitimate write left at that point is the removal's own reversal —
        // which runs through `undo_removal` below, not through this callback.
        if stale_commit_should_be_dropped(removed.get_untracked()) {
            return;
        }
        if c.to == 0 {
            remove(c.from);
            return;
        }
        // Optimistic on both numbers at once: the stepper already wrote `value`,
        // so the header must move with it or the two disagree on screen. The
        // section header (when there is one) follows the same commit.
        here_delta.update(|d| *d += c.to - c.from);
        if let Some(d) = section_delta {
            d.update(|d| *d += section_slot_delta(c.from, c.to, desired));
        }
        spawn_local(async move {
            // `try_*`, like `remove` above: a signal read at the top of a
            // spawned future can outlive its row.
            let Some(id) = holding_id.try_get_untracked() else {
                return;
            };
            match crate::set_holding_quantity(id, c.to).await {
                Ok(()) => {
                    // The sidebar badges are a different read; refresh them.
                    // The *view* is deliberately not refetched — see the module
                    // doc (it would dispose the stepper mid-undo).
                    tree.refetch();
                }
                Err(e) => {
                    value.set(c.from);
                    here_delta.update(|d| *d -= c.to - c.from);
                    if let Some(d) = section_delta {
                        d.update(|d| *d += section_slot_delta(c.to, c.from, desired));
                    }
                    toast.show(
                        ToastOptions::message(format!("Couldn't save: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        // The floor is the component's own 0 again. It was 1 for two tasks
        // because a committed 0 ran `DELETE FROM holdings` while the undo the
        // stepper always offers re-POSTed the dead id — a success toast over
        // vanished copies. The floor made that unreachable and, with no per-row
        // move affordance shipped, made a binder card **impossible to remove**.
        //
        // Both halves are fixed rather than fenced off. A committed 0 now goes
        // through `remove_holding`, a move with no destination, whose ledger row
        // carries the grain and the board — so the undo is the ledger's
        // `undone_at` and gives the same copies back. And the stepper no longer
        // promises that undo itself (`caller_reports`), because it would be the
        // wrong operation: this callback owns the message and the reversal.
        <Show
            when=move || !removed.get()
            fallback=move || {
                view! {
                    <span
                        class="text-muted-foreground"
                        data-testid="here-count"
                        title="removed — Undo from the toast, or reload to see the card gone"
                    >
                        "—"
                    </span>
                }
            }
        >
            <CountStepper
                value
                label=label.get_value()
                on_commit
                caller_reports=Callback::new(|c: StepperCommit| c.to == 0)
                class="justify-end"
            />
        </Show>
    }
    .into_any()
}

/// Keyset paging controls — forward-only, for the reason `/my`'s are
/// (a cursor describes "everything after this row").
#[component]
fn Pager(next: Option<String>, paged: Memo<bool>, q: String, id: String) -> impl IntoView {
    const LINK: &str =
        "border-input hover:bg-accent hover:text-accent-foreground rounded-md border px-3 py-1.5 text-sm";
    let start_url = collection_url(&id, &q, None);
    let next_url = next.as_deref().map(|c| collection_url(&id, &q, Some(c)));

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
    view_res: Resource<Result<CollectionViewPayload, ServerFnError<String>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
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
            faces: vec![],
        }
    }

    #[test]
    fn url_omits_empty_parts_and_encodes() {
        assert_eq!(collection_url("abc", "", None), "/my/collections/abc");
        assert_eq!(collection_url("abc", "", Some("")), "/my/collections/abc");
        assert_eq!(
            collection_url("abc", "bolt", None),
            "/my/collections/abc?q=bolt"
        );
        assert_eq!(
            collection_url("abc", "", Some("cur")),
            "/my/collections/abc?cursor=cur"
        );
        assert_eq!(
            collection_url("abc", "fire // ice", Some("c d")),
            "/my/collections/abc?q=fire%20%2F%2F%20ice&cursor=c%20d"
        );
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
    fn stale_commits_are_dropped_only_once_the_row_is_removed() {
        // The defect: a "3 → 1" count-change toast's own Undo firing after the
        // row it targets was removed must not turn into a write.
        assert!(stale_commit_should_be_dropped(true));
        // A live row's commits — including its very first one — must never be
        // dropped, or the stepper could never save anything.
        assert!(!stale_commit_should_be_dropped(false));
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

    #[test]
    fn section_slot_delta_is_a_plain_copy_delta_when_nothing_is_desired() {
        // `desired == 0` is the case the first (wrong) cut of this fix
        // implicitly assumed applied everywhere: here the slot delta and the
        // copy delta genuinely coincide.
        assert_eq!(section_slot_delta(3, 1, 0), -2);
        assert_eq!(section_slot_delta(0, 3, 0), 3);
        assert_eq!(section_slot_delta(3, 0, 0), -3);
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
            counts_summary(&totals(102, 18, 6, 0, 0), 0),
            "120 here (102 own + 18 rolled up) · 6 wanted"
        );
        // No children, nothing wanted: just the one number.
        assert_eq!(counts_summary(&totals(7, 0, 0, 0, 0), 0), "7 here");
        // A committed stepper edit moves the header without a refetch.
        assert_eq!(counts_summary(&totals(7, 0, 0, 0, 0), 3), "10 here");
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
