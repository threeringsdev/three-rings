//! Batch move — the selection tray's "Move to…" (specs/app-ui.md → Selection
//! tray; specs/collection-api.md → "Move (batch)").
//!
//! Six things are worth knowing before editing this file.
//!
//! **A `/my` selection cannot be piped into a move, and this is where that is
//! resolved.** [`SelectionKey::Card`] names an oracle card and nothing else: the
//! `/my` row aggregates every collection, and its `printing_id` is the
//! *representative* (has-art-first) printing, which the caller may own zero of.
//! So the row answers neither "from which collection" nor "which printing".
//! Passing `from_collection_id: None` would not be "unknown" — in
//! [`shared::MoveItem`] it means **external intake**, i.e. conjuring copies from
//! outside the system. The resolution therefore happens **server-side against
//! the caller's actual holdings** ([`resolve_card`]). Exactly one candidate
//! source ⇒ move it; anything else ⇒ refuse, by name, with a reason.
//!
//! **Resolution reads holdings ungrouped, and that is load-bearing.** Every
//! read model this feature could have used collapses the grain a move is
//! addressed at: `collection_view` groups by `(printing, board)` and
//! `CardDetail::ownership` by `(collection, printing)`. So a Trade Binder row
//! reading `present = 3` can be three *foils*, and a `/my` row can be copies
//! that sit on a sideboard — both indistinguishable from movable ones, both
//! selectable. Resolving against those collapsed reads meant discovering the
//! problem inside `holding_take`, as `Conflict("no copies to move")`, which in a
//! single-transaction batch kills every other card in the selection and names
//! none of them. `CollectionStore::holdings_of_oracle` returns the ungrouped
//! rows so [`movable`] can be decided *before* the write, per entry.
//!
//! **Refusals are reported, never dropped.** Whatever the server would not move
//! comes back as [`Skipped`] and stays checked in the tray, with a toast saying
//! why. A batch that silently shrank would be indistinguishable from one that
//! worked — and a batch that died whole would be worse.
//!
//! **One copy per selected entry.** The tray counts *entries* ("2 cards"), not
//! copies, and `SelectedCard` carries no quantity, so a move of one copy per
//! entry is the only reading of the pill that cannot lie. It is also the small
//! blast radius: a mis-click moves one card, not a playset. Quantity is fixed
//! server-side rather than taken from the caller (the `quick_add` precedent —
//! an adapter whose wire contract is wider than its name is a trap).
//!
//! **The undo is one action over N ledger rows.** A batch move writes one
//! `moves` row per item — the ledger has no batch id — so "one undo covering
//! the whole batch" is `CollectionStore::undo_moves`, which reverses the list in
//! a *single transaction*. Looping the single-move undo would be N
//! transactions, and a failure part-way would leave the batch half-reverted
//! behind a toast that said it was undone.
//!
//! **Boards and off-default grains are now moved, not refused.** They were
//! refused for one task because `moves` had no board column and `holding_take`
//! pinned `board = 'main'` at the default finish/condition/language, so a move
//! from a sideboard row — or a foil-only stack — would have taken *different
//! copies than the row the user checked*, or none at all. The ledger now carries
//! a board at each end and `MoveItem` carries the full grain, so resolution
//! passes on the stack it actually found instead of restating a default. One
//! refusal survives, and it is not a limitation of the write: a stack holding
//! several grains and no default one (2 foil + 1 etched) does not say which copy
//! the user meant, and this code's whole job is not to invent that answer.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use shared::{Board, CollectionSummary, Condition, Finish, HoldingLine, Id, SuggestedDestination};

use crate::catalog::destination::{
    picker_order, Destination, DestinationChoice, DestinationList, DestinationOption,
};
use crate::components::ui::popover::{Popover, PopoverAlign, PopoverContent, PopoverTrigger};
use crate::components::ui::selection_tray::{SelectedCard, SelectionKey, SelectionState};
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};

// ------------------------------------------------------ what a batch takes ---

/// One entry of a batch move on the wire: what the row addresses, plus the
/// oracle card it is a copy of.
///
/// The oracle rides along because resolution needs the caller's holdings of
/// *that card* ungrouped, and a `Held` key names only a printing. Trusting the
/// client for it is safe by construction rather than by politeness: it is used
/// solely to look up the caller's own (RLS-scoped) holdings, and every
/// resolution path then re-checks that the named collection/printing/board
/// actually appears in what came back. A wrong oracle therefore produces a
/// [`SkipReason::NoCopies`] refusal, never a write somewhere else.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SelectionItem {
    pub key: SelectionKey,
    pub oracle_id: Id,
}

impl From<&SelectedCard> for SelectionItem {
    fn from(card: &SelectedCard) -> Self {
        Self {
            key: card.key,
            oracle_id: card.oracle_id,
        }
    }
}

// ------------------------------------------------ what a batch move reports ---

/// The result of one batch move: the ledger rows it wrote (the undo handle),
/// which selection entries actually moved, and which were refused and why.
///
/// Entries are named by [`SelectionKey::token`] rather than by index so the
/// client can match them back to tray rows without assuming the server kept the
/// order — and so the wire shape carries no card names the client already has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MoveOutcome {
    /// One per moved item, in batch order — all undone together.
    pub move_ids: Vec<Id>,
    /// Tokens of the entries that moved.
    pub moved: Vec<String>,
    /// Entries the server declined to move, each with its reason.
    pub skipped: Vec<Skipped>,
}

/// One entry the batch refused, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    pub token: String,
    pub reason: SkipReason,
}

/// Why a selected row was not moved. An enum, not a server-composed sentence:
/// the wording is the client's business and the reasons are a closed set.
///
/// Every variant is a question only the user can answer. There is deliberately
/// no variant left for "the write cannot express that" — a board and a full
/// grain are both addressable now, so anything unambiguous moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    /// The copies are already in the destination.
    AlreadyThere,
    /// One stack, several finish/condition/language grains, none of them the
    /// default — so which copies the row meant is genuinely undecided.
    Grain(usize),
    /// Nothing is held any more — the selection outlived the copies (another
    /// tab, the stepper, or simply a tray left open a long time).
    NoCopies,
    /// A pick-list line whose *need* is gone: the destination no longer wants
    /// more copies of that card than it holds. The other half of `NoCopies` —
    /// that one is the source going away, this one is the gap closing — and it
    /// is why a pull's quantity is re-derived server-side rather than trusted
    /// (`my::needs`).
    NoLongerNeeded,
    /// A `/my` row whose copies sit in several collections: which one to take
    /// from is the user's call, not a guess this code may make.
    ManyCollections(usize),
    /// A `/my` row held under several printings in one collection: same
    /// problem, one level down.
    ManyPrintings(usize),
    /// A `/my` row split across a deck's boards: the two are separate rows on
    /// the collection page, and only there can the user say which they meant.
    ManyBoards(usize),
}

impl SkipReason {
    /// The half-sentence the toast appends to a card name.
    pub fn phrase(self) -> String {
        match self {
            Self::AlreadyThere => "is already there".to_string(),
            Self::Grain(n) => {
                format!("is held in {n} finishes or conditions at once — a move has to name one")
            }
            Self::NoCopies => "has no copies left to move — reload the page".to_string(),
            Self::NoLongerNeeded => "is no longer missing here — reload the page".to_string(),
            Self::ManyCollections(n) => {
                format!("is in {n} collections — open one and select the row there")
            }
            Self::ManyPrintings(n) => {
                format!("is held under {n} printings — open its collection and select the row")
            }
            Self::ManyBoards(n) => {
                format!("sits on {n} boards — open its deck and select the row you mean")
            }
        }
    }
}

/// Is this holding a candidate at all?
///
/// It used to also require the mainboard and the default grain, because the
/// write could address neither. Both now ride on `MoveItem`, so the only thing
/// that disqualifies a stack is having nothing in it.
pub fn movable(h: &HoldingLine) -> bool {
    h.quantity > 0
}

/// Whether a holding sits at the grain a caller who states nothing would mean.
fn default_grain(h: &HoldingLine) -> bool {
    h.finish == Finish::default()
        && h.condition == Condition::default()
        && h.language == shared::default_language()
}

/// Choose the one stack a row's checkbox meant, out of the grains it summed.
///
/// The default grain wins when it is there — it is what an unqualified "move
/// this card" means, and it keeps a plain single beside a foil playset behaving
/// as it always did. Failing that, a stack with exactly one grain is
/// unambiguous however exotic it is (a foil-only row is a real row, reachable on
/// the dev fixture today, and refusing it was the defect this replaces).
/// Several grains and no default one is a question, and this returns `None`
/// rather than answering it.
fn pick<'a>(candidates: &[&'a HoldingLine]) -> Option<&'a HoldingLine> {
    if let Some(h) = candidates.iter().find(|h| default_grain(h)) {
        return Some(h);
    }
    match candidates {
        [only] => Some(only),
        _ => None,
    }
}

/// How many distinct values `f` takes across the candidates.
fn distinct<T: Ord, F: Fn(&HoldingLine) -> T>(candidates: &[&HoldingLine], f: F) -> usize {
    let mut seen: Vec<T> = candidates.iter().map(|h| f(h)).collect();
    seen.sort();
    seen.dedup();
    seen.len()
}

// ------------------------------------------------------------- resolution ---

/// The stack a resolved selection moves — grain-complete, because that is the
/// addressing the write uses. Built from the holding the resolution actually
/// found, never restated from defaults: a `MoveItem` assembled at the default
/// grain beside a foil-only stack is a write aimed at copies that do not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveSource {
    pub from: Id,
    pub printing_id: Id,
    pub finish: Finish,
    pub condition: Condition,
    pub language: String,
    pub board: Board,
}

impl From<&HoldingLine> for MoveSource {
    fn from(h: &HoldingLine) -> Self {
        Self {
            from: h.collection_id,
            printing_id: h.printing_id,
            finish: h.finish,
            condition: h.condition,
            language: h.language.clone(),
            board: h.board,
        }
    }
}

/// What a selection resolves to, given everything the caller holds of that card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardSource {
    /// The one unambiguous candidate stack.
    Move(MoveSource),
    Refuse(SkipReason),
}

/// Resolve a `/my` row into a move source, or refuse.
///
/// `holdings` is every copy of this oracle card the caller owns, **ungrouped**
/// (`CollectionStore::holdings_of_oracle`) — which is what makes board and
/// grain visible here at all. `CardDetail::ownership`, the obvious candidate,
/// groups by `(collection, printing)`, so a card whose only copies are
/// sideboarded or foil looked movable through it and blew up the batch instead.
///
/// The destination is not a *candidate source*, so it is filtered out first: a
/// card sitting in both the Trade Binder and the deck you are moving to has
/// exactly one place it can come from, and refusing that as "ambiguous" would
/// be needlessly useless.
///
/// After that the rule is deliberately strict — **exactly one** movable
/// candidate. Anything else is a question only the user can answer, and the
/// enum's whole purpose is that this code cannot invent an answer.
pub fn resolve_card(holdings: &[HoldingLine], to: Id) -> CardSource {
    let held: Vec<&HoldingLine> = holdings.iter().filter(|h| movable(h)).collect();
    if held.is_empty() {
        return CardSource::Refuse(SkipReason::NoCopies);
    }
    let candidates: Vec<&HoldingLine> =
        held.into_iter().filter(|h| h.collection_id != to).collect();
    if candidates.is_empty() {
        return CardSource::Refuse(SkipReason::AlreadyThere);
    }
    // Narrow outward-in, so the refusal names the outermost thing that is
    // ambiguous: which collection, then which printing, then which board. Each
    // is a question the user answers by opening a page where the choice is a
    // visible row; only the innermost (which grain) has no such page.
    let collections = distinct(&candidates, |h| h.collection_id);
    if collections > 1 {
        return CardSource::Refuse(SkipReason::ManyCollections(collections));
    }
    let printings = distinct(&candidates, |h| h.printing_id);
    if printings > 1 {
        return CardSource::Refuse(SkipReason::ManyPrintings(printings));
    }
    let boards = distinct(&candidates, |h| h.board.to_pg());
    if boards > 1 {
        return CardSource::Refuse(SkipReason::ManyBoards(boards));
    }
    match pick(&candidates) {
        Some(h) => CardSource::Move(h.into()),
        None => CardSource::Refuse(SkipReason::Grain(candidates.len())),
    }
}

/// Resolve a `/my/collections/:id` row, or refuse.
///
/// The row is grain-*addressed* (collection + printing + board) but not
/// grain-*complete*: `CardRow` has no finish/condition/language, and the view's
/// `present` sums across all of them (`GROUP BY printing_id, board`). So
/// `present = 3` on a row whose three copies are foil is a checkbox whose grain
/// the page cannot state. The ungrouped holdings read is what supplies it — and
/// supplying it is now the point, where it used to be grounds for refusal.
///
/// The board comes from the row, not from an assumption: a deck's mainboard and
/// sideboard rows for one printing are two checkboxes, and taking the row's word
/// for which is what keeps them from lying about each other.
pub fn resolve_held(
    holdings: &[HoldingLine],
    from: Id,
    printing_id: Id,
    board: Board,
    to: Id,
) -> CardSource {
    if from == to {
        return CardSource::Refuse(SkipReason::AlreadyThere);
    }
    // Exactly the stack behind the row: one collection, one printing, one
    // board — every finish/condition/language the row summed into its count.
    let stack: Vec<&HoldingLine> = holdings
        .iter()
        .filter(|h| {
            h.collection_id == from
                && h.printing_id == printing_id
                && h.board == board
                && movable(h)
        })
        .collect();
    if stack.is_empty() {
        return CardSource::Refuse(SkipReason::NoCopies);
    }
    match pick(&stack) {
        Some(h) => CardSource::Move(h.into()),
        None => CardSource::Refuse(SkipReason::Grain(stack.len())),
    }
}

/// Put a card's name on a whole-batch failure the server attributed to one item.
///
/// A batch move is one transaction, so one unmovable item rolls the batch back
/// and the error describes the *batch*. `move_batch` tags the failure with the
/// item's index (`shared::batch_item_error`), the `move_selection` adapter turns
/// that index into the entry's token, and this turns the token into the name the
/// tray showed. Without the chain, a real failure is diagnosable only by
/// bisecting the selection — which is exactly what the batch-move task recorded.
///
/// A message that is not token-prefixed, or whose token no longer matches a
/// tray entry, is passed through untouched: naming an innocent card is worse
/// than naming none.
pub fn name_batch_failure(message: &str, names: &[(String, String)]) -> String {
    let Some((token, rest)) = message.split_once(": ") else {
        return message.to_string();
    };
    match names.iter().find(|(t, _)| t == token) {
        Some((_, name)) => format!("{name} {rest}"),
        None => message.to_string(),
    }
}

/// Fold per-card destination suggestions into one ranking for the whole
/// selection: a collection that wants three of the selected cards outranks one
/// that wants a single copy of one. Shortfall descending, then name, matching
/// `suggested_destinations`' own order for the single-card case.
pub fn merge_suggestions(per_card: Vec<Vec<SuggestedDestination>>) -> Vec<SuggestedDestination> {
    let mut merged: Vec<SuggestedDestination> = Vec::new();
    for row in per_card.into_iter().flatten() {
        match merged
            .iter_mut()
            .find(|m| m.collection_id == row.collection_id)
        {
            Some(m) => {
                m.desired += row.desired;
                m.present += row.present;
                m.shortfall += row.shortfall;
            }
            None => merged.push(row),
        }
    }
    merged.sort_by(|a, b| {
        b.shortfall.cmp(&a.shortfall).then_with(|| {
            a.collection_name
                .to_lowercase()
                .cmp(&b.collection_name.to_lowercase())
        })
    });
    merged
}

/// The picker's rows: the collections that *want* the selection first, each
/// hinting its shortfall, then everything else in the toolbar picker's order
/// (Inbox pinned, then by name).
///
/// Both halves are needed. Suggestions alone would leave a plain "put these in
/// that binder" move impossible whenever no collection happens to desire the
/// cards, which is the common case for a binder; the full list alone would
/// throw away the ranking this task exists to use.
pub fn picker_options(
    suggested: &[SuggestedDestination],
    all: &[CollectionSummary],
) -> Vec<DestinationChoice> {
    let mut rows: Vec<DestinationChoice> = Vec::new();
    for s in suggested {
        // Resolve names/flags from the live list so a suggestion cannot show a
        // stale name — and skip any suggestion for a collection that no longer
        // exists, rather than offering a destination every move would 404 on.
        if let Some(c) = all.iter().find(|c| c.id == s.collection_id) {
            rows.push(DestinationChoice {
                dest: Destination {
                    id: c.id,
                    name: c.name.clone(),
                    is_inbox: c.is_inbox,
                },
                hint: Some(format!("wants {}", s.shortfall)),
            });
        }
    }
    let rest = all
        .iter()
        .filter(|c| !rows.iter().any(|r| r.dest.id == c.id))
        .cloned()
        .collect::<Vec<_>>();
    rows.extend(picker_order(rest).into_iter().map(|c| {
        DestinationChoice::plain(Destination {
            id: c.id,
            name: c.name,
            is_inbox: c.is_inbox,
        })
    }));
    rows
}

// ----------------------------------------------------------------- wording ---

/// The confirmation toast's message. The count is stated in **copies**, because
/// the pill counts entries and one copy each is the thing a reader would
/// otherwise have to infer.
pub fn moved_message(moved: usize, destination: &str) -> String {
    let copies = if moved == 1 { "1 copy" } else { "1 copy each" };
    let cards = if moved == 1 {
        "1 card".to_string()
    } else {
        format!("{moved} cards")
    };
    format!("Moved {cards} ({copies}) → {destination}")
}

/// The refusal toast. Names up to [`NAMED_SKIPS`] cards with their reasons and
/// counts the rest, so a batch of thirty refusals is still one readable line.
pub fn skipped_message(skipped: &[(String, SkipReason)]) -> String {
    let head = skipped
        .iter()
        .take(NAMED_SKIPS)
        .map(|(name, reason)| format!("{name} {}", reason.phrase()))
        .collect::<Vec<_>>()
        .join("; ");
    let rest = skipped.len().saturating_sub(NAMED_SKIPS);
    let lead = if skipped.len() == 1 {
        "1 card wasn't moved".to_string()
    } else {
        format!("{} cards weren't moved", skipped.len())
    };
    if rest > 0 {
        format!("{lead}: {head}; and {rest} more")
    } else {
        format!("{lead}: {head}")
    }
}

/// How many refusals a toast names before it starts counting.
const NAMED_SKIPS: usize = 2;

// ---------------------------------------------------------------- staleness ---

/// Bumped whenever copies move, so the views that render holdings refetch.
///
/// The tray lives in the shell and the tables live in pages, so a move has no
/// handle on the resource it invalidated. This is that handle: pages include it
/// in their resource's *source*, which is a refetch by construction rather than
/// an effect that has to remember to fire. Provided once, by the shell.
#[derive(Clone, Copy)]
pub struct HoldingsRevision(pub RwSignal<u32>);

impl HoldingsRevision {
    pub fn bump(self) {
        self.0.update(|n| *n += 1);
    }
}

pub fn provide_holdings_revision() -> HoldingsRevision {
    let revision = HoldingsRevision(RwSignal::new(0));
    provide_context(revision);
    revision
}

/// The revision a page's resource should depend on. Zero and constant outside
/// the shell (the bench), so a page component stays mountable there.
pub fn holdings_revision() -> Signal<u32> {
    match use_context::<HoldingsRevision>() {
        Some(r) => Signal::derive(move || r.0.get()),
        None => Signal::derive(|| 0),
    }
}

// ------------------------------------------------------------- the control ---

/// The tray's primary action: "Move to…", the destination picker, the batch
/// write, and the one Undo that covers it.
///
/// Mounted inside the tray's `Show`, so its resources are fetched only while
/// there is a selection to move.
#[component]
pub fn MoveSelection(selection: SelectionState) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    // Optional, both of them: the bench hosts this control outside the shell.
    let tree = use_context::<crate::my::tree::CollectionTreeResource>();
    let revision = use_context::<HoldingsRevision>();
    let last_move = use_context::<crate::components::palette::LastMoveState>();

    let open = RwSignal::new(false);
    let pending = RwSignal::new(false);

    // The distinct oracle cards in the selection — `suggested_destinations` is
    // per-oracle, and two selected rows can be the same card in two places.
    let oracles = Memo::new(move |_| {
        let mut ids = selection
            .items()
            .with(|v| v.iter().map(|c| c.oracle_id).collect::<Vec<_>>());
        ids.sort();
        ids.dedup();
        ids
    });

    let suggested = Resource::new(
        move || oracles.get(),
        |ids| async move {
            // Deliberately NOT wrapped in a named payload, unlike its sibling
            // `collections` below. It would be the right shape to wrap — it
            // answers `Ok(vec![])` for the common case (an empty tray) and
            // `{"Ok":[]}` is a universal key for every list-shaped resource — but
            // two things rule it out. It is never serialized (the tray cannot
            // server-render: `SelectionState` starts empty on every load, so this
            // whole subtree is absent from the SSR output, confirmed by dumping
            // every `/my/*` route's slots and finding no array payload), and
            // wrapping it makes `Resource<Result<Wrapper, _>>` non-`Copy`, which
            // turns the `Suspend` closure below into an `FnOnce` the view macro
            // rejects. Paying a real restructuring cost to close a hole that has
            // no payload behind it is the wrong trade; filed instead.
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            crate::selection_destinations(ids).await
        },
    );
    let collections = Resource::new(
        || (),
        |_| async { crate::catalog::destination::collection_list().await },
    );

    // Written by an `Effect`, never read in render: a resource read in plain
    // render is unresolved during SSR and already resolved at hydration, and
    // hydration *claims* the server's text without rewriting it (the wrong
    // "Show results" label `ResultsToolbar` shipped before this was understood).
    // The tray only exists once a selection does, and a selection starts empty on
    // every load, so this surface is never server-rendered and the Effect is the
    // whole story here.
    let load_failed = RwSignal::new(false);
    Effect::new(move |_| {
        let failed = matches!(collections.get(), Some(Err(_)));
        if failed != load_failed.get_untracked() {
            load_failed.set(failed);
        }
    });

    let choose = Callback::new(move |dest: Destination| {
        if pending.get_untracked() {
            return;
        }
        let entries = selection.items().get_untracked();
        if entries.is_empty() {
            return;
        }
        // Names for the refusal toast: the server answers in tokens (it has no
        // business shipping back strings the client already holds).
        let names: Vec<(String, String)> = entries
            .iter()
            .map(|c| (c.key.token(), c.name.clone()))
            .collect();
        let items: Vec<SelectionItem> = entries.iter().map(SelectionItem::from).collect();

        pending.set(true);
        open.set(false);
        spawn_local(async move {
            let result = crate::move_selection(dest.id, items).await;
            pending.set(false);
            match result {
                Ok(outcome) => {
                    // Drop what moved; leave the refusals checked — they are
                    // what still needs doing, and the toast names them.
                    selection.remove_tokens(&outcome.moved);
                    if !outcome.move_ids.is_empty() {
                        if let Some(t) = tree {
                            t.0.refetch();
                        }
                        if let Some(r) = revision {
                            r.bump();
                        }
                        let move_ids = outcome.move_ids.clone();
                        // The whole batch is one undo, for ⌘K as for the toast.
                        crate::components::palette::note_last_move(last_move, move_ids.clone());
                        toast.show(
                            ToastOptions::message(moved_message(
                                outcome.moved.len(),
                                &dest.label(),
                            ))
                            .kind(ToastKind::Success)
                            .action(
                                "Undo",
                                Callback::new(move |()| {
                                    undo(toast, tree, revision, last_move, move_ids.clone())
                                }),
                            ),
                        );
                    }
                    let refused = label_skips(&outcome.skipped, &names);
                    if !refused.is_empty() {
                        toast.show(
                            ToastOptions::message(skipped_message(&refused)).kind(ToastKind::Error),
                        );
                    }
                }
                Err(e) => {
                    // The batch is one transaction, so a failure here moved
                    // nothing — say so, because the selection is still on
                    // screen and "did some of that land?" is otherwise
                    // unanswerable from the toast. And name the card when the
                    // server could attribute the failure to one entry.
                    toast.show(
                        ToastOptions::message(format!(
                            "Couldn't move: {} — nothing was moved",
                            name_batch_failure(&crate::my::collection::message_of(&e), &names,)
                        ))
                        .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        // Aligned to its own end and opening upward (the popover's default
        // block-start placement): this control sits at the bottom of the
        // viewport, where a downward panel would be off screen.
        <Popover id="tray-destination" open=open align=PopoverAlign::End>
            <PopoverTrigger
                class="bg-background text-foreground h-auto shrink-0 rounded-md border-0 px-3 py-1.5 text-[13px] font-medium"
                attr:data-testid="tray-move"
            >
                {move || if pending.get() { "Moving…" } else { "Move to…" }}
            </PopoverTrigger>
            <PopoverContent class="w-[280px] p-0">
                <DestinationList empty="No collection to move to." failed=load_failed>
                    // Same boundary the catalog's picker uses, and for the same
                    // reason: the rows come from resources, and only a
                    // suspense boundary keeps a render in step with them.
                    <Transition fallback=|| {
                        view! {
                            <p class="text-muted-foreground p-3 text-sm">"Loading collections…"</p>
                        }
                    }>
                        {move || Suspend::new(async move {
                            // The tree of collections is what this list *is*, so
                            // its failure is reported (`load_failed` → the list's
                            // own failed arm) rather than flattened into zero rows
                            // that read as "you have nowhere to move these".
                            let all = collections.await.unwrap_or_default().collections;
                            // Suggestions are a *ranking* hint — collections whose
                            // desired exceeds present for these cards. Losing them
                            // costs the wireframe's ordering, not the ability to
                            // move, and no arm here would be more honest than the
                            // plain list: this one degrades on purpose.
                            let ranked = suggested.await.unwrap_or_default();
                            picker_options(&ranked, &all)
                                .into_iter()
                                .map(|choice| {
                                    view! { <DestinationOption choice on_choose=choose /> }
                                })
                                .collect_view()
                        })}
                    </Transition>
                </DestinationList>
            </PopoverContent>
        </Popover>
    }
}

/// Reverse the whole batch — one call, one transaction (see the module docs).
fn undo(
    toast: ToastHandle,
    tree: Option<crate::my::tree::CollectionTreeResource>,
    revision: Option<HoldingsRevision>,
    last_move: Option<crate::components::palette::LastMoveState>,
    move_ids: Vec<Id>,
) {
    let count = move_ids.len();
    // The palette must stop offering the same reversal (`forget`'s doc).
    crate::components::palette::forget_last_move(last_move, &move_ids);
    spawn_local(async move {
        match crate::undo_selection_move(move_ids).await {
            Ok(()) => {
                if let Some(t) = tree {
                    t.0.refetch();
                }
                if let Some(r) = revision {
                    r.bump();
                }
                let cards = if count == 1 { "1 card" } else { "them" };
                toast.show(ToastOptions::message(format!("Put {cards} back")));
            }
            Err(e) => {
                toast.show(
                    ToastOptions::message(format!(
                        "Couldn't undo: {}",
                        crate::my::collection::message_of(&e)
                    ))
                    .kind(ToastKind::Error),
                );
            }
        }
    });
}

/// Pair each refusal with the card name the tray showed for it. A token with no
/// matching entry is impossible in practice (the batch is built from the tray)
/// but is reported rather than dropped — a silent disappearance is the exact
/// failure this whole reporting path exists to prevent.
pub fn label_skips(skipped: &[Skipped], names: &[(String, String)]) -> Vec<(String, SkipReason)> {
    skipped
        .iter()
        .map(|s| {
            let name = names
                .iter()
                .find(|(token, _)| *token == s.token)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| "A card".to_string());
            (name, s.reason)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Id {
        Id::from_u128(n)
    }

    /// A plain movable holding: mainboard, nonfoil, NM, English.
    fn own(collection: u128, printing: u128, quantity: i32) -> HoldingLine {
        HoldingLine {
            id: Id::new_v4(),
            collection_id: id(collection),
            printing_id: id(printing),
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: shared::default_language(),
            board: Board::Main,
            quantity,
        }
    }

    fn foil(collection: u128, printing: u128, quantity: i32) -> HoldingLine {
        HoldingLine {
            finish: Finish::Foil,
            ..own(collection, printing, quantity)
        }
    }

    fn on_board(board: Board, collection: u128, printing: u128, quantity: i32) -> HoldingLine {
        HoldingLine {
            board,
            ..own(collection, printing, quantity)
        }
    }

    /// What a resolved move should look like for a plain mainboard single.
    fn plain(collection: u128, printing: u128) -> CardSource {
        CardSource::Move(MoveSource {
            from: id(collection),
            printing_id: id(printing),
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: shared::default_language(),
            board: Board::Main,
        })
    }

    #[test]
    fn one_source_resolves_to_that_printing() {
        let entries = [own(1, 100, 3)];
        assert_eq!(resolve_card(&entries, id(9)), plain(1, 100));
    }

    #[test]
    fn the_destination_is_not_a_candidate_source() {
        // Held in the deck we're moving to *and* in the binder: exactly one
        // place it can come from, so this resolves rather than refusing.
        let entries = [own(9, 100, 1), own(1, 101, 2)];
        assert_eq!(resolve_card(&entries, id(9)), plain(1, 101));
    }

    #[test]
    fn two_collections_are_a_question_for_the_user() {
        let entries = [own(1, 100, 1), own(2, 100, 1)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::ManyCollections(2))
        );
    }

    #[test]
    fn two_printings_in_one_collection_are_ambiguous_too() {
        // Same collection, two printings — the row on `/my` names neither, and
        // its representative printing may be one of them or neither.
        let entries = [own(1, 100, 1), own(1, 101, 1)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::ManyPrintings(2))
        );
    }

    #[test]
    fn nothing_held_never_becomes_an_intake() {
        // The bug this whole enum exists to prevent: `from = None` in a
        // `MoveItem` means copies arriving from outside the system, so an
        // unresolvable selection must refuse, not fall back to it.
        assert_eq!(
            resolve_card(&[], id(9)),
            CardSource::Refuse(SkipReason::NoCopies)
        );
    }

    #[test]
    fn everything_already_in_the_destination_says_so() {
        let entries = [own(9, 100, 2)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::AlreadyThere)
        );
    }

    // ---- the grain and board a rendered row cannot show ----
    //
    // These four used to assert refusals. The write can address a grain and a
    // board now, so what they assert is that the *resolved source carries them*
    // — a `MoveItem` restated at the default grain beside a foil-only stack is
    // a write aimed at copies that do not exist, which is how one row used to
    // kill a whole batch.

    #[test]
    fn a_foil_only_card_moves_as_foil() {
        // `/my` shows OWNED 3 and `collection_view` shows HERE 3; neither says
        // "foil". Resolving to the default grain would reach `holding_take`,
        // find nothing, and roll the whole transaction back.
        let entries = [foil(1, 100, 3)];
        let expected = CardSource::Move(MoveSource {
            finish: Finish::Foil,
            ..match plain(1, 100) {
                CardSource::Move(m) => m,
                _ => unreachable!(),
            }
        });
        assert_eq!(resolve_card(&entries, id(9)), expected);
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9)),
            expected
        );
    }

    #[test]
    fn a_foil_stack_beside_a_plain_one_moves_the_plain_one() {
        // The row sums both (the view groups by printing+board). Both are
        // movable now, so the tie-break is what an unqualified "move this card"
        // means: the default grain.
        let entries = [foil(1, 100, 3), own(1, 100, 1)];
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9)),
            plain(1, 100)
        );
    }

    #[test]
    fn two_exotic_grains_with_no_default_is_the_one_refusal_left() {
        // 3 foil + 1 etched: nothing about the row says which the checkbox
        // meant, and inventing an answer is the thing this module may not do.
        let entries = [
            foil(1, 100, 3),
            HoldingLine {
                finish: Finish::Etched,
                ..own(1, 100, 1)
            },
        ];
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9)),
            CardSource::Refuse(SkipReason::Grain(2))
        );
    }

    #[test]
    fn a_sideboarded_my_row_moves_off_the_sideboard() {
        // `CardDetail::ownership` groups board away, so this looked movable
        // before the ungrouped read — and `holding_take` used to pin
        // `board = 'main'`, so the write took nothing.
        let entries = [on_board(Board::Side, 1, 100, 2)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Move(MoveSource {
                board: Board::Side,
                ..match plain(1, 100) {
                    CardSource::Move(m) => m,
                    _ => unreachable!(),
                }
            })
        );
    }

    #[test]
    fn a_my_row_split_across_boards_is_a_question_for_the_user() {
        // Two boards is two rows on the deck page; the `/my` row is one, and
        // picking for the user would move copies they can see but did not point
        // at.
        let entries = [own(1, 100, 2), on_board(Board::Side, 1, 100, 2)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::ManyBoards(2))
        );
    }

    #[test]
    fn a_collection_row_moves_the_board_it_names() {
        // The same two stacks, addressed from the collection page: each row
        // names its own board, so each resolves to its own copies.
        let entries = [own(1, 100, 2), on_board(Board::Side, 1, 100, 2)];
        let main = match plain(1, 100) {
            CardSource::Move(m) => m,
            _ => unreachable!(),
        };
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Side, id(9)),
            CardSource::Move(MoveSource {
                board: Board::Side,
                ..main.clone()
            })
        );
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9)),
            CardSource::Move(main)
        );
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(1)),
            CardSource::Refuse(SkipReason::AlreadyThere)
        );
    }

    #[test]
    fn a_stale_selection_is_refused_rather_than_aborting_the_batch() {
        // The copies were moved away in another tab after the row was checked.
        // Without the read this reached `holding_take` and took the batch with
        // it; the tray is long-lived by design, so this is not exotic.
        assert_eq!(
            resolve_held(&[], id(1), id(100), Board::Main, id(9)),
            CardSource::Refuse(SkipReason::NoCopies)
        );
        // …and so is a row whose *printing* is gone while the card remains.
        let entries = [own(1, 999, 2)];
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9)),
            CardSource::Refuse(SkipReason::NoCopies)
        );
    }

    #[test]
    fn every_refusal_says_what_is_wrong() {
        // A reason with no phrase is a refusal the user cannot act on.
        for reason in [
            SkipReason::AlreadyThere,
            SkipReason::Grain(2),
            SkipReason::NoCopies,
            SkipReason::ManyCollections(2),
            SkipReason::ManyPrintings(2),
            SkipReason::ManyBoards(2),
        ] {
            assert!(!reason.phrase().is_empty(), "{reason:?} has no phrase");
        }
        assert!(SkipReason::Grain(2).phrase().contains("finishes"));
        assert!(SkipReason::NoCopies.phrase().contains("reload"));
    }

    #[test]
    fn a_whole_batch_failure_gets_the_card_name_the_tray_showed() {
        // The chain: `move_batch` tags the failure with an item index,
        // `move_selection` swaps that for the entry's token, this swaps the
        // token for the name. Break any link and the toast names no card at
        // all — which is the state the batch-move task recorded as the defect.
        let names = [("held:a:b:main".to_string(), "Bolt".to_string())];
        assert_eq!(
            name_batch_failure("held:a:b:main: no copies to move", &names),
            "Bolt no copies to move"
        );
        // Unattributed, or a token no longer in the tray: passed through
        // untouched, because naming an innocent card is worse than naming none.
        assert_eq!(name_batch_failure("unauthorized", &names), "unauthorized");
        assert_eq!(
            name_batch_failure("card:zzz: no copies to move", &names),
            "card:zzz: no copies to move"
        );
    }

    fn suggestion(collection: u128, name: &str, shortfall: i32) -> SuggestedDestination {
        SuggestedDestination {
            collection_id: id(collection),
            collection_name: name.to_string(),
            desired: shortfall,
            present: 0,
            shortfall,
        }
    }

    #[test]
    fn suggestions_add_up_across_the_selection() {
        // Two cards, both wanted by the same deck: that deck outranks one that
        // wants a single copy of one of them.
        let merged = merge_suggestions(vec![
            vec![suggestion(1, "Deck", 1), suggestion(2, "Shoebox", 2)],
            vec![suggestion(1, "Deck", 2)],
        ]);
        let names: Vec<_> = merged.iter().map(|s| s.collection_name.as_str()).collect();
        assert_eq!(names, vec!["Deck", "Shoebox"]);
        assert_eq!(merged[0].shortfall, 3);
    }

    fn collection(n: u128, name: &str, is_inbox: bool) -> CollectionSummary {
        CollectionSummary {
            id: id(n),
            parent_id: None,
            kind: shared::CollectionKind::Binder,
            name: name.to_string(),
            is_inbox,
            position: 0.0,
            format: None,
        }
    }

    #[test]
    fn wanted_destinations_lead_and_everything_else_still_appears() {
        let all = [
            collection(1, "Deck", false),
            collection(2, "Inbox", true),
            collection(3, "Shoebox", false),
        ];
        let rows = picker_options(&[suggestion(3, "Shoebox", 2)], &all);
        let labels: Vec<_> = rows.iter().map(|r| r.dest.name.as_str()).collect();
        // Suggested first (with its hint), then Inbox, then the rest by name —
        // and the suggested one is not repeated in the tail.
        assert_eq!(labels, vec!["Shoebox", "Inbox", "Deck"]);
        assert_eq!(rows[0].hint.as_deref(), Some("wants 2"));
        assert!(rows[1].hint.is_none());
    }

    #[test]
    fn a_suggestion_for_a_deleted_collection_is_not_offered() {
        let rows = picker_options(&[suggestion(7, "Gone", 3)], &[collection(1, "Inbox", true)]);
        let labels: Vec<_> = rows.iter().map(|r| r.dest.name.as_str()).collect();
        assert_eq!(labels, vec!["Inbox"]);
    }

    #[test]
    fn the_toast_counts_copies_not_just_cards() {
        assert_eq!(moved_message(1, "🗂 Deck"), "Moved 1 card (1 copy) → 🗂 Deck");
        assert_eq!(
            moved_message(3, "🗂 Deck"),
            "Moved 3 cards (1 copy each) → 🗂 Deck"
        );
    }

    #[test]
    fn refusals_are_named_then_counted() {
        let one = [("Bolt".to_string(), SkipReason::ManyCollections(2))];
        assert_eq!(
            skipped_message(&one),
            "1 card wasn't moved: Bolt is in 2 collections — open one and select the row there"
        );

        let many = [
            ("Bolt".to_string(), SkipReason::AlreadyThere),
            ("Counterspell".to_string(), SkipReason::ManyBoards(2)),
            ("Ancestral".to_string(), SkipReason::NoCopies),
            ("Brainstorm".to_string(), SkipReason::NoCopies),
        ];
        assert_eq!(
            skipped_message(&many),
            "4 cards weren't moved: Bolt is already there; Counterspell sits on 2 boards — \
             open its deck and select the row you mean; and 2 more"
        );
    }

    #[test]
    fn a_refusal_keeps_its_name() {
        let skipped = [Skipped {
            token: "card:x".to_string(),
            reason: SkipReason::NoCopies,
        }];
        let names = [("card:x".to_string(), "Bolt".to_string())];
        assert_eq!(
            label_skips(&skipped, &names),
            vec![("Bolt".to_string(), SkipReason::NoCopies)]
        );
        // An unmatched token is still reported — never dropped.
        assert_eq!(label_skips(&skipped, &[]).len(), 1);
    }
}
