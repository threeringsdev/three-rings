# Project TODO

Cross-cutting tasks that don't belong to a single spec. Spec-specific tasks live in their spec files.

## Now

- [ ] Flesh out spec 001 (data model) — everything depends on it
- [ ] Set up Neon project (free tier) and verify sqlx connectivity
- [ ] Scaffold Cargo workspace: `app/` (Leptos+Tauri), `server/` (Axum), `shared/` (types)

## Next

- [ ] CI: fmt, clippy, test on push
- [ ] Verify Tauri v2 mobile targets build from the shared Leptos frontend
- [ ] Spike: Leptos SSR vs. CSR-only for the web target

## Later / parked

- [ ] Bundled read-only catalog for offline browsing on desktop/mobile (deliberately deferred — see README Decision 1)
- [ ] Decks and sharing features
- [ ] Import/export (CSV, Moxfield)

## Decisions log

- 2026-07: API-first on Neon chosen over offline-first Turso designs. Rationale in README.
