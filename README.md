# Three Rings

A cross-platform Magic: The Gathering card collection manager. Track your collection against the full card catalog, on desktop, mobile, and the web.

## What it does

Three Rings lets a user browse the shared MTG card catalog (~100K cards, routinely updated as new sets and price data land) and maintain a personal collection that references it — quantities, printings, conditions, and eventually decks and sharing.

## Architecture

**API-first, single database.** All clients are thin: they authenticate against a Rust API server, which owns all data access to a single Postgres database. There is no client-side database and no sync engine.

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│   Desktop   │  │   Mobile    │  │     Web     │
│Tauri+Leptos │  │Tauri+Leptos │  │Leptos (WASM)│
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘
       └────────────────┼────────────────┘
                 HTTPS / JSON API
                        │
              ┌─────────▼─────────┐        ┌──────────────┐
              │  Rust API server  │◄───────│ Catalog       │
              │   (Axum + sqlx)   │        │ ingestion job │
              └─────────┬─────────┘        │ (Scryfall)    │
              ┌─────────▼─────────┐        └──────────────┘
              │  Postgres (Neon)  │
              │ catalog + users + │
              │    collections    │
              └───────────────────┘
```

### Stack

| Layer | Choice | Notes |
|---|---|---|
| Frontend | [Leptos](https://www.leptos.dev/) | Shared UI across all three targets |
| Desktop & mobile shell | [Tauri](https://tauri.app/) | Rust core, native webview |
| API server | Rust (Axum + sqlx) | Single backend for all clients |
| Database | Postgres on [Neon](https://neon.com/) | Serverless, scale-to-zero, usage-based pricing |
| Auth | Session/JWT at the API layer | Clients never hold database credentials |
| Catalog ingestion | Scheduled Rust job | Pulls card data (e.g. Scryfall bulk) into the catalog tables |

## Architecture decisions and rationale

These choices came out of an explicit evaluation of offline-first designs on Turso vs. an API-first design on Neon (July 2026).

### Decision 1: API-first, no offline requirement

Offline support was the requirement driving all the complexity in earlier designs. Dropping it collapses the architecture: no client-side database, no sync protocol, no conflict resolution, no per-user credential minting.

**Trade-off accepted:** offline is an architecture decision, not a feature — retrofitting it later means rewriting the data layer, not iterating. This was decided deliberately. If limited offline catalog browsing is wanted later, a bundled read-only catalog file on desktop/mobile is a cheap add that doesn't reintroduce sync (see `specs/`).

### Decision 2: Single database, not per-user databases

Three options were evaluated:

1. **Per-user DB seeded with the catalog** — rejected. A routinely-updated 100K-row catalog would be written N times (once per user DB) on every update; cost and complexity scale linearly with users. Sharing features get harder, not easier.
2. **Single DB for catalog + all user data** — **chosen.** With an API server mediating all access, the drawbacks originally identified (custom sync engine, client-side data security) don't apply — they were artifacts of the offline requirement. Catalog updates are written once. Cross-referencing collection ↔ catalog is a server-side SQL join. Sharing features are straightforward since all data is co-located.
3. **Split catalog DB + per-user collection DBs** — was the right shape for the offline-first design, unnecessary without it.

### Decision 3: Neon over Turso

Turso's differentiators — offline sync (CDC-based push/pull), embedded replicas, cheap per-user databases — are exactly the features an API-first design doesn't use. What remains favors Postgres:

- Mature, boring technology vs. a beta ground-up SQLite rewrite (Turso's engine had real compat gaps at evaluation time: partial `schema.table.column` support, no `WITH RECURSIVE`, partial window functions).
- Better full-text search and indexing for card search across 100K rows.
- Row-level security available as defense-in-depth beneath the API's authorization.
- First-class Rust ecosystem (sqlx/diesel, Axum).
- Neon pricing suits prototype scale: free tier with 100 CU-hours/month, pay-as-you-go Launch plan with no monthly floor, $0.35/GB-month storage.

**Revisit trigger:** if full offline becomes a hard requirement again, the fallback evaluated was Turso Sync (per-user collection DB + distributed read-only catalog file) or Postgres + a sync layer like PowerSync/ElectricSQL.

### Decision 4: All-Rust stack retained

Leptos + Tauri + Axum keeps the entire codebase in Rust with shared types between client and server (one crate for API request/response types eliminates a class of drift bugs).

## Repository layout

```
├── README.md          # this file
├── specs/             # feature specs and project todos — see specs/README.md
└── (workspace crates to come: app/, server/, shared/)
```

## Status

Pre-implementation. Planning lives in `specs/`.
