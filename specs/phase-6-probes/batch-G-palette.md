# Batch G — palette / quick-add / overlays

Fast triage pass, 2026-07-30. Read-only. Line numbers below are current as of
this pass (the queue's cited numbers have drifted).

## P6-028 — palette has no focus trap; ⌘K stacks over other modals

### (a) missing focus trap, framed as a vendored-`Dialog` gap

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/palette.rs:892-908` — the open-`Effect`
  only calls `focus_field()` (moves the caret into the search input); no
  `keydown` handler constrains `Tab` to the dialog's focusable set anywhere in
  the file. Traced the claim that this is a vendored-`Dialog` gap: the palette
  is `CommandDialog` (`app/src/components/ui/command.rs:420-437`), which wraps
  `Dialog`/`DialogContent` from `app/src/components/ui/dialog.rs`; that file
  has **zero** `Tab`/`keydown`/`trap` handling (only an `Escape` listener at
  `:136`, confirmed absent by `grep`). `Dialog` has exactly three other
  consumers — `app/src/components/ui/command.rs`, `app/src/my/collection.rs`,
  `app/src/my/tree_manage.rs` (its create/rename dialogs) — so a fix in
  `dialog.rs` genuinely reaches all of them, not just the palette. It does
  **not** reach `Sheet` or `Popover`: both are independently vendored
  (`app/src/components/ui/sheet.rs`, `popover.rs`) with their own markup and
  no shared trap logic, so "every dialog in the app" is accurate only for
  literal `Dialog`/`CommandDialog` consumers, not the wider overlay family.
  The priority read stands: fixing this in `dialog.rs` is a primitive-level
  fix with a 4x multiplier (palette, command.rs, collection.rs, tree_manage.rs
  create/rename), not a palette-only patch.
- **size**: M (the trap itself is contained, but it's `dialog.rs` — a shared
  primitive touched by 4 call sites, so the verification surface is wider)
- **disposition**: KEEP — RESCOPE the title/description to say explicitly
  "fix in `dialog.rs`, verify against all 4 `Dialog` consumers" so whoever
  picks it up doesn't scope it as a palette patch.
- **blocked-by / duplicate-of**: none found overlapping this exact gap.

### (b) ⌘K chord toggles unconditionally, stacking over an open dialog

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/palette.rs:760-766` — the `keydown`
  listener does `open.update(|o| *o = !*o)` on chord match with no check of
  `manage.create_open`/`rename_open`/any other overlay's open state. Nothing
  in `PaletteBody` reads `TreeManage`'s dialog flags before toggling.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: none.

---

## P6-030 — palette minors (a–h)

### (a) `RowSet::key` omits `Place::meta`

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/palette.rs:998-1012` — `key()` builds from
  `places_label`, `commands_first`, `p.key.token()`, `p.name`, and
  `c.slug()`; `meta` (destructured separately in `PlaceRow`,
  `palette.rs:1103-1109`) never enters the string.
- **size**: S
- **disposition**: KEEP.

### (b) same key, no section delimiter between places and commands

- **verdict**: CONFIRMED
- **evidence**: same block, `palette.rs:998-1012` — every field (place
  token+name pairs, then command slugs) is joined with the same `'\u{1f}'`
  byte; nothing marks the places/commands boundary. A collision needs a
  collection literally named e.g. `new-binder`, matching the entry's own
  characterization.
- **size**: S
- **disposition**: KEEP — bundle with (a), same function, same fix window.

### (c) palette's `Undo last move` bypasses the pick-list's `on_undo`

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/needs.rs:762-766` — `toggle` refuses to re-tick a
  row once `checked.get_untracked()` is true. The palette's
  `PaletteCommand::UndoLastMove` (`app/src/components/palette.rs:789-822`)
  calls `crate::undo_move`/`crate::undo_selection_move` directly and never
  touches `needs.rs`'s local `done: RwSignal<HashSet<String>>` or any
  `on_undo` callback — that callback only fires from the **toast's** own Undo
  button (`app/src/my/needs.rs:875-897`, inside `report()`). A pick-list line
  ticked via the toast path stays ticked/struck-through if reversed from ⌘K.
- **size**: S
- **disposition**: KEEP.

### (d) `is_mac()` reads deprecated `navigator.platform`

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/palette.rs:1188-1195` —
  `window().navigator().platform()`. `userAgentData.platform` isn't in
  `web-sys`, matching the entry's stated reason for not using it.
- **size**: S
- **disposition**: KEEP — low priority, no forcing function.

### (e) `tr_recent_places` is per-origin, not per-user, survives sign-out

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/palette.rs:1216-1234` — `load_recents`/
  `store_recents` go straight to `window().local_storage()` keyed by the
  origin-scoped `RECENT_STORAGE_KEY` constant (`:118`), no user id folded in.
  `grep -rn RECENT_STORAGE_KEY` finds no sign-out/logout clear anywhere.
  `at_rest`'s reconcile (`palette.rs:441-460`) does filter the ring against
  the live index and drop keys the index no longer has — the only backstop,
  exactly as described.
- **size**: S
- **disposition**: KEEP.

### (f) hoisting `provide_tree_manage` to the shell lets dialog state survive a mode switch

- **verdict**: PARTLY — mechanism confirmed, but the current effect is worse
  than "reappears on return."
- **evidence**: `app/src/my/tree_manage.rs:280` `provide_tree_manage()` is
  called exactly once, from `app/src/shell.rs:187` (shell root, not
  per-mode). `TreeDialogs` (`tree_manage.rs:446`) is likewise mounted once at
  the shell (`shell.rs:292`), as a sibling of `<main><Outlet/></main>`
  (`shell.rs:266`), **not** nested inside `SidebarRail`/`CollectionTreeNav`
  and not gated on `my_mode` anywhere in `TreeDialogs`'s body. Because of
  that, an open create/rename dialog does not merely "reappear on return" —
  since `TreeDialogs` renders unconditionally regardless of route, the open
  `Dialog` stays visibly mounted (its `open=manage.create_open` signal is
  untouched by navigation) straight through a Catalog-mode visit, overlaying
  the Catalog page the whole time, not just resurfacing when you switch back.
- **size**: S
- **disposition**: KEEP — RESCOPE the description to match what's actually
  observable ("the dialog stays open across the whole mode switch, not just
  on return") before anyone picks this up, or a repro attempt will look like
  it doesn't match the ticket.
- **blocked-by / duplicate-of**: none.

### (g) `TreeDialogs` invisible below `md` (inside `hidden md:block` aside)

- **verdict**: STALE — already fixed, and the fix is documented in-place.
- **evidence**: `app/src/my/tree_manage.rs:274-279` — doc comment: "Provided
  by the **app shell**, not by the tree... `TreeDialogs` is mounted at the
  shell for the same reason plus a sharper one: the sidebar is off-screen
  below `md`, and a dialog cannot be shown from inside a hidden subtree."
  `shell.rs:288-292` confirms `<TreeDialogs />` is mounted at the shell root,
  outside any `hidden md:block` wrapper, and `shell.rs:417-419` documents the
  sidebar itself no longer being `hidden md:block` either ("It used to be
  `hidden md:block`, which meant a phone had no collection tree at all") —
  it's now a slide-over drawer at every width. Both premises of this
  sub-entry (dialogs gated behind `hidden md:block`, no mobile story) are
  gone.
- **size**: — (n/a, drop)
- **disposition**: DROP — superseded by a shell-level fix that already
  shipped and left comments explaining exactly why.
- **blocked-by / duplicate-of**: fixed as a side effect of whatever change
  produced (f)'s shell hoist.

### (h) `Undo` recording verified only on the removal path

- **verdict**: CONFIRMED
- **evidence**: `grep -rn note_last_move app/src` shows 5 call sites —
  `catalog.rs:1092`, `my/move_selection.rs:620`, `my/needs.rs:872`,
  `my/collection.rs:1526,1701`. `end2end/tests/command-palette.spec.ts`'s
  three `Undo last move` tests (`:442`, `:484`, `:524`) all drive
  `removeStack` (a `my/collection.rs` removal) and reverse it via ⌘K.
  `end2end/tests/quick-add.spec.ts`, `batch-move.spec.ts`, `needs.spec.ts`
  all assert their own toast's "Undo" button, never the ⌘K command — so the
  `note_last_move` calls in the quick-add/batch-move/pull paths are not
  exercised by any ⌘K-driven assertion; a dropped call there would not fail
  the suite.
- **size**: S
- **disposition**: KEEP — this is a test-coverage gap, not a code bug;
  candidate for a small e2e addition rather than a prod fix.
- **blocked-by / duplicate-of**: none.

---

## P6-069 — quick-add panel minors (a–k)

### (a) `PresentSection` has no empty-query gate

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/quick_add.rs:617-641` — gates only on
  `!present.read().is_empty()`. `present` is computed in
  `app/src/my/collection.rs:253-259` from `present_matches(&v.cards)` where
  `v.cards` comes straight from `collection_view`'s current page — no
  emptiness check on the search `q` anywhere in that chain. With `?q=` empty,
  `v.cards` is the collection's unfiltered first page, so every present card
  on it renders under "IN THIS COLLECTION."
- **size**: S
- **disposition**: KEEP.

### (b) `Escape` decodes to `Pass` when `rows == 0`

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/quick_add.rs:121-124` — `decode()`'s
  first line is `if rows == 0 { return Action::Pass; }`, which short-circuits
  before the `"Escape" => Action::Cancel` arm is ever reached.
- **size**: S
- **disposition**: KEEP.

### (c) early-return failure paths in `add` leave `count` set

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/quick_add.rs:443-467` — both the
  no-destination and the Have-with-no-printing early returns happen after
  `pending.set(None)` but before any `count.set(None)`; that reset only
  happens in the success branch (`:476`). A stale `×N` count survives the
  failure and would be reused by a bare `⏎`.
- **size**: S
- **disposition**: KEEP — minor as stated (any keystroke clears the chip).

### (d) doc drift: `here` vs. the actual HERE cell

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/quick_add.rs:188-191` doc claims `here`
  (`present + present_rollup`) is "the same number the page's HERE column
  renders." The actual cell, `app/src/my/collection.rs:1395-1415`, renders
  `<HereCount present>` (own count only) plus a **separate** dimmed
  `+{present_rollup}` span when rollup > 0 — two numbers, not one summed
  figure. The sum matches `here_total()` (`collection.rs:1049-1050`), not the
  cell's rendered text.
- **size**: S
- **disposition**: KEEP — doc fix only.

### (e) grammar mismatch: `search_catalog` vs. `collection_view`'s plain substring

- **verdict**: CONFIRMED
- **evidence**: candidates go through `crate::search_catalog`
  (`quick_add.rs:359`); doc comments at `app/src/lib.rs:488-491` and
  `:530-531` state, respectively, that `all_cards`'/`collection_view`'s `q`
  is "a plain name substring, deliberately not the catalog grammar." Two
  different query languages behind one field, confirmed as described.
- **size**: S
- **disposition**: KEEP — doc/UX fix, not urgent.

### (f) post-add `clear()` always pushes a history entry

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/query_bar.rs:95-106` — `commit`'s
  `replace = was_searching && !q.is_empty()`; `clear()` (`:167-173`) calls
  `commit(String::new())`, so `q.is_empty()` is always true there, forcing
  `replace = false` on every add-triggered clear.
- **size**: S
- **disposition**: KEEP.

### (g) after Escape, field keeps focus so `focusin` can't re-fire

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/quick_add.rs:538-545` — `Action::Cancel`
  (non-counting) does `open.set(false)` only, no `.blur()`. The
  focus-holding interval that could re-trigger anything is itself gated on
  `open.get_untracked()` (`:414-417`), so once closed it's inert. The
  `on:focusin=move |_| open.set(true)` handler (`:576`) needs a real focus
  transition to fire, which a still-focused input never produces again on
  its own — matches "needs typing or a click away and back."
- **size**: S
- **disposition**: KEEP.

### (h) missing ARIA on the field; footer sits inside `role="listbox"`

- **verdict**: CONFIRMED
- **evidence**: the quick-add field is `QueryBar`'s plain
  `<InputGroupInput>` (`app/src/components/query_bar.rs:198-210`) — no
  `aria-expanded`/`aria-controls`/`aria-activedescendant` anywhere in that
  component (it does not use the shared `CommandInput`, which is a different,
  unrelated field used by the palette). The panel container is
  `role="listbox"` (`quick_add.rs:595`) wrapping `<CommandList>` (holding
  `PresentSection`'s `<a>` rows and `CandidateSection`'s `role="option"`
  rows) **and** `<PanelFooter>` as a sibling inside the same listbox div
  (`quick_add.rs:608-613`).
- **size**: S
- **disposition**: KEEP — bundle with (g), both are the field/panel a11y
  wiring.

### (i) `use_command_nav().expect(...)` is a runtime panic, not a compile guarantee

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/quick_add.rs:329` and `:739` — both
  `QuickAddSurface` and `Candidate` call
  `use_command_nav().expect("... renders inside a Command")`. Self-evident
  from the code: any `.expect()` is a wasm panic path if the invariant is
  ever violated, not something the type system enforces.
- **size**: S
- **disposition**: KEEP — low priority, matches every other `.expect()`
  pattern already accepted elsewhere in this codebase for context-dependent
  components.

### (j) `Candidate.index` (local) vs. `nav.highlighted()` (registry-wide) could disagree

- **verdict**: PARTLY / UNVERIFIABLE without a runtime repro
- **evidence**: `app/src/components/quick_add.rs:729-742` — `index` is
  `cards.into_iter().enumerate()` local to one `CandidateSection` render
  (`:706-717`); its own doc comment claims this equals "the index `command`'s
  highlight is expressed in" because "nothing is filtered." `nav.highlighted()`
  (`ui/command.rs:192`) is the shared registry position across the whole
  `Command`. The failure mode the entry describes needs two `CandidateSection`
  generations mounted at once during a `Transition` swap — plausible given
  `Transition`'s stale-content-during-pending behavior, but confirming it
  actually happens (rather than being prevented by Leptos's reconciliation)
  needs a DOM inspection during a rapid query change, not a static read.
  **Runtime check to confirm/deny**: rapid-fire two different quick-add
  queries and inspect whether two `data-testid="quick-add-candidate"` groups
  are ever simultaneously in the DOM with overlapping `index` values.
- **size**: S
- **disposition**: KEEP — PARK (trigger: someone actually reports two chips
  live, or a query-swap flicker becomes visible in manual testing).
- **blocked-by / duplicate-of**: none.

### (k) `Resource::get()` serves stale payload for one round trip after a collection switch

- **verdict**: CONFIRMED
- **evidence**: inherent Leptos `Resource` semantics (stale-while-refetching
  unless explicitly cleared) — `app/src/my/collection.rs`'s `resolved` Memo
  passes `view_res.get()` straight through with no pending-state
  differentiation beyond the outer `Transition`'s initial-load fallback,
  which does not fire again on a source-key change. Cosmetic as stated: both
  header and body are equally stale for that one round trip.
- **size**: S
- **disposition**: KEEP — cosmetic, low priority.
- **blocked-by / duplicate-of**: none.

---

## P6-071 — `AddToast` (8 fields) / `QuickAddPanel` (10 props) at the edge of comfortable

- **verdict**: PARTLY — `QuickAddPanel`'s count is exactly right;
  `AddToast`'s count is stale and has already grown past the cited number.
- **evidence**: `QuickAddPanel` (`app/src/components/quick_add.rs:238-266`)
  has exactly 10 props: `field_id`, `text`, `url_q`, `to_url`, `placeholder`,
  `aria_label`, `destination`, `default_kind`, `present`, `on_undo` —
  confirmed. `AddToast` (`app/src/catalog.rs:1044-1062`) now has **9**
  fields — `toast`, `tree`, `name`, `dest`, `kind`, `quantity`,
  `undo_move_id`, `after_undo`, `last_move` — one more than the entry's
  cited 8 (`last_move` reads as the newest addition, per its doc comment
  referencing ⌘K's `Undo last move`). The entry's own thesis ("group them if
  either grows again") has already happened on the `AddToast` side.
- **size**: S
- **disposition**: KEEP — RESCOPE: update the count to 9 and treat this as
  "do it now," not "watch for future growth," since the growth condition
  already fired.
- **blocked-by / duplicate-of**: none.

---

## P6-088 — overlay children instantiated eagerly; the V2 "revisit if expensive" trigger has fired

- **verdict**: CONFIRMED
- **evidence**: `Dialog`, `Sheet`, and `Popover` all take `children: Children`
  (called once, unconditionally, at construction — not `ChildrenFn` gated by
  a `Show`). Confirmed directly: `app/src/components/ui/dialog.rs:56-67`
  (`{children()}` inside `<Provider>`, no `Show`/`open` gate around it),
  `sheet.rs:92-100`, `popover.rs:161`. The cited incident is real and its
  workaround is in the tree today: `app/src/cards.rs:283-397`'s `CardPreview`
  — `SheetContent`'s body is gated by a **caller-local latch**
  (`sheet_seen`, `:385-390`, `<Show when=move || sheet_seen.get()>`) with a
  comment explaining why it's keyed on the latch rather than the live
  `sheet_open` signal (unmounting on `sheet_open` would empty the sheet mid
  close-animation). This is exactly "fixed with a per-caller latch" — the
  primitive itself (`sheet.rs`) still eagerly builds children on every
  instantiation; only this one consumer worked around it.
- **size**: M, decision first (matches the existing triage row) — the code
  work is contained, but "should `dialog`/`popover`/`sheet` gate children on
  open by default" is a design decision with consumer-visible tradeoffs
  (a caller relying on eager mount for prefetch, or hitting the same
  close-animation-vs-latch problem `cards.rs` already solved once, would need
  to re-solve it or lose the default).
- **disposition**: KEEP — needs the decision made explicit (a short doc note
  or spec answer) before code changes; today it's still "the next consumer
  rediscovers the latch pattern," as the entry warns.
- **blocked-by / duplicate-of**: none found; `cards.rs`'s latch is precedent,
  not a duplicate.

---

## P6-095 — vendored `collapsible`'s `class` padding leaks height when closed

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/collapsible.rs:97-107` —
  `CollapsibleContent`'s outer div carries the grid-collapse classes
  (`grid-rows-[0fr]` closed / `grid-rows-[1fr]` open, `overflow-hidden`); the
  caller's `class` prop lands on the **inner** div merged with `min-h-0`
  (`:105`). The doc comment above the component (`:81-83`) still reads "class
  applies to the inner content div (padding, flex, gap, etc.)" — no warning.
  The exact bug and workaround are already documented and applied at the
  cited call site: `app/src/my/all_cards.rs:507-511`'s comment states
  verbatim "`min-h-0` zeroes the content box, not the padding box" and the
  fix moved padding onto the child `<ul class="space-y-0.5 pt-1">`
  (`:513`) instead of `CollapsibleContent`'s own `class`. The other
  `CollapsibleContent` consumer, `app/src/my/tree.rs:396-403`, passes no
  padding class at all, so it's unaffected either way.
- **size**: S
- **disposition**: KEEP — the fix is either a doc-comment update on
  `collapsible.rs` (cheapest: state the constraint) or moving the clipped
  region so `class` is safe again; the workaround already proves the doc-only
  fix is sufficient to unblock consumers, so that's the cheaper path.
- **blocked-by / duplicate-of**: none.
