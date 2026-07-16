//! Collection-domain DTOs (the wire projection of `CollectionStore`).
//!
//! Session-scoped: every read/write here runs, on the hosted side, inside a
//! per-request transaction that `SET LOCAL app.user_id`, so data-model's RLS
//! policies apply as a backstop. This crate carries only the seam-proving slice
//! today (`list_collections`); the tree CRUD / holdings / desires / moves DTOs
//! land with collection-api, importing them from here.

use serde::{Deserialize, Serialize};

use crate::Id;

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
