# Batch C — needs / shopping / pull

Triage verification pass. Read-only; all entries checked against code as of
2026-07-30 on `docs/phase-6-triage`. Line numbers below are current greps, not
the (drifted) numbers in TODO-Phase-6.md.

## P6-049 (bundle a–j)

Umbrella: "Needs/shopping minors from its review round, none reaching the
major bar." All ten sub-items still reproduce against current code.

### (a) shared `pending` signal disables every Owned-elsewhere row

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:517` — `OwnedElsewhere` creates one
  `pending = RwSignal::new(false)` and passes the *same* signal into every
  `ElsewhereRow` at line 558 (`.map(|row| view! { <ElsewhereRow row collection_id pending /> })`).
  `ElsewhereRow`'s `pull` closure (line 591-599) gates on that shared signal.
- **size:** S
- **disposition:** KEEP

### (b) "pulled" struck through on any non-empty `pulled`, residual drops out

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:793` — `PickRowView`'s tick handler:
  `let moved = !outcome.pulled.is_empty();` then marks the line `done` on
  `moved`, regardless of whether `outcome.pulled`'s copy count matches the
  line's `row.copies`.
- **size:** S
- **disposition:** KEEP

### (c) row-level Pull elsewhere leaves stale lines on an already-open pick list

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:335-338` — `picks`/`done` are created
  **outside** the `Transition` specifically so a tick survives a resource
  refetch (comment: "a snapshot… see module doc"); `open_picks` (line 521-523)
  captures `all.get_value()` once. A closed need is invisible to the client
  and `pull_needs` (`app/src/lib.rs:1376-1382`) refuses it as
  `SkipReason::NoLongerNeeded` — the only outcome a stale checklist line can
  produce. This tradeoff is deliberate (documented) but the described
  consequence is real and unaddressed.
- **size:** S
- **disposition:** KEEP

### (d) a Pull that resolves to nothing raises no toast

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:855-926` (`report`) — only fires a
  success toast when `outcome.move_ids` is non-empty and only fires error
  toasts by iterating `outcome.skipped`. When `items` (or `allocate(...)`'s
  output) is empty, `pull_needs` (`app/src/lib.rs:1367-1426`) leaves both
  `pulled` and `skipped` empty and `move_ids` empty (line 1429-1430) — `report`
  then does nothing at all.
- **size:** S
- **disposition:** KEEP

### (e) Short table has no owned-elsewhere reconciliation column

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:950-955` — `ShortBucket`'s header is
  exactly `Card | Want | Here | Short`, no fourth column explaining a partly
  fillable row's arithmetic.
- **size:** S
- **disposition:** KEEP

### (f) `needs` / `holdings_of_oracle` / `move_batch` run in three transactions

- **verdict:** CONFIRMED
- **evidence:** `app/src/backend/hosted.rs:1372-1420` (`needs`, own
  `scoped_tx()`/`commit`), `:1137-1154` (`holdings_of_oracle`, own
  `scoped_tx()`/`commit`, called once per distinct oracle inside
  `pull_needs`'s loop), and `move_batch` (own transaction, called once more at
  `app/src/lib.rs:1432-1437`) — three (or more, with multiple oracles)
  independent commits, a check-then-act window across all of them.
- **size:** M
- **disposition:** KEEP

### (g) `every_refusal_says_what_is_wrong` omits `SkipReason::NoLongerNeeded`

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/move_selection.rs:994-1008` — the reason list is
  `AlreadyThere, Grain(2), NoCopies, ManyCollections(2), ManyPrintings(2),
  ManyBoards(2)`; `NoLongerNeeded` (defined `move_selection.rs:144`, phrase at
  `:165`) is absent.
- **size:** S
- **disposition:** KEEP

### (h) `default_grain` duplicated across needs.rs and move_selection.rs

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:183-187` and
  `app/src/my/move_selection.rs:189-193` — byte-identical logic (`finish ==
  default() && condition == default() && language == default_language()`),
  two private `fn default_grain`.
- **size:** S
- **disposition:** KEEP

### (i) `/needs` empty state unreachable from navigation

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/collection.rs:742-754` — the only in-app link to
  `/my/collections/:id/needs` is `{chip.map(|text| view!{ <a data-testid="needs-chip">… } })}`;
  `chip = needs_chip(&totals)` (`:608`, `:641`) is `None` exactly when nothing
  is missing (test `needs_chip_matches_the_storyboard_and_vanishes_when_complete`,
  `:1969`) — the one state that renders `needs.rs`'s "All set" arm.
- **size:** S
- **disposition:** KEEP

### (j) failed clipboard copy shows no on-screen fallback hint

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/shopping.rs:211-232` — `copy` sets `copied` to
  `copy_export()`'s bool; the UI only ever renders `<Show when=move ||
  copied.get()>"Copied"</Show>` (line 220-224). `copy_export` (`:253-274`)
  returns `false` on missing element or a failed `exec_command`, and nothing
  renders in that case — the doc comment's promised "copy it yourself"
  fallback has no matching markup.
- **size:** S
- **disposition:** KEEP

## P6-050

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:167` (`pub fn dedupe`) is unit-tested
  directly by `a_duplicated_pull_line_does_not_multiply_the_move`
  (`needs.rs:1162`); the call site `dedupe(items)` inside `#[server] pull_needs`
  is `app/src/lib.rs:1367`. `end2end/tests/needs.spec.ts` has five tests
  (`@fast`, lines 192/245/296/375/423) covering split-buckets, Pull, Pull-all
  grouping, shopping export, and sideboard-vs-mainboard — none sends a
  duplicate `(oracle_id, from_collection_id)` line, so nothing at the API/e2e
  layer would catch deleting the `dedupe(...)` call.
- **size:** S
- **disposition:** KEEP

## P6-051

- **verdict:** CONFIRMED
- **evidence:** `specs/app-ui.md:123-124` still reads "Pull/pull-all are
  client-composed from `move_cards` + `suggested_destinations`" — the exact
  stale line. The correction already exists as a **Findings** entry
  (`specs/app-ui.md:1234-1245`, dated 2026-07-25: "shipped composition is
  `needs` + `holdings_of_oracle` + `move_batch`"), and `app/src/lib.rs:1306-1310`
  carries the same correction as a code comment — but the task-description
  paragraph itself (line 123-124) was never edited to match. Trivial doc fix.
- **size:** S
- **disposition:** KEEP

## P6-053

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs` has no "print"/"share" affordance
  anywhere in the pick-list UI (grepped, only false-positive matches like
  `PartialEq`/`printing_id`), and `PickRowView`'s `toggle` (line 780-786) takes
  `want: bool` — an all-or-nothing tick, no quantity input.
- **size:** M
- **disposition:** KEEP

## P6-074

- **verdict:** CONFIRMED
- **evidence:** `app/src/backend/hosted.rs:1372-1396` (`needs`) — both the
  `desires` CTE (`GROUP BY oracle_id`) and the `holdings` CTEs (`GROUP BY
  oracle_id`, `oracle_id`) are board-blind; `shared::collection::NeedRow`
  (`shared/src/collection.rs:615-623`) has no `board` field. Meanwhile
  board-aware grouping (`GROUP BY oracle_id, board`) exists elsewhere in the
  same file (`:814-819`) for card-row/slot rendering, and `hosted.rs:919`
  already carries a comment naming this exact scenario ("renders a Sideboard
  row reading WANTED 1 while the chip counts…") — the discrepancy is known in
  the code, not just the queue.
- **size:** M
- **disposition:** KEEP

## P6-094

- **verdict:** CONFIRMED
- **evidence:** `app/src/seed.rs:128-131` — `build_depth`'s own doc comment:
  "(The still-open 'wants one card from **two** collections' gap — which
  makes WANTED-is-a-sum indistinguishable from WANTED-is-a-max on `/my` — is a
  different shape and stays its own queued task; this block does not smuggle
  it in.)" The seed's existing blocks (`SENTINEL`/`Inbox`, `BULK`, `DEPTH`)
  each desire cards from exactly one collection apiece; no block wants one
  card from two collections at once. Self-documented as this exact gap.
- **size:** S
- **disposition:** KEEP

## P6-104

- **verdict:** CONFIRMED
- **evidence:** `app/src/backend/hosted.rs:1372-1401` (`needs`) and
  `:1452-1482` (`shopping_list`) both `fetch_all` with no `LIMIT`/cursor —
  contrast with the keyset pattern already in the same file at `:266`
  (card search) and `:779` / `:2787` (paged card rows). `P6-108` (the full
  Scryfall bulk load, `specs/TODO-Phase-6.md:138`) is still `[ ]` — today's
  catalog is the ~2,976-printing POC subset, so neither read currently
  operates at the scale that would make paging matter.
- **size:** M
- **disposition:** PARK — trigger: `P6-108` lands (full ~116K-printing
  catalog) **and** profiling of `needs()`/`shopping_list()` at realistic
  collection/desire sizes shows them hot. Until then this is speculative
  optimization work with no data to justify it.
- **blocked-by:** `P6-108`
