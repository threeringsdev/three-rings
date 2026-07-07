# UI component system

**Status:** accepted
**Depends on:** —

## Problem

The app needs a consistent, styled component set (buttons, dialogs, tables, forms, command palette for card search) without building a design system from scratch.

## Scope

In: adopting [Rust/UI](https://github.com/rust-ui/ui) as the component source, Tailwind CSS integration, component organization conventions. Out: app-specific composite components (card grid, collection table) — those live with their features.

## Design

Rust/UI is a shadcn-style **registry, not a crate**: components are copied into the repo (manually or via `ui-cli`) and owned by us thereafter. Consequences:

- Components live in `app/src/components/ui/`; once copied they are our code — edit freely, no upstream version to fight.
- Trade-off accepted: no automatic upstream fixes. Pulling improvements is a manual diff.
- Supporting crates come as normal dependencies: `tw-merge` (class merging), `icons` (Leptos icon components). *Exception found during the spike (task 6): `leptos_ui` (Rust/UI's `clx!` macro crate) force-enables `leptos/nightly`, which would break our stable-toolchain build via feature unification — so the small `clx!` macro is vendored at `app/src/components/ui/clx.rs` instead of depended on.*
- Copy mechanism (`ui-cli` vs. manual paste): executor's choice; whichever is used, the result is committed source we own.
- Tailwind CSS (v4) joins the build pipeline via the Tailwind CLI — this is the one non-Rust build tool in the stack. Must be wired into both `cargo leptos` dev/build and the Tauri `beforeBuildCommand` chain.
- Rust/UI's own repo and its [start-tauri-fullstack](https://github.com/rust-ui/start-tauri-fullstack) starter follow the same app/server/src-tauri layout we've planned — use them as reference when scaffolding, and evaluate whether to scaffold *from* the starter instead of from tauri-leptos-ssr (they're the same pattern; the starter includes Rust/UI pre-wired).

## Open questions

- *(resolved — architecture spike, task 1, 2026-07-06)* Scaffold from `start-tauri-fullstack` vs. `tauri-leptos-ssr` + manual Rust/UI setup? → **`tauri-leptos-ssr` + manual Rust/UI setup.** start-tauri-fullstack does not embed the server in-process (thin Tauri shell → external/networked SSR server, `csr`-default app), so it does not match this project's embedded-Axum architecture — the two starters are *not* the same pattern. Rust/UI components will be copied in ourselves, using start-tauri-fullstack's pre-wired components/Tailwind setup as reference. Details in [architecture-spike](architecture-spike.md) Findings.
- *(resolved during execution — spike + Phase 1b gap analysis)* Rust/UI is young (~300 stars) — spot-check the components we need (dialog, popover, table, combobox) for SSR/hydration correctness before committing broadly.

Theming/dark mode is a design decision — moved to [ui-design](ui-design.md). This spec only requires that the component system supports theming (it does: CSS variables + Tailwind dark variant).
