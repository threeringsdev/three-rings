# Catalog ingestion

**Status:** draft
**Depends on:** [data-model](data-model.md)

## Problem

The card catalog (~100K printings) must be populated and kept current as new sets release and prices update, without manual work.

## Scope

In: a first POC load of a **representative card subset**, then the full bulk
load, scheduled incremental updates, and price refresh. Out: user-facing features.

## Design

**Phased delivery (maintainer decision 2026-07-14).**
[collection-api](collection-api.md) settled catalog search as **real SQL against
our ingested catalog**, so ingestion is now on the Phase 4 critical path — but the
*whole* ~100K catalog isn't needed up front. Deliver in three stages, and the
fleshed-out spec must specify each:

1. **POC subset** — ingest a small but *representative* sample (multi-face
   layouts, tokens/checklist, a spread of sets / colors / types / rarities) so
   catalog search runs end-to-end against real rows.
2. **Full bulk load** — the complete Scryfall bulk import (~100K printings).
3. **Update flow** — scheduled incremental card/set updates plus the
   higher-cadence price refresh; idempotent and resumable.

To be worked out (starting considerations for the stages above):

- Source: Scryfall bulk data files (daily) — check current rate-limit and attribution requirements.
- A scheduled Rust job (or cron-triggered binary) that downloads bulk data, diffs against current catalog, and upserts.
- Upserts should be idempotent and resumable; a failed run must not leave the catalog half-updated (transaction batching strategy needed for 100K rows on serverless Postgres).
- Price updates are the high-frequency path; decide cadence (daily?) and whether they run separately from card/set ingestion.
- Track ingestion runs in a table for observability.

**Scope note (from data-model's 2026-07-14 Scryfall shape review):**

- **Layouts in scope include tokens, checklist, emblem, and art-series cards**, not
  just normal/DFC printings — a collection app tracks them, and they are the resolve
  targets for `cards.all_parts`. They carry `rarity`/`collector_number`/`oracle_id`
  (verified), so data-model's `NOT NULL` columns hold; ingestion should assert this
  rather than assume it, and relax the column if a future layout violates it.
- **Populate the new jsonb columns:** `cards.card_faces` and `printings.faces` for
  multi-face layouts (per-face data is *omitted at top level* by Scryfall, so a naive
  top-level read loses cost/text/colors/P-T/images), `cards.legalities`,
  `cards.all_parts`, and the added scalar/array fields on both tables.
- **`reversible_card` has no top-level `oracle_id`** — read it from
  `card_faces[0].oracle_id`. Every other layout keeps a single top-level `oracle_id`.

## Open questions

- Where does the job run (Neon has no cron; needs a host — fly.io, GitHub Actions, etc.)?
- Image handling: link to Scryfall-hosted images vs. caching.
- Bulk-data source selection: the `default_cards` bulk file covers tokens/checklist;
  confirm it includes every layout we opted into (vs. `oracle_cards`, which collapses
  printings).
