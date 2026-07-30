# Batch B — selection tray & batch move

Triage pass against code as of `docs/phase-6-triage` (2026-07-30). Read-only —
no files modified except this report. Line numbers below are current greps,
not the numbers in TODO-Phase-6.md (which have drifted).

Headline finding: commit `7649d80a` ("feat(ui): responsive audit, and close
the Phase 5 Stage 3 boundary (#77)", 2026-07-27) and the earlier "Undoable
removal + deck teardown" work (2026-07-25) already fixed several of these
entries. P6-045, P6-046d, P6-046e, P6-061i, and P6-061j are STALE. P6-061g's
specific cited assertion was also rewritten since filing.

## P6-045

Selection checkbox 16×16px tap target on mobile.

- **verdict**: STALE
- **evidence**: `app/src/components/ui/selection_tray.rs:230-231` —
  `SelectionCheckbox`'s outer `<span>` (the actual click target, not a bare
  checkbox) is `flex size-11 cursor-pointer items-center justify-center
  md:size-4` — a 44px tap target on mobile, reverting to the compact 16px
  control only at `md:`+. `git blame` dates this to commit `7649d80a`
  (2026-07-27), which post-dates the triage doc's snapshot of the code.
- **size**: — (already fixed)
- **disposition**: DROP
- **blocked-by / duplicate-of**: none

## P6-046 (bundle a–h)

### (a) tray counts entries, not copies
- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/selection_tray.rs:360-366` —
  `count_label(n)` returns `"1 card"` / `"{n} cards"` off `items.len()`
  (entry count), with no notion of per-entry quantity.
- **size**: S (decision first, per original triage)
- **disposition**: KEEP — bundle with P6-062 (quantity semantics decision)

### (b) multi-grain rows selectable but ungradeable
- **verdict**: CONFIRMED
- **evidence**: `app/src/my/collection.rs:1345` — `selectable =
  (present > 0).then(...)` gates the checkbox on `present` alone, ignoring
  `holding_id`. `app/src/my/collection.rs:1445-1453` — the count stepper
  (`HereCount`) explicitly refuses to render when `holding_id` is `None`
  ("several finishes or conditions here — edit them individually"), i.e. the
  stepper treats the same condition as unaddressable but the checkbox does not.
- **size**: S
- **disposition**: KEEP

### (c) same physical card from /my and a collection row yields two indistinguishable entries
- **verdict**: CONFIRMED
- **evidence**: `app/src/my/all_cards.rs:394` uses `SelectionKey::Card {
  oracle_id }`; `app/src/my/collection.rs:1339-1343` uses
  `SelectionKey::Held { collection_id, printing_id, board }`. No dedup by
  `oracle_id` anywhere in `selection_tray.rs`'s `toggle_in`/`items`, so both
  land in the tray as separate entries sharing the same `image_uri`
  (`TrayStack`, `selection_tray.rs:325-355`, renders one thumb per entry).
- **size**: S
- **disposition**: KEEP

### (d) toast paints over tray's clear "×"
- **verdict**: STALE
- **evidence**: `app/src/shell.rs:330-337` — `toaster_offset(tray_up)` moves
  the `Toaster` to `bottom-[8.5rem] md:bottom-[4.5rem]` whenever
  `!selection.is_empty()` (wired at `shell.rs:281-283`), clearing the dock.
  Landed in `7649d80a` alongside P6-045's fix.
- **size**: — (already fixed)
- **disposition**: DROP

### (e) dock offset from content column by half the sidebar rail
- **verdict**: STALE
- **evidence**: `app/src/shell.rs:357` — dock class now carries
  `md:left-60`, with a doc comment (`shell.rs:345-352`) explicitly describing
  this exact bug and the fix ("`md:left-60` is the sidebar rail's width, and
  it is what makes the pill centre on the table it describes rather than on
  the window"). Same commit, `7649d80a`.
- **size**: — (already fixed)
- **disposition**: DROP

### (f) aria-live count enters DOM with its first content
- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/selection_tray.rs:275-290` — the
  `aria-live="polite"` count span is a child of `<Show when=move ||
  !items.with(Vec::is_empty)>`, which mounts the whole tray (including the
  live region) on the first pick, so AT typically never announces it.
  Unchanged by the responsive-audit commit.
- **size**: S
- **disposition**: KEEP

### (g) selection entries go stale, nothing prunes
- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/selection_tray.rs` has no
  prune/invalidate logic — only `toggle`/`toggle_in`/`clear`
  (lines 145-176). No key-liveness check tied to holding/collection deletion
  or teardown.
- **size**: S
- **disposition**: KEEP

### (h) `collectionWithCards` fixture can hang the test
- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/selection-tray.spec.ts:148-160` —
  `collectionWithCards` picks any collection with `present > 0` from
  `/api/collections/tree`, then the consuming test
  (`selection-tray.spec.ts:224`) does
  `page.locator(COL_ROW_SELECT).first().waitFor()` without pinning to a
  specific card. If the collection's held row falls past the first
  name-ordered keyset page, no selectable checkbox is on-screen and the wait
  is unbounded — structurally confirmed; whether it actually flakes depends
  on seed-data ordering (unverified at runtime).
- **size**: S
- **disposition**: KEEP

## P6-047

No "select all on this page"; no per-collection checkbox in `/my` location breakdown.

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/all_cards.rs:348-350` — header select column is
  `<TableHead class="w-11 md:w-8"><span class="sr-only">"Select"</span>
  </TableHead>` — no control, just a label. `all_cards.rs:490-536` —
  `LocationSummary`'s multi-collection branch renders a plain `<ul>` of
  `<a>` links (`location-list`), no checkboxes.
- **size**: M
- **disposition**: KEEP

## P6-048

Selection tray has no keyboard entry point (`x` shortcut).

- **verdict**: CONFIRMED
- **evidence**: `specs/app-ui.md:108-109` specifies "select (checkbox /
  long-press / `x`) affordances" for `/my/collections/:id` rows. No `x`
  keydown handler exists anywhere touching `SelectionState`/`toggle` — the
  only selection-adjacent keydown handlers found are in `collection.rs:469`
  (count stepper) and `tree.rs:487` (context menu), neither wired to
  selection toggle.
- **size**: S
- **disposition**: KEEP

## P6-061 (bundle a–l)

### (a) `undo_selection_move` accepts uncapped `Vec<Id>`
- **verdict**: CONFIRMED
- **evidence**: `app/src/lib.rs:1182-1197` — `undo_selection_move` has no
  length check on `move_ids`, versus `move_selection` at `lib.rs:1047-1049`
  which errors past `SELECTION_MOVE_MAX` (100).
- **size**: S
- **disposition**: KEEP

### (b) `selection_destinations` silently `.take(100)`s
- **verdict**: CONFIRMED
- **evidence**: `app/src/lib.rs:1219` — `for oracle_id in
  oracle_ids.into_iter().take(SELECTION_MOVE_MAX)` truncates silently rather
  than erroring like `move_selection` does.
- **size**: S
- **disposition**: KEEP

### (c) `suggested` resource re-fires on every toggle, N+1 query
- **verdict**: CONFIRMED
- **evidence**: `app/src/my/move_selection.rs:544-563` — `Resource::new(move
  || oracles.get(), ...)` keys on a `Memo` of deduped oracle ids derived
  straight from `selection.items()`; any toggle that changes the oracle set
  refetches regardless of whether the picker is open. Server side
  (`lib.rs:1211-1227`) loops `suggested_destinations` sequentially, one query
  per oracle.
- **size**: S
- **disposition**: KEEP

### (d) batch-path TOCTOU (resolution vs. `move_batch` transaction)
- **verdict**: CONFIRMED, but deliberate and already documented
- **evidence**: `specs/app-ui.md:1366-1374` (Findings, "Undoable removal +
  deck teardown", 2026-07-25) names this exact residual TOCTOU as an accepted
  tradeoff — closing it fully would mean `move_batch` taking selection keys
  instead of move items, "a different API." Row-path TOCTOU was separately
  closed (`app-ui.md:1333-1345`, `move_holding` reading `FOR UPDATE` inside
  the write transaction).
- **size**: S
- **disposition**: PARK — trigger: revisit if `move_batch`'s addressing
  scheme changes, or if the TOCTOU is observed causing real double-moves

### (e) `HoldingsRevision` has only two consumers; `/cards/:oracle` not invalidated by a move
- **verdict**: CONFIRMED
- **evidence**: `grep -rn HoldingsRevision app/src` shows it wired into
  `/my` and `/my/collections/:id` paths only (`move_selection.rs`,
  `collection.rs`); no reference from a card-detail/`/cards/:oracle` route.
- **size**: S
- **disposition**: KEEP

### (f) tray's `empty="No collection to move to."` also shows for a search that filters everything out
- **verdict**: CONFIRMED
- **evidence**: `app/src/my/move_selection.rs:672` passes
  `empty="No collection to move to."` into `DestinationList`.
  `app/src/catalog/destination.rs:222-228,235` documents that `empty` "can
  only ever speak about filtering" and is shown by `CommandEmpty` whenever
  the item registry is empty — no separate signal distinguishes "nothing to
  move to" from "your search matched nothing." (The `failed`-read case *was*
  since split out — see states.spec.ts / selection-tray.spec.ts:116-121 — but
  that's a different collapse than this one.)
- **size**: S
- **disposition**: KEEP

### (g) `selection-tray.spec.ts` empty-state assertion is vacuous (`toContainText` vs `style:display`)
- **verdict**: STALE
- **evidence**: `CommandEmpty` (`app/src/components/ui/command.rs:315-326`)
  still hides via `style:display`, so the underlying anti-pattern is real in
  principle — but the specific assertion the entry cited no longer exists.
  `end2end/tests/selection-tray.spec.ts:116-128`'s comment states the old
  `toContainText("No collection to move to.")` assertion was replaced (part
  of "the state-arms task"); the current test uses
  `toContainText("Couldn't load your collections.")` /
  `not.toContainText("No collection to move to.")` against the *failed-read*
  arm, which is a real conditional unmount (`Show when=failed`), not a
  `style:display` toggle — so `toContainText` is valid there. A grep of other
  e2e files found the one genuine `style:display` case
  (`destination-picker.spec.ts:238`) correctly asserted with `toBeVisible()`,
  not `toContainText`.
- **size**: — (cited instance fixed)
- **disposition**: DROP

### (h) no client-side cap on selection size (>100 rows → raw server error string)
- **verdict**: CONFIRMED
- **evidence**: `SELECTION_MOVE_MAX` is never referenced anywhere under
  `app/src/components/ui/selection_tray.rs` or `app/src/my/move_selection.rs`
  — no client-side guard exists; a >100 selection would only be caught by the
  server's `lib.rs:1047-1049` check, surfacing as a `ServerFnError` string.
- **size**: S
- **disposition**: KEEP

### (i) `SkipReason::Board` renders "is on the side board" / "is on the maybe board"
- **verdict**: STALE
- **evidence**: `app/src/my/move_selection.rs:130-153` — current
  `SkipReason` enum is `NoCopies, AlreadyThere, Grain(usize),
  ManyCollections(usize), ManyPrintings(usize), ManyBoards(usize)`. No
  `Board` variant. `grep -rn "is on the side board"` across `app/src` and
  `shared/src` returns nothing. `specs/app-ui.md:1358-1360` (Findings,
  2026-07-25) confirms: "`movable` dropped to `quantity > 0`... so
  `SkipReason::Board` is gone entirely."
- **size**: — (variant removed)
- **disposition**: DROP

### (j) Toaster overlaps tray dock (duplicate of the selection-tray-round finding)
- **verdict**: STALE
- **evidence**: same fix as P6-046d — `toaster_offset` in `shell.rs:330-337`.
- **size**: — (already fixed)
- **disposition**: DROP; duplicate-of P6-046d (both fixed by the same commit)

### (k) two independent `list_collections` resources when catalog picker + tray both mounted
- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog/destination.rs:353` —
  `Resource::new(|| (), |_| async { collection_list().await })` for the
  catalog picker; `app/src/my/move_selection.rs:565-568` — a second,
  independent `Resource::new(|| (), ...)` calling the same
  `catalog::destination::collection_list()` for the tray. No shared cache.
- **size**: S
- **disposition**: KEEP

### (l) undo does not restore moved entries to the tray (deliberate)
- **verdict**: CONFIRMED (as described — deliberate, not a bug)
- **evidence**: `app/src/my/move_selection.rs:610` clears moved entries via
  `selection.remove_tokens(&outcome.moved)` on a successful move;
  `undo` (`move_selection.rs:708-739`) calls `undo_selection_move` and shows
  a toast but never re-adds anything to `SelectionState`.
- **size**: S
- **disposition**: PARK — trigger: revisit if user feedback says undo
  "losing" the tray selection reads as wrong

## P6-062

Batch move is one copy per entry, fixed server-side; needs quantity on `SelectedCard` + tray control.

- **verdict**: CONFIRMED
- **evidence**: `app/src/lib.rs:1109,1161` — `quantity: SELECTION_MOVE_QUANTITY`
  where `const SELECTION_MOVE_QUANTITY: i32 = 1`. `SelectedCard`
  (`selection_tray.rs:94-104`) has no quantity field. The toast text is
  literally `"(1 copy each)"` (`move_selection.rs:449`,
  `move_selection.rs:1093` test), confirming the entry's own note that this
  is "honest today rather than wrong."
- **size**: M
- **disposition**: KEEP — bundle with P6-046a (grain/quantity decision cluster)

## P6-063

Ambiguous `/my` selections are refused, not disambiguated.

- **verdict**: PARTLY
- **evidence**: `app/src/my/move_selection.rs:130-153` confirms
  `ManyCollections(usize)`, `ManyPrintings(usize)`, `ManyBoards(usize)`, and
  `Grain(usize)` all still exist and are returned as `CardSource::Refuse` —
  so the core claim ("refused, not disambiguated") holds for those four
  cases. But the entry's own framing — "non-mainboard entries are refused by
  name (`Grain` / `Board`) until the removal/teardown task widens the write
  path" — is now stale: `specs/app-ui.md:1358-1364` documents that the
  removal/teardown task (2026-07-25) already landed and removed the `Board`
  refusal entirely (`movable` now `quantity > 0`, grain-complete
  `MoveSource`), keeping only `Grain(n)` "deliberately" because "nothing
  about the row says which [grain] the checkbox meant." So the entry is
  correct about there still being refusal-not-disambiguation, wrong that this
  is gated on a still-pending task.
- **size**: M
- **disposition**: RESCOPE — drop the "until removal/teardown" framing (that
  task shipped); rescope to "add a picker-side disambiguation step for
  `ManyCollections`/`ManyPrintings`/`ManyBoards` (and possibly `Grain`,
  though that one is called out as deliberately kept)"

## Cross-cutting notes

- Two responsive-audit-era fixes (`7649d80a`, `81aab9f3`, and the teardown
  commit) retired five of the twenty sub-items in this batch (P6-045,
  P6-046d, P6-046e, P6-061i, P6-061j) plus made P6-063's framing stale and
  P6-061g's cited assertion moot. Whoever files the next round of triage
  batches should assume the "selection-tray review round" and "batch-move
  review round" source material predates these fixes.
- `specs/app-ui.md` Findings sections ("Undoable removal + deck teardown
  (2026-07-25)" and "Responsive audit + stage close (2026-07-27)") are the
  authoritative record of what already shipped against this surface — worth
  reading before re-filing anything in this batch.

## UNITS proposal for the rewritten queue

- (P6-046a)+(P6-046b)+(P6-046c)+(P6-062) → "settle tray grain & quantity
  semantics" (L) — decision first: what a tray entry counts, how multi-grain
  rows are handled, and per-row quantity capability are one coherent design
  question.
- (P6-046f)+(P6-046g)+(P6-046h) → "tray a11y & staleness hygiene" (S) —
  first-pick announcement, stale-entry pruning, pin the e2e fixture.
- (P6-047)+(P6-048) → "expand selection entry points" (M) — select-all,
  per-location checkbox, `x` keyboard shortcut.
- (P6-061a)+(P6-061b)+(P6-061h) → "enforce SELECTION_MOVE_MAX consistently"
  (S) — undo endpoint, destinations ranking, and a client-side cap all need
  the same ceiling.
- (P6-061c)+(P6-061k) → "dedupe & gate tray data resources" (S) — suggested
  destinations and list_collections both refetch/duplicate more than needed.
- (P6-061e)+(P6-061f) → "close small batch-move gaps" (S) — `/cards/:oracle`
  ownership invalidation, empty-vs-no-match string precision.
- (P6-063) → "add ManyCollections/ManyPrintings/ManyBoards disambiguation to
  the tray picker" (M), rescoped per above.
- Parked, no unit: P6-061d (residual TOCTOU, documented tradeoff), P6-061l
  (undo doesn't restore tray, deliberate).
