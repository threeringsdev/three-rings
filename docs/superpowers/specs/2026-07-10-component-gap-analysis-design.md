# Component gap analysis — task design

**Deliverable of:** Phase 1b task "Component gap analysis vs. Rust/UI registry" — see [specs/ui-design.md](../../../specs/ui-design.md), [specs/ui-components.md](../../../specs/ui-components.md)
**Approved:** 2026-07-10, maintainer, section by section (deliverable/method; revised runtime-check section).

## Goal

Map every UI primitive appearing in the Phase 1b wireframes ([design/wireframes.pen](../../../design/wireframes.pen)) to the Rust/UI registry, classify each as direct / composite / gap, and resolve the two open questions this task owns. Paper analysis only — no components are vendored by this task.

## Deliverable

`design/component-gap-analysis.md`, containing:

1. **Registry pin:** rust-ui/ui commit `43e1e32` (2026-07-01) — component sources under `app_crates/registry/src/ui/`. All mapping and review claims are against this commit.
2. **Primitive catalog:** one deduped table keyed by UI primitive (~20–25 rows): primitive → screens where it appears → registry match → verdict. Completeness is checked by an internal walk of every wireframe screen (9 core screens + hover-preview overlay + the 4 `Proto — Add flow` storyboard rows); the walk itself is not part of the deliverable.
3. **Verdict classes:**
   - **Direct** — registry component adopted as-is (expected: `sonner` for the undo toast, `hover_card` for the hover preview, `kbd`, `sheet`, …)
   - **Composite** — assembled from registry parts, ours to build (expected: quick-add panel from `command`+`popover`+`input`; destination picker from `dropdown_menu`+`command`)
   - **Gap** — nothing in the registry covers it; custom component required (expected: collection tree with drag-reparent; in-place count stepper; `action_bar` to be read before ruling on the selection tray)
   - App-specific composites (three-counts row, card grid) are noted as feature-side work per the ui-components scope line — not registry gaps.
4. **SSR/hydration code review** (see below), results per component.

## SSR/hydration code review

Discharges the ui-components open question ("spot-check the components we need for SSR/hydration correctness") at the code level; runtime verification is deferred to the component bench (below). The spike already proved `table` at runtime.

- Components reviewed at the pinned commit: `dialog`, `popover`, `command`, `hover_card`, `sheet`, `sonner`.
- Hazard checklist per component: client-only APIs outside effects (`window`/`document` at render time), portal usage, effect-driven initial rendering (hydration mismatch risk), `leptos_ui`/nightly dependency (the `clx!` hazard that already required vendoring), non-deterministic IDs, event wiring that assumes CSR.
- Output per component: adopt as-is / adopt with deviations (named) / needs runtime verification before adoption.

## Decisions taken at design time (maintainer, 2026-07-10)

- **Runtime spot-check skipped for this task.** Originally chosen, then withdrawn in favor of a standing component-bench page with its own spec. Rationale: don't vendor components before anything uses them; give styling review a permanent home instead of a one-off scratch page.
- **Global ⌘K command palette is a v1 feature** — resolves the ui-design open question. The registry's `command` component carries the palette UI; the quick-add panel already vendors the same parts.

## New draft spec

`specs/ui-component-bench.md` (status `draft`; maintainer accepts separately): a Storybook-like single page rendering every vendored component with representative variants, kept permanently, for reviewing and editing app styling and for adoption-time SSR/hydration verification.

## Spec and queue updates (final commit of the task)

- **specs/ui-design.md:** Findings entry for this task; palette open question annotated resolved (V1).
- **specs/ui-components.md:** open question annotated — code review done here, runtime verification deferred to the bench; findings note the no-combobox discovery (`command` is the stand-in).
- **specs/TODO.md:**
  - Phase 1b: `[ ] Design the global ⌘K command palette (actions, scope, wireframe) (specs: ui-design)`
  - Phase 3: `[ ] Build the component bench page (specs: ui-component-bench)`
  - This task's `[~]` → `[x]` in the same commit as the final work.

## Definition of done (per specs/README.md)

- `design/component-gap-analysis.md` committed with catalog + review results.
- Findings recorded in both gating specs; open questions annotated.
- Follow-up tasks added to TODO.md; checkbox flipped in the final commit.
