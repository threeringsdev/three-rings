# UI component bench

**Status:** draft
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
- Two jobs: **(1) styling review** (see every affected component at once after a
  theme/class edit) and **(2) adoption-time SSR/hydration verification** — the
  runtime home the gap analysis deferred to (concrete checklist below).
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
- **Bench-local theme toggle from the start:** a light/dark switch that flips the
  `dark` class on the bench's own container so both themes are reviewable. This
  is scoped to the bench subtree and does **not** wait on ui-design's open
  app-wide theming question; when that lands, the bench adopts the real
  `theme_toggle` component and drives the app-level mechanism.

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

Edit theme variables or a component's classes, reload the bench, see every
affected component at once in both themes.

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
- **Feature-wiring convenience (execution):** keep `component-bench` on across the
  three dev entry points (`cargo leptos watch`, `cargo tauri dev`,
  `cargo tauri android dev`) while keeping it off in the CI/Render release builds
  — settle the exact flag plumbing (leptos `--lib-features`/`--bin-features`,
  `src-tauri` feature set) when the bench is built.

## Findings

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
