//! `/my/collections/:id/needs` — what a collection is missing, and where to get
//! it (specs/app-ui.md → `/my/collections/:id/needs`).
//!
//! Four things are worth knowing before editing this file.
//!
//! **The page is about *acquisition*, and its arithmetic is board-blind.**
//! `CollectionStore::needs` groups desires and holdings by `oracle_id` alone,
//! while the collection view's card rows group by `(oracle, board)`. So a deck
//! holding a card on `main` and wanting it on `side` renders a Sideboard row
//! reading `HERE — / WANTED 1` while this page shows nothing. That reads like a
//! contradiction and is not one: the deck already *has* the copy, so there is
//! nothing to pull and nothing to buy — the only outstanding action is moving it
//! between boards, which is [card-tagging](../../../specs/card-tagging.md)'s
//! quantity-preserving relabel and not a move at all. Both buckets here are
//! defined by an operation (`move_cards` into this collection, or buying), and
//! neither operation can fix a mis-boarded copy.
//!
//! That is why this page is built on the board-blind arithmetic **deliberately**
//! rather than by inheritance, and why the subtitle says "more copies than it
//! holds" instead of "unfilled slots". Making `needs()` board-aware would
//! manufacture rows whose Pull button cannot work: a pull lands copies on
//! `to_board = Main` (the ledger's `to_board` is always `main` today — see
//! specs/app-ui.md → "Undoable removal + deck teardown"), so a sideboard need
//! would survive every pull aimed at it, forever. The honest board-aware version
//! needs a board-addressed destination first; until then, "unfilled deck slot"
//! is a *different* concept from "missing copy" and only the second one is
//! shipped. Recorded rather than silently inherited.
//!
//! **The pick list is client-composed, and quantity is never the caller's.** No
//! backend read was added for it: [`allocate`] spreads each row's gap over the
//! `locations` the needs read already carries, in that read's own order
//! (quantity desc, then name), and the sum of an allocation is the row's
//! `owned_elsewhere` by construction. The server runs the *same* function over
//! its *own* fresh `needs()` read, so the number on the checklist and the number
//! that moves are the same function of the same shape — a client asking for 99
//! copies gets the allocation, not the 99.
//!
//! **A pull is grain-agnostic where the tray's move is not, on purpose.** The
//! selection tray refuses a stack holding several grains and no default one
//! ([`SkipReason::Grain`]) because a checkbox on one *row* cannot say which copy
//! it meant. Here the intent is explicit — "fill this collection's gap from that
//! collection" — so [`plan_pull`] takes copies across grains, default grain
//! first and then in a stable order, emitting one `MoveItem` per stack it draws
//! from. The ledger therefore records exactly which stacks moved and undo is
//! still exact.
//!
//! **The pick list is a snapshot, and it lives outside the payload it came
//! from.** Generating it captures the allocation once; the table above it keeps
//! tracking the database (every pull bumps the holdings revision, which is one
//! of this page's resource sources), but the checklist itself must not — a list
//! that rebuilt as you ticked it would delete the line you just ticked and
//! renumber the ones you had not reached. Mounting it inside the resource-driven
//! body made exactly that happen: the last tick emptied the needs rows, the
//! whole section unmounted, and the checklist plus its "Done" button vanished
//! mid-walk. It is therefore a page-level signal rendered outside the
//! `Transition`, cleared on close and on navigation.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde::{Deserialize, Serialize};
use shared::{CardLocation, HoldingLine, Id, NeedRow, NeedsView};
use std::collections::HashSet;

use super::collection::{ancestor_path, assembled_roots, message_of, needs_chip};
use super::move_selection::{movable, MoveSource, Skipped};
use super::tree::CollectionTreeResource;
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

// ------------------------------------------------------------ the wire ---

/// One line of a pull: a card this collection needs, and the collection to take
/// it from. Deliberately carries **no quantity** — see the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullItem {
    pub oracle_id: Id,
    pub from_collection_id: Id,
}

impl PullItem {
    /// The name a pick-list row is known by on both sides. Like
    /// [`SelectionKey::token`](crate::components::ui::selection_tray::SelectionKey::token),
    /// it exists so the server can report per-line outcomes without shipping
    /// back card names the client already holds.
    pub fn token(&self) -> String {
        format!("{}@{}", self.oracle_id, self.from_collection_id)
    }
}

/// One pick-list line that moved, and how many copies it moved.
///
/// The copy count is reported rather than inferred: a line can draw from several
/// `holdings` stacks (grains), so `move_ids.len()` counts ledger rows, not
/// copies, and a toast that used it would misstate every mixed-grain pull.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pulled {
    pub token: String,
    pub copies: i32,
}

/// What a pull did: the ledger rows it wrote (one Undo covers them all), the
/// lines that moved, and the lines refused with why.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PullOutcome {
    pub move_ids: Vec<Id>,
    pub pulled: Vec<Pulled>,
    pub skipped: Vec<Skipped>,
}

impl PullOutcome {
    /// Copies moved across every line — the number the toast states.
    pub fn copies(&self) -> i32 {
        self.pulled.iter().map(|p| p.copies).sum()
    }
}

// -------------------------------------------------------- the arithmetic ---

/// Copies to take from each location to fill one row's gap, in the needs read's
/// own location order (quantity desc, then collection name).
///
/// This is the pick list. It is a pure function of the row precisely so the page
/// that *shows* the plan and the adapter that *performs* it cannot disagree —
/// the server re-runs it against its own fresh read rather than trusting the
/// numbers the client rendered.
///
/// `sum(allocate(gap, locations)) == min(gap, sum(locations))`, which is exactly
/// [`NeedRow::owned_elsewhere`]'s definition — the invariant that makes "the
/// pick list adds up to the Owned-elsewhere bucket" true rather than hopeful.
pub fn allocate(gap: i32, locations: &[CardLocation]) -> Vec<(Id, i32)> {
    let mut remaining = gap.max(0);
    let mut out = Vec::new();
    for loc in locations {
        if remaining <= 0 {
            break;
        }
        let take = remaining.min(loc.quantity.max(0));
        if take > 0 {
            out.push((loc.collection_id, take));
            remaining -= take;
        }
    }
    out
}

/// The gap this row is trying to close: copies desired here beyond copies held
/// here. `owned_elsewhere + short` by construction.
pub fn gap_of(row: &NeedRow) -> i32 {
    (row.desired - row.present_here).max(0)
}

/// Collapse a pull request to **one line per (card, source)** — the shape
/// [`allocate`] produces, enforced rather than assumed.
///
/// Repeating a line is the one way a caller could smuggle a quantity through an
/// API that deliberately takes none. The server's plan is a fixed per-pair
/// number and the holdings it reads are not consumed between lines, so two
/// identical items would each plan the whole gap and move it twice — four copies
/// into a gap of two. A duplicate is a no-op, not a multiplier.
pub fn dedupe(items: Vec<PullItem>) -> Vec<PullItem> {
    let mut seen: HashSet<(Id, Id)> = HashSet::new();
    items
        .into_iter()
        .filter(|i| seen.insert((i.oracle_id, i.from_collection_id)))
        .collect()
}

/// One stack a pull draws copies from, and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullLine {
    pub source: MoveSource,
    pub quantity: i32,
}

/// Whether a holding sits at the grain a caller who states nothing would mean.
fn default_grain(h: &HoldingLine) -> bool {
    h.finish == shared::Finish::default()
        && h.condition == shared::Condition::default()
        && h.language == shared::default_language()
}

/// Plan the stacks one pick-list line draws from: up to `want` copies out of
/// `from`, default grain first and then in a stable order.
///
/// Unlike the selection tray's resolution this never refuses a mixed-grain
/// stack. The tray's checkbox sat on a *row* that summed grains away, so "which
/// copy did you mean" was genuinely unanswered; here the user asked to fill a
/// gap from a named collection, and every copy in it answers that equally. What
/// must not be arbitrary is *which stacks the ledger records*, and it is not:
/// one `MoveItem` per stack drawn from, at that stack's real grain and board.
pub fn plan_pull(holdings: &[HoldingLine], from: Id, want: i32) -> Vec<PullLine> {
    let mut stacks: Vec<&HoldingLine> = holdings
        .iter()
        .filter(|h| h.collection_id == from && movable(h))
        .collect();
    // Plain copies leave before foils, mainboards before sideboards — and the
    // remaining keys only exist so two runs over the same data draw the same
    // stacks. `to_pg` because none of these enums is `Ord`.
    stacks.sort_by(|a, b| {
        let key = |h: &HoldingLine| {
            (
                !default_grain(h),
                h.board.to_pg(),
                h.finish.to_pg(),
                h.condition.to_pg(),
                h.language.clone(),
                h.printing_id,
            )
        };
        key(a).cmp(&key(b))
    });

    let mut remaining = want.max(0);
    let mut out = Vec::new();
    for h in stacks {
        if remaining <= 0 {
            break;
        }
        let take = remaining.min(h.quantity);
        out.push(PullLine {
            source: MoveSource::from(h),
            quantity: take,
        });
        remaining -= take;
    }
    out
}

/// The needs page's own header counts, folded out of the rows it is showing.
///
/// Derived from the rows rather than fetched, so the sentence on this page
/// cannot disagree with the list under it; it is the *same* formatter the
/// collection header's chip uses ([`needs_chip`]), so the chip and the page it
/// links to cannot disagree either.
pub fn totals_of(rows: &[NeedRow]) -> shared::CollectionTotals {
    let missing: i32 = rows.iter().map(gap_of).sum();
    let owned_elsewhere: i32 = rows.iter().map(|r| r.owned_elsewhere).sum();
    shared::CollectionTotals {
        present: 0,
        present_rollup: 0,
        desired: rows.iter().map(|r| r.desired).sum(),
        missing,
        owned_elsewhere,
        to_buy: missing - owned_elsewhere,
    }
}

// ------------------------------------------------------------ the picks ---

/// One pick-list line as rendered: a card, how many copies to pull, and the
/// token both sides know it by.
#[derive(Debug, Clone, PartialEq)]
pub struct PickRow {
    pub item: PullItem,
    pub name: String,
    pub copies: i32,
}

/// The pick list, grouped by the collection you walk to.
#[derive(Debug, Clone, PartialEq)]
pub struct PickGroup {
    pub collection_id: Id,
    pub collection_name: String,
    pub rows: Vec<PickRow>,
}

/// Fold every row's [`allocate`] plan into one checklist grouped by source
/// collection — the physical shape of the job ("go to the Trade Binder, pull
/// these four"), which is why it groups by *where you walk* rather than by card.
pub fn pick_list(rows: &[NeedRow]) -> Vec<PickGroup> {
    let mut groups: Vec<PickGroup> = Vec::new();
    for row in rows {
        for (collection_id, copies) in allocate(gap_of(row), &row.locations) {
            let name = row
                .locations
                .iter()
                .find(|l| l.collection_id == collection_id)
                .map(|l| l.collection_name.clone())
                .unwrap_or_default();
            let pick = PickRow {
                item: PullItem {
                    oracle_id: row.oracle_id,
                    from_collection_id: collection_id,
                },
                name: row.name.clone(),
                copies,
            };
            match groups.iter_mut().find(|g| g.collection_id == collection_id) {
                Some(g) => g.rows.push(pick),
                None => groups.push(PickGroup {
                    collection_id,
                    collection_name: name,
                    rows: vec![pick],
                }),
            }
        }
    }
    groups.sort_by_key(|g| g.collection_name.to_lowercase());
    groups
}

// -------------------------------------------------------------- the page ---

#[component]
pub fn NeedsPage() -> impl IntoView {
    let params = use_params_map();
    let url_id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let tree = expect_context::<CollectionTreeResource>().0;
    let revision = super::move_selection::holdings_revision();

    let needs_res = Resource::new(
        move || (url_id.get(), revision.get()),
        |(id, _revision)| async move {
            let id = Id::parse_str(&id).map_err(|_| {
                ServerFnError::<String>::ServerError("that is not a collection id".into())
            })?;
            crate::collection_needs(id).await
        },
    );

    // The generated pick list, and which of its lines are already pulled. Both
    // live *outside* the Transition so a tick does not lose the checklist —
    // see the module doc on why this is a snapshot.
    let picks = RwSignal::new(None::<Vec<PickGroup>>);
    let done = RwSignal::new(HashSet::<String>::new());

    view! {
        <div class="flex min-w-0 flex-col gap-4 p-4 md:p-6" data-testid="needs-page">
            <NeedsHeader url_id tree />
            <Transition fallback=|| {
                view! { <RowsSkeleton /> }
            }>
                {move || Suspend::new(async move {
                    match needs_res.await {
                        Ok(view) => {
                            view! { <NeedsBody view picks /> }
                                .into_any()
                        }
                        Err(e) => {
                            view! {
                                <p
                                    role="alert"
                                    data-testid="needs-error"
                                    class="border-destructive/40 bg-destructive/10 text-destructive rounded-md border px-3 py-2 text-sm"
                                >
                                    {format!("Couldn't load these needs: {}", message_of(&e))}
                                </p>
                            }
                                .into_any()
                        }
                    }
                })}
            </Transition>
            // **Outside the Transition, deliberately.** Every pull bumps the
            // holdings revision, which is one of `needs_res`'s sources, so the
            // body above re-renders on every tick — and the last tick empties it
            // entirely. A checklist mounted in there would disappear from under
            // the hand walking it (it did: the pick list vanished mid-walk and
            // the "Done" button with it). The list belongs to the *page*, not to
            // the payload it was generated from.
            <PickListPanel url_id needs_res tree picks done />
        </div>
    }
}

/// Back link, breadcrumb and title. Its own boundary over the tree resource —
/// every write on this page refetches the tree for the sidebar badges, and the
/// rule this repo learned the hard way is that **nothing large awaits the tree**
/// (specs/app-ui.md → Findings, binder/deck view).
#[component]
fn NeedsHeader(
    url_id: Memo<String>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-1">
            <Suspense fallback=|| {
                view! { <Skeleton class="h-4 w-48" /> }
            }>
                {move || {
                    let id = url_id.get();
                    Suspend::new(async move {
                        let nodes = assembled_roots(tree.await);
                        let name = Id::parse_str(&id)
                            .ok()
                            .and_then(|id| ancestor_path(&nodes, id))
                            .and_then(|path| path.last().map(|c| c.name.clone()));
                        let href = format!("/my/collections/{id}");
                        view! {
                            <a
                                href=href
                                class="text-muted-foreground hover:text-foreground flex w-fit items-center gap-1 text-sm"
                                data-testid="needs-back"
                            >
                                <span aria-hidden="true">"‹"</span>
                                {name.unwrap_or_else(|| "Back to the collection".to_string())}
                            </a>
                        }
                    })
                }}
            </Suspense>
            <h1 class="text-2xl font-bold" data-testid="needs-title">
                "Needs"
            </h1>
            // The subtitle is load-bearing, not decoration: this page counts
            // *copies to acquire*, which is not the same question as "which deck
            // slots are unfilled" (module doc).
            <p class="text-muted-foreground text-sm">
                "Cards this collection wants more copies of than it holds."
            </p>
        </div>
    }
}

#[component]
fn RowsSkeleton() -> impl IntoView {
    view! {
        <div class="space-y-2" aria-busy="true" aria-label="Loading needs">
            {(0..6).map(|_| view! { <Skeleton class="h-10 w-full" /> }).collect_view()}
        </div>
    }
}

#[component]
fn NeedsBody(view: NeedsView, picks: RwSignal<Option<Vec<PickGroup>>>) -> impl IntoView {
    let collection_id = view.collection_id;
    let rows = view.rows;
    let summary = needs_chip(&totals_of(&rows));
    // A row contributes to both buckets when part of its gap is fillable and
    // part is not (desired 4, here 0, two elsewhere → pull two, buy two). The
    // split is per *copy*, so the row belongs in each bucket it has copies in —
    // filtering it into one would drop copies from the other's total.
    let elsewhere: Vec<NeedRow> = rows
        .iter()
        .filter(|r| r.owned_elsewhere > 0)
        .cloned()
        .collect();
    let short: Vec<NeedRow> = rows.iter().filter(|r| r.short > 0).cloned().collect();

    if rows.is_empty() {
        return view! {
            // Stated in **copies**, because copies are all this page can see.
            // "Nothing missing" was an unqualified claim it has no basis for: a
            // deck whose only unmet slots are board-level reads it as "your deck
            // is complete", and the arithmetic behind it never looked at a board
            // (module doc). The second clause is what keeps the first one true.
            <p class="text-muted-foreground py-12 text-center text-sm" data-testid="needs-empty">
                "Nothing to pull or buy — this collection holds every copy it wants. Unfilled board slots aren't counted here."
            </p>
        }
        .into_any();
    }

    view! {
        <p class="text-sm font-medium" data-testid="needs-summary">
            {summary.unwrap_or_else(|| "Nothing missing".to_string())}
        </p>
        {(!elsewhere.is_empty())
            .then(|| view! { <OwnedElsewhere rows=elsewhere collection_id picks /> })}
        {(!short.is_empty()).then(|| view! { <ShortBucket rows=short /> })}
    }
    .into_any()
}

// --------------------------------------------------------- owned elsewhere ---

#[component]
fn OwnedElsewhere(
    rows: Vec<NeedRow>,
    collection_id: Id,
    picks: RwSignal<Option<Vec<PickGroup>>>,
) -> impl IntoView {
    let pending = RwSignal::new(false);
    let total: i32 = rows.iter().map(|r| r.owned_elsewhere).sum();
    let all = StoredValue::new(rows.clone());

    let open_picks = move |_| {
        picks.set(Some(pick_list(&all.get_value())));
    };

    view! {
        <section class="flex flex-col gap-2" data-testid="needs-elsewhere">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <h2 class="text-lg font-semibold">
                    "Owned elsewhere"
                    <span class="text-muted-foreground ml-2 text-sm font-normal">
                        {format!("{total} copies you already have")}
                    </span>
                </h2>
                <Button
                    variant=ButtonVariant::Outline
                    attr:data-testid="pull-all"
                    on:click=open_picks
                >
                    "Pull all…"
                </Button>
            </div>
            <TableWrapper class="max-h-none">
                <Table {..} data-testid="needs-elsewhere-table">
                    <TableHeader>
                        <TableRow>
                            <TableHead>"Card"</TableHead>
                            <TableHead>"Where"</TableHead>
                            <TableHead class="text-right">"Need"</TableHead>
                            <TableHead class="text-right">"Pull"</TableHead>
                            <TableHead class="w-24">
                                <span class="sr-only">"Actions"</span>
                            </TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {rows
                            .into_iter()
                            .map(|row| view! { <ElsewhereRow row collection_id pending /> })
                            .collect_view()}
                    </TableBody>
                </Table>
            </TableWrapper>
        </section>
    }
}

#[component]
fn ElsewhereRow(row: NeedRow, collection_id: Id, pending: RwSignal<bool>) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    let tree = expect_context::<CollectionTreeResource>().0;
    let revision = use_context::<super::move_selection::HoldingsRevision>();
    let oracle_id = row.oracle_id;
    let name = row.name.clone();
    let gap = gap_of(&row);
    let fillable = row.owned_elsewhere;
    let locations = row.locations.clone();
    // The whole row in one tap: every source its allocation names, one
    // transaction, one Undo.
    let items = StoredValue::new(
        allocate(gap, &row.locations)
            .into_iter()
            .map(|(from_collection_id, _)| PullItem {
                oracle_id,
                from_collection_id,
            })
            .collect::<Vec<_>>(),
    );
    let label = StoredValue::new(row.name.clone());

    let pull = move |_| {
        if pending.get_untracked() {
            return;
        }
        pending.set(true);
        let items = items.get_value();
        spawn_local(async move {
            let result = crate::pull_needs(collection_id, items).await;
            pending.set(false);
            match result {
                // No explicit refetch: the holdings revision `report` bumps is
                // one of this page's resource sources, so following the
                // database is structural rather than a call someone has to
                // remember to add (the rule `/my` and the collection view
                // already follow).
                Ok(outcome) => report(&outcome, &label.get_value(), toast, tree, revision, None),
                Err(e) => {
                    toast.show(
                        ToastOptions::message(format!("Couldn't pull: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    view! {
        <TableRow {..} data-testid="needs-row" data-oracle=oracle_id.to_string()>
            <TableCell class="p-2 font-medium">
                <a href=format!("/cards/{oracle_id}") class="hover:underline">
                    {name}
                </a>
            </TableCell>
            <TableCell class="text-muted-foreground p-2 text-sm">
                <ul data-testid="need-locations">
                    {locations
                        .into_iter()
                        .map(|loc| {
                            view! {
                                <li>
                                    <a
                                        href=format!("/my/collections/{}", loc.collection_id)
                                        class="hover:underline"
                                    >
                                        {format!("{} in {}", loc.quantity, loc.collection_name)}
                                    </a>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </TableCell>
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="need-gap">
                {gap}
            </TableCell>
            <TableCell class="p-2 text-right tabular-nums" {..} data-testid="need-fillable">
                {fillable}
            </TableCell>
            <TableCell class="p-2 text-right">
                <Button
                    variant=ButtonVariant::Outline
                    attr:data-testid="pull-row"
                    attr:disabled=move || pending.get()
                    on:click=pull
                >
                    "Pull"
                </Button>
            </TableCell>
        </TableRow>
    }
}

// ------------------------------------------------------------- pick list ---

/// The checklist behind "Pull all…" — grouped by the collection you walk to,
/// one line per card, each tick recording that line's move.
#[component]
fn PickListPanel(
    url_id: Memo<String>,
    needs_res: Resource<Result<NeedsView, ServerFnError<String>>>,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
    picks: RwSignal<Option<Vec<PickGroup>>>,
    done: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let close = move |_| {
        picks.set(None);
        done.set(HashSet::new());
        // The list is gone, so the page can rebuild from the database again.
        // (Every tick already refetched it — what closing recovers is nothing;
        // this is for the case where a tick *failed* and the page is stale.)
        needs_res.refetch();
    };
    // Navigating to another collection's needs must not leave the previous
    // one's checklist on screen — its lines name a destination that is no longer
    // this page.
    Effect::new(move |_| {
        url_id.track();
        picks.set(None);
        done.set(HashSet::new());
    });

    view! {
        <Show when=move || picks.read().is_some()>
            <div
                class="bg-card flex flex-col gap-3 rounded-md border p-3"
                data-testid="pick-list"
            >
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <h3 class="font-semibold">"Pick list"</h3>
                    <Button
                        variant=ButtonVariant::Ghost
                        attr:data-testid="pick-list-close"
                        on:click=close
                    >
                        "Done"
                    </Button>
                </div>
                <p class="text-muted-foreground text-xs">
                    "Tick a card as you pull it — each tick records the move."
                </p>
                {move || {
                    let collection_id = Id::parse_str(&url_id.get()).unwrap_or_default();
                    picks
                        .get()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|group| {
                            view! { <PickGroupView group collection_id tree done /> }
                        })
                        .collect_view()
                }}
            </div>
        </Show>
    }
}

#[component]
fn PickGroupView(
    group: PickGroup,
    collection_id: Id,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
    done: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let name = group.collection_name.clone();
    let source_id = group.collection_id;
    view! {
        <div class="flex flex-col gap-1" data-testid="pick-group" data-collection=source_id.to_string()>
            <a
                href=format!("/my/collections/{source_id}")
                class="text-sm font-semibold hover:underline"
                data-testid="pick-group-name"
            >
                {name}
            </a>
            <ul class="flex flex-col gap-1">
                {group
                    .rows
                    .into_iter()
                    .map(|row| view! { <PickRowView row collection_id tree done /> })
                    .collect_view()}
            </ul>
        </div>
    }
}

#[component]
fn PickRowView(
    row: PickRow,
    collection_id: Id,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
    done: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    let revision = use_context::<super::move_selection::HoldingsRevision>();
    let busy = RwSignal::new(false);
    let token = StoredValue::new(row.item.token());
    let label = StoredValue::new(row.name.clone());
    let item = row.item;
    let checked = Signal::derive(move || done.read().contains(&token.get_value()));

    let toggle = Callback::new(move |want: bool| {
        // A tick is a write, so it is one-way: unticking would have to reverse a
        // move, and the reversal already has a name (the toast's Undo), which
        // reports failure instead of silently re-checking a box.
        if !want || busy.get_untracked() || checked.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let result = crate::pull_needs(collection_id, vec![item]).await;
            busy.set(false);
            match result {
                Ok(outcome) => {
                    let moved = !outcome.pulled.is_empty();
                    if moved {
                        done.update(|d| {
                            d.insert(token.get_value());
                        });
                    }
                    let undo_token = token.get_value();
                    report(
                        &outcome,
                        &label.get_value(),
                        toast,
                        tree,
                        revision,
                        Some(Callback::new(move |()| {
                            let undo_token = undo_token.clone();
                            done.update(|d| {
                                d.remove(&undo_token);
                            });
                        })),
                    );
                }
                Err(e) => {
                    toast.show(
                        ToastOptions::message(format!("Couldn't pull: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        <li
            class="flex items-center gap-2 text-sm"
            data-testid="pick-row"
            data-token=row.item.token()
            data-state=move || if checked.get() { "pulled" } else { "todo" }
        >
            <Checkbox
                checked
                disabled=Signal::derive(move || busy.get())
                on_checked_change=toggle
                aria_label=format!("Pull {} {}", row.copies, row.name)
            />
            <span
                class=move || {
                    if checked.get() { "text-muted-foreground line-through" } else { "" }
                }
                data-testid="pick-label"
            >
                {format!("{} × {}", row.copies, row.name)}
            </span>
        </li>
    }
}

/// The toast every pull raises, and the Undo behind it.
///
/// One place, because a pull has three outcomes worth stating — copies moved,
/// lines refused, and both at once — and two call sites (the row button and the
/// checklist tick) that must not word them differently.
fn report(
    outcome: &PullOutcome,
    label: &str,
    toast: ToastHandle,
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
    revision: Option<super::move_selection::HoldingsRevision>,
    on_undo: Option<Callback<()>>,
) {
    if !outcome.move_ids.is_empty() {
        tree.refetch();
        if let Some(r) = revision {
            r.bump();
        }
        let copies = outcome.copies();
        let move_ids = outcome.move_ids.clone();
        let copies_label = if copies == 1 {
            "1 copy".to_string()
        } else {
            format!("{copies} copies")
        };
        toast.show(
            ToastOptions::message(format!("Pulled {copies_label} of {label}"))
                .kind(ToastKind::Success)
                .action(
                    "Undo",
                    Callback::new(move |()| {
                        let move_ids = move_ids.clone();
                        spawn_local(async move {
                            match crate::undo_selection_move(move_ids).await {
                                Ok(()) => {
                                    tree.refetch();
                                    if let Some(r) = revision {
                                        r.bump();
                                    }
                                    // Only now: un-ticking the pick-list line
                                    // before the reversal lands would offer the
                                    // line again while the copies were still
                                    // moved, and a second tick would pull copies
                                    // this collection no longer needs.
                                    if let Some(cb) = on_undo {
                                        cb.run(());
                                    }
                                    toast.show(ToastOptions::message("Put them back"));
                                }
                                Err(e) => {
                                    toast.show(
                                        ToastOptions::message(format!(
                                            "Couldn't undo: {}",
                                            message_of(&e)
                                        ))
                                        .kind(ToastKind::Error),
                                    );
                                }
                            }
                        });
                    }),
                ),
        );
    }
    for skip in &outcome.skipped {
        toast.show(
            ToastOptions::message(format!("{label} {}", skip.reason.phrase()))
                .kind(ToastKind::Error),
        );
    }
}

// ----------------------------------------------------------------- short ---

/// The buy bucket — what nobody holds. Its counts are what `/my/shopping`
/// aggregates, so the row links there rather than restating a total.
#[component]
fn ShortBucket(rows: Vec<NeedRow>) -> impl IntoView {
    let total: i32 = rows.iter().map(|r| r.short).sum();
    view! {
        <section class="flex flex-col gap-2" data-testid="needs-short">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <h2 class="text-lg font-semibold">
                    "Short"
                    <span class="text-muted-foreground ml-2 text-sm font-normal">
                        {format!("{total} copies to buy")}
                    </span>
                </h2>
                <a href="/my/shopping" class="text-sm underline" data-testid="needs-shopping-link">
                    "Shopping list →"
                </a>
            </div>
            <TableWrapper class="max-h-none">
                <Table {..} data-testid="needs-short-table">
                    <TableHeader>
                        <TableRow>
                            <TableHead>"Card"</TableHead>
                            <TableHead class="text-right">"Want"</TableHead>
                            <TableHead class="text-right">"Here"</TableHead>
                            <TableHead class="text-right">"Short"</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {rows
                            .into_iter()
                            .map(|row| {
                                let oracle_id = row.oracle_id;
                                view! {
                                    <TableRow {..} data-testid="short-row" data-oracle=oracle_id.to_string()>
                                        <TableCell class="p-2 font-medium">
                                            <a
                                                href=format!("/cards/{oracle_id}")
                                                class="hover:underline"
                                            >
                                                {row.name}
                                            </a>
                                        </TableCell>
                                        <TableCell class="p-2 text-right tabular-nums">
                                            {row.desired}
                                        </TableCell>
                                        <TableCell class="p-2 text-right tabular-nums">
                                            {row.present_here}
                                        </TableCell>
                                        <TableCell
                                            class="p-2 text-right font-medium tabular-nums"
                                            {..}
                                            data-testid="short-count"
                                        >
                                            {row.short}
                                        </TableCell>
                                    </TableRow>
                                }
                            })
                            .collect_view()}
                    </TableBody>
                </Table>
            </TableWrapper>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{Board, Condition, Finish};
    use uuid::Uuid;

    fn loc(id: Id, name: &str, quantity: i32) -> CardLocation {
        CardLocation {
            collection_id: id,
            collection_name: name.to_string(),
            quantity,
        }
    }

    fn need(desired: i32, present_here: i32, locations: Vec<CardLocation>) -> NeedRow {
        let gap = desired - present_here;
        let elsewhere: i32 = locations.iter().map(|l| l.quantity).sum();
        let owned_elsewhere = elsewhere.min(gap);
        NeedRow {
            oracle_id: Uuid::from_u128(1),
            name: "Lightning Bolt".to_string(),
            desired,
            present_here,
            owned_elsewhere,
            short: gap - owned_elsewhere,
            locations,
        }
    }

    fn holding(collection: Id, quantity: i32, finish: Finish, board: Board) -> HoldingLine {
        HoldingLine {
            id: Uuid::new_v4(),
            collection_id: collection,
            printing_id: Uuid::from_u128(9),
            finish,
            condition: Condition::Nm,
            language: "en".to_string(),
            board,
            quantity,
        }
    }

    #[test]
    fn an_allocation_never_exceeds_the_gap_or_the_stock() {
        let a = Uuid::from_u128(10);
        let b = Uuid::from_u128(11);
        // Gap smaller than what is out there: fill from the first location and
        // stop — the second is not touched at all.
        assert_eq!(
            allocate(2, &[loc(a, "Trade Binder", 3), loc(b, "Shoebox", 4)]),
            vec![(a, 2)]
        );
        // Gap larger than the first location: spill into the second.
        assert_eq!(
            allocate(5, &[loc(a, "Trade Binder", 3), loc(b, "Shoebox", 4)]),
            vec![(a, 3), (b, 2)]
        );
        // Gap larger than everything: take everything, and no more.
        assert_eq!(
            allocate(9, &[loc(a, "Trade Binder", 3), loc(b, "Shoebox", 4)]),
            vec![(a, 3), (b, 4)]
        );
        assert!(allocate(0, &[loc(a, "Trade Binder", 3)]).is_empty());
        assert!(allocate(-1, &[loc(a, "Trade Binder", 3)]).is_empty());
    }

    #[test]
    fn a_pick_list_adds_up_to_the_owned_elsewhere_bucket() {
        // The invariant the page's two numbers rest on: what the checklist tells
        // you to fetch is exactly what the bucket claims you already own. Both
        // are `min(gap, Σ locations)` — but by two different routes, which is
        // why it is worth pinning.
        let a = Uuid::from_u128(10);
        let b = Uuid::from_u128(11);
        for row in [
            need(4, 0, vec![loc(a, "Trade Binder", 1), loc(b, "Shoebox", 1)]),
            need(4, 1, vec![loc(a, "Trade Binder", 9)]),
            need(2, 0, vec![loc(a, "Trade Binder", 1), loc(b, "Shoebox", 5)]),
            need(3, 3, vec![loc(a, "Trade Binder", 5)]),
        ] {
            let planned: i32 = allocate(gap_of(&row), &row.locations)
                .iter()
                .map(|(_, n)| n)
                .sum();
            assert_eq!(
                planned, row.owned_elsewhere,
                "allocation must equal the row's owned_elsewhere"
            );
        }
    }

    #[test]
    fn the_pick_list_groups_by_the_collection_you_walk_to() {
        let a = Uuid::from_u128(10);
        let b = Uuid::from_u128(11);
        let mut bolt = need(3, 0, vec![loc(b, "Shoebox", 2), loc(a, "Trade Binder", 4)]);
        bolt.oracle_id = Uuid::from_u128(1);
        let mut swan = need(1, 0, vec![loc(a, "Trade Binder", 1)]);
        swan.oracle_id = Uuid::from_u128(2);
        swan.name = "Snapcaster Mage".to_string();

        let groups = pick_list(&[bolt, swan]);
        assert_eq!(groups.len(), 2);
        // Alphabetical, so a physical walk is stable between renders.
        assert_eq!(groups[0].collection_name, "Shoebox");
        assert_eq!(groups[1].collection_name, "Trade Binder");
        // Shoebox is first in the row's own (quantity-desc) location order, so
        // it absorbs 2 of the 3 and Trade Binder gets the remaining 1 — plus
        // the whole of the second card.
        assert_eq!(groups[0].rows[0].copies, 2);
        assert_eq!(groups[1].rows.len(), 2);
        assert_eq!(groups[1].rows[0].copies, 1);
        assert_eq!(groups[1].rows[1].name, "Snapcaster Mage");
    }

    #[test]
    fn totals_fold_the_rows_the_way_the_chip_states_them() {
        let a = Uuid::from_u128(10);
        let rows = vec![
            need(4, 0, vec![loc(a, "Trade Binder", 3)]),
            need(2, 0, vec![]),
        ];
        let totals = totals_of(&rows);
        assert_eq!(totals.missing, 6);
        assert_eq!(totals.owned_elsewhere, 3);
        // `to_buy` is derived, and must equal the rows' own `short` sum or the
        // headline disagrees with the bucket under it.
        assert_eq!(totals.to_buy, rows.iter().map(|r| r.short).sum::<i32>());
    }

    #[test]
    fn a_pull_takes_plain_copies_before_foils_and_stops_at_the_gap() {
        let src = Uuid::from_u128(20);
        let holdings = vec![
            holding(src, 3, Finish::Foil, Board::Main),
            holding(src, 2, Finish::Nonfoil, Board::Main),
        ];
        let plan = plan_pull(&holdings, src, 3);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].source.finish, Finish::Nonfoil);
        assert_eq!(plan[0].quantity, 2);
        // The remainder comes off the foil stack — at the foil grain, never a
        // restated default (a `MoveItem` at the wrong grain is a write aimed at
        // copies that do not exist).
        assert_eq!(plan[1].source.finish, Finish::Foil);
        assert_eq!(plan[1].quantity, 1);
    }

    #[test]
    fn a_pull_only_draws_from_the_collection_it_names() {
        let src = Uuid::from_u128(20);
        let other = Uuid::from_u128(21);
        let holdings = vec![
            holding(other, 9, Finish::Nonfoil, Board::Main),
            holding(src, 1, Finish::Nonfoil, Board::Side),
        ];
        let plan = plan_pull(&holdings, src, 4);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].quantity, 1);
        // The board comes off the stack that was found, so undo puts the copy
        // back on the sideboard it left.
        assert_eq!(plan[0].source.board, Board::Side);
    }

    #[test]
    fn a_duplicated_pull_line_does_not_multiply_the_move() {
        // The invariant this whole design rests on is that the caller never
        // supplies a quantity — and repetition was a way to supply one anyway.
        // Modelled as the adapter composes it: one `plan_pull` per line, against
        // a per-pair allocation that is *not* decremented between lines.
        let src = Uuid::from_u128(20);
        let holdings = vec![holding(src, 4, Finish::Nonfoil, Board::Main)];
        let item = PullItem {
            oracle_id: Uuid::from_u128(1),
            from_collection_id: src,
        };
        let want = 2; // the gap, as `allocate` planned it for this pair
        let copies = |lines: &[PullItem]| -> i32 {
            lines
                .iter()
                .flat_map(|i| plan_pull(&holdings, i.from_collection_id, want))
                .map(|l| l.quantity)
                .sum()
        };

        assert_eq!(
            copies(&dedupe(vec![item, item])),
            2,
            "a duplicated line must move the gap once"
        );
        // And the hole is real, not hypothetical: the same composition without
        // the dedupe moves four copies into a gap of two.
        assert_eq!(copies(&[item, item]), 4);
        // Two *different* sources of the same card are not duplicates — that is
        // the ordinary multi-source pull and it must survive.
        let other = PullItem {
            from_collection_id: Uuid::from_u128(21),
            ..item
        };
        assert_eq!(dedupe(vec![item, other, item]), vec![item, other]);
    }

    #[test]
    fn a_pull_from_an_empty_collection_plans_nothing() {
        let src = Uuid::from_u128(20);
        assert!(plan_pull(&[], src, 4).is_empty());
        // A zeroed stack is not a stack.
        assert!(plan_pull(&[holding(src, 0, Finish::Nonfoil, Board::Main)], src, 4).is_empty());
    }
}
