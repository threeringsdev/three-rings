# Collection API

**Status:** implemented
**Depends on:** [data-access-backends](data-access-backends.md), [data-model](data-model.md), [auth](auth.md)

[data-access-backends](data-access-backends.md) owns the trait split and the
`shared/` types crate this spec's DTOs live in; this spec's endpoints *are* the
wire projection of that spec's `CatalogStore`/`CollectionStore` methods. The
reverse coupling — data-access's native store impl is a client of these
endpoints — stays prose there, not a `Depends on:`. So the dependency is
one-directional (**collection-api → data-access-backends**) with no gating
cycle.

## Problem

Clients need endpoints to search the catalog and manage a personal collection —
the HTTP surface the Leptos UI (hosted) and the native app (via its HTTPS
backend) both call. The dependent specs already pin most of the shape: the
concept model and derived counts ([ui-design](ui-design.md), [data-model](data-model.md)),
the public-vs-authed route split ([information-architecture](../design/information-architecture.md)),
and the trait seam this surface projects ([data-access-backends](data-access-backends.md)).
This spec defines the operations, their request/response types, and the
collection-side mechanics those specs deferred here.

## Scope

**In:** the HTTP endpoint surface (the wire projection of the
`CatalogStore`/`CollectionStore` trait methods), request/response DTOs, keyset
pagination, batch and error conventions, the anonymous-vs-session route split,
and the collection mechanics data-model/ui-design deferred to "collection-api"
(undo, reparent cycle prevention, inbox provisioning, needs/shopping-list
computation).

**Out:** the trait/impl split and the `shared/` crate mechanics
([data-access-backends](data-access-backends.md)); the query-syntax subset and
query↔rail contract ([catalog-search](catalog-search.md)); the ingestion
pipeline that populates the catalog ([catalog-ingestion](catalog-ingestion.md));
decks-beyond-basics, sharing, trade, and import/export (future specs). CSV
import specifically is deferred — see [Open questions](#open-questions).

## Design

### Shape: endpoints are the trait methods projected to HTTP

collection-api defines **operations and their DTOs, not a parallel REST resource
model.** Each `CatalogStore`/`CollectionStore` method is one JSON-over-HTTP
operation:

- The **hosted deployment** mounts an Axum router that deserializes a call and
  dispatches to the **sqlx (hosted) impl**.
- The **native impl is the HTTP client** of those same routes (forwarding the
  Better Auth `tr_jwt` as `Authorization: Bearer`; silent-refresh on `401` is
  data-access's).
- The **web UI's Leptos server functions call the trait in-process** — never
  over loopback — honoring data-access's "one terminus for data authorization"
  rule.
- Request/response types live in the **`shared/` crate owned by
  [data-access-backends](data-access-backends.md)**; this spec specifies their
  fields.

Routes are operation-named / RPC-ish (`POST /api/catalog/search`,
`POST /api/collections/{id}/have`). The exact path scheme is a thin convention
settled alongside the trait-split task; what is *contractual* is **one operation
per trait method with a stable DTO**, so the two backends cannot drift.

Two domains, matching the two trait/endpoint groups:

| Domain | Trait | Access | Operations |
|---|---|---|---|
| **Catalog** | `CatalogStore` | anonymous-safe | `search`, `card_detail`, `card_summary` |
| **Collection** | `CollectionStore` | session-scoped | tree CRUD, holdings/desires writes, moves, reads, inbox |

### Auth & the request envelope

- **Catalog endpoints** require no session, but read the `AuthUser` extractor
  **opportunistically** — when a caller is logged in, catalog reads attach the
  caller's ownership data (the card page's "your copies & locations" block).
- **Collection endpoints** require a session. The `AuthUser` extractor
  ([auth](auth.md)) yields the `sub` uuid (the Neon Auth / Better Auth user id);
  each request runs inside a per-request transaction that first does
  `SET LOCAL app.user_id = <sub>` so data-model's RLS policies apply as a
  backstop beneath this terminus. A missing or invalid session is `401`.
- The native client rides the same token — no bespoke native↔hosted token (see
  [data-access-backends](data-access-backends.md)).

### Pagination: keyset, both domains

Every large list endpoint uses **keyset (cursor) pagination** — an opaque,
URL-safe `cursor` encoding the last row's sort key(s) plus a stable tiebreaker
(the row uuid / printing id), with a `limit`. The response is
`{ items, next_cursor }`, where `next_cursor` is `null` at the end.

This applies to **catalog search *and* collection reads** (collection view, All
cards, needs, shopping list). Collection reads are **not** assumed small: a
serious collection can approach catalog scale (~100K rows), so it paginates like
the catalog. Per-endpoint sort keys are fixed so the cursor is well-defined:

- catalog `search` — name, then oracle id *(corrected 2026-07-16 at
  catalog-search acceptance: was "relevance, then name" — relevance ranking is
  keyset-hostile and Scryfall's own default sort is name; relevance is
  deferred to an `order:` extension, see [catalog-search](catalog-search.md))*;
- collection view — name (or set), then id;
- shopping list — shortfall desc, then name.

The opaque cursor keeps catalog searches shareable/restorable
(`/catalog?q=…&cursor=…`) and lets the server SSR the first page then hydrate
"load more" without re-running from offset 0.

### Read models (denormalized response DTOs)

The server joins collection↔catalog and computes the counts; clients receive
flat rows. **Counts are computed for the visible page, not the whole
collection** — the discipline that keeps a 100K-card view bounded.

- **`CardRow`** — a card entry in a collection view: oracle + printing render
  fields (name, set, collector number, image uri, mana cost, type line, colors),
  plus the three counts *in this context* — present (here), desired (here),
  owned (global aggregate). The portion of present rolled up from child
  collections is a distinct field so the UI can mark it.
- **`CardSummary`** — the hover/quick-preview subset: image, name, key info,
  your-copies count.
- **`CardDetail`** (`/cards/:id`) — full oracle + printings + rulings +
  `all_parts` relations (public, SSR-able, deep-linkable) plus an optional
  ownership block (your copies & locations) present only when authed.
- **`CollectionView`** — the collection's metadata + child collections + one
  keyset page of `CardRow`s + rollup counts. Decks additionally carry format,
  commander(s) (the `commander` **built-in tag** — see
  [card-tagging](card-tagging.md), which retires the `deck_commanders` table), and
  the needs chip summary (`6 missing — 4 owned elsewhere · 2 to buy`). `CardRow`
  carries its assigned **tags** and **board** in a deck context.
- **`AllCardsView`** — the virtual everything-view: aggregates across all
  collections incl. Inbox; present is replaced by a location summary per card
  (`7 across 3 collections`, expandable to per-location).
- **`NeedsView`** (per collection) — rows split into **Owned elsewhere** (with a
  per-location listing, e.g. `2 in Trade Binder`) and **Short** (to buy).
- **`ShoppingList`** (global) — one row per short card (shortfall count + which
  collections want it); **text-exportable**.

**Scale note.** data-model defines owned / present-rollup / shortfall / needs as
read-time computations and calls a personal collection "small enough that
on-demand aggregation is fine." With catalog-scale collections in play that no
longer holds at the top end. The endpoints here compute aggregates **per visible
page**; if per-user aggregates still prove hot, the remedy is the
`owned_by_card` **materialized view data-model already named as its escape
hatch**. Filed as a cross-spec follow-up (see [Findings](#findings)); your
correction makes it materially more likely than data-model assumed.

### Collection mutations

- **`+ Have` / `+ Want`** — single-card writes. Have upserts `holdings`
  (collection, printing, finish/condition/language) and appends an intake move
  (`from = NULL`); Want upserts `desires` (oracle grain, optional printing pin).
  The upsert increments quantity on the existing unique row.
- **Batch add** — one request carrying N add-lines (the time-to-enter-50-cards
  path, and playset entry via `⇧⏎` set-count). Returns a **per-line result
  vector** so one bad line doesn't sink the batch (chosen over all-or-nothing).
- **Edit present count** — set / increment / decrement a holding's quantity
  (the stepper); quantity 0 deletes the holding row.
- **Move (single)** — `from → to`, at printing + finish + condition + language
  grain, `quantity` (default = the destination's need, capped at present-here).
  One transaction updates both holdings and appends a `moves` row. A companion
  **suggested-destinations** read returns collections where desired > present
  for the card (the destination picker's ranking).
- **Move (batch)** — the persistent selection tray: N `(card, from)` pairs → one
  destination, one transaction, N move rows.
- **Undo** — **flag, not compensating row.** Reverse the holdings effect and
  stamp `undone_at` on the original move; the append-only ledger keeps the row,
  undo is idempotent, and history reads cleanly. Targets a specific move id (the
  toast) or the last move (⌘K "undo last move").
- **Pull / Pull-all** — Pull is a pre-filled single move from an owned-elsewhere
  source → one confirm. Pull-all generates a pick list grouped by source
  collection; checking an item records its move.
- **Teardown** — "Empty deck" moves everything to a chosen destination;
  "Return to previous locations" reads, per printing/finish, the most-recent
  move *into* a collection (via `moves_to_recent_idx`), falling back to Inbox
  where there is no history. Returns a destination-grouped **preview** before
  confirm.
- **Tree CRUD** — create (binder/deck), rename, delete, reparent, reorder
  (fractional `position`, a one-row write). **Reparent cycle prevention is an
  app-side ancestor walk** — reject if the target parent is a descendant of the
  moved node (a DB trigger is the named backstop if this proves fragile). The
  **Inbox** row (`is_inbox`) is undeletable and unrenamable — enforced here.
- **Inbox provisioning** — **lazy on first authed load.** The first `/my`
  request (All cards / collection list) ensures the user's one `is_inbox` row
  exists, made idempotent by the `collections_one_inbox` unique index. No
  webhook infrastructure; this resolves data-model's open question.

### Tags & boards (surface for [card-tagging](card-tagging.md))

**Gating note.** These operations belong to [card-tagging](card-tagging.md)
(`draft`, review 2026-07-15) and land with **its** task, not the base
collection-api endpoints task — they are listed here because collection-api owns
the wire surface, but they are *not* part of this spec's accepted surface until
card-tagging is accepted. card-tagging retires the `deck_commanders` table
(commander becomes a built-in tag).

New `CollectionStore` methods, projected to HTTP like the rest:

- **Tag CRUD** — create / rename / delete an **account**- or **deck**-scoped tag;
  list the tags in scope for a collection (system built-ins + the user's account
  tags + that deck's tags). Deleting a tag cascades its `card_tags`.
- **Assignment** — add / remove a tag on a card in a collection (anchored at
  `(collection, oracle)`); read a card's tags; read a deck's cards grouped by a
  tag (or by a built-in such as `commander`). The API enforces that the card is in
  the deck, that a deck-scoped tag is only applied within its own collection, and
  removes a card's tags when its last holding **and** desire leave the deck.
- **Board** — set / change a card's board within a deck (`main`/`side`/`maybe`):
  a quantity-preserving `holdings`/`desires` update that splits a stack when only
  part of it changes board. **Not a `moves` entry** — it re-labels in place.
- **Commander / companion** — assigned through the tag-assignment endpoint using
  the built-in tags; the API enforces the ≤ 2 / ≤ 1 caps and recomputes the deck's
  color identity from its `commander`-tagged cards.

### Error model

One error enum, defined here but **living in the data-access-owned `shared/`
crate**, that both impls map into — the hosted impl from DB/validation errors,
the native impl from HTTP status. Variants → status:

| Variant | Status | When |
|---|---|---|
| `NotFound` | 404 | unknown id |
| `Unauthorized` | 401 | missing/invalid session on a collection endpoint |
| `Forbidden` | 403 | RLS / ownership violation |
| `Conflict` | 409 | uniqueness, reparent cycle, inbox-protected op |
| `Validation` | 422 | malformed DTO / bad quantity |
| `Upstream` | 502 / 500 | DB or downstream failure |

Wire shape: `{ "error": { "code", "message", "details"? } }`; the native client
deserializes it back into the same enum. This proposes the shape that
[data-access-backends](data-access-backends.md) left open ("error-type
unification — one error enum both impls map into — shape TBD"); collection-api
owns the endpoint surface, data-access owns the crate the enum lives in.

### Search endpoint boundary

The search endpoint's **contract** is specified here — request params, keyset
pagination, and the result DTO — and its **backend is SQL against our ingested
catalog** (not a Scryfall proxy: collection features FK to `printings`, so our
catalog rows must exist regardless, and ingestion is the natural prerequisite).
It explicitly delegates:

- the query-syntax → SQL translation and the rail/query vocabulary to
  [catalog-search](catalog-search.md);
- the populated catalog to [catalog-ingestion](catalog-ingestion.md).

Those two are drafts; they are **not** hard `Depends on` here, so this spec can
be accepted without them done. The dependency is a sequencing note: the search
endpoint returns real results only once catalog-search defines the translation
and catalog-ingestion has loaded the catalog.

## Open questions

None blocks a maintainer's draft→accepted decision; each notes where it
resolves.

- **Route path scheme / verb conventions.** RPC-ish operation names over a thin
  path convention; the concrete routes are the trait methods' wire form, so they
  settle with the trait-split task. *(resolved during execution — the
  data-access-backends trait split)*
- **Live-search debounce / latency budget.** Shared with
  [catalog-search](catalog-search.md)'s identical open question; the endpoint
  must be fast enough for results-as-you-type (keyset + the trgm/base indexes
  data-model provides). *(resolved with catalog-search)*
- **Large-collection aggregate performance.** Whether owned / present-rollup at
  catalog-scale collections needs data-model's `owned_by_card` materialized view
  (and which collection-read indexes). Profile against real data.
  *(resolved during execution — the endpoint implementation; cross-spec
  follow-up filed against data-model)*
- ~~Pagination style (cursor vs. offset) for 100K-row catalog search.~~
  **Resolved:** keyset/cursor, applied to catalog search *and* collection reads
  (see [Pagination](#pagination-keyset-both-domains)).
- ~~CSV import (Moxfield/Archidekt formats) — v1 or later?~~ **Resolved:
  later.** No dependent spec requires it, ui-design lists import/export as out
  of scope (future spec), and it is parked in TODO's Later/parked. The only v1
  export is the shopping-list-as-text.

## Findings

- 2026-07-14 — **Spec fleshed out from draft (maintainer design session).**
  - **API shape: endpoints are the `CatalogStore`/`CollectionStore` methods
    projected to HTTP** (one operation + `shared/` DTO per method), not a
    separate REST resource model and not raw Leptos server-fn re-invocation —
    the only shape that honors data-access's already-accepted trait seam and its
    drift guarantee.
  - **Search backend: SQL against our ingested catalog**, not a Scryfall proxy —
    collection features FK to `printings`, so our rows must exist regardless.
    Query translation stays catalog-search's; catalog population stays
    catalog-ingestion's.
  - **Pagination: keyset, both domains.** Prompted by the maintainer's
    correction that **collections can approach catalog scale (~100K)** — so
    collection reads paginate like the catalog, and data-model's "collections are
    small, on-demand aggregation is fine" assumption does not hold at the top
    end. Aggregates are computed per visible page; the `owned_by_card`
    materialized view is the escape hatch if they stay hot.
  - **Smaller mechanics resolved:** undo = stamp `undone_at` (flag, not
    compensating row); reparent cycle prevention = app-side ancestor walk;
    inbox provisioning = lazy on first authed load; batch add = per-line
    results. These close the corresponding deferrals in data-model and ui-design.
  - **Error enum proposed** (variants + wire shape) to resolve
    data-access-backends' open "error-type unification" question; the enum lives
    in data-access's `shared/` crate.

- **Cross-spec follow-ups** (added to TODO, not silently absorbed):
  - data-model: profile large-collection aggregates; promote `owned_by_card` to
    a materialized view + add collection-read indexes if hot. (Its own named
    escape hatch, now more likely given catalog-scale collections.)
  - The `shared/` error enum defined here lands with the data-access trait-split
    task; data-access-backends' error-type OQ is annotated with a pointer to this
    spec's §Error model (done, 2026-07-14).
  - Dependency direction made canonical: **collection-api `Depends on:`
    data-access-backends** (one-way, for the `shared/` types + trait seam); the
    reverse endpoint-client coupling stays prose in data-access. No gating cycle.

- 2026-07-16 — **Implemented: the full endpoint surface, both backends, verified
  on dev.** Built as six verified slices (each proven end-to-end against the Neon
  dev branch, then the temp driver reverted): tree CRUD + lazy inbox; holdings/
  desires writes + batch; CollectionView reads; moves + undo/teardown/suggest;
  global reads (all-cards/needs/shopping); catalog (detail/summary/search shell).
  Every operation is a `CatalogStore`/`CollectionStore` trait method with a
  `shared/` DTO, implemented once by `HostedBackend` (sqlx) and once by
  `NativeBackend` (the HTTP client of the hosted JSON routes). Design decisions
  settled during implementation:
  - **Endpoint surface = the explicit hosted JSON routes** (`/api/collections…`,
    `/api/moves…`, `/api/cards/{id}…`, `/api/all-cards`, `/api/shopping-list`,
    `/api/catalog/search`), operation-named/RPC-ish, mounted only in the hosted
    deployment; the native client targets them. Route paths live in one shared
    `paths` module so client and router can't drift. **No per-operation Leptos
    server-fn wrappers were added** — those are a thin per-screen UI adapter over
    the same trait and ride with each UI task; the machine API (routes + trait +
    both backends + DTOs) is the deliverable here.
  - **`CardRow` grain = `(printing, board)`.** Present sums a card's copies across
    finish/condition/language within the collection; `owned` is the global
    per-oracle aggregate (the `owned_by_card` view); `present_rollup` sums
    holdings in the strict descendant collections (per printing, board-agnostic);
    `desired` is the per-(oracle, board) target, so it repeats on each printing
    row of that oracle (the UI shows it once). Keyset by (name, printing, board).
  - **Moves are board-agnostic** — the `moves` ledger has no board column, so
    `move_cards`/teardown act on the mainboard; board re-labels are
    [card-tagging](card-tagging.md)'s separate quantity-preserving op, not a move.
    `move_cards` decrements source / upserts dest / appends the ledger row in one
    tx; `from`/`to = None` model intake/removal; undo reverses the effect and
    stamps `undone_at` (idempotent); teardown snapshots then relocates all boards.
  - **Keyset pagination** on the potentially-large reads (collection view,
    all-cards, catalog search) via an opaque base64 (name, id[, board]) cursor
    with a `limit+1` probe; **needs and shopping-list return full lists** — they
    are derived and bounded in practice (keyset is a filed follow-up if profiling
    says otherwise).
  - **Catalog endpoints read the session opportunistically** (a valid bearer/
    cookie JWT yields the ownership block / owned counts, else anonymous public
    data) via a `HeaderMap`-based lookup — axum's `Option<AuthUser>` needs
    `OptionalFromRequestParts`, which the extractor doesn't implement.
  - **Ownership guard is load-bearing:** `holdings`/`desires` RLS gates only on
    their own `user_id`, not the collection's, so every write validates that the
    target collection is owned (RLS makes a non-owned one invisible → NotFound).
  - **`shared::ApiError`** carries the errors (FK miss → NotFound, unique →
    Conflict, CHECK → Validation, else Upstream); DB internals are logged, never
    shipped. sqlx gained the `uuid` + `json` features (native id + jsonb decode).
  - **Tags on `CardRow` deferred to card-tagging** — the `board` column is read
    here; tag *assignment* stays that spec's task (collection-api §Tags & boards
    was already gated to it).
  - **Follow-ups filed in TODO:** keyset for needs/shopping if they grow; the
    native `401` silent-refresh (data-access-backends' open item) now has real
    session endpoints to exercise it against.
