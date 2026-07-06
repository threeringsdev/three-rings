# Architecture spike

**Status:** accepted
**Depends on:** —

## Problem

The proposed architecture (shared `app` crate with Leptos SSR + embedded Axum, shipped as web app and Tauri-wrapped native apps, talking to Neon) hasn't been tried by us. Before investing in the data model or features, prove the skeleton end-to-end — riskiest assumption first.

## Scope

In: project scaffold, a trivial schema (one table, e.g. `cards(id, name)` with a few seed rows), one server function reading it, one page rendering it, building and running on every target. Out: real data model, auth, UI polish, deployment to real infrastructure — anything not needed to prove the pipeline.

## Success criteria

The same `app` crate, with a one-table Neon database behind it, demonstrably:

- [ ] Runs as an Android app (Tauri, embedded Axum — **the architecture gate**, see Failure policy)
- [ ] Runs as a macOS desktop app (Tauri, embedded Axum, dynamic port)
- [ ] Serves SSR + hydration via the `server` binary running locally (real deployment is out of scope)
- [ ] Renders one Rust/UI component (proves Tailwind pipeline in both build paths)
- [ ] Dev loop works: `cargo tauri dev` / `cargo leptos watch` with hot reload

## Plan

Ordered to test the riskiest assumption (embedded SSR on mobile) before investing in the rest.

1. Pick scaffold base: [start-tauri-fullstack](https://github.com/rust-ui/start-tauri-fullstack) (Rust/UI pre-wired) vs. [tauri-leptos-ssr](https://github.com/codeitlikemiley/tauri-leptos-ssr) — evaluate both, pick one (see ui-components).
2. Scaffold workspace; commit before modifications.
3. Build/run macOS desktop (should work — the templates prove this pattern).
4. **Build/run Android (emulator acceptable): does the embedded Axum + WebView pattern work at all on mobile?** Static page is sufficient; no DB needed for this gate.
5. Set up Neon free-tier project; single migration; connect via sqlx from the server path.
6. One server function + one page listing rows, using one Rust/UI component.
7. Verify web target: `server` binary locally, SSR + hydration.
8. CI: fmt, clippy, test, and web build on push.

## Failure policy

If embedded SSR does not work on Android (step 4), **stop the spike and reassess the architecture** — do not proceed to steps 5–8, do not silently fall back to CSR. Write up findings, then revisit the README architecture with the human before any further work.

## Time-box

None. The spike runs until success criteria are met or the failure policy triggers.

## Findings

(Record here as the spike proceeds — this section becomes the input to data-access-backends and future specs.)

- Where do DB credentials live during the spike? (Native builds talking directly to Neon is acceptable *for the spike only* — data-access-backends removes this before any real user data.)
