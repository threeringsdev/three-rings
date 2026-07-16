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
