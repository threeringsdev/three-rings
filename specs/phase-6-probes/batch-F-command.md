# Batch F — command primitive / set picker

Fast triage pass, 2026-07-30. Read-only. Line numbers below are current as of
this pass (the queue's cited numbers have drifted).

## P6-011 — `CommandEmpty` can't distinguish "not fetched" from "failed" from "empty"

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/command.rs:315-330` — `CommandEmpty`
  still renders purely off `ctx.visible_ids().is_empty()`, no fetch-state
  input. The set picker already routes around it with its own hand-rolled
  four-arm `EitherOf4` (`app/src/catalog/rail.rs:1032-1101`), whose comment at
  `:1071-1076` names exactly this reasoning ("`CommandEmpty` … has nothing to
  infer from; the row count is known right here"). The palette still depends
  on the inference for its own empty state: `app/src/components/palette.rs:957-959`,
  `<CommandEmpty data-testid="palette-empty">"No matches"</CommandEmpty>`.
- **size**: M
- **disposition**: KEEP — unresolved primitive-level gap, and the set picker
  is now a second consumer (after the states-sweep pickers) supplying its own
  error arm rather than fixing the primitive, exactly as the entry predicts.
- **blocked-by / duplicate-of**: related to `P6-024g` (same conflation
  pattern in `DestinationList`, different file).

## P6-029 — `command` has no scroll-into-view; `visible_ids()` is O(n²) with no cap

- **verdict**: CONFIRMED (both halves)
- **evidence**: no `scroll_into_view`/`scrollIntoView` call anywhere under
  `app/src/` (`grep -rn` empty). `CommandList`'s base class is
  `max-h-[300px]` (`app/src/components/ui/command.rs:41`); consumers cap
  further — palette `max-h-[21rem]` (`app/src/components/palette.rs:956`),
  destination picker `max-h-64` (`app/src/catalog/destination.rs:265`) — so
  `↑↓` past any of those folds moves the highlight with no compensating
  scroll. Second half: `CommandItem`'s `highlighted` memo calls
  `ctx.visible_ids()` (an O(n) filter over the whole registry) once **per
  item**, every time any item's visibility changes
  (`command.rs:380-387`, `visible_ids()` itself at `:102-109`) — still
  exactly the O(n²)-per-keystroke shape the entry describes, and still
  uncapped. `shared/src/catalog.rs:250-255`'s doc comment independently
  confirms the cost model ("that primitive's registry is O(n) per item … a
  full list is O(n²) work on mount and again on every keystroke") as the
  reason the set picker asks the server for a bounded window instead.
- **size**: S
- **disposition**: KEEP, RESCOPE — split into two independent standalone
  fixes (see UNITS below); they touch different code paths (view-layer
  scroll vs. registry lookup) and have been triaged separately (a11y/UX vs.
  performance) already.
- **blocked-by / duplicate-of**: —
- **UNITS**:
  - `command: scroll the highlighted row into view on ↑↓` — S
  - `command: cap/memoize visible_ids() to kill the per-keystroke O(n²) walk` — S

## P6-033 — set chips render the code, never the name

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog/rail.rs:975-999` — the chip builder does
  `let label = code.to_ascii_uppercase();` unconditionally; there is no name
  lookup at all in the chip path (only the row inside the fetched 25-window,
  `SetOption` at `:1148-1166`, ever renders `{name}`). No by-codes lookup
  exists anywhere in the tree: `grep -rn "by_codes\|codes_to_names\|set_names"`
  over `app/src` and `shared/src` is empty.
- **size**: M
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

## P6-034 — `CommandItem`'s `aria-selected` means "keyboard-highlighted", not "multi-select member"

- **verdict**: CONFIRMED
- **evidence**: `app/src/components/ui/command.rs:400` —
  `aria-selected=move || highlighted.get().to_string()`, driven solely by
  keyboard-highlight position. The set picker's multi-select membership
  rides `data-selected` on an inner `<span>`, not `aria-selected` on the row
  (`app/src/catalog/rail.rs:1154-1159`), with the comment at `:1150-1153`
  explicitly stating why ("its own `aria-selected` means
  'keyboard-highlighted', a different thing"). Nothing exposes membership to
  assistive tech.
- **size**: M
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

## P6-035 — set picker offers sets with no ingested printings

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:327-366` — `list_sets`'s SQL
  selects straight off `sets` with no join to `printings`/`holdings`; any of
  the ~1045 dev-catalog sets can be picked regardless of whether it has any
  ingested cards. `P6-108` (the full Scryfall bulk load) is still `[ ]` in
  `specs/TODO.md`/`TODO-Phase-6.md:138` — not yet run — so the maintainer's
  existing "moots P6-035" note (`specs/TODO-Phase-6-triage.md:66`) has not
  yet fired.
- **size**: S
- **disposition**: PARK behind `P6-108`. The fix (a "has cards" join) is easy
  to build but pointless to verify against a 2,976-printing dev catalog where
  nearly every set is already empty; re-triage once the bulk load lands and
  most sets legitimately have cards.
- **blocked-by / duplicate-of**: blocked-by `P6-108` (trigger: `P6-108`
  merges/runs — re-open only if picks against the full catalog still return
  a meaningful share of zero-result sets).

## P6-036 — set-picker minors (bundle a–i)

### (a) 25-row window with no "N more" indicator

- **verdict**: PARTLY — behavior confirmed, stated mechanism is wrong
- **evidence**: The observable claim holds: `list_sets` returns at most
  `SetQuery::limit()` rows (default 25, `shared/src/catalog.rs:270-272`) and
  no total-match count is returned or rendered anywhere in
  `app/src/catalog/rail.rs`'s `SetPicker`, so a set past the window is
  unreachable through the widget. But the entry's claimed mechanism —
  "`SetQuery::limit` exists; `list_sets` never passes one" — is now false:
  `hosted.rs:345,352` **does** bind `.bind(query.limit())` into the SQL's
  `LIMIT $2`. The real gap is one level up: the `list_sets` server fn
  (`app/src/lib.rs:352-355`) always constructs `SetQuery { q: Some(q), limit:
  None }`, so nothing ever overrides the 25 default, and no count/"more"
  signal is plumbed back to the UI.
- **size**: S
- **disposition**: KEEP, RESCOPE — reword to "the `list_sets` server fn
  never exposes a `limit` override, and no total-match count/'N more'
  indicator is returned or rendered" rather than the current (incorrect)
  claim that the SQL never receives a limit.
- **blocked-by / duplicate-of**: —

### (b)+(c) unescaped `ILIKE` wildcards; sort resolves to the `::text` alias

- **verdict**: CONFIRMED (both)
- **evidence**: `app/src/backend/hosted.rs:332-345` —
  `code ILIKE '%' || $1 || '%'` / `name ILIKE '%' || $1 || '%'`, `$1` bound
  straight from `query.term()` with no escaping of `%`/`_` (parameterized,
  so not an injection — just wrong-looking results for those chars). Same
  query's `ORDER BY … released_at DESC NULLS LAST, code` sits after a
  `SELECT … released_at::text AS released_at`; Postgres resolves an
  unqualified `ORDER BY` name to the output alias when one exists, so the
  sort is on the `::text` cast, lexicographic-only-by-luck (ISO dates
  collate like dates).
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (d) two identical `list_sets` round trips per SSR

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog/rail.rs:434-480` — `FilterRail` and
  `FilterSheet` both render `RailBody` (`:484-554`), which mounts its own
  `SetPicker` (`:551`), each with its own `Resource` (`:924-932`). Both are
  always in the DOM at once (the sheet is CSS-hidden via `md:hidden` at
  `:449`, not conditionally rendered), and `expanded` seeds open when the URL
  already carries selected codes (`section_seeded_open(..., count.get_untracked())`,
  `:905-908`), so a `?q=s:…` load auto-expands and fetches in both instances.
  The code's own comment at `:896-900` names this exact tradeoff ("the rail
  renders twice per page (desktop + mobile sheet)").
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (e) debounced-read / immediate-Enter race

- **verdict**: CONFIRMED (structurally; visual repro needs a browser, out of
  scope for this pass)
- **evidence**: `CommandInput`'s `on:input` sets `ctx.query` synchronously
  every keystroke (`app/src/components/ui/command.rs:302`) and separately
  invokes the caller's `on_search_change` callback. `SetPicker`'s
  `on_search` (`app/src/catalog/rail.rs:966-973`) debounces that callback
  through `set_timeout_with_handle(…, SEARCH_DEBOUNCE_MS)` (250 ms) before
  writing the `search` signal that actually re-keys the `list_sets`
  `Resource`. `Command should_filter=false` (`rail.rs:1016`) means
  `CommandItem::is_visible` is always `true` regardless of `ctx.query`
  (`command.rs:348-350`), so the row list itself doesn't reflect the
  keystroke until the debounce fires — the exact window the entry describes
  as producing a wrong `s:` code on an Enter that lands mid-window.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (f) `s:mh3,mh3` renders two identical chips

- **verdict**: CONFIRMED
- **evidence**: `shared/src/search.rs:173-182` — `csv()` splits, unquotes,
  lowercases, and collects with no dedup, unlike `color_letters()`
  immediately below it (`:185-199`) which explicitly does
  `if !out.contains(&up) { out.push(up) }`. `toggle_code`
  (`app/src/catalog/rail.rs:860-869`) also never dedupes on add. Chips render
  straight off that undeduped `codes` vec (`:975-999`), each carrying the
  same `data-code`/no differentiator.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (g) no positive control for the cursor-drop test

- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/filter-rail.spec.ts:507-521` — "a set pick
  drops the page cursor" navigates to a URL carrying `cursor=${first}`
  (`:513`), clicks a chip, waits for the URL without `cursor`, then asserts
  `searchParams.get("cursor")` is null (`:519`). Nothing between the initial
  `page.goto` and the click asserts the loaded page actually had `cursor` in
  its URL first. The `// positive control` comment at `:520` is on a
  different assertion (that the rail's search box is visible post-navigation),
  not on cursor presence. The test would still pass if `page.goto` never
  carried a cursor at all.
- **size**: S
- **disposition**: KEEP — trivial, one extra assertion.
- **blocked-by / duplicate-of**: —

### (h) unit test mixes two fixtures

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog/rail.rs:1505-1525`,
  `a_code_the_picker_never_lists_is_still_the_users_selection` — `st.set` is
  derived from `read("s:xyz,mh3")` (`:1509`, `["xyz","mh3"]`), but both
  `rewrite` calls at `:1516` and `:1520-1524` rewrite the *different* base
  string `"s:xyz"` (single code). The test never round-trips a single
  consistent fixture through read → toggle → rewrite; it only exercises
  `toggle_code` + `set_term` composition against hand-picked inputs.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### (i) `EitherOf4`-in-`Suspend` has no targeted hydration assertion

- **verdict**: CONFIRMED
- **evidence**: `grep -rln "EitherOf4"` under `app/src/` returns only
  `app/src/catalog/rail.rs` (`:1045,1053,1078,1090`) — first use in the repo,
  inside `Suspend::new` (`:1033-1100`). `end2end/tests/filter-rail.spec.ts`
  exercises the four arms individually (`set-empty` at `:490`, `set-error`
  implied by a following test, etc.) but every test's hydration check is the
  same generic `hydrated(page)` helper (imported at `:2`, used ~20 times
  through the file) — no assertion targets the `EitherOf4`/`Suspend`
  combination specifically as a hydration risk.
- **size**: S
- **disposition**: KEEP.
- **blocked-by / duplicate-of**: —

### UNITS — P6-036 regrouped for the rewritten queue

- `set picker: escape ILIKE wildcards + sort by the real released_at column` — S (b+c)
- `set picker: expose a limit override on list_sets + surface a match count/"N more"` — S (rescoped a)
- `set picker: share one list_sets Resource between FilterRail and FilterSheet` — S (d)
- `search grammar: dedupe csv value lists (s: and any future multi-value field)` — S (f)
- `set picker: fix debounced-search / immediate-Enter race` — S (e)
- `set picker tests: cursor positive control + fix mismatched-fixture unit test + targeted EitherOf4/Suspend hydration assertion` — S (g+h+i, same file area, trivially bundled)
