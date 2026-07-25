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
//! header's own count follows a page-level delta so the two never disagree
//! on screen.
//!
//! **A deck is the same page with three differences** (spec): a header card
//! for format + commanders, cards grouped by board and type with slot counts,
//! and Want as the add default. The teardown action is the fourth.
//!
//! **The URL is the whole view state** — `?q=` (in-collection quick search)
//! and `?cursor=` (keyset page), same contract as `/my`.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_params_map, use_query_map};
use shared::{Board, CardRow, CardSummary, CollectionKind, CollectionView, Id, QuickAddKind};
use std::collections::HashSet;

use super::tree::{assemble, CollectionTreeResource, TreeNode};
use crate::cards::CardPreview;
use crate::components::query_bar::QueryBar;
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::breadcrumb::{
    Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator,
};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::count_stepper::{CountStepper, StepperCommit};
use crate::components::ui::dialog::{
    Dialog, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader,
    DialogTitle,
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

    let view_res = Resource::new(
        move || (url_id.get(), url_q.get(), url_cursor.get()),
        |(id, q, cursor)| async move {
            let id = Id::parse_str(&id).map_err(|_| {
                ServerFnError::<String>::ServerError("that is not a collection id".into())
            })?;
            let cursor = (!cursor.is_empty()).then_some(cursor);
            crate::collection_view(id, q, cursor).await
        },
    );

    // Copies committed through the steppers since this page loaded. The header
    // adds it so HERE and "N here" cannot disagree without a reload (see the
    // module doc on why a commit does not refetch the view).
    let here_delta = RwSignal::new(0);
    // A new page load (or a re-search) starts the delta over — the freshly
    // fetched totals already include everything committed before it.
    Effect::new(move |_| {
        let _ = (url_id.get(), url_q.get(), url_cursor.get());
        here_delta.set(0);
    });

    let paged = Memo::new(move |_| !url_cursor.read().is_empty());
    let teardown_open = RwSignal::new(false);

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
                    match view_res.await {
                        Ok(view) => {
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
                        Err(e) => view! { <LoadError e /> }.into_any(),
                    }
                })}
            </Transition>

            <div class="flex items-center gap-2">
                <div class="min-w-0 flex-1">
                    <QueryBar
                        text=query_text
                        url_q
                        // A new search starts at page one: carrying the old
                        // cursor forward pages into a set that no longer exists.
                        to_url=Callback::new(move |q: String| {
                            collection_url(&url_id.get_untracked(), &q, None)
                        })
                        id="collection-query"
                        placeholder="Search this collection or add cards…"
                        aria_label="Search this collection"
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
                        match view_res.await {
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
fn assembled_roots(
    dto: Option<Result<shared::CollectionTree, ServerFnError<String>>>,
) -> Vec<TreeNode> {
    match dto {
        Some(Ok(t)) => assemble(t).roots,
        _ => Vec::new(),
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
fn LoadError(e: ServerFnError<String>) -> impl IntoView {
    view! {
        <p
            role="alert"
            data-testid="collection-error"
            class="border-destructive/40 bg-destructive/10 text-destructive rounded-md border px-3 py-2 text-sm"
        >
            {format!("Couldn't load this collection: {}", message_of(&e))}
        </p>
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
struct Crumb {
    id: Id,
    name: String,
}

/// The chain of collections from the top level down to `id`, inclusive.
/// `None` when the tree does not contain the node (a fresh collection the
/// cached tree predates, or no tree at all) — callers fall back to the
/// collection's own name rather than rendering half a path.
fn ancestor_path(nodes: &[TreeNode], id: Id) -> Option<Vec<Crumb>> {
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
fn needs_chip(totals: &shared::CollectionTotals) -> Option<String> {
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
    view_res: Resource<Result<CollectionView, ServerFnError<String>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    let id = view.collection.id;
    let name = view.collection.name.clone();
    let kind = view.collection.kind;
    let format = view.collection.format.clone();
    let totals = view.totals;
    let commanders = view.commanders.clone();
    let chip = needs_chip(&totals);

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
                <Show when=move || kind == CollectionKind::Deck>
                    <Button
                        variant=ButtonVariant::Outline
                        attr:data-testid="teardown-open"
                        on:click=move |_| teardown_open.set(true)
                    >
                        "Empty deck…"
                    </Button>
                </Show>
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
                // roots sit.
                let (back_href, back_label) = match crumbs.len() {
                    0 | 1 => ("/my".to_string(), "All cards".to_string()),
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

#[component]
fn CollectionTable(
    view: CollectionView,
    folders: Vec<shared::CollectionSummary>,
    here_delta: RwSignal<i32>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    let is_deck = view.collection.kind == CollectionKind::Deck;
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
                        <TableHead>"Card"</TableHead>
                        <TableHead class="hidden md:table-cell">"Type"</TableHead>
                        <TableHead class="hidden sm:table-cell">"Mana"</TableHead>
                        <TableHead class="text-right">"Here"</TableHead>
                        <TableHead class="text-right">"Wanted"</TableHead>
                        <TableHead class="text-right">"Owned"</TableHead>
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
                                view! {
                                    <TableRow {..} data-testid="deck-section">
                                        <TableCell
                                            class="text-muted-foreground bg-muted/40 p-2 text-xs font-semibold tracking-wide uppercase"
                                            {..}
                                            colspan="6"
                                            data-section=label_attr
                                        >
                                            {label} " · " {slots.to_string()}
                                        </TableCell>
                                    </TableRow>
                                    {section
                                        .rows
                                        .into_iter()
                                        .map(|row| view! { <CardTableRow row here_delta /> })
                                        .collect_view()}
                                }
                            })
                            .collect_view()
                            .into_any()
                    } else {
                        rows.into_iter()
                            .map(|row| view! { <CardTableRow row here_delta /> })
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
                class="text-muted-foreground p-2 text-right italic tabular-nums"
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
            <TableCell class="p-2 text-right">""</TableCell>
            <TableCell class="p-2 text-right">""</TableCell>
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
fn CardTableRow(row: ViewRow, here_delta: RwSignal<i32>) -> impl IntoView {
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
        faces,
        ..
    } = row.row;

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
        >
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
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="here-cell">
                <div class="flex items-center justify-end gap-1">
                    <HereCount name=name.clone() present holding_id here_delta />
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
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="wanted-count">
                {wanted.map(|n| n.to_string()).unwrap_or_else(|| "—".to_string())}
            </TableCell>
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="owned-count">
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
    holding_id: Option<Id>,
    here_delta: RwSignal<i32>,
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

    let value = RwSignal::new(present);
    let toast = expect_context::<ToastHandle>();
    let tree = expect_context::<CollectionTreeResource>().0;
    let on_commit = Callback::new(move |c: StepperCommit| {
        // Optimistic on both numbers at once: the stepper already wrote `value`,
        // so the header must move with it or the two disagree on screen.
        here_delta.update(|d| *d += c.to - c.from);
        spawn_local(async move {
            match crate::set_holding_quantity(holding_id, c.to).await {
                Ok(()) => {
                    // The sidebar badges are a different read; refresh them.
                    // The *view* is deliberately not refetched — see the module
                    // doc (it would dispose the stepper mid-undo).
                    tree.refetch();
                }
                Err(e) => {
                    value.set(c.from);
                    here_delta.update(|d| *d -= c.to - c.from);
                    toast.show(
                        ToastOptions::message(format!("Couldn't save: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        <CountStepper
            value
            label=name
            on_commit
            class="justify-end"
        />
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
    view_res: Resource<Result<CollectionView, ServerFnError<String>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // "" = return to previous locations; otherwise a collection id.
    let destination = RwSignal::new(String::new());

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
                    toast.show(ToastOptions::message(format!(
                        "Emptied — {} card{} moved",
                        receipt.moves,
                        if receipt.moves == 1 { "" } else { "s" },
                    )));
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
                                <option value="">"Their previous locations"</option>
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
                            "\"Their previous locations\" sends each card back to the collection it was last moved here from — Inbox where there is no history."
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
