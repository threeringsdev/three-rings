# Data model

**Status:** implemented
**Depends on:** [auth](auth.md) — Neon Auth (Better Auth) provides the `neon_auth."user"` table this schema references (see [Users & the auth boundary](#users--the-auth-boundary))

Design inputs (not hard dependencies): the [architecture-spike](architecture-spike.md)
Findings (Neon + sqlx constraints), the accepted [ui-design](ui-design.md) concept
model (collections, the three counts, needs, moves), and
[data-access-backends](data-access-backends.md) (the API-as-terminus stance that
frames the RLS decision).

## Problem

Define the Postgres schema for the card catalog and user collections — the
foundation every other spec builds on. The spike proved connectivity with a
throwaway `cards(id serial, name)` table ([`migrations/0001_create_cards.sql`](../migrations/0001_create_cards.sql));
this spec replaces it with the real model.

The schema must support, concretely:

- A Scryfall-sourced catalog (~100K printings) where **oracle identity** (a card)
  and **printing** (a specific set/collector-number/finish object) are distinct,
  and collections reference printings.
- The ui-design **concept model**: a nested tree of binder/deck collections; three
  counts per card entry (present, desired, owned); needs (owned-elsewhere vs.
  short); and an undoable, teardown-capable move log.
- Fuzzy catalog search (name and, later, oracle text) — the base indexes live
  here; the query→SQL translation is [catalog-search](catalog-search.md)'s.

## Scope

**In:** catalog tables (sets, cards, printings, prices, rulings), the collection
tree, the present/desired holding tables, the move ledger, the base search
indexes, the RLS policy design, and the migration plan.

**Out:** the ingestion pipeline ([catalog-ingestion](catalog-ingestion.md)); the
query-syntax subset and query↔rail contract ([catalog-search](catalog-search.md));
the trait split and hosted/native backends ([data-access-backends](data-access-backends.md));
auth internals — password hashing, sessions, verification ([auth](auth.md), which
owns `users`); decks-beyond-basics, sharing, trade, import/export (future specs).

## Design

### Conventions

- **Primary keys.** Catalog rows use Scryfall's own UUIDs (`oracle_id` for cards,
  the printing `id`, the set `id`) so ingestion upserts are natural and idempotent.
  User-owned rows (`collections`, `holdings`, `desires`, `moves`) use
  `uuid DEFAULT gen_random_uuid()` — non-enumerable (matters for a public-repo app)
  and consistent across the schema. The **user identifier is `uuid`**: it is
  `neon_auth."user".id`, which Neon Auth (Better Auth) issues as `uuid` (verified
  against the live table — see below). So every `user_id` column and the RLS GUC are
  `uuid`.
- **Timestamps** are `timestamptz`, default `now()`.
- **Money** is `numeric(10,2)`; **counts/quantities** are `integer` with
  `CHECK (quantity > 0)`.
- **Owned is never stored.** It is a pure aggregate of present across a user's
  collections (a view / query), by the concept model's definition.

### Enums & shared types

```sql
CREATE TYPE collection_kind AS ENUM ('binder', 'deck');
CREATE TYPE card_finish     AS ENUM ('nonfoil', 'foil', 'etched');   -- Scryfall's finish set
CREATE TYPE card_condition  AS ENUM ('nm', 'lp', 'mp', 'hp', 'dmg'); -- physical grade; default 'nm'
```

- **Colors / color identity / keywords** are `text[]` (Scryfall ships arrays;
  colors are single letters `W U B R G`).
- **Language** is a `text` Scryfall language code (`en`, `ja`, …), default `'en'`
  — an open set, so not an enum.
- Adding a `card_finish`/`card_condition` value later is an `ALTER TYPE ... ADD
  VALUE` (cheap, non-blocking); the sets are stable enough to justify enums over
  `text + CHECK`.

### Catalog schema (public, read-mostly)

```sql
CREATE TABLE sets (
    id           uuid PRIMARY KEY,          -- Scryfall set id
    code         text NOT NULL UNIQUE,      -- 'mh3', 'lea', …
    name         text NOT NULL,
    set_type     text NOT NULL,             -- 'expansion','core','commander',…
    released_at  date,
    card_count   integer,
    icon_svg_uri text
);

CREATE TABLE cards (                        -- ORACLE identity (one row per distinct card)
    oracle_id       uuid PRIMARY KEY,       -- Scryfall oracle_id; for layout='reversible_card' read from card_faces[0].oracle_id
    name            text NOT NULL,          -- combined "Front // Back" on multi-face layouts
    mana_cost       text,                   -- single-face only; NULL on multi-face → see card_faces
    cmc             numeric,                -- fractional for un-cards
    type_line       text,                   -- combined on multi-face
    oracle_text     text,                   -- single-face only; NULL on multi-face → see card_faces
    colors          text[] NOT NULL DEFAULT '{}',   -- single-face; per-face colors live in card_faces
    color_identity  text[] NOT NULL DEFAULT '{}',   -- whole-card (Scryfall aggregates across faces)
    color_indicator text[],                 -- frame color pip (e.g. Ancestral Vision); per-face on DFCs
    keywords        text[] NOT NULL DEFAULT '{}',
    power           text,                   -- '*'/'1+*' → text, not int; single-face, NULL on multi-face
    toughness       text,
    loyalty         text,
    produced_mana   text[],                 -- mainly catalog-search's `produces:` filter
    layout          text,                   -- 'normal','split','transform','modal_dfc','adventure','flip','meld','reversible_card','token','double_faced_token',…
    legalities      jsonb,                  -- flat {format: legal|not_legal|banned|restricted}; rendered on card page
    card_faces      jsonb,                  -- NULL single-face; else ordered per-face oracle data → see "Multi-face cards & relations"
    all_parts       jsonb,                  -- NULL unless related; token/meld/combo links → see "Multi-face cards & relations"
    reserved        boolean NOT NULL DEFAULT false,
    game_changer    boolean,                -- commander-bracket flag; renders near legalities
    edhrec_rank     integer
);

CREATE TABLE printings (                    -- a specific physical object collections point at
    id               uuid PRIMARY KEY,      -- Scryfall card id
    oracle_id        uuid NOT NULL REFERENCES cards(oracle_id),
    set_id           uuid NOT NULL REFERENCES sets(id),
    collector_number text NOT NULL,
    rarity           text NOT NULL,         -- 'common','uncommon','rare','mythic','special','bonus' (tokens/checklist carry 'common')
    finishes         card_finish[] NOT NULL DEFAULT '{}',  -- which finishes this printing exists in
    lang             text NOT NULL DEFAULT 'en',
    frame            text,
    frame_effects    text[] NOT NULL DEFAULT '{}',  -- 'showcase','extendedart','inverted',… distinguishes treatments within a set
    border_color     text,
    full_art         boolean NOT NULL DEFAULT false,
    textless         boolean NOT NULL DEFAULT false,
    promo            boolean NOT NULL DEFAULT false,
    promo_types      text[] NOT NULL DEFAULT '{}',   -- 'boosterfun','godzillaseries','buyabox',…
    flavor_name      text,                            -- displayed name for Godzilla/Dracula/etc. variants (name still holds the real card)
    artist           text,                            -- single-face; per-face in faces jsonb on DFCs
    flavor_text      text,                            -- printing-level; per-face on DFCs
    watermark        text,                            -- guild/clan/set watermark, when present
    security_stamp   text,                            -- 'oval','triangle','acorn',… anti-counterfeit stamp
    games            text[] NOT NULL DEFAULT '{}',    -- 'paper','arena','mtgo' this printing exists in
    digital          boolean NOT NULL DEFAULT false,  -- Arena/MTGO-only printing; label/filter in the printing picker
    released_at      date,
    image_uris       jsonb,                 -- single-face images; NULL on multi-face → see faces. Link-out (caching deferred → catalog-ingestion)
    faces            jsonb,                 -- NULL single-face; else per-printing per-face data → see "Multi-face cards & relations"
    UNIQUE (set_id, collector_number, lang)
);

CREATE TABLE prices (                       -- latest snapshot only; upserted by ingestion
    printing_id uuid PRIMARY KEY REFERENCES printings(id) ON DELETE CASCADE,
    usd         numeric(10,2),
    usd_foil    numeric(10,2),
    usd_etched  numeric(10,2),
    eur         numeric(10,2),
    eur_foil    numeric(10,2),
    tix         numeric(10,2),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE rulings (                       -- rendered on the card page; population is catalog-ingestion's
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    oracle_id    uuid NOT NULL REFERENCES cards(oracle_id),
    published_at date,
    source       text,                       -- 'wotc','scryfall'
    comment      text NOT NULL
);
CREATE INDEX rulings_oracle_idx ON rulings (oracle_id);
```

Prices sit in their own table (keyed on `printing_id`) precisely so the
high-churn daily refresh **never rewrites catalog rows** — the spec's original
concern. Latest-only: ingestion `UPSERT`s one row per printing; a price
time-series is a deliberately deferred later table (see Open questions).

**What we store vs. link out** (resolves the draft's oracle/rulings question):
store every oracle/printing field the app renders — the card page SSRs from our
DB and must not call Scryfall per request — including `rulings`, `legalities`,
the multi-face `card_faces`/`faces` arrays, and the `all_parts` relations (all
rendered on the card page). We do **not** store fields we never render — purchase
URIs, multi-currency beyond usd/eur/tix, `related_uris`, the rulings/prints/set
search URIs, or Scryfall's id cross-reference block. Images are linked, not
cached (caching is catalog-ingestion's call). *(Corrected 2026-07-14: `legalities`
was previously on the "never store" list — it is rendered, so it is now stored.
See the Scryfall shape review in Decisions.)*

### Multi-face cards & relations

Scryfall splits multi-face layouts (`transform`, `modal_dfc`, `split`,
`adventure`, `flip`, `meld`, `reversible_card`, `double_faced_token`) into a
`card_faces` array and **omits the per-face fields at top level** — a transform
card has no top-level `mana_cost`, `oracle_text`, `colors`, `power`/`toughness`,
or `image_uris` (verified against live objects, 2026-07-14). We mirror that split
rather than fight it:

- **Oracle-scoped face data → `cards.card_faces jsonb`**, an ordered array
  `[{name, mana_cost, type_line, oracle_text, colors, color_indicator, power,
  toughness, loyalty, defense}]`. `NULL` for single-face cards, whose data stays
  in the top-level columns (kept there for name/text search + trgm).
- **Per-printing per-face data → `printings.faces jsonb`**, an ordered array
  `[{image_uris, artist, flavor_text}]` — these vary by printing. `NULL` for
  single-face printings (top-level `image_uris`/`artist`/`flavor_text` used).
- **`reversible_card` has no top-level `oracle_id`** — ingestion reads it from
  `card_faces[0].oracle_id` (both faces share it). Every other layout keeps a
  single top-level `oracle_id`, so `cards` keying holds.

**Why jsonb, not a normalized `card_faces` table:** faces are never referenced by
another row (`holdings`/`desires`/`moves` point at printings and oracle, never a
face), are always read and written whole with their card, and the per-printing
face data is jsonb regardless — so a table would add a join on every multi-face
render and an oracle/printing asymmetry for no integrity gain. **Escape hatch:**
if [catalog-search](catalog-search.md) later needs per-face attribute filtering it
can't get from card-level aggregates, promote `card_faces` to a table then.

**Related cards — `cards.all_parts jsonb`** stores Scryfall's related-card links
`[{id, component, name, type_line}]` (`component` ∈ `token`/`meld_part`/
`meld_result`/`combo_piece`), powering "makes a Squirrel token" / "melds into
Brisela" on the card page. `NULL` for the majority of cards with no relations.
Ingestion/render notes:

- **`id` is a printing id, not an `oracle_id`** — resolve links via
  `printings.id`, then hop to `oracle_id` for a stable click-through. The
  referenced printing is whichever this card's ingested object named (e.g. that
  set's token art).
- **Each entry includes the card itself** (verified: Bruna lists Bruna) — filter
  self out by id on render.
- Tokens and checklist cards are opted **into** the catalog (maintainer decision
  2026-07-14; they carry `rarity`/`collector_number`/`oracle_id`, verified, so the
  `NOT NULL` columns hold), so these links resolve to real rows via a join; the
  stored `name`/`type_line` is only a fallback for anything not yet ingested.
- Bidirectional lookup ("what makes a Squirrel token?") is **not** modeled here —
  an oracle-text search for "create a Squirrel token" serves it, so no jsonb GIN
  index or `related_cards` table is warranted.

### Users & the auth boundary

Per the maintainer decision (2026-07-11), auth is **[Neon Auth](auth.md)**, which
turns out to be **Better Auth** running inside the Neon platform (confirmed against
the live database, not the earlier Stack Auth assumption). It stores its tables
**directly** in a `neon_auth` schema in our database — a live table, *not* an
async-synced mirror. Data-model **references** the users table; it does not define
its own `users`.

The `neon_auth."user"` contract this schema relies on (Better Auth schema, verified
2026-07-11 on both branches):

```sql
-- Owned and maintained by Neon Auth / Better Auth (do NOT create or migrate this):
-- neon_auth."user" (            -- "user" is a reserved word → always quote it
--   id            uuid PRIMARY KEY,   -- the user id our tables FK to (UUID)
--   email         text NOT NULL,
--   name          text NOT NULL,
--   emailVerified boolean,
--   image         text,
--   createdAt     timestamptz,
--   updatedAt     timestamptz,
--   role, banned, banReason, banExpires  -- admin/ban fields (unused by us)
-- )
-- Siblings in the schema: session, account, verification, jwks, organization, …
```

What this means for the FK design (all simpler than the earlier draft assumed):

- **`id` is `uuid`** → every `user_id` in this schema is `uuid` and the RLS GUC is
  compared with a `::uuid` cast.
- **It's the real, in-database table** (same branch, same transaction visibility),
  so there is **no async-sync race** — a hard FK is safe. Better Auth **hard-deletes**
  a user row (no `deleted_at`), so `ON DELETE CASCADE` from our tables actually
  fires. The DDL below uses a hard FK with `ON DELETE CASCADE`.
- Mild caveat: the `neon_auth` schema is Neon-managed and could change under a
  future Better Auth migration; if one ever conflicts with our FK, revisit. Not a
  day-one concern.

Consequence for ordering: Neon Auth is **already provisioned on both branches**
(`production` and `dev`), so `neon_auth."user"` already exists — the collection
migrations are unblocked. The Neon Auth task (Phase 3, ahead of this one) is now
about wiring the app to it, not provisioning. See [Migration plan](#migration-plan).

### Collection schema (the tree)

```sql
CREATE TABLE collections (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- Neon Auth (Better Auth) user id
    parent_id  uuid REFERENCES collections(id) ON DELETE CASCADE,  -- NULL = top level
    kind       collection_kind NOT NULL,
    name       text NOT NULL,
    is_inbox   boolean NOT NULL DEFAULT false,
    position   numeric NOT NULL DEFAULT 0,     -- fractional index for O(1) drag-reorder among siblings
    format     text,                            -- decks only (e.g. 'commander','modern')
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (format IS NULL OR kind = 'deck')
);

-- exactly one Inbox per user
CREATE UNIQUE INDEX collections_one_inbox ON collections (user_id) WHERE is_inbox;
CREATE INDEX collections_user_parent_idx ON collections (user_id, parent_id);

CREATE TABLE deck_commanders (                  -- 0..2 commanders (partners); decks only, enforced in app
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    printing_id   uuid NOT NULL REFERENCES printings(id),
    PRIMARY KEY (collection_id, printing_id)
);
```

- **Inbox** is a normal row flagged `is_inbox` (undeletable/renamable enforced in
  the API); **All cards** is the virtual aggregate view — no table, per the IA doc.
- **Tree metaphor:** any collection may hold both child collections and its own
  cards; a childless binder acts as a folder. `parent_id` self-reference gives the
  nesting; cycle prevention is an app/trigger concern (collection-api), not a
  schema constraint.
- `position` uses fractional indexing (LexoRank-style) so drag-to-reorder writes
  one row, not the whole sibling list.

### The three counts — two holding tables

Following the accepted "two tables" modeling decision: **present** and **desired**
have different grains and lifecycles, so they get separate tables.

```sql
-- PRESENT: physical copies in a collection, at printing grain.
CREATE TABLE holdings (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    printing_id   uuid NOT NULL REFERENCES printings(id),
    finish        card_finish   NOT NULL DEFAULT 'nonfoil',
    condition     card_condition NOT NULL DEFAULT 'nm',
    language      text NOT NULL DEFAULT 'en',
    quantity      integer NOT NULL CHECK (quantity > 0),
    UNIQUE (collection_id, printing_id, finish, condition, language)
);

-- DESIRED: a target count in a collection, at ORACLE grain by default,
-- with an optional pin to a specific printing.
CREATE TABLE desires (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    oracle_id     uuid NOT NULL REFERENCES cards(oracle_id),
    printing_id   uuid REFERENCES printings(id),   -- NULL = any printing; set = "specific printing only" pin
    quantity      integer NOT NULL CHECK (quantity > 0),
    UNIQUE NULLS NOT DISTINCT (collection_id, oracle_id, printing_id)
);
```

- `holdings` is the **source of truth for present**; the move ledger (below) is an
  audit trail, not the state.
- `desires.printing_id` is `NULL` for the common "a deck wants 4 Bolts, any
  printing" case and non-null for a collector's pin. `UNIQUE NULLS NOT DISTINCT`
  (Postgres 15+, available on Neon) collapses to one unpinned desire per
  (collection, card) without a `COALESCE` expression index.
- A pinned `printing_id` should share the row's `oracle_id`; cross-table CHECKs
  aren't possible, so the API enforces it (candidate for a trigger if it proves
  fragile).

**Derived counts (views/queries, never stored):**

- **Owned** (per user, per oracle card):
  ```sql
  CREATE VIEW owned_by_card AS
  SELECT c.user_id, p.oracle_id, sum(h.quantity)::int AS owned
  FROM holdings h
  JOIN collections c ON c.id = h.collection_id
  JOIN printings   p ON p.id = h.printing_id
  GROUP BY c.user_id, p.oracle_id;
  ```
- **Present rollup** (own + all descendants) — a recursive CTE over `collections`
  summing `holdings.quantity`; the rolled-up portion is distinguished in the UI.
- **Shortfall / shopping list** — per card: `sum(desired across a user's collections)
  − owned`, floored at 0.
- **Needs (owned-elsewhere)** — for a collection: `desired − present here`, with the
  gap located against `holdings` in the user's *other* collections.

All four are read-time computations; a personal collection is small enough that
on-demand aggregation is fine. If profiling later says otherwise, `owned_by_card`
is the first candidate for a materialized view.

### Move ledger

```sql
CREATE TABLE moves (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id            uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- Neon Auth (Better Auth) user id
    printing_id        uuid NOT NULL REFERENCES printings(id),
    finish             card_finish   NOT NULL,
    condition          card_condition NOT NULL,
    language           text NOT NULL,
    from_collection_id uuid REFERENCES collections(id) ON DELETE SET NULL,  -- NULL = external intake (+Have)
    to_collection_id   uuid REFERENCES collections(id) ON DELETE SET NULL,  -- NULL = removal
    quantity           integer NOT NULL CHECK (quantity > 0),
    created_at         timestamptz NOT NULL DEFAULT now(),
    undone_at          timestamptz                                          -- set when reversed
);
CREATE INDEX moves_user_created_idx ON moves (user_id, created_at DESC);
CREATE INDEX moves_to_recent_idx    ON moves (to_collection_id, printing_id, created_at DESC);
```

- Append-only ledger. Every `+ Have`, move, and teardown writes a row; `holdings`
  is updated in the same transaction. `from = NULL` models intake; `to = NULL`
  models removal.
- **Undo** (the toast) reverses the `holdings` effect and stamps `undone_at`
  (exact mechanics — compensating row vs. flag — are collection-api's).
- **"Return to previous locations"** teardown reads the most-recent move *into* a
  collection per (printing, finish, …) via `moves_to_recent_idx`; cards with no
  history fall back to Inbox.

### Search indexing (base only)

```sql
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX cards_name_trgm_idx ON cards USING gin (name gin_trgm_ops);  -- fuzzy name (both search surfaces)
CREATE INDEX printings_oracle_idx ON printings (oracle_id);
CREATE INDEX printings_set_idx    ON printings (set_id);
```

This is the floor: fuzzy name search plus the join/filter indexes both search
surfaces need. Oracle-text full-text (`o:` in Scryfall), type/color/rarity filter
indexes, and the query→SQL mapping are **[catalog-search](catalog-search.md)'s**
to design; that spec may add a generated `tsvector` column + GIN and further
btree/GIN indexes. Data-model provides the base; catalog-search refines.

### Row-level security (enabled day one)

Per the maintainer decision, RLS is on from the start as **defense-in-depth beneath**
the hosted API (which remains the authorization terminus per
data-access-backends — RLS is a backstop, not the sole layer).

- **User tables** (`collections`, `deck_commanders`, `holdings`, `desires`,
  `moves`): `ENABLE ROW LEVEL SECURITY` + `FORCE ROW LEVEL SECURITY`, with a
  policy scoping every row to the current user:
  ```sql
  ALTER TABLE collections ENABLE ROW LEVEL SECURITY;
  ALTER TABLE collections FORCE ROW LEVEL SECURITY;
  CREATE POLICY collections_owner ON collections
      USING (user_id = current_setting('app.user_id', true)::uuid)
      WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);
  ```
  Tables keyed by `collection_id` (not `user_id` directly) scope via an `EXISTS`
  on the owning collection, or carry a denormalized `user_id` for a direct
  policy — decide during implementation (denormalized `user_id` is simpler and
  faster; the trade is keeping it consistent).
- **Catalog tables** (`sets`, `cards`, `printings`, `prices`, `rulings`) are
  public read; either leave RLS off or add a permissive read-all policy. Writes
  happen only through the ingestion role.
- **Mechanism.** The app connects as a **non-owner application role** (the migration
  owner bypasses RLS, hence `FORCE`), and every request runs its queries inside a
  transaction that first does `SET LOCAL app.user_id = <authenticated user id>`.
  The id is the **Neon Auth user id (uuid)** — the `sub` claim of the verified JWT —
  the hosted Axum API extracts from the validated session; the API stays the
  authorization terminus per
  data-access-backends; we deliberately do **not** use Neon's Data API / JWT-RLS
  path (`auth.user_id()` from `pg_session_jwt`), which would move enforcement out
  of the API. Policies read `current_setting('app.user_id', true)`. This requires
  the data-access layer to always open a per-request transaction and set the GUC —
  a real cost, accepted deliberately (see Decisions). Anonymous catalog requests
  set no GUC and touch only public tables.

### Migration plan

`sqlx` embedded migrations (spike-proven), run against Neon's **direct** endpoint
— *not* the PgBouncer pooler, whose transaction mode breaks migration advisory
locks (architecture-spike Finding). Numbered `NNNN_description.sql`.

**How migrations run (decided 2026-07-12; delivery revised 2026-07-12 — Option B).**
Neon has no managed migration runner (unlike Supabase's CLI+platform), so we run
them ourselves — but **not from the app**. The spike wired `MIGRATOR.run` into
server startup ([app/src/db.rs](../app/src/db.rs)); that call is removed. Migrations
run as a **separate owner-privileged step** (`server --migrate`, reading
`MIGRATION_DATABASE_URL` = the `neondb_owner` credential). The running web server
connects as the non-owner **`app_runtime`** role (created 2026-07-12 on both
branches: CRUD + RLS-subject, no DDL/superuser/bypassrls), so the long-running
process never holds owner/DDL power.

*Who invokes that step* depends on the plan. The clean target is a **Render
pre-deploy command** (`/app/server --migrate`), which runs migrations in-deploy
under an owner URL in Render env only. **But Render's free tier has no pre-deploy
hook**, and our standing rule keeps DB creds out of GitHub Actions, so on free tier
we use **Option B: apply migrations manually from the trusted dev container** via
[`scripts/migrate.sh`](../scripts/migrate.sh) (`dev` default, `prod` with a
confirmation), reading owner strings from `.devcontainer/.env`. The owner credential
then lives **only** on the maintainer's machine — never on Render (so an app
compromise yields only `app_runtime`) and never in CI. Render holds just
`DATABASE_URL` = `app_runtime`. Discipline: run `scripts/migrate.sh prod` **before**
pushing code that depends on the new schema (expand-first / backward-compatible
migrations). Upgrade path: on a paid Render plan, move this to a pre-deploy command
and drop the manual step. This removes app-startup DDL entirely — the runtime needs
only one credential (`app_runtime`), no in-app role juggling.

Neon Auth is **already provisioned on both branches**, so `neon_auth."user"`
exists and both groups below can run in sequence. Kept as two groups for clarity
(and because the catalog group has no auth dependency at all):

1. **Catalog group (no auth dependency):** drop the spike `cards` table, then
   `sets`, `cards`, `printings`, `prices`, `rulings`, the enums, `pg_trgm` + base
   indexes, and permissive read policies.
2. **Collection group (references `neon_auth."user"`):** `collections`,
   `deck_commanders`, `holdings`, `desires`, `moves`, plus RLS enable/force/policies
   and the application role.

The spike's `cards(id serial, name)` is replaced (the new `cards` is oracle
identity, unrelated) — the first catalog migration `DROP TABLE cards` before
recreating it; there is no real data to preserve.

## Findings (implementation — 2026-07-15)

The initial schema shipped as three forward-only `sqlx` migrations, applied to the
Neon **dev** branch via `scripts/migrate.sh dev`:

- `0002_catalog.sql` — drops the spike `cards`, the `card_finish` enum, the five
  catalog tables (`sets`, `cards`, `printings`, `prices`, `rulings`), `pg_trgm` +
  the three base search indexes, and `app_runtime` read grants.
- `0003_collections.sql` — the `collection_kind`/`card_condition` enums, the five
  user tables, the `owned_by_card` view, RLS enable+force+policies, `app_runtime`
  CRUD grants.
- `0004_catalog_app_readonly.sql` — the default-ACL correction below.

**RLS is real, verified end-to-end.** A rolled-back probe transaction as
`app_runtime`: `SET LOCAL app.user_id = <a real neon_auth."user".id>`, insert a
collection, see it (count 1); switch the GUC to a random uuid → the row vanishes
(count 0). `relrowsecurity` + `relforcerowsecurity` confirmed on all five user
tables and off on the catalog tables; the five owner policies present.

**Neon default-ACL finding → `0004`.** `app_runtime` is granted CRUD (`arwd`) on
*every* table `neondb_owner` creates, via a pre-existing `pg_default_acl` entry (set
when the role was provisioned, 2026-07-12) — so `0002`'s catalog tables landed
writable by the app role and its explicit `GRANT SELECT` was redundant. The spec
requires catalog writes only through the owner/ingestion role, so `0004` revokes
`INSERT/UPDATE/DELETE/TRUNCATE` on the catalog tables from `app_runtime`, leaving
`SELECT`. Verified: `has_table_privilege('app_runtime','cards','INSERT')` is now
false, `SELECT` true. **Future catalog tables (e.g. catalog-search's) must repeat
this revoke**, or the role's default privileges get narrowed globally — a
data-access/ops call.

**`owned_by_card` uses `security_invoker = true`** so it runs with the querying
`app_runtime`'s RLS context (the `app.user_id` GUC), not the RLS-forced owner's —
keeping the per-user scoping unambiguous.

**`scripts/migrate.sh` hardened.** The `sqlx::migrate!` macro embeds the migrations
dir at compile time, and cargo did not reliably rebuild `app` when a *new* `.sql`
file was added — so `--migrate` silently reported "up to date" without applying
`0004` until `app` was force-recompiled. `migrate.sh` now `touch`es
`app/src/db.rs` before building to guarantee a re-embed.

**Prod not yet migrated — deliberate (expand-first).** Dropping the spike `cards`
and replacing it with the oracle-identity `cards` is a breaking change against the
*currently deployed* spike code, whose `/cards` page reads `SELECT id, name FROM
cards`. Migrating the Neon **production** branch now would break the live site, so
prod migration is deferred to coordinate with the **data-access-backends** deploy
(which removes the spike `app/src/db.rs` reads); run `scripts/migrate.sh prod` as
part of that landing. Consequence for local dev: after this migration the dev-branch
`/cards` spike page shows its graceful "Failed to load cards" error (the `id` column
is gone) until data-access-backends replaces it — expected, non-fatal.

## Decisions (this review)

- **Scryfall shape review (2026-07-14) — fields, multi-face, relations.** Fetched
  live objects across every tricky layout (normal, transform, modal_dfc, split,
  adventure, flip, meld, battle, reversible_card, token, double_faced_token, plus
  showcase/Godzilla printings) and corrected the accepted schema:
  - Multi-face layouts carry **one** top-level `oracle_id` (so `cards` keying
    holds) **except `reversible_card`**, which has it only per-face — but they
    **omit per-face fields at top level**, so the accepted schema would have
    ingested every DFC with null cost/text/colors/P-T and every DFC printing with
    null images. Added `cards.card_faces` + `printings.faces` jsonb; chose jsonb
    over a normalized table (faces are never FK targets, written whole, symmetric
    with per-printing face data — see [Multi-face cards & relations](#multi-face-cards--relations)).
  - Added rendered fields the accepted schema dropped: `color_indicator`,
    `defense`, `legalities`, `produced_mana`, `game_changer` on `cards`;
    `frame_effects`, `full_art`, `textless`, `promo_types`, `flavor_name`,
    `artist`, `flavor_text`, `watermark`, `security_stamp`, `games`, `digital` on
    `printings`. Treatment/variant identity (showcase vs. base, Godzilla names) is
    core to a *collection* app. `prices` needed no change — its keys already match.
  - Added `all_parts` jsonb and opted tokens + checklist cards into the catalog so
    relations resolve. `legalities` moved from the "never store" list to stored.
  - Cross-spec follow-ups filed: [catalog-ingestion](catalog-ingestion.md) (ingest
    token/checklist/emblem/art-series layouts; populate the new jsonb columns;
    source reversible `oracle_id` from `card_faces[0]`) and
    [catalog-search](catalog-search.md) (multi-face `o:` must concatenate
    `card_faces[].oracle_text`; new filter inputs available).
- **Two holding tables** — `holdings` (present, printing grain) and `desires`
  (desired, oracle grain + optional pin). Separate grains/lifecycles beat one
  table with nullable columns and a split uniqueness constraint.
- **RLS from day one** — enabled + forced on user tables as a backstop under the
  API terminus; app connects as a non-owner role and sets `app.user_id` per
  transaction.
- **Latest-only prices** — one upserted row per printing; a price time-series is
  deferred to a later table.
- **Auth = Neon Auth (Better Auth); users via `neon_auth."user"`** — data-model
  references the Neon-managed live table with **`uuid`** `user_id` FKs
  (`ON DELETE CASCADE`), doesn't define its own `users`. Neon Auth is already
  provisioned on both branches, so the table exists; the Phase 3 Neon Auth task
  wires the app to it rather than provisioning. *(Corrected 2026-07-11 against the
  live DB: earlier text-id / `users_sync` / async-sync assumptions came from stale
  Stack Auth docs and were wrong — see Findings in [auth](auth.md).)*

## Open questions

Accepted with these deferred; none blocks acceptance — each notes where it resolves.

- ~~Multi-face card representation~~ **Resolved 2026-07-14 (Scryfall shape
  review):** `card_faces`/`faces` jsonb over a normalized table — see
  [Multi-face cards & relations](#multi-face-cards--relations).
- ~~`all_parts` / related cards~~ **Resolved 2026-07-14:** stored as `cards.all_parts
  jsonb`; tokens + checklist cards opted into the catalog so links resolve; no
  bidirectional index (oracle-text search covers it).
- ~~Hard FK vs. soft reference to the users table~~ **Resolved by the live-DB
  finding (2026-07-11):** Better Auth's `neon_auth."user"` is a real in-database
  table with a `uuid` PK and hard deletes, so a hard FK with `ON DELETE CASCADE` is
  correct — no async-sync race, no soft-delete to work around. (The question only
  existed under the mistaken Stack Auth `users_sync` model.)
- **Inbox provisioning on first login.** Each user needs their one `is_inbox`
  collection; decide where/when it's created (lazily on first `/my` load vs. a
  Better Auth post-signup webhook — `project_config.webhook_config` exists). No async
  race to worry about now. *(resolved during execution — the Neon Auth task /
  collection-api)*
- ~~Denormalize `user_id` onto `holdings`/`desires`/`deck_commanders`~~ **Resolved
  2026-07-15 (initial migrations — denormalize):** every user table carries
  `user_id uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE`, so all
  five RLS policies are one direct `user_id = current_setting('app.user_id',
  true)::uuid` — no `EXISTS`-on-collection hop. The API keeps the denormalized
  `user_id` consistent with the owning collection's (data-access-backends). See
  Findings.
- ~~Condition/language granularity in v1~~ **Resolved 2026-07-15 (initial
  migrations — keep):** `holdings`/`moves` keep full `finish` + `condition` +
  `language` (defaulted `nonfoil`/`nm`/`en`); the UI reveals them later. Cheap now,
  hard to retrofit into the uniqueness constraint.
- **Price time-series** — when (if) we want price charts, add an append-only
  `price_history(printing_id, finish, amount, observed_at)` with a retention/
  partition policy. Out of scope now; noted so the latest-only `prices` shape
  doesn't foreclose it. *(deferred — a future spec; not a v1 concern)*
- ~~App role provisioning on Neon~~ **Resolved:** the non-owner `app_runtime` role
  and its `DATABASE_URL` wiring landed in Phase 3 (role created 2026-07-12; local
  `.env` `DATABASE_URL` = `app_runtime`). Table grants + a least-privilege
  correction (a pre-existing Neon default-ACL over-granted catalog CRUD to
  `app_runtime`; `0004` revokes it) landed with the initial migrations — see
  Findings.
