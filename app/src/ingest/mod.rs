//! Catalog ingestion — the Scryfall bulk pipeline (specs/catalog-ingestion.md).
//!
//! One pipeline, run modes selecting the coverage: `Poc` streams the same
//! `default_cards` bulk file through the checked-in subset filter (plus a
//! relation-closure second pass so `all_parts` links resolve inside the
//! subset); `Bulk` is the unfiltered bootstrap / rebuild / price true-up.
//! Invoked as `server --ingest <poc|bulk>` under the least-privilege
//! `catalog_ingest` role (`INGEST_DATABASE_URL`). The daily incremental path
//! (manifest diff + hydration) is stage 3 and lands with the full-load task.
//!
//! Consistency model: every batch is one transaction of complete, FK-ordered
//! rows (cards before printings before prices; sets upserted up front), so an
//! interrupted run leaves a valid catalog and recovery is re-run — the
//! in-statement hash gate makes that cheap (only real changes write).

pub mod extract;
pub mod filter;
pub mod scryfall;
pub mod upsert;

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use extract::{extract, Extracted, SourceCard};
use filter::PocFilter;
use scryfall::Client;

/// Rows per upsert transaction.
const BATCH: usize = 500;
/// Reuse a downloaded bulk file younger than this (dev convenience — the
/// upstream file only regenerates every 12–24 h anyway).
const REUSE_DOWNLOAD_SECS: u64 = 12 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// The checked-in representative subset (stage 1).
    Poc,
    /// The full bulk load (stage 2: bootstrap; thereafter rebuild/true-up).
    Bulk,
}

impl Mode {
    fn kind(self) -> &'static str {
        match self {
            Mode::Poc => "poc",
            Mode::Bulk => "bulk",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("{0}")]
    Env(&'static str),
    #[error("scryfall http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("extract: {0}")]
    Extract(#[from] extract::ExtractError),
    #[error("{0}")]
    Source(String),
}

/// Per-run counts, recorded on the `ingestion_runs` row. `cards`/`printings`
/// count rows actually written (inserted or hash-dirty updated) — unchanged
/// rows are skipped in-statement and never counted.
#[derive(Debug, Default, Serialize)]
pub struct Stats {
    pub sets: u64,
    pub scanned: u64,
    pub matched: u64,
    pub relation_pulled: u64,
    pub cards: u64,
    pub printings: u64,
    pub prices: u64,
    pub rulings: u64,
}

/// Run the bulk path end-to-end. The entry point behind `server --ingest`.
pub async fn run(mode: Mode) -> Result<Stats, IngestError> {
    let url = std::env::var("INGEST_DATABASE_URL").map_err(|_| {
        IngestError::Env(
            "INGEST_DATABASE_URL is not set — the catalog_ingest connection string \
             (see .devcontainer/.env.example)",
        )
    })?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await?;
    let client = Client::new()?;

    let bulk = client.bulk_file("default_cards").await?;
    // Gate bulk runs on the upstream snapshot; POC re-runs are always live
    // (the filter, not the snapshot, is what changes between them).
    if mode == Mode::Bulk && upsert::already_ingested(&pool, mode.kind(), &bulk.updated_at).await? {
        println!(
            "ingest: bulk snapshot {} already ingested — nothing to do",
            bulk.updated_at
        );
        return Ok(Stats::default());
    }

    let run_id = upsert::run_start(&pool, mode.kind(), &bulk.updated_at).await?;
    match execute(&pool, &client, mode, &bulk).await {
        Ok(stats) => {
            let json = serde_json::to_value(&stats)
                .map_err(|e| IngestError::Source(format!("stats serialization: {e}")))?;
            upsert::run_succeed(&pool, run_id, &json).await?;
            Ok(stats)
        }
        Err(e) => {
            // Best-effort failure stamp — the original error is what matters.
            let _ = upsert::run_fail(&pool, run_id, &e.to_string()).await;
            Err(e)
        }
    }
}

async fn execute(
    pool: &PgPool,
    client: &Client,
    mode: Mode,
    bulk: &scryfall::BulkFile,
) -> Result<Stats, IngestError> {
    let mut stats = Stats::default();

    // Sets first, every run — printings FK them and there is no sets bulk file.
    let sets = client.sets().await?;
    stats.sets = upsert::sets(pool, &sets).await?;
    println!("ingest: {} sets upserted", stats.sets);

    let path = download_cached(client, &bulk.download_uri, "default_cards").await?;
    let filter = match mode {
        Mode::Poc => Some(PocFilter::poc()?),
        Mode::Bulk => None,
    };

    // Pass 1: stream, filter, extract, upsert in batches. Track printing ids +
    // referenced all_parts ids for the closure pass, oracle ids for rulings,
    // and which oracles already had their card row admitted (first-seen wins).
    let mut batch = Batch::default();
    let mut ingested: HashSet<Uuid> = HashSet::new();
    let mut oracle_ids: HashSet<Uuid> = HashSet::new();
    let mut needed: HashSet<Uuid> = HashSet::new();
    let mut seen_oracles: HashSet<Uuid> = HashSet::new();

    let mut lines = scryfall::bulk_lines(&path).await?;
    while let Some(line) = lines.next_line().await? {
        let Some(line) = card_line(&line) else {
            continue;
        };
        stats.scanned += 1;
        let source = SourceCard::parse(line)?;
        if let Some(f) = &filter {
            if !f.matches(source.set.as_deref(), source.id) {
                continue;
            }
        }
        let e = extract(source)?;
        stats.matched += 1;
        collect_part_ids(&e, &mut needed);
        ingested.insert(e.printing.id);
        oracle_ids.insert(e.card.oracle_id);
        batch.admit(e, &mut seen_oracles);
        if batch.len() >= BATCH {
            flush(pool, &mut batch, &mut stats).await?;
        }
    }
    flush(pool, &mut batch, &mut stats).await?;
    println!(
        "ingest: pass 1 done — {} scanned, {} matched",
        stats.scanned, stats.matched
    );

    // Pass 2 (filtered runs): one-level all_parts closure so relation links
    // resolve inside the subset. Deliberately not transitive.
    if filter.is_some() {
        needed.retain(|id| !ingested.contains(id));
        if !needed.is_empty() {
            let mut lines = scryfall::bulk_lines(&path).await?;
            while let Some(line) = lines.next_line().await? {
                let Some(line) = card_line(&line) else {
                    continue;
                };
                let source = SourceCard::parse(line)?;
                if !needed.contains(&source.id) {
                    continue;
                }
                let e = extract(source)?;
                stats.relation_pulled += 1;
                ingested.insert(e.printing.id);
                oracle_ids.insert(e.card.oracle_id);
                batch.admit(e, &mut seen_oracles);
                if batch.len() >= BATCH {
                    flush(pool, &mut batch, &mut stats).await?;
                }
            }
            flush(pool, &mut batch, &mut stats).await?;
            println!(
                "ingest: pass 2 done — {} relation targets pulled",
                stats.relation_pulled
            );
        }
    }

    stats.rulings = swap_rulings(pool, client, &oracle_ids).await?;
    println!("ingest: {} rulings swapped in", stats.rulings);
    Ok(stats)
}

/// Normalize one bulk-file line to a bare card-object JSON string, or `None`
/// for structural lines. Scryfall's docs describe the bulk files as pure
/// JSONL, but the live `/bulk-data` download URIs still serve the legacy
/// one-object-per-line JSON *array* (`[` / `{…},` / `]`) — observed
/// 2026-07-16. Handle both.
fn card_line(line: &str) -> Option<&str> {
    let t = line.trim();
    let t = t.strip_suffix(',').unwrap_or(t);
    if t.is_empty() || t == "[" || t == "]" {
        None
    } else {
        Some(t)
    }
}

fn collect_part_ids(e: &Extracted, needed: &mut HashSet<Uuid>) {
    let Some(parts) = e.card.all_parts.as_ref().and_then(|p| p.as_array()) else {
        return;
    };
    for part in parts {
        if let Some(id) = part
            .get("id")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse().ok())
        {
            needed.insert(id);
        }
    }
}

/// One flush-worth of rows, FK-ordered at write time.
#[derive(Default)]
struct Batch {
    cards: Vec<extract::CardRow>,
    printings: Vec<extract::PrintingRow>,
    prices: Vec<extract::PriceRow>,
}

impl Batch {
    /// Admit one extracted card. The oracle row is written only for the FIRST
    /// printing seen this run: oracle-scoped content genuinely varies across a
    /// few printings of the same oracle (e.g. a reversible_card reprint of a
    /// normal card carries card_faces), so without one deterministic winner,
    /// later batches would flip the row and every re-run would rewrite it.
    /// A multi-row upsert also may not touch the same row twice, so this
    /// doubles as the in-batch dedupe.
    fn admit(&mut self, e: Extracted, seen_oracles: &mut HashSet<Uuid>) {
        if seen_oracles.insert(e.card.oracle_id) {
            self.cards.push(e.card);
        }
        self.printings.push(e.printing);
        self.prices.push(e.prices);
    }

    fn len(&self) -> usize {
        self.printings.len()
    }

    fn is_empty(&self) -> bool {
        self.printings.is_empty()
    }
}

/// One FK-ordered transaction per batch: cards, then printings, then prices.
async fn flush(pool: &PgPool, batch: &mut Batch, stats: &mut Stats) -> Result<(), IngestError> {
    if batch.is_empty() {
        return Ok(());
    }
    let cards: Vec<_> = batch.cards.iter().collect();
    let printings: Vec<_> = batch.printings.iter().collect();
    let prices: Vec<_> = batch.prices.iter().collect();

    let mut tx = pool.begin().await?;
    stats.cards += upsert::cards(&mut tx, &cards).await?;
    stats.printings += upsert::printings(&mut tx, &printings).await?;
    stats.prices += upsert::prices(&mut tx, &prices).await?;
    tx.commit().await?;
    *batch = Batch::default();
    Ok(())
}

/// Replace `rulings` with the current bulk file, filtered to oracle ids
/// present in the catalog (its FK target). Atomic: DELETE + inserts in one
/// transaction; no inbound FKs make the swap safe.
async fn swap_rulings(
    pool: &PgPool,
    client: &Client,
    oracle_ids: &HashSet<Uuid>,
) -> Result<u64, IngestError> {
    let bulk = client.bulk_file("rulings").await?;
    let path = download_cached(client, &bulk.download_uri, "rulings").await?;

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM rulings").execute(&mut *tx).await?;
    let mut total = 0u64;
    let mut batch: Vec<upsert::RulingRow> = Vec::new();
    let mut lines = scryfall::bulk_lines(&path).await?;
    while let Some(line) = lines.next_line().await? {
        let Some(line) = card_line(&line) else {
            continue;
        };
        let row: upsert::RulingRow = serde_json::from_str(line)
            .map_err(|e| IngestError::Source(format!("unparseable ruling: {e}")))?;
        if !oracle_ids.contains(&row.oracle_id) {
            continue;
        }
        batch.push(row);
        if batch.len() >= 2000 {
            total += upsert::rulings(&mut tx, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        total += upsert::rulings(&mut tx, &batch).await?;
    }
    tx.commit().await?;
    Ok(total)
}

/// Download a bulk file to the OS temp dir, reusing a recent complete download
/// (interrupted ones never count — `download` renames from `.part` last).
async fn download_cached(client: &Client, uri: &str, name: &str) -> Result<PathBuf, IngestError> {
    let path = std::env::temp_dir().join(format!("three-rings-{name}.jsonl.gz"));
    if let Ok(age) = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|m| m.elapsed().unwrap_or_default())
    {
        if age.as_secs() < REUSE_DOWNLOAD_SECS {
            println!(
                "ingest: reusing {} (downloaded {}m ago)",
                path.display(),
                age.as_secs() / 60
            );
            return Ok(path);
        }
    }
    println!("ingest: downloading {name} …");
    client.download(uri, &path).await?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::extract::extract_line;
    use super::{card_line, Batch};
    use std::collections::HashSet;

    #[test]
    fn first_seen_oracle_version_wins_within_a_run() {
        // Oracle-scoped content genuinely varies across a few printings of the
        // same oracle (e.g. a reversible_card reprint of a normal card carries
        // card_faces). Without one global winner per run, later batches flip
        // the row back and forth and every re-run rewrites it (observed: 54 of
        // 2,665 POC cards).
        let normal = include_str!("fixtures/normal.json");
        let first = extract_line(normal).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(normal).unwrap();
        v["id"] = "11111111-1111-1111-1111-111111111111".into();
        v["oracle_text"] = "a divergent per-printing oracle snapshot".into();
        let second = extract_line(&v.to_string()).unwrap();
        assert_eq!(first.card.oracle_id, second.card.oracle_id);
        assert_ne!(first.card.ingest_hash, second.card.ingest_hash);

        let mut seen = HashSet::new();
        let mut batch = Batch::default();
        let first_card = first.card.clone();
        batch.admit(first, &mut seen);
        batch.admit(second, &mut seen);
        assert_eq!(batch.cards.len(), 1, "one card row per oracle per run");
        assert_eq!(batch.cards[0], first_card, "the first-seen version wins");
        assert_eq!(batch.printings.len(), 2, "every printing still lands");
        assert_eq!(batch.prices.len(), 2);
    }

    #[test]
    fn card_line_handles_both_bulk_layouts() {
        // legacy JSON-array layout (live /bulk-data URIs, observed 2026-07-16)
        assert_eq!(card_line("["), None);
        assert_eq!(card_line("]"), None);
        assert_eq!(
            card_line(r#"{"object":"card","id":"x"},"#),
            Some(r#"{"object":"card","id":"x"}"#)
        );
        // final array entry has no trailing comma
        assert_eq!(
            card_line(r#"{"object":"card","id":"y"}"#),
            Some(r#"{"object":"card","id":"y"}"#)
        );
        // pure JSONL (the documented format)
        assert_eq!(
            card_line(r#"{"object":"card"}"#),
            Some(r#"{"object":"card"}"#)
        );
        assert_eq!(card_line(""), None);
        assert_eq!(card_line("  "), None);
    }
}
