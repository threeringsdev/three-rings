//! Tag & board DTOs — the wire projection of the tag/board `CollectionStore`
//! methods (specs/card-tagging.md, surface in specs/collection-api.md §Tags &
//! boards).
//!
//! Two annotation shapes: **tags** (labels — system/account/deck scoped, a
//! `tags` + `card_tags` many-to-many) and **boards** (a quantity-bearing
//! partition — the `board` column on `holdings`/`desires`). Boards ride the
//! existing holding/desire DTOs (see [`crate::collection::Board`]); this module
//! adds the tag types plus the board *re-label* request.

use serde::{Deserialize, Serialize};

use crate::{Board, Id};

/// A tag's scope, derived from the two nullable FKs on `tags`
/// (`user_id`/`collection_id`) — no separate enum column to keep in sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum TagScope {
    /// `user_id` NULL, `collection_id` NULL — built-in, world-readable, applies
    /// in any collection. Seeded by the migration; never created via the API.
    System,
    /// `user_id` set, `collection_id` NULL — the user's own, applies in any of
    /// their collections.
    Account,
    /// `user_id` set, `collection_id` set — the user's own, applies only within
    /// that collection.
    Deck { collection_id: Id },
}

impl TagScope {
    /// Derive the scope from a `tags` row's two nullable FKs.
    pub fn from_fks(user_id: Option<Id>, collection_id: Option<Id>) -> Self {
        match (user_id, collection_id) {
            (None, _) => TagScope::System,
            (Some(_), None) => TagScope::Account,
            (Some(_), Some(collection_id)) => TagScope::Deck { collection_id },
        }
    }
}

/// A tag definition visible in a collection's scope (system + the user's account
/// tags + that deck's tags).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: Id,
    pub name: String,
    /// Stable slug for the built-in system tags (`commander`/`companion`);
    /// `None` for user tags.
    pub builtin: Option<String>,
    /// Optional UI accent color.
    pub color: Option<String>,
    pub scope: TagScope,
}

/// Create an account- or deck-scoped tag. `collection_id = None` → **account**
/// scope (any of the user's collections); `Some` → **deck** scope (that
/// collection only). System (built-in) tags are seeded, never created here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTag {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    /// `Some` = deck-scoped to this collection; `None` = account-scoped.
    #[serde(default)]
    pub collection_id: Option<Id>,
}

/// Rename a tag (its own, not a built-in).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameTag {
    pub name: String,
}

/// Assign / remove a tag on a card in a collection. Anchored at
/// `(collection_id, oracle_id)` so it spans `holdings` **and** `desires` and
/// survives a card going from desired to held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagAssignment {
    pub collection_id: Id,
    pub oracle_id: Id,
    pub tag_id: Id,
}

/// A card carrying a tag (the "deck cards grouped by tag" / commander read).
/// Oracle-grain render fields plus the whole-card `color_identity` the commander
/// summary aggregates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaggedCard {
    pub oracle_id: Id,
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    /// A representative image (from any printing's `image_uris.normal`).
    pub image_uri: Option<String>,
    /// Whole-card color identity (Scryfall aggregates across faces).
    pub color_identity: Vec<String>,
}

/// A deck's commanders and the color identity derived from them — the built-in
/// `commander` tag's read model. Color identity is **not stored**: it is the
/// union of the commander-tagged cards' `color_identity`, recomputed on read, so
/// it is always current after an assignment change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckCommanders {
    pub commanders: Vec<TaggedCard>,
    /// WUBRG-ordered union of the commanders' color identities.
    pub color_identity: Vec<String>,
}

/// Re-label part or all of a holding/desire stack onto another board — a
/// quantity-preserving in-place update (**not** a `moves` entry). `quantity =
/// None` moves the whole row; a partial quantity splits the stack, merging into
/// the destination board's row when one already exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetBoard {
    pub board: Board,
    /// Copies to re-label; `None` = the whole row. Must be `0 < quantity <=`
    /// the row's current quantity.
    #[serde(default)]
    pub quantity: Option<i32>,
}

/// The canonical WUBRG order for presenting a color set (identity/colors).
const WUBRG: [&str; 5] = ["W", "U", "B", "R", "G"];

/// Union a set of per-card color-identity arrays into one WUBRG-ordered,
/// de-duplicated identity. Colorless (an empty union) stays empty. Any symbol
/// outside WUBRG (there are none in real data) is appended after, stably.
pub fn union_color_identity<'a>(sets: impl IntoIterator<Item = &'a [String]>) -> Vec<String> {
    let mut present: Vec<String> = Vec::new();
    for set in sets {
        for c in set {
            if !present.iter().any(|p| p == c) {
                present.push(c.clone());
            }
        }
    }
    let mut ordered: Vec<String> = WUBRG
        .iter()
        .filter(|w| present.iter().any(|p| p == *w))
        .map(|w| w.to_string())
        .collect();
    // Keep any non-WUBRG symbols (defensive) after the canonical five.
    for c in present {
        if !WUBRG.contains(&c.as_str()) {
            ordered.push(c);
        }
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_derives_from_fks() {
        let cid = Id::from_u128(1);
        let uid = Id::from_u128(2);
        assert_eq!(TagScope::from_fks(None, None), TagScope::System);
        // A system tag ignores any (spurious) collection_id.
        assert_eq!(TagScope::from_fks(None, Some(cid)), TagScope::System);
        assert_eq!(TagScope::from_fks(Some(uid), None), TagScope::Account);
        assert_eq!(
            TagScope::from_fks(Some(uid), Some(cid)),
            TagScope::Deck { collection_id: cid }
        );
    }

    #[test]
    fn color_identity_unions_in_wubrg_order_deduped() {
        let a = vec!["R".to_string(), "G".to_string()];
        let b = vec!["W".to_string(), "R".to_string()];
        let got = union_color_identity([a.as_slice(), b.as_slice()]);
        assert_eq!(got, vec!["W", "R", "G"]);
    }

    #[test]
    fn color_identity_empty_stays_colorless() {
        let empty: Vec<String> = Vec::new();
        assert!(union_color_identity([empty.as_slice()]).is_empty());
        assert!(union_color_identity(std::iter::empty::<&[String]>()).is_empty());
    }
}
