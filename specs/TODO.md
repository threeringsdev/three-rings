# Project TODO — execution queue

**This file is the single source of truth for what to work on next.** The selection algorithm, spec gating, and definition of done are in [README.md](README.md) ("Working the queue"). State legend: `[ ]` available · `[~]` in progress · `[x]` done.

Phases execute top to bottom; tasks within a phase top to bottom. A task's `(specs: ...)` lists every spec it is gated on — all must be `accepted` (status is read from the spec files, not recorded here). Tasks without a specs annotation are ungated.

## Phase 1 — architecture spike

Ordered riskiest-first; see the spec's Failure policy — if the Android gate fails, STOP the phase.

- [ ] Compare scaffold bases (start-tauri-fullstack vs. tauri-leptos-ssr); record choice + rationale in architecture-spike.md (specs: [architecture-spike](architecture-spike.md), [ui-components](ui-components.md))
- [ ] Scaffold Cargo workspace from the chosen base; commit unmodified (specs: [architecture-spike](architecture-spike.md))
- [ ] Build + run: macOS desktop target (embedded Axum) (specs: [architecture-spike](architecture-spike.md))
- [ ] Build + run: Android (emulator OK) — the architecture gate; static page sufficient (specs: [architecture-spike](architecture-spike.md))
- [ ] Set up Neon project (free tier): one trivial table, seed rows, sqlx connectivity from the server path (specs: [architecture-spike](architecture-spike.md))
- [ ] One server function + one page rendering DB rows, using at least one Rust/UI component (specs: [architecture-spike](architecture-spike.md), [ui-components](ui-components.md))
- [ ] Verify web target: `server` binary locally, SSR + hydration (specs: [architecture-spike](architecture-spike.md))
- [ ] Write up findings in architecture-spike.md; mark spec implemented (specs: [architecture-spike](architecture-spike.md))

## Phase 1b — UI design — parallel with Phase 1, human-led

- [ ] Information architecture / nav structure (specs: [ui-design](ui-design.md))
- [ ] Wireframe core screens (catalog search, collection, add-flow, auth, shell) (specs: [ui-design](ui-design.md))
- [ ] Prototype the add-to-collection flow (specs: [ui-design](ui-design.md))
- [ ] Component gap analysis vs. Rust/UI registry (specs: [ui-design](ui-design.md), [ui-components](ui-components.md))

## Phase 2 — foundations

- [ ] CI: fmt, clippy, test, web build (incl. Tailwind pipeline) on push
- [ ] Flesh out data-model spec using spike findings + designs; write initial migrations (specs: [data-model](data-model.md))
- [ ] Design the data-access trait split; remove spike-era direct DB access (prerequisite: Phase 1 complete) (specs: [data-access-backends](data-access-backends.md))

## Later / parked (not in the queue — promote to a phase before working)

- Bundled read-only catalog for offline browsing on desktop/mobile (deliberately deferred)
- Decks and sharing features
- Import/export (CSV, Moxfield)

## Decisions log

- 2026-07: API-first on Neon chosen over offline-first Turso designs. Rationale in README.
- 2026-07: Architecture spike prioritized ahead of data model — architecture unproven.
- 2026-07: Spec numbering dropped; filenames are the stable IDs, this file owns execution order.
- 2026-07: Tasks gated on spec status via `(specs: ...)` annotations; only humans accept specs.
- 2026-07: Spike decisions: web = local run only; mobile = Android; mobile SSR failure = stop and reassess; no time-box; Android gate moved ahead of DB work (fail fast).
