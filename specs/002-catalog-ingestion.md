# 002: Catalog ingestion

**Status:** draft
**Depends on:** 001

## Problem

The card catalog (~100K printings) must be populated and kept current as new sets release and prices update, without manual work.

## Scope

In: initial bulk load, scheduled incremental updates, price refresh. Out: user-facing features.

## Design

To be worked out. Starting considerations:

- Source: Scryfall bulk data files (daily) — check current rate-limit and attribution requirements.
- A scheduled Rust job (or cron-triggered binary) that downloads bulk data, diffs against current catalog, and upserts.
- Upserts should be idempotent and resumable; a failed run must not leave the catalog half-updated (transaction batching strategy needed for 100K rows on serverless Postgres).
- Price updates are the high-frequency path; decide cadence (daily?) and whether they run separately from card/set ingestion.
- Track ingestion runs in a table for observability.

## Open questions

- Where does the job run (Neon has no cron; needs a host — fly.io, GitHub Actions, etc.)?
- Image handling: link to Scryfall-hosted images vs. caching.

## Tasks

- [ ] Confirm Scryfall bulk data terms and format
- [ ] Prototype bulk load into Neon; measure duration and CU cost
- [ ] Design diff/upsert strategy
