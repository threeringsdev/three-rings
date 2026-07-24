//! Catalog-domain DTOs (the wire projection of `CatalogStore`).
//!
//! Anonymous-safe reads. This crate carries only the seam-proving slice today;
//! the full `search` / `card_detail` / `card_summary` DTOs land with
//! collection-api's catalog endpoints, importing them from here.

use serde::{Deserialize, Serialize};

use crate::Id;

/// Layouts whose printings carry a genuinely distinct back-face image
/// (`printings.faces[i].image_uris`) — the layouts the flip control applies to.
/// `split` / `flip` / `adventure` / `aftermath` / `prepare` also have two
/// oracle faces but **one** image (their printing faces carry no `image_uris`
/// at all, verified on the dev catalog), so flippability is keyed off layout
/// rather than "has more than one face" (specs/TODO.md, DFC back-face task).
pub const BACK_FACE_LAYOUTS: &[&str] = &[
    "transform",
    "modal_dfc",
    "reversible_card",
    "double_faced_token",
    "art_series",
];

/// Whether this layout has a real, separately-imaged back face.
pub fn has_back_face(layout: Option<&str>) -> bool {
    layout.is_some_and(|l| BACK_FACE_LAYOUTS.contains(&l))
}

/// One oracle face parsed out of the raw `cards.card_faces` jsonb — the data
/// the flip control swaps through. Unknown jsonb keys (colors, artist, …) are
/// ignored; everything but `name` is optional because face shapes vary by
/// layout (lands have no mana cost, planeswalker backs carry `loyalty`, battle
/// fronts `defense`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardFace {
    pub name: String,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub power: Option<String>,
    #[serde(default)]
    pub toughness: Option<String>,
    #[serde(default)]
    pub loyalty: Option<String>,
    #[serde(default)]
    pub defense: Option<String>,
}

/// Parse `card_faces` jsonb into typed faces; empty on NULL or malformed data.
fn parse_faces(card_faces: Option<&serde_json::Value>) -> Vec<CardFace> {
    card_faces
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

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
    /// Per-face images (`faces[i].image_uris.normal`), index-parallel to the
    /// card's `card_faces`; empty for single-face printings. `image_uri` stays
    /// the flattened front-face value. `serde(default)` so an older hosted API
    /// answering a newer native client degrades to "no flip" rather than a
    /// deserialization error.
    #[serde(default)]
    pub face_image_uris: Vec<Option<String>>,
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

impl CardDetail {
    /// The per-face oracle data the flip control swaps between, parsed from
    /// the raw `card_faces` jsonb. Non-empty only for a layout with a real
    /// back face ([`has_back_face`]) and at least two well-formed faces —
    /// every other case (single-face, the one-image multi-face layouts,
    /// malformed jsonb) is an empty vec, which reads as "no flip control".
    pub fn flip_faces(&self) -> Vec<CardFace> {
        if !has_back_face(self.layout.as_deref()) {
            return vec![];
        }
        let faces = parse_faces(self.card_faces.as_ref());
        if faces.len() < 2 {
            return vec![];
        }
        faces
    }
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
    /// Per-face flip data (name / mana / type + the representative printing's
    /// per-face art). Non-empty only for a layout with a real back face — the
    /// flippability decision is made server-side off [`BACK_FACE_LAYOUTS`], so
    /// clients key the control off `faces.len() >= 2` without re-deriving
    /// layout rules. `serde(default)`: see `PrintingSummary::face_image_uris`.
    #[serde(default)]
    pub faces: Vec<CardFaceSummary>,
}

/// The preview subset of one face — what the hover card / touch sheet swap
/// when flipped. `image_uri` is the representative printing's art for this
/// face index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardFaceSummary {
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub image_uri: Option<String>,
}

impl CardFaceSummary {
    /// Zip the oracle faces (raw `card_faces` jsonb) with the representative
    /// printing's per-face images into the preview flip list. Empty unless the
    /// layout has a real back face and at least two faces parse — the same
    /// gate as [`CardDetail::flip_faces`], applied where the summary
    /// projections are built.
    pub fn build(
        layout: Option<&str>,
        card_faces: Option<&serde_json::Value>,
        face_images: &[Option<String>],
    ) -> Vec<CardFaceSummary> {
        if !has_back_face(layout) {
            return vec![];
        }
        let faces = parse_faces(card_faces);
        if faces.len() < 2 {
            return vec![];
        }
        faces
            .into_iter()
            .enumerate()
            .map(|(i, f)| CardFaceSummary {
                name: f.name,
                mana_cost: f.mana_cost,
                type_line: f.type_line,
                image_uri: face_images.get(i).cloned().flatten(),
            })
            .collect()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn faces_json() -> serde_json::Value {
        json!([
            {"name": "Front Face", "mana_cost": "{1}{R}", "type_line": "Creature — Test",
             "oracle_text": "Front text.", "power": "1", "toughness": "3", "colors": ["R"]},
            {"name": "Back Face", "type_line": "Land", "oracle_text": "Back text."}
        ])
    }

    fn detail(layout: Option<&str>, card_faces: Option<serde_json::Value>) -> CardDetail {
        CardDetail {
            oracle_id: Id::nil(),
            name: "Front Face // Back Face".into(),
            mana_cost: None,
            cmc: None,
            type_line: None,
            oracle_text: None,
            colors: vec![],
            color_identity: vec![],
            keywords: vec![],
            power: None,
            toughness: None,
            loyalty: None,
            layout: layout.map(Into::into),
            legalities: None,
            card_faces,
            all_parts: None,
            printings: vec![],
            rulings: vec![],
            ownership: None,
        }
    }

    #[test]
    fn back_face_layouts_are_the_two_image_ones() {
        for l in [
            "transform",
            "modal_dfc",
            "reversible_card",
            "double_faced_token",
            "art_series",
        ] {
            assert!(has_back_face(Some(l)), "{l} should flip");
        }
        // Two oracle faces but one image — keyed out by layout, per the task.
        for l in [
            "split",
            "flip",
            "adventure",
            "aftermath",
            "prepare",
            "normal",
            "meld",
            "token",
        ] {
            assert!(!has_back_face(Some(l)), "{l} should not flip");
        }
        assert!(!has_back_face(None));
    }

    #[test]
    fn flip_faces_parses_both_faces() {
        let d = detail(Some("transform"), Some(faces_json()));
        let faces = d.flip_faces();
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].name, "Front Face");
        assert_eq!(faces[0].mana_cost.as_deref(), Some("{1}{R}"));
        assert_eq!(faces[0].power.as_deref(), Some("1"));
        assert_eq!(faces[1].name, "Back Face");
        assert_eq!(faces[1].mana_cost, None);
        assert_eq!(faces[1].oracle_text.as_deref(), Some("Back text."));
    }

    #[test]
    fn flip_faces_is_empty_off_the_layout_allowlist() {
        // An adventure has two well-formed oracle faces but one image — the
        // layout gate, not the face count, is what decides.
        let d = detail(Some("adventure"), Some(faces_json()));
        assert!(d.flip_faces().is_empty());
    }

    #[test]
    fn flip_faces_is_empty_on_missing_or_malformed_jsonb() {
        assert!(detail(Some("transform"), None).flip_faces().is_empty());
        // Not an array of face objects → parse fails closed, no control.
        assert!(detail(Some("transform"), Some(json!({"name": "x"})))
            .flip_faces()
            .is_empty());
        // Fewer than two faces → nothing to flip to.
        assert!(detail(Some("transform"), Some(json!([{"name": "only"}])))
            .flip_faces()
            .is_empty());
    }

    #[test]
    fn summary_faces_zip_names_with_face_images() {
        let imgs = vec![
            Some("https://img/front".to_string()),
            Some("https://img/back".to_string()),
        ];
        let faces = CardFaceSummary::build(Some("modal_dfc"), Some(&faces_json()), &imgs);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].name, "Front Face");
        assert_eq!(faces[0].image_uri.as_deref(), Some("https://img/front"));
        assert_eq!(faces[1].name, "Back Face");
        assert_eq!(faces[1].image_uri.as_deref(), Some("https://img/back"));
    }

    #[test]
    fn summary_faces_tolerate_missing_images() {
        // A face index past the image list (or a NULL element) renders the
        // skeleton, not an error — the oracle swap is still worth having.
        let faces = CardFaceSummary::build(Some("transform"), Some(&faces_json()), &[None]);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].image_uri, None);
        assert_eq!(faces[1].image_uri, None);
    }

    #[test]
    fn summary_faces_are_empty_off_the_allowlist_or_underparsed() {
        let imgs = vec![Some("https://img/a".to_string())];
        assert!(CardFaceSummary::build(Some("split"), Some(&faces_json()), &imgs).is_empty());
        assert!(CardFaceSummary::build(None, Some(&faces_json()), &imgs).is_empty());
        assert!(CardFaceSummary::build(Some("transform"), None, &imgs).is_empty());
    }
}
