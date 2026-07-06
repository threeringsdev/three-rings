# Architecture spike

**Status:** draft
**Depends on:** —

## Problem

The proposed architecture (shared `app` crate with Leptos SSR + embedded Axum, shipped as web app and Tauri-wrapped native apps, talking to Neon) hasn't been tried by us. Before investing in the data model or features, prove the skeleton end-to-end.

## Scope

In: project scaffold, a trivial schema (one table, e.g. `cards(id, name)` with a few seed rows), one server function reading it, one page rendering it, building and running on every target. Out: real data model, auth, UI polish — anything not needed to prove the pipeline.

## Success criteria

The same `app` crate, with a one-table Neon database behind it, demonstrably:

- [ ] Serves SSR + hydration as a hosted web app (`server` binary)
- [ ] Runs as a macOS desktop app (Tauri, embedded Axum, dynamic port)
- [ ] Builds and runs on at least one mobile target (iOS or Android)
- [ ] Renders one Rust/UI component (proves Tailwind pipeline in both build paths)
- [ ] Dev loop works: `cargo tauri dev` / `cargo leptos watch` with hot reload

## Plan

1. Pick scaffold base: [start-tauri-fullstack](https://github.com/rust-ui/start-tauri-fullstack) (Rust/UI pre-wired) vs. [tauri-leptos-ssr](https://github.com/codeitlikemiley/tauri-leptos-ssr) — evaluate both, pick one (see ui-components).
2. Scaffold workspace; commit before modifications.
3. Set up Neon free-tier project; single migration; connect via sqlx from the hosted path.
4. One server function + one page listing rows.
5. Build/run each target; record problems and workarounds as findings below.
6. CI: fmt, clippy, test, and web build on push.

## Findings

(Record here as the spike proceeds — this section becomes the input to data-access-backends and future specs.)

- Mobile SSR-in-process: does the embedded Axum pattern work on iOS/Android at all, or is mobile CSR-only?
- Where do DB credentials live during the spike? (Native builds talking directly to Neon is acceptable *for the spike only* — data-access-backends removes this before any real user data.)

## Tasks

Tracked in `TODO.md` (Now/Next phases map to this spec).
