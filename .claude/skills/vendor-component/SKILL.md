---
name: vendor-component
description: Vendor a Rust/UI registry component into app/src/components/ui/ the three-rings way — trap checks, asset vendoring, bench section in the same commit, runtime verification checklist. Use whenever the user asks to add, vendor, copy, or adopt a UI component (dialog, popover, command, sheet, sonner, button, hover_card, or any shadcn/Rust-UI-style primitive), even if they just say "we need a dialog for X" as part of feature work.
---

# Vendor-component — adopt a Rust/UI component correctly

Rust/UI is a registry, not a crate: components are copied in and owned by us
(specs/ui-components.md). **Before copying**, read the component's entry in
[design/component-gap-analysis.md](../../../design/component-gap-analysis.md)
(reviewed at rust-ui/ui `43e1e32` — prefer that pin) — but verify its claims
against the actual upstream source; at least one blanket claim there is wrong
(sonner does *not* import `leptos_ui`).

Copy `app_crates/registry/src/ui/<name>.rs` into `app/src/components/ui/`
with the header convention from [table.rs](../../../app/src/components/ui/table.rs)
(upstream path, MIT, every deviation listed); register in `mod.rs`.

## Trap checks (each is real)

- **`leptos_ui` imports** — never add the dep (it force-enables
  `leptos/nightly`, poisoning the stable build via feature unification).
  If the component uses `clx!`/`void!`/`transition!`, take them from the
  vendored [clx.rs](../../../app/src/components/ui/clx.rs), extending it from
  upstream source as needed.
- **Non-workspace deps** — check the component's `use` lines against
  `[workspace.dependencies]` (e.g. sonner derives `strum::Display`). Prefer a
  small hand-written impl over a new dependency; record it as a deviation.
- **`use_random_id`** — diverges SSR-vs-client from the server's second render
  onward; apply the gap analysis's deterministic-caller-ID deviation
  (`dialog`/`popover`/`hover_card`/`sheet`).
- **JS/CSS assets** — vendor anything the component references
  (`window.ScrollLock` → `use_scroll_lock`; `sonner.js`/`sonner.css`) into
  `public/` (the cargo-leptos assets-dir) or open handlers throw at runtime.
- **Theme tokens** — classes referencing tokens `style/input.css` doesn't
  define produce no CSS (Tailwind v4 only generates what it can resolve).
  Extend `input.css` and mirror new tokens into the bench's `COLOR_TOKENS`.
- **Extra attributes** — clx components take only `class` + `children`; pass
  anything else via spread: `<TableRow {..} data-state="selected">`.

## Bench section — same commit, no exceptions

Demo fn in `app/src/bench/<name>.rs` (static data; meaningful variants,
minimal) + one `SECTIONS` line in [app/src/bench/mod.rs](../../../app/src/bench/mod.rs).
Dev entry: `cargo leptos watch --features component-bench` → `/dev/components`.
If port 3000 hangs or refuses, something else may hold it (the bind happens
*after* the build): `LEPTOS_SITE_ADDR=127.0.0.1:3100 cargo leptos serve
--features component-bench`.

## Verification checklist (full text: specs/ui-component-bench.md)

1. **SSR** — curl view-source shows the rendered markup.
2. **Hydration** — demo works, no mismatch warnings
   ([end2end/bench-check.mjs](../../../end2end/bench-check.mjs); extend it for
   the new section where cheap).
3. **ID stability** — reload after ≥2 server renders (generated-ID components).
4. **Assets/ScrollLock** — handlers don't throw, effects render.
5. **Native-webview positioning** — WKWebView + Android WebView for
   CSS-anchor-positioned components. Manual/host-side; if not run now, say so
   explicitly rather than implying it passed.

Mark N/A items as N/A with a reason. Close out with the **validate** skill
(its bench clippy lines are the only compile coverage the gated code gets);
record discovered deviations in the component header *and* the gap analysis.
