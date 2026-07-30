# Batch A — tree management

Fast triage pass over `P6-015`, `P6-023`, `P6-026`, `P6-093` against HEAD
(`docs/phase-6-triage`, atop `f9639d6`). Read-only — no files touched other than
this report. Verbatim entry text pulled from `specs/TODO-Phase-6.md` and the
prior classification row in `specs/TODO-Phase-6-triage.md`.

## P6-015 — IA doc vs. server disagree on renaming the Inbox

**Claim:** `design/information-architecture.md:37` calls the Inbox "undeletable,
**renamable**"; `app/src/backend/hosted.rs:492` carries `AND NOT is_inbox` on
rename, so the API refuses. The UI now follows the server on two surfaces (tree
row menu, header kebab), cementing the guard.

**Verdict: CONFIRMED**

- `app/src/backend/hosted.rs:492` — `rename_collection` still runs
  `UPDATE collections SET name = $2 WHERE id = $1 AND NOT is_inbox`. The guard is
  unchanged.
- `design/information-architecture.md:37` — still reads: "**Inbox** is a real
  collection (undeletable, renamable) pinned first in the tree...". The doc has
  not been updated to match the code, and the code has not been relaxed to match
  the doc.
- Did not re-verify the "two UI surfaces both follow the server" clause beyond
  what the entry already asserts (line-number drift aside, the guard's existence
  is the load-bearing fact and it holds); not worth a separate tool call at
  triage depth.

**Size: S** — either fix (drop the guard, or edit one doc line) is a small,
mechanical change once someone picks a side. The `S` in the prior triage row is
right; the qualifier "decision first" is the actual blocker, not code size.

**Disposition: KEEP**, gated on a maintainer ruling (doc wrong vs. guard wrong).
Not blocked by any other P6 id in this batch.

---

## P6-023 — sibling reorder is drag-only (mouse-only)

**Claim:** Reordering a collection among siblings it is already among has no
keyboard or touch path — `Move to…` (from the tree-move task) covers
reparenting, landing the moved node last among new siblings, but not
within-parent position changes. No ordering UI exists because no wireframe
specifies one.

**Verdict: CONFIRMED** — and the code's own doc comments already admit the gap
as a tracked follow-up, not something I had to infer:

- `app/src/my/tree_manage.rs:1070-1072` (doc comment on `plan_move`): "**What it
  does not cover: reordering among siblings you are already among.** ... moving a
  row up or down within one group is still drag-only. Queued as follow-up..."
- `plan_move`'s destinations (`move_destinations`, ~line 1031) and `commit_move`
  (~line 1104) only ever call `reparent_collection` + `reorder_collection` for a
  *new* parent; there is no code path that reorders within an unchanged parent
  outside the raw HTML5 drag handlers feeding `plan_drop`/`commit_drop`
  (`DropIntent::Before`/`After`, ~line 1005-1024), which fire only on
  `dragover`/`drop` DOM events — no keyboard or touch equivalent exists anywhere
  in `tree.rs` or `tree_manage.rs`.
- No wireframe reference to a reorder affordance found in a scan of the grep
  hits above; the entry's "no wireframe specifies one" claim was not
  independently re-verified against `design/wireframes.pen` (binary/opaque
  format, out of triage budget) but nothing in the Rust code contradicts it.

**Size: L, design first** — matches the prior triage row. This is a real
accessibility/mobile gap, but the fix is blocked on inventing an affordance
(up/down buttons? a keyboard shortcut while a row is focused? a touch
long-press reorder mode?) before any code gets written.

**Disposition: PARK** — trigger: a wireframe/design decision defining the
non-drag sibling-reorder affordance. Until that exists this is not
actionable as a coding task.

**Related, not duplicate:** P6-093 also touches `plan_drop`/sibling `position`
math, but is a distinct concern (numeric precision, not affordance).

---

## P6-026 — `ContextMenuItem` gaps: no `aria-disabled`, no typeahead, ESC uncoordinated

**Claim:** `ContextMenuItem` has no `aria-disabled`, no typeahead, and the
menu's ESC dismissal is still not coordinated through an overlay stack — all
three pre-existing, all three more visible now the menu is keyboard-operable.

**Verdict: CONFIRMED**

- **No `aria-disabled` / disabled state:** `ContextMenuItem`
  (`app/src/components/ui/context_menu.rs:459-486`) takes only `on_select`,
  `children`, `class` — no `disabled` prop, and a repo-wide grep for
  `"disabled"` in this file returns zero hits.
- **No typeahead:** grep for `"typeahead"` in the file returns zero hits; the
  `on:keydown` roving-focus handler (~line 335 onward) only handles
  `ArrowDown`/`ArrowUp`/`Home`/`End` — no character-key first-match jump.
- **ESC not overlay-stack-coordinated:** the file's own module doc says so
  explicitly at `context_menu.rs:28-29`: "ESC and outside-pointerdown dismissal
  are our own `window` listeners... ESC is not overlay-stack-coordinated — same
  known caveat as `popover`." The `window_event_listener(keydown, ...)` at
  ~line 311-316 confirms: it closes on `Escape` unconditionally whenever `open`
  is true, with no coordination against other overlays (dialogs, popovers) that
  might also want the keystroke.
- The module doc also confirms the "newly visible because keyboard-operable"
  framing: "**the panel is keyboard-operable** (added for the tree's
  `Move to…`...). Upstream had none of this — the panel was right-click-only."

**Size: M** — matches the prior triage row; three independent, moderate,
well-scoped a11y fixes in one file.

**Disposition: KEEP.** Real, not urgent-critical, correctly sized. Could be
split into three standalone units (`aria-disabled`, typeahead, ESC
overlay-stack coordination) if picked up, but the entry doesn't use `(a)/(b)/(c)`
lettering in `TODO-Phase-6.md` today so no forced split here.

---

## P6-093 — fractional sibling `position` can collide / exhaust precision

**Claim:** `plan_drop` in `app/src/my/tree_manage.rs` computes drop positions as
the midpoint of neighboring siblings; repeated midpoint inserts between the same
pair (or an existing tie) can produce a position equal to a neighbor's, silently
falling back to name-order. Deemed unreachable at POC scale (~50 inserts to
exhaust f64 between one pair); deferred rather than building a rebalance.

**Verdict: CONFIRMED**

- `app/src/my/tree_manage.rs:988-1029` (`plan_drop`) still computes, for
  `DropIntent::Before`/`After`:
  `(Some(a), Some(b)) => (a + b) / 2.0` with no collision check, no epsilon
  guard, no rebalance path.
- Repo-wide grep for `renumber`/`rebalance`/`f64::EPSILON`/"too close" across
  `app/src/my/tree_manage.rs`, `app/src/backend/hosted.rs`, and
  `specs/collection-api.md` returns zero hits — the fix described in the entry
  ("renumber a sibling group to evenly-spaced integers when a midpoint gets too
  close to a neighbor") has not been built.
- The module doc at `tree_manage.rs:4-6` still describes the mechanism as
  "reorder by dropping on a row's edge band — fractional `position` midpoints",
  consistent with the entry's description; nothing has changed since the Codex
  review that originally flagged it.

**Size: deferred** (per prior triage row — this is explicitly a "fix when it
bites" item, not sized as near-term work).

**Disposition: PARK** — trigger: observed position collisions in real data
(repeated midpoint inserts between the same sibling pair — the entry's own
napkin math puts exhaustion at ~50 inserts between one pair given integer seed
positions — or ingestion/import paths that seed fractional, not integer,
positions from day one, which would erode the safety margin much faster).

**Related, not duplicate:** shares `plan_drop`/`tree_manage.rs` real estate with
P6-023 but is a distinct numerical-precision concern, not an affordance gap.

---

## Summary

All four entries CONFIRMED — no drift, nothing fixed or moot since these were
written. None block or duplicate each other; P6-023 and P6-093 are adjacent
(same function, `plan_drop`) but orthogonal concerns. P6-015 and P6-026 are
independent of the other three. No sub-item lettering (`(a)/(b)/(c)`) appears in
any of these four `TODO-Phase-6.md` entries, so no forced-split `UNITS:` is
required by the batch protocol; P6-026 bundles three related a11y fixes that
could be split if picked up, noted above.
