# Project TODO — execution queue

**This file is the single source of truth for what to work on next.** The selection algorithm and definition of done are in [README.md](README.md) ("Working the queue"). State legend: `[ ]` available · `[~]` in progress · `[x]` done.

Phases execute top to bottom. Tasks within a phase execute top to bottom unless a stated prerequisite blocks.

## Phase 1 — architecture spike ([architecture-spike](architecture-spike.md))

- [ ] Compare scaffold bases (start-tauri-fullstack vs. tauri-leptos-ssr); record choice + rationale in architecture-spike.md
- [ ] Scaffold Cargo workspace from the chosen base; commit unmodified
- [ ] Set up Neon project (free tier): one trivial table, seed rows, sqlx connectivity from the server path
- [ ] One server function + one page rendering DB rows, using at least one Rust/UI component
- [ ] Build + run: hosted web target
- [ ] Build + run: macOS desktop target (embedded Axum)
- [ ] Build + run: one mobile target (iOS or Android); record whether embedded SSR works on mobile
- [ ] Write up findings in architecture-spike.md; mark spec implemented

## Phase 1b — UI design ([ui-design](ui-design.md)) — parallel with Phase 1, human-led

- [ ] Information architecture / nav structure
- [ ] Wireframe core screens (catalog search, collection, add-flow, auth, shell)
- [ ] Prototype the add-to-collection flow
- [ ] Component gap analysis vs. Rust/UI registry

## Phase 2 — foundations

- [ ] CI: fmt, clippy, test, web build on push
- [ ] Flesh out [data-model](data-model.md) using spike findings + designs; write initial migrations
- [ ] Design [data-access-backends](data-access-backends.md) trait split; remove spike-era direct DB access (prerequisite: Phase 1 complete)

## Later / parked (not in the queue — promote to a phase before working)

- Bundled read-only catalog for offline browsing on desktop/mobile (deliberately deferred)
- Decks and sharing features
- Import/export (CSV, Moxfield)

## Decisions log

- 2026-07: API-first on Neon chosen over offline-first Turso designs. Rationale in README.
- 2026-07: Architecture spike prioritized ahead of data model — architecture unproven.
- 2026-07: Spec numbering dropped; filenames are the stable IDs, this file owns execution order.
