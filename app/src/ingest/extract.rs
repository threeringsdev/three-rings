//! Scryfall card-object → catalog-row extraction.
//!
//! The pure core of the pipeline: one bulk-file line (or one hydrated card
//! object) in, one `(CardRow, PrintingRow, PriceRow)` triple out, with the
//! layout rules from data-model's 2026-07-14 Scryfall shape review applied and
//! the schema's NOT NULL expectations *asserted* (fail loudly, never silently
//! skip or null — specs/catalog-ingestion.md → Extraction rules).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Multi-face layouts that must carry `card_faces` (meld deliberately absent —
/// meld parts are three separate single-face objects linked via `all_parts`).
const MULTIFACE_LAYOUTS: &[&str] = &[
    "transform",
    "modal_dfc",
    "split",
    "adventure",
    "flip",
    "reversible_card",
    "double_faced_token",
    "art_series",
];

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("unparseable card object: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("card {name:?} ({id}): {problem}")]
    Invalid {
        id: String,
        name: String,
        problem: String,
    },
}

/// The Scryfall card-object fields we read; everything else is dropped
/// (purchase URIs, cross-reference ids, … — data-model's store-vs-drop list).
/// Schema-required fields are `Option` here so their absence surfaces as a
/// diagnosable [`ExtractError::Invalid`] with the card's name, not a bare
/// serde error.
#[derive(Debug, Deserialize)]
pub struct SourceCard {
    pub id: Uuid,
    pub name: String,
    pub oracle_id: Option<Uuid>,
    pub layout: Option<String>,
    // oracle-scoped
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub colors: Option<Vec<String>>,
    pub color_identity: Option<Vec<String>>,
    pub color_indicator: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub produced_mana: Option<Vec<String>>,
    pub legalities: Option<Value>,
    pub card_faces: Option<Vec<Value>>,
    pub all_parts: Option<Vec<Value>>,
    pub reserved: Option<bool>,
    pub game_changer: Option<bool>,
    pub edhrec_rank: Option<i32>,
    // printing-scoped
    pub set_id: Option<Uuid>,
    pub set: Option<String>,
    pub collector_number: Option<String>,
    pub rarity: Option<String>,
    pub finishes: Option<Vec<String>>,
    pub lang: Option<String>,
    pub frame: Option<String>,
    pub frame_effects: Option<Vec<String>>,
    pub border_color: Option<String>,
    pub full_art: Option<bool>,
    pub textless: Option<bool>,
    pub promo: Option<bool>,
    pub promo_types: Option<Vec<String>>,
    pub flavor_name: Option<String>,
    pub artist: Option<String>,
    pub flavor_text: Option<String>,
    pub watermark: Option<String>,
    pub security_stamp: Option<String>,
    pub games: Option<Vec<String>>,
    pub digital: Option<bool>,
    pub released_at: Option<String>,
    pub image_uris: Option<Value>,
    pub prices: Option<Value>,
}

impl SourceCard {
    pub fn parse(line: &str) -> Result<Self, ExtractError> {
        Ok(serde_json::from_str(line)?)
    }
}

/// One row for `cards` (oracle identity). Field order is the serialization
/// order, which the ingest hash and the `jsonb_to_recordset` upsert both rely
/// on — append new fields at the end (before `ingest_hash`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CardRow {
    pub oracle_id: Uuid,
    pub name: String,
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub colors: Vec<String>,
    pub color_identity: Vec<String>,
    pub color_indicator: Option<Vec<String>>,
    pub keywords: Vec<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub produced_mana: Option<Vec<String>>,
    pub layout: Option<String>,
    pub legalities: Option<Value>,
    pub card_faces: Option<Value>,
    pub all_parts: Option<Value>,
    pub reserved: bool,
    pub game_changer: Option<bool>,
    pub edhrec_rank: Option<i32>,
    pub ingest_hash: i64,
}

/// One row for `printings`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PrintingRow {
    pub id: Uuid,
    pub oracle_id: Uuid,
    pub set_id: Uuid,
    pub collector_number: String,
    pub rarity: String,
    pub finishes: Vec<String>,
    pub lang: String,
    pub frame: Option<String>,
    pub frame_effects: Vec<String>,
    pub border_color: Option<String>,
    pub full_art: bool,
    pub textless: bool,
    pub promo: bool,
    pub promo_types: Vec<String>,
    pub flavor_name: Option<String>,
    pub artist: Option<String>,
    pub flavor_text: Option<String>,
    pub watermark: Option<String>,
    pub security_stamp: Option<String>,
    pub games: Vec<String>,
    pub digital: bool,
    pub released_at: Option<String>,
    pub image_uris: Option<Value>,
    pub faces: Option<Value>,
    pub ingest_hash: i64,
}

/// One row for `prices` — Scryfall ships decimal strings; they pass through
/// verbatim and Postgres casts text → numeric exactly. Deliberately outside
/// the ingest hashes so daily price churn never dirties catalog rows.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PriceRow {
    pub printing_id: Uuid,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub usd_etched: Option<String>,
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
    pub tix: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
    pub card: CardRow,
    pub printing: PrintingRow,
    pub prices: PriceRow,
    /// Set code (`'mh3'`) — carried for the POC filter and diagnostics.
    pub set_code: String,
}

/// Per-face keys that are ORACLE-scoped → `cards.card_faces`.
const ORACLE_FACE_KEYS: &[&str] = &[
    "name",
    "mana_cost",
    "type_line",
    "oracle_text",
    "colors",
    "color_indicator",
    "power",
    "toughness",
    "loyalty",
    "defense",
];

/// Per-face keys that vary BY PRINTING → `printings.faces`.
const PRINTING_FACE_KEYS: &[&str] = &["image_uris", "artist", "flavor_text"];

/// Keys of an `all_parts` entry we store (data-model's subset).
const ALL_PARTS_KEYS: &[&str] = &["id", "component", "name", "type_line"];

/// Copy the listed keys (skipping absent/null ones) out of a JSON object.
fn subset(obj: &Value, keys: &[&str]) -> Value {
    let mut m = serde_json::Map::new();
    if let Some(o) = obj.as_object() {
        for k in keys {
            if let Some(v) = o.get(*k) {
                if !v.is_null() {
                    m.insert((*k).to_string(), v.clone());
                }
            }
        }
    }
    Value::Object(m)
}

fn subset_array(items: &[Value], keys: &[&str]) -> Value {
    Value::Array(items.iter().map(|v| subset(v, keys)).collect())
}

/// Stable 64-bit content hash of a row serialized with `ingest_hash = 0`.
/// Serialization order is the struct's field order, so the hash is stable
/// across runs and machines (xxh3 over canonical serde_json bytes).
fn stable_hash<T: Serialize>(row: &T) -> i64 {
    let bytes = serde_json::to_vec(row).expect("catalog row serializes");
    xxhash_rust::xxh3::xxh3_64(&bytes) as i64
}

/// Extract the catalog rows from one parsed card object.
pub fn extract(source: SourceCard) -> Result<Extracted, ExtractError> {
    let id_str = source.id.to_string();
    let fail = |problem: String| ExtractError::Invalid {
        id: id_str.clone(),
        name: source.name.clone(),
        problem,
    };
    let layout = source.layout.as_deref();

    // NOT NULL assertions — fail loudly with the card named, never skip/null.
    let oracle_id = match source.oracle_id {
        Some(o) => o,
        // reversible_card carries oracle_id only per-face (both faces share it)
        None if layout == Some("reversible_card") => source
            .card_faces
            .as_deref()
            .and_then(|f| f.first())
            .and_then(|f| f.get("oracle_id"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| fail("reversible_card missing card_faces[0].oracle_id".into()))?,
        None => return Err(fail("missing oracle_id".into())),
    };
    let require =
        |field: &str, v: Option<String>| v.ok_or_else(|| fail(format!("missing {field}")));
    let set_id = source.set_id.ok_or_else(|| fail("missing set_id".into()))?;
    let set_code = require("set", source.set.clone())?;
    let collector_number = require("collector_number", source.collector_number.clone())?;
    let rarity = require("rarity", source.rarity.clone())?;
    if layout.is_some_and(|l| MULTIFACE_LAYOUTS.contains(&l)) && source.card_faces.is_none() {
        return Err(fail(format!(
            "multi-face layout {layout:?} without card_faces"
        )));
    }

    // Multi-face split: oracle-scoped face data → cards.card_faces, printing-
    // scoped face data → printings.faces; NULL for single-face rows.
    let card_faces = source
        .card_faces
        .as_deref()
        .map(|f| subset_array(f, ORACLE_FACE_KEYS));
    let printing_faces = source
        .card_faces
        .as_deref()
        .map(|f| subset_array(f, PRINTING_FACE_KEYS));
    let all_parts = source
        .all_parts
        .as_deref()
        .map(|p| subset_array(p, ALL_PARTS_KEYS));

    let mut card = CardRow {
        oracle_id,
        name: source.name.clone(),
        mana_cost: source.mana_cost,
        cmc: source.cmc,
        type_line: source.type_line,
        oracle_text: source.oracle_text,
        colors: source.colors.unwrap_or_default(),
        color_identity: source.color_identity.unwrap_or_default(),
        color_indicator: source.color_indicator,
        keywords: source.keywords.unwrap_or_default(),
        power: source.power,
        toughness: source.toughness,
        loyalty: source.loyalty,
        produced_mana: source.produced_mana,
        layout: source.layout,
        legalities: source.legalities,
        card_faces,
        all_parts,
        reserved: source.reserved.unwrap_or(false),
        game_changer: source.game_changer,
        edhrec_rank: source.edhrec_rank,
        ingest_hash: 0,
    };
    card.ingest_hash = stable_hash(&card);

    let mut printing = PrintingRow {
        id: source.id,
        oracle_id,
        set_id,
        collector_number,
        rarity,
        finishes: source.finishes.unwrap_or_default(),
        lang: source.lang.unwrap_or_else(|| "en".into()),
        frame: source.frame,
        frame_effects: source.frame_effects.unwrap_or_default(),
        border_color: source.border_color,
        full_art: source.full_art.unwrap_or(false),
        textless: source.textless.unwrap_or(false),
        promo: source.promo.unwrap_or(false),
        promo_types: source.promo_types.unwrap_or_default(),
        flavor_name: source.flavor_name,
        artist: source.artist,
        flavor_text: source.flavor_text,
        watermark: source.watermark,
        security_stamp: source.security_stamp,
        games: source.games.unwrap_or_default(),
        digital: source.digital.unwrap_or(false),
        released_at: source.released_at,
        image_uris: source.image_uris,
        faces: printing_faces,
        ingest_hash: 0,
    };
    printing.ingest_hash = stable_hash(&printing);

    let price = |key: &str| {
        source
            .prices
            .as_ref()
            .and_then(|p| p.get(key))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let prices = PriceRow {
        printing_id: source.id,
        usd: price("usd"),
        usd_foil: price("usd_foil"),
        usd_etched: price("usd_etched"),
        eur: price("eur"),
        eur_foil: price("eur_foil"),
        tix: price("tix"),
    };

    Ok(Extracted {
        card,
        printing,
        prices,
        set_code,
    })
}

/// Convenience: parse + extract one bulk-file line.
pub fn extract_line(line: &str) -> Result<Extracted, ExtractError> {
    extract(SourceCard::parse(line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMAL: &str = include_str!("fixtures/normal.json"); // Lightning Bolt, msc 806
    const TRANSFORM: &str = include_str!("fixtures/transform.json"); // Delver of Secrets, isd 51
    const SPLIT: &str = include_str!("fixtures/split.json"); // Fire // Ice, dmr 215
    const REVERSIBLE: &str = include_str!("fixtures/reversible.json"); // Zndrsplt, sld 379
    const TOKEN: &str = include_str!("fixtures/token.json"); // Cat Warrior, tmh3 5
    const MELD: &str = include_str!("fixtures/meld.json"); // Bruna, inr 14
    const GODZILLA: &str = include_str!("fixtures/godzilla.json"); // Huntmaster Liger, iko 370
    const DIGITAL: &str = include_str!("fixtures/digital.json"); // Plains, ana 1a
    const UN_FRACTIONAL: &str = include_str!("fixtures/un_fractional.json"); // Little Girl, unh 16

    fn ex(fixture: &str) -> Extracted {
        extract_line(fixture).expect("fixture extracts")
    }

    #[test]
    fn normal_card_extracts_top_level_fields() {
        let e = ex(NORMAL);
        assert_eq!(e.card.name, "Lightning Bolt");
        assert_eq!(e.card.mana_cost.as_deref(), Some("{R}"));
        assert_eq!(e.card.cmc, Some(1.0));
        assert_eq!(e.card.colors, vec!["R"]);
        assert_eq!(e.card.layout.as_deref(), Some("normal"));
        assert!(e.card.oracle_text.is_some());
        assert!(e.card.card_faces.is_none(), "single-face → card_faces NULL");
        assert_eq!(e.set_code, "msc");
        assert_eq!(e.printing.collector_number, "806");
        assert_eq!(e.printing.rarity, "uncommon");
        assert_eq!(e.printing.finishes, vec!["nonfoil", "foil"]);
        assert_eq!(e.printing.lang, "en");
        assert_eq!(e.printing.promo_types, vec!["universesbeyond"]);
        assert!(e.printing.image_uris.is_some());
        assert!(e.printing.faces.is_none(), "single-face → faces NULL");
        assert_eq!(e.prices.usd.as_deref(), Some("0.74"));
        assert_eq!(e.prices.usd_etched, None);
        assert_eq!(e.prices.tix.as_deref(), Some("0.02"));
    }

    #[test]
    fn transform_card_gets_faces_and_null_top_level() {
        let e = ex(TRANSFORM);
        assert_eq!(e.card.name, "Delver of Secrets // Insectile Aberration");
        // Scryfall omits per-face fields at top level on multi-face layouts;
        // we mirror that: the top-level columns stay NULL, faces carry them.
        assert_eq!(e.card.mana_cost, None);
        assert_eq!(e.card.oracle_text, None);
        assert_eq!(e.card.colors, Vec::<String>::new());
        assert_eq!(e.card.cmc, Some(1.0), "cmc IS top-level on transform");

        let faces = e.card.card_faces.expect("oracle faces");
        let faces = faces.as_array().unwrap();
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0]["name"], "Delver of Secrets");
        assert_eq!(faces[0]["mana_cost"], "{U}");
        assert_eq!(faces[1]["power"], "3");
        // oracle-scoped faces must NOT carry printing-scoped data
        assert!(faces[0].get("image_uris").is_none());
        assert!(faces[0].get("artist").is_none());

        assert!(
            e.printing.image_uris.is_none(),
            "transform: per-face images only"
        );
        let pfaces = e.printing.faces.expect("printing faces");
        let pfaces = pfaces.as_array().unwrap();
        assert_eq!(pfaces.len(), 2);
        assert!(pfaces[0].get("image_uris").is_some());
        assert_eq!(pfaces[0]["artist"], "Nils Hamm");
        // and printing faces must NOT re-carry oracle data
        assert!(pfaces[0].get("oracle_text").is_none());
    }

    #[test]
    fn split_card_keeps_shared_top_level_image_with_per_face_artists() {
        let e = ex(SPLIT);
        assert_eq!(e.card.name, "Fire // Ice");
        assert_eq!(e.card.mana_cost.as_deref(), Some("{1}{R} // {1}{U}"));
        assert_eq!(e.card.colors, vec!["R", "U"]);
        let faces = e.card.card_faces.expect("both halves");
        assert_eq!(faces.as_array().unwrap()[1]["name"], "Ice");
        // split cards share one physical image but credit two artists
        assert!(e.printing.image_uris.is_some());
        let pfaces = e.printing.faces.expect("printing faces");
        let pfaces = pfaces.as_array().unwrap();
        assert_eq!(pfaces[0]["artist"], "David Martin");
        assert_eq!(pfaces[1]["artist"], "Franz Vohwinkel");
    }

    #[test]
    fn reversible_card_reads_oracle_id_from_first_face() {
        let e = ex(REVERSIBLE);
        assert_eq!(
            e.card.oracle_id,
            "502849a6-8e65-40f3-b348-a41c4f939768"
                .parse::<Uuid>()
                .unwrap(),
            "reversible_card has no top-level oracle_id — read card_faces[0]"
        );
        assert_eq!(e.printing.oracle_id, e.card.oracle_id);
    }

    #[test]
    fn missing_oracle_id_on_non_reversible_is_an_error() {
        let mut v: Value = serde_json::from_str(NORMAL).unwrap();
        v.as_object_mut().unwrap().remove("oracle_id");
        let err = extract_line(&v.to_string()).unwrap_err();
        assert!(
            matches!(err, ExtractError::Invalid { ref problem, .. } if problem.contains("oracle_id")),
            "got: {err}"
        );
    }

    #[test]
    fn missing_rarity_is_an_error_naming_the_card() {
        let mut v: Value = serde_json::from_str(TOKEN).unwrap();
        v.as_object_mut().unwrap().remove("rarity");
        let err = extract_line(&v.to_string()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Cat Warrior") && msg.contains("rarity"),
            "got: {msg}"
        );
    }

    #[test]
    fn multiface_layout_without_faces_is_an_error() {
        let mut v: Value = serde_json::from_str(TRANSFORM).unwrap();
        v.as_object_mut().unwrap().remove("card_faces");
        let err = extract_line(&v.to_string()).unwrap_err();
        assert!(
            matches!(err, ExtractError::Invalid { ref problem, .. } if problem.contains("card_faces")),
            "got: {err}"
        );
    }

    #[test]
    fn token_keeps_all_parts_relations() {
        let e = ex(TOKEN);
        assert_eq!(e.card.layout.as_deref(), Some("token"));
        let parts = e.card.all_parts.expect("relations");
        let parts = parts.as_array().unwrap();
        assert_eq!(parts.len(), 3);
        // stored shape is the data-model subset {id, component, name, type_line}
        assert!(parts[0].get("id").is_some());
        assert!(parts[0].get("component").is_some());
        assert!(parts[0].get("uri").is_none(), "source-only keys dropped");
    }

    #[test]
    fn meld_card_is_single_face_with_meld_parts() {
        let e = ex(MELD);
        assert_eq!(e.card.layout.as_deref(), Some("meld"));
        assert!(e.card.card_faces.is_none(), "meld parts are separate cards");
        let parts = e.card.all_parts.expect("meld relations");
        let comps: Vec<&str> = parts
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["component"].as_str().unwrap())
            .collect();
        assert!(comps.contains(&"meld_result"));
        assert!(comps.contains(&"meld_part"));
    }

    #[test]
    fn fractional_cmc_survives() {
        let e = ex(UN_FRACTIONAL);
        assert_eq!(e.card.cmc, Some(0.5));
    }

    #[test]
    fn flavor_name_variant_keeps_real_name() {
        let e = ex(GODZILLA);
        assert_eq!(e.card.name, "Huntmaster Liger");
        assert_eq!(
            e.printing.flavor_name.as_deref(),
            Some("King Caesar, Ancient Guardian")
        );
        assert_eq!(e.card.layout.as_deref(), Some("mutate"));
    }

    #[test]
    fn digital_printing_is_flagged() {
        let e = ex(DIGITAL);
        assert!(e.printing.digital);
        assert_eq!(e.printing.games, vec!["arena"]);
        assert_eq!(e.prices.usd, None);
    }

    #[test]
    fn hash_is_stable_and_ignores_prices() {
        let a = ex(NORMAL);
        let b = ex(NORMAL);
        assert_eq!(a.card.ingest_hash, b.card.ingest_hash);
        assert_eq!(a.printing.ingest_hash, b.printing.ingest_hash);
        assert_ne!(a.card.ingest_hash, 0, "hash must actually be computed");

        // price changes must not dirty catalog rows
        let mut v: Value = serde_json::from_str(NORMAL).unwrap();
        v["prices"]["usd"] = Value::String("999.99".into());
        let c = extract_line(&v.to_string()).unwrap();
        assert_eq!(a.card.ingest_hash, c.card.ingest_hash);
        assert_eq!(a.printing.ingest_hash, c.printing.ingest_hash);
        assert_eq!(c.prices.usd.as_deref(), Some("999.99"));

        // a real data change must dirty exactly the row it touches
        let mut v: Value = serde_json::from_str(NORMAL).unwrap();
        v["oracle_text"] = Value::String("Deals 4 damage now".into());
        let d = extract_line(&v.to_string()).unwrap();
        assert_ne!(a.card.ingest_hash, d.card.ingest_hash);
        assert_eq!(a.printing.ingest_hash, d.printing.ingest_hash);
    }
}
