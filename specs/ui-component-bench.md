# UI component bench

**Status:** draft
**Depends on:** [ui-components](ui-components.md)

## Problem

Rust/UI components are copied into the repo and owned by us ([ui-components](ui-components.md)) — there is no upstream site to look at and no single place to see what we have. Reviewing or editing app styling (theme variables, Tailwind classes on a component) currently means finding every screen that uses the component. And each newly adopted component needs its SSR/hydration behavior verified somewhere before a feature depends on it (the gap analysis reviewed sources only; runtime verification was deliberately deferred to this bench).

## Scope

In: one Storybook-like page rendering every vendored component in `app/src/components/ui/` with representative variants/states; kept permanently in the repo; used for styling review and for adoption-time SSR/hydration verification.

Out: the rest of the Storybook feature set (stories-as-files, controls/knobs, addons), visual regression testing, deploying or publishing the bench, documenting component APIs.

## Design

- A single Leptos route in the app (proposed: `/dev/components`), server-rendered so SSR is exercised on every load; interactive demos exercise hydration.
- One titled section per vendored component, rendering its meaningful variants (e.g. button: default/outline/destructive/disabled; dialog: a trigger that opens it). Demos are minimal — enough to see the styling and prove interactivity, not documentation.
- **Adoption convention:** vendoring a new component includes adding its bench section in the same commit — the bench is complete by construction.
- Styling workflow: edit theme variables or component classes, reload the bench, see every affected component at once.

## Open questions

- Route and gating: always compiled in, debug-builds only, or behind a feature flag? (Pre-v1 there is no harm in always-on; revisit before any public deploy.)
- Theme toggle on the bench: include from the start, or add when the ui-design theming question resolves?
- One page with anchors vs. per-component subroutes as the vendored set grows?
- Where in the queue: the build task (added to TODO.md by the gap-analysis task) sits in Phase 3 (foundations) — should it move earlier so the bench exists before the first feature screens are styled?
