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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    pub desired: i32,
    /// Owned — global aggregate of present across all the user's collections
    /// (per oracle card).
    pub owned: i32,
    /// The portion of present rolled up from descendant collections (distinct so
    /// the UI can mark it).
    pub present_rollup: i32,
    /// Deck board this row belongs to (`main` outside a deck).
    pub board: Board,
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
/// `from = None` is an external intake, `to = None` a removal. Moves are
/// board-agnostic (the ledger has no board) — they act on the mainboard;
/// board re-labels are a separate card-tagging op, not a move.
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
    pub quantity: i32,
}

/// The id of a created move — returned so the toast can offer Undo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveReceipt {
    pub move_id: Id,
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
    pub quantity: i32,
}

/// Batch move: many items to one destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchMove {
    pub to_collection_id: Option<Id>,
    pub items: Vec<MoveItem>,
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

/// Result of a teardown — how many move rows it wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeardownReceipt {
    pub moves: i64,
}

/// A row of the virtual "All cards" view (specs/collection-api.md → AllCardsView):
/// per oracle card, the global owned count and how many collections hold it (the
/// `7 across 3 collections` summary replacing per-collection present).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllCardsRow {
    pub oracle_id: Id,
    pub name: String,
    pub owned: i32,
    pub in_collections: i32,
}

/// One keyset page of the everything-view, sorted by (name, oracle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllCardsView {
    pub cards: Vec<AllCardsRow>,
    pub next_cursor: Option<String>,
}

/// Where a needed card sits in another of the user's collections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeedLocation {
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
    pub locations: Vec<NeedLocation>,
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
