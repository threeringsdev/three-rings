//! Batch move — the selection tray's "Move to…" (specs/app-ui.md → Selection
//! tray; specs/collection-api.md → "Move (batch)").
//!
//! Seven things are worth knowing before editing this file.
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
//! **How many copies move is the user's answer, and the picker is where it is
//! given** (P6-150, maintainer ruling 2026-08-15). A tray entry is a *card*;
//! all-or-one-copy does not serve the point of moving cards between
//! collections. So the quantity is never guessed: an entry whose stack holds
//! exactly **one** copy needs no answer and moves directly, and anything larger
//! is refused as [`SkipReason::Several`] — which opens the picker, where every
//! row carries a stepper. The number then rides the wire on [`Pick`] and is
//! **validated server-side against the caller's actual ungrouped holdings**,
//! per entry: too many copies is that entry's own polite refusal
//! ([`SkipReason::NotEnough`]), never a clamp and never a dead batch. The tray's
//! pill still counts entries, because that is what it shows; the toasts count
//! **copies**, because that is what moved.
//!
//! **The undo is one action over N ledger rows.** A batch move writes one
//! `moves` row per item — the ledger has no batch id — so "one undo covering
//! the whole batch" is `CollectionStore::undo_moves`, which reverses the list in
//! a *single transaction*. Looping the single-move undo would be N
//! transactions, and a failure part-way would leave the batch half-reverted
//! behind a toast that said it was undone.
//!
//! **A question the batch cannot answer is asked, not reported.** Four of the
//! refusals below — [`SkipReason::ManyCollections`], [`ManyPrintings`],
//! [`ManyBoards`], [`Several`] — say "this row means more than one thing, and
//! this code may not choose for you". They open the **which-copies step**
//! ([`WhichCopiesDialog`]): the concrete stacks behind the card, **one row per
//! full grain** (collection, printing, board, finish, condition, language) with
//! its size and a quantity stepper, and a second submit through the *same*
//! write path — each picked row becomes a [`SelectionKey::Held`] entry carrying
//! a [`Pick`], which names one stack and one count, so `move_batch` and
//! `MoveItem` are untouched. The refusal toast survives for whoever cancels out
//! of the step, and for every refusal the step cannot answer (see
//! [`SkipReason::is_askable`]).
//!
//! [`ManyPrintings`]: SkipReason::ManyPrintings
//! [`ManyBoards`]: SkipReason::ManyBoards
//! [`Several`]: SkipReason::Several
//!
//! **Boards and off-default grains are moved, not refused, and the picker
//! splits them.** They were refused for one task because `moves` had no board
//! column and `holding_take` pinned `board = 'main'` at the default
//! finish/condition/language, so a move from a sideboard row — or a foil-only
//! stack — would have taken *different copies than the row the user checked*,
//! or none at all. The ledger now carries a board at each end and `MoveItem`
//! carries the full grain, so resolution passes on the stack it actually found
//! instead of restating a default. The last refusal that survived that work —
//! a stack holding several grains and no default one (2 foil + 1 etched) — is
//! gone with P6-150: those are **two rows** in the picker now, so the answer is
//! asked for rather than invented, and the old `SkipReason::Grain` has no
//! remaining case to name.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use shared::{Board, CollectionSummary, Condition, Finish, HoldingLine, Id, SuggestedDestination};

use crate::catalog::destination::{
    picker_order, Destination, DestinationChoice, DestinationList, DestinationOption,
};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::dialog::{
    Dialog, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader,
    DialogTitle,
};
use crate::components::ui::popover::{Popover, PopoverAlign, PopoverContent, PopoverTrigger};
use crate::components::ui::selection_tray::{SelectedCard, SelectionKey, SelectionState};
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};

// ------------------------------------------------------ what a batch takes ---

/// One entry of a batch move on the wire: what the row addresses, the oracle
/// card it is a copy of, and — once the user has been through the picker — the
/// exact copies and how many of them.
///
/// The oracle rides along because resolution needs the caller's holdings of
/// *that card* ungrouped, and a `Held` key names only a printing. Trusting the
/// client for it is safe by construction rather than by politeness: it is used
/// solely to look up the caller's own (RLS-scoped) holdings, and every
/// resolution path then re-checks that the named collection/printing/board
/// actually appears in what came back. A wrong oracle therefore produces a
/// [`SkipReason::NoCopies`] refusal, never a write somewhere else. The same
/// sentence covers [`Pick`]: it selects among the caller's own rows and is
/// re-checked against them, so a grain nobody holds refuses instead of writing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionItem {
    pub key: SelectionKey,
    pub oracle_id: Id,
    /// The picker's answer, when the user has given one. `None` is a plain tray
    /// row that has been asked nothing — it moves only if its stack holds
    /// exactly one copy, and is refused as [`SkipReason::Several`] otherwise
    /// (which is what opens the picker). Optional on the wire so the shape a
    /// tray row sends is exactly the shape it always sent.
    #[serde(default)]
    pub pick: Option<Pick>,
}

impl SelectionItem {
    /// How this entry is named in [`MoveOutcome`] — the tray token, plus the
    /// grain when one was picked.
    ///
    /// The suffix is not decoration. The picker's rows are **full grain**, so
    /// two picks on one card can differ only by finish: both are
    /// `held:<collection>:<printing>:main`, and an outcome that named them the
    /// same could not say which of the two moved. A `None` pick keeps exactly
    /// the token the tray, the DOM and every earlier task already use.
    pub fn token(&self) -> String {
        match &self.pick {
            None => self.key.token(),
            Some(p) => format!(
                "{}#{}/{}/{}",
                self.key.token(),
                p.finish.to_pg(),
                p.condition.to_pg(),
                p.language
            ),
        }
    }
}

impl From<&SelectedCard> for SelectionItem {
    fn from(card: &SelectedCard) -> Self {
        Self {
            key: card.key,
            oracle_id: card.oracle_id,
            pick: None,
        }
    }
}

/// The picker's answer for one row: which copies, and how many of them.
///
/// The grain completes what [`SelectionKey::Held`] leaves out — a `Held` key is
/// `(collection, printing, board)`, which is the grain a *rendered row* has,
/// and the picker's rows go one level finer because that is where "2 foil + 1
/// etched" stops being one thing. Quantity is the caller's here and only here:
/// it is checked against the stack the resolution actually finds, and a request
/// larger than the stack is refused per entry ([`SkipReason::NotEnough`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub finish: Finish,
    pub condition: Condition,
    pub language: String,
    pub quantity: i32,
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
///
/// Four of them are questions the *move* asks on the spot
/// ([`Self::is_askable`]); the rest end the batch's story for that card and
/// are reported as toasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    /// The copies are already in the destination.
    AlreadyThere,
    /// The stack holds more than one copy and nobody has said how many to move
    /// — the quantity question P6-150 exists to ask. The count is the whole
    /// stack, across every grain in it, because that is the number the row the
    /// user checked was showing.
    Several(i32),
    /// A picked row asked for more copies than its stack holds — the stepper's
    /// ceiling read a stack that has since shrunk. The count is what is really
    /// there.
    NotEnough(i32),
    /// A picked row asked for no copies at all. Unreachable from the picker
    /// (a zero row is not submitted) and refused rather than treated as a
    /// successful move of nothing.
    NoneRequested,
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
    ///
    /// The askable ones still name what is undecided (the count is the whole
    /// point — "in 2 collections" is why the row could not move) but no longer
    /// send the reader to another page: the move asks the question itself now,
    /// and this sentence is what they see after **declining** to answer it.
    /// "Pick the copies to move" is therefore an offer they can take by moving
    /// again, not an errand.
    pub fn phrase(self) -> String {
        match self {
            Self::AlreadyThere => "is already there".to_string(),
            Self::Several(n) => {
                format!("has {n} copies — pick how many to move")
            }
            Self::NotEnough(n) => {
                format!("has only {n} left — pick fewer copies")
            }
            Self::NoneRequested => "had no copies picked".to_string(),
            Self::NoCopies => "has no copies left to move — reload the page".to_string(),
            Self::NoLongerNeeded => "is no longer missing here — reload the page".to_string(),
            Self::ManyCollections(n) => {
                format!("is in {n} collections — pick the copies to move")
            }
            Self::ManyPrintings(n) => {
                format!("is held under {n} printings — pick the copies to move")
            }
            Self::ManyBoards(n) => {
                format!("sits on {n} boards — pick the copies to move")
            }
        }
    }

    /// Is this refusal a question the which-copies step can put to the user —
    /// *which* copies, or *how many*?
    ///
    /// The three `Many*` arms and `Several` are, and they are the only ones.
    /// The others are not narrower questions, they are different sentences
    /// entirely: `AlreadyThere` has nothing to choose between (the copies are
    /// at the destination), `NoCopies`/`NoLongerNeeded` name something the
    /// fresh server read just proved gone, and `NotEnough`/`NoneRequested` are
    /// answers to a question the picker **already asked** — re-opening it on
    /// them would loop the user through the same dialog on a stack that just
    /// told them its real size.
    ///
    /// `Grain` used to sit on the other side of this line, for a reason that
    /// expired with P6-150: the step's rows were `(collection, printing,
    /// board)`, so a stack holding several finish/condition/language grains was
    /// one row on it and offering it as a choice would have shown rows the user
    /// could not tell apart. The rows are full grain now, so that stack is
    /// simply several rows, and the variant has no case left to name.
    pub fn is_askable(self) -> bool {
        matches!(
            self,
            Self::ManyCollections(_)
                | Self::ManyPrintings(_)
                | Self::ManyBoards(_)
                | Self::Several(_)
        )
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

/// Turn the stack a key resolved to into the copies that will actually move.
///
/// **This is where quantity is decided, and it decides it exactly twice.**
///
/// * **Nobody asked** (`pick = None`, a plain tray row): a stack holding
///   exactly one copy has one answer and moves; anything larger is
///   [`SkipReason::Several`], which opens the picker. Guessing "one of them"
///   was the old behavior and is what P6-150 removed — a row reading 3 copies
///   that quietly moves 1 is a move the user did not ask for, and the default
///   grain it used to prefer could be a *different copy* than the row's biggest
///   stack. Note that this is also why `Grain` is gone: several grains means
///   the total exceeds one, so the question asked is "how many, from which of
///   these rows", not a refusal.
/// * **The picker answered** (`pick = Some`): the named grain must still be
///   there (else [`SkipReason::NoCopies`] — it emptied between the ask and the
///   submit), the count must be at least one, and it must fit in what the
///   stack **actually** holds now. A request over that ceiling is refused with
///   the real size rather than clamped: a silent clamp would move a different
///   number of cards than the dialog said, behind a success toast.
fn take(stack: &[&HoldingLine], pick: Option<&Pick>) -> CardSource {
    let Some(pick) = pick else {
        let total: i32 = stack.iter().map(|h| h.quantity).sum();
        return match stack {
            // Unreachable through either caller (both refuse an empty stack as
            // `NoCopies` first) and matched anyway, because the alternative is
            // a "has 0 copies — pick how many to move" toast.
            [] => CardSource::Refuse(SkipReason::NoCopies),
            [only] if only.quantity == 1 => CardSource::Move {
                source: (*only).into(),
                quantity: 1,
            },
            _ => CardSource::Refuse(SkipReason::Several(total)),
        };
    };
    let Some(h) = stack.iter().find(|h| {
        h.finish == pick.finish && h.condition == pick.condition && h.language == pick.language
    }) else {
        return CardSource::Refuse(SkipReason::NoCopies);
    };
    if pick.quantity < 1 {
        return CardSource::Refuse(SkipReason::NoneRequested);
    }
    if pick.quantity > h.quantity {
        return CardSource::Refuse(SkipReason::NotEnough(h.quantity));
    }
    CardSource::Move {
        source: (*h).into(),
        quantity: pick.quantity,
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
    /// The one unambiguous candidate stack, and how many of its copies move.
    /// The quantity is separate from [`MoveSource`] because that type is also
    /// the pull path's grain (`backend::pull_plan`), which carries its own,
    /// differently-derived count.
    Move {
        source: MoveSource,
        quantity: i32,
    },
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
/// candidate stack, and (unless the picker said otherwise) exactly one copy in
/// it. Anything else is a question only the user can answer, and the enum's
/// whole purpose is that this code cannot invent an answer.
pub fn resolve_card(holdings: &[HoldingLine], to: Id, pick: Option<&Pick>) -> CardSource {
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
    take(&candidates, pick)
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
    pick: Option<&Pick>,
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
    take(&stack, pick)
}

/// Resolve one batch entry against the caller's holdings, **spending what it
/// takes** so the rest of the batch sees what is left.
///
/// **The snapshot is shared, and that is the bug this closes.** `move_selection`
/// reads a card's holdings once per *oracle* (the tray can hold the same card
/// twice, and one read serves both), then validates every entry against it. With
/// the read never decremented, two entries drawing on the same stack each
/// validated against the **full** pile: both passed, both became `MoveItem`s,
/// and the second `holding_take` inside `move_batch`'s single transaction hit
/// `Conflict("insufficient copies to move")` — rolling back the **whole batch**,
/// including every unrelated card in it. That is precisely the failure the
/// per-entry refusal contract exists to prevent, arrived at from the other side.
///
/// Spending here makes the joint overdraw an ordinary per-entry refusal: the
/// later item sees the reduced stack and comes back `NotEnough(what is left)`
/// or `NoCopies`, by name, while everything else in the batch still moves. It is
/// also honest about what the batch is doing — inside one transaction, the
/// copies an earlier item took really are gone by the time a later one runs.
pub fn resolve_item(holdings: &mut [HoldingLine], item: &SelectionItem, to: Id) -> CardSource {
    let pick = item.pick.as_ref();
    let source = match item.key {
        SelectionKey::Held {
            collection_id,
            printing_id,
            board,
        } => resolve_held(holdings, collection_id, printing_id, board, to, pick),
        SelectionKey::Card { .. } => resolve_card(holdings, to, pick),
    };
    if let CardSource::Move { source, quantity } = &source {
        spend(holdings, source, *quantity);
    }
    source
}

/// Take `quantity` copies off the stack `source` names, in the snapshot the
/// rest of the batch validates against.
///
/// Addressed by the full grain, exactly as `holding_take` will be — a row that
/// does not match is left alone rather than approximated, and a quantity that
/// somehow exceeds it clamps to zero here (the write would refuse it first;
/// this is a snapshot, and a negative count in it would make the *next* entry's
/// refusal a lie).
///
/// **Spending the *first* match is correct because the database says there is
/// only one.** `holdings_uniq` makes
/// `(collection_id, printing_id, finish, condition, language, board)` unique,
/// so the six fields matched here identify at most one row of
/// `holdings_of_oracle`'s output; the read itself neither groups nor filters on
/// quantity (`movable` does that, downstream), so a second matching row would
/// mean the constraint is gone — and this would then spend one of two rows the
/// write is about to treat as one. If that constraint is ever relaxed, this
/// must fold the matches instead of taking the first.
fn spend(holdings: &mut [HoldingLine], source: &MoveSource, quantity: i32) {
    if let Some(h) = holdings.iter_mut().find(|h| {
        h.collection_id == source.from
            && h.printing_id == source.printing_id
            && h.board == source.board
            && h.finish == source.finish
            && h.condition == source.condition
            && h.language == source.language
    }) {
        h.quantity = (h.quantity - quantity).max(0);
    }
}

// ------------------------------------------------- the which-copies step ---
//
// Everything between here and `name_batch_failure` is the disambiguation
// pipeline, in the order the data flows: the stacks a card's copies actually
// sit in (read), the cards a batch could not resolve (split out of the
// outcome), the two joined into rows (the dialog's model), the rows the user
// ticked (picks), and those picks turned back into wire items and tray tokens.
// All of it is pure, and deliberately so — the dialog does no arithmetic.

/// One physical stack of a card's copies: a **full grain** — collection,
/// printing, board, finish, condition, language — and how many copies sit in
/// it.
///
/// **The grain is one level finer than [`SelectionKey::Held`], and that is the
/// P6-150 change.** A `Held` key is what a *rendered row* knows
/// (`collection_view` groups by printing and board), so the step used to list
/// its rows at that grain and sum the finishes inside them — which made "two
/// foils beside one etched" a single row nobody could act on, and left
/// `SkipReason::Grain` as a dead end. Splitting here is what lets the picker ask the question
/// instead: each row goes back as a `Held` key **plus** a [`Pick`] naming the
/// grain and the count, so `resolve_held` still resolves it against the same
/// fresh holdings read and no new write path exists for any of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackTally {
    pub collection_id: Id,
    pub printing_id: Id,
    pub board: Board,
    pub finish: Finish,
    pub condition: Condition,
    pub language: String,
    pub quantity: i32,
}

/// The stacks a user can point at, out of ungrouped holdings.
///
/// Input is `holdings_of_oracle`'s own shape (the read that already backs
/// resolution), so this adds no query — and since that read is already one row
/// per grain, this is a filter, not a rollup. It still merges equal grains
/// rather than trusting that: two rows the picker cannot tell apart would be
/// two steppers over the same copies, which is a way to move more than the
/// stack holds.
///
/// Order is the input's order — the hosted read sorts by `(collection,
/// printing, board, finish)` — so the rows the dialog lists are stable between
/// two opens rather than hash-shuffled.
pub fn stacks_of(holdings: &[HoldingLine]) -> Vec<StackTally> {
    let mut out: Vec<StackTally> = Vec::new();
    for h in holdings.iter().filter(|h| movable(h)) {
        match out.iter_mut().find(|t| {
            t.collection_id == h.collection_id
                && t.printing_id == h.printing_id
                && t.board == h.board
                && t.finish == h.finish
                && t.condition == h.condition
                && t.language == h.language
        }) {
            Some(t) => t.quantity += h.quantity,
            None => out.push(StackTally {
                collection_id: h.collection_id,
                printing_id: h.printing_id,
                board: h.board,
                finish: h.finish,
                condition: h.condition,
                language: h.language.clone(),
                quantity: h.quantity,
            }),
        }
    }
    out
}

/// A stack with the words a human needs to tell it from the next one.
///
/// `printing` is `Option` and is filled **only when a card's copies span more
/// than one printing** — the read that builds these skips the catalog lookup
/// entirely otherwise (`crate::selection_stacks`). A set/number chip on a card
/// held under one printing distinguishes nothing and costs a card-detail read
/// per card to say it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyStack {
    pub collection_id: Id,
    pub collection_name: String,
    pub printing_id: Id,
    pub printing: Option<String>,
    pub board: Board,
    pub finish: Finish,
    pub condition: Condition,
    pub language: String,
    pub quantity: i32,
}

impl CopyStack {
    /// The row as an answer to move `quantity` of it.
    pub fn pick(&self, quantity: i32) -> Pick {
        Pick {
            finish: self.finish,
            condition: self.condition,
            language: self.language.clone(),
            quantity,
        }
    }
}

/// Every stack of one card, as the step's read answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardStacks {
    pub oracle_id: Id,
    pub stacks: Vec<CopyStack>,
}

/// `crate::selection_stacks`' payload, behind a named field for the reason
/// [`crate::catalog::destination::CollectionListPayload`] documents at length:
/// a bare `Result<Vec<_>, _>` serializes as `{"Ok":[…]}`, which is a universal
/// key any other list-shaped resource can decode. This one is never serialized
/// today (the dialog is client-only, like the tray around it), and the wrapper
/// is what keeps that from being the thing holding the property up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StacksPayload {
    pub cards: Vec<CardStacks>,
}

/// The set/number chip: `MH3 #123`. Set-less printings (the catalog allows a
/// null set code) keep the number, which is still a distinction.
pub fn printing_label(set_code: Option<&str>, collector_number: &str) -> String {
    match set_code {
        Some(code) => format!("{} #{collector_number}", code.to_uppercase()),
        None => format!("#{collector_number}"),
    }
}

/// One **card** the batch could not resolve — not one tray entry — carrying the
/// name it was shown under so the step and the fallback toast say the same word
/// for it.
///
/// **A card, because the tray can hold the same copies twice.** `/my` and a
/// collection page are two views of one shelf, and selecting a card on each
/// makes two entries whose copies are the same physical cards (the P6-150
/// ruling's question 3). Left apart they became two sections offering the same
/// stacks, two steppers over one pile, and — since both address the same
/// `(collection, printing, board)` — two wire items the server could not tell
/// apart in its own outcome. [`split_skips`] merges them here instead, which is
/// the only place that knows both entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskedCard {
    /// The **original** tray entries this section answers for, in tray order —
    /// each with the refusal it earned, because two entries for one card can be
    /// refused for different reasons and each one's sentence is owed to the
    /// user if it goes unanswered.
    pub entries: Vec<AskedEntry>,
    pub oracle_id: Id,
    pub name: String,
}

/// One tray entry inside a merged section: what it addressed, and why the batch
/// refused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AskedEntry {
    /// The tray key. It is what the step offers rows for — a `Held` entry means
    /// "these copies, here", a `Card` entry names no place and so reaches every
    /// stack of its oracle ([`card_choices`]) — and it is what a moved row is
    /// attributed back to ([`StackPick::of`]).
    pub key: SelectionKey,
    pub reason: SkipReason,
}

impl AskedCard {
    /// Every tray token this section answers for.
    pub fn tokens(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.key.token()).collect()
    }

    /// The entries whose copies include this row — the ones a pick on it
    /// answers for. See [`StackPick::of`] for why this is per row and not per
    /// section.
    fn covering(&self, row: &CopyStack) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| addressed_by(&e.key, row))
            .map(|e| e.key.token())
            .collect()
    }
}

/// Split a finished batch's refusals into the ones the step can ask about and
/// the ones that go straight to a toast, in one pass over the outcome.
///
/// The tray entries come in because a `Skipped` names a token and nothing else
/// (by design — the server ships back no strings the client already holds), and
/// the step needs the card's name and oracle id. A token with no matching
/// entry cannot happen (the batch was built from the tray) but is treated as
/// un-askable rather than dropped: it still reaches the toast through
/// [`label_skips`], which has its own "A card" fallback. Silently losing a
/// refusal is the one outcome this whole reporting path exists to prevent.
///
/// **Askable refusals for the same card merge into one question** (the ruling's
/// question 3). Two tray entries over the same copies — the `/my` row and the
/// collection row — are two refusals here, and asking twice would be asking the
/// user to apportion one pile across two identical lists.
///
/// **The merge is a display merge only, and every entry keeps its own
/// identity.** Two `Held` entries of one card are *not* the same copies — a
/// deck's mainboard and sideboard rows, or two binders — and the union of their
/// stacks is one honest list to choose from, but retiring one because the other
/// moved would drop a tray entry that gave up nothing, silently. So each entry
/// keeps its key and its reason here, a pick is attributed to the entries whose
/// copies it actually came from ([`StackPick::of`]), and an entry nothing came
/// out of is reported like any other refusal ([`unanswered`]). Nothing merges
/// across cards, and nothing merges into a refusal the step cannot ask about.
pub fn split_skips(
    skipped: &[Skipped],
    entries: &[SelectedCard],
) -> (Vec<AskedCard>, Vec<Skipped>) {
    let mut ask: Vec<AskedCard> = Vec::new();
    let mut tell = Vec::new();
    for s in skipped {
        let entry = entries.iter().find(|c| c.key.token() == s.token);
        match (s.reason.is_askable(), entry) {
            (true, Some(c)) => {
                let asked = AskedEntry {
                    key: c.key,
                    reason: s.reason,
                };
                match ask.iter_mut().find(|a| a.oracle_id == c.oracle_id) {
                    Some(a) => {
                        if !a.entries.iter().any(|e| e.key == c.key) {
                            a.entries.push(asked);
                        }
                    }
                    None => ask.push(AskedCard {
                        entries: vec![asked],
                        oracle_id: c.oracle_id,
                        name: c.name.clone(),
                    }),
                }
            }
            _ => tell.push(s.clone()),
        }
    }
    (ask, tell)
}

/// One card's section of the dialog: what was asked, and the rows to answer it
/// with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardChoices {
    pub card: AskedCard,
    pub rows: Vec<CopyStack>,
}

/// Join the cards the batch refused to the stacks the follow-up read found.
///
/// Three filters, all of them rules the resolution applies too, restated here
/// because the read behind `stacks` is a plain "everything you hold of this
/// card" and knows nothing about the move:
///
/// * **the destination is not a source** — offering "move these from the deck
///   you are moving into" is a row whose only outcomes are a no-op or an
///   `AlreadyThere` refusal;
/// * **empty stacks are not rows** — `stacks_of` already drops them, and this
///   restates it because the read and the write are separate requests;
/// * **an entry asks only about what it addressed** — a `Held` entry is a
///   collection-page row, which means "these copies, here". The stacks read
///   answers per *oracle*, so without this a row selected in one binder would
///   offer to move copies out of another one the user never pointed at. Its
///   grains are still several rows, which is the whole point of asking. A
///   `Card` entry (`/my`) is the opposite: it names no place, so every place is
///   a candidate. A merged section ([`AskedCard::keys`]) offers the **union**,
///   which is what a `/my` row plus a collection row for the same card asked
///   for between them.
///
/// **Each stack appears once however many keys reach it.** The rows are the
/// payload's own list, filtered in one pass, and `stacks_of` already made that
/// list one entry per full grain — so a stack two merged keys both address is
/// one row with one stepper, not two steppers over the same pile.
///
/// A card whose rows all fall away keeps its section with zero rows rather than
/// disappearing from the dialog: its copies moved or vanished between the batch
/// and this read, and a card silently missing from a list the user was asked to
/// act on is exactly the "did that land?" state the toasts exist to refuse.
pub fn card_choices(cards: Vec<AskedCard>, stacks: &[CardStacks], to: Id) -> Vec<CardChoices> {
    cards
        .into_iter()
        .map(|card| {
            let rows = stacks
                .iter()
                .find(|s| s.oracle_id == card.oracle_id)
                .map(|s| {
                    s.stacks
                        .iter()
                        .filter(|r| {
                            r.collection_id != to
                                && r.quantity > 0
                                && card.entries.iter().any(|e| addressed_by(&e.key, r))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            CardChoices { card, rows }
        })
        .collect()
}

/// Does this stack sit inside what one tray entry pointed at?
fn addressed_by(key: &SelectionKey, row: &CopyStack) -> bool {
    match key {
        SelectionKey::Held {
            collection_id,
            printing_id,
            board,
        } => {
            row.collection_id == *collection_id
                && row.printing_id == *printing_id
                && row.board == *board
        }
        SelectionKey::Card { .. } => true,
    }
}

/// One row the user chose copies from: the stack, how many of it, plus the tray
/// entry and name it answers for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPick {
    /// The original tray tokens this answers — the merged entries whose copies
    /// this row actually is ([`StackPick::of`]), because one row can be the
    /// answer to a `/my` entry and a collection entry at once, and both stop
    /// being questions when it moves.
    pub tokens: Vec<String>,
    pub name: String,
    pub oracle_id: Id,
    pub row: CopyStack,
    /// Copies to move off this stack. Always ≥ 1: a row left at zero is not a
    /// pick, and [`picks_of`] drops it before anything sees it.
    pub quantity: i32,
}

impl StackPick {
    /// The pick as a tray-style key. Precise by construction at the row's own
    /// grain, with the finish/condition/language the key cannot carry riding
    /// alongside it in [`Self::pick`].
    pub fn key(&self) -> SelectionKey {
        SelectionKey::Held {
            collection_id: self.row.collection_id,
            printing_id: self.row.printing_id,
            board: self.row.board,
        }
    }

    /// The pick as one wire item.
    pub fn item(&self) -> SelectionItem {
        SelectionItem {
            key: self.key(),
            oracle_id: self.oracle_id,
            pick: Some(self.row.pick(self.quantity)),
        }
    }

    /// How the server will name this pick back (see [`SelectionItem::token`]).
    pub fn item_token(&self) -> String {
        self.item().token()
    }

    /// A pick, attributed to **the entries whose copies it is** — not to every
    /// entry in its section.
    ///
    /// The section is a display merge over one card, and two `Held` entries in
    /// it (a deck's mainboard and sideboard rows; two binders) address
    /// genuinely different copies. Attributing a row to all of them let a user
    /// who zeroed one entry's rows and confirmed another's watch the zeroed
    /// entry leave the tray having moved nothing and said nothing — the silent
    /// drop this file exists to prevent. `addressed_by` is the same containment
    /// the rows were selected with: a `Card` entry contains every stack of its
    /// oracle, a `Held` entry exactly its own.
    fn of(card: &AskedCard, row: &CopyStack, quantity: i32) -> Self {
        Self {
            tokens: card.covering(row),
            name: card.name.clone(),
            oracle_id: card.oracle_id,
            row: row.clone(),
            quantity,
        }
    }
}

/// The dialog's state — its sections and the stepper value standing against
/// each row, in row order — as the picks a submit is made of.
///
/// Pure, and the dialog does no arithmetic of its own: it renders `sections`
/// and owns a flat `Vec<i32>` of counts, and this is the one place that knows
/// the two line up. **Rows left at zero are not picks** — that is how a user
/// declines a stack now that every row starts at one, and shipping a
/// zero-quantity item would only earn a [`SkipReason::NoneRequested`] refusal
/// naming a card the user deliberately left alone.
pub fn picks_of(sections: &[CardChoices], counts: &[i32]) -> Vec<StackPick> {
    let mut out = Vec::new();
    let mut i = 0;
    for section in sections {
        for row in &section.rows {
            let quantity = counts.get(i).copied().unwrap_or(0);
            if quantity > 0 {
                out.push(StackPick::of(&section.card, row, quantity));
            }
            i += 1;
        }
    }
    out
}

/// The picks as a batch the existing write already takes.
///
/// **No new mutation exists for this step, and none is needed.** A `Held` key
/// carries `(collection, printing, board)` and the [`Pick`] carries the rest —
/// the grain and the count — so the server resolves it through the same
/// `resolve_held` a collection-page row uses, against its own fresh holdings
/// read. Two rows picked on one card are two items, so a user who wants two
/// foils from one binder and one plain from another gets exactly that.
pub fn picked_items(picks: &[StackPick]) -> Vec<SelectionItem> {
    picks.iter().map(StackPick::item).collect()
}

/// Names for the second pass's refusal toast, keyed by the tokens *that* pass
/// answers in (grain-suffixed `held:…`), not the tray's.
pub fn picked_names(picks: &[StackPick]) -> Vec<(String, String)> {
    picks
        .iter()
        .map(|p| (p.item_token(), p.name.clone()))
        .collect()
}

/// Copies the server actually reported moved, out of what was picked.
///
/// Counted from `moved`, never from the picks: a pick the server refused
/// (`NotEnough`, a stack that emptied) must not be claimed as a copy that
/// landed.
pub fn moved_copies(picks: &[StackPick], moved: &[String]) -> i32 {
    picks
        .iter()
        .filter(|p| moved.contains(&p.item_token()))
        .map(|p| p.quantity)
        .sum()
}

/// The distinct **cards** those landed copies belonged to — "of 2 cards".
///
/// Counted by oracle, not by tray token: the tray can hold one card twice (the
/// `/my` row and the collection row for the same copies), and a sentence
/// reading "2 cards" over one card's copies is exactly the kind of statement
/// about the user's collection these toasts exist not to make. Sections are
/// merged per oracle ([`split_skips`]), so this is also one per section.
pub fn moved_cards(picks: &[StackPick], moved: &[String]) -> usize {
    let mut oracles: Vec<Id> = picks
        .iter()
        .filter(|p| moved.contains(&p.item_token()))
        .map(|p| p.oracle_id)
        .collect();
    oracles.sort_unstable();
    oracles.dedup();
    oracles.len()
}

/// The **tray** tokens a finished disambiguation move answered.
///
/// The second pass moves `held:` tokens; the tray still holds the `card:`
/// entries those answer. Without this translation the pill would keep counting
/// a card whose copies just moved — [`tokens_to_drop`] would look for
/// `held:…` tokens among `card:…` entries and match nothing.
///
/// **One moved stack retires the entry**, even where the user picked two rows
/// and only one moved: the entry's question was *which copies*, the user
/// answered it, and what did not move is named in its own refusal toast. An
/// entry left checked after that would be inviting the same question again.
///
/// **It retires every entry the moved copies belonged to, and only those.** A
/// `/my` row and a collection row over the same copies are one question here,
/// so a row that answers both retires both — leaving one checked would have the
/// pill still counting a card the user just dealt with. But two `Held` entries
/// in one section are *different* copies, and the one that gave nothing up
/// stays checked and is reported ([`StackPick::of`], [`unanswered`]).
pub fn answered_tokens(picks: &[StackPick], moved: &[String]) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for p in picks.iter().filter(|p| moved.contains(&p.item_token())) {
        for token in &p.tokens {
            if !tokens.contains(token) {
                tokens.push(token.clone());
            }
        }
    }
    tokens
}

/// The **tray entries** the step asked about that nothing came out of.
///
/// Two callers, one sentence: **cancelling** is this with no picks at all, and
/// **submitting a partial answer** is this with the picks that were made. The
/// second one is the case that would otherwise go silent — `answered` suppresses
/// the close toast, so a user who ticked one of three cards and hit the button
/// would get a confirmation for the one and nothing whatsoever about the other
/// two, which is the "did that land?" state every toast in this file exists to
/// refuse.
///
/// **Per entry, not per section, and each with its own reason.** A section can
/// stand for several tray entries, and an entry whose rows were all left at
/// zero is still checked in the tray afterwards — so it is owed the sentence
/// the batch refused it with, even when a sibling entry of the same card moved.
/// Identical (name, reason) pairs collapse: two entries refused the same way
/// for the same card say it once.
pub fn unanswered(cards: &[AskedCard], picks: &[StackPick]) -> Vec<(String, SkipReason)> {
    let mut out: Vec<(String, SkipReason)> = Vec::new();
    for card in cards {
        for entry in &card.entries {
            let token = entry.key.token();
            if picks.iter().any(|p| p.tokens.contains(&token)) {
                continue;
            }
            let line = (card.name.clone(), entry.reason);
            if !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out
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

/// The confirmation toast, for **both** submit paths.
///
/// **Both numbers are said out loud and neither is inferred**, which is what
/// P6-150 required and what one sentence can now carry for both passes. The
/// old pair could not: the batch's message counted tray entries and asserted
/// "1 copy each" — true only while quantity was fixed at one — and the picker
/// grew a second sentence ("across 2 cards") for the case that broke it. With
/// quantity chosen per row, copies and cards are simply two different counts of
/// the same move: 5 copies of 3 cards is 5 copies of 3 cards whichever path
/// produced it, and the one-card case still names the card count ("of 1 card")
/// so the two shapes read as the same sentence.
///
/// Both counts come from what the **server reported moved**, never from what
/// was asked for: a refused entry must not be claimed as a copy that landed.
pub fn moved_message(copies: i32, cards: usize, destination: &str) -> String {
    let copies = if copies == 1 {
        "1 copy".to_string()
    } else {
        format!("{copies} copies")
    };
    let cards = if cards == 1 {
        "1 card".to_string()
    } else {
        format!("{cards} cards")
    };
    format!("Moved {copies} of {cards} → {destination}")
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
    let report = MoveReport {
        toast,
        tree,
        revision,
        last_move,
        selection,
    };

    let open = RwSignal::new(false);
    let pending = RwSignal::new(false);
    // The which-copies step. `step` is the question (and its answer's audience:
    // the toast raised if the user walks away); `step_open` is the dialog's own
    // open state, shared so the batch can open it programmatically.
    let step = RwSignal::new(None::<WhichCopies>);
    let step_open = RwSignal::new(false);
    let answered = RwSignal::new(false);

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
    // **Whether either read behind the picker's rows hasn't resolved yet** —
    // `DestinationList`'s `loading` prop (P6-011/P6-163, the same fix the
    // tree's own `Move to…` dialog got: see `TreeDialogs` in
    // `tree_manage.rs`). Before this, the `Transition` below put its own
    // "Loading collections…" line beside `CommandEmpty`'s registry-inferred
    // "No collection to move to." — with nothing awaited yet, the registry
    // had zero items either way, so both rendered at once. Starts `true` for
    // the same reason `tree_manage.rs`'s `load_loading` does: a `Resource`
    // genuinely is pending before its first read, not "assume already
    // loaded". `suggested` is included even though it degrades silently on
    // its own failure (a ranking hint, not a correctness requirement — see
    // the `Suspend` below): the `Transition` fallback shows until *both*
    // resources resolve, so `loading` has to track both or it would clear
    // early and let the registry-inferred line show while rows are still
    // pending.
    let load_loading = RwSignal::new(true);
    Effect::new(move |_| {
        let collections_snapshot = collections.get();
        let pending = collections_snapshot.is_none() || suggested.get().is_none();
        if pending != load_loading.get_untracked() {
            load_loading.set(pending);
        }
        let failed = matches!(collections_snapshot, Some(Err(_)));
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
                    // Drop what moved, and what the server just proved gone
                    // (`NoCopies`); leave every other refusal checked — that is
                    // still work to do, and the step or the toast names it. See
                    // `tokens_to_drop`'s own doc for the line between the two.
                    report.moved(&outcome, &dest, &tokens_to_drop(&outcome));
                    // The refusals split here: what the step can ask about
                    // becomes the dialog, everything else is a toast exactly as
                    // before. Unambiguous entries in the same batch have
                    // already moved above — the step covers only the remainder,
                    // which is the honest-partial shape Pull established.
                    let (ask, tell) = split_skips(&outcome.skipped, &entries);
                    report.refused(&label_skips(&tell, &names));
                    if !ask.is_empty() {
                        step.set(Some(WhichCopies { dest, cards: ask }));
                        answered.set(false);
                        step_open.set(true);
                    }
                }
                Err(e) => report.failed(&e, &names),
            }
        });
    });

    // The second pass: the picked stacks, as `Held` items, through the same
    // server fn. Nothing here can be ambiguous again (a `Held` key names one
    // stack), so its refusals are reported and never re-asked.
    let confirm = Callback::new(move |picks: Vec<StackPick>| {
        let Some(WhichCopies { dest, cards }) = step.get_untracked() else {
            return;
        };
        if picks.is_empty() || pending.get_untracked() {
            return;
        }
        // A partial answer is still a refusal for the rest. Closing on `answered`
        // suppresses the cancel toast, so without this a user who ticked one of
        // three asked cards would hear about that one and nothing at all about
        // the other two — while they stay checked in the tray.
        report.refused(&unanswered(&cards, &picks));
        let names = picked_names(&picks);
        let items = picked_items(&picks);
        pending.set(true);
        spawn_local(async move {
            let result = crate::move_selection(dest.id, items).await;
            pending.set(false);
            match result {
                Ok(outcome) => {
                    // Both halves are said in the picks' own terms: the tray
                    // holds `card:` entries and this pass answered in
                    // grain-suffixed `held:` tokens, one card can have
                    // contributed several of them, and each of those carried a
                    // count of its own.
                    report.moved_as(
                        &outcome,
                        &answered_tokens(&picks, &outcome.moved),
                        moved_message(
                            moved_copies(&picks, &outcome.moved),
                            moved_cards(&picks, &outcome.moved),
                            &dest.label(),
                        ),
                    );
                    report.refused(&label_skips(&outcome.skipped, &names));
                }
                Err(e) => report.failed(&e, &names),
            }
        });
    });

    // Walking away from the step is not a silent drop: the refusals it was
    // opened for are raised as the toast they would have been. Written as a
    // close *transition* (not a `Show`) so every exit — Cancel, the ✕, the
    // backdrop, Escape — lands on the same line.
    Effect::new(move |was_open: Option<bool>| {
        let now = step_open.get();
        if was_open == Some(true) && !now {
            if !answered.get_untracked() {
                if let Some(s) = step.get_untracked() {
                    // Cancelling is the no-picks case of the same sentence the
                    // partial submit above raises.
                    report.refused(&unanswered(&s.cards, &[]));
                }
            }
            step.set(None);
        }
        now
    });

    view! {
        <WhichCopiesDialog step open=step_open answered on_confirm=confirm />
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
                // No `empty` override: `DestinationList`'s own default ("No
                // collection matches.") is the true sentence here too.
                // `empty` can only ever speak about *filtering* (its own doc)
                // — "No collection to move to." was a claim about having
                // nowhere to move copies, which typing a search term that
                // matches nothing does not make true. And the zero-collections
                // case this used to (over-)cover cannot happen here:
                // `collection_list()` — the read `collections` above resolves
                // through — provisions the caller's undeletable Inbox row as a
                // side effect (`CollectionStore::list_collections` calls
                // `ensure_inbox` before it returns rows; collection-api.md →
                // "Inbox provisioning"), so this list is never really empty,
                // only ever filtered down to nothing.
                <DestinationList failed=load_failed loading=load_loading>
                    // Same boundary the catalog's picker uses, and for the same
                    // reason: the rows come from resources, and only a
                    // suspense boundary keeps a render in step with them. The
                    // fallback is empty, not a "Loading…" line of its own —
                    // `load_loading` above already puts that message on
                    // `DestinationList`'s `CommandEmpty` (P6-163).
                    <Transition fallback=|| ()>
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

/// Everything a finished move touches besides the write itself: the tray, the
/// views the copies left and arrived in, the palette's undo memory, and the
/// toasts.
///
/// `Copy`, and a struct rather than five captured variables, because there are
/// now **two** submit paths — the batch and the which-copies step's second pass
/// — and they must report identically. A second hand-rolled copy of this block
/// is how a step that "mostly works" ends up not refetching the page it
/// changed, or not offering an Undo.
#[derive(Clone, Copy)]
struct MoveReport {
    toast: ToastHandle,
    tree: Option<crate::my::tree::CollectionTreeResource>,
    revision: Option<HoldingsRevision>,
    last_move: Option<crate::components::palette::LastMoveState>,
    selection: SelectionState,
}

impl MoveReport {
    /// The batch's own reporting: [`moved_message`], whose unit is the tray
    /// entry.
    fn moved(self, outcome: &MoveOutcome, dest: &Destination, drop_tokens: &[String]) {
        // One copy per entry on this path by construction: a plain tray row
        // carries no [`Pick`], and resolution moves such a row only when its
        // whole stack is a single copy (anything larger is `Several`, which
        // opens the picker). So copies and cards are the same number here, and
        // both are counted rather than one of them asserted.
        let moved = outcome.moved.len();
        self.moved_as(
            outcome,
            drop_tokens,
            moved_message(moved as i32, moved, &dest.label()),
        );
    }

    /// Reconcile the tray, invalidate what the write changed, and raise the one
    /// toast that undoes the whole batch.
    ///
    /// **Both the tokens and the sentence are the caller's**, because the two
    /// submit paths count in different units. The batch answers in tray tokens
    /// where one entry is one card is one copy; the which-copies pass answers in
    /// `held:` stack tokens, several of which can belong to one card — so it
    /// brings [`answered_tokens`] and [`picked_message`]. Everything below this
    /// line is identical for both, which is the point of the split.
    ///
    /// The tray reconciliation happens whether or not anything moved: a batch
    /// that moved nothing can still have proved an entry gone.
    fn moved_as(self, outcome: &MoveOutcome, drop_tokens: &[String], message: String) {
        self.selection.remove_tokens(drop_tokens);
        if outcome.move_ids.is_empty() {
            return;
        }
        if let Some(t) = self.tree {
            t.0.refetch();
        }
        if let Some(r) = self.revision {
            r.bump();
        }
        let move_ids = outcome.move_ids.clone();
        // The whole batch is one undo, for ⌘K as for the toast.
        crate::components::palette::note_last_move(self.last_move, move_ids.clone());
        let (toast, tree, revision, last_move) =
            (self.toast, self.tree, self.revision, self.last_move);
        toast.show(
            ToastOptions::message(message)
                .kind(ToastKind::Success)
                .action(
                    "Undo",
                    Callback::new(move |()| {
                        undo(toast, tree, revision, last_move, move_ids.clone())
                    }),
                ),
        );
    }

    /// The refusal toast, or nothing at all when there is nothing to refuse.
    fn refused(self, refused: &[(String, SkipReason)]) {
        if refused.is_empty() {
            return;
        }
        self.toast
            .show(ToastOptions::message(skipped_message(refused)).kind(ToastKind::Error));
    }

    /// A whole-batch failure. The batch is one transaction, so this moved
    /// nothing — say so, because the selection is still on screen and "did some
    /// of that land?" is otherwise unanswerable from the toast. And name the
    /// card when the server could attribute the failure to one entry.
    fn failed(self, e: &ServerFnError<shared::ApiError>, names: &[(String, String)]) {
        self.toast.show(
            ToastOptions::message(format!(
                "Couldn't move: {} — nothing was moved",
                name_batch_failure(&crate::my::collection::message_of(e), names)
            ))
            .kind(ToastKind::Error),
        );
    }
}

/// The question the which-copies step is open on: where the batch was headed,
/// and the cards it could not resolve.
///
/// The destination is captured *with* the question rather than read live from
/// the picker: the second pass must land where the first one was aimed, and the
/// picker is a control the user can reopen while the dialog is up.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WhichCopies {
    dest: Destination,
    cards: Vec<AskedCard>,
}

/// What the picker's row list is showing, and the `data-state` it renders.
///
/// Three states, because two of them look alike and mean opposite things: a
/// card with no rows is "you hold none of these any more", and that sentence
/// must never stand in for "the read has not come back yet". Keeping them one
/// value (rather than two booleans) is what makes the impossible pair —
/// loading *and* ready — unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    Loading,
    Failed,
    Ready,
}

impl RowState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Failed => "failed",
            Self::Ready => "ready",
        }
    }
}

/// The row list and the state it is in, in **one** signal.
///
/// Two signals written back to back are two notifications, and a render between
/// them sees a pair that never existed — populated rows under "Finding your
/// copies…", or (worse, and the shape of the bug this state exists to prevent)
/// `ready` over an empty list. One value cannot be observed half-written,
/// whatever the scheduler does. The stepper counts stay outside it on purpose:
/// they change on every press, and folding them in here would re-render the
/// whole list each time.
#[derive(Clone, PartialEq)]
struct RowsView {
    state: RowState,
    sections: Vec<CardChoices>,
}

impl RowsView {
    fn waiting() -> Self {
        Self {
            state: RowState::Loading,
            sections: Vec::new(),
        }
    }
}

/// The step itself: one section per unresolved card, one row per stack its
/// copies actually sit in **at full grain**, a quantity stepper on each, and a
/// submit that goes back through the ordinary batch move.
///
/// **A step, not a second move flow.** It does not pick a destination (the
/// batch already did), does not touch the tray (its caller's `MoveReport`
/// does), and does not know what a `MoveItem` is — it collects counts and hands
/// them back as [`StackPick`]s.
///
/// **Its rows live in a signal, not inside the `Suspend`.** A stepper is state
/// standing beside each row, and the two have to be indexable together
/// ([`picks_of`]); resolving the read in an `Effect` — the same shape the
/// picker's `load_failed`/`load_loading` above use, and sound for the same
/// reason (this subtree never server-renders, since a selection starts empty on
/// every document load) — is what lets the counts be a plain `Vec<i32>` the
/// submit can read without walking the view.
#[component]
fn WhichCopiesDialog(
    step: RwSignal<Option<WhichCopies>>,
    open: RwSignal<bool>,
    /// Set before closing when the user actually submitted, so the caller's
    /// close-transition Effect knows not to raise the refusal toast.
    answered: RwSignal<bool>,
    on_confirm: Callback<Vec<StackPick>>,
) -> impl IntoView {
    // The rows, and the stepper value standing against each of them in row
    // order. Written together, always — `picks_of` is the one place that knows
    // they line up.
    let rows = RwSignal::new(RowsView::waiting());
    let counts = RwSignal::new(Vec::<i32>::new());

    // The stacks behind the question. A read of its own, deliberately: the
    // batch's own resolution had these rows in hand but shipping them back
    // would have taught `MoveOutcome` to carry display strings for a dialog
    // that may never open, and this read is fresher than that payload anyway —
    // it is taken at the moment the user is asked, not at the moment the batch
    // was refused. Staleness beyond that is the server's to catch: the second
    // pass re-resolves every pick against its own fresh holdings, so a stack
    // that emptied in between comes back as an honest `NoCopies` refusal, and
    // one that shrank as `NotEnough` naming its real size.
    //
    // **The payload carries the question it answers**, and that is not
    // bookkeeping: `Resource::get()` hands back the previously resolved value
    // while a refetch is in flight, and the first value this resource ever
    // resolves is an *empty* payload — the closure short-circuits while `step`
    // is `None`, which it is until the first refusal opens the dialog. Trusting
    // that on the next `get()` made every card in the selection render "No
    // copies left to move — reload the page" with the confirm disabled, for the
    // whole round trip, self-correcting when the real read landed. Carrying the
    // ids back is what makes "is this answer about this question" decidable at
    // all; `await`ing inside a `Transition` used to make it moot.
    let stacks = Resource::new(
        move || {
            step.get()
                .map(|s| s.cards.iter().map(|c| c.oracle_id).collect::<Vec<Id>>())
                .unwrap_or_default()
        },
        |ids| async move {
            if ids.is_empty() {
                return (ids, Ok(StacksPayload::default()));
            }
            let payload = crate::selection_stacks(ids.clone()).await;
            (ids, payload)
        },
    );

    // Every row opens at **one copy** (the maintainer's small-blast-radius
    // default, P6-150): the common shape is a single stack whose only open
    // question is "how many", and making that user press `+` before the button
    // will do anything is a step for nothing. What is chosen is never hidden —
    // each row shows its count and the submit button names the total — so a
    // card scattered over three stacks says "Move 3 copies" before it is
    // pressed, and a stack the user does not want is stepped to 0.
    Effect::new(move |_| {
        let asked = step.get();
        let loaded = stacks.get();
        // Counts first, then the rows-and-state pair: the steppers a row reads
        // are already in place the moment that row can be rendered.
        let waiting = || {
            counts.set(Vec::new());
            rows.set(RowsView::waiting());
        };
        let Some(WhichCopies { dest, cards }) = asked else {
            waiting();
            return;
        };
        let asked_ids: Vec<Id> = cards.iter().map(|c| c.oracle_id).collect();
        match loaded {
            None => waiting(),
            // A payload produced for a *different* question — the previous
            // open's, or the empty one this resource resolves before any dialog
            // exists. Still loading, whatever it says (see the resource above).
            Some((ids, _)) if ids != asked_ids => waiting(),
            // Not an empty list: that would read as "you hold none of these",
            // which is the opposite of what the batch just refused for.
            Some((_, Err(_))) => {
                counts.set(Vec::new());
                rows.set(RowsView {
                    state: RowState::Failed,
                    sections: Vec::new(),
                });
            }
            Some((_, Ok(payload))) => {
                let sections = card_choices(cards, &payload.cards, dest.id);
                counts.set(vec![1; sections.iter().map(|c| c.rows.len()).sum()]);
                rows.set(RowsView {
                    state: RowState::Ready,
                    sections,
                });
            }
        }
    });

    let total = Memo::new(move |_| counts.with(|v| v.iter().sum::<i32>()));

    let submit = move || {
        let chosen = picks_of(&rows.get_untracked().sections, &counts.get_untracked());
        if chosen.is_empty() {
            return;
        }
        answered.set(true);
        open.set(false);
        on_confirm.run(chosen);
    };

    view! {
        <Dialog id="tray-which-copies" open=open>
            // `text-foreground` and `text-left` are not decoration: this dialog
            // is mounted **inside the tray pill**, which is the app's one
            // inverted surface (`bg-foreground text-background`), and
            // `DialogContent` does not portal — so without them the panel
            // renders background-colored, centered text on its own background.
            <DialogContent aria_label="Which copies?" class="text-foreground text-left">
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>"Which copies?"</DialogTitle>
                        <DialogDescription>
                            {move || {
                                let to = step
                                    .get()
                                    .map(|s| s.dest.label())
                                    .unwrap_or_else(|| "the destination".to_string());
                                format!("Choose how many copies of each to move to {to}.")
                            }}
                        </DialogDescription>
                    </DialogHeader>
                    // Mounted only while open, for the reason the tree's move
                    // picker is: a closed dialog keeps its box in the DOM, so
                    // leaving the rows mounted would duplicate this seam behind
                    // a closed overlay on every My-cards page.
                    <Show when=move || open.get()>
                        // `data-state` is the seam a test can hold this on:
                        // "loaded, and this card has nothing" and "not loaded
                        // yet" render differently and mean opposite things, and
                        // only one of them may ever be shown for a read that is
                        // still in flight (see the resource above).
                        <div
                            class="max-h-[45vh] space-y-4 overflow-y-auto"
                            data-testid="which-copies"
                            data-state=move || rows.get().state.as_str()
                        >
                            <Show when=move || rows.get().state == RowState::Loading>
                                <p class="text-muted-foreground text-sm">"Finding your copies…"</p>
                            </Show>
                            <Show when=move || rows.get().state == RowState::Failed>
                                <p
                                    role="alert"
                                    class="text-destructive text-sm"
                                    data-testid="which-copies-error"
                                >
                                    "Couldn't load your copies. Close this and try the move again."
                                </p>
                            </Show>
                            <PickerRows sections=Signal::derive(move || rows.get().sections) counts />
                        </div>
                    </Show>
                    <DialogFooter>
                        <DialogClose attr:data-testid="which-copies-cancel">"Cancel"</DialogClose>
                        <Button
                            attr:data-testid="which-copies-confirm"
                            attr:disabled=move || total.get() < 1
                            on:click=move |_| submit()
                        >
                            {move || {
                                match total.get() {
                                    0 => "Move copies".to_string(),
                                    1 => "Move 1 copy".to_string(),
                                    n => format!("Move {n} copies"),
                                }
                            }}
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </DialogContent>
        </Dialog>
    }
}

/// The picker's rows: every asked card's section, in the order [`picks_of`]
/// walks them, each told where its rows start in the dialog's flat count
/// vector.
///
/// A component of its own so the **bench** can mount the one control this
/// feature adds without the dialog, the destination, or a server around it
/// (`app/src/bench/copy_picker.rs`). That is not a convenience: the tray's
/// hosting pages are authed and the Android dev webview cannot hold a session
/// (ui-work-loop Findings), so the bench is the only place the stepper's touch
/// behavior is reachable on a real phone engine.
#[component]
pub fn PickerRows(
    #[prop(into)] sections: Signal<Vec<CardChoices>>,
    counts: RwSignal<Vec<i32>>,
) -> impl IntoView {
    view! {
        {move || {
            let mut offset = 0;
            sections
                .get()
                .into_iter()
                .map(|choices| {
                    let first = offset;
                    offset += choices.rows.len();
                    view! { <CardSection choices counts first /> }
                })
                .collect_view()
        }}
    }
}

/// One card's block of the step: its name, and a row per stack of its copies.
///
/// `first` is where this section's rows start in the dialog's flat count
/// vector — the sections are rendered in the order [`picks_of`] walks them, so
/// one running offset is all the two need to agree.
#[component]
fn CardSection(choices: CardChoices, counts: RwSignal<Vec<i32>>, first: usize) -> impl IntoView {
    let CardChoices { card, rows } = choices;
    let name = card.name.clone();
    let empty = rows.is_empty();

    view! {
        <section data-testid="which-copies-card" data-card=name.clone()>
            <h4 class="text-sm font-medium">{name.clone()}</h4>
            <Show
                when=move || !empty
                fallback=|| {
                    view! {
                        <p class="text-muted-foreground text-sm">
                            "No copies left to move — reload the page."
                        </p>
                    }
                }
            >
                <ul class="mt-1 space-y-0.5">
                    {rows
                        .clone()
                        .into_iter()
                        .enumerate()
                        .map(|(i, row)| {
                            // `index=`, never `slot=`: leptos reserves `slot`
                            // on a component node for its `#[slot]` composition
                            // mechanism, and a prop by that name makes the
                            // **whole node vanish from the view with no error**
                            // — the dialog rendered rows with no stepper on
                            // them and only an "unused variable" warning said
                            // so (P6-150).
                            let index = first + i;
                            let label = stack_label(&row);
                            let max = row.quantity;
                            view! {
                                <li
                                    class="flex w-full items-center justify-between gap-2 rounded-md px-1 py-1 text-sm"
                                    data-testid="which-copies-row"
                                    data-stack=row.collection_id.to_string()
                                >
                                    <span class="truncate" data-testid="pick-label">
                                        {label.clone()}
                                    </span>
                                    <PickCount counts=counts index=index max=max label=label />
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </Show>
        </section>
    }
}

/// The `− n +` on one picker row: how many copies of *this* stack to move.
///
/// **Not [`CountStepper`](crate::components::ui::count_stepper::CountStepper),
/// deliberately.** That component is the collection view's in-place editor: it
/// commits on blur, raises its own undo toast, and hides its ± buttons below
/// `sm` (a phone taps the number and types). All three are wrong inside a form
/// whose commit is the dialog's own button — a blur-commit that fires as the
/// pointer travels to "Move copies", a toast offering to undo a write that has
/// not happened, and, on the surface this task exists to serve, no visible
/// control at all. This is the same shape without any of the persistence
/// contract: it writes one slot of the dialog's count vector and nothing else.
#[component]
fn PickCount(
    counts: RwSignal<Vec<i32>>,
    /// This row's place in the dialog's flat count vector.
    index: usize,
    /// The stack's size — the ceiling, so a stepper cannot ask for copies that
    /// are not there. (The server re-checks it: the stack can shrink between
    /// this read and the submit.)
    max: i32,
    #[prop(into)] label: String,
) -> impl IntoView {
    let value = Signal::derive(move || counts.with(|v| v.get(index).copied().unwrap_or(0)));
    let set = move |n: i32| {
        counts.update(|v| {
            if let Some(slot) = v.get_mut(index) {
                *slot = n.clamp(0, max);
            }
        })
    };
    // 44 px targets at phone width (the tray checkbox's own number), reverting
    // to the dense desktop size at `sm` where the pointer is a mouse.
    let button = "size-11 sm:size-8 text-base";

    view! {
        <span class="flex shrink-0 items-center gap-0.5" data-testid="pick-quantity">
            <Button
                variant=ButtonVariant::Ghost
                class=button
                attr:data-testid="pick-dec"
                attr:aria-label=format!("One fewer copy of {label}")
                attr:disabled=move || value.get() <= 0
                on:click=move |_| set(value.get_untracked() - 1)
            >
                "−"
            </Button>
            // No `aria-label` of its own: this is a live region, so its
            // *content* is what gets announced, and a label here would replace
            // the number with the row's whole sentence — read out again on
            // every press, without the count that changed. The two buttons
            // beside it carry the row-identifying label, which is where a
            // control among many needs one.
            <span class="w-6 text-center tabular-nums" data-testid="pick-value" aria-live="polite">
                {move || value.get()}
            </span>
            <Button
                variant=ButtonVariant::Ghost
                class=button
                attr:data-testid="pick-inc"
                attr:aria-label=format!("One more copy of {label}")
                // Operands reversed on purpose, so this comparison carries no
                // `>`: the view macro reads a bare `>` as the end of the
                // opening tag, and `value.get() >= max` silently made `= max …`
                // the button's *text* and left it permanently disabled — the
                // stepper could never be raised. (Parenthesising also works and
                // is what clippy's `unused_parens` then objects to.)
                attr:disabled=move || max <= value.get()
                on:click=move |_| set(value.get_untracked() + 1)
            >
                "+"
            </Button>
        </span>
    }
}

/// One stack's sentence: where it is, which printing (only when that
/// distinguishes anything — see [`CopyStack`]), which board (only inside a deck
/// where it is not the mainboard), **what makes these copies different from the
/// next row's** (finish, condition, language — each named only when it is not
/// the ordinary one), and how big the stack is.
///
/// The count is the stack's own size again — `3 copies`, not the `1 of 3
/// copies` a checkbox row needed — because the stepper beside it now states
/// what is being taken. The grain parts are the P6-150 addition and the reason
/// the row can be trusted: two rows reading `Trade Binder · 2 copies` and
/// `Trade Binder · 1 copy` are indistinguishable, and one of them is the foils.
pub fn stack_label(row: &CopyStack) -> String {
    let mut parts = vec![row.collection_name.clone()];
    if let Some(printing) = &row.printing {
        parts.push(printing.clone());
    }
    if row.board != Board::default() {
        parts.push(
            match row.board {
                Board::Side => "sideboard",
                Board::Maybe => "maybeboard",
                Board::Main => "mainboard",
            }
            .to_string(),
        );
    }
    if row.finish != Finish::default() {
        parts.push(
            match row.finish {
                Finish::Foil => "foil",
                Finish::Etched => "etched",
                Finish::Nonfoil => "nonfoil",
            }
            .to_string(),
        );
    }
    if row.condition != Condition::default() {
        parts.push(row.condition.to_pg().to_uppercase());
    }
    if row.language != shared::default_language() {
        parts.push(row.language.to_uppercase());
    }
    parts.push(if row.quantity == 1 {
        "1 copy".to_string()
    } else {
        format!("{} copies", row.quantity)
    });
    parts.join(" · ")
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

/// Which tray entries a finished batch move should drop (P6-122, staleness
/// policy on [`SelectionKey`](crate::components::ui::selection_tray::SelectionKey)):
/// what moved, **plus** what was refused as [`SkipReason::NoCopies`] — the one
/// refusal that names something *provably gone* rather than a real ambiguity.
///
/// Every other refusal (`Several`, `NotEnough`, `NoneRequested`,
/// `ManyCollections`, `ManyPrintings`, `ManyBoards`, `AlreadyThere`,
/// `NoLongerNeeded`) names a question the user can still act on — open the
/// picker, ask for fewer copies, open the right page — so those stay checked as
/// [`SelectionState::remove_tokens`](crate::components::ui::selection_tray::SelectionState::remove_tokens)'s
/// own doc says: they are work still to do. `NoCopies` is not a question; the
/// stack it named no longer exists (a stepper commit, a teardown, a collection
/// whose holdings relocated out from under it), and the fresh server read that
/// produced it is the one validation this policy always trusts — see the
/// staleness policy for why that trust is the load-bearing guarantee here.
/// Leaving it checked would mean the pill keeps counting something the server
/// just proved gone until the user notices and clears it by hand.
pub fn tokens_to_drop(outcome: &MoveOutcome) -> Vec<String> {
    let mut tokens = outcome.moved.clone();
    tokens.extend(
        outcome
            .skipped
            .iter()
            .filter(|s| s.reason == SkipReason::NoCopies)
            .map(|s| s.token.clone()),
    );
    tokens
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
        CardSource::Move {
            source: MoveSource {
                from: id(collection),
                printing_id: id(printing),
                finish: Finish::Nonfoil,
                condition: Condition::Nm,
                language: shared::default_language(),
                board: Board::Main,
            },
            quantity: 1,
        }
    }

    /// The grain out of a resolved move, for the tests that assert on it.
    fn moved_source(source: CardSource) -> MoveSource {
        match source {
            CardSource::Move { source, .. } => source,
            other => panic!("expected a move, got {other:?}"),
        }
    }

    /// The picker's answer for the default grain.
    fn want(quantity: i32) -> Pick {
        Pick {
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: shared::default_language(),
            quantity,
        }
    }

    #[test]
    fn one_source_of_one_copy_resolves_to_that_printing() {
        let entries = [own(1, 100, 1)];
        assert_eq!(resolve_card(&entries, id(9), None), plain(1, 100));
    }

    #[test]
    fn a_stack_of_several_copies_is_a_question_about_how_many() {
        // P6-150: the row says 3, and this code may not decide that "move it"
        // meant one of them. The picker asks, and the answer comes back as a
        // `Pick` — which is checked against this same read.
        let entries = [own(1, 100, 3)];
        assert_eq!(
            resolve_card(&entries, id(9), None),
            CardSource::Refuse(SkipReason::Several(3))
        );
        assert_eq!(
            resolve_card(&entries, id(9), Some(&want(2))),
            CardSource::Move {
                source: moved_source(plain(1, 100)),
                quantity: 2,
            }
        );
    }

    #[test]
    fn the_destination_is_not_a_candidate_source() {
        // Held in the deck we're moving to *and* in the binder: exactly one
        // place it can come from, so this resolves rather than refusing.
        let entries = [own(9, 100, 1), own(1, 101, 1)];
        assert_eq!(resolve_card(&entries, id(9), None), plain(1, 101));
    }

    #[test]
    fn two_collections_are_a_question_for_the_user() {
        let entries = [own(1, 100, 1), own(2, 100, 1)];
        assert_eq!(
            resolve_card(&entries, id(9), None),
            CardSource::Refuse(SkipReason::ManyCollections(2))
        );
    }

    #[test]
    fn two_printings_in_one_collection_are_ambiguous_too() {
        // Same collection, two printings — the row on `/my` names neither, and
        // its representative printing may be one of them or neither.
        let entries = [own(1, 100, 1), own(1, 101, 1)];
        assert_eq!(
            resolve_card(&entries, id(9), None),
            CardSource::Refuse(SkipReason::ManyPrintings(2))
        );
    }

    #[test]
    fn a_requested_quantity_is_checked_against_the_stack_that_is_really_there() {
        // The three per-entry quantity refusals, all of them against the
        // caller's own ungrouped holdings: over the stack's size, none at all,
        // and a grain nobody holds. None of them is a clamp, and none of them
        // is a batch failure — each is this entry's own polite refusal.
        let entries = [own(1, 100, 2)];
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), Some(&want(3))),
            CardSource::Refuse(SkipReason::NotEnough(2)),
            "asking for more than the stack holds names what is really there"
        );
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), Some(&want(0))),
            CardSource::Refuse(SkipReason::NoneRequested),
        );
        assert_eq!(
            resolve_held(
                &entries,
                id(1),
                id(100),
                Board::Main,
                id(9),
                Some(&want(-1))
            ),
            CardSource::Refuse(SkipReason::NoneRequested),
        );
        let gone = Pick {
            finish: Finish::Etched,
            ..want(1)
        };
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), Some(&gone)),
            CardSource::Refuse(SkipReason::NoCopies),
            "a grain the stack does not hold is refused, never written at another grain"
        );
        // The whole stack is a legal answer — it is the ceiling, not a limit
        // below it.
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), Some(&want(2))),
            CardSource::Move {
                source: moved_source(plain(1, 100)),
                quantity: 2,
            }
        );
    }

    fn item(key: SelectionKey, oracle: u128, pick: Option<Pick>) -> SelectionItem {
        SelectionItem {
            key,
            oracle_id: id(oracle),
            pick,
        }
    }

    fn held(collection: u128, printing: u128) -> SelectionKey {
        SelectionKey::Held {
            collection_id: id(collection),
            printing_id: id(printing),
            board: Board::Main,
        }
    }

    #[test]
    fn a_batch_cannot_draw_one_stack_twice() {
        // The whole-batch killer: two entries over the same stack, each legal
        // against the pile they both started from. Validated against an
        // un-spent snapshot both passed, and the second `holding_take` inside
        // `move_batch`'s single transaction then rolled back **every card in
        // the batch**. Spending the snapshot turns the second one into an
        // ordinary per-entry refusal naming what is actually left.
        let mut holdings = vec![own(1, 100, 3)];
        let first = item(held(1, 100), 1, Some(want(2)));
        assert_eq!(
            resolve_item(&mut holdings, &first, id(9)),
            CardSource::Move {
                source: moved_source(plain(1, 100)),
                quantity: 2,
            }
        );
        assert_eq!(holdings[0].quantity, 1, "the snapshot spent what it gave");
        let second = item(held(1, 100), 1, Some(want(2)));
        assert_eq!(
            resolve_item(&mut holdings, &second, id(9)),
            CardSource::Refuse(SkipReason::NotEnough(1)),
            "the second entry sees what the first left, not the pile it started from"
        );
        // …and the copy still there is movable by a third entry that asks for
        // it — the refusal narrowed the batch, it did not poison the stack.
        let third = item(held(1, 100), 1, Some(want(1)));
        assert!(matches!(
            resolve_item(&mut holdings, &third, id(9)),
            CardSource::Move { quantity: 1, .. }
        ));
        assert_eq!(holdings[0].quantity, 0);
        // Drained: a fourth is refused as gone rather than written.
        let fourth = item(held(1, 100), 1, Some(want(1)));
        assert_eq!(
            resolve_item(&mut holdings, &fourth, id(9)),
            CardSource::Refuse(SkipReason::NoCopies)
        );
    }

    #[test]
    fn spending_addresses_the_grain_it_took_from() {
        // A duplicate pair of *unpicked* tray entries over a two-copy stack —
        // the raw-POST shape, and the one where the second entry's own
        // resolution changes because of the first: 2 left is `Several`, 1 left
        // is a plain move.
        let mut holdings = vec![own(1, 100, 2)];
        let row = item(held(1, 100), 1, None);
        assert_eq!(
            resolve_item(&mut holdings, &row, id(9)),
            CardSource::Refuse(SkipReason::Several(2)),
            "nothing spent by a refusal"
        );
        assert_eq!(holdings[0].quantity, 2);
        // Now take one foil out of a mixed stack and check the *other* grain is
        // untouched: spending the wrong row would make the next entry's
        // refusal a lie about copies that are still there.
        let mut mixed = vec![foil(1, 100, 2), own(1, 100, 1)];
        let foils = item(
            held(1, 100),
            1,
            Some(Pick {
                finish: Finish::Foil,
                ..want(2)
            }),
        );
        assert!(matches!(
            resolve_item(&mut mixed, &foils, id(9)),
            CardSource::Move { quantity: 2, .. }
        ));
        assert_eq!(
            mixed.iter().map(|h| h.quantity).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn a_grain_is_taken_from_its_own_stack_not_the_row_total() {
        // 2 foil + 1 plain is one collection-page row of 3 and **two** picker
        // rows. Asking for 2 foils is legal; asking for 2 plain is not, even
        // though the row the user checked said 3.
        let entries = [foil(1, 100, 2), own(1, 100, 1)];
        let foils = Pick {
            finish: Finish::Foil,
            ..want(2)
        };
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), Some(&foils)),
            CardSource::Move {
                source: MoveSource {
                    finish: Finish::Foil,
                    ..moved_source(plain(1, 100))
                },
                quantity: 2,
            }
        );
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), Some(&want(2))),
            CardSource::Refuse(SkipReason::NotEnough(1))
        );
    }

    #[test]
    fn nothing_held_never_becomes_an_intake() {
        // The bug this whole enum exists to prevent: `from = None` in a
        // `MoveItem` means copies arriving from outside the system, so an
        // unresolvable selection must refuse, not fall back to it.
        assert_eq!(
            resolve_card(&[], id(9), None),
            CardSource::Refuse(SkipReason::NoCopies)
        );
    }

    #[test]
    fn everything_already_in_the_destination_says_so() {
        let entries = [own(9, 100, 2)];
        assert_eq!(
            resolve_card(&entries, id(9), None),
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
        // `/my` shows OWNED 1 and `collection_view` shows HERE 1; neither says
        // "foil". Resolving to the default grain would reach `holding_take`,
        // find nothing, and roll the whole transaction back.
        let entries = [foil(1, 100, 1)];
        let expected = CardSource::Move {
            source: MoveSource {
                finish: Finish::Foil,
                ..moved_source(plain(1, 100))
            },
            quantity: 1,
        };
        assert_eq!(resolve_card(&entries, id(9), None), expected);
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), None),
            expected
        );
    }

    #[test]
    fn several_grains_in_one_stack_are_asked_about_not_guessed() {
        // Two shapes of the same question, and the reason `SkipReason::Grain`
        // is gone. Before P6-150 the first of these silently moved the plain
        // copy (the default grain won) and the second refused outright, because
        // the picker's rows could not tell foil from etched. Both are now the
        // same sentence: more than one copy is here, say which and how many.
        let mixed = [foil(1, 100, 3), own(1, 100, 1)];
        assert_eq!(
            resolve_held(&mixed, id(1), id(100), Board::Main, id(9), None),
            CardSource::Refuse(SkipReason::Several(4))
        );
        let exotic = [
            foil(1, 100, 3),
            HoldingLine {
                finish: Finish::Etched,
                ..own(1, 100, 1)
            },
        ];
        assert_eq!(
            resolve_held(&exotic, id(1), id(100), Board::Main, id(9), None),
            CardSource::Refuse(SkipReason::Several(4))
        );
        // …and the picker has two rows to ask it with, which is what the old
        // refusal said no list could render.
        assert_eq!(stacks_of(&exotic).len(), 2);
    }

    #[test]
    fn a_sideboarded_my_row_moves_off_the_sideboard() {
        // `CardDetail::ownership` groups board away, so this looked movable
        // before the ungrouped read — and `holding_take` used to pin
        // `board = 'main'`, so the write took nothing.
        let entries = [on_board(Board::Side, 1, 100, 1)];
        assert_eq!(
            resolve_card(&entries, id(9), None),
            CardSource::Move {
                source: MoveSource {
                    board: Board::Side,
                    ..moved_source(plain(1, 100))
                },
                quantity: 1,
            }
        );
    }

    #[test]
    fn a_my_row_split_across_boards_is_a_question_for_the_user() {
        // Two boards is two rows on the deck page; the `/my` row is one, and
        // picking for the user would move copies they can see but did not point
        // at.
        let entries = [own(1, 100, 2), on_board(Board::Side, 1, 100, 2)];
        assert_eq!(
            resolve_card(&entries, id(9), None),
            CardSource::Refuse(SkipReason::ManyBoards(2))
        );
    }

    #[test]
    fn a_collection_row_moves_the_board_it_names() {
        // The same two stacks, addressed from the collection page: each row
        // names its own board, so each resolves to its own copies.
        let entries = [own(1, 100, 1), on_board(Board::Side, 1, 100, 1)];
        let main = moved_source(plain(1, 100));
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Side, id(9), None),
            CardSource::Move {
                source: MoveSource {
                    board: Board::Side,
                    ..main.clone()
                },
                quantity: 1,
            }
        );
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), None),
            CardSource::Move {
                source: main,
                quantity: 1,
            }
        );
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(1), None),
            CardSource::Refuse(SkipReason::AlreadyThere)
        );
    }

    #[test]
    fn a_stale_selection_is_refused_rather_than_aborting_the_batch() {
        // The copies were moved away in another tab after the row was checked.
        // Without the read this reached `holding_take` and took the batch with
        // it; the tray is long-lived by design, so this is not exotic.
        assert_eq!(
            resolve_held(&[], id(1), id(100), Board::Main, id(9), None),
            CardSource::Refuse(SkipReason::NoCopies)
        );
        // …and so is a row whose *printing* is gone while the card remains.
        let entries = [own(1, 999, 2)];
        assert_eq!(
            resolve_held(&entries, id(1), id(100), Board::Main, id(9), None),
            CardSource::Refuse(SkipReason::NoCopies)
        );
    }

    #[test]
    fn every_refusal_says_what_is_wrong() {
        // A reason with no phrase is a refusal the user cannot act on.
        for reason in [
            SkipReason::AlreadyThere,
            SkipReason::Several(3),
            SkipReason::NotEnough(2),
            SkipReason::NoneRequested,
            SkipReason::NoCopies,
            SkipReason::NoLongerNeeded,
            SkipReason::ManyCollections(2),
            SkipReason::ManyPrintings(2),
            SkipReason::ManyBoards(2),
        ] {
            assert!(!reason.phrase().is_empty(), "{reason:?} has no phrase");
        }
        // The counts are the point of the two quantity refusals: "pick fewer"
        // with no number is not an instruction anyone can follow.
        assert!(SkipReason::Several(3).phrase().contains('3'));
        assert!(SkipReason::NotEnough(2).phrase().contains('2'));
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
    fn the_toast_counts_copies_and_cards_separately() {
        // Both numbers said out loud, neither inferred — the sentence both
        // submit paths use now that a card can move any number of copies.
        assert_eq!(
            moved_message(1, 1, "🗂 Deck"),
            "Moved 1 copy of 1 card → 🗂 Deck"
        );
        assert_eq!(
            moved_message(5, 3, "🗂 Deck"),
            "Moved 5 copies of 3 cards → 🗂 Deck"
        );
        // The shape the old "(1 copy each)" wording could not say at all:
        // several copies of a single card.
        assert_eq!(
            moved_message(3, 1, "🗂 Deck"),
            "Moved 3 copies of 1 card → 🗂 Deck"
        );
    }

    #[test]
    fn refusals_are_named_then_counted() {
        let one = [("Bolt".to_string(), SkipReason::ManyCollections(2))];
        assert_eq!(
            skipped_message(&one),
            "1 card wasn't moved: Bolt is in 2 collections — pick the copies to move"
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
             pick the copies to move; and 2 more"
        );
    }

    // ---- the which-copies step ----

    fn selected(key: SelectionKey, oracle: u128, name: &str) -> SelectedCard {
        SelectedCard {
            key,
            oracle_id: id(oracle),
            name: name.to_string(),
            image_uri: None,
        }
    }

    fn my_row(oracle: u128, name: &str) -> SelectedCard {
        selected(
            SelectionKey::Card {
                oracle_id: id(oracle),
            },
            oracle,
            name,
        )
    }

    fn skip(token: &str, reason: SkipReason) -> Skipped {
        Skipped {
            token: token.to_string(),
            reason,
        }
    }

    fn stack(collection: u128, printing: u128, quantity: i32) -> CopyStack {
        CopyStack {
            collection_id: id(collection),
            collection_name: format!("Binder {collection}"),
            printing_id: id(printing),
            printing: None,
            board: Board::Main,
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: shared::default_language(),
            quantity,
        }
    }

    #[test]
    fn only_the_which_copies_questions_are_askable() {
        // The step's rows are full grains of one card, and a stepper stands
        // beside each: the three `Many*` refusals name which row, `Several`
        // names how many. Nothing else on this enum is a choice the dialog can
        // offer — `NotEnough`/`NoneRequested` are answers to a question it
        // already asked, and re-opening on them would loop.
        for reason in [
            SkipReason::ManyCollections(2),
            SkipReason::ManyPrintings(2),
            SkipReason::ManyBoards(2),
            SkipReason::Several(3),
        ] {
            assert!(reason.is_askable(), "{reason:?} is a which-copies question");
        }
        for reason in [
            SkipReason::AlreadyThere,
            SkipReason::NotEnough(2),
            SkipReason::NoneRequested,
            SkipReason::NoCopies,
            SkipReason::NoLongerNeeded,
        ] {
            assert!(!reason.is_askable(), "{reason:?} has no rows to offer");
        }
    }

    #[test]
    fn every_grain_is_its_own_row() {
        // P6-150: the rows are the **full** grain, so a foil and a plain copy
        // of the same printing in the same binder are two rows — the split the
        // collection page cannot render and `SkipReason::Grain` used to refuse
        // over. Order is the read's order, so the dialog is stable between two
        // opens.
        let holdings = [foil(1, 100, 3), own(1, 100, 1), own(2, 100, 2)];
        assert_eq!(
            stacks_of(&holdings)
                .iter()
                .map(|t| (t.collection_id, t.finish, t.quantity))
                .collect::<Vec<_>>(),
            vec![
                (id(1), Finish::Foil, 3),
                (id(1), Finish::Nonfoil, 1),
                (id(2), Finish::Nonfoil, 2),
            ]
        );
        // Condition and language split too — two rows a label must tell apart.
        let graded = [
            own(1, 100, 2),
            HoldingLine {
                condition: Condition::Lp,
                ..own(1, 100, 1)
            },
            HoldingLine {
                language: "ja".to_string(),
                ..own(1, 100, 4)
            },
        ];
        assert_eq!(stacks_of(&graded).len(), 3);
        // Boards stay apart — they are two rows on the deck page and two
        // choices here, which is the whole of `ManyBoards`.
        let boards = [own(1, 100, 2), on_board(Board::Side, 1, 100, 1)];
        assert_eq!(stacks_of(&boards).len(), 2);
        // Equal grains are one row even if the read hands them over twice: two
        // steppers over the same copies could ask for more than is there.
        assert_eq!(
            stacks_of(&[own(1, 100, 2), own(1, 100, 3)])
                .iter()
                .map(|t| t.quantity)
                .collect::<Vec<_>>(),
            vec![5]
        );
        // An emptied stack is not a row.
        assert!(stacks_of(&[own(1, 100, 0)]).is_empty());
    }

    #[test]
    fn refusals_split_into_what_can_be_asked_and_what_can_only_be_said() {
        let entries = [my_row(1, "Bolt"), my_row(2, "Brainstorm")];
        let skipped = [
            skip(
                "card:00000000-0000-0000-0000-000000000001",
                SkipReason::ManyCollections(2),
            ),
            skip(
                "card:00000000-0000-0000-0000-000000000002",
                SkipReason::NoCopies,
            ),
        ];
        let (ask, tell) = split_skips(&skipped, &entries);
        assert_eq!(ask.len(), 1);
        assert_eq!(ask[0].name, "Bolt");
        assert_eq!(ask[0].oracle_id, id(1));
        assert_eq!(ask[0].entries[0].reason, SkipReason::ManyCollections(2));
        assert_eq!(tell, vec![skipped[1].clone()]);
    }

    #[test]
    fn one_card_selected_twice_is_asked_about_once() {
        // The ruling's question 3. `/my` and the collection page are two views
        // of one shelf, so selecting the card on each makes two entries over
        // the *same copies*. Two sections would apportion one pile across two
        // identical lists — and, since both address the same
        // `(collection, printing, board)`, would submit two wire items the
        // server cannot tell apart in its own outcome.
        let key = held(1, 100);
        let entries = [
            my_row(1, "Bolt"),
            SelectedCard {
                key,
                ..my_row(1, "Bolt")
            },
        ];
        let skipped = [
            skip(&entries[0].key.token(), SkipReason::ManyCollections(2)),
            skip(&entries[1].key.token(), SkipReason::Several(3)),
        ];
        let (ask, tell) = split_skips(&skipped, &entries);
        assert!(tell.is_empty());
        assert_eq!(ask.len(), 1, "one card, one question");
        assert_eq!(
            ask[0].tokens(),
            vec![entries[0].key.token(), entries[1].key.token()],
            "…answering for both tray entries"
        );
        // Cancelling says **both** sentences: two entries are still checked
        // afterwards, and each was refused for its own reason. Reporting only
        // the first would leave the second entry sitting in the tray with
        // nothing ever said about it.
        assert_eq!(
            unanswered(&ask, &[]),
            vec![
                ("Bolt".to_string(), SkipReason::ManyCollections(2)),
                ("Bolt".to_string(), SkipReason::Several(3)),
            ]
        );

        // The merged section offers the **union** of what the two entries
        // addressed — the `/my` entry named no place, so every place — with
        // each stack appearing exactly once.
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![stack(1, 100, 3), stack(2, 100, 1)],
        }];
        let sections = card_choices(ask, &stacks, id(9));
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0]
                .rows
                .iter()
                .map(|r| (r.collection_id, r.quantity))
                .collect::<Vec<_>>(),
            vec![(id(1), 3), (id(2), 1)],
        );

        // One picked row retires **both** tray entries: the question each of
        // them asked has been answered, and a pill still counting the other
        // would be asking it again.
        let picks = picks_of(&sections, &[1, 0]);
        assert_eq!(picks.len(), 1);
        assert_eq!(picks[0].tokens, sections[0].card.tokens());
        let landed = [picks[0].item_token()];
        assert_eq!(answered_tokens(&picks, &landed), picks[0].tokens);
        // …and the toast counts one card, not two entries' worth.
        assert_eq!(moved_copies(&picks, &landed), 1);
        assert_eq!(moved_cards(&picks, &landed), 1);
        assert!(unanswered(
            &sections.iter().map(|s| s.card.clone()).collect::<Vec<_>>(),
            &picks
        )
        .is_empty());
    }

    #[test]
    fn a_collection_entry_alone_still_only_asks_about_its_own_stack() {
        // The merge must not widen a lone `Held` entry: the union is only as
        // wide as the keys that were actually selected.
        let entries = [SelectedCard {
            key: held(1, 100),
            ..my_row(1, "Bolt")
        }];
        let skipped = [skip(&entries[0].key.token(), SkipReason::Several(3))];
        let (ask, _) = split_skips(&skipped, &entries);
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![stack(1, 100, 3), stack(2, 100, 1)],
        }];
        let sections = card_choices(ask, &stacks, id(9));
        assert_eq!(
            sections[0]
                .rows
                .iter()
                .map(|r| r.collection_id)
                .collect::<Vec<_>>(),
            vec![id(1)]
        );
    }

    #[test]
    fn two_collection_rows_of_one_card_are_different_copies() {
        // The merge is a *display* merge, and this is the case that proves the
        // difference. A deck's mainboard and sideboard rows are one card and
        // two tray entries over copies that are not each other's. Answering one
        // and zeroing the other used to retire **both** — the zeroed entry left
        // the tray having moved nothing and said nothing, which is the silent
        // drop every toast in this file exists to prevent.
        let main = held(1, 100);
        let side = SelectionKey::Held {
            collection_id: id(1),
            printing_id: id(100),
            board: Board::Side,
        };
        let card = asked_over(1, "Bolt", &[main, side], SkipReason::Several(2));
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![
                stack(1, 100, 2),
                CopyStack {
                    board: Board::Side,
                    ..stack(1, 100, 2)
                },
            ],
        }];
        let sections = card_choices(vec![card], &stacks, id(9));
        assert_eq!(sections.len(), 1, "one card is still one section");
        assert_eq!(sections[0].rows.len(), 2, "…listing both entries' stacks");

        // Zero the mainboard row, take one off the sideboard.
        let picks = picks_of(&sections, &[0, 1]);
        assert_eq!(picks.len(), 1);
        assert_eq!(
            picks[0].tokens,
            vec![side.token()],
            "a pick answers only for the entries whose copies it is"
        );
        let landed = [picks[0].item_token()];
        assert_eq!(
            answered_tokens(&picks, &landed),
            vec![side.token()],
            "the mainboard entry is still checked"
        );
        assert_eq!(
            unanswered(
                &sections.iter().map(|s| s.card.clone()).collect::<Vec<_>>(),
                &picks
            ),
            vec![("Bolt".to_string(), SkipReason::Several(2))],
            "…and it is told why, rather than vanishing"
        );
    }

    #[test]
    fn a_card_entry_answers_for_the_stacks_it_contains() {
        // The intended collapse, and its edge: a `/my` entry contains every
        // stack of its oracle, so a row in the binder the `Held` entry named
        // answers for both — while a row *elsewhere* answers only for the `/my`
        // entry, and the collection entry stays checked with its own sentence.
        let card = AskedCard {
            entries: vec![
                AskedEntry {
                    key: SelectionKey::Card { oracle_id: id(1) },
                    reason: SkipReason::ManyCollections(2),
                },
                AskedEntry {
                    key: held(1, 100),
                    reason: SkipReason::Several(3),
                },
            ],
            oracle_id: id(1),
            name: "Bolt".to_string(),
        };
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![stack(1, 100, 3), stack(2, 100, 1)],
        }];
        let sections = card_choices(vec![card], &stacks, id(9));
        let cards: Vec<AskedCard> = sections.iter().map(|s| s.card.clone()).collect();

        // The shared stack: both entries retire, nothing is left unsaid.
        let both = picks_of(&sections, &[1, 0]);
        assert_eq!(both[0].tokens.len(), 2);
        assert_eq!(
            answered_tokens(&both, &[both[0].item_token()]).len(),
            2,
            "the /my entry and the collection row both had this stack answered"
        );
        assert!(unanswered(&cards, &both).is_empty());

        // The other binder: only the `/my` entry contains it.
        let elsewhere = picks_of(&sections, &[0, 1]);
        assert_eq!(
            elsewhere[0].tokens,
            vec![SelectionKey::Card { oracle_id: id(1) }.token()]
        );
        assert_eq!(
            unanswered(&cards, &elsewhere),
            vec![("Bolt".to_string(), SkipReason::Several(3))],
            "the collection row gave up nothing and says so"
        );
    }

    #[test]
    fn a_card_entry_beside_two_collection_rows_attributes_each_stack_once() {
        // The composite: `/my` plus both of a deck's board rows. Every row
        // retires the `/my` entry and exactly the board entry it came from, so
        // no entry is ever retired for copies it did not give up.
        let main = held(1, 100);
        let side = SelectionKey::Held {
            collection_id: id(1),
            printing_id: id(100),
            board: Board::Side,
        };
        let anywhere = SelectionKey::Card { oracle_id: id(1) };
        let card = asked_over(1, "Bolt", &[anywhere, main, side], SkipReason::Several(2));
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![
                stack(1, 100, 2),
                CopyStack {
                    board: Board::Side,
                    ..stack(1, 100, 2)
                },
            ],
        }];
        let sections = card_choices(vec![card], &stacks, id(9));
        let cards: Vec<AskedCard> = sections.iter().map(|s| s.card.clone()).collect();
        let picks = picks_of(&sections, &[1, 0]);
        assert_eq!(
            picks[0].tokens,
            vec![anywhere.token(), main.token()],
            "the mainboard row answers for /my and the mainboard entry"
        );
        assert_eq!(
            unanswered(&cards, &picks),
            vec![("Bolt".to_string(), SkipReason::Several(2))],
            "the sideboard entry is still a question, said once"
        );
        // Both rows taken: nothing is left unanswered, and all three retire.
        let all = picks_of(&sections, &[1, 1]);
        let landed: Vec<String> = all.iter().map(|p| p.item_token()).collect();
        assert_eq!(answered_tokens(&all, &landed).len(), 3);
        assert!(unanswered(&cards, &all).is_empty());
    }

    #[test]
    fn two_different_cards_never_merge() {
        let entries = [my_row(1, "Bolt"), my_row(2, "Brainstorm")];
        let skipped = [
            skip(&entries[0].key.token(), SkipReason::Several(2)),
            skip(&entries[1].key.token(), SkipReason::Several(2)),
        ];
        let (ask, _) = split_skips(&skipped, &entries);
        assert_eq!(ask.len(), 2);
        assert!(ask.iter().all(|c| c.entries.len() == 1));
    }

    #[test]
    fn an_unmatched_token_is_told_rather_than_asked() {
        // Impossible in practice (the batch is built from the tray), but a
        // refusal that reaches neither the dialog nor the toast is the silent
        // disappearance this whole path exists to prevent.
        let skipped = [skip("card:ghost", SkipReason::ManyCollections(2))];
        let (ask, tell) = split_skips(&skipped, &[]);
        assert!(ask.is_empty());
        assert_eq!(tell.len(), 1);
    }

    fn asked(oracle: u128, name: &str) -> AskedCard {
        AskedCard {
            entries: vec![AskedEntry {
                key: SelectionKey::Card {
                    oracle_id: id(oracle),
                },
                reason: SkipReason::ManyCollections(2),
            }],
            oracle_id: id(oracle),
            name: name.to_string(),
        }
    }

    /// A section standing for the tray entries `keys` addressed, each refused
    /// with `reason`.
    fn asked_over(
        oracle: u128,
        name: &str,
        keys: &[SelectionKey],
        reason: SkipReason,
    ) -> AskedCard {
        AskedCard {
            entries: keys.iter().map(|&key| AskedEntry { key, reason }).collect(),
            ..asked(oracle, name)
        }
    }

    #[test]
    fn the_destination_is_not_offered_as_a_source_to_pick() {
        // The same rule `resolve_card` applies, restated because the stacks read
        // knows nothing about where the batch was headed: picking "move these
        // from the deck you are moving into" can only no-op or be refused.
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![stack(1, 100, 2), stack(9, 100, 1), stack(2, 100, 0)],
        }];
        let rows = card_choices(vec![asked(1, "Bolt")], &stacks, id(9));
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .rows
                .iter()
                .map(|r| r.collection_id)
                .collect::<Vec<_>>(),
            vec![id(1)],
            "the destination and the emptied stack are both unpickable"
        );
    }

    #[test]
    fn a_collection_row_is_only_asked_about_its_own_stack() {
        // The entry was a `/my/collections/:id` row — "these copies, here" —
        // so the step may split it by grain but must not offer to move copies
        // out of a binder the user never pointed at. The `/my` entry over the
        // same holdings is the opposite: it names no place, so every place is a
        // candidate.
        let stacks = [CardStacks {
            oracle_id: id(1),
            stacks: vec![
                stack(1, 100, 2),
                CopyStack {
                    finish: Finish::Foil,
                    ..stack(1, 100, 1)
                },
                stack(2, 100, 5),
                CopyStack {
                    board: Board::Side,
                    ..stack(1, 100, 4)
                },
            ],
        }];
        let here = asked_over(1, "Bolt", &[held(1, 100)], SkipReason::Several(3));
        let rows = card_choices(vec![here], &stacks, id(9));
        assert_eq!(
            rows[0]
                .rows
                .iter()
                .map(|r| (r.collection_id, r.board, r.finish, r.quantity))
                .collect::<Vec<_>>(),
            vec![
                (id(1), Board::Main, Finish::Nonfoil, 2),
                (id(1), Board::Main, Finish::Foil, 1),
            ],
            "its own stack, split by grain — not the other binder, not the sideboard"
        );
        // The same holdings behind a `/my` row: all four stacks are offered.
        let anywhere = card_choices(vec![asked(1, "Bolt")], &stacks, id(9));
        assert_eq!(anywhere[0].rows.len(), 4);
    }

    #[test]
    fn a_card_whose_copies_vanished_keeps_its_section() {
        // Its stacks moved or emptied between the batch and this read. A card
        // silently absent from a list the user was asked to act on is the
        // "did that land?" state the toasts exist to refuse.
        let rows = card_choices(vec![asked(1, "Bolt")], &[], id(9));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].rows.is_empty());
    }

    #[test]
    fn a_picked_row_is_a_held_item_carrying_its_grain_and_count() {
        // Still no new write and no new mutation: a pick is the same `Held` key
        // a collection-page row submits, with the grain and the quantity the
        // key cannot carry riding alongside it. The server resolves it through
        // `resolve_held` against its own fresh read.
        let card = asked(1, "Bolt");
        let picks = vec![
            StackPick::of(&card, &stack(1, 100, 5), 2),
            StackPick::of(
                &card,
                &CopyStack {
                    board: Board::Side,
                    ..stack(2, 101, 1)
                },
                1,
            ),
        ];
        assert_eq!(
            picked_items(&picks),
            vec![
                SelectionItem {
                    key: SelectionKey::Held {
                        collection_id: id(1),
                        printing_id: id(100),
                        board: Board::Main,
                    },
                    oracle_id: id(1),
                    pick: Some(Pick {
                        finish: Finish::Nonfoil,
                        condition: Condition::Nm,
                        language: shared::default_language(),
                        quantity: 2,
                    }),
                },
                SelectionItem {
                    key: SelectionKey::Held {
                        collection_id: id(2),
                        printing_id: id(101),
                        board: Board::Side,
                    },
                    oracle_id: id(1),
                    pick: Some(Pick {
                        finish: Finish::Nonfoil,
                        condition: Condition::Nm,
                        language: shared::default_language(),
                        quantity: 1,
                    }),
                },
            ]
        );
        // …and the second pass's toast can still name the card, in the tokens
        // *that* pass answers in.
        assert_eq!(
            picked_names(&picks)
                .into_iter()
                .map(|(t, n)| format!("{t}|{n}"))
                .collect::<Vec<_>>(),
            vec![
                format!("{}|Bolt", picks[0].item_token()),
                format!("{}|Bolt", picks[1].item_token()),
            ]
        );
    }

    #[test]
    fn two_grains_of_one_stack_are_two_distinguishable_items() {
        // The bug this token suffix exists to prevent: both rows are
        // `held:<collection>:<printing>:main`, so an outcome naming them by the
        // bare tray token could not say which of the two moved — and the
        // toast, the tray reconciliation and the copy count all read that
        // list.
        let card = asked(1, "Bolt");
        let plain = StackPick::of(&card, &stack(1, 100, 2), 1);
        let foil = StackPick::of(
            &card,
            &CopyStack {
                finish: Finish::Foil,
                ..stack(1, 100, 3)
            },
            3,
        );
        assert_eq!(plain.key(), foil.key(), "same row, so the same tray key");
        assert_ne!(plain.item_token(), foil.item_token());
        assert!(plain.item_token().starts_with(&plain.key().token()));
        // An item nobody picked keeps exactly the token the tray uses — the
        // one the DOM, the earlier tasks and `tokens_to_drop` all match on.
        let untouched = SelectionItem {
            key: plain.key(),
            oracle_id: id(1),
            pick: None,
        };
        assert_eq!(untouched.token(), plain.key().token());
        // Only what the server said moved is counted, and it is counted in
        // copies.
        let picks = vec![plain.clone(), foil.clone()];
        assert_eq!(moved_copies(&picks, &[foil.item_token()]), 3);
        assert_eq!(moved_cards(&picks, &[foil.item_token()]), 1);
        assert_eq!(
            moved_copies(&picks, &[plain.item_token(), foil.item_token()]),
            4
        );
    }

    #[test]
    fn a_moved_pick_retires_the_tray_row_it_answered_for() {
        // The pass moves grain-suffixed `held:` tokens; the tray holds the
        // `card:` entry those answer. Without the translation the pill keeps
        // counting a card whose copies just moved.
        let card = asked(1, "Bolt");
        let here = StackPick::of(&card, &stack(1, 100, 2), 1);
        let there = StackPick::of(&card, &stack(2, 100, 1), 1);
        let picks = vec![here.clone(), there.clone()];
        // One of two moved: the question was answered, so the entry goes —
        // whatever did not move has its own refusal toast.
        assert_eq!(answered_tokens(&picks, &[here.item_token()]), card.tokens());
        // Both moved: named once, not twice.
        assert_eq!(
            answered_tokens(&picks, &[here.item_token(), there.item_token()]),
            card.tokens()
        );
        // Nothing moved: nothing leaves the tray.
        assert!(answered_tokens(&picks, &[]).is_empty());
    }

    #[test]
    fn a_row_says_where_it_is_what_it_is_and_how_big_it_is() {
        // The count is the stack's own size now — the stepper beside it states
        // what is being taken — and the grain parts are what keep two rows of
        // the same binder from being indistinguishable.
        assert_eq!(stack_label(&stack(1, 100, 3)), "Binder 1 · 3 copies");
        assert_eq!(stack_label(&stack(1, 100, 1)), "Binder 1 · 1 copy");
        // A printing chip only when the read decided it distinguishes
        // something, a board only when it is not the mainboard, and each grain
        // part only when it is not the ordinary one.
        assert_eq!(
            stack_label(&CopyStack {
                printing: Some("MH3 #123".to_string()),
                board: Board::Side,
                finish: Finish::Foil,
                condition: Condition::Lp,
                language: "ja".to_string(),
                ..stack(1, 100, 2)
            }),
            "Binder 1 · MH3 #123 · sideboard · foil · LP · JA · 2 copies"
        );
        // The two rows the old label could not tell apart.
        assert_ne!(
            stack_label(&stack(1, 100, 2)),
            stack_label(&CopyStack {
                finish: Finish::Foil,
                ..stack(1, 100, 2)
            })
        );
    }

    #[test]
    fn the_dialogs_counts_become_picks_in_row_order() {
        // The one place that knows the flat `Vec<i32>` of steppers lines up
        // with the sections' rows. A slip here moves the wrong stack's copies,
        // which is the failure the whole picker exists to prevent.
        let sections = vec![
            CardChoices {
                card: asked(1, "Bolt"),
                rows: vec![
                    stack(1, 100, 2),
                    CopyStack {
                        finish: Finish::Foil,
                        ..stack(1, 100, 3)
                    },
                ],
            },
            CardChoices {
                card: asked(2, "Brainstorm"),
                rows: vec![stack(3, 200, 4)],
            },
        ];
        let picks = picks_of(&sections, &[1, 0, 4]);
        assert_eq!(
            picks
                .iter()
                .map(|p| (
                    p.name.as_str(),
                    p.row.finish,
                    p.row.collection_id,
                    p.quantity
                ))
                .collect::<Vec<_>>(),
            vec![
                ("Bolt", Finish::Nonfoil, id(1), 1),
                ("Brainstorm", Finish::Nonfoil, id(3), 4),
            ],
            "a row stepped to zero is not a pick, and the rest keep their own rows"
        );
        // Nothing chosen at all is no submit — the button is disabled, and this
        // is the same answer if it were not.
        assert!(picks_of(&sections, &[0, 0, 0]).is_empty());
        // A short count vector cannot mis-address a row: missing slots read as
        // zero rather than shifting.
        assert!(picks_of(&sections, &[]).is_empty());
    }

    #[test]
    fn a_partial_answer_still_refuses_the_rest() {
        // `answered` suppresses the cancel toast, so without this the untouched
        // cards would get no sentence at all while staying checked in the tray.
        let bolt = asked(1, "Bolt");
        let brainstorm = asked(2, "Brainstorm");
        let cards = vec![bolt.clone(), brainstorm];
        let picks = vec![StackPick::of(&bolt, &stack(1, 100, 2), 1)];
        assert_eq!(
            unanswered(&cards, &picks),
            vec![("Brainstorm".to_string(), SkipReason::ManyCollections(2))]
        );
        // Cancelling is the same sentence with no picks at all…
        assert_eq!(unanswered(&cards, &[]).len(), 2);
        // …and a complete answer refuses nothing.
        assert!(unanswered(&cards[..1], &picks).is_empty());
    }

    #[test]
    fn a_printing_chip_survives_a_set_less_printing() {
        assert_eq!(printing_label(Some("mh3"), "123"), "MH3 #123");
        assert_eq!(printing_label(None, "123"), "#123");
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

    #[test]
    fn tokens_to_drop_takes_the_moved_and_the_provably_gone() {
        // Three entries: one moved, one refused because its stack is simply
        // gone (a stepper commit, a teardown — see `SelectionKey`'s staleness
        // policy), one refused because it is a real, still-actionable
        // ambiguity. Only the first two should stop being counted.
        let outcome = MoveOutcome {
            move_ids: vec![id(1)],
            moved: vec!["held:a:b:main".to_string()],
            skipped: vec![
                Skipped {
                    token: "held:c:d:main".to_string(),
                    reason: SkipReason::NoCopies,
                },
                Skipped {
                    token: "card:e".to_string(),
                    reason: SkipReason::ManyCollections(2),
                },
            ],
        };
        let mut dropped = tokens_to_drop(&outcome);
        dropped.sort();
        assert_eq!(
            dropped,
            vec!["held:a:b:main".to_string(), "held:c:d:main".to_string()]
        );
    }

    #[test]
    fn tokens_to_drop_leaves_every_other_refusal_alone() {
        // `AlreadyThere`, `Several`, `NotEnough`, `ManyPrintings`,
        // `ManyBoards` all name a real question the user can still answer —
        // none of them mean the stack is gone, so none of them should be swept
        // off the tray with `NoCopies`.
        let outcome = MoveOutcome {
            move_ids: vec![],
            moved: vec![],
            skipped: vec![
                Skipped {
                    token: "a".to_string(),
                    reason: SkipReason::AlreadyThere,
                },
                Skipped {
                    token: "b".to_string(),
                    reason: SkipReason::Several(2),
                },
                Skipped {
                    token: "e".to_string(),
                    reason: SkipReason::NotEnough(1),
                },
                Skipped {
                    token: "c".to_string(),
                    reason: SkipReason::ManyPrintings(2),
                },
                Skipped {
                    token: "d".to_string(),
                    reason: SkipReason::ManyBoards(2),
                },
            ],
        };
        assert_eq!(tokens_to_drop(&outcome), Vec::<String>::new());
    }
}
