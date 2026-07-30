# Batch D — moves / removal / teardown

Fast triage pass, 2026-07-30. Read-only. Line numbers below are current as of
this pass (the queue's cited numbers have drifted).

## P6-031 — teardown toast has no Undo

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/collection.rs:1702` — the success path builds
  `ToastOptions::message("Emptied — {moved} card{s} moved")` with **no
  `.action("Undo", …)`**, while `receipt.move_ids` is right there at `:1700`
  and is already handed to `note_last_move` at `:1701` (so ⌘K can reverse it).
  `HostedBackend::teardown` returns `TeardownReceipt { move_ids }`
  (`app/src/backend/hosted.rs:1256`), and `undo_selection_move` /
  `CollectionStore::undo_moves` already takes a `Vec<Id>`
  (`app/src/lib.rs:1182`). The wiring is a toast action over an existing
  endpoint.
- **size**: S
- **disposition**: KEEP. Absorbs `P6-055g` (see below).
- **blocked-by / duplicate-of**: absorbs `P6-055g`; adjacent to `P6-056`.

## P6-039 — three definitions of "owned" in `hosted.rs`

- **verdict**: CONFIRMED, and now **four** sites, not three.
- **evidence**:
  1. the `owned_by_card` view, read by `owned_by_oracle`
     (`app/src/backend/hosted.rs:100`) — now shared by `card_summary` (`:260`)
     **and** `search` (`:314`);
  2. `all_cards`' inline `held` CTE (`:1274`) — `sum(h.quantity)::int` over
     `holdings JOIN printings GROUP BY p.oracle_id`;
  3. `collection_tree`'s `shopping_short` `o` CTE (`:417`) — same shape,
     un-`::int`-cast;
  4. `shopping_list`'s `o` CTE (`:1458`) — same shape again, `::int` cast.
  (3) and (4) are byte-near-identical and are the pair that must agree for the
  sidebar badge and the shopping page to match; nothing enforces any of it.
- **size**: S
- **disposition**: KEEP, RESCOPE to "collapse the three inline CTEs onto the
  `owned_by_card` view (or one shared SQL const) and add the agreement test" —
  and say **four** sites in the entry text.
- **blocked-by / duplicate-of**: —

## P6-054 — `set_holding_quantity(id, 0)` DELETEs with no ledger entry

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:702-714` — `if req.quantity <= 0 {
  DELETE FROM holdings WHERE id = $1 … return Ok(None) }`, no `append_move`, no
  ledger row, inside a plain `scoped_tx`. The "no finger on the trigger" claim
  also still holds: `app/src/my/collection.rs:1557-1561` — `on_commit` routes
  `c.to == 0` to `remove(c.from)` → `crate::remove_holding` →
  `move_holding(to = None)` (`app/src/lib.rs:625-631`), and the stepper's
  `caller_reports=|c| c.to == 0` (`collection.rs:1616`) hands that commit to the
  caller. No shipped caller reaches the DELETE branch.
- **size**: M (decision first: route quantity *decrements* through the ledger,
  or make only 0 a move and accept the inconsistency)
- **disposition**: KEEP as a spec-owner decision, then implement. Do **not**
  patch blind.
- **blocked-by / duplicate-of**: shares the ledger-consistency question with
  `P6-055c`.

## P6-055 — removal/teardown minors (bundle a–l)

### (a) dead `holding_id` after remove → Undo

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/collection.rs:1492-1501` — the undo path sets
  `here_delta`, `removed.set(false)` and `value.set(copies)` **synchronously**,
  then bumps `revision` (`HoldingsRevision`, a source of the page's async
  resource — `collection.rs:164,179`). The stepper is therefore re-shown against
  the id captured by `on_commit` at `:1557` for the whole duration of the
  refetch; the code's own comment at `:1496-1498` admits "the id this closure
  captured is dead for good". An edit in that window posts the dead id and the
  error branch at `:1577` renders "Couldn't save: not found: holding".
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: pairs with (b).

### (b) an earlier stepper Undo toast survives a removal

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/count_stepper.rs:213-232` — every
  non-`caller_reports` commit shows a toast whose `undo` callback re-runs
  `on_commit` with the captured row state; its only guard is
  `value.try_get_untracked()` (row disposed) and `cur != from`. A removal does
  **not** dispose the row (the view is deliberately not refetched —
  `collection.rs:1522-1524`), so a 3→1 toast still standing after the removal
  fires `set_holding_quantity(dead_id, 3)` and errors.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: pairs with (a).

### (c) `remove_holding` takes the whole stack while the toast names the rendered count

- **verdict**: CONFIRMED (as a reporting mismatch — the write is deliberate)
- **evidence**: `app/src/lib.rs:629` sends `quantity: None`, and `:615-617`
  now explicitly defends that ("**The whole stack, not a client-supplied
  count**"). But `app/src/my/collection.rs:1527-1535` still builds
  `"Removed {label} ({copies} copies)"` from `c.from`, the *rendered* count. A
  stack grown in another tab loses more copies than the toast — the user's only
  record — names.
- **size**: S
- **disposition**: KEEP, RESCOPE to "make the removal receipt carry the count
  actually removed and report that" (the write stays whole-stack).
- **blocked-by / duplicate-of**: —

### (d) deck section slot counts ignore `here_delta`

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/collection.rs:1146-1160` — `section_slots` sums
  `r.row.present` straight off the fetched rows; the header at `:1224` renders
  `{label} " · " {slots}` from that pure value. The page header *does* apply the
  delta (`:664`, `:698`). After a removal the section header contradicts both
  the row's "—" (`:1607`) and the page header until a refetch.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: pairs with (e).

### (e) a removed row keeps its selection checkbox

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/collection.rs:1345` — `let selectable = (present >
  0).then(|| SelectedCard { … })`, computed once from the pre-removal fetched
  `present`; it is not reactive on the row's `removed` signal (which lives
  inside `HereCount`, `:1464`). The checkbox renders at `:1380` regardless, so
  selecting a removed row yields a `NoCopies`/`Conflict("no copies to move")`
  refusal instead of being unselectable.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: pairs with (d).

### (f) `teardown` accepts `to_collection_id == collection_id`

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:1193-1200` — `teardown` calls
  `require_owned_collection` on `collection_id` and, for `EmptyTo`, on
  `to_collection_id`, but **never compares them**. The loop at `:1226-1253`
  then does `holding_take(collection_id, …, board)` +
  `holding_add(dest = collection_id, …, Board::Main)` and `append_move` with
  `Some(collection_id)` on both ends — collapsing every board onto `main` and
  writing `from == to` ledger rows. `apply_move` (`:1943-1967`) is the path that
  forbids that elsewhere; the UI only *excludes* the deck from the destination
  list (`collection.rs:559`, unit-tested at `:2035`), which is not a server
  check.
- **size**: S
- **disposition**: KEEP. Highest-value item in the bundle — it is the one that
  silently rewrites board assignments.
- **blocked-by / duplicate-of**: —

### (g) teardown dialog claims "every move is in the history"

- **verdict**: STALE
- **evidence**: `app/src/my/collection.rs:1724` still carries the sentence, but
  it is now **true**: `teardown` appends one `moves` row per board
  (`hosted.rs:1245-1253`) and returns `move_ids`, and `note_last_move`
  (`collection.rs:1701`) makes ⌘K → *Undo last move* reverse the whole teardown.
  The residue — "no surface undoes a teardown" *from the toast / on mobile* — is
  exactly `P6-031`.
- **size**: S (nothing left of its own)
- **disposition**: MERGE→`P6-031`.
- **blocked-by / duplicate-of**: duplicate-of `P6-031`.

### (h) `previous_location` ignores `undone_at`

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:2202-2210` — the query is
  `SELECT from_collection_id FROM moves WHERE to_collection_id = $1 AND …
  AND from_collection_id IS NOT NULL ORDER BY created_at DESC LIMIT 1`. No
  `undone_at IS NULL`. An already-reversed move is still the most recent row and
  still decides where `Teardown::ReturnToPrevious` sends the copies
  (`hosted.rs:1240`).
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (i) `move_batch` locks rows in item order

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:1015-1033` — one `scoped_tx` for the
  whole batch, iterating `req.items` in the caller's order and calling
  `apply_move` per item; `apply_move` → `holding_take`
  (`:2057-2062`, `SELECT … FOR UPDATE`) and `holding_take_clamp` (`:2103-2107`,
  same). Lock acquisition order is therefore the client's list order, so two
  concurrent overlapping batches submitted in opposite order deadlock; Postgres
  kills one and it surfaces as a 502.
- **size**: S (sort the batch by a stable key before the loop)
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (j) migration 0009 has no backfill for non-`main` intake rows

- **verdict**: CONFIRMED
- **evidence**: `migrations/0009_move_boards.sql` adds `from_board`/`to_board`
  `NOT NULL DEFAULT 'main'` and argues "no backfill is needed" — while its own
  header names the exact counter-case: "`+ Have` at `board = 'side'` appends an
  intake move" that "could not be undone correctly". Every such pre-0009 row now
  reads `to_board = 'main'`. `undo_one` (`app/src/backend/hosted.rs:2003,2009`)
  decodes `to_board` and calls `holding_take_clamp(to, …, to_board, …)`, so
  undoing a historical non-`main` intake takes from the mainboard. Only
  pre-migration rows are affected — `add_holding` now records the real board
  (`:640-646`).
- **size**: S (a `0010` backfill joining `moves` to the holding's board, or an
  explicit decision to accept the drift and say so in 0009)
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (k) `/// POST /api/moves` doc line attached to the wrong fn

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/routes.rs:351-355` — the doc block reads
  `/// POST /api/moves — move copies between collections.` then
  `/// POST /api/holdings/{id}/move — …`, and sits directly above
  `async fn move_holding`. `async fn move_cards` at `:371` has **no** doc
  comment.
- **size**: S (trivial)
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: pairs with (l).

### (l) bench toast reads "1 copies"

- **verdict**: CONFIRMED
- **evidence**: `app/src/bench/count_stepper.rs:67` —
  `format!("Removed Counterspell ({} copies)", c.from)`, unpluralized, while the
  product path pluralizes at `app/src/my/collection.rs:1527-1531`.
- **size**: S (trivial)
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: pairs with (k).

## P6-056 — teardown destination-grouped preview unimplemented

- **verdict**: CONFIRMED
- **evidence**: `specs/collection-api.md:176` still specifies "Returns a
  destination-grouped **preview** before"; `TeardownDialog`
  (`app/src/my/collection.rs:1660-1790`) renders a destination `<select>` and a
  `teardown-confirm` button that calls `teardown_collection` directly at
  `:1696` — no dry-run, no grouping. The backend `teardown`
  (`hosted.rs:1193`) has no preview mode either. The entry's "Related: nothing
  undoes a teardown" tail is now stale (see `P6-055g` / `P6-031`).
- **size**: M (needs a server-side dry run — the `ReturnToPrevious` grouping is
  `previous_location` per grain, not something the client can compute)
- **disposition**: KEEP, RESCOPE by striking the stale "nothing undoes a
  teardown" tail.
- **blocked-by / duplicate-of**: should land after `P6-055h` (the preview would
  otherwise show destinations computed from undone moves).

## P6-057 — multi-grain cell has no removal or edit affordance

- **verdict**: CONFIRMED (with one narrowing)
- **evidence**: `app/src/my/collection.rs:1439-1459` — `HereCount`'s
  `holding_id: None` branch returns plain text with
  `title="several finishes or conditions here — edit them individually"`; no
  stepper, no remove. Narrowing: such a row **is** still selectable
  (`:1345`, `SelectionKey::Held { collection_id, printing_id, board }`), so the
  selection tray can *move* it — but there is no removal and no per-grain edit.
- **size**: M
- **disposition**: KEEP, RESCOPE to "per-grain expansion (or a per-row move/
  remove affordance) for `holding_id = None` cells" and note in the entry that
  the selection tray already covers the *move* case, so this is now specifically
  remove + edit.
- **blocked-by / duplicate-of**: —

## P6-058 — three undo adapters / duplicated e2e helpers

- **verdict**: PARTLY (both halves have shifted)
- **evidence**: Half one is largely answered. `undo_move`
  (`app/src/lib.rs:650`) and `undo_quick_add` (`:980`) do both bottom out in
  `CollectionStore::undo_move`, but `undo_selection_move` (`:1182`) calls
  `undo_moves` — a *different*, batched trait method — so "all three bottom out
  in `undo_move`" is wrong. And the "document why each exists" alternative is
  already satisfied at `app/src/lib.rs:643-648` ("the two are separate adapters
  because each surface's endpoint names what it undoes"). Half two is
  CONFIRMED and worse than filed: `end2end/tests/helpers.ts` exports only
  `hydrated` and `AUTH_STATE`, while `createCollection` is redefined in **nine**
  spec files (`batch-move`, `collection-tree-move`, `collection-tree-manage`,
  `command-palette`, `collection-header-kebab`, `quick-add`, `needs`,
  `collection-view`, `removal`), `addHave` in five, `unownedCards` in four.
- **size**: S
- **disposition**: RESCOPE to the e2e half only — "lift `createCollection` /
  `addHave` / `unownedCards` into `end2end/tests/helpers.ts` (9/5/4 copies
  today)" — and DROP the undo-adapter half as answered by the `lib.rs:643-648`
  rationale.
- **blocked-by / duplicate-of**: —

## P6-064 — `/api/catalog/search` returns `owned: null` for authed callers

- **verdict**: STALE
- **evidence**: `app/src/backend/hosted.rs:313-321` — `search` now collects the
  page's `oracle_id`s, calls `self.owned_by_oracle(&ids)` and maps each row
  through `owned_of(&owned, r.oracle_id)` into `into_summary(n)`.
  `owned_by_oracle` (`:91-107`) returns `None` only when there is no session and
  otherwise reads `owned_by_card` inside `scoped_tx`. The `into_summary(None)`
  the entry cites no longer exists on this path. Confirmed as re-filed
  duplicate: `P6-038a` describes the *consequence* of the fix (the new
  session-only transaction turns an ownership-read failure into a 500 for
  signed-in users), which is only possible because the read now exists.
- **size**: —
- **disposition**: DROP.
- **blocked-by / duplicate-of**: duplicate-of the card-detail owned-badge item;
  its live residue is `P6-038a`.
