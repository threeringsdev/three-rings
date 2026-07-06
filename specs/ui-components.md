# UI component system

**Status:** draft
**Depends on:** —

## Problem

The app needs a consistent, styled component set (buttons, dialogs, tables, forms, command palette for card search) without building a design system from scratch.

## Scope

In: adopting [Rust/UI](https://github.com/rust-ui/ui) as the component source, Tailwind CSS integration, component organization conventions. Out: app-specific composite components (card grid, collection table) — those live with their features.

## Design

Rust/UI is a shadcn-style **registry, not a crate**: components are copied into the repo (manually or via `ui-cli`) and owned by us thereafter. Consequences:

- Components live in `app/src/components/ui/`; once copied they are our code — edit freely, no upstream version to fight.
- Trade-off accepted: no automatic upstream fixes. Pulling improvements is a manual diff.
- Supporting crates come as normal dependencies: `tw-merge` (class merging), `icons` (Leptos icon components).
- Tailwind CSS (v4) joins the build pipeline via the Tailwind CLI — this is the one non-Rust build tool in the stack. Must be wired into both `cargo leptos` dev/build and the Tauri `beforeBuildCommand` chain.
- Rust/UI's own repo and its [start-tauri-fullstack](https://github.com/rust-ui/start-tauri-fullstack) starter follow the same app/server/src-tauri layout we've planned — use them as reference when scaffolding, and evaluate whether to scaffold *from* the starter instead of from tauri-leptos-ssr (they're the same pattern; the starter includes Rust/UI pre-wired).

## Open questions

- Scaffold from `start-tauri-fullstack` vs. `tauri-leptos-ssr` + manual Rust/UI setup?
- Rust/UI is young (~300 stars) — spot-check the specific components we need (dialog, popover, table, combobox) for SSR/hydration correctness before committing broadly.
- Theming: dark mode from day one?

## Tasks

- [ ] Compare the two starters; pick scaffold base
- [ ] Copy in an initial component set and verify SSR + hydration in both web and Tauri builds
- [ ] Wire Tailwind into CI
