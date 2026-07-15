# UI component bench

**Status:** implemented
**Depends on:** [ui-components](ui-components.md)

## Problem

Rust/UI components are copied into the repo and owned by us
([ui-components](ui-components.md)) — there is no upstream site to look at and no
single place to see what we have. Two concrete gaps follow:

- **Styling review is scattered.** Reviewing or editing app styling (theme
  variables, Tailwind classes on a component) means hunting down every screen
  that uses the component.
- **Runtime verification has no home.** The Phase 1b gap analysis
  ([design/component-gap-analysis.md](../design/component-gap-analysis.md))
  reviewed the six interactive components (`dialog`, `popover`, `command`,
  `hover_card`, `sheet`, `sonner`) *at the source level only* and explicitly
  deferred runtime SSR/hydration verification to this bench. It also surfaced
  four cross-cutting hazards that only manifest at runtime and must be *caught*
  somewhere before a feature depends on the component (see the checklist below).

Today only `table` is vendored (`app/src/components/ui/`), so the bench is, for
now, mostly a **harness + adoption convention** that pays off as feature work
vendors the rest of the catalogued set.

## Scope

In:

- One route rendering every vendored component in `app/src/components/ui/` with
  representative variants/states; kept permanently in the repo.
- Two jobs: **(1) styling review** — a static panel of the current theme tokens
  atop every component below, so a theme/class edit's effect is visible at a
  glance — and **(2) adoption-time SSR/hydration verification**, the runtime home
  the gap analysis deferred to (concrete checklist below).
- **Reachable inside the native shells** (`cargo tauri dev` → WKWebView;
  `cargo tauri android dev` → Android WebView), because the anchor-positioning
  hazard only appears on those engines.

Out: the rest of the Storybook feature set (stories-as-files, controls/knobs,
addons), visual-regression testing, deploying/publishing the bench, documenting
component APIs.

## Design

### Route, gating, shape

- A single Leptos route in `app` (proposed `/dev/components`), server-rendered so
  SSR is exercised on every load; interactive demos exercise hydration.
- **Gated behind a `component-bench` cargo feature** on the `app` crate
  (maintainer decision 2026-07-15). All bench code — route registration, the
  section modules, the jump-nav — is `#[cfg(feature = "component-bench")]`. Dev
  workflows enable it; the production web build (`cargo leptos build --release`
  in the root `Dockerfile`) and shipped app builds leave it off. A cargo feature
  (rather than `#[cfg(debug_assertions)]`) was chosen deliberately so the bench
  can *also* be switched on in a local **release** build — to verify a component
  against the embedded-Axum release SSR path or a release webview when needed.
- One page, one titled `#anchor` section per component, with a sticky
  table-of-contents / jump-nav built from the same section registry.
  (Per-component subroutes rejected for now: more routing for no gain at this set
  size; revisit only if page weight bites.)
- One section renders a component's meaningful variants (e.g. button:
  default/outline/destructive/disabled; dialog: a trigger that opens it). Demos
  are minimal — enough to see styling and prove interactivity, not documentation.
- **Bench-local light/dark toggle — the one dynamic control.** A switch that
  flips the `dark` class on the bench's container, re-rendering both the theme
  panel (below) and every component section in the toggled mode. Scoped to the
  bench subtree; does **not** wait on ui-design's open app-wide theming question.
  Note the current reality: [`style/input.css`](../style/input.css) has **no
  `.dark` block yet** (theming is ui-design's call), so the toggle is wired and
  ready but only shows a real difference once the dark palette lands — at which
  point the bench adopts the real `theme_toggle` component and drives the
  app-level mechanism.

### Theme panel (static)

Rust/UI's registry demo ships a "create" page that live-edits a small set of
theme variables (color / font / border-radius) and watches every component react.
The bench carries that *legibility* — **statically**. A panel at the top of the
page displays the **current primary theme tokens** so the theme → component
relationship is visible at a glance, without building an editor:

- **One row per primary token.** Today, from
  [`style/input.css`](../style/input.css): the color tokens `--background`,
  `--foreground`, `--card`, `--card-foreground`, `--muted`, `--muted-foreground`,
  `--border`, `--ring`, plus `--radius` (a font token joins them if ui-design's
  theming introduces one). Each row is a swatch/preview + the token name + its
  resolved value.
- **Read-only** — no sliders or pickers. Editing the theme means editing
  `style/input.css` and reloading; that *is* the styling workflow. The only
  dynamic input on the page is the light/dark toggle above.
- **Reflects the *active* mode's resolved values**, so toggling dark updates the
  panel alongside the components. The panel should read the live computed token
  values (swatches via `var(--token)`; the value text via the element's computed
  style) — single source of truth = the CSS — so it can never drift from a
  Rust-side copy of the palette.

### Adoption convention — "complete by construction"

Vendoring a new component includes adding its bench section **in the same
commit**. Mechanically: a `bench` module with one demo function per component
plus a single registry list the page and jump-nav iterate; adding a component =
add its demo fn + one registry line. The bench is therefore never a fixed
milestone to "finish" — it is a living page kept total by the convention.

### Verification checklist (job 2)

When a component is vendored, its bench section is where these runtime checks are
performed — the gap analysis did the source review; the bench closes it out. For
each newly adopted component:

1. **SSR** — the server response (web view-source, or the native embedded server)
   shows the component's rendered markup, not an empty shell.
2. **Hydration** — after wasm loads, the interactive demo works (open/close,
   filter, hover, toast) with no hydration-mismatch console warnings.
3. **ID stability** — reload after the SSR server has served ≥2 renders and
   confirm no ID mismatch / broken `popovertarget`/`aria` wiring. This is where
   the gap analysis's `use_random_id` hazard surfaces (the process-global counter
   diverges from the client's from the server's *second* render onward); it
   validates the deterministic-caller-ID deviation. Affects
   `dialog`/`popover`/`hover_card`/`sheet`.
4. **Vendored-asset / ScrollLock** — for components whose inline behavior
   references `window.ScrollLock` (a separate registry item, `use_scroll_lock`)
   or external assets (`sonner.js` / `sonner.css`), confirm those are vendored
   alongside the `.rs` so open handlers don't throw and effects actually render.
5. **Native-webview positioning** — load the section in WKWebView
   (`cargo tauri dev`) and Android WebView (`cargo tauri android dev`) and confirm
   CSS-anchor-positioned components (`popover`, `hover_card`) position correctly.
   This "silently breaks" only on those engines, so the web target alone can't
   clear it.

(The `leptos_ui` nightly hazard is a build-time concern, not a runtime check — it
is retired per-component by the vendored [clx.rs](../app/src/components/ui/clx.rs)
treatment; the bench merely compiling on the stable toolchain confirms it.)

### Styling workflow (job 1)

Edit the theme tokens in [`style/input.css`](../style/input.css) (or a
component's classes), reload the bench: the theme panel re-renders with the new
token values and every affected component below updates with it — in either mode
via the light/dark toggle.

## Open questions

- ~~Route and gating: always-on / debug-only / feature flag?~~ **Resolved
  (2026-07-15):** `component-bench` cargo feature (see Design) — dev-only, off in
  production builds, opt-in-able in a local release build.
- ~~Theme toggle: from the start or when theming resolves?~~ **Resolved
  (2026-07-15):** bench-local `dark`-class toggle from the start; adopt the real
  `theme_toggle` when ui-design's app-wide theming lands.
- ~~One page with anchors vs. per-component subroutes?~~ **Resolved
  (2026-07-15):** one page, anchored sections + jump-nav; revisit only on
  page-weight pressure.
- ~~Where in the queue?~~ **Resolved (2026-07-15):** independent UI infra,
  buildable in parallel with the data/API layer (gated only on this spec, not on
  any data task) — pulled ahead of the data chain in Phase 4 rather than trailing
  it.
- ~~Feature-wiring convenience (execution): keep `component-bench` on across the
  three dev entry points while keeping it off in the CI/Render release builds —
  settle the exact flag plumbing.~~ **Resolved (2026-07-15, implementation):**
  `frontend`, `server`, and `src-tauri` each define a `component-bench` feature
  forwarding to `app/component-bench`, so one cargo-leptos flag switches both
  halves: `cargo leptos watch --features component-bench` (applies to lib *and*
  bin targets). `tauri.conf.json`'s `beforeDevCommand` carries the flag, which
  covers `cargo tauri dev` and `cargo tauri android dev` (both iterate against
  the watch server via `devUrl`). The `[[workspace.metadata.leptos]]`
  `bin-features`/`lib-features` stay empty, so the CI/Render release builds
  (plain `cargo leptos build --release`) exclude the bench with zero changes.
  Local bench-enabled release build (the embedded-Axum release SSR path):
  `cargo leptos build --release --features component-bench`, then
  `cargo tauri build --features component-bench` (the src-tauri feature puts the
  bench in the embedded router; the leptos flag puts it in the wasm bundle that
  build bundles as `frontendDist`).

## Findings

- 2026-07-15 — **Implemented** (`/dev/components`, feature-gated, `table`
  section + static theme panel + bench-local dark toggle + jump-nav; spec →
  implemented). Execution decisions beyond the accepted spec, and what
  verification showed:
  - **Feature plumbing** (resolves the last OQ; details there): per-crate
    forwarding features + one `--features component-bench` flag on
    cargo-leptos, which applies to both lib and bin targets;
    `beforeDevCommand` carries it for the Tauri dev entry points; release
    builds unchanged.
  - **Route registration mechanics.** The `view!` macro can't express a
    feature-gated route: `#[cfg]` isn't supported on a `<Route>` node, an
    expression child of `<Routes>` is treated as a render child (not a route
    def), and the tempting `impl MatchNestedRoutes for ()` stub is a trap —
    `()` *matches every path* (it would shadow the 404 fallback) and
    generates a phantom `PathSegment::Unit` route on the server. So `App`
    now composes the route tuple in plain Rust (`view! { ...4 <Route/>s }
    .into_inner()`, with the bench route appended under `#[cfg]`) and feeds
    `Routes` through its props builder (`RoutesProps::builder()`), keeping a
    single source of truth for the route list.
  - **Theme panel values fill in post-hydration.** SSR can't resolve CSS
    custom properties, so rows render an `…` placeholder server-side; a
    hydrate-gated `Effect` reads `getComputedStyle` off each row's
    `NodeRef` (re-running on mode toggle, since swatches update via CSS but
    the value *text* doesn't). web-sys grew `Element`/`HtmlDivElement`/
    `CssStyleDeclaration` features for this.
  - **CI coverage added** ([validate.yml](../.github/workflows/validate.yml)
    "Clippy (component bench)"): the bench is cfg'd out of every existing
    gate command, so without explicit `--features` clippy lines (ssr native +
    hydrate wasm) its code would never be linted — cfg'd-out rot ships
    itself. No bench tests were added; the gate's coverage is compile+lint,
    and runtime behavior is the drive below.
  - **Verification** (host, dev profile): SSR body carries the bench markup
    (`table` rows, `data-state="selected"`, `var(--token)` swatches);
    hydration is clean (no console errors/warnings) with theme values
    resolving and the dark toggle flipping the container class — automated
    as [end2end/bench-check.mjs](../end2end/bench-check.mjs) (checklist
    items 1–2 for future sections); feature-off build 404s `/dev/components`
    while `/`, `/cards`, `/login`, `/signup` all still SSR + hydrate clean
    (guarding the routing refactor). Checklist items 3–5 are N/A for
    `table` (no `use_random_id`, no JS assets, no anchor positioning; it was
    the spike's runtime-verified component) — they start applying with the
    first interactive adoption. Native-webview *reachability* is wired via
    `beforeDevCommand`; item 5's WKWebView/Android WebView pass stays a
    per-adoption step for anchor-positioned components.
  - Incidental: `end2end/package-lock.json` is out of sync with its
    `package.json` (`npm ci` refuses; predates this task, left as found —
    the drives above ran against an existing install).

- 2026-07-15 — **Spec fleshed out for review (maintainer session).**
  - **Elevated the verification-harness purpose** to co-equal with styling
    review: the gap analysis reviewed the six interactive components at source
    only and deferred runtime SSR/hydration verification here, plus flagged four
    cross-cutting hazards (`use_random_id` ID divergence,
    `window.ScrollLock`/vendored-asset dependence, `leptos_ui` nightly, CSS-anchor
    positioning in the native webviews). The Design now carries a concrete
    per-adoption verification checklist that turns "verify at the bench" into a
    repeatable procedure, and makes native-webview reachability a hard scope
    requirement (the anchor-positioning hazard is invisible on the web target).
  - **Static theme panel added (maintainer request).** Mirrors Rust/UI's "create"
    demo but *static*: the bench displays the current primary theme tokens (the
    [`style/input.css`](../style/input.css) color set + `--radius`; a font token
    later) as read-only swatches so the theme → component effect is legible, with
    the light/dark toggle as the single dynamic control. The panel reads live
    computed values (no Rust-side palette copy) so it can't drift; note
    `style/input.css` has no `.dark` block yet, so the toggle is wired but visibly
    inert until ui-design's theming lands.
  - **Gating = `component-bench` cargo feature** (over `debug_assertions`):
    dev-only, stripped from production/shipped builds, but switch-on-able in a
    local release build to exercise the embedded-Axum release SSR path.
  - **Queue placement = build in parallel:** the task is independent of the
    blocked data specs (gated only on this one), renders just `table` today, and
    its value compounds with adoption — so it's positioned ahead of the data/API
    chain, not behind it, and the "add the bench section in the same commit"
    convention is meant to be in force before component #2 is vendored.
  - **Low-stakes OQs resolved:** bench-local theme toggle now; one page with
    anchored sections + jump-nav.
  - **Current vendored set = `table` only**, so the initial bench build is
    scaffold + `table` section + the convention; the catalogued components in
    [design/component-gap-analysis.md](../design/component-gap-analysis.md) are
    the forward worklist, each verified via the checklist as it's adopted.
