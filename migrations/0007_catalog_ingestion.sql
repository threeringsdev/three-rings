-- Catalog-ingestion schema additions (specs/catalog-ingestion.md → Writing to
-- Postgres): the change-gating hashes, the incremental path's manifest sync
-- state, the run-observability table, and the least-privilege ingestion role's
-- grants.
--
-- Grants at the foot assume the `catalog_ingest` role already exists
-- (provisioned out-of-band on both Neon branches like `app_runtime`, 2026-07-16
-- — it carries a login password we deliberately keep out of migrations). A
-- missing role fails the GRANT loudly, which correctly signals the ops
-- prerequisite. `app_runtime` needs no grants here: catalog tables are
-- read-only to it by 0005's default privilege.

-- Stable 64-bit hash of the extracted column tuple (prices excluded), computed
-- by the pipeline. Upserts skip rows whose hash is unchanged, so bulk true-ups
-- write only deltas (Neon written-bytes + autovacuum aware).
ALTER TABLE cards     ADD COLUMN ingest_hash bigint;
ALTER TABLE printings ADD COLUMN ingest_hash bigint;

-- Manifest sync state for the stage-3 incremental path: the /cards/manifest
-- timestamps as of the last ingest of this printing. NULL after bulk loads
-- (bulk objects don't carry them); the first manifest sweep baselines them.
ALTER TABLE printings ADD COLUMN manifest_data_updated_at  timestamptz;
ALTER TABLE printings ADD COLUMN manifest_image_updated_at timestamptz;

-- One row per pipeline run (either path), for observability and the bulk-mode
-- snapshot gate. app_runtime reads it via 0005's default privilege.
CREATE TABLE ingestion_runs (
    id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind              text NOT NULL,           -- 'poc' | 'bulk' | 'update'
    source_updated_at timestamptz,             -- the bulk file's updated_at (bulk path)
    started_at        timestamptz NOT NULL DEFAULT now(),
    finished_at       timestamptz,
    status            text NOT NULL DEFAULT 'running',  -- running | succeeded | failed
    stats             jsonb,                   -- per-table written/skipped counts
    error             text
);

-- catalog_ingest: CRUD on the catalog group + its run table, nothing else —
-- no DDL, no user tables (holdings/desires/moves/collections stay invisible).
GRANT USAGE ON SCHEMA public TO catalog_ingest;
GRANT SELECT, INSERT, UPDATE, DELETE
    ON sets, cards, printings, prices, rulings, ingestion_runs
    TO catalog_ingest;
-- identity-column sequences behind rulings.id / ingestion_runs.id
GRANT USAGE, SELECT ON SEQUENCE rulings_id_seq, ingestion_runs_id_seq TO catalog_ingest;
