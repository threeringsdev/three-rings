//! Batched catalog upserts (specs/catalog-ingestion.md → Writing to Postgres).
//!
//! Each batch binds ONE jsonb parameter — the serialized row array — and lets
//! `jsonb_to_recordset` type the columns server-side (uuid, numeric, date,
//! enum arrays, jsonb) so no per-column bind gymnastics or extra sqlx type
//! features are needed. Catalog upserts are hash-gated in-statement: an
//! unchanged row (`ingest_hash` equal) is skipped entirely, so re-runs and
//! true-ups write only deltas. `prices` upserts unconditionally — that table
//! exists to absorb the churn (data-model's split rationale).

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};

use super::extract::{CardRow, PriceRow, PrintingRow};
use super::scryfall::SetRow;
use super::IngestError;

fn as_jsonb<T: Serialize>(rows: &[T]) -> Result<serde_json::Value, IngestError> {
    serde_json::to_value(rows)
        .map_err(|e| IngestError::Source(format!("row serialization failed: {e}")))
}

/// Upsert the full `/sets` inventory (~1K rows — one statement, every run,
/// so printings' `set_id` FKs always resolve).
pub async fn sets(pool: &PgPool, rows: &[SetRow]) -> Result<u64, IngestError> {
    let done = sqlx::query(
        r#"
        INSERT INTO sets (id, code, name, set_type, released_at, card_count, icon_svg_uri)
        SELECT t.id, t.code, t.name, t.set_type, t.released_at, t.card_count, t.icon_svg_uri
        FROM jsonb_to_recordset($1) AS t(
            id uuid, code text, name text, set_type text,
            released_at date, card_count integer, icon_svg_uri text)
        ON CONFLICT (id) DO UPDATE SET
            code = excluded.code, name = excluded.name, set_type = excluded.set_type,
            released_at = excluded.released_at, card_count = excluded.card_count,
            icon_svg_uri = excluded.icon_svg_uri
        "#,
    )
    .bind(as_jsonb(rows)?)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// Upsert oracle rows. Caller must have deduped by `oracle_id` within the
/// batch — one multi-row statement may not touch the same row twice.
pub async fn cards(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[&CardRow],
) -> Result<u64, IngestError> {
    let done = sqlx::query(
        r#"
        INSERT INTO cards (oracle_id, name, mana_cost, cmc, type_line, oracle_text,
                           colors, color_identity, color_indicator, keywords,
                           power, toughness, loyalty, produced_mana, layout,
                           legalities, card_faces, all_parts, reserved, game_changer,
                           edhrec_rank, ingest_hash)
        SELECT t.oracle_id, t.name, t.mana_cost, t.cmc, t.type_line, t.oracle_text,
               t.colors, t.color_identity, t.color_indicator, t.keywords,
               t.power, t.toughness, t.loyalty, t.produced_mana, t.layout,
               t.legalities, t.card_faces, t.all_parts, t.reserved, t.game_changer,
               t.edhrec_rank, t.ingest_hash
        FROM jsonb_to_recordset($1) AS t(
            oracle_id uuid, name text, mana_cost text, cmc numeric, type_line text,
            oracle_text text, colors text[], color_identity text[],
            color_indicator text[], keywords text[], power text, toughness text,
            loyalty text, produced_mana text[], layout text, legalities jsonb,
            card_faces jsonb, all_parts jsonb, reserved boolean, game_changer boolean,
            edhrec_rank integer, ingest_hash bigint)
        ON CONFLICT (oracle_id) DO UPDATE SET
            name = excluded.name, mana_cost = excluded.mana_cost, cmc = excluded.cmc,
            type_line = excluded.type_line, oracle_text = excluded.oracle_text,
            colors = excluded.colors, color_identity = excluded.color_identity,
            color_indicator = excluded.color_indicator, keywords = excluded.keywords,
            power = excluded.power, toughness = excluded.toughness,
            loyalty = excluded.loyalty, produced_mana = excluded.produced_mana,
            layout = excluded.layout, legalities = excluded.legalities,
            card_faces = excluded.card_faces, all_parts = excluded.all_parts,
            reserved = excluded.reserved, game_changer = excluded.game_changer,
            edhrec_rank = excluded.edhrec_rank, ingest_hash = excluded.ingest_hash
        WHERE cards.ingest_hash IS DISTINCT FROM excluded.ingest_hash
        "#,
    )
    .bind(as_jsonb(rows)?)
    .execute(&mut **tx)
    .await?;
    Ok(done.rows_affected())
}

/// Upsert printing rows (unique by `id` within any one batch by construction —
/// the bulk file carries each printing once and the closure pass re-checks).
pub async fn printings(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[&PrintingRow],
) -> Result<u64, IngestError> {
    let done = sqlx::query(
        r#"
        INSERT INTO printings (id, oracle_id, set_id, collector_number, rarity,
                               finishes, lang, frame, frame_effects, border_color,
                               full_art, textless, promo, promo_types, flavor_name,
                               artist, flavor_text, watermark, security_stamp,
                               games, digital, released_at, image_uris, faces,
                               ingest_hash)
        SELECT t.id, t.oracle_id, t.set_id, t.collector_number, t.rarity,
               t.finishes, t.lang, t.frame, t.frame_effects, t.border_color,
               t.full_art, t.textless, t.promo, t.promo_types, t.flavor_name,
               t.artist, t.flavor_text, t.watermark, t.security_stamp,
               t.games, t.digital, t.released_at, t.image_uris, t.faces,
               t.ingest_hash
        FROM jsonb_to_recordset($1) AS t(
            id uuid, oracle_id uuid, set_id uuid, collector_number text, rarity text,
            finishes card_finish[], lang text, frame text, frame_effects text[],
            border_color text, full_art boolean, textless boolean, promo boolean,
            promo_types text[], flavor_name text, artist text, flavor_text text,
            watermark text, security_stamp text, games text[], digital boolean,
            released_at date, image_uris jsonb, faces jsonb, ingest_hash bigint)
        ON CONFLICT (id) DO UPDATE SET
            oracle_id = excluded.oracle_id, set_id = excluded.set_id,
            collector_number = excluded.collector_number, rarity = excluded.rarity,
            finishes = excluded.finishes, lang = excluded.lang,
            frame = excluded.frame, frame_effects = excluded.frame_effects,
            border_color = excluded.border_color, full_art = excluded.full_art,
            textless = excluded.textless, promo = excluded.promo,
            promo_types = excluded.promo_types, flavor_name = excluded.flavor_name,
            artist = excluded.artist, flavor_text = excluded.flavor_text,
            watermark = excluded.watermark, security_stamp = excluded.security_stamp,
            games = excluded.games, digital = excluded.digital,
            released_at = excluded.released_at, image_uris = excluded.image_uris,
            faces = excluded.faces, ingest_hash = excluded.ingest_hash
        WHERE printings.ingest_hash IS DISTINCT FROM excluded.ingest_hash
        "#,
    )
    .bind(as_jsonb(rows)?)
    .execute(&mut **tx)
    .await?;
    Ok(done.rows_affected())
}

/// Upsert price snapshots — unconditional (no hash gate); Scryfall's decimal
/// strings cast text → numeric server-side, exactly.
pub async fn prices(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[&PriceRow],
) -> Result<u64, IngestError> {
    let done = sqlx::query(
        r#"
        INSERT INTO prices (printing_id, usd, usd_foil, usd_etched, eur, eur_foil, tix)
        SELECT t.printing_id, t.usd, t.usd_foil, t.usd_etched, t.eur, t.eur_foil, t.tix
        FROM jsonb_to_recordset($1) AS t(
            printing_id uuid, usd numeric, usd_foil numeric, usd_etched numeric,
            eur numeric, eur_foil numeric, tix numeric)
        ON CONFLICT (printing_id) DO UPDATE SET
            usd = excluded.usd, usd_foil = excluded.usd_foil,
            usd_etched = excluded.usd_etched, eur = excluded.eur,
            eur_foil = excluded.eur_foil, tix = excluded.tix,
            updated_at = now()
        "#,
    )
    .bind(as_jsonb(rows)?)
    .execute(&mut **tx)
    .await?;
    Ok(done.rows_affected())
}

/// One rulings bulk-file entry (the fields we store).
#[derive(Debug, Serialize, serde::Deserialize)]
pub struct RulingRow {
    pub oracle_id: uuid::Uuid,
    pub source: Option<String>,
    pub published_at: Option<String>,
    pub comment: String,
}

/// Insert one batch of rulings (no upsert — the caller swaps the whole table
/// inside a single transaction: DELETE all, then insert batches).
pub async fn rulings(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[RulingRow],
) -> Result<u64, IngestError> {
    let done = sqlx::query(
        r#"
        INSERT INTO rulings (oracle_id, published_at, source, comment)
        SELECT t.oracle_id, t.published_at, t.source, t.comment
        FROM jsonb_to_recordset($1) AS t(
            oracle_id uuid, published_at date, source text, comment text)
        "#,
    )
    .bind(as_jsonb(rows)?)
    .execute(&mut **tx)
    .await?;
    Ok(done.rows_affected())
}

// --- ingestion_runs bookkeeping ---------------------------------------------

pub async fn run_start(
    pool: &PgPool,
    kind: &str,
    source_updated_at: &str,
) -> Result<i64, IngestError> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ingestion_runs (kind, source_updated_at) VALUES ($1, $2::timestamptz) RETURNING id",
    )
    .bind(kind)
    .bind(source_updated_at)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn run_succeed(
    pool: &PgPool,
    id: i64,
    stats: &serde_json::Value,
) -> Result<(), IngestError> {
    sqlx::query(
        "UPDATE ingestion_runs SET finished_at = now(), status = 'succeeded', stats = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(stats)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn run_fail(pool: &PgPool, id: i64, error: &str) -> Result<(), IngestError> {
    sqlx::query(
        "UPDATE ingestion_runs SET finished_at = now(), status = 'failed', error = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// The bulk-mode gate: has a run of this kind already succeeded against this
/// exact bulk snapshot?
pub async fn already_ingested(
    pool: &PgPool,
    kind: &str,
    source_updated_at: &str,
) -> Result<bool, IngestError> {
    let hit: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM ingestion_runs
            WHERE kind = $1 AND status = 'succeeded'
              AND source_updated_at = $2::timestamptz)
        "#,
    )
    .bind(kind)
    .bind(source_updated_at)
    .fetch_one(pool)
    .await?;
    Ok(hit)
}
