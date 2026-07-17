# Catalog ingestion

**Status:** accepted
**Depends on:** [data-model](data-model.md)

## Problem

The card catalog (~116K printings, live count 2026-07-16) must be populated and
kept current as new sets release and prices update, without manual work.
[collection-api](collection-api.md) settled catalog search as **real SQL against
our ingested catalog**, and every collection row FKs `printings` — so nothing
user-facing works until these tables have rows. Ingestion is on the Phase 4
critical path.

## Scope

**In:** the ingestion machinery and its delivery stages — the **bulk path**
(POC representative subset, full bootstrap load, occasional rebuild/true-up)
and the **incremental path** (the scheduled daily update); the ingestion
credential; run observability; Scryfall policy compliance (headers,
attribution, image rules); reconciliation of Scryfall card migrations
(merged/deleted ids); the price-freshness policy.

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
  data regenerates every 12–24 h (observed ~09:00–09:30 UTC daily). Current
  inventory:

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
- **`/cards/manifest`** — Scryfall's sanctioned sync surface ("efficient
  information about each Card so that you can compare it with your downstream
  system or sync process"), and the docs steer daily syncs here explicitly:
  "Bulk data is only collected once every 12–24 hours. You can use the card
  API methods to retrieve fresh objects instead. You can also use the
  /cards/manifest method to check for anything that has changed."
  **15,000 entries per page** → the whole catalog is ~8 pages, well inside
  the 10 req/min limit (a full sweep paces out to ~1 minute). Entries carry
  `id`, `oracle_id`, `name`, `set_code`, `collector_number`, `lang`,
  `created_at`, `data_updated_at`, `image_updated_at`. `order=released`
  (default) or `order=imageupdated`, both descending. The **no-`lang`**
  listing (115,955 entries today) matches the `default_cards` universe;
  `lang=en` (113,315) drops foreign-only printings — sweep without `lang`.
  - **Observation (2026-07-16):** across 30,000 sampled entries (pages 1
    and 4), `data_updated_at`, `created_at`, and `oracle_id` were **all
    NULL**; only `image_updated_at` carried values. Whether that's pending
    population on a new endpoint or simply that cards rarely change once
    they exist (the maintainer's read — NULL as the steady state), the
    handling is the same: the manifest reliably signals *new printings*
    (unknown ids), *image updates*, and *disappearances* (missing ids); the
    diff reads `data_updated_at` whenever present, and the bulk true-up
    covers any data-only drift it doesn't signal.
- **`POST /cards/collection`** hydrates up to **75 cards per request** by
  `id` (2 req/s), returning full card objects (prices included); unresolved
  ids come back in a `not_found` array. Typical daily change volumes (tens
  to hundreds) hydrate in seconds; even a 15K mass image-rescan day is ~200
  requests ≈ 2 minutes.
- **Prices update upstream once per day, period** — bulk card objects embed
  them; docs: "fetching card data more frequently than 24 hours will not
  yield new prices". There is no fresher free source. Per the maintainer
  (2026-07-16), **frequent price updates are a non-goal for this app** —
  policy below.
- **Sets have no bulk file.** `GET /sets` returns all sets in **one
  unpaginated response** (1,045 today) carrying every column data-model's
  `sets` table stores (`id`, `code`, `name`, `set_type`, `released_at`,
  `card_count`, `icon_svg_uri`).
- **Card Migrations API.** `GET /migrations` (paginated) lists the rare cases
  where Scryfall discards a card id: `migration_strategy: merge` (repoint old
  id → new id) or `delete` (id gone, no replacement). This is the sanctioned
  way to reconcile downstream databases — relevant because `holdings` /
  `desires` / `moves` FK `printings.id`.

### Source selection (resolves the draft's open question)

**`default_cards`** is the bulk source: it is the only file at printing grain
that doesn't multiply by language, and the docs state it contains *every card
object* — "every card type in every product … including double-faced cards,
planar cards, schemes, vanguards, tokens, and funny cards", which covers the
layouts data-model opted in (tokens, checklist, emblem, art-series). The POC
asserts that coverage empirically (below). `rulings` rides its own bulk file;
`sets` come from `/sets`; the daily delta comes from `/cards/manifest` +
`/cards/collection`. Note `default_cards` (and the matching no-`lang`
manifest) means `printings.lang` is non-`'en'` only for foreign-only
printings — expected.

### Two paths, one core

Two acquisition paths feed **one shared core** (extract → batch → upsert →
record). The maintainer's framing (2026-07-16): the bulk file is for
*bootstrapping and occasional rebuilds*, not the daily loop — reprocessing a
560 MB-and-growing file daily buys almost nothing because cards themselves
rarely change; what dailies are really for is *new cards*.

```
BULK path (bootstrap / rebuild / true-up; manual)
  /bulk-data metadata ──► gate on updated_at
  default_cards.jsonl.gz ► stream line-by-line ► optional POC filter ─┐
  rulings.jsonl.gz ──────► swap rulings                               │
                                                                      ▼
INCREMENTAL path (daily, scheduled)                             shared core:
  /cards/manifest sweep (~8 pages) ► diff vs catalog ─┐         extract →
  /cards/collection (75 ids/req) ◄ hydrate changed ───┴───────► batch →
  /migrations ► reconcile merged/deleted ids                    upsert →
                                                                ingestion_runs
  (/sets upserted at the start of every run, either path)
```

The **incremental diff**: sweep the manifest, then hydrate exactly the
entries that are (a) **unknown ids** — new printings, the daily payoff;
(b) known ids whose `data_updated_at`/`image_updated_at` advanced past our
stored values (data timestamps inert until Scryfall backfills them);
(c) ids in our catalog but **absent from the manifest** are *not* deleted —
they're logged and left to the migrations reconciliation. Hydrated objects
flow through the same extraction and (hash-gated) upserts as bulk lines, and
refresh their prices incidentally. `not_found` hydration responses are
logged. An optional refinement — `order=released` / `order=imageupdated`
early-exit paging instead of the full sweep — is noted for execution, but the
full ~8-page sweep is already ~1 minute and enables the membership check.

**Price policy (maintainer, 2026-07-16):** price freshness is not first-class.
Prices refresh (1) incidentally on every hydrated card, and (2) catalog-wide
whenever the bulk path runs — the occasional **true-up** (manual, cadence at
the maintainer's discretion) that also catches data-only drift the manifest
can't yet signal. Hash gating makes a true-up cheap on writes (prices land
unconditionally in their churn-absorbing table; unchanged catalog rows are
skipped); the cost is the download.

**Delivery stages** map onto the paths: stage 1 = bulk path with the POC
filter; stage 2 = bulk path unfiltered (bootstrap, and thereafter the
rebuild/true-up tool); stage 3 = the incremental path + its schedule +
migrations reconciliation.

### Where the pipeline lives; credentials

- **`server --ingest <bulk|update>`** subcommands, following the
  `server --migrate` precedent. The hosted server already ships an HTTP
  client (the auth proxy), so no new dependency class; and the delivery
  Docker image then already contains the ingester — the scheduled job (stage
  3) runs the same image with a different command, no second Dockerfile. (A
  separate workspace bin crate is the fallback if the added deps — gzip
  decode, streaming JSON — bloat the server build measurably.)
- **A dedicated `catalog_ingest` role** (least privilege, mirroring the
  `app_runtime` pattern: created manually on both branches since roles are
  per-branch, grants carried in the migration): CRUD on the catalog tables +
  `ingestion_runs`, nothing else — no DDL, no user-table access.
  `app_runtime` stays read-only on catalog (migrations `0004`/`0005`).
  Bulk runs happen manually from the dev container with the `catalog_ingest`
  URL in `.devcontainer/.env`, same discipline as
  [`scripts/migrate.sh`](../scripts/migrate.sh); at stage 3 the credential
  also lives in the **Render cron job's** env — and *only* there: the web
  service keeps exactly `app_runtime`, so the process serving traffic never
  holds catalog-write power. Connect via the **direct** endpoint (not the
  pooler), like migrations.

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
  so a bulk true-up writes only real changes instead of rewriting 116K rows
  (Neon bills written bytes; autovacuum churn). `prices` upserts
  unconditionally — its own table exists precisely to absorb price churn
  (data-model's split rationale).
- **Manifest sync state:** `printings.manifest_data_updated_at` and
  `printings.manifest_image_updated_at` (nullable `timestamptz`) store the
  manifest timestamps as of the last ingest, giving the incremental diff its
  per-row comparison. Bulk objects don't carry these, so a bulk run leaves
  them NULL and the next manifest sweep records them **without hydrating**
  rows that already exist (baseline mode — the bulk snapshot is equally
  fresh).
- **Rulings = atomic swap.** No inbound FKs, identity PK, low churn: delete +
  reinsert all (~26 MB) in one transaction, gated on the rulings file's
  `updated_at`. Bulk path only.
- **Idempotent re-run = resumability.** A run that dies mid-way leaves a
  valid, FK-consistent catalog (some rows new, some old — the same skew
  mid-run visibility already has). Recovery is *re-run*, which the hash
  gating and manifest diff make cheap. No checkpoint bookkeeping.
- **`ingestion_runs`** (catalog group; `app_runtime` read-only by `0005`
  default):

  ```sql
  CREATE TABLE ingestion_runs (
      id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      kind              text NOT NULL,          -- 'poc' | 'bulk' | 'update'
      source_updated_at timestamptz,            -- bulk file's updated_at (bulk runs)
      started_at        timestamptz NOT NULL DEFAULT now(),
      finished_at       timestamptz,
      status            text NOT NULL DEFAULT 'running',  -- running|succeeded|failed
      stats             jsonb,                  -- per-table upserted/skipped/deleted, hydrated, not_found, …
      error             text
  );
  ```

- **Gating.** Bulk runs first read `/bulk-data` metadata and compare against
  the last successful bulk run's `source_updated_at`; unchanged → record a
  no-op run and exit. Incremental runs always sweep (the sweep *is* the
  cheap check). Failures exit nonzero with `status='failed'`.

**Schema additions this spec owns** (one migration): `ingestion_runs`, the two
`ingest_hash` columns, the two `manifest_*_updated_at` columns, and the
`catalog_ingest` grants.

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

A checked-in, deterministic filter (set codes + explicit card ids) over the
bulk path, so the POC is reproducible:

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

### Stage 2 — full load (bootstrap; thereafter the rebuild/true-up tool)

The unfiltered bulk run, dev branch first, then prod. This same run is the
standing **rebuild/true-up**: re-run occasionally (manual, maintainer's
cadence) to refresh all prices and catch data-only drift the manifest can't
yet signal. **Acceptance:** printing count within tolerance of the live total
(~116K; spot-check `sets.card_count` per set); every layout present; an
immediate re-run is a fast near-no-op (hash gating verified); wall-clock +
row counts recorded in the spec's Findings.

### Stage 3 — daily incremental update, scheduled on Render cron

The incremental path (manifest sweep → targeted hydration → shared core),
plus migration reconciliation, on a real clock:

- **Scheduling = a Render cron job** (maintainer decision 2026-07-16,
  superseding the same-day pg_cron pick — see Decisions). Same repo, same
  root `Dockerfile` (Docker runtime, rebuilt on push like the web service),
  schedule daily shortly after Scryfall's ~09:00–09:30 UTC regeneration
  window (exact expression at execution), command `server --ingest update`.
  Render guarantees **at most one active run** per cron job, kills runs at
  12 h (ours take minutes), and provides a dashboard "Trigger Run" button
  for ad-hoc runs. Billing is prorated per-second with a **$1/month minimum
  per cron service** — effectively ~$1/month at our runtimes. Unlike
  pg_cron, the clock is independent of app/DB wake state: the job spins up
  its own instance and Neon wakes on connection.
- **Migration reconciliation, catalog-side only:** re-fetch `/migrations`
  each run (idempotent by construction — apply-if-applicable, no high-water
  mark): `merge`/`delete` remove the old printing row **iff nothing
  references it**; a user-referenced id is kept and logged in `stats` for the
  runbook. Automatic repointing of `holdings`/`desires`/`moves` is
  **deliberately out**: those tables are RLS-forced, and Neon can't mint
  `BYPASSRLS` roles, so an automated job can't touch them safely — the
  documented owner-side runbook (temporary `NO FORCE`, repoint old→new
  summing quantities on unique-key collision, re-`FORCE`) handles the
  realistically-rare case manually.
- **No prune-by-absence.** Scryfall's database is additive; removals arrive
  only via the migrations API. A printing absent from the manifest without a
  migration is logged, never deleted.
- **Acceptance:** a scheduled run completes in single-digit minutes; only
  changed/new cards are hydrated (verified via `stats`); a new set's cards
  appear the day after release; a no-change day records a near-no-op run;
  the web service's env is untouched (no ingest credential there).

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

## Findings (stage 1 — POC subset, 2026-07-16)

The bulk path shipped as `server --ingest <poc|bulk>` (app crate,
`app/src/ingest/`, `hosted`-gated like `db`) + [`scripts/ingest.sh`](../scripts/ingest.sh)
(migrate.sh discipline); migration `0007` (hashes, manifest columns,
`ingestion_runs`, grants) applied to **dev** — prod is additive-safe and rides
the next prod touch. The `catalog_ingest` role exists on both branches
(passwords only in `.devcontainer/.env`; `INGEST_DATABASE_URL` /
`PROD_INGEST_DATABASE_URL`, documented in `.env.example`).

**POC numbers (Neon dev):** 1,045 sets; 115,955 lines scanned; **1,333
matched** (mh3 / tmh3 / amh3 / m3c / lea + the 15-card menagerie); **1,643
relation targets** pulled by the closure pass; **2,976 printings / 2,665
cards / 9,654 rulings** written. Streaming pass over the 531 MB file is ~1–2
min; whole run a few minutes, memory flat.

**Acceptance — all pass:** every opted-in layout present (token 73 — the
checklist card among them, art_series 54, transform 32, modal_dfc 21, emblem
7, split 7, meld 3 — the full Bruna/Gisela/Brisela triple, reversible_card 3,
double_faced_token 2, flip, mutate; the full sets brought bonus layouts —
saga, planar, scheme, class, case, prototype, prepare — extracted without any
NOT NULL assertion firing); zero multi-face rows missing `card_faces`;
transform rows have NULL top-level cost/text with populated faces; the
reversible printing's `oracle_id` came from `card_faces[0]`; **zero unresolved
`all_parts` links** among subset cards; trgm search finds "Lightning Bolt"
from "lightnig bolt"; run rows recorded (including an honest `failed` row from
the gzip surprise below). **Idempotency verified live:** an immediate re-run
writes **0 cards / 0 printings** (hash gate); prices (2,976) and rulings
(9,654) rewrite by design.

**Source-truth surprises (all handled in code):**

- **The live `/bulk-data` download URIs still serve the legacy one-object-
  per-line JSON *array*** (`[` / `{…},` / `]`) — not the documented pure
  JSONL — and Scryfall's CDN serves the **identity (uncompressed) body** to
  clients that don't advertise gzip. The pipeline sends
  `Accept-Encoding: gzip`, sniffs the gzip magic bytes on disk, and
  normalizes both line layouts, so either representation works.
- **Oracle-scoped content varies across printings of one oracle** (e.g.
  Zndrsplt is a `normal` printing in one set and a `reversible_card` in SLD —
  different `card_faces`, different combined name). One deterministic winner
  per run (**first-seen**) is required, or re-runs rewrite those rows forever
  (observed: 54 of 2,665 before the fix; 0 after).
- **Battles carry layout `transform`** now (Invasion of Zendikar), and
  **checklist cards are layout `token`** named "… Checklist" — neither is a
  distinct layout; the menagerie covers both under their real layouts.
- `mana_cost` can be `""` with `oracle_text` NULL on tokens; split cards keep
  a shared top-level image with **per-face artists**; transform keeps
  top-level `cmc`/`artist` while omitting cost/text/colors.

**Execution decisions:** batches are one jsonb bind through
`jsonb_to_recordset` (server-side typing: uuid/numeric/date/enum-array/jsonb
— no extra sqlx type features); prices pass through as Scryfall's decimal
strings, cast text → numeric exactly; the POC filter is compiled in
(`include_str!`); downloads cache 12 h in the OS temp dir with a `.part`
rename guarding partial files; the snapshot gate applies to `bulk` runs only
(POC re-runs are always live — the filter, not the snapshot, changes between
them); rulings swap runs even on the POC (filtered to ingested oracles) so
the pipeline is complete.

## Decisions (this review)

- **Source = `default_cards` + `rulings` bulk files + `/sets` API** —
  `oracle_cards` collapses printings; `all_cards` multiplies by language for
  no v1 benefit; sets have no bulk file but one unpaginated API call.
- **Daily updates = manifest diff + targeted hydration, not bulk
  reprocessing** (maintainer, 2026-07-16) — the 560 MB-and-growing daily
  redownload bought little (cards rarely change; the daily payoff is *new*
  cards). `/cards/manifest` is 15,000 entries/page (~8 pages, ~1 min sweep)
  and is Scryfall's documented sync surface; hydration via
  `/cards/collection` (75 ids/req). Observed 2026-07-16: only
  `image_updated_at` carries values — new-card detection works via unknown
  ids; the diff reads `data_updated_at` whenever present.
- **Bulk path = bootstrap, rebuild, and true-up** — kept for the POC,
  testing, first load, and occasional manual re-runs that refresh all prices
  and catch data-only drift.
- **Price freshness is a non-goal** (maintainer, 2026-07-16) — prices
  refresh incidentally on hydrated cards and catalog-wide at true-up
  cadence; no daily all-cards price job.
- **One shared core, filtered for the POC** — no throwaway loader; the two
  acquisition paths feed the same extract/upsert machinery.
- **Images link out** to Scryfall-hosted URIs (rate-limit-free origin);
  caching is a future spec if ever needed.
- **Change-gating by content hash (prices excluded)** so true-ups write
  deltas, not the whole catalog — Neon written-bytes + autovacuum aware.
- **`server --ingest` + `catalog_ingest` role** — one Docker image serves web
  and the cron job; least-privilege credential lives only in the dev
  container and the cron job's env, never on the web service.
- **Scheduling = Render cron job** (maintainer, 2026-07-16 — supersedes the
  same-day pg_cron due-marker design). pg_cron was verified viable-but-
  compromised: available on the project, but with no `http`/`pg_net` it
  could only write a due-marker for the server to poll, ticks fire only
  while the Neon compute is awake (freshness would have been
  usage-proportional on free-tier autosuspend), and the web service would
  have carried the ingest credential. Render cron removes all three
  problems for ~$1/month: a guaranteed clock, no server-side poller, creds
  isolated to the job. GitHub Actions remains rejected (standing "no DB
  creds in GitHub" rule).
- **Migrations reconciled catalog-side only; no prune-by-absence** — user-row
  repointing is a documented manual runbook (RLS-forced tables, no BYPASSRLS
  on Neon).

## Open questions

- ~~Does `default_cards` really include emblem/art-series/checklist layouts?~~
  **Resolved 2026-07-16 (stage 1):** yes, empirically — emblems (7),
  art_series (54), and checklist cards (layout `token`) all ingested from the
  POC subset. See Findings.
- ~~`server --ingest` subcommand vs. separate workspace bin, and env-var
  naming.~~ **Resolved 2026-07-16 (stage 1):** `server --ingest <poc|bulk>`
  subcommand (the `--migrate` precedent held; no measurable server-build
  bloat), `INGEST_DATABASE_URL` / `PROD_INGEST_DATABASE_URL`.
- Batched multi-row upserts vs. `COPY` + staging merge for the full load.
  *(resolved during execution — stage 2, from measured timings; the POC's
  jsonb_to_recordset batches took low single-digit minutes for 3K rows
  including two full file scans, so batches look sufficient)*
- ~~Exact POC set codes + menagerie card ids.~~ **Resolved 2026-07-16
  (stage 1):** checked into
  [`app/src/ingest/poc_filter.json`](../app/src/ingest/poc_filter.json).
- Manifest `data_updated_at`/`created_at`/`oracle_id` population — NULL
  across all sampled entries today; re-observe when building the diff and
  handle populated values from day one. *(resolved during execution —
  stage 3)*
- Render cron specifics: exact schedule expression, instance size (memory
  headroom for hydration batches is trivial; the bulk path doesn't run
  there), build-minutes impact of a second repo-built service. *(resolved
  during execution — stage 3)*
