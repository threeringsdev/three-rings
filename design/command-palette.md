# Global ⌘K command palette

**Deliverable of:** Phase 1b task "Design the global ⌘K command palette" — see [specs/ui-design.md](../specs/ui-design.md) (palette resolved as **v1**, maintainer decision 2026-07-10 at gap-analysis design review)
**Approved:** 2026-07-10, maintainer, section by section (behavior with revised command set; deliverables).
**Wireframe:** `⌘K — Command palette` region in [wireframes.pen](wireframes.pen) (built from this design).

## Purpose

The keyboard-jump layer over the whole logged-in app. The sidebar tree is otherwise mouse-only; the palette makes "go to any collection" a two-second keyboard act from anywhere, and carries a small set of global commands. It is an accelerator over existing affordances — every palette entry duplicates a designed path, never replaces one.

## Trigger and availability

- **`⌘K`** (mac) / **`Ctrl+K`** (elsewhere), global across both modes of the logged-in app.
- **Not on mobile in v1.** The palette is a hardware-keyboard accelerator, not a feature surface, so omitting it doesn't violate the IA's "navigation collapses, features don't" rule; it returns for free on keyboard-attached devices later.
- **No logged-out palette** — everything it targets is session-gated (`/my/*`).
- **`/` is never bound by the palette** — it belongs to the in-collection quick-add ([add-flow-prototype](add-flow-prototype.md)). The registry `command` script's built-in `/` binding is discarded with the rest of its vanilla-JS layer (see [component-gap-analysis](component-gap-analysis.md) deviations).
- `esc` closes; the palette holds focus while open (centered modal over a scrim).

## Contents — two kinds of rows

**Places** (⏎ navigates):
- Every collection and deck, flattened from the tree, displayed with parent path meta: `Trade Binder — Binders`, `Grixis Control — Decks`.
- System places: All cards, Inbox, Shopping list.
- Mode jumps: `Go to Catalog`, `Go to My cards`.

**Commands** (⏎ runs) — the fixed v1 registry, three entries:
- `New binder…` / `New deck…` — navigate to My cards and spawn the tree's inline-create row at root: the same flow the tree's context menu owns (per the IA, tree management is in-place); the palette only triggers it.
- `Undo last move` — the same action the undo toast and the topbar history affordance expose.

`Sign out` was considered and dropped (maintainer): rare, destructive-ish, stays in the user menu only. No card matches in v1 (maintainer): card lookup stays in the two designed search surfaces (in-collection quick-add; Catalog mode) — the palette matches places and commands only, so it never becomes a third card-search surface. A card jump (`/cards/:id`) can be added later without redesign.

## States

- **At rest (empty query):** RECENT group — last-visited places, most recent first — then COMMANDS. First RECENT row pre-selected, so `⌘K ⏎` bounces to your last collection.
- **Typing:** fuzzy filter over place names and command labels, grouped COLLECTIONS / COMMANDS (groups with no match drop out). Best match pre-selected; `↑↓` navigate, `⏎` commits.
- **Footer:** kbd hints `↑↓ navigate · ⏎ open · esc close`, matching the quick-add panel's footer idiom.
- No-match state: a quiet empty row ("No matches"); typing is never an error.

## Component mapping (per component-gap-analysis)

`CommandDialog` (caller-supplied deterministic id) + `Command`/`CommandInput`/`CommandItem` from the vendored registry set; client-side filtering (`should_filter=true` — the place list is small and local, no server round-trip); keyboard handling written in Leptos, replacing the registry's vanilla-JS layer; `ScrollLock` deviation as catalogued. The palette and the quick-add panel share the same vendored core.

## Wireframe spec

Region `⌘K — Command palette` in wireframes.pen, overlay-frame idiom (like the hover-preview overlay — overlay only, no full-screen milestones; scrim/centering documented in the note):

| Frame | State |
|---|---|
| P1 | At rest: input placeholder `Where to?`, RECENT (3 rows with path metas), COMMANDS (3 rows), kbd footer |
| P2 | Typed `tra`: COLLECTIONS group (`Trade Binder — Binders` highlighted best match, `Trade duplicates — Bulk Box` second), commands dropped out, same footer |

One note beside the frames documents: trigger keys, centered-modal-over-scrim presentation, logged-in desktop only, `/` non-binding, ⏎ navigate/run split.

## Feeds into

- **ui-design.md:** Findings entry on completion; the open question was already annotated resolved (v1).
- **Foundations/features:** the fixed command registry is deliberately tiny — context-aware actions (Move selection…, Empty deck…) were explicitly deferred; if they arrive later they extend the same palette, not a new surface.
- **component-gap-analysis:** no new registry needs — the palette uses the already-catalogued `command` composite.
