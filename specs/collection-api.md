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
  **`NeedRow` grain = `(oracle, board)`**, the same grain as `CardRow`, so a
  mainboard copy cannot cancel a sideboard want (P6-074). `locations` stays
  board-blind (a copy elsewhere fills any board's need), and the pool behind it
  is **apportioned** across a card's board rows rather than offered whole to
  each — see the Findings entry.
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
  destination, one transaction, N move rows. Each item carries its own
  `quantity` (`MoveItem`), and the UI adapter over it (`move_selection`) is
  where a tray entry becomes one: an entry resolving to a stack of exactly one
  copy moves unasked, and anything larger is refused per entry so the
  which-copies picker can ask **how many and which version** (P6-150 ruling,
  2026-08-15 — app-ui.md → Selection tray). A picked entry names the full grain
  and a count; the server **validates that count against the caller's real
  ungrouped holdings** for that stack and refuses that entry politely if it no
  longer fits — never a clamp, never a whole-batch failure.
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

- 2026-08-09 — **Collapsed the four "owned per card" definitions onto
  `owned_by_card`** (P6-039, prep for [collection-deletion](collection-deletion.md)'s
  `deleted_at IS NULL` filter, which now only has to land once). `hosted.rs`
  had **four definitions total, counting the view itself**: the canonical
  `owned_by_card` view (read via the shared `owned_by_oracle` helper, backing
  `card_summary`/`search`) plus **three inline copies** that each re-derived
  "sum holdings quantity, joined through printings, grouped by oracle id" by
  hand — `collection_tree`'s shopping-short badge, `all_cards`, and
  `shopping_list`. All three now select `oracle_id, owned FROM owned_by_card`
  instead. `collection_view`'s `held` CTE (near its own
  `LEFT JOIN owned_by_card`) turned out **not** to be a fourth inline copy —
  it is collection-scoped `present`-in-this-collection data joined to
  `printings` for its `oracle_id`, a different computation from the global
  `owned` aggregate, and was left alone (`collection_view` already read
  `owned_by_card` directly for its actual `owned` column). Added a structural
  regression test
  (`owned_definition_guard` in `hosted.rs`) that greps the file's own source
  (via `include_str!`, whitespace-normalized) for the unfiltered
  `holdings`⋈`printings` grouped-by-oracle idiom and fails `cargo test` if it
  reappears — no DB needed, so it runs in CI. The one surprise: the guard's own
  literal needle self-matched on first pass (`include_str!` includes the test
  itself), fixed by assembling the needle from two halves at runtime.

- 2026-08-10 — **`previous_location` now excludes undone moves** (P6-113):
  added `undone_at IS NULL` beside the existing live-collection filter, so a
  relocation reversed by `undo_one` (notably a delete's own moves after
  `undo_delete`, P6-190) can no longer decide a future `ReturnToPrevious`
  destination; pinned with a live-DB test that calls `previous_location`
  directly after a delete → undo cycle (`previous_location_ignores_an_undone_move`
  in `hosted.rs`).

- 2026-08-10 — **`pull_needs` plans and writes in one transaction** (P6-120).
  Before: the `pull_needs` server fn (`lib.rs`) composed three independently
  committed calls — `needs()`, a per-card `holdings_of_oracle()`, then
  `move_batch()` — so the plan could be built from rows the write no longer
  held by the time it ran (same-device reachable: per-row busy guards let two
  ticks overlap, per `app-ui.md`'s P6-119 Findings). Fixed with the read →
  plan → write-one-tx house pattern `delete_collection`/`delete_plan.rs`
  already established.

  **Planner/executor split, ported faithfully.** New
  `app/src/backend/pull_plan.rs` (hosted-only, same `#[cfg]` as
  `delete_plan`): `PullSnapshot` (fresh needs rows + a locked
  `oracle_id -> Vec<HoldingLine>` map), `plan_pull_needs` (pure — classifies
  each item `AlreadyThere` / `NoLongerNeeded` / `NoCopies` / pulled, exactly
  the loop the server fn used to run inline) and `oracle_ids_of` (the
  canonical lock-order helper, below). **The allocation arithmetic itself was
  not re-implemented** — `allocate`/`gap_of`/`dedupe`/`plan_pull` stay in
  `my::needs.rs` (unconditionally compiled, since the client's pick list
  calls them directly) and `pull_plan` imports them rather than duplicating,
  honoring that module's own "same function over its own fresh read" rule.
  Zero behavior delta versus the pre-P6-120 loop: same three skip reasons,
  same per-(oracle, source) dedup, same default-grain-first stack order.

  **`HostedBackend::pull_needs`** (new `CollectionStore` method) does the
  actual read → plan → write: `read_needs_rows` (extracted from `needs()`,
  now shared by both) supplies the fresh gap; then, for each **distinct**
  oracle id `items` names — in **sorted, not caller-supplied, order** — one
  `SELECT … FOR UPDATE OF h` locks every holdings stack of that card across
  every live collection (the same breadth `holdings_of_oracle` reads); then
  `plan_pull_needs` runs over exactly what got locked; then every planned
  line writes through the existing `holding_take`/`holding_add`/`append_move`
  trio (the same triple `delete_collection`/`teardown` use). One commit.
  `needs()` itself is now `read_needs_rows` + the `require_owned_collection`
  guard + commit — unchanged output, less code.

  **Locking.** Only the `holdings` row is locked (`FOR UPDATE OF h`), never
  the joined `printings` catalog row — first draft locked both by omitting
  `OF h` and (unrelated to that, but caught by the same first e2e run) hit a
  real bug: `HOLDING_COLS`'s bare `id` column is ambiguous once `printings`
  (which also has an `id`) is joined in, surfacing as "column reference `id`
  is ambiguous" — `holdings_of_oracle`'s own hand-qualified column list
  (`h.id`, `h.collection_id`, …) was copied instead of reusing the constant.
  **Canonical lock order** (`pull_plan::oracle_ids_of`, sorted distinct
  oracle ids): the deliberately-unfixed P6-114 finding is that `move_batch`
  locks rows in **caller-supplied** item order, so two concurrent batches
  whose item lists overlap but arrive in opposite order can deadlock. Free to
  close for this one new operation since it already has to enumerate the ids
  — sorted before the lock loop rather than iterating `items` as given. This
  is *not* the general cross-operation ordering machinery P6-114 itself
  declined to build (a `pull_needs` call can still theoretically deadlock
  against an unrelated `move_batch`/`move_holding` touching an overlapping
  row set in a different order — Postgres's own detector aborts one side with
  a retryable error in that case, same safety net as everywhere else in this
  file; not attempted to remove here).

  **Review round: the read/lock *order* mattered, not just "one
  transaction."** The first draft read the destination's fresh gap
  (`read_needs_rows`) *before* the lock loop — still inside one transaction,
  still an improvement on three, but not enough. Two overlapping pulls into
  the same destination sharing an oracle id would each read the *same* stale
  gap up front, then serialize on the lock loop (the second blocks until the
  first commits), but by then each had already fixed its own `want` from the
  pre-lock read — so both would plan against the original, larger gap and,
  once both committed, overshoot `desired` at the destination. Fixed by
  swapping the order: lock first, read the gap second. Because the lock query
  is bulk over the whole oracle (previous paragraph), a second overlapping
  pull cannot even *start* its own gap read until it has acquired those same
  rows, which cannot happen before the first pull commits — so the gap read
  is now guaranteed no older than the locks, not merely coincident with them
  in the same transaction. `pull_plan.rs`'s module doc was corrected to state
  this precisely ("every row a write might touch was locked before the gap
  that sized the plan was read") rather than the looser "the plan and the
  write are always looking at the same rows," which was true of the
  mechanism (one transaction) but not sufficient on its own to prove the
  guarantee.

  **The lock footprint is deliberately wider than the pre-P6-120 path, and
  that is a real tradeoff, not free.** The old composition's only lock was
  `holding_take`'s own — one row, at write time, exactly the stack being
  decremented. This method now locks **every live holdings row of each
  requested oracle, for the whole transaction** (bulk per oracle id, not
  filtered to the item's own `from_collection_id` — see the comment in
  `hosted.rs`), because that bulk breadth is what makes two overlapping
  `pull_needs` calls sharing a card serialize correctly (previous paragraph).
  The cost: `pull_needs`'s blocking surface against an unrelated `move_batch`
  (or another `pull_needs`) touching *any* holding of that same oracle, in
  *any* of the caller's collections, grows accordingly — a `move_batch`
  moving a wholly different stack of the same card, in a collection this
  pull never names, can now be made to wait behind it, where before the two
  operations' single-row locks would never have collided unless they named
  the literal same stack. Accepted as the shape of the fix rather than
  narrowed further (e.g. to only the `from_collection_id`s the items name):
  narrowing would silently reopen the pull-vs-pull race the bulk lock
  exists to close, for the source-collection-not-named case.

  **Endpoint.** No pull endpoint existed before this task (checked). Added
  `POST /api/collections/{id}/pull` (`op::PULL`), body = `Vec<PullItem>`,
  mirroring `batch_add`'s exact shape (destination id in the path, items
  whole in the body) rather than inventing a new top-level path. `native.rs`
  forwards the whole call in one POST — previously the native backend made
  the pre-P6-120 *three* separate HTTP round trips (via `needs`,
  `holdings_of_oracle`, `move_batch`), each its own hosted transaction, an
  even wider window than the in-process hosted case.

  **Wire types stayed in `app::my`, not moved to `shared`.** `PullItem` /
  `Pulled` / `PullOutcome` (and `Skipped`/`SkipReason` from
  `my::move_selection`) already crossed the wire as a Leptos server-fn's
  argument/return types before this method existed, without living in
  `shared` — same placement `move_selection`'s own `Skipped`/`SkipReason`
  already established for an analogous adapter. `backend::mod.rs` /
  `hosted.rs` / `native.rs` / `routes.rs` import them from `crate::my::needs`
  / `crate::my::move_selection` directly (`my` is unconditionally compiled,
  so this is available to every target `backend` builds for); `PullOutcome`'s
  JSON shape is byte-for-byte unchanged, so P6-119's client keeps working
  with zero changes to `my/needs.rs`'s UI call sites. `lib.rs`'s
  `pull_needs` server fn is now a thin wrapper: the `SELECTION_MOVE_MAX` cap
  check (mechanical, no DB) stays there, matching where `move_selection`'s
  identical cap check already lives — `move_batch`'s own hosted route has no
  such cap either (P6-123, a separately tracked gap, not touched here).

  **Who else calls `needs()`/`holdings_of_oracle()` — left alone.** `needs()`
  still backs `GET /api/collections/{id}/needs` (`routes.rs`) and the
  `collection_needs` server fn (`lib.rs`) — the needs page's own read.
  `holdings_of_oracle()` still backs `GET /api/cards/{id}/holdings`
  (`routes.rs`) and, unfixed here, `move_selection`'s server fn (`lib.rs`,
  ~1200): the selection tray's batch move has the **same** three-transaction
  shape (`holdings_of_oracle` read, then `move_batch` write, no lock
  carried between them) — same class of defect as this task, on a different
  operation. Out of this task's surgical scope; recorded rather than
  silently absorbed, no Workbook task filed per this task's own
  no-workbook-commands instruction.

  **Tests.** `pull_plan.rs` gained 7 unit tests: the three skip reasons
  (`AlreadyThere`/`NoLongerNeeded`/`NoCopies`), a multi-source allocation, a
  duplicate-item no-multiply case, `oracle_ids_of`'s sort+dedup, and the
  concurrency shape this task exists for —
  `a_source_with_fewer_copies_than_the_ask_plans_the_honest_partial`: a
  snapshot whose locked `holdings` (simulating another operation having
  already drained some stock between the pick list's own snapshot and this
  transaction's lock) holds fewer copies than the fresh allocation's `want`,
  asserting the plan reports the honest partial (`Pulled { copies: 1 }` off
  an ask of 2) rather than erring or moving more than is there. No new
  `#[ignore]`d live-DB test added — the existing `needs.spec.ts` e2e (which
  already exercises Pull, Pull-all, and P6-119's own partial-pull-via-an-out-
  of-band-drain scenario against the real dev branch) covers the unified
  endpoint end to end; adding a redundant live-DB unit test on top was judged
  not worth it (task said "only if it fits naturally; not required"). **The
  native `/api/collections/{id}/pull` route itself has no automated
  coverage** — the planner tests are pure (no HTTP), and `needs.spec.ts`
  drives the web UI, which calls `HostedBackend` in-process and never touches
  `native.rs`/`routes.rs`'s new wire path at all. That path only gets
  exercised by an on-device (Tauri/APK) smoke pass against a running hosted
  deployment, which this task did not run.

  **Verified:** `cargo fmt --all -- --check` clean; `cargo clippy -p app`
  clean (`-D warnings`) across `--features hosted --all-targets`,
  `--features native --all-targets`, `--features hydrate --target
  wasm32-unknown-unknown`, and both `component-bench` combos; `cargo test -p
  app --features hosted`: 284 passed (277 baseline + 7 new `pull_plan`
  tests), 4 ignored (DB-gated, untouched); `cargo test -p shared`: 34 passed
  (no wire types changed). e2e: `needs.spec.ts` full file, chromium
  `--workers=1`, 7/7, run twice for stability (both green) — including the
  P6-119 partial-pull test, which now exercises the *unified* endpoint
  rather than the three-transaction one it was originally written against,
  and still passes unchanged.

  **Surprise.** The ambiguous-`id` SQL error above only exists because of a
  half-step in porting `holdings_of_oracle`'s pattern: that method's own
  query already hand-qualifies every column for exactly this reason (it also
  joins `printings`), and the fix here is simply to have copied that
  qualification instead of reaching for the `HOLDING_COLS` constant (which is
  safe only in `move_holding`'s and the two `RETURNING` sites' single-table
  contexts). Caught immediately by the first e2e run against the real
  database — a `cargo test`-only pass would not have caught it, since no unit
  test exercises real SQL.

- 2026-08-10 — **Hardened `jsonb_array_elements(p.faces)` against a non-array
  shape** (P6-124, originally filed as a hard prerequisite of P6-108's full
  Scryfall bulk load — corrected below). Three sites in `hosted.rs` gated the
  face-extraction subquery on `p.faces IS NOT NULL` — `card_detail`'s
  printings query, `collection_view`'s card-row query, and the
  `REPRESENTATIVE_PRINTING_JOIN` const shared by
  `card_summary`/`search`/`all_cards`. That check only excludes SQL NULL: a
  scalar, an object, or the JSON literal `null` all pass it and still make
  `jsonb_array_elements` throw ("cannot extract elements from a
  scalar"/"…object"), erroring the *entire* query — every OTHER row on that
  catalog page too, not just the malformed one. Fixed by gating all three
  sites on `jsonb_typeof(p.faces) = 'array'` instead, so a malformed row's
  `face_image_uris` degrades to NULL and the rest of that row (and every
  sibling row) still renders. Pinned with a structural test,
  `faces_shape_guard` (the `owned_definition_guard`/`soft_delete_guard`
  allowlist convention — needles assembled at runtime so `include_str!`
  can't self-match): asserts every `jsonb_array_elements(p.faces)` call is
  wrapped in the `jsonb_typeof` CASE and that the old bare not-null gate is
  gone outright, not merely outnumbered.

  **Correction to the original task premise.** P6-124 was filed on the
  belief that a non-array `faces` was reachable via a large enough ingest —
  "unreachable today only because the POC subset happens not to produce
  one," implying the 116K-row full load (P6-108) would eventually surface
  it. That's mistaken, checked directly against the ingester:
  `SourceCard.card_faces` is typed `Option<Vec<Value>>`
  (`app/src/ingest/extract.rs:63`) — serde rejects any Scryfall card object
  whose `card_faces` field isn't a JSON array before extraction ever runs —
  and `subset_array`, the function that produces `printings.faces` from it,
  always returns `Value::Array`. A non-array `printings.faces` is
  **structurally impossible via the ingester at any input size**; the full
  bulk load cannot surface this shape, so this guard is not actually a
  bulk-load blocker. What it *is*: defense-in-depth against **out-of-band
  writes** — manual `psql`, a future ingester bug, a hand-run fixture — the
  only route that can put a non-array value in `printings.faces` today.
  Three things worth being honest about in that light:
  - (a) **No CHECK constraint or ingest-side assertion enforces the array
    shape** — `printings.faces` (migration `0002_catalog.sql`) is bare
    `jsonb`, so this read-side guard is the *only* barrier, and a bad row
    still degrades silently (NULL `face_image_uris`, no alert raised)
    rather than failing loudly. Accepted for now given the shape is
    structurally unreachable through the one write path that exists
    (ingestion); revisit if a second writer (a hand-authored fixture, an
    admin tool) is ever added.
  - (b) **`cards.card_faces` has the identical out-of-band exposure and is
    deliberately left unguarded**, because every site that reads it was
    checked and is non-erroring on a shape mismatch regardless: the
    generated search column uses
    `jsonb_path_query_array(card_faces, '$[*].oracle_text')` (migration
    `0008_search_indexes.sql`), and `search::sql` uses
    `coalesce(c.card_faces, '[]'::jsonb) @> …` plus another
    `jsonb_path_query_array` call — jsonb `@>` never errors on a shape
    mismatch (it evaluates false), and Postgres jsonpath's default lax mode
    silently wraps/skips non-array, non-object operands instead of raising.
    `hosted.rs`'s own reads of `card_faces` never unnest it at all (`CASE
    WHEN … THEN c.card_faces END`, a passthrough), so there is no
    `jsonb_array_elements`-shaped hazard on that column to guard against.
  - (c) The live-verification follow-up this entry originally promised for
    `collection_view`'s copy of the guard is **dropped, not filed**: its
    only value was closing a gap before the bulk load, and the premise that
    motivated it collapsed. The SQL is byte-identical to the two sites that
    *were* live-verified below, and the only scenario it would catch is the
    out-of-band one (a) already names as an accepted risk.

  **Live-DB proof** (Neon dev, as `app_runtime` — catalog reads carry no RLS,
  so no `scoped_tx`/GUC is needed here, unlike the collection-table checks
  this pattern follows). Inserted a fully scratch card + printing (fresh
  UUIDs, an existing set for the FK, `faces = '"oops"'::jsonb` — a JSON
  string scalar) as `neondb_owner`, simulating exactly the out-of-band write
  this guard now exists for. On the pre-fix SQL (byte-identical to the
  `card_detail` and `REPRESENTATIVE_PRINTING_JOIN` literals): both the
  scratch card's own printings query, and the lateral-join pattern run across
  the scratch card *plus* a real `transform` card (Ral, Monsoon Mage //
  Leyline Prodigy), errored outright (`cannot extract elements from a
  scalar`) — the transform card's own well-formed row never got a chance to
  render, which is the blast-radius claim. On the fixed SQL both queries
  returned successfully: the scratch row's `face_image_uris` is NULL (its
  `image_uri`/`finishes` degrade too, as expected for a card with no real
  image data) while the transform card's two-element `face_image_uris` array
  is untouched. Cleanup: deleted both scratch rows as owner; `cards`/
  `printings`/`sets` counts returned to the pre-test 2637/2976/1045 and a
  `jsonb_typeof(faces) <> 'array'` sweep over `printings` returns 0.

- 2026-08-12 — **Ownership decoration degrades instead of 500ing** (P6-135,
  was P6-038a). `search`/`card_summary`/`card_detail` already have their public
  rows in hand when the session-scoped ownership read runs; a signed-in reader
  whose ownership read fails now gets `owned`/`ownership` as `None` (the
  anonymous shape, logged server-side) instead of a 500 — see `degrade_or` in
  `hosted.rs`.

- 2026-08-12 — **Board-aware needs** (P6-074). The needs computation was the one
  board-blind read left in an otherwise board-aware app: `holdings` and
  `desires` both carry `card_board` in their uniqueness keys and the collection
  view's card rows already aggregate per `(oracle/printing, board)`, but
  `read_needs_rows` summed desires and present-here by **oracle alone**. A deck
  holding a card on `main` and wanting one on `side` therefore had its want
  arithmetically cancelled by the mainboard copy: the deck page rendered a
  Sideboard row reading `WANTED 1 / HERE —` while the needs chip was absent and
  `GET /api/collections/{id}/needs` returned `[]`. The two halves contradicted
  each other on the same screen.

  **Grain alignment.** `NeedRow` gains a `board: Board` field, and the gap is
  computed per `(oracle, board)`: `d` (desires) and `ph` (present-here) group by
  `(oracle_id, board)` and join on **both** columns, so `desired > present_here`
  is evaluated per board.

  **One query, not two copies of it.** The chip's `totals` and the `/needs`
  page used to carry hand-copied versions of these CTEs, which made "the chip
  agrees with the page it links to" a matter of discipline — the first cut of
  this task had to change both in lockstep, and the pre-fix code carried a
  comment refusing to half-fix one of them. The review's fix removed the
  duplication instead: **`read_need_gaps`** is now the single read, and
  `collection_view` folds its rows through `fold_need_totals` while
  `read_needs_rows` decorates them with per-location offers. The header query
  keeps only `present` / `present_rollup` / `desired`. They cannot drift because
  there is nothing left to drift.

  **The elsewhere pool is board-blind, and is therefore APPORTIONED across a
  card's board rows — not applied whole to each.** `pe` and the `locations`
  query still group by oracle alone, because only copies *inside* this
  collection are committed to a board: a copy in the Trade Binder can fill
  **any** board's need. Partitioning the offers at the source was rejected —
  nothing in the data assigns a binder copy to a board, so any split would be
  invented. But applying the *whole* pool to each row independently (the first
  cut) was worse than invented, it was wrong: want 1 on `main` and 1 on `side`,
  hold none, one copy in a binder, and both rows claimed `owned_elsewhere: 1`
  with `short: 0`. The chip read "2 missing — 2 owned elsewhere" with the to-buy
  clause dropped entirely and no Short bucket rendered, while `/my/shopping`
  (per-oracle, and correct) said one to buy — two surfaces contradicting each
  other about one card. The pick list offered two pullable lines against one
  copy, and the second tick round-tripped into a misleading skip.

  The rule, in `my::needs::apportion_elsewhere` (pure, and the only
  implementation — both hosted call sites go through it): **greedy in row
  order**, earlier rows filling first and later rows seeing only the remainder.
  The order is the read's canonical `ORDER BY c.name, d.board`, which is also
  the order the page renders and the pick list walks, so the read side and the
  write side spend the pool identically. `card_board` is declared
  `('main','side','maybe')`, so the mainboard fills before the sideboard,
  matching the UI's `BOARD_ORDER`. Consumption is tracked per oracle id rather
  than by assuming rows of one card are adjacent, so two cards sharing a name
  cannot perturb each other. Invariants, unit-tested rather than asserted:
  `Σ owned_elsewhere ≤ pool` per oracle; `Σ short == max(0, Σ gap − pool)` per
  oracle (the greedy fill spends exactly `min(Σ gap, pool)`, so nothing is lost
  or invented); and a single-board oracle gets `min(gap, pool)` — byte-identical
  to the pre-review behaviour, which is what keeps every existing binder case
  unchanged.

  **The offers a row shows are capped by its apportioned share**, via
  `my::needs::offers_of` — one function, used by the pick list, the row's own
  Pull button and the server's planner, so the three cannot disagree. It
  allocates `owned_elsewhere` rather than `gap_of(row)`, which restores the
  identity the pick list rests on (`sum(offers_of(row)) == row.owned_elsewhere`)
  and means a row whose share came out zero offers nothing at all — it is a
  Short row and the page already filters it out of the Owned-elsewhere bucket on
  that same number.

  **A bounded residual, recorded rather than papered over:** apportioning fixes
  the *totals*, not the per-*location* split. `offers_of` walks `locations` from
  the front for each row independently, so a card with 2 in binder A and 1 in
  binder B, wanted 2 on `main` and 1 on `side`, names A on both rows — 3 copies
  asked of a stack holding 2. Fixing that would need a location allocation
  stateful across an oracle's rows, which `ElsewhereRow` (which renders one row
  and knows nothing of its siblings) cannot do without the three call sites
  diverging — the drift `my::needs`' own module doc forbids. It is covered at
  write time instead, where it matters: `plan_pull_needs` tracks per
  `holdings.id` what earlier lines of the same plan already committed and plans
  each subsequent line against the decremented stacks, so the second board gets
  an honest partial or `SkipReason::NoCopies` rather than a write `holding_take`
  would reject mid-loop, aborting the whole pull. That is the same shape a stale
  pick list already produces and the toast already says. `dedupe`'s key is now
  `(oracle, board, from)`.

  **Pull board-landing rule: a pull lands on the board that wanted it.** This
  was a latent bug the board-blind read hid — `HostedBackend::pull_needs` passed
  a hardcoded `Board::Main` to both `holding_add` and the ledger's `to_board`, so
  a non-mainboard need could never have been closed by pulling; the copies would
  land on `main` and the same need would be re-offered forever. `PullItem` now
  carries the **destination** board (the `NeedRow`'s board) and `PullWrite`
  carries it through as `to_board`. The *source* board is unchanged and still
  read off the locked holding, so the two ends of the move are independent and
  undo still puts copies back on the stack they left. `PullItem::token` grew the
  board too (`oracle@from@board`) — one card pulled from one binder onto two
  boards is two lines, and a token that omitted the board would make their
  per-line outcomes indistinguishable.

  **Wire compatibility.** `NeedRow.board` is a plain required field, not
  `#[serde(default)]`: both halves of the wire ship together (the native shell
  embeds the same `shared` crate it decodes with), nothing persists old-shape
  `NeedRow`/`PullItem` JSON, and no consumer of either carries
  `#[serde(deny_unknown_fields)]` (checked repo-wide — the six that do are
  `cards.rs`, `catalog.rs`, `catalog/destination.rs`, `quick_add.rs`,
  `all_cards.rs`, `my/collection.rs`, none of them needs types).

  **UI.** The needs page labels a row's board the way the deck page labels its
  sections: nothing at all for `main`, the board's name otherwise. The
  vocabulary is shared rather than duplicated — `my::collection::board_label`
  reads the same `BOARD_ORDER` table `group_deck` uses — so the two pages cannot
  call the same board different things. Rows also carry `data-board`, matching
  `collection-row`. The chip's arithmetic is unchanged apart from the grain: it
  sums per-row gaps, so a card missing on two boards contributes both.

  **Tests.** `pull_plan.rs` gained five unit tests (sideboard want pulls onto the
  sideboard; two boards are two lines with two tokens; two boards sharing one
  copy do not plan it twice; a second board takes what the first left; two boards
  naming the same binder cannot overdraw it) and `my/needs.rs` nine (pick line
  carries the board; two boards are two pick lines; `dedupe` keeps both boards
  while still collapsing a repeat; the chip counts a sideboard want a mainboard
  copy used to cancel; plus the five apportioning tests — one copy cannot satisfy
  two boards, a single-board oracle is unchanged, the two invariants table-tested
  across pool-larger/smaller/equal/zero and interleaved oracles, the chip says
  "1 to buy" on the shared-copy case through the real formatter, and offers never
  exceed a row's share). The planner's multi-board fixtures go through an
  `apportioned()` helper that mirrors what `read_needs_rows` builds, so the
  planner is not tested against snapshots the read cannot produce.
  `cargo test -p app --features hosted`: 319 passed.

  The e2e that **pinned the old decision** (`needs.spec.ts` → "a want on the
  sideboard … is not a need") was inverted rather than deleted — it now asserts
  the need, the chip, `board: "side"` on the wire, the `Sideboard` label on the
  row, and that pulling it lands the copy on the sideboard with the mainboard
  copy untouched. A second e2e covers the apportioning end to end: want 1 on
  each of two boards against one elsewhere copy, asserting the two rows'
  `owned_elsewhere` are 1 and 0, that the chip names one to buy, that the pick
  list offers one line rather than two, and that `/my/shopping` agrees with all
  of it. `states.spec.ts`'s empty-state assertion moved from "board slots" to
  "between boards" for the same reason as the page text: the old caveat
  ("Unfilled board slots aren't counted here") became false.

- **`LineResult::Error` could never serialize before P6-083's retagging
  (2026-08-13).** `shared::ApiError`'s internally-tagged serde repr panicked at
  serialize time for any variant with content, so `POST /api/collections/{id}/batch`
  returned an opaque 500 the moment any per-line outcome was an error — masked
  because the only caller is `app/src/seed.rs` (no UI path). P6-083's switch to
  adjacent tagging (`tag = "code", content = "message"`) repairs it as a side
  effect and changes that route's per-line error JSON shape; native artifacts
  built before P6-083 never saw the old shape work, so nothing can regress.

- **Batch move carries a per-item quantity the server validates (P6-150,
  2026-08-15).** `MoveItem.quantity` was already on the wire and already honored
  by `move_batch`/`append_move`/`undo_one`; what changed is that the UI adapter
  over it (`move_selection`) stopped fixing it at 1. A tray entry with no
  picker answer moves only when the stack it resolves to holds exactly **one**
  copy, and is otherwise refused per entry so the which-copies picker can ask
  how many and which grain; a picked entry names
  `(finish, condition, language, quantity)` and is re-resolved against a fresh
  `holdings_of_oracle` read. The three quantity refusals are per entry and never
  fatal to the batch: over the stack's real size, zero requested, and a grain
  that emptied between the ask and the submit. **No clamping** — the number the
  dialog showed is the number that moves, or nothing moves for that entry and
  the toast says why. Undo is unchanged and still reverses the ledger's own
  recorded quantities in one transaction (`undo_moves`), which is what makes a
  3-copy move undo as 3 copies rather than 1.
