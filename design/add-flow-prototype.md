# Add-to-collection flow — click-through prototype design

**Deliverable of:** Phase 1b task "Prototype the add-to-collection flow" — see [specs/ui-design.md](../specs/ui-design.md)
**Approved:** 2026-07-10, maintainer, section by section (medium, scope, layout, and both design sections).
**Prototype artifact:** storyboard frames in [wireframes.pen](wireframes.pen) (built from this design).

## Purpose and medium

The spec's metric for the add flow is **time-to-enter-50-cards** on a keyboard-first path. This prototype validates it as a **click-through state sequence in Pencil**: Pencil has no interactive links between frames, so "click-through" means frames arranged as a walkable storyboard — each transition captioned with the exact input that causes it. Validation becomes an explicit **input-cost accounting**: every transition is labeled with its keystrokes/taps, and a summary note computes cost-per-card and projects the 50-card total.

Chosen over an interactive HTML prototype (would measure real typing but leaves the design toolchain) and a Leptos spike (front-runs Phase 2/3 foundations).

## Scope

All add surfaces, desktop and mobile:

1. In-collection type-ahead, binder context (Have-led) — the metric path, deepest walk
2. Deck-context variant (Want-led emphasis flip)
3. Catalog mode add (sticky destination picker, quick actions, logged-out state)
4. Mobile intake (tap-based add into Inbox)

Out of scope: move flows (separate validation per spec Process), pick lists, deck teardown.

## Canvas organization

- New frames in a fresh canvas area below the existing screens, one storyboard row per surface: `Proto — Add flow · Desktop`, `· Deck context`, `· Catalog`, `· Mobile`. Each reads left-to-right.
- **Milestone frames** are full screens (1440×900 desktop, 390×844 mobile) — used only where the whole screen meaningfully changes. **Close-up frames** are ~720px crops of just the quick-add panel — used for the keystroke-by-keystroke rhythm.
- Between frames, a `note` caption states the transition input (`types "ligh"`, `⏎`, `⇧⏎` …), so the storyboard doubles as the keystroke ledger.
- Existing components (Tree Item, Card Row, Card Tile) and the existing quick-add panel structure are reused; no new components are expected. Component needs discovered here feed the component-gap-analysis task, not this one.
- The existing `Desktop — Add flow (type-ahead)` screen is adjusted into milestone M2 rather than duplicated.

## Storyboards

### Desktop type-ahead — binder context (Trade Binder, Have-led)

| Frame | Kind | State | Transition in |
|---|---|---|---|
| M1 | full | Collection at rest; persistent type-ahead field idle in header with `/` focus hint | — |
| S1 | close-up | Field focused, empty; placeholder + hint footer (`↑↓ navigate · ⏎ add 1 here · ⇧⏎ set count · ⌥⏎ want instead`) | `/` or click |
| S2 | close-up | `ligh` typed; both sections populated (IN THIS COLLECTION filter hits, ADD FROM CATALOG candidates); best catalog match pre-highlighted with `⏎ Have` chip | types `ligh` |
| S3 | close-up | Highlight moved one row — disambiguation costs one keystroke per step | `↓` |
| M2 | full | Landed: field cleared and still focused, new row in collection behind, count bumped, undo toast. **The loop point** — next card starts immediately | `⏎` |
| S4 | close-up | Count-entry variant: inline `× __` on highlighted row; digits then `⏎` commits | `⇧⏎` |
| S5 | close-up | Want-instead variant: row committed to WANTED; chip flips to `⏎ Want` styling for the action | `⌥⏎` |

### Deck context (Grixis Control, Want-led) — flip only

| Frame | Kind | State |
|---|---|---|
| D1 | close-up | Typed query with default flipped: highlighted match chip reads `⏎ Want`, footer swaps to `⌥⏎ have instead` |
| D2 | close-up | Landed at panel level: WANTED bumped, deck header needs chip ticks up |

Two frames suffice — everything else matches the binder walk; the point is that only the default flips.

### Catalog surface

| Frame | Kind | State |
|---|---|---|
| C1 | full | Results with sticky `Adding to: 📥 Inbox ▾` picker open: suggested/recent destinations first, then tree, type-ahead on top |
| C2 | close-up | Result tile after `+ Have`: count badge on tile, toast naming destination (`Added 1 → Inbox`) |
| C3 | close-up | Logged-out result: quick actions replaced by `Sign in to add` prompt (per auth split) |

### Mobile intake (Inbox)

| Frame | Kind | State |
|---|---|---|
| Mb1 | full | Inbox with type-ahead field, thumb-reachable layout |
| Mb2 | full | Field focused, OS keyboard up, suggestions in the space above; each catalog row carries a tap-target `+ Have` |
| Mb3 | full | Landed: field cleared, row present, toast with Undo above keyboard |

## Input-cost accounting

- One `note` per storyboard totals its input costs; one summary note carries the projection and risk calls.
- **Desktop steady state:** ~4–6 name characters + `⏎` ≈ **5–7 keystrokes per card, zero pointer use**; 50 cards ≈ 250–350 keystrokes. Detours: disambiguation `↓` ×n; set-count `⇧⏎` + digits + `⏎`.
- **Mobile steady state:** focus (1 tap, once per session) + ~5 characters + 1 tap on the match per card.
- Risk calls to validate against the storyboard: how often disambiguation is needed at 4–6 typed characters, and whether the set-count detour stays cheap enough for playset entry.

## Definition of done (per specs/README.md)

- Storyboards built in wireframes.pen per this design; findings + keystroke numbers recorded in [specs/ui-design.md](../specs/ui-design.md) Findings.
- TODO checkbox flipped `[x]` in the final commit.
- New component needs or follow-up work added as TODO tasks (component gap analysis is the next Phase 1b task), never silently absorbed.
