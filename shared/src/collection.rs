//! Collection-domain DTOs (the wire projection of `CollectionStore`).
//!
//! Session-scoped: every read/write here runs, on the hosted side, inside a
//! per-request transaction that `SET LOCAL app.user_id`, so data-model's RLS
//! policies apply as a backstop.

use serde::{Deserialize, Serialize};

use crate::{ApiError, Id};

/// Physical finish — mirrors the `card_finish` Postgres enum (specs/data-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    #[default]
    Nonfoil,
    Foil,
    Etched,
}

/// Physical condition grade — mirrors the `card_condition` Postgres enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    #[default]
    Nm,
    Lp,
    Mp,
    Hp,
    Dmg,
}

/// Deck board — mirrors the `card_board` Postgres enum (specs/card-tagging.md).
/// A quantity-bearing partition; meaningful only inside a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Board {
    #[default]
    Main,
    Side,
    Maybe,
}

/// The default holding/desire language (Scryfall code).
pub fn default_language() -> String {
    "en".to_string()
}

macro_rules! pg_enum {
    ($t:ty, $($variant:ident => $label:literal),+ $(,)?) => {
        impl $t {
            /// The Postgres enum label (bound as text, cast in SQL).
            pub fn to_pg(self) -> &'static str {
                match self { $(<$t>::$variant => $label),+ }
            }
            /// Parse the Postgres enum's text form.
            pub fn from_pg(s: &str) -> Option<Self> {
                match s { $($label => Some(<$t>::$variant),)+ _ => None }
            }
        }
    };
}
pg_enum!(Finish, Nonfoil => "nonfoil", Foil => "foil", Etched => "etched");
pg_enum!(Condition, Nm => "nm", Lp => "lp", Mp => "mp", Hp => "hp", Dmg => "dmg");
pg_enum!(Board, Main => "main", Side => "side", Maybe => "maybe");

/// A collection's kind. Mirrors the `collection_kind` Postgres enum
/// (specs/data-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    Binder,
    Deck,
}

impl CollectionKind {
    /// Parse the Postgres enum's text form (`kind::text`).
    pub fn from_pg(s: &str) -> Option<Self> {
        match s {
            "binder" => Some(CollectionKind::Binder),
            "deck" => Some(CollectionKind::Deck),
            _ => None,
        }
    }

    /// The Postgres `collection_kind` label (bound as text, cast in SQL).
    pub fn to_pg(self) -> &'static str {
        match self {
            CollectionKind::Binder => "binder",
            CollectionKind::Deck => "deck",
        }
    }
}

/// One row of a user's collection tree — the flat shape the list endpoint
/// returns; the client reassembles the tree from `parent_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub id: Id,
    /// `None` at top level.
    pub parent_id: Option<Id>,
    pub kind: CollectionKind,
    pub name: String,
    /// The single undeletable Inbox row is flagged here.
    pub is_inbox: bool,
    /// Fractional index for drag-reorder among siblings.
    pub position: f64,
    /// Set on decks only (e.g. `commander`, `modern`).
    pub format: Option<String>,
}

/// Create a binder or deck (specs/collection-api.md -> Tree CRUD). `format` is
/// deck-only; the API rejects a format on a binder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewCollection {
    /// `None` = top level.
    pub parent_id: Option<Id>,
    pub kind: CollectionKind,
    pub name: String,
    pub format: Option<String>,
}

/// Rename a collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rename {
    pub name: String,
}

/// Reparent a collection. `new_parent_id = None` moves it to the top level. The
/// API rejects a cycle (target is the node or one of its descendants).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reparent {
    pub new_parent_id: Option<Id>,
}

/// Reorder among siblings via a fractional index the client computed (midpoint
/// of the two neighbors it dropped between) -- a one-row write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reorder {
    pub position: f64,
}

/// One collection in the sidebar-tree read: its summary row plus its **own**
/// present-copies count (children not rolled up — the client reassembles the
/// tree from `parent_id` and rolls badges up during that walk).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionTreeRow {
    pub summary: CollectionSummary,
    /// Copies held in this collection alone (`SUM(holdings.quantity)`).
    pub present: i64,
    /// Copies desired in this collection alone (`SUM(desires.quantity)`),
    /// never rolled up — a want is scoped to the deck that states it, unlike
    /// a have there is no "the child's wants also belong to the parent"
    /// reading. Added for the delete confirm's honest wants count
    /// (specs/collection-deletion.md → step 4, `P6-189`): a delete opened
    /// from a sidebar row has no `collection_view` to read it from, so it
    /// has to ride the same tree read `present` already does.
    #[serde(default)]
    pub desired: i64,
}

/// The My-cards sidebar in one round-trip (specs/app-ui.md → Collection tree):
/// every collection with its own present count, plus the shopping-list badge.
/// Like `list_collections`, the read lazily provisions the Inbox — it is
/// equally a "first `/my` request" (specs/collection-api.md → Inbox
/// provisioning).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionTree {
    /// Flat, ordered by `(position, name)`; nesting is the client's walk.
    pub collections: Vec<CollectionTreeRow>,
    /// Distinct cards short (the Shopping list pinned row's badge).
    pub shopping_short: i64,
}

/// A present-copies row (`holdings`), at printing + finish/condition/language +
/// board grain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingLine {
    pub id: Id,
    pub collection_id: Id,
    pub printing_id: Id,
    pub finish: Finish,
    pub condition: Condition,
    pub language: String,
    pub board: Board,
    pub quantity: i32,
}

/// A desired-count row (`desires`), at oracle grain with an optional printing pin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesireLine {
    pub id: Id,
    pub collection_id: Id,
    pub oracle_id: Id,
    /// `None` = any printing; `Some` = pinned to a specific printing.
    pub printing_id: Option<Id>,
    pub board: Board,
    pub quantity: i32,
}

/// `+ Have` — add present copies of a printing to a collection. Upserts the
/// unique (collection, printing, finish, condition, language, board) row,
/// incrementing its quantity, and appends an intake `moves` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddHave {
    pub printing_id: Id,
    #[serde(default)]
    pub finish: Finish,
    #[serde(default)]
    pub condition: Condition,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub board: Board,
    /// Copies to add (must be > 0).
    pub quantity: i32,
}

/// `+ Want` — add a desired count for a card in a collection. Upserts the unique
/// (collection, oracle, printing, board) row, incrementing its quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddWant {
    pub oracle_id: Id,
    /// `None` = any printing; `Some` = pin to a specific printing.
    #[serde(default)]
    pub printing_id: Option<Id>,
    #[serde(default)]
    pub board: Board,
    /// Desired copies to add (must be > 0).
    pub quantity: i32,
}

/// Set a holding's absolute quantity (the stepper). `0` deletes the row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetQuantity {
    pub quantity: i32,
}

/// One line of a batch add (the enter-50-cards path). Internally tagged by
/// `kind` so the client can mix haves and wants in one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddLine {
    Have(AddHave),
    Want(AddWant),
}

/// Per-line outcome of a batch add — one bad line doesn't sink the batch
/// (specs/collection-api.md chose per-line results over all-or-nothing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LineResult {
    Ok,
    Error { error: ApiError },
}

/// A card entry in a collection view (specs/collection-api.md → Read models).
/// Grain is `(printing, board)`: present sums a card's copies across
/// finish/condition/language within this collection; the three counts are all
/// *in this context* except `owned` (a global aggregate).
///
/// A row can be **desire-only** (`present == 0`): a card this collection wants
/// but does not hold. Those rows carry the card's *representative* printing
/// (the has-art-first pick the catalog uses) because there is no held printing
/// to name — see [`CollectionView`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardRow {
    pub oracle_id: Id,
    pub printing_id: Id,
    pub name: String,
    /// Set code (e.g. `mh3`), if the printing's set is known.
    pub set_code: Option<String>,
    pub collector_number: String,
    /// A representative image (from `printings.image_uris.normal`), if present.
    pub image_uri: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub colors: Vec<String>,
    /// Present here — copies of this printing/board in *this* collection.
    pub present: i32,
    /// Desired here — target count for this card/board in *this* collection.
    /// Oracle-grained, so it repeats on every printing row of that oracle; the
    /// UI shows it once (see `crate::CardRow` consumers).
    pub desired: i32,
    /// Owned — global aggregate of present across all the user's collections
    /// (per oracle card).
    pub owned: i32,
    /// The portion of present rolled up from descendant collections (distinct so
    /// the UI can mark it).
    pub present_rollup: i32,
    /// Deck board this row belongs to (`main` outside a deck).
    pub board: Board,
    /// The one `holdings` row behind this cell — the in-place count stepper's
    /// write target (`set_holding_quantity`).
    ///
    /// `None` when the cell is **not** addressable by a single number: either it
    /// aggregates several finish/condition/language grains of the same
    /// (printing, board), or it is a desire-only row with no holding at all.
    /// The stepper renders only where this is `Some`, because a lone count
    /// cannot say which grain it meant.
    #[serde(default)]
    pub holding_id: Option<Id>,
    /// Per-face flip data for **this row's printing** (not a representative
    /// one), so a collection row's preview flips the copy you actually hold.
    /// Non-empty only for a layout with a real back face — the same
    /// server-side gate as [`crate::CardSummary::faces`].
    #[serde(default)]
    pub faces: Vec<crate::CardFaceSummary>,
}

/// Collection-wide counts for the view header — the one thing on the page that
/// is deliberately **not** per-page (specs/collection-api.md computes card-row
/// aggregates for the visible page only; a header that did the same would
/// change as you paged).
///
/// The wireframe's header reads
/// `120 here (102 own + 18 rolled up) · 6 wanted` and its needs chip
/// `6 missing — 4 owned elsewhere · 2 to buy`; these are those six numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionTotals {
    /// Copies held in this collection itself.
    pub present: i32,
    /// Copies held in its strict descendants (the italic/dimmed rollup).
    pub present_rollup: i32,
    /// Total desired here, across every card and board.
    pub desired: i32,
    /// Σ over cards of `max(desired − present_here, 0)` — the needs chip's
    /// headline. Equals `owned_elsewhere + to_buy` by construction.
    pub missing: i32,
    /// The part of `missing` fillable from the caller's other collections
    /// (per card, `min(gap, held elsewhere)`) — the needs view's first bucket.
    pub owned_elsewhere: i32,
    /// The part of `missing` nobody holds — the shopping-list bucket.
    pub to_buy: i32,
}

impl CollectionTotals {
    /// Copies in this collection *and* everything under it — the number the
    /// sidebar badge shows for the same node.
    pub fn present_total(&self) -> i32 {
        self.present + self.present_rollup
    }
}

/// One keyset page of a collection's card rows plus the collection's own
/// metadata and its immediate children (specs/collection-api.md → CollectionView).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionView {
    pub collection: CollectionSummary,
    /// Immediate child collections (the tree is rebuilt client-side from the
    /// full `list_collections`; this is the one-level view for the header).
    pub children: Vec<CollectionSummary>,
    pub cards: Vec<CardRow>,
    /// Opaque cursor for the next page, or `None` at the end.
    pub next_cursor: Option<String>,
    /// Whole-collection counts for the header + needs chip (not per-page).
    #[serde(default)]
    pub totals: CollectionTotals,
    /// Decks only: the `commander`-tagged cards and their derived color
    /// identity (specs/collection-api.md → "Decks additionally carry format,
    /// commander(s)"). `None` for a binder — never queried there.
    #[serde(default)]
    pub commanders: Option<crate::DeckCommanders>,
}

/// Keyset page request: an opaque `cursor` from a prior page (or `None` for the
/// first) and a `limit`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

impl Page {
    /// The effective page size, clamped to a sane range (default 50, max 200).
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200) as i64
    }
}

/// Move physical copies between collections (specs/collection-api.md → Move).
/// `from = None` is an external intake, `to = None` a removal.
///
/// **A move addresses a board at each end, and the two need not agree.**
/// `from_board` names the stack the copies are taken from — a deck's sideboard
/// is a different stack of the same printing, and a write that assumed `main`
/// would take copies the caller never pointed at (or none at all). `to_board` is
/// where they land, normally `main`: copies moved out of a sideboard into a
/// binder are just copies in a binder. Re-labelling a board *in place* is
/// card-tagging's separate quantity-preserving op, not a move.
///
/// Both default to `main`, so a caller that has no boards writes the same
/// request it always did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveRequest {
    pub from_collection_id: Option<Id>,
    pub to_collection_id: Option<Id>,
    pub printing_id: Id,
    #[serde(default)]
    pub finish: Finish,
    #[serde(default)]
    pub condition: Condition,
    #[serde(default = "default_language")]
    pub language: String,
    /// Board the copies leave (ignored for an intake, where `from` is `None`).
    #[serde(default)]
    pub from_board: Board,
    /// Board the copies land on (ignored for a removal, where `to` is `None`).
    #[serde(default)]
    pub to_board: Board,
    pub quantity: i32,
}

/// Move copies **out of one specific `holdings` row** — the grain-addressed
/// write, addressed by the id of the stack rather than by a grain the caller
/// re-states (specs/collection-api.md → Move; specs/app-ui.md → the collection
/// view's removal).
///
/// This exists because of a rule the batch-move task wrote down the hard way:
/// *anything deciding whether a write is possible cannot use a read model that
/// groups away the write's addressing.* A rendered collection row is
/// `(printing, board)` with finish/condition/language summed away, so it cannot
/// state the grain a [`MoveRequest`] needs. Naming the holding instead means the
/// **server** reads the grain, the board and the owning collection — inside the
/// same transaction that performs the write, which is also what closes the
/// check-then-write window a client-side resolution leaves open.
///
/// `to_collection_id = None` is a removal, and it is undoable like any other
/// move: the ledger row records the full grain and the board it came off, so
/// undo puts *those* copies back on *that* board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingMove {
    /// Destination, or `None` to remove the copies from the collection.
    pub to_collection_id: Option<Id>,
    /// Copies to move; `None` means the whole stack.
    ///
    /// `None` rather than a number the client computed from a rendered count:
    /// "remove this row" is what the user asked for, and a stale count would
    /// otherwise leave copies behind or fail the write.
    #[serde(default)]
    pub quantity: Option<i32>,
}

/// The id of a created move — returned so the toast can offer Undo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveReceipt {
    pub move_id: Id,
}

/// What undoing a move restores — the id of the holding row the reversed
/// move's copies landed back on, when they landed at the move's own
/// `from_collection_id`.
///
/// `None` in three cases, all meaning "there is no such holding to name":
/// the move being undone had no origin collection (a quick-add's intake has
/// nothing to point a caller back at — its undo only removes copies from the
/// destination); the move was already undone (idempotent — a second call
/// writes nothing, so there is nothing new to restore); and the origin
/// collection has since been soft-deleted, so `hosted::undo_one` redirects
/// the copies to the Inbox instead of `from_collection_id` (maintainer
/// ruling, specs/collection-deletion.md → Open questions). That redirect
/// lands the copies somewhere real, but naming that holding here would let a
/// caller still rendering the *original* collection's row — the
/// collection-view stepper — rewire itself onto an unrelated Inbox holding
/// through a row that no longer describes it. A plain removal's undo is
/// `Some` only when its origin collection is still live; the redirect case
/// is not rare enough to hand-wave away.
///
/// This exists so a caller holding a *dead* id — the collection-view stepper,
/// after removing the row it edits — can rewire itself to the *live* one the
/// undo just created, rather than waiting on an unrelated refetch to remount
/// it with fresh data (app-ui.md → Findings, the stale-stepper-id defect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReceipt {
    pub restored_holding_id: Option<Id>,
}

/// Which catalog quick action fired. The two differ in grain — a Have is
/// per-printing, a Want is per-oracle — and in whether the result is undoable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickAddKind {
    Want,
    Have,
}

/// What a catalog quick-add produced, and whether the confirmation toast can
/// offer Undo.
///
/// `Some` for `+ Have`: holdings writes append a `moves` row, and undo is that
/// ledger's `undone_at` flag (specs/collection-api.md → Undo). `None` for
/// `+ Want`: desires are not part of the move ledger and there is no
/// desire-quantity operation to compensate with, so a Want is confirmed but not
/// undoable — the toast drops its action rather than offering one that lies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickAddReceipt {
    pub undo_move_id: Option<Id>,
}

/// One line of a batch move — the persistent selection tray: N `(card, from)`
/// pairs to one destination, applied in a single transaction (all-or-nothing).
/// Boards work exactly as they do on [`MoveRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveItem {
    pub from_collection_id: Option<Id>,
    pub printing_id: Id,
    #[serde(default)]
    pub finish: Finish,
    #[serde(default)]
    pub condition: Condition,
    #[serde(default = "default_language")]
    pub language: String,
    /// Board the copies leave (ignored for an intake).
    #[serde(default)]
    pub from_board: Board,
    /// Board they land on (ignored for a removal).
    #[serde(default)]
    pub to_board: Board,
    pub quantity: i32,
}

/// Batch move: many items to one destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchMove {
    pub to_collection_id: Option<Id>,
    pub items: Vec<MoveItem>,
}

/// Prefix a batch-move failure with the **position** of the item that caused it.
///
/// A batch move is one transaction, so a single bad item rolls the whole batch
/// back — and `Conflict("no copies to move")` on its own names none of the cards
/// the user selected, which made a real failure diagnosable only by bisecting
/// the selection (specs/app-ui.md → Findings, batch move). The server does not
/// know the card *names*; the caller that built the list does. So the wire
/// carries the one thing that bridges them: the index.
///
/// This is part of `move_batch`'s contract rather than either side's private
/// convention, which is why both the writer and [`batch_item_index`] live here —
/// a format duplicated at two call sites is a format that drifts.
pub fn batch_item_error(index: usize, message: &str) -> String {
    format!("item {index}: {message}")
}

/// Recover the item position from a [`batch_item_error`] message, or `None` if
/// the failure was not attributable to one item (a transport or session error,
/// say). The remainder is returned so a caller can restate it beside a name.
pub fn batch_item_index(message: &str) -> Option<(usize, &str)> {
    let rest = message.strip_prefix("item ")?;
    let (index, rest) = rest.split_once(": ")?;
    Some((index.parse().ok()?, rest))
}

/// A collection that wants a card more than it currently has — the destination
/// picker's ranking (specs/collection-api.md → suggested-destinations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedDestination {
    pub collection_id: Id,
    pub collection_name: String,
    pub desired: i32,
    pub present: i32,
    pub shortfall: i32,
}

/// Empty a collection: move everything to `EmptyTo` a chosen destination, or
/// `ReturnToPrevious` — each card back to the most-recent collection it was
/// moved *into* here from (falling back to Inbox where there is no history).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Teardown {
    EmptyTo { to_collection_id: Id },
    ReturnToPrevious,
}

/// Result of a teardown — **the ledger rows it wrote**, in write order.
///
/// The ids, not a count, and the count is `move_ids.len()` rather than a second
/// field that could disagree with it. A teardown is the most destructive action
/// in the app and it is made entirely of reversible moves — the confirm dialog
/// says so ("every move is in the history") — so the caller needs handles, not a
/// number. ⌘K's `Undo last move` reverses the whole set through `undo_moves`;
/// without them it would find an *older, unrelated* move as "the last one" and
/// reverse that instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeardownReceipt {
    pub move_ids: Vec<Id>,
}

/// Where a deleted collection's **holdings** go
/// (specs/collection-deletion.md → The two dispositions).
///
/// Anything the user chooses to move out moves through the real `moves` ledger;
/// anything they do not simply goes hidden with the collection. `ToParent` is
/// the default because a have is a physical object that has to be *somewhere*.
///
/// The user never sees the word `Discard`: both controls label it
/// **"Remove from Collection"** (resolved 2026-08-05 in the spec). The variant
/// keeps the shorter name because it is what the code and the ledger reasoning
/// call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum HaveDisposition {
    /// The nearest surviving parent — always the deleted collection's own
    /// `parent_id`, since children re-parent rather than cascade — or the Inbox
    /// when it was top-level.
    #[default]
    ToParent,
    /// A collection the user picked. Must be live and owned, like any other
    /// move destination; the collection being deleted is refused.
    To { collection_id: Id },
    /// Each card back to the most recent **live** collection it was moved into
    /// this one from, Inbox where the history has none — the existing
    /// [`Teardown::ReturnToPrevious`] machinery, reused verbatim.
    ReturnToPrevious,
    /// Writes **nothing**: the holdings stay attached to the now-hidden
    /// collection, vanish from every count and every view because the
    /// collection is filtered out, and return intact on undo.
    Discard,
}

/// Where a deleted collection's **desires** go (specs/collection-deletion.md).
///
/// Chosen separately from [`HaveDisposition`], and defaulting the other way: a
/// want is an intention that was very likely scoped to the deck being deleted,
/// so leaving it attached to the hidden collection is the sane default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WantDisposition {
    /// Writes nothing — the desires go hidden with the collection.
    #[default]
    Discard,
    /// Move them to a live, owned collection (merging into its existing rows).
    To { collection_id: Id },
}

/// Delete a collection — which, since specs/collection-deletion.md, **relocates
/// rather than destroys**: the row is hidden (`deleted_at`), its live children
/// re-point at its parent, and its holdings move out as real ledger moves.
///
/// **Every field is optional on the wire.** The dispositions default to the
/// spec's `ToParent` / `Discard`, so a caller with no picker yet (the tree's
/// confirm dialog until step 4) sends nothing at all. `collection_id` defaults
/// to the nil uuid, which the hosted route reads as *unstated* and fills from
/// the path — see [`resolve_path_id`](Self::resolve_path_id). That keeps the
/// operation's long-standing "`POST …/{id}/delete` with no body" contract
/// working while the DTO keeps the shape specs/collection-deletion.md gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteCollectionReq {
    #[serde(default)]
    pub collection_id: Id,
    #[serde(default)]
    pub haves: HaveDisposition,
    #[serde(default)]
    pub wants: WantDisposition,
}

impl DeleteCollectionReq {
    /// The spec's defaults — holdings to the parent, wants left attached.
    pub fn defaults(collection_id: Id) -> Self {
        Self {
            collection_id,
            haves: HaveDisposition::default(),
            wants: WantDisposition::default(),
        }
    }

    /// Reconcile a request body with the collection named in the URL path.
    ///
    /// Unstated (the nil uuid, i.e. absent from the JSON) takes the path's id.
    /// **Stated and different is refused**, rather than one side quietly
    /// winning: a request that names two collections has no safe reading, and
    /// silently deleting the *other* one is the worst answer available.
    pub fn resolve_path_id(mut self, path_id: Id) -> crate::ApiResult<Self> {
        if self.collection_id == Id::nil() {
            self.collection_id = path_id;
            Ok(self)
        } else if self.collection_id != path_id {
            Err(ApiError::Validation(
                "collection_id does not match the path".into(),
            ))
        } else {
            Ok(self)
        }
    }
}

/// One relocated desire, enough to reverse a `WantDisposition::To` on undo
/// (specs/collection-deletion.md → step 5; maintainer ruling 2026-08-10,
/// Dylan: "undo must fully reverse `WantDisposition::To`").
///
/// Desires have no ledger — `WantDisposition::To` is a merge-and-drop
/// (`INSERT … ON CONFLICT ON CONSTRAINT desires_uniq DO UPDATE SET quantity =
/// desires.quantity + EXCLUDED.quantity`, then `DELETE` the source rows), so
/// there is no `move_id` for undo to hand to `undo_moves`. This is the undo
/// handle instead: `desires_uniq` is `UNIQUE NULLS NOT DISTINCT (collection_id,
/// oracle_id, printing_id, board)`, so `(oracle_id, printing_id, board)` plus
/// [`to_collection_id`](Self::to_collection_id) is the row's exact identity at
/// both ends, and `quantity` is the exact amount to move back. The source
/// collection is not repeated here — it is
/// [`DeleteCollectionReceipt::collection_id`], the same one every other handle
/// in the receipt reverses against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelocatedDesire {
    /// Where the delete merged this row into (`WantDisposition::To`'s
    /// destination). Undo decrements *this* row (clamped — the destination's
    /// want may have changed since) before re-inserting `quantity` at the
    /// source, mirroring how [`HaveDisposition::To`]'s `move_id`s reverse
    /// through the real ledger.
    pub to_collection_id: Id,
    pub oracle_id: Id,
    /// `None` = "any printing" (`desires.printing_id`'s own meaning).
    pub printing_id: Option<Id>,
    pub board: Board,
    pub quantity: i32,
}

/// What a delete wrote — **handles, not counts**, for the same reason
/// [`TeardownReceipt`] carries ids: the undo behind the toast has to reverse
/// exactly this operation (clear `deleted_at`, reverse every `move_id`,
/// re-parent every id in `reparented` back, re-insert every relocated desire),
/// and a count cannot do that.
///
/// `Discard`ed rows produce no ids/handles because they produce no writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteCollectionReceipt {
    /// The collection that was hidden.
    pub collection_id: Id,
    /// Holdings relocations, in write order (empty for `Discard`).
    pub move_ids: Vec<Id>,
    /// Children whose `parent_id` changed (they survive — delete removes
    /// exactly one node).
    pub reparented: Vec<Id>,
    /// Relocated desires (`WantDisposition::To` only — `Discard` writes
    /// nothing and needs no handle). Additive since `P6-188` shipped this
    /// receipt without it: `#[serde(default)]` so a receipt encoded before
    /// this field existed still decodes, as an empty list — undo just has
    /// nothing to reverse on the want side, which is the honest reading of an
    /// old receipt rather than a decode failure.
    #[serde(default)]
    pub desires: Vec<RelocatedDesire>,
}

/// A row of the virtual "All cards" view (specs/collection-api.md → AllCardsView):
/// per oracle card, the aggregate counts across *every* collection.
///
/// `/my` is "same row treatment as collection view" (specs/app-ui.md), so the
/// render fields come from the very same [`CardSummary`](crate::CardSummary) the
/// catalog rows use — one projection, one preview component, one flip control,
/// rather than a parallel set of near-identical columns.
///
/// The two derived numbers are deliberately *not* fields: `owned` is
/// `card.owned` (filled from the same holdings aggregate as `locations`) and the
/// `7 across 3 collections` denominator is `locations.len()`. A stored copy of
/// either could disagree with the list it summarizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllCardsRow {
    /// Name, art, mana cost, type line, representative printing, flip faces —
    /// and `owned`, the global present total, always `Some` here (this view is
    /// session-scoped; there is no anonymous `/my`).
    pub card: crate::CardSummary,
    /// Desired across every collection — the WANTED column.
    pub wanted: i32,
    /// Every collection holding copies, quantity-desc then name: the expandable
    /// location summary that replaces the collection view's HERE column.
    pub locations: Vec<CardLocation>,
}

impl AllCardsRow {
    /// Copies held across all collections — the OWNED column. Equals the sum of
    /// [`locations`](Self::locations) by construction.
    pub fn owned(&self) -> i32 {
        self.card.owned.unwrap_or(0)
    }

    /// How many collections hold at least one copy (the `across N collections`
    /// half of the location summary).
    pub fn in_collections(&self) -> usize {
        self.locations.len()
    }
}

/// One keyset page of the everything-view, sorted by (name, oracle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllCardsView {
    pub cards: Vec<AllCardsRow>,
    pub next_cursor: Option<String>,
}

/// Where copies of a card sit — one of the user's collections and how many are
/// in it. Shared by the needs view (fillable-from locations) and `/my`'s
/// expandable location summary; the shape was identical, so the name is the
/// general one rather than either caller's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardLocation {
    pub collection_id: Id,
    pub collection_name: String,
    pub quantity: i32,
}

/// A needed card in a collection (desired > present here). The gap splits into
/// `owned_elsewhere` (fillable from the user's other collections, with
/// `locations`) and `short` (still to buy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeedRow {
    pub oracle_id: Id,
    pub name: String,
    pub desired: i32,
    pub present_here: i32,
    pub owned_elsewhere: i32,
    pub short: i32,
    pub locations: Vec<CardLocation>,
}

/// A collection's needs, split Owned-elsewhere vs Short (specs → NeedsView).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeedsView {
    pub collection_id: Id,
    pub rows: Vec<NeedRow>,
}

/// One short card on the global shopping list: total desired across all
/// collections minus owned, floored at 0, plus which collections want it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShoppingRow {
    pub oracle_id: Id,
    pub name: String,
    pub desired_total: i32,
    pub owned: i32,
    pub shortfall: i32,
    pub wanted_by: Vec<String>,
}

/// The global, text-exportable shopping list (specs → ShoppingList).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShoppingList {
    pub rows: Vec<ShoppingRow>,
}

/// One row of the "Recently deleted" list (specs/collection-deletion.md →
/// step 5). Deliberately thin — name, kind, when — the spec is explicit that
/// this list carries **no counts**: it exists so a soft delete is reachable
/// after its toast is gone, not to describe what's inside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletedCollectionRow {
    pub id: Id,
    pub name: String,
    pub kind: CollectionKind,
    /// Server-formatted, not a raw timestamp — this crate is wasm-safe and
    /// dependency-free by design (no date library), and "deleted when" needs
    /// nothing more precise than a string the server already rendered.
    pub deleted_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_failure_carries_the_item_it_belongs_to() {
        let msg = batch_item_error(3, "no copies to move");
        assert_eq!(batch_item_index(&msg), Some((3, "no copies to move")));
    }

    /// The defaults are contractual, not stylistic: the confirm dialog opens on
    /// them, and a caller that posts neither disposition (every caller until the
    /// dialog's pickers land) must get "holdings to the parent, wants stay
    /// attached" — specs/collection-deletion.md → The two dispositions.
    #[test]
    fn the_delete_defaults_are_to_parent_and_discard() {
        let req = DeleteCollectionReq::defaults(Id::from_u128(1));
        assert_eq!(req.haves, HaveDisposition::ToParent);
        assert_eq!(req.wants, WantDisposition::Discard);

        // …and a body carrying only the id deserializes to the same thing, so
        // the wire default and the Rust default cannot drift apart.
        let wire: DeleteCollectionReq =
            serde_json::from_str(r#"{"collection_id":"00000000-0000-0000-0000-000000000001"}"#)
                .expect("id-only body");
        assert_eq!(wire, req);
    }

    /// The endpoint has always accepted `POST /api/collections/{id}/delete`
    /// with **no body**, and callers rely on it (every e2e cleanup helper posts
    /// either nothing or `{}`). Adding dispositions must not turn those into
    /// 422s, so an empty body is a complete request whose id comes from the
    /// path.
    #[test]
    fn an_empty_body_takes_its_collection_from_the_path() {
        let path_id = Id::from_u128(1);
        let empty: DeleteCollectionReq = serde_json::from_str("{}").expect("empty body");
        assert_eq!(empty.collection_id, Id::nil(), "unstated");
        assert_eq!(
            empty.resolve_path_id(path_id),
            Ok(DeleteCollectionReq::defaults(path_id))
        );
    }

    /// Stated and matching is fine; stated and different is refused outright —
    /// picking a winner would mean deleting a collection the caller did not
    /// name in the URL.
    #[test]
    fn a_body_that_names_another_collection_is_refused() {
        let path_id = Id::from_u128(1);
        let same = DeleteCollectionReq::defaults(path_id);
        assert_eq!(same.clone().resolve_path_id(path_id), Ok(same));

        let other = DeleteCollectionReq::defaults(Id::from_u128(2));
        assert!(matches!(
            other.resolve_path_id(path_id),
            Err(ApiError::Validation(_))
        ));
    }

    /// Both dispositions are internally tagged on `mode`, like [`Teardown`] —
    /// the native client and the hosted router share these strings.
    #[test]
    fn the_dispositions_round_trip_on_a_mode_tag() {
        let dest = Id::from_u128(7);
        for (value, json) in [
            (HaveDisposition::ToParent, r#"{"mode":"to_parent"}"#),
            (
                HaveDisposition::ReturnToPrevious,
                r#"{"mode":"return_to_previous"}"#,
            ),
            (HaveDisposition::Discard, r#"{"mode":"discard"}"#),
            (
                HaveDisposition::To {
                    collection_id: dest,
                },
                r#"{"mode":"to","collection_id":"00000000-0000-0000-0000-000000000007"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<HaveDisposition>(json).unwrap(),
                value
            );
        }
        for (value, json) in [
            (WantDisposition::Discard, r#"{"mode":"discard"}"#),
            (
                WantDisposition::To {
                    collection_id: dest,
                },
                r#"{"mode":"to","collection_id":"00000000-0000-0000-0000-000000000007"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<WantDisposition>(json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn an_unattributed_failure_stays_unattributed() {
        // Anything that is not one item's fault (a session error, a transport
        // failure) must not be read as item 0 — naming an innocent card is
        // worse than naming none.
        assert_eq!(batch_item_index("unauthorized"), None);
        assert_eq!(batch_item_index("item x: nope"), None);
        assert_eq!(batch_item_index("item 2"), None);
    }

    /// The receipt's `desires` field is additive (`P6-190`, step 5): a receipt
    /// encoded before it existed must still decode, as an empty list — not a
    /// decode failure. `#[serde(default)]` is what makes that true; this pins
    /// it against a future edit that drops the attribute.
    #[test]
    fn an_old_receipt_without_desires_still_decodes() {
        let wire = r#"{"collection_id":"00000000-0000-0000-0000-000000000001",
                        "move_ids":[],"reparented":[]}"#;
        let receipt: DeleteCollectionReceipt = serde_json::from_str(wire).expect("old receipt");
        assert_eq!(receipt.desires, Vec::new());
    }

    /// The receipt round-trips a populated `desires` list byte-for-byte — the
    /// shape undo actually reverses: `to_collection_id` (where the delete
    /// merged the want into), the row's identity
    /// (`oracle_id`/`printing_id`/`board`), and the `quantity` to move back.
    #[test]
    fn the_receipt_round_trips_relocated_desires() {
        let receipt = DeleteCollectionReceipt {
            collection_id: Id::from_u128(1),
            move_ids: vec![Id::from_u128(2)],
            reparented: vec![Id::from_u128(3)],
            desires: vec![
                RelocatedDesire {
                    to_collection_id: Id::from_u128(4),
                    oracle_id: Id::from_u128(5),
                    printing_id: Some(Id::from_u128(6)),
                    board: Board::Main,
                    quantity: 3,
                },
                RelocatedDesire {
                    to_collection_id: Id::from_u128(4),
                    oracle_id: Id::from_u128(7),
                    printing_id: None,
                    board: Board::Side,
                    quantity: 1,
                },
            ],
        };
        let json = serde_json::to_string(&receipt).expect("serializes");
        let back: DeleteCollectionReceipt = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(back, receipt);
    }
}
