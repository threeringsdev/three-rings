# Card tagging & deck boards

**Status:** implemented
**Depends on:** [data-model](data-model.md) (owns the tables this extends),
[collection-api](collection-api.md) (the endpoints that project the tag/board
operations), [auth](auth.md) (the user id everything scopes to)

Design inputs (not hard dependencies): the [ui-design](ui-design.md) concept
model (decks, the three counts) and [data-access-backends](data-access-backends.md)
(the trait seam the new operations are methods on).

## Problem

Cards inside a collection — especially a deck — need categorizations beyond what
the card object provides. Two kinds, and only one needs storage:

- **Derived groupings** — by type, name, mana cost, color, rarity. These are
  catalog columns, so "group this deck by type / CMC" is a `GROUP BY` at read
  time. **No storage.**
- **Assigned metadata** — not derivable from the card: which cards are the
  **commander(s)**, which **board** a copy sits on (main / sideboard / maybe),
  **companion**, and arbitrary user categories (`Draw spells`, `Ramp`, `Removal`).
  This *is* stored, and it's a **tagging** problem: built-in tags the app
  understands, plus user-created tags at **per-account** and **per-deck** scope.

The original [data-model](data-model.md) modeled only commanders, via a dedicated
`deck_commanders` table — a point solution that doesn't generalize to the wider
need uncovered here (2026-07-15 review). This spec defines the general model and
**retires `deck_commanders`** (commander becomes a built-in tag).

## Scope

**In:** the two annotation shapes and their storage — the **board** partition
(a column on `holdings`/`desires`) and the **tag** system (`tags` + `card_tags`,
with system / account / deck scopes); the built-in `commander`/`companion` tags
and their app-enforced semantics; RLS for the new tables; the migration
(including dropping `deck_commanders`); and the endpoint surface (specified in
detail by [collection-api](collection-api.md)).

**Out:**
- **Derived groupings** (catalog-computed, no storage).
- **Per-slot physical-copy allocation** and deck versions — the "Option C"
  reified-decklist model, **deliberately rejected** (maintainer decision
  2026-07-15): tracking *which physical copy* fills *which slot* is not needed —
  aggregate per-board quantity is sufficient ("2 Bolts main + 1 side; you own 4,
  3 are in this deck"). Boards therefore live as a quantity-bearing column on the
  existing holding/desire rows, not on a new slot entity.
- Tag-driven sharing, export, and cross-deck "saved-search" smart tags (future).

## Design

### Two annotation shapes — why boards ≠ tags

The assigned metadata splits by **cardinality**, and that split drives the
storage:

- **Partition — boards** (`main` / `side` / `maybe`). Each *copy* belongs to
  exactly one board, and it is **quantity-bearing**: a card's copies split across
  boards (2 Pithing Needle main, 1 side). A partition with quantities is exactly
  what the holding/desire grain already models, so boards become a **column in
  that grain**, not a separate structure.
- **Labels — tags** (`commander`, `companion`, `Draw spells`, …). Many-to-many,
  boolean (present / absent), whole-card. A card carries several at once. These
  are a classic tag table.

Trying to force boards into the label model loses the quantity split; trying to
force labels into columns loses open-endedness. So we model each as what it is.

### Boards — a column on `holdings` and `desires`

```sql
CREATE TYPE card_board AS ENUM ('main', 'side', 'maybe');   -- deck boards; 'main' = mainboard
```

`board card_board NOT NULL DEFAULT 'main'` is added to **both** `holdings` and
`desires`, and joins each table's uniqueness key so the same card can hold
distinct per-board quantities:

- `holdings` unique → `(collection_id, printing_id, finish, condition, language, board)`
- `desires`  unique → `(collection_id, oracle_id, printing_id, board)` (still
  `NULLS NOT DISTINCT`)

Notes:

- **Meaningful only for decks.** Binders keep every row at the `'main'` default;
  the API never surfaces a board control for `kind = 'binder'`.
- **Owned / present aggregates are unaffected** — they sum across boards. Board
  narrows a *within-deck* view; it never changes how many copies you own or how
  many are present in the deck (that's still the sum over its boards).
- A **board change is not a move** between collections — it re-labels copies in
  place (a `holdings`/`desires` update at a quantity, splitting the row when only
  part of a stack changes board). The `moves` ledger is untouched.

### Tags — scoped definitions

```sql
CREATE TABLE tags (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- NULL = system/built-in
    collection_id uuid REFERENCES collections(id) ON DELETE CASCADE,        -- non-NULL = deck-scoped
    name          text NOT NULL,
    builtin       text,        -- stable slug for system tags ('commander','companion'); NULL for user tags
    color         text,        -- optional UI accent
    created_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (collection_id IS NULL OR user_id IS NOT NULL),                 -- a deck tag is owned
    CHECK (builtin IS NULL OR (user_id IS NULL AND collection_id IS NULL)) -- builtins are system-scoped
);

-- scope-unique names (partial unique indexes, one per scope):
CREATE UNIQUE INDEX tags_system_name  ON tags (name)                WHERE user_id IS NULL;
CREATE UNIQUE INDEX tags_account_name ON tags (user_id, name)       WHERE user_id IS NOT NULL AND collection_id IS NULL;
CREATE UNIQUE INDEX tags_deck_name    ON tags (collection_id, name) WHERE collection_id IS NOT NULL;
```

**Scope is derived from the two nullable FKs** (no separate enum to keep in sync):

| Scope | `user_id` | `collection_id` | Visible to | Applies in |
|---|---|---|---|---|
| **system** (built-in) | NULL | NULL | everyone | any collection |
| **account** | set | NULL | that user | any of the user's collections |
| **deck** | set | set | that user | only that collection |

### `card_tags` — assignments

```sql
CREATE TABLE card_tags (
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    oracle_id     uuid NOT NULL REFERENCES cards(oracle_id),
    tag_id        uuid NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    user_id       uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- denormalized (RLS)
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, oracle_id, tag_id)
);
CREATE INDEX card_tags_tag_idx        ON card_tags (tag_id);                 -- "which cards carry tag X"
CREATE INDEX card_tags_collection_idx ON card_tags (collection_id, oracle_id);
```

- **Anchored at `(collection_id, oracle_id)`** — the card, in the deck — so a tag
  spans `holdings` *and* `desires` and survives a card going from desired to held.
  This is the same "whole-card within a collection" grain as the derived
  groupings, so tags and groupings compose in one view.
- **`user_id` denormalized** for a direct RLS policy, consistent with the other
  user tables (data-model's execution decision).
- **Membership is app-enforced.** There is no single "card is in this collection"
  table (it's the union of holdings + desires — Option C, which would have been
  that table, was rejected), so `card_tags` FKs only to `cards(oracle_id)` for
  card validity; the API ensures the card is actually in the deck when tagging and
  removes a card's `card_tags` rows when its last holding **and** desire leave the
  deck.
- **Deck-tag containment** (a deck-scoped tag is only applied within its own
  collection: `tags.collection_id = card_tags.collection_id`) is app-enforced —
  no cross-table CHECK is possible.

### Built-in tags & their semantics (app-enforced)

Seeded as **system** tags (`builtin` slug set); their rules live in
[collection-api](collection-api.md), not the schema — same "enforced in app"
stance data-model already takes for commanders:

- **`commander`** — ≤ 2 per deck (partners / Background / Doctor's-companion),
  must be a legal commander, and **defines the deck's color identity**. This
  replaces `deck_commanders`. The tag is oracle-grain; the *printing* shown as the
  commander (art) comes from that card's holding/desire in the deck, or a default
  printing when it is neither — see Open questions.
- **`companion`** — ≤ 1; the full deckbuilding-restriction check is deferred to
  the rules layer (the tag + the ≤1 cap are the schema-adjacent part).

User tags (account or deck scope) carry no semantics — they are pure labels.

### Row-level security

Both new tables get RLS **enabled + forced**, like every user table:

- **`card_tags`** — the standard direct-owner policy on the denormalized
  `user_id`.
- **`tags`** — two policies so **system tags are world-readable but not
  user-writable**:
  ```sql
  CREATE POLICY tags_read  ON tags FOR SELECT
      USING (user_id IS NULL OR user_id = current_setting('app.user_id', true)::uuid);
  CREATE POLICY tags_owner ON tags FOR ALL
      USING (user_id = current_setting('app.user_id', true)::uuid)
      WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);
  ```
  Postgres OR-combines permissive policies, so `SELECT` sees *own + system* while
  `INSERT/UPDATE/DELETE` are restricted to *own* (a `NULL` `user_id` fails the
  `tags_owner` `WITH CHECK`). System tags are therefore seeded by the migration
  **owner before `FORCE` is applied** (otherwise the owner, itself RLS-forced,
  couldn't insert a `user_id IS NULL` row).

### Migration

One new numbered migration (continuing data-model's sequence, expected `0006`),
run via [`scripts/migrate.sh`](../scripts/migrate.sh):

1. `CREATE TYPE card_board`; add `board` to `holdings` and `desires` (default
   `'main'`, existing rows unaffected); swap each uniqueness key to include
   `board`.
2. `CREATE TABLE tags`; **seed** the `commander` + `companion` system rows;
   `CREATE TABLE card_tags`; indexes.
3. Enable + **force** RLS and create the policies (after the seed).
4. **Grant** — because migration `0005` made `app_runtime` read-only by default,
   the new user tables need explicit write: `GRANT INSERT, UPDATE, DELETE ON tags,
   card_tags TO app_runtime` (SELECT comes from the default). This is exactly the
   convention `0005` established: user-table migrations grant CRUD explicitly.
5. `DROP TABLE deck_commanders` — it is empty, so the drop is clean.

Backward-compatible: `board` defaults on existing rows; no data is lost.

### Endpoint surface (detailed in collection-api)

The operations are new `CollectionStore` methods projected to HTTP by
[collection-api](collection-api.md); listed here for completeness:

- **Tag CRUD** — create / rename / delete an **account** or **deck** tag; list the
  tags in scope for a collection (system + account + that deck's).
- **Assignment** — add / remove a tag on a card in a collection; read a card's
  tags; read a deck's cards grouped by a tag (or by a built-in like `commander`).
- **Board** — set / change a card's board within a deck (quantity-preserving
  `holdings`/`desires` update, splitting a stack when only part changes board).
- **Commander / companion** — assigned through the tag-assignment endpoint using
  the built-in tags; the API enforces the ≤2 / ≤1 caps and recomputes color
  identity.

## Open questions

- ~~Board default for binders.~~ **Resolved 2026-07-16 (`0006`):** `board
  card_board NOT NULL DEFAULT 'main'` on both `holdings` and `desires` — a
  placeholder the API ignores for binders, keeping the uniqueness keys simple and
  existing rows migrating without a rewrite.
- ~~**Commander's displayed printing** when the commander card is neither held
  nor desired in the deck.~~ **Resolved (tag/board API):** the `deck_commanders`
  read returns a representative image via `image_uris->>'normal'` from *any*
  printing of the oracle (`LIMIT 1`) — the tag is oracle-grain, so the render
  just needs a stable picture; picking a "preferred" printing is a later UI
  refinement, not an API shape. *(resolved during execution — collection-api)*
- ~~**Companion enforcement depth.**~~ **Resolved (tag/board API): cap-only for
  v1.** `assign_tag` enforces the ≤ 1 companion (and ≤ 2 commander) caps; the full
  companion deckbuilding-restriction validation — like full legal-commander
  validation — is deferred to a rules-engine layer (`builtin_cap` is the
  schema-adjacent part). *(resolved during execution — collection-api)*
- ~~**Tag cosmetics & ordering.**~~ **Resolved (tag/board API):** `color` ships
  on `NewTag`/`Tag`; a user-defined tag **ordering** (`position`) and per-tag icon
  stay deferred until the UI needs them. List order is deterministic (built-ins
  first, then by name). *(resolved during execution — collection-api / ui-design)*
- **Typed / key-value fields.** The examples are all label-shaped (`Draw spells`),
  so tags are modeled as labels, not key-value fields. If a real need for typed
  per-card fields appears, it's a future extension, not a v1 shape change.
  *(deferred — a future spec)*
- **Cross-deck "smart" tags** (a saved catalog-search materialized as a tag) —
  out of scope; derived groupings + [catalog-search](catalog-search.md) cover the
  live case. *(deferred — a future spec)*

## Findings (schema migration — 2026-07-16)

The schema half shipped as [`migrations/0006_card_tagging.sql`](../migrations/0006_card_tagging.sql),
applied to the Neon **dev** branch via `scripts/migrate.sh dev`. The tag/board
**API** half is a separate queued task; this spec stays `accepted` (not
`implemented`) until that lands.

- **Boards** — `card_board` enum; `board NOT NULL DEFAULT 'main'` added to
  `holdings` and `desires`, each existing UNIQUE constraint swapped for one
  including `board`. The old inline constraints had auto-generated (truncated)
  names, so the migration drops them by lookup (`pg_constraint` where `contype='u'`,
  one per table) rather than a guessed literal — verified the new keys are
  `(collection_id, printing_id, finish, condition, language, board)` and
  `(collection_id, oracle_id, printing_id, board)`.
- **Tags** — `tags` + `card_tags` created; the two built-in system tags
  (`commander`, `companion`) seeded **before** `FORCE ROW LEVEL SECURITY` (an
  RLS-forced owner can't insert a `user_id IS NULL` row). RLS enabled + forced on
  both; policies `tags_read` (SELECT: own + system), `tags_owner` (ALL: own only),
  `card_tags_owner` (ALL: own).
- **Grants** — `app_runtime` got `SELECT` on both tables **automatically from the
  `0005` default-privilege flip** (confirmed: `has_table_privilege` SELECT = true
  without an explicit grant); the migration adds only `INSERT/UPDATE/DELETE`, per
  the convention `0005` established.
- **`deck_commanders` dropped** (was empty).
- **Verified end-to-end on dev** (rolled-back probe as `app_runtime`): system tags
  are visible (2), a user can insert an **account** tag, and a write to a **system**
  tag (`user_id NULL`) is blocked by `tags_owner`'s `WITH CHECK` — the dual-policy
  behaves as designed. `card_tags` reuses the proven denormalized-`user_id` owner
  policy.
- **Prod** not migrated — rides the data-access-backends cutover with `0002`–`0005`
  (same expand-first reasoning; see data-model Findings).

## Findings (tag/board API — 2026-07-16)

The API half shipped, completing the spec (`accepted` → `implemented`). Eleven
new `CollectionStore` methods, each a `shared/` DTO implemented once by
`HostedBackend` (sqlx) and mirrored by `NativeBackend` (the HTTP client of the
hosted JSON routes), projected to the routes in collection-api §Tags & boards.
The whole surface was proven end-to-end against the Neon **dev** branch (a temp
driver seeded a set/three cards/printing, exercised every path, asserted, and
cleaned up — reverted, not committed).

- **New DTOs (`shared/tags.rs`):** `Tag` + `TagScope` (`System`/`Account`/`Deck`,
  derived from the two nullable FKs via `TagScope::from_fks` — no scope enum to
  keep in sync), `NewTag`/`RenameTag`, `TagAssignment` (carries collection +
  oracle + tag, so assignment isn't a deeply-nested path), `TaggedCard`,
  `DeckCommanders`, `SetBoard`, and `union_color_identity` (WUBRG-ordered,
  de-duplicated; unit-tested).
- **Tag CRUD.** `create_tag` inserts an **account** (`collection_id = None`) or
  **deck** tag; `builtin` stays NULL (the API never mints system tags); a
  scope-duplicate name trips a partial unique index → `Conflict`. `rename_tag`/
  `delete_tag` carry `AND builtin IS NULL` and rely on `tags_owner` RLS, so a
  built-in or another user's tag reads as `NotFound`; delete cascades `card_tags`.
  `list_tags(collection)` returns system + own-account + this-deck's, built-ins
  first then by name.
- **Assignment enforcement** (`assign_tag`, all inside the `SET LOCAL app.user_id`
  tx, order matters): owned-collection guard → tag visible (else `NotFound`) →
  **deck-tag containment** (a deck-scoped tag only in its own collection, else
  `Conflict`) → **card-in-deck** (a holding *or* desire exists, else `Validation`)
  → **built-in caps** (`commander` ≤ 2, `companion` ≤ 1; counts *distinct* oracles
  already carrying the built-in, **excluding the one being assigned**, so
  re-assigning an existing commander stays idempotent) → upsert `ON CONFLICT DO
  NOTHING`. `unassign_tag` is an idempotent delete. Full legal-commander /
  companion-restriction validation is deferred (rules engine) — see OQs.
- **Reads.** `card_tags(collection, oracle)`, `cards_with_tag(collection, tag)`
  (the "group a deck by a tag" view), and `deck_commanders(collection)` — the
  latter returns the commander-tagged cards **plus the color identity derived on
  read** (the WUBRG union of their `color_identity`); nothing is stored, so it is
  always current after an assignment change (this is what "recompute color
  identity" means here — there is no color-identity column).
- **Boards** (`set_holding_board` / `set_desire_board`, by row id like
  `set_holding_quantity`): a quantity-preserving re-label, **not** a `moves`
  entry. It upserts the destination-board row (`ON CONFLICT ON CONSTRAINT
  holdings_uniq`/`desires_uniq` — the board is in each key, so source and dest are
  distinct rows) then decrements/deletes the source (`take_or_delete_*`), splitting
  a partial stack and merging into an existing destination row. `quantity = None`
  moves the whole row. A **binder** holding/desire is rejected (`Validation`,
  "boards apply to decks only") — the "meaningful only for decks" rule enforced at
  the API, not just hidden in the UI.
- **Orphan cleanup** (a card's `card_tags` removed when its last holding **and**
  desire leave the deck) is carried by the schema's `ON DELETE CASCADE` on
  `card_tags.collection_id` for whole-collection teardown; per-card orphan cleanup
  on the *last line* leaving rides the holdings/desires write paths and is a thin
  follow-up if a UI surfaces stale tags (filed).
- **Native mirror + routes.** Routes are operation-named/RPC-ish, mounted only in
  the hosted deployment; paths live in the shared `paths` module (`/api/tags`,
  `/api/tags/assign`, `/api/tags/{id}/rename`, `/api/collections/{id}/tags`,
  `/api/collections/{id}/cards/{oracle}/tags`, `/api/holdings/{id}/board`, …). A
  `mount_has_no_route_conflicts` unit test guards the mixed static/param forms.
  Full merge gate green in-container.
