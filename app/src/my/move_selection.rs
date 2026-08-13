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
//! **An ambiguous `/my` row is a question, and the move now asks it.** Three of
//! the refusals below — [`SkipReason::ManyCollections`], [`ManyPrintings`],
//! [`ManyBoards`] — say "your copies are spread out, and this code may not
//! choose for you". They used to end the batch's story for that card and send
//! the user to another page. They now open the **which-copies step**
//! ([`WhichCopiesDialog`]): the concrete stacks behind the card, one row per
//! (collection, printing, board) with its count, checkboxes, and a second
//! submit through the *same* write path — each picked row becomes a
//! [`SelectionKey::Held`] entry, which is by construction unambiguous, so
//! nothing about the server, the wire, or `move_batch` changed to allow it. The
//! refusal toast survives for whoever cancels out of the step, and for every
//! refusal the step cannot answer (see [`SkipReason::is_ambiguous`]).
//!
//! [`ManyPrintings`]: SkipReason::ManyPrintings
//! [`ManyBoards`]: SkipReason::ManyBoards
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
use crate::components::ui::button::Button;
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::dialog::{
    Dialog, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader,
    DialogTitle,
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
///
/// Three of them are questions the *move* can now ask on the spot
/// ([`Self::is_ambiguous`]); the rest end the batch's story for that card and
/// are reported as toasts, as all seven used to be.
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
    ///
    /// The three ambiguous ones still name what is undecided (the count is the
    /// whole point — "in 2 collections" is why the row could not move) but no
    /// longer send the reader to another page: the move asks the question
    /// itself now, and this sentence is what they see after **declining** to
    /// answer it. "Pick the copies to move" is therefore an offer they can take
    /// by moving again, not an errand.
    pub fn phrase(self) -> String {
        match self {
            Self::AlreadyThere => "is already there".to_string(),
            Self::Grain(n) => {
                format!("is held in {n} finishes or conditions at once — a move has to name one")
            }
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

    /// Is this refusal a *which copies did you mean* question the which-copies
    /// step can put to the user?
    ///
    /// The three `Many*` arms are, and they are the only ones. The others are
    /// not narrower questions, they are different sentences entirely:
    /// `AlreadyThere` has nothing to choose between (the copies are at the
    /// destination), `NoCopies`/`NoLongerNeeded` name something the fresh
    /// server read just proved gone, and `Grain` is a distinction **no read
    /// model this app has can render as a row** — the stacks the step lists are
    /// `(collection, printing, board)`, exactly the grain
    /// [`SelectionKey::Held`] addresses, and a stack whose several
    /// finish/condition/language grains hold no default one is one row on that
    /// list, not several. Offering it as a choice would mean showing rows the
    /// user cannot tell apart.
    pub fn is_ambiguous(self) -> bool {
        matches!(
            self,
            Self::ManyCollections(_) | Self::ManyPrintings(_) | Self::ManyBoards(_)
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

// ------------------------------------------------- the which-copies step ---
//
// Everything between here and `name_batch_failure` is the disambiguation
// pipeline, in the order the data flows: the stacks a card's copies actually
// sit in (read), the cards a batch could not resolve (split out of the
// outcome), the two joined into rows (the dialog's model), the rows the user
// ticked (picks), and those picks turned back into wire items and tray tokens.
// All of it is pure, and deliberately so — the dialog does no arithmetic.

/// One physical stack of a card's copies: a `(collection, printing, board)`
/// triple and how many copies sit in it.
///
/// **This is exactly the grain [`SelectionKey::Held`] addresses**, which is
/// what makes the whole step cost nothing on the server: a picked row is a
/// `Held` key, and a `Held` key resolves through [`resolve_held`] — the same
/// path a collection-page row already takes. The finish/condition/language
/// *inside* a stack are summed here, again exactly as the collection page's own
/// row sums them; picking a stack that holds several grains and no default one
/// therefore still lands on [`SkipReason::Grain`], which is the honest answer
/// (see [`SkipReason::is_ambiguous`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackTally {
    pub collection_id: Id,
    pub printing_id: Id,
    pub board: Board,
    pub quantity: i32,
}

/// Roll ungrouped holdings up into the stacks a user can point at.
///
/// Input is `holdings_of_oracle`'s own shape (the read that already backs
/// resolution), so this adds no query. Order is the input's order — the hosted
/// read sorts by `(collection, printing, board, finish)` — so the rows the
/// dialog lists are stable between two opens rather than hash-shuffled.
pub fn stacks_of(holdings: &[HoldingLine]) -> Vec<StackTally> {
    let mut out: Vec<StackTally> = Vec::new();
    for h in holdings.iter().filter(|h| movable(h)) {
        match out.iter_mut().find(|t| {
            t.collection_id == h.collection_id
                && t.printing_id == h.printing_id
                && t.board == h.board
        }) {
            Some(t) => t.quantity += h.quantity,
            None => out.push(StackTally {
                collection_id: h.collection_id,
                printing_id: h.printing_id,
                board: h.board,
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
    pub quantity: i32,
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

/// One tray entry the batch could not resolve, carrying the name it was shown
/// under so the step and the fallback toast say the same word for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousCard {
    /// The **original** tray token — a `card:<oracle>` key. The picks made
    /// against this card submit `held:` tokens, so this is the only thing that
    /// can put the answer back on the row the user actually checked
    /// ([`answered_tokens`]).
    pub token: String,
    pub oracle_id: Id,
    pub name: String,
    pub reason: SkipReason,
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
pub fn split_skips(
    skipped: &[Skipped],
    entries: &[SelectedCard],
) -> (Vec<AmbiguousCard>, Vec<Skipped>) {
    let mut ask = Vec::new();
    let mut tell = Vec::new();
    for s in skipped {
        let entry = entries.iter().find(|c| c.key.token() == s.token);
        match (s.reason.is_ambiguous(), entry) {
            (true, Some(c)) => ask.push(AmbiguousCard {
                token: s.token.clone(),
                oracle_id: c.oracle_id,
                name: c.name.clone(),
                reason: s.reason,
            }),
            _ => tell.push(s.clone()),
        }
    }
    (ask, tell)
}

/// One card's section of the dialog: what was asked, and the rows to answer it
/// with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardChoices {
    pub card: AmbiguousCard,
    pub rows: Vec<CopyStack>,
}

/// Join the cards the batch refused to the stacks the follow-up read found.
///
/// Two filters, both of them the same rules [`resolve_card`] applies, restated
/// here because the read behind `stacks` is a plain "everything you hold of
/// this card" and knows nothing about the move:
///
/// * **the destination is not a source** — offering "move these from the deck
///   you are moving into" is a row whose only outcomes are a no-op or an
///   `AlreadyThere` refusal;
/// * **empty stacks are not rows** — `stacks_of` already drops them, and this
///   restates it because the read and the write are separate requests.
///
/// A card whose rows all fall away keeps its section with zero rows rather than
/// disappearing from the dialog: its copies moved or vanished between the batch
/// and this read, and a card silently missing from a list the user was asked to
/// act on is exactly the "did that land?" state the toasts exist to refuse.
pub fn card_choices(cards: Vec<AmbiguousCard>, stacks: &[CardStacks], to: Id) -> Vec<CardChoices> {
    cards
        .into_iter()
        .map(|card| {
            let rows = stacks
                .iter()
                .find(|s| s.oracle_id == card.oracle_id)
                .map(|s| {
                    s.stacks
                        .iter()
                        .filter(|r| r.collection_id != to && r.quantity > 0)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            CardChoices { card, rows }
        })
        .collect()
}

/// One row the user ticked: the stack to take a copy from, plus the tray entry
/// and name it answers for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPick {
    /// The original `card:` token this answers — see [`AmbiguousCard::token`].
    pub token: String,
    pub name: String,
    pub oracle_id: Id,
    pub collection_id: Id,
    pub printing_id: Id,
    pub board: Board,
}

impl StackPick {
    /// The pick as a tray-style key: precise by construction, which is the
    /// entire trick this step turns.
    pub fn key(&self) -> SelectionKey {
        SelectionKey::Held {
            collection_id: self.collection_id,
            printing_id: self.printing_id,
            board: self.board,
        }
    }

    fn of(card: &AmbiguousCard, row: &CopyStack) -> Self {
        Self {
            token: card.token.clone(),
            name: card.name.clone(),
            oracle_id: card.oracle_id,
            collection_id: row.collection_id,
            printing_id: row.printing_id,
            board: row.board,
        }
    }
}

/// The picks as a batch the existing write already takes.
///
/// **No new mutation exists for this step, and none is needed.** A `Held` key
/// carries `(collection, printing, board)`, which is what every refusal here
/// said was missing; the server resolves it through the same `resolve_held` a
/// collection-page row uses, against its own fresh holdings read, and one copy
/// per item is the same quantity rule the first pass followed. Two ticks on one
/// card are two items, so a user who wants a copy from each of two collections
/// gets exactly that.
pub fn picked_items(picks: &[StackPick]) -> Vec<SelectionItem> {
    picks
        .iter()
        .map(|p| SelectionItem {
            key: p.key(),
            oracle_id: p.oracle_id,
        })
        .collect()
}

/// Names for the second pass's refusal toast, keyed by the tokens *that* pass
/// answers in (`held:…`), not the tray's.
pub fn picked_names(picks: &[StackPick]) -> Vec<(String, String)> {
    picks
        .iter()
        .map(|p| (p.key().token(), p.name.clone()))
        .collect()
}

/// The **tray** tokens a finished disambiguation move answered.
///
/// The second pass moves `held:` tokens; the tray still holds the `card:`
/// entries those answer. Without this translation the pill would keep counting
/// a card whose copies just moved — [`tokens_to_drop`] would look for
/// `held:…` tokens among `card:…` entries and match nothing.
///
/// **One moved stack retires the entry**, even where the user ticked two rows
/// and only one moved: the entry's question was *which copies*, the user
/// answered it, and what did not move is named in its own refusal toast. An
/// entry left checked after that would be inviting the same question again.
pub fn answered_tokens(picks: &[StackPick], moved: &[String]) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for p in picks {
        if moved.contains(&p.key().token()) && !tokens.contains(&p.token) {
            tokens.push(p.token.clone());
        }
    }
    tokens
}

/// The cards the step asked about that the user left untouched.
///
/// Two callers, one sentence: **cancelling** is this with no picks at all, and
/// **submitting a partial answer** is this with the picks that were made. The
/// second one is the case that would otherwise go silent — `answered` suppresses
/// the close toast, so a user who ticked one of three cards and hit the button
/// would get a confirmation for the one and nothing whatsoever about the other
/// two, which is the "did that land?" state every toast in this file exists to
/// refuse.
pub fn unanswered(cards: &[AmbiguousCard], picks: &[StackPick]) -> Vec<(String, SkipReason)> {
    cards
        .iter()
        .filter(|c| !picks.iter().any(|p| p.token == c.token))
        .map(|c| (c.name.clone(), c.reason))
        .collect()
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

/// The confirmation toast's message for a **batch** move. The count is stated in
/// **copies**, because the pill counts entries and one copy each is the thing a
/// reader would otherwise have to infer.
///
/// Its premise — one entry is one card is one copy — is the tray's, and it is
/// exactly what the which-copies step breaks: there, several ticks can belong to
/// the same card. That pass phrases itself with [`picked_message`] instead of
/// stretching this one over a shape it was never true for.
pub fn moved_message(moved: usize, destination: &str) -> String {
    let copies = if moved == 1 { "1 copy" } else { "1 copy each" };
    let cards = if moved == 1 {
        "1 card".to_string()
    } else {
        format!("{moved} cards")
    };
    format!("Moved {cards} ({copies}) → {destination}")
}

/// The confirmation toast for the **which-copies** pass, counted from the picks
/// that actually moved.
///
/// [`moved_message`] cannot speak here. Its unit is the tray entry, and one
/// entry is one card; a pick is a *stack*, and ticking all three stacks of one
/// Bolt moves three copies of **one** card. Phrased through the batch message
/// that reads "Moved 3 cards (1 copy each)" — a plain false statement about the
/// user's collection, on the step's headline flow.
///
/// So both numbers are said out loud, and neither is inferred: copies is how
/// many ticks landed (one copy per pick, the same server-fixed quantity the
/// batch uses), cards is how many distinct tray entries those ticks belonged to.
/// The one-card case names the card count anyway ("of 1 card") rather than
/// dropping to bare copies, so the two shapes read as the same sentence.
pub fn picked_message(picks: &[StackPick], moved: &[String], destination: &str) -> String {
    let landed: Vec<&StackPick> = picks
        .iter()
        .filter(|p| moved.contains(&p.key().token()))
        .collect();
    let copies = if landed.len() == 1 {
        "1 copy".to_string()
    } else {
        format!("{} copies", landed.len())
    };
    let mut tokens: Vec<&str> = landed.iter().map(|p| p.token.as_str()).collect();
    tokens.sort();
    tokens.dedup();
    match tokens.len() {
        1 => format!("Moved {copies} of 1 card → {destination}"),
        n => format!("Moved {copies} across {n} cards → {destination}"),
    }
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
                    // holds `card:` entries and this pass answered in `held:`
                    // tokens, and one card can have contributed several of them
                    // — which is exactly what `moved_message` cannot say.
                    report.moved_as(
                        &outcome,
                        &answered_tokens(&picks, &outcome.moved),
                        picked_message(&picks, &outcome.moved, &dest.label()),
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
<<<<<<< HEAD
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
                <DestinationList failed=load_failed>
||||||| parent of 790a92e (fix(ui): the tray picker gets the same one-state-at-a-time treatment)
                <DestinationList empty="No collection to move to." failed=load_failed>
=======
                <DestinationList
                    empty="No collection to move to."
                    failed=load_failed
                    loading=load_loading
                >
>>>>>>> 790a92e (fix(ui): the tray picker gets the same one-state-at-a-time treatment)
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
        self.moved_as(
            outcome,
            drop_tokens,
            moved_message(outcome.moved.len(), &dest.label()),
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
    cards: Vec<AmbiguousCard>,
}

/// The step itself: one section per unresolved card, one row per stack its
/// copies actually sit in, and a submit that goes back through the ordinary
/// batch move.
///
/// **A step, not a second move flow.** It does not pick a destination (the
/// batch already did), does not touch the tray (its caller's `MoveReport`
/// does), and does not know what a `MoveItem` is — it collects ticks and hands
/// them back as [`StackPick`]s.
#[component]
fn WhichCopiesDialog(
    step: RwSignal<Option<WhichCopies>>,
    open: RwSignal<bool>,
    /// Set before closing when the user actually submitted, so the caller's
    /// close-transition Effect knows not to raise the refusal toast.
    answered: RwSignal<bool>,
    on_confirm: Callback<Vec<StackPick>>,
) -> impl IntoView {
    let picks = RwSignal::new(Vec::<StackPick>::new());

    // The stacks behind the question. A read of its own, deliberately: the
    // batch's own resolution had these rows in hand but shipping them back
    // would have taught `MoveOutcome` to carry display strings for a dialog
    // that may never open, and this read is fresher than that payload anyway —
    // it is taken at the moment the user is asked, not at the moment the batch
    // was refused. Staleness beyond that is the server's to catch: the second
    // pass re-resolves every pick against its own fresh holdings, so a stack
    // that emptied in between comes back as an honest `NoCopies` refusal.
    let stacks = Resource::new(
        move || {
            step.get()
                .map(|s| s.cards.iter().map(|c| c.oracle_id).collect::<Vec<Id>>())
                .unwrap_or_default()
        },
        |ids| async move {
            if ids.is_empty() {
                return Ok(StacksPayload::default());
            }
            crate::selection_stacks(ids).await
        },
    );

    // A new question starts with nothing ticked. Answering *for* the user is
    // the one thing this dialog exists to stop doing.
    Effect::new(move |_| {
        step.track();
        picks.set(Vec::new());
    });

    let toggle = Callback::new(move |pick: StackPick| {
        picks.update(|v| match v.iter().position(|p| *p == pick) {
            Some(i) => {
                v.remove(i);
            }
            None => v.push(pick),
        })
    });

    let submit = move || {
        let chosen = picks.get_untracked();
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
                                format!(
                                    "These are in more than one place. Tick the copies to move to {to} — one copy from each.",
                                )
                            }}
                        </DialogDescription>
                    </DialogHeader>
                    // Mounted only while open, for the reason the tree's move
                    // picker is: a closed dialog keeps its box in the DOM, so
                    // leaving the rows mounted would duplicate this seam behind
                    // a closed overlay on every My-cards page.
                    <Show when=move || open.get()>
                        <div
                            class="max-h-[45vh] space-y-4 overflow-y-auto"
                            data-testid="which-copies"
                        >
                            <Transition fallback=|| {
                                view! {
                                    <p class="text-muted-foreground text-sm">"Finding your copies…"</p>
                                }
                            }>
                                {move || {
                                    let asked = step.get();
                                    Suspend::new(async move {
                                        let Some(WhichCopies { dest, cards }) = asked else {
                                            return ().into_any();
                                        };
                                        // Not `unwrap_or_default()`: an empty list
                                        // here would read as "you hold none of
                                        // these", which is the opposite of what
                                        // the batch just refused for.
                                        let Ok(payload) = stacks.await else {
                                            return view! {
                                                <p
                                                    role="alert"
                                                    class="text-destructive text-sm"
                                                    data-testid="which-copies-error"
                                                >
                                                    "Couldn't load your copies. Close this and try the move again."
                                                </p>
                                            }
                                                .into_any();
                                        };
                                        card_choices(cards, &payload.cards, dest.id)
                                            .into_iter()
                                            .map(|choices| {
                                                view! { <CardSection choices picks toggle /> }
                                            })
                                            .collect_view()
                                            .into_any()
                                    })
                                }}
                            </Transition>
                        </div>
                    </Show>
                    <DialogFooter>
                        <DialogClose attr:data-testid="which-copies-cancel">"Cancel"</DialogClose>
                        <Button
                            attr:data-testid="which-copies-confirm"
                            attr:disabled=move || picks.with(Vec::is_empty)
                            on:click=move |_| submit()
                        >
                            {move || {
                                match picks.with(Vec::len) {
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

/// One card's block of the step: its name, why it was asked about, and a row
/// per stack.
#[component]
fn CardSection(
    choices: CardChoices,
    picks: RwSignal<Vec<StackPick>>,
    toggle: Callback<StackPick>,
) -> impl IntoView {
    let CardChoices { card, rows } = choices;
    let name = card.name.clone();
    let empty = rows.is_empty();
    let card = StoredValue::new(card);

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
                        .map(|row| {
                            let pick = StackPick::of(&card.get_value(), &row);
                            let checked = {
                                let pick = pick.clone();
                                Signal::derive(move || picks.with(|v| v.contains(&pick)))
                            };
                            let label = stack_label(&row);
                            view! {
                                <li>
                                    // The whole row is the hit area, for the reason
                                    // the tray's own checkbox documents: a 16 px box
                                    // is not a phone target. The box keeps its role
                                    // and focus and has no handler of its own, so a
                                    // keyboard Space and a tap take one code path.
                                    <span
                                        class="flex w-full cursor-pointer items-center gap-2 rounded-md px-1 py-1.5 text-sm"
                                        data-testid="which-copies-row"
                                        data-stack=row.collection_id.to_string()
                                        on:click={
                                            let pick = pick.clone();
                                            move |_| toggle.run(pick.clone())
                                        }
                                    >
                                        <Checkbox checked aria_label=label.clone() />
                                        <span class="truncate">{label}</span>
                                    </span>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </Show>
        </section>
    }
}

/// One stack's sentence: where it is, which printing (only when that
/// distinguishes anything — see [`CopyStack`]), which board (only inside a deck
/// where it is not the mainboard), and **what a tick does to it**.
///
/// That last part is why the count reads `1 of 3 copies` rather than
/// `3 copies`: a row is a checkbox that moves exactly one copy, and a row
/// labelled with the stack's whole size invites the reading that ticking it
/// moves the stack. The dialog's description says "one copy from each", but it
/// sits above a scrolling list and cannot be the only place that is stated.
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
    parts.push(if row.quantity == 1 {
        "1 copy".to_string()
    } else {
        format!("1 of {} copies", row.quantity)
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
/// Every other refusal (`Grain`, `ManyCollections`, `ManyPrintings`,
/// `ManyBoards`, `AlreadyThere`) names a question the user can still act on —
/// open the right page, pick the right grain — so those stay checked exactly as
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
            quantity,
        }
    }

    #[test]
    fn only_the_which_copies_questions_are_askable() {
        // The step lists `(collection, printing, board)` rows. The three `Many*`
        // refusals name exactly those dimensions; nothing else on this enum is a
        // choice between rows a user could tell apart.
        for reason in [
            SkipReason::ManyCollections(2),
            SkipReason::ManyPrintings(2),
            SkipReason::ManyBoards(2),
        ] {
            assert!(
                reason.is_ambiguous(),
                "{reason:?} is a which-copies question"
            );
        }
        for reason in [
            SkipReason::AlreadyThere,
            SkipReason::Grain(2),
            SkipReason::NoCopies,
            SkipReason::NoLongerNeeded,
        ] {
            assert!(!reason.is_ambiguous(), "{reason:?} has no rows to offer");
        }
    }

    #[test]
    fn a_stack_is_one_row_however_many_grains_it_holds() {
        // The rows the step offers are the grain the *write* is addressed at
        // minus finish/condition/language — so a foil and a plain copy of the
        // same printing in the same binder are one row of 4, exactly as the
        // collection page renders them.
        let holdings = [foil(1, 100, 3), own(1, 100, 1), own(2, 100, 2)];
        assert_eq!(
            stacks_of(&holdings),
            vec![
                StackTally {
                    collection_id: id(1),
                    printing_id: id(100),
                    board: Board::Main,
                    quantity: 4,
                },
                StackTally {
                    collection_id: id(2),
                    printing_id: id(100),
                    board: Board::Main,
                    quantity: 2,
                },
            ]
        );
        // Boards stay apart — they are two rows on the deck page and two
        // choices here, which is the whole of `ManyBoards`.
        let boards = [own(1, 100, 2), on_board(Board::Side, 1, 100, 1)];
        assert_eq!(stacks_of(&boards).len(), 2);
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
        assert_eq!(ask[0].reason, SkipReason::ManyCollections(2));
        assert_eq!(tell, vec![skipped[1].clone()]);
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

    fn asked(oracle: u128, name: &str) -> AmbiguousCard {
        AmbiguousCard {
            token: SelectionKey::Card {
                oracle_id: id(oracle),
            }
            .token(),
            oracle_id: id(oracle),
            name: name.to_string(),
            reason: SkipReason::ManyCollections(2),
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
    fn a_card_whose_copies_vanished_keeps_its_section() {
        // Its stacks moved or emptied between the batch and this read. A card
        // silently absent from a list the user was asked to act on is the
        // "did that land?" state the toasts exist to refuse.
        let rows = card_choices(vec![asked(1, "Bolt")], &[], id(9));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].rows.is_empty());
    }

    #[test]
    fn a_picked_row_is_an_ordinary_held_item() {
        // The whole trick: no new write, no new wire shape — a pick is the same
        // key a collection-page row already submits, so the server resolves it
        // through `resolve_held` against its own fresh read.
        let card = asked(1, "Bolt");
        let picks = vec![
            StackPick::of(&card, &stack(1, 100, 2)),
            StackPick::of(
                &card,
                &CopyStack {
                    board: Board::Side,
                    ..stack(2, 101, 1)
                },
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
                },
                SelectionItem {
                    key: SelectionKey::Held {
                        collection_id: id(2),
                        printing_id: id(101),
                        board: Board::Side,
                    },
                    oracle_id: id(1),
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
                format!("{}|Bolt", picks[0].key().token()),
                format!("{}|Bolt", picks[1].key().token()),
            ]
        );
    }

    #[test]
    fn a_moved_pick_retires_the_tray_row_it_answered_for() {
        // The pass moves `held:` tokens; the tray holds the `card:` entry those
        // answer. Without the translation the pill keeps counting a card whose
        // copies just moved.
        let card = asked(1, "Bolt");
        let here = StackPick::of(&card, &stack(1, 100, 2));
        let there = StackPick::of(&card, &stack(2, 100, 1));
        let picks = vec![here.clone(), there.clone()];
        // One of two moved: the question was answered, so the entry goes —
        // whatever did not move has its own refusal toast.
        assert_eq!(
            answered_tokens(&picks, &[here.key().token()]),
            vec![card.token.clone()]
        );
        // Both moved: named once, not twice.
        assert_eq!(
            answered_tokens(&picks, &[here.key().token(), there.key().token()]),
            vec![card.token]
        );
        // Nothing moved: nothing leaves the tray.
        assert!(answered_tokens(&picks, &[]).is_empty());
    }

    #[test]
    fn a_row_says_where_it_is_and_what_ticking_it_does() {
        // Not "3 copies": the row is a checkbox that moves exactly one, and a
        // label naming the stack's whole size reads as though ticking it takes
        // the stack.
        assert_eq!(stack_label(&stack(1, 100, 3)), "Binder 1 · 1 of 3 copies");
        assert_eq!(stack_label(&stack(1, 100, 1)), "Binder 1 · 1 copy");
        // A printing chip only when the read decided it distinguishes
        // something, and a board only when it is not the mainboard.
        assert_eq!(
            stack_label(&CopyStack {
                printing: Some("MH3 #123".to_string()),
                board: Board::Side,
                ..stack(1, 100, 2)
            }),
            "Binder 1 · MH3 #123 · sideboard · 1 of 2 copies"
        );
    }

    #[test]
    fn the_step_counts_copies_and_cards_separately() {
        // The headline flow, and the one `moved_message` gets wrong: three ticks
        // on one card are three copies of ONE card. Phrased through the batch
        // message this read "Moved 3 cards (1 copy each)".
        let bolt = asked(1, "Bolt");
        let picks: Vec<StackPick> = [stack(1, 100, 2), stack(2, 100, 1), stack(3, 100, 5)]
            .iter()
            .map(|row| StackPick::of(&bolt, row))
            .collect();
        let all: Vec<String> = picks.iter().map(|p| p.key().token()).collect();
        assert_eq!(
            picked_message(&picks, &all, "🗂 Deck"),
            "Moved 3 copies of 1 card → 🗂 Deck"
        );

        // Two cards, one tick each: the plural form names both numbers rather
        // than leaving either to be inferred.
        let brainstorm = asked(2, "Brainstorm");
        let pair = vec![
            StackPick::of(&bolt, &stack(1, 100, 2)),
            StackPick::of(&brainstorm, &stack(1, 200, 4)),
        ];
        let both: Vec<String> = pair.iter().map(|p| p.key().token()).collect();
        assert_eq!(
            picked_message(&pair, &both, "🗂 Deck"),
            "Moved 2 copies across 2 cards → 🗂 Deck"
        );

        // Counted from what *moved*, not from what was ticked: a pick the
        // server refused must not be claimed as a copy that landed.
        assert_eq!(
            picked_message(&picks, &all[..1], "🗂 Deck"),
            "Moved 1 copy of 1 card → 🗂 Deck"
        );
    }

    #[test]
    fn a_partial_answer_still_refuses_the_rest() {
        // `answered` suppresses the cancel toast, so without this the untouched
        // cards would get no sentence at all while staying checked in the tray.
        let bolt = asked(1, "Bolt");
        let brainstorm = asked(2, "Brainstorm");
        let cards = vec![bolt.clone(), brainstorm];
        let picks = vec![StackPick::of(&bolt, &stack(1, 100, 2))];
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
        // `AlreadyThere`, `Grain`, `ManyPrintings`, `ManyBoards` all name a
        // real question the user can still answer — none of them mean the
        // stack is gone, so none of them should be swept off the tray with
        // `NoCopies`.
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
                    reason: SkipReason::Grain(2),
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
