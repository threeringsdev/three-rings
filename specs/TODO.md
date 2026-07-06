# Project TODO

Cross-cutting tasks that don't belong to a single spec. Spec-specific tasks live in their spec files.

## Now — architecture spike (spec 007)

The architecture is unproven; prove the skeleton before fleshing out the data model or features.

- [ ] Compare scaffold bases (start-tauri-fullstack vs. tauri-leptos-ssr); pick one
- [ ] Scaffold Cargo workspace (app/, frontend/, server/, src-tauri/)
- [ ] Set up Neon project (free tier), one trivial table + seed rows, sqlx connectivity
- [ ] One server function + one page rendering DB rows, with one Rust/UI component
- [ ] Build + run: hosted web, macOS desktop, one mobile target
- [ ] Record findings in spec 007 (especially: does embedded SSR work on mobile?)

## Now — UI design (spec 008, parallel)

- [ ] Information architecture / nav structure
- [ ] Wireframe core screens (catalog search, collection, add-flow, auth, shell)
- [ ] Prototype the add-to-collection flow

## Next

- [ ] CI: fmt, clippy, test, web build on push
- [ ] Flesh out spec 001 (data model) informed by spike findings + designs
- [ ] Spec 005 (data-access backends) — remove spike-era direct DB access before real user data

## Later / parked

- [ ] Bundled read-only catalog for offline browsing on desktop/mobile (deliberately deferred)
- [ ] Decks and sharing features
- [ ] Import/export (CSV, Moxfield)

## Decisions log

- 2026-07: API-first on Neon chosen over offline-first Turso designs. Rationale in README.
- 2026-07: Architecture spike (007) prioritized ahead of data model (001) — architecture unproven.
