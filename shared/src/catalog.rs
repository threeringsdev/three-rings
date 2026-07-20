//! Catalog-domain DTOs (the wire projection of `CatalogStore`).
//!
//! Anonymous-safe reads. This crate carries only the seam-proving slice today;
//! the full `search` / `card_detail` / `card_summary` DTOs land with
//! collection-api's catalog endpoints, importing them from here.

use serde::{Deserialize, Serialize};

use crate::Id;

/// Result of the anonymous catalog-size probe (`CatalogStore::card_count`) — the
/// number of distinct oracle cards ingested. Zero until catalog-ingestion runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCount {
    pub cards: i64,
}

/// A printing under a card's detail — the printing-picker row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintingSummary {
    pub id: Id,
    pub set_code: Option<String>,
    pub set_name: Option<String>,
    pub collector_number: String,
    pub rarity: String,
    pub image_uri: Option<String>,
    pub finishes: Vec<String>,
}

/// A ruling rendered on the card page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ruling {
    pub published_at: Option<String>,
    pub source: Option<String>,
    pub comment: String,
}

/// One line of the "your copies & locations" ownership block (authed only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnershipEntry {
    pub collection_id: Id,
    pub collection_name: String,
    pub printing_id: Id,
    pub quantity: i32,
}

/// Full card page (`/cards/:id`): oracle data + printings + rulings + related
/// parts, plus an ownership block present only when the caller is signed in
/// (specs/collection-api.md → CardDetail). jsonb columns pass through verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardDetail {
    pub oracle_id: Id,
    pub name: String,
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub colors: Vec<String>,
    pub color_identity: Vec<String>,
    pub keywords: Vec<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub layout: Option<String>,
    pub legalities: Option<serde_json::Value>,
    pub card_faces: Option<serde_json::Value>,
    pub all_parts: Option<serde_json::Value>,
    pub printings: Vec<PrintingSummary>,
    pub rulings: Vec<Ruling>,
    /// Present only when authed: the caller's copies & where they are.
    pub ownership: Option<Vec<OwnershipEntry>>,
}

/// Hover / quick-preview subset (specs → CardSummary). `owned` is filled only
/// when the caller is signed in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSummary {
    pub oracle_id: Id,
    pub name: String,
    /// The card's **representative printing** — the one whose art `image_uri`
    /// shows, and the grain `+ Have` adds at (holdings are per-printing, while
    /// a catalog row is per-oracle). Prefers a printing that has an image,
    /// falling back to the card's first printing so a card whose printings all
    /// lack art is still addable. `None` only for a card with no printings.
    pub printing_id: Option<Id>,
    pub image_uri: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub owned: Option<i32>,
}

/// A catalog search request. `q` is the raw Scryfall-style query string; its
/// translation to SQL is [catalog-search](catalog-search.md)'s — this shell does
/// a name match until then. Pairs with a [`crate::Page`] for keyset paging.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: Option<String>,
}

/// One keyset page of catalog search results, sorted by (name, oracle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResults {
    pub cards: Vec<CardSummary>,
    pub next_cursor: Option<String>,
}
