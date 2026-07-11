# Data model

**Status:** accepted
**Depends on:** [auth](auth.md) — Neon Auth provisions the `neon_auth.users_sync` table this schema references (see [Users & the auth boundary](#users--the-auth-boundary))

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
  and consistent across the schema. The **user identifier is `text`**, not `uuid`:
  it comes from Neon Auth's `neon_auth.users_sync.id` (see below), so every
  `user_id` column and the RLS GUC are `text`.
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
    oracle_id      uuid PRIMARY KEY,        -- Scryfall oracle_id
    name           text NOT NULL,
    mana_cost      text,
    cmc            numeric,                 -- fractional for un-cards
    type_line      text,
    oracle_text    text,
    colors         text[] NOT NULL DEFAULT '{}',
    color_identity text[] NOT NULL DEFAULT '{}',
    keywords       text[] NOT NULL DEFAULT '{}',
    power          text,                    -- '*'/'1+*' → text, not int
    toughness      text,
    loyalty        text,
    layout         text,                    -- 'normal','split','transform',…
    reserved       boolean NOT NULL DEFAULT false,
    edhrec_rank    integer
);

CREATE TABLE printings (                    -- a specific physical object collections point at
    id               uuid PRIMARY KEY,      -- Scryfall card id
    oracle_id        uuid NOT NULL REFERENCES cards(oracle_id),
    set_id           uuid NOT NULL REFERENCES sets(id),
    collector_number text NOT NULL,
    rarity           text NOT NULL,         -- 'common','uncommon','rare','mythic','special','bonus'
    finishes         card_finish[] NOT NULL DEFAULT '{}',  -- which finishes this printing exists in
    lang             text NOT NULL DEFAULT 'en',
    frame            text,
    border_color     text,
    promo            boolean NOT NULL DEFAULT false,
    released_at      date,
    image_uris       jsonb,                 -- Scryfall image_uris; link-out (caching deferred → catalog-ingestion)
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
DB and must not call Scryfall per request — including `rulings` (the card page
shows them). We do **not** store fields we never render (Scryfall's legalities
block, purchase URIs, multi-currency beyond usd/eur/tix, etc.). Images are
linked, not cached (caching is catalog-ingestion's call).

### Users & the auth boundary

Per the maintainer decision (2026-07-11), auth is **[Neon Auth](auth.md)** (Stack
Auth-backed), which manages users for us: enabling it provisions a
`neon_auth.users_sync` table in the database, kept in sync from the auth provider.
Data-model **references** that table; it does not define its own `users`.

The `neon_auth.users_sync` contract this schema relies on:

```sql
-- Provisioned and maintained by Neon Auth (do not create or migrate this):
-- neon_auth.users_sync (
--   id         text PRIMARY KEY,   -- the user id our tables FK to (TEXT, not uuid)
--   email      text,
--   name       text,
--   created_at timestamptz,
--   updated_at timestamptz,
--   deleted_at timestamptz,        -- soft delete: row stays, deleted_at is set
--   raw_json   jsonb
-- )
```

Two properties shape the FK design:

- **`id` is `text`** → every `user_id` in this schema is `text` (and the RLS GUC
  is compared as text, no `::uuid` cast).
- **Sync is asynchronous and deletes are soft.** A just-signed-up user may act
  before their `users_sync` row lands (a hard FK would raise), and user deletion
  sets `deleted_at` rather than removing the row (so `ON DELETE CASCADE` never
  fires from it). Whether to use a hard FK to `neon_auth.users_sync(id)` or a soft
  reference (validate/handle the race in the API, app-driven cleanup on
  soft-delete) is **the auth task's to finalize** — the DDL below writes the FK as
  the intent, annotated.

Consequence for ordering: the **Neon Auth task lands first** (Phase 3, ahead of
this task — the maintainer's option-1 choice), so `neon_auth.users_sync` exists
before the collection migrations run. See [Migration plan](#migration-plan).

### Collection schema (the tree)

```sql
CREATE TABLE collections (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    text NOT NULL REFERENCES neon_auth.users_sync(id),  -- Neon Auth id (text); hard-FK vs. soft-ref: auth task decides
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
    user_id            text NOT NULL REFERENCES neon_auth.users_sync(id),  -- Neon Auth id (text)
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
      USING (user_id = current_setting('app.user_id', true))          -- text, no cast
      WITH CHECK (user_id = current_setting('app.user_id', true));
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
  The id is the **Neon Auth user id (text)** the hosted Axum API extracts from the
  validated session — the API stays the authorization terminus per
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

The **Neon Auth task lands first** (option 1), provisioning
`neon_auth.users_sync`, so both groups below can run in sequence when this task
starts. Kept as two groups for clarity (and because the catalog group has no auth
dependency at all):

1. **Catalog group (no auth dependency):** drop the spike `cards` table, then
   `sets`, `cards`, `printings`, `prices`, `rulings`, the enums, `pg_trgm` + base
   indexes, and permissive read policies.
2. **Collection group (references `neon_auth.users_sync`):** `collections`,
   `deck_commanders`, `holdings`, `desires`, `moves`, plus RLS enable/force/policies
   and the application role.

The spike's `cards(id serial, name)` is replaced (the new `cards` is oracle
identity, unrelated) — the first catalog migration `DROP TABLE cards` before
recreating it; there is no real data to preserve.

## Decisions (this review)

- **Two holding tables** — `holdings` (present, printing grain) and `desires`
  (desired, oracle grain + optional pin). Separate grains/lifecycles beat one
  table with nullable columns and a split uniqueness constraint.
- **RLS from day one** — enabled + forced on user tables as a backstop under the
  API terminus; app connects as a non-owner role and sets `app.user_id` per
  transaction.
- **Latest-only prices** — one upserted row per printing; a price time-series is
  deferred to a later table.
- **Auth = Neon Auth; users via `neon_auth.users_sync`** — data-model references
  the Neon-managed table (text `user_id` FKs), doesn't define its own `users`.
  Sequencing resolved via **option 1**: a Neon Auth task is added to Phase 3 ahead
  of this one, so `users_sync` exists before the collection migrations run.

## Open questions

Accepted with these deferred; none blocks acceptance — each notes where it resolves.

- **Hard FK vs. soft reference to `neon_auth.users_sync(id)`.** The sync is
  asynchronous (a new user can act before their row lands → a hard FK raises) and
  deletes are soft (`deleted_at`, so `ON DELETE CASCADE` never fires). Keep a hard
  FK (and handle the first-request race in the API) or drop to a soft reference with
  app-driven cleanup on soft-delete? The DDL here writes the hard FK as the intent.
  *(resolved during execution — the Neon Auth task, shared with data-access-backends)*
- **Inbox provisioning on first login.** Each user needs their one `is_inbox`
  collection; given async user sync, where/when is it created (lazily on first
  `/my` load vs. an auth post-signup hook)? *(resolved during execution — the Neon
  Auth task / collection-api)*
- **Denormalize `user_id` onto `holdings`/`desires`/`deck_commanders`** for direct
  RLS policies (simpler/faster) vs. `EXISTS`-on-collection policies (no
  duplication, must stay consistent)? Leaning denormalize. *(resolved during
  execution — the initial migrations)*
- **Condition/language granularity in v1.** The schema tracks `finish` +
  `condition` + `language` on holdings, but the wireframes only surface finish so
  far. Keep the columns (defaulted) and let the UI reveal them later, or trim v1 to
  finish-only? Leaning keep — cheap, and hard to add retroactively to the uniqueness
  constraint. *(resolved during execution — the initial migrations)*
- **Price time-series** — when (if) we want price charts, add an append-only
  `price_history(printing_id, finish, amount, observed_at)` with a retention/
  partition policy. Out of scope now; noted so the latest-only `prices` shape
  doesn't foreclose it. *(deferred — a future spec; not a v1 concern)*
- **App role provisioning on Neon** — creating the non-owner application role and
  wiring `DATABASE_URL` to it (vs. the migration owner) is an ops step that
  touches Render/`.devcontainer` env. *(resolved during execution — the Neon Auth
  task, shared with data-access-backends)*
