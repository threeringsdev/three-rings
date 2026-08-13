//! `/my/collections/:id/needs` — what a collection is missing, and where to get
//! it (specs/app-ui.md → `/my/collections/:id/needs`).
//!
//! Five things are worth knowing before editing this file.
//!
//! **The arithmetic is board-aware on the wanting side and board-blind on the
//! offering side** (P6-074). `CollectionStore::needs` groups this collection's
//! desires and holdings by `(oracle, board)` — the same grain the collection
//! view's card rows use — so a deck holding a card on `main` and wanting it on
//! `side` shows a real need here, exactly as its Sideboard row's
//! `HERE — / WANTED 1` already claimed. Until P6-074 those two disagreed: the
//! read summed by oracle alone, so the mainboard copy cancelled the sideboard
//! want and this page showed nothing.
//!
//! The offers are the deliberate asymmetry: `owned_elsewhere` and `locations`
//! group by oracle alone, because only copies *inside* this collection are
//! committed to a board. A copy in the Trade Binder can fill **any** board's
//! need, so when one card needs copies on two boards, both rows show the same
//! elsewhere total and the same locations — "places you could pull from", not
//! stock split between them. `crate::backend::pull_plan` is where that shared
//! stock is reconciled at write time: it decrements the stacks it has already
//! planned against, so two boards drawing on one copy produce the honest
//! partial (which [`pull_line_outcome`] already knows how to say) rather than
//! the same copy planned twice.
//!
//! **A pull lands on the board that wanted it.** [`PullItem`] carries the need
//! row's board and `pull_plan` writes `to_board` from it, so a sideboard need
//! is actually closed by pulling into it. Before P6-074 every pull hardcoded
//! `to_board = main`, which is why a board-aware needs read could not have
//! shipped on its own: the sideboard need would have survived every pull aimed
//! at it, forever.
//!
//! What is still *not* here, and is still a different concept: relabelling a
//! copy this collection **already holds** from one board to another. That is
//! [card-tagging](../../../specs/card-tagging.md)'s quantity-preserving op, not
//! an acquisition, and this page's two buckets are both defined by an operation
//! that brings copies *in* (`move_cards`, or buying). A deck holding one copy
//! on `main` and wanting one on `main` **and** one on `side` needs one more
//! copy, and that is what this page says.
//!
//! **The pick list is client-composed, and quantity is never the caller's.** No
//! backend read was added for it: [`allocate`] spreads each row's gap over the
//! `locations` the needs read already carries, in that read's own order
//! (quantity desc, then name), and the sum of an allocation is the row's
//! `owned_elsewhere` by construction. The server runs the *same* function over
//! its *own* fresh `needs()`-shaped read, so the number on the checklist and
//! the number that moves are the same function of the same shape — a client
//! asking for 99 copies gets the allocation, not the 99. `allocate`,
//! [`gap_of`], [`dedupe`] and [`plan_pull`] are reused as-is by
//! `crate::backend::pull_plan` (P6-120): the read, the plan and the write used
//! to be three separately-committed calls composed in the `pull_needs` server
//! fn; they now run inside one transaction in the hosted backend, calling
//! straight back into these same functions rather than re-deriving them.
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

use super::collection::{ancestor_path, assembled_roots, board_label, message_of, needs_chip};
use super::move_selection::{movable, MoveSource, Skipped};
use super::tree::CollectionTreeResource;
use super::tree_manage::TreeManage;
use crate::components::states::{ErrorNote, StateBadge, Tone};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

// ------------------------------------------------------------ the wire ---

/// One line of a pull: a card this collection needs, which board wants it, and
/// the collection to take it from. Deliberately carries **no quantity** — see
/// the module doc.
///
/// `board` is the *destination* board (P6-074): the board of the [`NeedRow`]
/// this line was allocated from, and therefore the board the pulled copies
/// land on. It is never the source stack's board — that one is read off the
/// holding the server actually draws from, so undo puts the copies back where
/// they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullItem {
    pub oracle_id: Id,
    pub from_collection_id: Id,
    pub board: shared::Board,
}

impl PullItem {
    /// The name a pick-list row is known by on both sides. Like
    /// [`SelectionKey::token`](crate::components::ui::selection_tray::SelectionKey::token),
    /// it exists so the server can report per-line outcomes without shipping
    /// back card names the client already holds.
    ///
    /// The board is part of it because it is part of the line's identity: one
    /// card pulled from one binder onto two different boards is two lines, and
    /// a token that omitted the board would make the outcomes of the two
    /// indistinguishable (both would strike through on either one's report).
    pub fn token(&self) -> String {
        format!(
            "{}@{}@{}",
            self.oracle_id,
            self.from_collection_id,
            self.board.to_pg()
        )
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

/// What a pull line should do with itself, given how many copies it asked for
/// and how many the server actually reports moving for its own token.
///
/// `Pulled::copies` already tells the truth about what moved (its own doc
/// comment: "reported rather than inferred") — the P6-119 defect was never a
/// missing wire fact, it was the caller treating *any* nonzero as the whole
/// ask. `Full`/`Partial`/`Zero` make that comparison a closed set instead of
/// the boolean `!outcome.pulled.is_empty()` that could not distinguish "moved
/// all of it" from "moved some of it".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullLineOutcome {
    /// Moved at least as much as was asked — today's strike-through case.
    Full,
    /// Moved something, but less than was asked. The line stays live, owing
    /// exactly `residual`.
    Partial { residual: i32 },
    /// Moved nothing for this token — already excluded from `pulled` entirely
    /// (a zero-copy line is reported as a [`Skipped`] instead, never as a
    /// `Pulled` with `copies: 0`), kept here so the caller's match is
    /// exhaustive rather than leaning on an inferred `else`.
    Zero,
}

/// Classify one line's tick against what it asked for.
///
/// `asked` is the caller's own displayed count — the pick list is a snapshot
/// (module doc), so this is deliberately *not* re-derived from a fresh server
/// read; it is simply "what the label already said" before this tick. `moved`
/// is [`Pulled::copies`], the one number the server actually reports moving.
pub fn pull_line_outcome(asked: i32, moved: i32) -> PullLineOutcome {
    if moved <= 0 {
        PullLineOutcome::Zero
    } else if moved >= asked {
        PullLineOutcome::Full
    } else {
        PullLineOutcome::Partial {
            residual: asked - moved,
        }
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

/// Collapse a pull request to **one line per (card, board, source)** — the
/// shape [`allocate`] produces over the board-grained needs rows, enforced
/// rather than assumed.
///
/// Repeating a line is the one way a caller could smuggle a quantity through an
/// API that deliberately takes none. The server's plan is a fixed per-key
/// number, so two identical items would each plan the whole gap and move it
/// twice — four copies into a gap of two. A duplicate is a no-op, not a
/// multiplier.
///
/// The board joined the key with `NeedRow::board` (P6-074): the same card
/// pulled from the same binder onto `main` and onto `side` is **two** legitimate
/// lines with two separate gaps, and collapsing them would silently drop one.
pub fn dedupe(items: Vec<PullItem>) -> Vec<PullItem> {
    let mut seen: HashSet<(Id, Id, shared::Board)> = HashSet::new();
    items
        .into_iter()
        .filter(|i| seen.insert((i.oracle_id, i.from_collection_id, i.board)))
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

/// One pick-list line as rendered: a card, how many copies to pull, which board
/// they land on, and the token both sides know it by.
#[derive(Debug, Clone, PartialEq)]
pub struct PickRow {
    pub item: PullItem,
    pub name: String,
    pub copies: i32,
    /// The need's board — repeated out of `item` so the label can render it
    /// without reaching through the wire type.
    pub board: shared::Board,
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
                    board: row.board,
                },
                name: row.name.clone(),
                copies,
                board: row.board,
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
    // The same trick for the *collection tree's* mutations: the "Owned
    // elsewhere" table's Where column and the pick list's group names come
    // straight out of `collection_needs`, which no `tree.refetch()` can
    // update. A sidebar rename left both naming the old collection until an
    // unrelated pull bumped the holdings revision. See `TreeManage::revision`.
    let manage = expect_context::<TreeManage>();

    let needs_res = Resource::new(
        move || (url_id.get(), revision.get(), manage.revision.get()),
        |(id, _revision, _tree_revision)| async move {
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
                        // The way out is already on the page: `NeedsHeader` sits
                        // *outside* this boundary (it awaits the tree, not the
                        // needs read), so its back link to the collection
                        // survives this arm — which is why this is the one error
                        // banner here that needs no `children`.
                        Err(e) => {
                            view! {
                                <ErrorNote
                                    what="Couldn't load these needs"
                                    e
                                    testid="needs-error"
                                    retry=Callback::new(move |()| needs_res.refetch())
                                />
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
            // *copies to acquire*, and since P6-074 it counts them **per
            // board**, so a deck that wants a card on the sideboard while
            // holding it on the mainboard is genuinely one copy short. What is
            // still not counted is a board *relabel* of a copy already held
            // (module doc).
            <p class="text-muted-foreground text-sm">
                "Cards this collection wants more copies of than it holds, board by board."
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
            // Stated in **copies**, because copies are all this page can see —
            // but they are now counted per board (P6-074), so the old caveat
            // ("Unfilled board slots aren't counted here") became false and was
            // dropped rather than left reassuring the user about a hole that no
            // longer exists. The remaining caveat is the one still true: a copy
            // this collection already holds, sitting on the wrong board, is a
            // relabel and not an acquisition (module doc).
            //
            // The badge is the `success` family because this arm is the *good*
            // kind of nothing — the collection holds every copy it wants — and
            // that is a different claim from `/my/all`'s "you haven't added any
            // cards yet", which is the *absent* kind. Same blank table, opposite
            // meanings; the tone is what tells them apart at a glance.
            <div
                class="text-muted-foreground flex flex-col items-center gap-2 py-12 text-center text-sm"
                data-testid="needs-empty"
            >
                <StateBadge tone=Tone::Resolved label="All set" />
                <p>
                    "Nothing to pull or buy — this collection holds every copy it wants, on every board. Moving a copy you already hold between boards isn't counted here."
                </p>
            </div>
        }
        .into_any();
    }

    view! {
        // No `unwrap_or_else`. The line is omitted when there is no summary to
        // give, the way `RootRow::count` omits a count it cannot vouch for: the
        // fallback used to be the bare **"Nothing missing"** — the same
        // unqualified claim the empty arm above was rewritten to stop making,
        // and worse here, because reaching it means rows *do* exist. It is
        // unreachable today (`needs()` filters on `desired > present_here`, so
        // every row it returns has a gap and `needs_chip` always answers), which
        // made it a dishonest sentence waiting for the first payload shape that
        // reaches it rather than one anybody had seen.
        {summary
            .map(|s| {
                view! {
                    <p class="text-sm font-medium" data-testid="needs-summary">
                        {s}
                    </p>
                }
            })}
        {(!elsewhere.is_empty())
            .then(|| view! { <OwnedElsewhere rows=elsewhere collection_id picks /> })}
        {(!short.is_empty()).then(|| view! { <ShortBucket rows=short /> })}
    }
    .into_any()
}

/// The board a need row belongs to, rendered the way the collection page's deck
/// sections render theirs: **nothing at all for the mainboard**, the board's
/// name otherwise ([`board_label`] is the shared vocabulary). A binder's
/// desires are all `main`, so this is silent everywhere outside a deck — the
/// label only ever appears where it distinguishes two rows of the same card.
#[component]
fn BoardTag(board: shared::Board) -> impl IntoView {
    board_label(board).map(|label| {
        view! {
            <span
                class="bg-muted text-muted-foreground ml-2 rounded px-1.5 py-0.5 align-middle text-xs font-normal"
                data-testid="need-board"
            >
                {label}
            </span>
        }
    })
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
    let last_move = use_context::<crate::components::palette::LastMoveState>();
    let oracle_id = row.oracle_id;
    let board = row.board;
    let name = row.name.clone();
    let gap = gap_of(&row);
    let fillable = row.owned_elsewhere;
    let locations = row.locations.clone();
    // The whole row in one tap: every source its allocation names, one
    // transaction, one Undo. Every line carries **this row's board**, so the
    // copies land on the board that wanted them (P6-074).
    let items = StoredValue::new(
        allocate(gap, &row.locations)
            .into_iter()
            .map(|(from_collection_id, _)| PullItem {
                oracle_id,
                from_collection_id,
                board,
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
                Ok(outcome) => report(
                    &outcome,
                    &label.get_value(),
                    toast,
                    ReportContext {
                        tree,
                        revision,
                        last_move,
                    },
                    None,
                    // `fillable` is this row's own asked total across every
                    // source its allocation named — the honest baseline the
                    // toast checks a shortfall against, same as the pick
                    // list's per-line ask (see `PickRowView`).
                    Some(fillable),
                ),
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
        <TableRow
            {..}
            data-testid="needs-row"
            data-oracle=oracle_id.to_string()
            data-board=board.to_pg()
        >
            <TableCell class="p-2 font-medium">
                <a href=format!("/cards/{oracle_id}") class="hover:underline">
                    {name}
                </a>
                <BoardTag board />
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
    let last_move = use_context::<crate::components::palette::LastMoveState>();
    let busy = RwSignal::new(false);
    let token = StoredValue::new(row.item.token());
    let label = StoredValue::new(row.name.clone());
    let item = row.item;
    let board = row.board;
    // The line's own honest count. Starts at the snapshot's ask; a partial
    // pull rewrites it to the residual rather than lying that the whole ask
    // moved (P6-119). The `pick-label` span below is its one and only
    // reader — the checkbox's `aria_label` deliberately carries no count at
    // all, since it is a plain (non-reactive) prop set once at mount and
    // could not follow this signal.
    let remaining = RwSignal::new(row.copies);
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
                    // What this line asked for, and what the server reports it
                    // actually moved *for this token* — not just "did anything
                    // move" (the P6-119 bug: any nonzero `pulled` struck the
                    // line through, even a source that only had 2 of the 4
                    // asked, and the residual 2 vanished from the walk).
                    let asked = remaining.get_untracked();
                    let moved = outcome
                        .pulled
                        .iter()
                        .find(|p| p.token == token.get_value())
                        .map(|p| p.copies)
                        .unwrap_or(0);
                    match pull_line_outcome(asked, moved) {
                        PullLineOutcome::Full => {
                            done.update(|d| {
                                d.insert(token.get_value());
                            });
                        }
                        // Stay unstruck, carrying the honest residual — the line
                        // is not done, it is owed fewer copies than before.
                        PullLineOutcome::Partial { residual } => remaining.set(residual),
                        // Unreachable via `outcome.pulled` today (a zero-copy
                        // line is a `Skipped`, never a `Pulled{copies: 0}`) —
                        // handled anyway so this match stays exhaustive rather
                        // than assuming that invariant here too.
                        PullLineOutcome::Zero => {}
                    }
                    let undo_token = token.get_value();
                    report(
                        &outcome,
                        &label.get_value(),
                        toast,
                        ReportContext {
                            tree,
                            revision,
                            last_move,
                        },
                        Some(Callback::new(move |()| {
                            let undo_token = undo_token.clone();
                            done.update(|d| {
                                d.remove(&undo_token);
                            });
                            // The reversed copies go back where they came
                            // from, so the line's own ask reverts too — a
                            // partial-then-undo must not leave the line
                            // quietly asking for less than it originally did.
                            remaining.set(asked);
                        })),
                        Some(asked),
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
                // No count here: `aria_label` is a plain `String` frozen at
                // mount, not reactive, so it could not follow a partial
                // pull's residual (P6-119 review) — leaving the count in
                // would tell a screen-reader user the original ask forever.
                // The visible `pick-label` span is reactive and is now the
                // one and only reader of the count.
                aria_label=format!("Pull {}", row.name)
            />
            <span
                class=move || {
                    if checked.get() { "text-muted-foreground line-through" } else { "" }
                }
                data-testid="pick-label"
            >
                {move || format!("{} × {}", remaining.get(), label.get_value())}
            </span>
            // Which board these copies land on — the same silence-for-main
            // convention as the tables above. Without it, a card wanted on two
            // boards would show two identical-looking lines in one group.
            <BoardTag board />
        </li>
    }
}

/// The page plumbing a pull's report has to follow through: the sidebar tree
/// (badge counts), the holdings revision (this page's own resource trigger)
/// and ⌘K's last-move memory. Bundled into one `Copy` struct because the three
/// always travel together at both `report` call sites, and doing so keeps
/// `report` under clippy's argument ceiling now that `asked` (P6-119) would
/// otherwise have tipped it over.
#[derive(Clone, Copy)]
struct ReportContext {
    tree: Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
    revision: Option<super::move_selection::HoldingsRevision>,
    last_move: Option<crate::components::palette::LastMoveState>,
}

/// The toast every pull raises, and the Undo behind it.
///
/// One place, because a pull has three outcomes worth stating — copies moved,
/// lines refused, and both at once — and two call sites (the row button and the
/// checklist tick) that must not word them differently. `asked`, when the
/// caller knows one, is a fourth outcome worth a different sentence: **a
/// shortfall**. Both callers pass their own honest total (the pick list's
/// per-line snapshot count, the row button's `owned_elsewhere`) so a source
/// that had less than it looked like — the residual, not just "it moved
/// something" — gets said out loud instead of read as an unqualified success.
fn report(
    outcome: &PullOutcome,
    label: &str,
    toast: ToastHandle,
    ctx: ReportContext,
    on_undo: Option<Callback<()>>,
    asked: Option<i32>,
) {
    let ReportContext {
        tree,
        revision,
        last_move,
    } = ctx;
    if !outcome.move_ids.is_empty() {
        tree.refetch();
        if let Some(r) = revision {
            r.bump();
        }
        let copies = outcome.copies();
        let move_ids = outcome.move_ids.clone();
        // A pull is a batch of moves — one Undo for the toast, one for ⌘K.
        crate::components::palette::note_last_move(last_move, move_ids.clone());
        let message = match asked.map(|asked| pull_line_outcome(asked, copies)) {
            Some(PullLineOutcome::Partial { residual }) => {
                let asked = asked.expect("asked is Some when this arm matched");
                // Cause-neutral and number-only, deliberately: "not found at
                // the source" would assert a cause the client cannot know —
                // a `NoLongerNeeded` skip beside this one means the *gap*
                // closed, not the source, and that skip already states its
                // own reason. "the source" would also be wrong on
                // `ElsewhereRow`'s path, whose `asked` spans every source its
                // allocation named, not one.
                format!("Pulled {copies} of {asked} {label} — {residual} still owed")
            }
            _ => {
                let copies_label = if copies == 1 {
                    "1 copy".to_string()
                } else {
                    format!("{copies} copies")
                };
                format!("Pulled {copies_label} of {label}")
            }
        };
        toast.show(
            ToastOptions::message(message)
                .kind(ToastKind::Success)
                .action(
                    "Undo",
                    Callback::new(move |()| {
                        let move_ids = move_ids.clone();
                        // The palette must stop offering the same reversal
                        // (`LastMoveState::forget`'s doc).
                        crate::components::palette::forget_last_move(last_move, &move_ids);
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
                                let board = row.board;
                                view! {
                                    <TableRow
                                        {..}
                                        data-testid="short-row"
                                        data-oracle=oracle_id.to_string()
                                        data-board=board.to_pg()
                                    >
                                        <TableCell class="p-2 font-medium">
                                            <a
                                                href=format!("/cards/{oracle_id}")
                                                class="hover:underline"
                                            >
                                                {row.name}
                                            </a>
                                            <BoardTag board />
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
        need_on(Board::Main, desired, present_here, locations)
    }

    fn need_on(
        board: Board,
        desired: i32,
        present_here: i32,
        locations: Vec<CardLocation>,
    ) -> NeedRow {
        let gap = desired - present_here;
        let elsewhere: i32 = locations.iter().map(|l| l.quantity).sum();
        let owned_elsewhere = elsewhere.min(gap);
        NeedRow {
            oracle_id: Uuid::from_u128(1),
            name: "Lightning Bolt".to_string(),
            board,
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
            board: Board::Main,
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

    // ------------------------------------------- P6-074: board-aware needs ---

    #[test]
    fn a_pick_line_carries_the_board_that_wanted_the_copies() {
        let a = Uuid::from_u128(10);
        let side = need_on(Board::Side, 2, 0, vec![loc(a, "Trade Binder", 3)]);
        let groups = pick_list(&[side]);
        assert_eq!(groups.len(), 1);
        let row = &groups[0].rows[0];
        assert_eq!(row.copies, 2);
        assert_eq!(row.board, Board::Side);
        assert_eq!(
            row.item.board,
            Board::Side,
            "the wire line names the destination board, not the source's"
        );
    }

    #[test]
    fn one_card_wanted_on_two_boards_is_two_pick_lines_with_two_tokens() {
        // The board-blind read could not produce this pair at all. Both rows
        // offer the same location (offers are board-blind on purpose), so the
        // two lines must still be distinguishable — the server reports per
        // token, and one token for both would strike through the wrong line.
        let a = Uuid::from_u128(10);
        let main = need(1, 0, vec![loc(a, "Trade Binder", 5)]);
        let side = need_on(Board::Side, 2, 0, vec![loc(a, "Trade Binder", 5)]);
        let groups = pick_list(&[main, side]);
        assert_eq!(groups.len(), 1, "one collection to walk to");
        assert_eq!(groups[0].rows.len(), 2);
        assert_ne!(
            groups[0].rows[0].item.token(),
            groups[0].rows[1].item.token()
        );
        assert_eq!(groups[0].rows[0].copies, 1);
        assert_eq!(groups[0].rows[1].copies, 2);
    }

    #[test]
    fn dedupe_keeps_the_two_boards_and_still_collapses_a_repeat() {
        let src = Uuid::from_u128(20);
        let main = PullItem {
            oracle_id: Uuid::from_u128(1),
            from_collection_id: src,
            board: Board::Main,
        };
        let side = PullItem {
            board: Board::Side,
            ..main
        };
        // Two boards are two legitimate lines — collapsing them would silently
        // drop one of the two gaps.
        assert_eq!(dedupe(vec![main, side]), vec![main, side]);
        // The same board twice is still the smuggled-quantity case dedupe
        // exists to refuse.
        assert_eq!(dedupe(vec![main, side, main, side]), vec![main, side]);
    }

    #[test]
    fn the_chip_counts_a_sideboard_want_a_mainboard_copy_used_to_cancel() {
        // The contradiction P6-074 closes, at the formatter: the deck holds one
        // copy on `main` (so no `main` row survives the `desired > present`
        // filter) and wants one on `side`. The rows are now per board, so the
        // side row stands on its own and the chip counts it.
        let a = Uuid::from_u128(10);
        let rows = vec![need_on(Board::Side, 1, 0, vec![loc(a, "Trade Binder", 1)])];
        let totals = totals_of(&rows);
        assert_eq!(totals.missing, 1);
        assert_eq!(totals.owned_elsewhere, 1);
        assert_eq!(totals.to_buy, 0);
        assert_eq!(
            super::super::collection::needs_chip(&totals).as_deref(),
            Some("1 missing — 1 owned elsewhere"),
        );
    }

    // ------------------------------------------------- P6-119: partial pulls ---

    #[test]
    fn moving_everything_asked_is_a_full_pull() {
        // The ordinary case, pinned so the boundary condition below (`moved ==
        // asked`) is provably `Full`, not a one-off `Partial { residual: 0 }`
        // that happened to render the same.
        assert_eq!(pull_line_outcome(4, 4), PullLineOutcome::Full);
        assert_eq!(pull_line_outcome(1, 1), PullLineOutcome::Full);
        // Moving *more* than the stale snapshot asked (the source restocked
        // between generating the pick list and ticking the line) is still a
        // full pull, never a negative residual. This pins today's decision
        // function against that input, not an endorsement that the server
        // *should* be free to move more than a line ever displayed.
        // P6-120 closed the read/plan/write race (the three-transaction
        // window that could move *different* copies than the plan saw) but
        // deliberately left this one alone: a fresh `needs()` read legitimately
        // widening the destination's gap since the snapshot was taken is not a
        // bug to fix, it is the same "quantity is never the caller's" rule this
        // module leads with — still recorded here as a real, un-narrowed input
        // to this function rather than a hypothetical one.
        assert_eq!(pull_line_outcome(2, 5), PullLineOutcome::Full);
    }

    #[test]
    fn moving_less_than_asked_is_a_partial_pull_carrying_the_honest_residual() {
        // The defect this task fixes: a line asking 4 whose source only had 2
        // left must not read as fully pulled, and what remains owed is 2, not
        // the original 4 restated.
        assert_eq!(
            pull_line_outcome(4, 2),
            PullLineOutcome::Partial { residual: 2 }
        );
        assert_eq!(
            pull_line_outcome(3, 1),
            PullLineOutcome::Partial { residual: 2 }
        );
        // One short of the ask is still a partial, not a full pull with an
        // off-by-one forgiven.
        assert_eq!(
            pull_line_outcome(5, 4),
            PullLineOutcome::Partial { residual: 1 }
        );
    }

    #[test]
    fn moving_nothing_for_this_token_is_zero_and_leaves_no_residual_claim() {
        // Today's already-correct behavior for a token absent from
        // `outcome.pulled` entirely (it is reported as a `Skipped` instead) —
        // pinned here so it cannot regress silently while the `Partial` arm
        // is added beside it.
        assert_eq!(pull_line_outcome(4, 0), PullLineOutcome::Zero);
        assert_eq!(pull_line_outcome(0, 0), PullLineOutcome::Zero);
        // Defensive: a negative report (should not happen — `Pulled::copies`
        // is a sum of non-negative `plan_pull` quantities) still resolves to
        // "nothing to strike through or shrink" rather than a bogus larger
        // residual than was ever asked.
        assert_eq!(pull_line_outcome(4, -1), PullLineOutcome::Zero);
    }
}
