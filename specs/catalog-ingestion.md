# Catalog ingestion

**Status:** draft
**Depends on:** [data-model](data-model.md)

## Problem

The card catalog (~116K printings, live count 2026-07-16) must be populated and
kept current as new sets release and prices update, without manual work.
[collection-api](collection-api.md) settled catalog search as **real SQL against
our ingested catalog**, and every collection row FKs `printings` — so nothing
user-facing works until these tables have rows. Ingestion is on the Phase 4
critical path.

## Scope

**In:** the ingestion pipeline (download → extract → upsert) and its three
delivery stages (POC representative subset, full bulk load, scheduled updates +
price refresh); the ingestion credential; run observability; Scryfall
policy compliance (headers, attribution, image rules); reconciliation of
Scryfall card migrations (merged/deleted ids).

**Out:** user-facing features; query→SQL translation
([catalog-search](catalog-search.md)); image caching/proxying (link-out decided
below; a future spec if it's ever needed); price history (data-model defers
it); non-English catalog expansion (the `all_cards` file is a later opt-in).

## Design

### Source facts (verified live, 2026-07-16)

Everything below was checked against `api.scryfall.com` and the current docs —
not remembered. Re-verify at implementation if months have passed.

- **Bulk data.** `GET /bulk-data` lists the files with daily-changing download
  URIs (`updated_at`, `size`, `download_uri` per file). Files are **gzipped
  JSONL** (`.jsonl.gz`, one card object per line — *not* one giant JSON array),
  so they stream through a gzip decoder line-by-line with flat memory. Bulk
  data regenerates every 12–24 h (observed ~09:00 UTC daily). Current inventory:

  | file | size (gz) | contents |
  |---|---|---|
  | `oracle_cards` | ~180 MB | one object per oracle id — collapses printings, **unusable** for our printing-grain catalog |
  | `default_cards` | ~560 MB | **every card object**, English (or printed language when the card exists in only one) |
  | `all_cards` | ~2.6 GB | every card object in every language |
  | `rulings` | ~26 MB | all rulings, keyed by `oracle_id` |
  | `unique_artwork`, `art_tags`, `oracle_tags` | — | not needed |

- **Rate limits.** The bulk-file origins (`*.scryfall.io`) have **no rate
  limits**. The API proper: 10 req/s general, 2 req/s on
  `/cards/search|named|random|collection`, 10 req/min on `/cards/manifest`.
  A 429 imposes a 30 s lockout; repeat offenders get banned — never ignore one.
- **Required headers.** Every `api.scryfall.com` request must send an accurate
  `User-Agent` (ours: `three-rings/<version>
  (+https://github.com/threeringsdev/three-rings)`) and an `Accept` header.
  Missing headers are refused (403 — observed directly).
- **Prices update once per day, period.** Bulk card objects embed the prices;
  docs: "fetching card data more frequently than 24 hours will not yield new
  prices", and bulk prices are "dangerously stale after 24 hours". Gameplay
  data changes much less often (weekly is fine if prices don't matter). **The
  draft's assumption of a higher-cadence price path was wrong** — there is no
  fresher free source, so price refresh *is* the daily bulk run.
- **Sets have no bulk file.** `GET /sets` returns all sets in **one
  unpaginated response** (1,045 today) carrying every column data-model's
  `sets` table stores (`id`, `code`, `name`, `set_type`, `released_at`,
  `card_count`, `icon_svg_uri`).
- **Card Migrations API.** `GET /migrations` (paginated) lists the rare cases
  where Scryfall discards a card id: `migration_strategy: merge` (repoint old
  id → new id) or `delete` (id gone, no replacement). This is the sanctioned
  way to reconcile downstream databases — relevant because `holdings` /
  `desires` / `moves` FK `printings.id`.
- **`/cards/manifest`** (new) pages through per-card
  `data_updated_at`/`image_updated_at` stamps — an alternative change-detection
  surface. At 10 req/min it's slower and more complex than diffing the free
  bulk file; noted as a future optimization, not used here.

### Source selection (resolves the draft's open question)

**`default_cards`** is the ingest source: it is the only file at printing
grain that doesn't multiply by language, and the docs state it contains *every
card object* — "every card type in every product … including double-faced
cards, planar cards, schemes, vanguards, tokens, and funny cards", which
covers the layouts data-model opted in (tokens, checklist, emblem,
art-series). The POC asserts that coverage empirically (below). `rulings`
rides its own bulk file; `sets` come from `/sets`. Note `default_cards` means
`printings.lang` is non-`'en'` only for foreign-only printings — expected.

### One pipeline, three delivery stages

There is **one** pipeline, built once at stage 1; the stages differ only in
filter and trigger:

```
/bulk-data metadata ──► gate on updated_at
/sets ────────────────► upsert sets (all 1,045, every run)
default_cards.jsonl.gz ► stream-decode line-by-line
                        ► optional subset filter (stage 1 only)
                        ► extract → batch → upsert cards, printings, prices
rulings.jsonl.gz ──────► swap rulings
/migrations ───────────► reconcile merged/deleted ids (stage 3)
ingestion_runs ────────► one row per run (observability)
```

1. **POC subset** — the full pipeline run with a checked-in filter, so
   catalog search gets real rows end-to-end without the 560 MB load.
2. **Full bulk load** — the same run, unfiltered.
3. **Update flow** — the same run, scheduled, plus migration reconciliation.

Building the real pipeline first and filtering it is deliberate: no throwaway
POC loader, and stages 2–3 become configuration rather than new code.

### Where the pipeline lives; credentials

- **`server --ingest`** subcommand, following the `server --migrate` precedent.
  The hosted server already ships an HTTP client (the auth proxy), so no new
  dependency class; and the delivery Docker image then already contains the
  ingester — a future scheduled job (stage 3) reuses the same image with a
  different command, no second Dockerfile. (A separate workspace bin crate is
  the fallback if the added deps — gzip decode, streaming JSON — bloat the
  server build measurably.)
- **A dedicated `catalog_ingest` role** (least privilege, mirroring the
  `app_runtime` pattern: created manually on both branches since roles are
  per-branch, grants carried in the migration): CRUD on the catalog tables +
  `ingestion_runs`, nothing else — no DDL, no user-table access, RLS-subject.
  `app_runtime` stays read-only on catalog (migrations `0004`/`0005`).
  Stage 1–2 runs happen manually from the dev container with the
  `catalog_ingest` URL in `.devcontainer/.env`, same discipline as
  [`scripts/migrate.sh`](../scripts/migrate.sh); at stage 3 the hosted server
  additionally holds it as a second Render env var (`INGEST_DATABASE_URL` or
  similar — naming at execution), since the server process is what executes
  scheduled runs (below). Still least-privilege: catalog CRUD only, no user
  tables. Connect via the **direct** endpoint (not the pooler), like
  migrations.

### Writing to Postgres (serverless-aware)

- **FK order.** Each run: upsert `sets` first, then per batch upsert `cards`
  before `printings` (printings FK cards + sets) with `prices` last. Every
  batch is one transaction of complete rows, so FKs hold at every commit
  boundary.
- **Batched multi-row upserts** (`INSERT … ON CONFLICT (id) DO UPDATE`,
  ~500–1000 rows/statement). `COPY` into a staging table + merge is the
  escape hatch if the full load proves slow — decide from POC/full-load
  timings, not up front.
- **In-batch dedupe hazard:** every card object carries its full oracle data,
  so one batch usually holds the same `oracle_id` several times — and
  multi-row `ON CONFLICT` errors on duplicate keys within one statement
  ("cannot affect row a second time"). Dedupe `cards` rows per batch in
  memory before writing.
- **Change-gating via content hash.** `cards.ingest_hash` and
  `printings.ingest_hash` store a stable 64-bit hash of the *extracted* column
  tuple — **prices excluded**. Upserts skip unchanged rows in-statement
  (`DO UPDATE … WHERE t.ingest_hash IS DISTINCT FROM excluded.ingest_hash`),
  so the daily run writes only real changes instead of rewriting 116K rows
  (Neon bills written bytes; autovacuum churn). `prices` upserts
  unconditionally — its own table exists precisely to absorb the daily churn
  (data-model's split rationale).
- **Rulings = atomic swap.** No inbound FKs, identity PK, low churn: delete +
  reinsert all (~26 MB) in one transaction, gated on the rulings file's
  `updated_at`.
- **Idempotent re-run = resumability.** A run that dies mid-way leaves a
  valid, FK-consistent catalog (some rows new, some old — the same skew
  mid-run visibility already has). Recovery is *re-run*, which the hash
  gating makes cheap. No checkpoint bookkeeping.
- **`ingestion_runs`** (catalog group; `app_runtime` read-only by `0005`
  default):

  ```sql
  CREATE TABLE ingestion_runs (
      id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      kind              text NOT NULL,          -- 'poc' | 'full' | 'update'
      source_updated_at timestamptz,            -- the bulk file's updated_at
      started_at        timestamptz NOT NULL DEFAULT now(),
      finished_at       timestamptz,
      status            text NOT NULL DEFAULT 'running',  -- running|succeeded|failed
      stats             jsonb,                  -- per-table upserted/skipped/deleted
      error             text
  );
  ```

- **Gate on `updated_at`.** Each run first reads `/bulk-data` metadata and
  compares against the last successful run's `source_updated_at`; unchanged →
  record a no-op run and exit. Failures exit nonzero with `status='failed'`.

**Schema additions this spec owns** (one migration): `ingestion_runs`, the two
`ingest_hash` columns, and the `catalog_ingest` grants.

### Extraction rules

Data-model's 2026-07-14 Scryfall shape review governs; the load-bearing points:

- Populate the jsonb columns: `cards.card_faces` + `printings.faces` for
  multi-face layouts (per-face data is omitted at top level — a naive
  top-level read loses cost/text/colors/P-T/images), plus `cards.legalities`
  and `cards.all_parts`.
- `reversible_card` has no top-level `oracle_id` — read
  `card_faces[0].oracle_id`. Every other layout: assert exactly one top-level
  `oracle_id`.
- Tokens, checklist, emblem, and art-series layouts are **in scope**. They
  carry `rarity`/`collector_number`/`oracle_id`; ingestion **asserts** the
  schema's `NOT NULL` expectations per line and fails loudly (then we relax
  the column deliberately) rather than silently skipping or nulling.
- Store only what data-model stores; drop the rest of the object (purchase
  URIs, cross-reference ids, …).

### Stage 1 — POC subset

A checked-in, deterministic filter (set codes + explicit card ids), so the POC
is reproducible:

- **A few full sets** for realistic search spread — e.g. one modern large
  expansion *plus its token set* (token printings live under a separate
  `t`-prefixed set code), one commander product, one vintage set for
  reserved-list/old-frame rows. Exact codes chosen at execution.
- **A layout menagerie** by explicit id — at least one of each: `transform`,
  `modal_dfc`, `split`, `adventure`, `flip`, `meld` (all three parts),
  `battle`, `reversible_card`, `token`, `double_faced_token`, `emblem`,
  `art_series`, checklist, a Godzilla/`flavor_name` variant, a showcase
  `frame_effects` printing, a fractional-cmc un-card, a digital-only printing.
- **Two-pass relation closure:** after the filtered stream, resolve the
  included cards' `all_parts` printing ids and ingest those too, so no
  dangling relation links inside the subset.

**Acceptance:** every opted-in layout present (this empirically settles
whether `default_cards` carries emblem/art-series/checklist — the docs say
yes); multi-face rows have `card_faces`/`faces` populated with top-level
fields NULL as designed; `all_parts` links resolve within the subset; NOT NULL
assertions never fired; an `ingestion_runs` row records the counts; trgm name
search returns sensible rows over the subset.

### Stage 2 — full load

The unfiltered run, dev branch first, then prod. **Acceptance:** printing
count within tolerance of the live total (~116K; compare against
`/bulk-data`-reported counts or spot-check `sets.card_count` per set); every
layout present; an immediate re-run is a fast near-no-op (hash gating
verified); wall-clock + row counts recorded in the spec's Findings.

### Stage 3 — update flow

- **One daily job** — card/set data and prices together; the once-per-day
  upstream price cadence (verified above) means no separate price pipeline
  exists to build. During spoiler season, new cards simply appear in the
  daily file. If the daily cadence ever matters less, weekly loses nothing
  but price freshness.
- **Migration reconciliation, catalog-side only:** re-fetch `/migrations`
  each run (idempotent by construction — apply-if-applicable, no high-water
  mark): `merge`/`delete` remove the old printing row **iff nothing
  references it**; a user-referenced id is kept and logged in `stats` for the
  runbook. Automatic repointing of `holdings`/`desires`/`moves` is
  **deliberately out**: those tables are RLS-forced, and Neon can't mint
  `BYPASSRLS` roles, so an automated job can't touch them safely —
  the documented owner-side runbook (temporary `NO FORCE`, repoint old→new
  summing quantities on unique-key collision, re-`FORCE`) handles the
  realistically-rare case manually.
- **No prune-by-absence.** Scryfall's database is additive; removals arrive
  only via the migrations API. A printing absent from the bulk file without a
  migration is logged, never deleted.
- **Scheduling = `pg_cron` (maintainer decision, 2026-07-16), via a
  due-marker.** Verified against the live project the same day: `pg_cron` 1.6
  is available (enable = set `cron.database_name` via the Neon API + compute
  restart, then `CREATE EXTENSION`), but **neither `http` nor `pg_net`
  exists**, so a pg_cron job cannot call a webhook — its tick can only write
  SQL state. And the ingester genuinely can't live in-database (a 560 MB
  gzipped-JSONL stream is not a plpgsql job). So the shape is:
  - **pg_cron = the clock.** A guarded job runs *hourly* (not once daily):
    insert a `kind='update', status='due'` row into `ingestion_runs` iff no
    run succeeded in the last ~20 h and none is already due/running. Hourly
    guarded ticks matter because of the next caveat.
  - **The hosted server = the worker.** While awake it polls cheaply (one
    indexed SELECT on a several-minute interval, plus a check at startup) for
    a due row, claims it (optimistic `UPDATE … WHERE status='due'`), and runs
    the ingestion in a background task under the `catalog_ingest` credential.
    No exposed trigger endpoint, no shared secret — the marker table is the
    whole interface.
  - **Verified caveat — ticks fire only while the Neon compute is active.**
    Neon's docs are explicit: pg_cron jobs do not run (and are not caught up)
    while an autosuspended compute is asleep. The Render free web service
    spins down without traffic too. Net effect on today's free-tier setup:
    **catalog freshness is proportional to app usage** — the first usage
    window after staleness crosses an hourly tick, writes the marker, and the
    awake server picks it up. Acceptable for a single-user app; an always-on
    compute (paid) later makes ticks exact with no design change.
  - The standing rule *"no DB creds in GitHub — no CI job talks to Neon"*
    stands; GitHub Actions was considered and rejected. Render cron jobs
    (paid) remain a fallback if pg_cron's awake-time semantics ever chafe.
  - Execution notes: verify streaming memory fits Render's 512 MB free
    instance; `cron.schedule_in_database()` is unsupported on Neon, so the
    cron job must be created in the app database itself.

### Compliance obligations (Scryfall / WotC Fan Content Policy)

Binding on us (current policy text, 2026-07-16):

- Accurate `User-Agent` + `Accept` on every API request (already in the
  pipeline design).
- **No paywalling** Scryfall-derived data; anonymous/free access to card data
  must remain possible. (Our catalog endpoints are already anonymous-safe.)
- No implying Scryfall endorsement; the app must add value beyond
  re-serving their data (a collection tracker clearly does).
- **Image rules** for the app's UI, since we hotlink `*.scryfall.io` (no rate
  limits there; link-out is this spec's image decision — caching/proxying
  ~100K+ images is real infra with no v1 payoff): never crop/cover the
  copyright or artist line, no distortion/recoloring/watermarking; `art_crop`
  usage requires artist + copyright attribution in the same interface.
- A visible credit ("card data and images © Scryfall / Wizards of the Coast;
  not endorsed") belongs in the app — rides the card-page/UI work, noted here
  so it isn't lost.

## Decisions (this review)

- **Source = `default_cards` + `rulings` bulk files + `/sets` API** —
  `oracle_cards` collapses printings; `all_cards` multiplies by language for
  no v1 benefit; sets have no bulk file but one unpaginated API call.
- **Price refresh = the daily bulk run** — upstream prices change once/day
  (verified), so the draft's separate higher-cadence price path is deleted
  from the design rather than deferred.
- **One pipeline, filtered for the POC** — no throwaway loader; stages 2–3
  are configuration, not new code.
- **Images link out** to Scryfall-hosted URIs (rate-limit-free origin);
  caching is a future spec if ever needed.
- **Change-gating by content hash (prices excluded)** so the daily run writes
  deltas, not the whole catalog — Neon written-bytes + autovacuum aware.
- **`server --ingest` + `catalog_ingest` role** — one Docker image serves web
  and (later) the scheduled job; least-privilege credential, `app_runtime`
  stays catalog-read-only.
- **Migrations reconciled catalog-side only; no prune-by-absence** — user-row
  repointing is a documented manual runbook (RLS-forced tables, no BYPASSRLS
  on Neon).
- **Scheduling = pg_cron due-marker + server-side worker** (maintainer,
  2026-07-16) — pg_cron writes a guarded hourly `due` row; the hosted server
  claims and runs it while awake. No `http`/`pg_net` on Neon (verified live),
  so no webhook path exists; freshness is usage-proportional on free-tier
  autosuspend, exact on a future always-on compute. GitHub Actions rejected
  (standing secrets rule); Render cron noted as fallback.

## Open questions

- Does `default_cards` really include emblem/art-series/checklist layouts?
  Docs say "every card object"; asserted empirically by the POC. *(resolved
  during execution — stage 1 acceptance)*
- `server --ingest` subcommand vs. separate workspace bin, and exact env-var
  naming for the `catalog_ingest` URL. *(resolved during execution — stage 1)*
- Batched multi-row upserts vs. `COPY` + staging merge for the full load.
  *(resolved during execution — stage 2, from measured timings)*
- Exact POC set codes + menagerie card ids. *(resolved during execution —
  stage 1, checked into the filter file)*
- pg_cron enablement mechanics (the `cron.database_name` API call + restart,
  on both branches), the exact guarded-tick SQL, and the server poll
  interval. *(resolved during execution — stage 3)*
