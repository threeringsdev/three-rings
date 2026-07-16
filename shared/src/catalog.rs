//! Catalog-domain DTOs (the wire projection of `CatalogStore`).
//!
//! Anonymous-safe reads. This crate carries only the seam-proving slice today;
//! the full `search` / `card_detail` / `card_summary` DTOs land with
//! collection-api's catalog endpoints, importing them from here.

use serde::{Deserialize, Serialize};

/// Result of the anonymous catalog-size probe (`CatalogStore::card_count`) — the
/// number of distinct oracle cards ingested. Zero until catalog-ingestion runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCount {
    pub cards: i64,
}
