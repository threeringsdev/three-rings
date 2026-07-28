# Phase 6 triage — severity classes

Triage pass over [TODO-Phase-6.md](TODO-Phase-6.md), 2026-07-28. **This document
classifies; it does not verify.** Every entry is taken at its own word — several
were written weeks apart by different review rounds against code that has since
changed, and at least four are visibly stale or duplicated. Correctness review is
the next stage; the "Verify first" list at the bottom is its input.

**Counts.** 107 entries, ~200 distinct defects once the "minors" bundles are
unpacked. 4 blockers · 15 data-integrity · 25 correctness · 14 missing capability
· 50 UX/a11y · 11 performance · 25 dev-loop · 32 hygiene/docs · 6 to close out.

## How to reference a task

Every checkbox entry in `TODO-Phase-6.md` carries a **permanent id** written into
the file itself — `` `P6-001` `` through `` `P6-107` ``, allocated in the order
the entries stood on 2026-07-28.

- **Ids are permanent.** Never renumbered, never reused. A new task takes the
  next free number wherever it is filed; a deleted task's number stays retired.
  Order in the file is free to change — the id is the handle, not the position.
- **Sub-items keep their entry's letters.** A bundled "minors from its review
  round" entry lists its parts as `(a)`, `(b)`, `(c)` in prose, so `P6-055c` is
  the third part of `P6-055`. Those letters are stable because the prose is. When
  a bundle is split into standalone tasks (see below), each part takes a **new**
  id and the split records which letter it came from.
- `grep -rn 'P6-055' specs/` finds every mention across the queue and the specs.

Size is a rough shirt: **S** ≤ half a day, **M** a day or two, **L** a week or a
design decision first.

## Where this ends up

This document is a **classification pass, not the work queue.** The queue we can
actually work through comes out of the verification stage, in three steps:

1. **Triage** (this document) — every entry placed in a severity class, with the
   bundles unpacked far enough to see that their parts don't share a class.
2. **Verify** — each entry checked against the code as it stands today. Entries
   survive, get corrected, or get closed. This is where a bundle gets **split**:
   its surviving parts become standalone entries with their own ids, because a
   bundle whose parts land in five different classes can never be one task.
3. **Order** — `TODO-Phase-6.md` is rewritten as a flat, verified, ordered list
   of standalone tasks grouped by the classes below, and this document is
   reduced to the rationale for that ordering.

Splitting the bundles happens *during* verification rather than now, on purpose:
roughly a quarter of the entries predate the loop recalibration of 2026-07-25 and
several are visibly stale, so exploding all ~150 sub-items up front would do
careful work on items we are about to delete.

The one structural observation: **the bundled "minors from its review round"
entries are not minor as a class.** Each was scoped by a reviewer under an
explicit instruction to hold everything below the major bar, so the bundles are
sorted by *review-round confidence*, not by user impact — and eight of the
fourteen data-integrity findings below are sub-items of a bundle labeled "none
reaching the major bar". Working these bundles as units will do the cheap parts
and skip the expensive ones.

---

## 1. Blockers — the app is not functional until these are fixed

| Id | Entry | Why blocker | Size |
|---|---|---|---|
| P6-108 (was P6-105) | **Run** the full Scryfall bulk load — the loader ships and is gate-tested; only the run remains. Catalog is 2,976 printings of ~116K, and cards outside five set codes are absent entirely, not clipped | A collection manager that cannot find most cards is not a functional collection manager. Also silently moots P6-035 (set picker offering empty sets) and makes the parked full-catalog disambiguation work untestable. Split 2026-07-28: the daily incremental left as `P6-109`, parked in TODO.md. | M (supervised run, not a build) |
| P6-059 | `scripts/migrate.sh` reported `migrations: up to date` on a run that had not embedded the new migration | The one manual step gating a **prod** deploy can report success having done nothing. Wrong failure mode for a schema gate. | S |

---

## 2. Data integrity & destructive actions

The user's collection is the asset. These either lose copies, mangle rows, or
ask for confirmation with a false statement. **Eight are sub-items of bundles
filed as "minor"** — I disagree with that placement and have said so per row.

| Id | Entry | Note | Size |
|---|---|---|---|
| P6-017a | Delete confirm says "0 nested collections" when `find_node` misses, while the server still cascades the children away | Destructive action with a false confirmation. Filed minor; conditioned on a stale/failed tree read, which is exactly when a user is most likely to retry. **Elevate.** | S |
| P6-017d | During an in-flight `view_res` refetch, a kebab click aims `menu_target` at the *previous* payload's collection; the dialog names a collection the URL no longer points at | Delete/rename pointed at the wrong collection. Filed minor. **Elevate.** | S |
| P6-055c | `remove_holding` sends `quantity: None` (the whole stack) while the toast reports the *rendered* count | A stack grown in another tab loses more copies than the toast names, and the toast is the user's only record. | S |
| P6-055f | `teardown` does not reject `to_collection_id == collection_id` | Collapses every board onto `main` and writes ledger rows with `from == to` that `apply_move` forbids elsewhere. UI excludes it; the endpoint accepts it. | S |
| P6-055h | `previous_location` ignores `undone_at` | An already-undone move still decides a "return to previous" destination. | S |
| P6-055j | Migration 0009 has no backfill for pre-existing `add_holding` intake rows at a non-`main` board | Undoing a historical row takes from the mainboard. API-only path today. | S |
| P6-055i | `FOR UPDATE` in item order inside `move_batch` — two concurrent overlapping batches in opposite order deadlock into a 502 | Ordering the locks fixes it. | S |
| P6-049b | A pull line is marked "pulled" on `!outcome.pulled.is_empty()`, so a partly-filled line is struck through and **the residual silently drops out of the walk** | The user believes they pulled copies they didn't. | S |
| P6-049f | `needs`, `holdings_of_oracle` and `move_batch` run in three separate transactions | Check-then-act across transactions — the same window `move_holding` deliberately closed for the row path. | M |
| P6-024a | `plan_move` returning `None` closes the move dialog **silently**, and `None` also covers "destination gone" / "destination forbidden" | The user believes the move happened; the tree not changing is the only correction. | S |
| P6-054 | `set_holding_quantity(id, 0)` still `DELETE`s the row with no ledger entry | Loaded gun, no finger on it (the stepper routes 0 through `move_holding`). Spec-owner call: route decrements through the ledger, or make 0 a move. | M, decision first |
| P6-046g | Selection-tray entries go stale — a key can reference a holding invalidated by a later commit/teardown/deletion, and nothing prunes | Harmless while read-only; batch move already writes, so it needs a stale-entry policy now. | S |
| P6-061a | `POST /api/moves/undo-batch` / `undo_selection_move` accept an **uncapped** `Vec<Id>` while `move_selection` caps at 100 | Asymmetric cap on the write's inverse. | S |
| P6-061b | `selection_destinations` silently `.take(100)`s instead of erroring | A >100 selection gets a ranking computed from an arbitrary subset. | S |
| P6-031 | Nothing undoes a **teardown** from the UI on mobile — `⌘K → Undo last move` can, but the palette is desktop-only and the toast has no Undo | The ledger side is done (`TeardownReceipt` carries `move_ids`), so this is wiring a toast action. A one-way destructive op on the phone. | S |

---

## 3. Correctness — wrong or misleading, not destructive

| Id | Entry | Size |
|---|---|---|
| P6-068 | `/my/collections/:id`'s setup-body reads of `view_res` re-suspend `RequireAuth`'s `<Suspense>`, unmounting the whole `/my/*` subtree on every `?q=` navigation. **Reclassified out of §1 2026-07-28** (verification): filed as a blocker because it was undiagnosed, and it now is — mechanism named, fix chosen (Option B, remove the mis-wiring). Flicker, lost focus and popover desync, not a functional stop | M |
| P6-010 | Hosted `collection_backend` lacks the session fallback `fetch_current_user` already has, so an idle `/my/*` tab reports "session expired" to a still-signed-in user. **Reclassified out of §1 2026-07-28** (verification): self-heals on one reload, needs a 15-min idle window, and most users never hit it. Low priority | S |
| P6-074 | Board-blind `needs()` contradicts board-aware rows on the same page: a Sideboard row reads HERE — / WANTED 1 while the needs chip is absent and `/needs` returns `[]`. Honest fix is `NeedRow.board` + a board-aware `needs()` | M |
| P6-002 + P6-014 | Same-type resource-payload collisions (two consumers of one payload type can swap payloads). **Same item filed twice.** Latent at today's id layout; root cause is P6-013 | M (or 0, if P6-013 lands upstream) |
| P6-013 | `initial_value()` ignores `during_hydration()` — the root cause of the `/my` empty-state bug, diagnosed to the line in `leptos_server-0.8.6`. Both app-side general fixes investigated and rejected as unsafe | **S to report upstream.** Highest leverage item in the file: retires a whole defect class |
| P6-083 | Server-fn error channel collapses every `ApiError` onto HTTP 500 — a 422 validation reaches the UI as a 500 with the right message. JSON API channel maps correctly; the Leptos channel doesn't | M |
| P6-038a | `search` now opens an RLS transaction it never needed, so a failure in the ownership read is a **500 for signed-in users only** — no fallback degrading `owned` to `None` while still serving results | S |
| P6-041 + P6-096 | Native backend splices `cursor` into the upstream query string unencoded, in `collection_view` and `all_cards` too. Bounded (own session, no auth material, RLS still scopes). Fix all three; fold in the three-percent-encoder consolidation | S |
| P6-086 | A facet click inside the query bar's 250 ms debounce window is overwritten by the debounce firing with its captured text — the rail edit is lost | M |
| P6-042b | The previous result set's "Next page →" stays clickable while a newer search is in flight; clicking it navigates to `(old_q, old_cursor)` and `QueryBar`'s re-seed **silently reverts the text the user just typed** | S |
| P6-036e | Debounced-read / immediate-Enter race in the set picker adds `s:leb` when the user typed toward "limited edition alpha" — a term they did not type | S |
| P6-017b | `/my`'s "Where" column shows **stale collection names** after a sidebar rename/delete (`all_cards` doesn't take `TreeManage::revision`) | S |
| P6-017c | `manage.revision` as a `view_res` source rebuilds the card table on any tree mutation, re-seeding every stepper — resurrects the "Undo silently did nothing" defect | S |
| P6-030c | `Undo last move` from the palette bypasses the pull's `on_undo`, leaving a pick-list line ticked and struck-through after the copies went back; `toggle` refuses to re-tick it | S |
| P6-030e | `tr_recent_places` in `localStorage` is per-**origin**, not per-user, and survives sign-out — foreign collection ids persist across a user switch, discarded only by `at_rest`'s index reconcile. No names leak; the reconcile is the only thing between it and a cross-account row | S |
| P6-012c | `/my/collections/<malformed>/needs` withholds children on the grounds that `NeedsHeader`'s back link is the way out — **that link points at the error page itself** | S |
| P6-046a | The tray counts **entries, not copies** — a row holding 4 reads "1 card". Decide alongside batch-move quantity semantics | S, decision first |
| P6-046b | Multi-grain rows are selectable but `Held { collection, printing, board }` can't name the grain — the count stepper already refuses those cells for this reason | S |
| P6-046c | The same physical card selected from `/my` (oracle grain) and a collection row (held grain) yields two indistinguishable tray entries reading "2 cards" | S |
| P6-049a | One `pending` signal shared by every Owned-elsewhere row — one in-flight Pull disables every row's button | S |
| P6-049d | A Pull whose items resolve to nothing raises **no toast at all** — a silent no-op rather than a refusal | S |
| P6-072c | The keywords badge row is card-level and doesn't participate in the DFC face swap, so a back face renders front-only keywords beside back-face oracle text | S |
| P6-072f | `jsonb_array_elements(p.faces)` hard-errors the whole detail/search/summary query on a non-array where the old projection was shape-tolerant. Unreachable via today's ingester **only because the POC subset happens not to produce one** — a 116K-row load is what surfaces it. **Prerequisite of `P6-108`**, not a sibling | S |
| P6-039 | Three definitions of "owned" in `hosted.rs` agree today and nothing enforces that they continue to | S |
| P6-003 | `selection_destinations` stays a raw array payload; safe **only** because the tray can never SSR. Reopens if that changes. Keep as a documented constraint | 0 today |

---

## 4. Missing capability — specified or drawn, not built

| Id | Entry | Size |
|---|---|---|
| P6-005 + P6-103 | **`+ Want` / `+ Have` are missing from the hover preview, the mobile card sheet, and the card detail page.** The wireframes draw them in the first two and the IA doc names them; on a collection row the preview is the only place they could live. Adapters exist (`QuickAddButton`, `raise_add_toast`) — wiring plus optimistic state. **The add flow is the product; treat this as the top of this class.** | M |
| P6-057 | A multi-grain cell (`holding_id = None`) has no removal or edit affordance — the last remaining "can't remove" case | M |
| P6-016 | The per-card-row kebab is unbuilt (`Card Row > Row Kebab` + its reserved header spacer) — the spec's per-row move affordance | M |
| P6-100 | DFC flip is unavailable on the catalog page and everywhere the card renders outside the hover view | S–M |
| P6-044 | Reverse paging (Previous) unbuilt on **both** `/my` and `/catalog` — needs a reverse-ordered query + `before` cursor. Build once, share | M |
| P6-056 | Teardown's destination-grouped **preview** is unimplemented — the dialog confirms directly | M |
| P6-062 | Batch move is one copy per entry, fixed server-side; needs a quantity on `SelectedCard` + a tray control | M |
| P6-063 | Ambiguous `/my` selections are **refused** (`ManyCollections` / `ManyPrintings`), not disambiguated | M |
| P6-047 | No "select all on this page"; no per-collection checkbox in the `/my` location breakdown — which is the natural place for the user to resolve the oracle-grain ambiguity P6-063 refuses | M |
| P6-085 | `+ Want` cannot be undone — needs `set_desire_quantity` on `CollectionStore`, both backends, a route, and a desire id carried back in `QuickAddReceipt` | M |
| P6-053 | Needs pick list has no print/share and no **partial** quantity tick | M |
| P6-023 | Reordering a collection among siblings it is already among is drag-only, therefore mouse-only — no keyboard or touch path at all | L, design first |
| P6-048 | Selection tray has no keyboard entry point; `app-ui.md` specifies `x` and only the tab-reachable checkbox shipped | S |
| P6-019 | **No icon set is vendored** — emoji stand-ins for lucide glyphs, and 🗂 (All cards) vs 📁 (a collection) render near-identically at 15 px, so the mobile `/my` root's aggregate-vs-collection distinction rests on `font-semibold` and a divider | M, pays off everywhere |

---

## 5. UX, a11y, and visual

**High** — user-visible on a normal path:

| Id | Entry | Size |
|---|---|---|
| P6-099 | Black background too dark; card borders blend into it | S |
| P6-101 | No back button from card detail to the catalog | S |
| P6-045 | Selection checkbox is a **16×16 px** tap target on mobile (padding sits on the `<td>`); the next cell is the card-detail link, so a mis-tap navigates away. Fix is a padded `<label>` | S |
| P6-001 | Two pre-existing table overflows: 9–14 px of sideways scroll at **375 px** (iPhone SE/mini, a real device) and 30–34 px at exactly 768 px | M |
| P6-028 | ⌘K palette has **no focus trap** — `Tab` walks out of the dialog into the page behind the scrim. This is a **vendored-`Dialog` gap, so fixing it fixes every dialog in the app**. Plus (b), the ⌘K listener toggles unconditionally and stacks over an already-open dialog | M |
| P6-029 | `command` has no scroll-into-view for the highlighted row — `↑↓` past the fold navigates a row the user cannot see. Affects the palette, the destination picker and quick-add equally | S |
| P6-098 | Catalog card size uncapped at large widths — cards get too big | S |
| P6-022 | `tw_merge` collapses `focus-visible:ring-ring/50` into `focus-visible:ring-[3px]`, so **`Item`'s focus ring loses its colour** for every consumer | S |
| P6-007 | Sub-44 px touch targets, two of which the frames themselves draw small (tile `+ Want`/`+ Have` at 28; filter sheet footer at 36 against its own frame's 44). Needs a ruling: does the 44 px guideline overrule a frame? | S, decision first |
| P6-021c | The rail's pinned `All cards` row links to `/my`, so tapping it **inside the mobile drawer** lands on the root list rather than the table — the drawer's own row appears to do nothing | S |
| P6-069b | `Escape` decodes to `Pass` when `rows == 0`, so Escape cannot close a quick-add panel open with no candidates | S |

**Medium** — a11y semantics, design deviations, and second-order polish:

| Id | Entry | Size |
|---|---|---|
| P6-011 | `CommandEmpty` can only ever speak about *filtering*, and three pickers lean on it to describe a failed fetch. **Can't be fixed in place** — the ⌘K palette's "No matches" depends on exactly that inference. Either the primitive learns the distinction or every consumer keeps supplying its own error arm | M |
| P6-024g | The same conflation renders "Loading collections…" and "No collection to move into." **simultaneously**; the latent pattern exists in the two other `DestinationList` consumers | S |
| P6-034 | `CommandItem`'s `aria-selected` means "keyboard-highlighted", so the set picker's **multi-select membership is invisible to AT**. Primitive-level, affects every `command` consumer | M |
| P6-026 | `ContextMenuItem` has no `aria-disabled`, no typeahead, and ESC is still uncoordinated through the overlay stack — all three pre-existing, all three newly visible now the menu is keyboard-operable | M |
| P6-017e + P6-024h | Neither kebab trigger sets `aria-expanded`, so AT can't tell the panel is open. **Fix both together** — the entries note that fixing one of two consumers is its own inconsistency | S |
| P6-024c | `ContextMenuItem` suppresses focus restore for *every* item though only the move dialog focuses anything, so `New binder inside…`/`Rename…`/`Delete…` leave focus on `<body>` and must be Tab-reached from the document start | S |
| P6-024d | `restore_focus` is reset only inside the `else if is_open` branch, so any close path where the popover never showed leaves it stuck `false` — silently disabling focus restore for the **next** dismissal | S |
| P6-012a | The four `role="alert"` `<p>`s are created *together with* their text, so announcement rests on the AT's insertion heuristic; and `role="alert"` present in the initial SSR HTML is **not announced at all** | S |
| P6-009d | The tray's 44 px hit area is a `<span>` with `on:click` and no `role`/`tabindex` while the accessible control is the inner button — **the tap target and the a11y target are different elements** | S |
| P6-069h | Quick-add field has no `aria-expanded`/`aria-controls`/`aria-activedescendant`, and its `role="listbox"` holds `<a>` links and a footer alongside the `role="option"` rows | S |
| P6-021d | `<nav aria-label="My cards">` duplicates the bottom tab bar's accessible name in the same document — "Collections" or `aria-labelledby` off the heading would be distinct | S |
| P6-021e | `list_skeleton` puts `aria-busy` and `aria-label` on a plain `<div>` with no role, so the label has nothing to attach to | S |
| P6-072e | DFC flip control has a static `aria-label`, no pressed/face state, and no announcement on swap | S |
| P6-072d | Preview flip state survives close/reopen — `PreviewBody` mounts once behind the `hovered`/`sheet_seen` latches — contradicting the "each starting at the front" comment. Reset or re-document, and pin either behavior | S |
| P6-006 | The mobile filter sheet slides from the **left**; the frame draws a bottom sheet with a grabber. `SheetDirection::Bottom` already exists — small, but unrecorded either way, so it needs a ruling rather than a silent choice | S, decision first |
| P6-008 | `/login` deviates from `Desktop — Sign in` on copy and chrome: no logo line, wrong heading, placeholders instead of labels, wrong sign-up copy, and `← Back to home` where the frame names it "Browse the catalog without an account →". The shared `BackHome` also serves OTP and reset, so its label isn't local. Rail is 240 px against the frame's 280 | S |
| P6-020 | The All-cards 390 px fit is **data-dependent** — no cell carries `whitespace-nowrap`, so one space-free collection name (or a long single-word card name) re-exceeds the 356 px wrapper and reintroduces the sideways scroll the assertion exists to catch | S |
| P6-033 | Set chips render the uppercase **code**, never the set name, unless that set happens to be in the current 25-row window. Resolving selected codes needs a by-codes read | M |
| P6-036a | The server-side set search window is a silent **25 with no "N more"** — `q=commander` matches 108 and shows 25, so a set past the window is unreachable through the picker. `SetQuery::limit` exists; `list_sets` never passes one | S |
| P6-035 | The set picker offers sets with **no ingested printings** (1045 sets, ~2976 printings), so most picks return zero results. A "has cards" filter would be a join | S, moot after `P6-108` |
| P6-042a | The result count states *this page's* row count with no qualifier, so the last page of a 73-result search reads "23 results" while mid-pages read "50+". Same number feeds the mobile sheet's footer. Keyset has no offset, so "51–73 of 73" needs a count query or a page ordinal | M |
| P6-042c + P6-042d | A "← Back to the start" control appears on the still-displayed page one before page two arrives, and an empty `<nav aria-label="Pagination">` renders on single-page results — a named landmark with no content | S |
| P6-042e | `last_good` now retains page-N rows, so a grammar error on a fresh page-one query renders **page-N results** as the dimmed "stale" set underneath it | S |
| P6-046d + P6-061j | A visible toast paints over the tray's clear "×" on both breakpoints — sonner is `z-[200]`, the dock `z-50`; ~44 px of overlap at 1440 and the bottom ~20 px of the pill on mobile | S |
| P6-046e | The dock is `fixed inset-x-0` and centers on the **viewport**, so on desktop the tray sits offset from the content column by half the 240 px rail rather than over the table it describes | S |
| P6-046f | The `aria-live="polite"` count enters the DOM together with its first content (the whole tray is inside `Show`), so the **first** pick is typically never announced | S |
| P6-049c | A row-level Pull that closes a need while the pick list is open leaves those lines on the checklist, where they can only ever produce a `NoLongerNeeded` error toast | S |
| P6-049e | The Short table's Want/Here/Short columns don't visibly reconcile for a partly-fillable row (`4 / 0 / 2`), with no owned-elsewhere column explaining the difference | S |
| P6-049j | When `execCommand("copy")` returns false the UI shows nothing — the intended "the text is still selected" fallback has no on-screen hint | S |
| P6-061f + P6-061i | The tray picker's `empty=` string also appears when a *search* filters every row out (the catalog says "No collection matches." there), and `SkipReason::Board` renders as "is on the side board" | S |
| P6-061h | No client-side cap on selection size, so selecting >100 rows produces a **raw server-fn error string in the toast** rather than being prevented in the tray | S |
| P6-069a | `PresentSection` has no empty-query gate, so with `?q=` empty — panel opened at rest, and the moment after every add clears the field — `IN THIS COLLECTION` renders the collection's entire first page | S |
| P6-069c + P6-069f + P6-069g | Quick-add: the two early-return failure paths leave `count` set so a later `⏎` reuses it; the post-add `clear()` pushes a history entry per add; after Escape the field keeps focus so `focusin` can't re-fire and reopening needs a click away and back | S |
| P6-084 | Google sign-in doesn't honor `?next` — carry the post-auth destination through the web OAuth callback (state param) and the Tauri poll path | M |
| P6-087 | Rail sections don't spring open when a filter arrives from the query bar: `<details>` openness is seeded once, so typing `r:rare` shows only the collapsed Rarity summary badge. Decide whether a first-time-populated section should open | S, decision first |
| P6-095 | A vendored `collapsible`'s `class` padding leaks height when closed — `min-h-0` zeroes the content box but not the padding, so `class="pt-1"` leaves a visible sliver of open track under every collapsed row. Apply `class` to the clipped region or document the constraint | S |
| P6-088 | Overlay children are instantiated eagerly, and the V2 review's "revisit only if a specific overlay's content proves expensive" has now been triggered (card-detail previews duplicated every card name and broke three tests, fixed with a per-caller latch). Decide whether `dialog`/`popover`/`sheet` gate children on open by default | M, decision first |
| P6-048 | Also §4 — the selection tray's missing keyboard entry point is an a11y gap as much as a capability gap | S |

---

## 6. Performance

| Id | Entry | Size |
|---|---|---|
| P6-097 | **Catalog first load is slow** — the first impression, and the only one here a user has reported directly | M, measure first |
| P6-029 | `visible_ids()`'s O(n²) runs over the palette's whole row set on **every keystroke**; no cap was added | S |
| P6-061c | `MoveSelection`'s `suggested` resource re-fires on **every checkbox toggle even with the picker closed**, and the server side is one sequential query per oracle | S |
| P6-009e | `results.get().map(\|p\| p.search)` clones a whole page of `CardSummary` on every reactive read, in two separate Effects | S |
| P6-021a | `MyRootNav` renders and `assemble()`s the tree on **desktop** too — three `assemble` calls per document load, and a hidden re-render on every debounced `?q=` keystroke | S |
| P6-021b | A phone's bare `/my` blocks under `SsrMode::Async` on the aggregate read and ships ~50 `<tr>`s that are never displayed. Skipping it needs a client hint or two documents | M |
| P6-090 | `CardPreview` runs a `match_media` + signal **per card** (~60 a page) for a global fact, with no change listener | S |
| P6-036d | Two identical `list_sets` round trips per SSR of any `?q=s:…` — the rail and the sheet each own a `SetPicker`. Sharing needs shell-level provision | S |
| P6-061k + P6-046 | Two independent `list_collections` resources live whenever the catalog picker and the tray are both mounted | S |
| P6-104 | `needs()` and `shopping_list()` are unpaginated — bounded in practice; add keyset if profiling at real scale shows them hot | M, after `P6-108` |
| P6-038e | `EXPLAIN` shows the `holdings` side as a Seq Scan per page — irrelevant at 101 rows, belongs with the large-collection aggregate work in `TODO.md` "Other" | 0 now |

---

## 7. Dev loop, tests, and process

Doesn't touch users; decides whether the next 100 tasks are correct. **P6-013 is in §1/§3
but is really this class too.** (P6-092 was here too; dropped 2026-07-28.)

| Id | Entry | Size |
|---|---|---|
| P6-060 | `hydrated(page)` doesn't imply a streamed island is interactive — four tests flake, a different subset each run. Mitigated by `--workers=1`; the real fix is a hydration-aware click helper | M |
| P6-027 | The `@fast` tier also has a **real data race** — specs share mutable fixture state on one seeded e2e user, so fixing hydration alone will not make the suite parallel-safe | M |
| P6-032 | Nothing enforces `.claude/skills/` ⇄ `.agents/skills/` parity; four of six mirrors had silently gone stale and prescribed the retired ~1.44M-token loop. Cheapest fix is a `diff -rq` gate step | S |
| P6-089 | `#![recursion_limit]` — all six clippy lines are structurally blind to it (codegen-time query). One `cargo build -p three_rings` closes it. (Was paired with P6-092, dropped 2026-07-28 — this stands or falls on its own.) | S |
| P6-082 | Assertion-strength sweep, deferred 2026-07-25: mutation passes are off, so vacuous tests are no longer caught when written | L |
| P6-037 | The recurring **vacuous-test shape** — four instances in one session, all "a test cannot distinguish behaviors its fixture does not distinguish". Generalizable guards written out; promote into the `e2e-suite` skill | S |
| P6-061g | A concrete instance of that pattern: `selection-tray.spec.ts` asserts the empty state with `toContainText`, which reads `textContent`, while `CommandEmpty` hides via `style:display` — **the assertion passes whether or not the element is shown** | S |
| P6-004 + P6-040b + P6-077 | Three e2e authoring traps to fold into the `e2e-suite` skill in one commit: a stale watch-server read is indistinguishable from a passing test (poll on a marker, not elapsed time); `cargo leptos watch` silently drops a save landing mid-rebuild and `touch` won't retrigger; the `tr_jwt` cookie expires in ~20 min and the symptom reads like a page bug (two debug cycles lost) | S |
| P6-040a | The `{..}` struct-update-syntax trap belongs in the `vendor-component` skill — documented only in a `cards.rs` comment, rediscovered twice | S |
| P6-065 + P6-075 + P6-052 | E2E data hygiene, one commit: a `globalTeardown` sweeping `zz-e2e-*` (timed-out tests leak scratch collections past the `finally`, and duplicate names then make by-name locators ambiguous), plus cleaning the dev Inbox's **88 accumulated Lightning Bolt desires** that dominate `/my/shopping`, plus scoping quick-add tests' wants so it stops growing. `cleanup-mutation-leftovers.mjs` exists and isn't wired in | S |
| P6-076 | Nine Android probes are unregistered folklore — one commit registers the lot | S |
| P6-081 | Cross-browser pass deferred wholesale 2026-07-25 — firefox/webkit breakage accumulates unseen. One `npx playwright test` measures the debt. Note the webkit-proxies-WKWebView rationale now rides on the desktop smoke | M, decision |
| P6-094 | The dev seed desires every card from exactly one collection, so "WANTED is the sum" and "WANTED is the max" are indistinguishable. Needs a third sentinelled seed block | S |
| P6-050 | The `pull_needs` dedupe guard is pinned only at the helper — deleting the `dedupe(...)` call in the `#[server]` body leaves the suite green. Applies to any invariant enforced inside a `#[server]` body | S |
| P6-043 | No catalog equivalent of `probe:paging` — nothing walks the search keyset for duplicate/skipped rows. Plus: a corrupt cursor surfaces as `Validation`, so the UI blames the *query* (a variant of P6-083) | S |
| P6-078 | Collection-view test-strength leftovers, all kill-verified: an inert `is_some()` guard with a factually wrong comment; a color-identity badge asserted against the helper that produced it; a 1-in-3 firefox flake from write tests seeding off the same printing | S |
| P6-079 | The stepper zero-floor e2e covers only `−` and typed-`0`-then-⏎; keyboard, paste, negative and non-numeric paths were verified live but are unasserted | S |
| P6-072a + P6-072b | DFC: dropping the `faces.len() < 2` guard survives the suite 26/26, and no test asserts the mana-cost/type-line/stats swap (the fixture card has no P/T on either face; `stats_line` has zero unit coverage) | S |
| P6-009a | `responsive.spec.ts:436`'s `.slice(0, 4)` takes holders in **tree order**, so it can miss every reproducing collection | S |
| P6-009b | The `px-1` savings switch back at `sm` while the 44 px select column persists to `md` — the **640–767 px band is 12 px tighter with nothing paying for it**, and the e2e measures only 390 and 1440 | S |
| P6-009c | `toaster_offset`'s unit test asserts string *shape* only, so it can't catch a wrong magnitude | S |
| P6-025 | Three tree-move claims needing a runtime check rather than an argument: the `set_timeout(0)` re-focus is timing-dependent (a loss shows up as flake, not a red gate); Tailwind emission order for `invisible`/`md:visible`; and `md:opacity-0` ordering, which **`toBeVisible()` cannot catch** since opacity 0 reads as visible | S |
| P6-017g | `bench/header_kebab.rs` renders a hand-copied stand-in menu, so it **looks like** it catches drift between the two real panels and doesn't | S |
| P6-058 | Three near-identical undo adapters bottom out in `CollectionStore::undo_move`; e2e API helpers are duplicated across three spec files and want lifting into `helpers.ts` | S |
| P6-070 | `end2end/probe-add-tmp.mjs` is a stale committed temp probe — delete or promote | S |

---

## 8. Hygiene, docs, and genuine minors

Test-strength minors stay here rather than in §7 — they sit below the bar of the
deliberate sweep P6-082 already schedules.

| Id | Entry | Size |
|---|---|---|
| P6-018 | `/my/all` is missing from **both** authoritative route maps (`app-ui.md` around line 58, and the IA route table), so an agent reading either won't know the route exists. Cheap, and agent-facing | S |
| P6-015 | The IA doc calls the Inbox "undeletable, **renamable**"; `hosted.rs:492` carries `AND NOT is_inbox`. The UI now follows the server on two surfaces, so whichever is wrong is cemented twice. Reconcile one | S, decision first |
| P6-051 | Correct the stale composition line in `app-ui.md` — the needs pick list is `needs` + `holdings_of_oracle` + `move_batch`, **not** `move_cards` + `suggested_destinations`. The wording predates `NeedRow.locations` | S |
| P6-067 | Decide whether custom gap components get a `component-gap-analysis.md` entry at all, or whether the `vendor-component` skill should say "vendored components only" | S, decision first |
| P6-071 | `AddToast` has 8 fields and `QuickAddPanel` 10 props, both at the edge of comfortable — group them if either grows again | S |
| P6-093 | Fractional sibling `position` can collide or exhaust precision on repeated midpoint inserts. Unreachable at POC scale (~50 inserts between one pair to exhaust f64); renumber a sibling group when it bites | deferred |
| P6-106 | Per-card `card_tags` orphan cleanup when the **last** holding and desire leave a deck. Whole-collection teardown already cascades; this is the per-line case | S |
| P6-012b | `describe_error` parses the `validation:` wire prefix independently of `states.rs:100 classify`, so there are **two copies of one prefix table** — and `/catalog`'s own search-error banner remains a sixth hand-rolled banner with no `data-failure` seam | S |
| P6-012d + P6-012e | Two docstrings that contradict the code they document: the sticky picker "SSRs for any session" (it's gated on a signed-in caller), and the error arm being "the *only* thing on the page" (`QuickAddPanel` and the shell rail/tabs remain) | S |
| P6-012f + P6-012g | `err.locator("..")` hops to the parent needlessly, so the assertion survives `destination-retry` moving *out* of the banner; and `btn` is dereferenced with no null check inside `page.evaluate`, so a missing `state-retry` throws out of the Android probe instead of reporting FAIL | S |
| P6-017f | `leaving` is computed before the await, so a navigation *during* a delete still yanks the user to the deleted node's parent | S |
| P6-017h | `page.locator("h1", { hasText: "All cards" }).first()` drops the single-visible-heading strictness the mobile-`/my` task established | S |
| P6-024i + P6-024j | `fn named(…)` is a verbatim alias for `fn row(…)` in the test module (dead indirection), and `⋯` adds one tab stop per tree row — standard for row actions, worth revisiting if the tree gets deep | S |
| P6-030a + P6-030b + P6-030d | Palette: `RowSet::key` omits `Place::meta` so a collection reparented mid-open keeps stale parent-path meta; the same key has no section delimiter between places and commands (a collision needs a collection literally named `new-binder`, but a marker costs nothing); `is_mac()` reads the deprecated `navigator.platform` (the non-deprecated read isn't in web-sys) | S |
| P6-030f | `provide_tree_manage` hoisted to the shell means `create_open`/`rename_open`/`menu_target`/`drag` now survive a Catalog ⇄ My-cards switch, so a dialog left open reappears on return | S |
| P6-030g | `TreeDialogs` sits inside the `hidden md:block` aside, so tree dialogs are invisible below `md` — harmless for a desktop-only palette, but `New binder…` has no mobile story if that changes | S |
| P6-030h | Undo **recording** is verified end-to-end only on the removal path; the add / batch-move / pull record sites are one-liners, so a dropped `record` call on those three wouldn't fail the suite | S |
| P6-036b + P6-036c | The set search term is interpolated into `ILIKE '%' \|\| $1 \|\| '%'`, so **`%` and `_` act as SQL wildcards** (parameterized — wrong-looking results, not injection); and `ORDER BY released_at` resolves to the `::text` output alias, so the sort is lexicographic and correct only because ISO dates collate like dates | S |
| P6-036f | `s:mh3,mh3` renders **two chips with identical `data-testid`/`data-code`** — a strict-mode violation for any future test, and removing one leaves the row's ✓ showing while the badge drops 2→1 | S |
| P6-036g + P6-036h + P6-036i | Set-picker test gaps: no positive control that the cursor was present, a unit test that really only exercises two helpers, and the repo's first `EitherOf4`-inside-`Suspend` covered only by the passing probe | S |
| P6-038b + P6-038c + P6-038d | Owned-badge test couplings: a vacuous negative assertion (see P6-037), `top`/`none` picked from a `limit=15` call while the page renders `limit=50` (sound only by an undeclared ordering coupling), and an SSR pin that any card with the same count satisfies | S |
| P6-038f + P6-038g | `ResultsList`'s `ml-2` moved onto a wrapper `<span>`, equivalent only because `Badge` renders `inline-flex`; and the Android owned-badge probe covers only the anonymous half by design | S |
| P6-042f | The Android paging probe has the exact survivor weakness already fixed in the Playwright suite — it asserts "no Next" but never that the pre-cursor card is absent, so it passes on a cursor-ignoring build | S |
| P6-055a + P6-055b | A dead `holding_id` window after remove→Undo (an edit there shows "Couldn't save: not found: holding"), and an *earlier* stepper Undo toast on the same row survives a removal and errors when pressed | S |
| P6-055d + P6-055e | Deck section slot counts ignore `here_delta`, so the section header contradicts the row and the page header until a refetch; and a removed row keeps its selection checkbox, yielding a `NoCopies` refusal instead of being unselectable | S |
| P6-055g + P6-055k + P6-055l | The teardown dialog's "every move is in the history" claim (now true — see P6-031), a `/// POST /api/moves` doc line attached to the wrong fn, and a bench toast reading "1 copies" | S |
| P6-061d + P6-061e + P6-061l | Batch-move: the residual TOCTOU note, `/cards/:oracle`'s ownership block not invalidated by a move (`HoldingsRevision` has only two consumers), and undo not putting moved entries back in the tray (deliberate — revisit if it reads wrong) | S |
| P6-069d + P6-069e | Quick-add doc drift on what `here` means, and candidates going through `search_catalog`'s grammar while `collection_view`'s `q` is a plain substring — so "answers to the same question" holds only for plain names | S |
| P6-069i + P6-069j + P6-069k | `use_command_nav().expect(...)` is a wasm panic rather than a compile-time guarantee; `Candidate.index` and `nav.highlighted()` index different spaces and would disagree if two result sets are briefly co-registered; and one `collection_view` round trip of stale panel state after switching collections | S |
| P6-009f | `responsive.spec.ts:406`'s comment reasoning is inverted — the filter is right, the stated reason is not | S |
| P6-009g | An orphaned `zz-e2e-inb-src-w1-9` collection on the Neon dev branch matches no current spec's prefix. **Do with P6-065** | S |
| P6-107 | Bundled read-only catalog for offline browsing on desktop/mobile | deliberately deferred |

---

## 9. Close out — no work, or work already done

| Id | Action |
|---|---|
| P6-066 | **Delete.** Self-declared superseded by P6-060; the entry is a record of a wrong diagnosis, which belongs in `ui-work-loop.md` Findings, not the queue |
| P6-014 | **Merge into P6-002.** Same defect, filed twice by two review rounds |
| P6-064 | **Verify, then likely delete.** Self-declares as a re-file of the card-detail item, and P6-038a describes `search` as having *gained* the ownership read — so this may already be fixed. If it isn't, it's §3 |
| P6-073, P6-080 | Already `[x]`. Move both to a Phase 6 done-log section (or to `DECISION-LOG.md` — P6-080 is a maintainer decision, not a task) so the queue holds only open work |
| P6-003 | Not a task — a documented constraint with no action while the tray can't SSR. Move to `app-ui.md` Findings |
| *(the "Other" section)* | Unnumbered on purpose — it is roadmap, not queue: full-catalog disambiguation (gated on `P6-108`), keystroke-budget validation, large-collection profiling, decks/sharing, import/export, and the two app-update-delivery paths. Leave as-is, out of triage |

---

## Verify first — the review stage's input

Ordered by how much the classification above would move if the premise is wrong:

1. **P6-064** — may already be fixed by the owned-badge task; P6-038a implies it.
2. ~~**P6-105** — confirm the dev catalog's actual size and whether the POC subset blocks the flows we care about, or only the long tail. This decides whether it's §1 or §4.~~ **Resolved 2026-07-28:** 2,976 printings of ~116K, and the subset is five set codes — not a clipped long tail, so any card outside them is absent entirely. **§1 confirmed.**
3. ~~**P6-102** — reproduce on the deployed Render app. If it's a config issue, it's S, not M.~~ **Resolved 2026-07-28:** deployed Render is fine; the defect was local-dev only and config-only. Done.
4. ~~**P6-068**~~ — **diagnosed 2026-07-28**, CONFIRMED. Setup-body resource reads re-suspend `RequireAuth`'s `<Suspense>`; not the router. Rescoped to a named Option-B fix (M) and reclassified §1 → §3; see `phase-6-probes/P6-068.md`.
5. ~~**P6-010**~~ — **verified 2026-07-28**, CONFIRMED. Rescoped to a named S fix and reclassified §1 → §3; see `phase-6-probes/P6-010.md`.
6. **P6-002/P6-013/P6-014** — the "measured not to fire at today's id layout" claim is layout-dependent and the layout has changed since. `npm run diag:resource-ids` re-measures it.
7. **The seven `P6-055*` data-integrity sub-items** — all were filed as minors from one review round against `hosted.rs`; confirm each still exists before sizing.
8. **P6-017a/P6-017d** — I elevated both out of a "minor" bundle. Worth a second opinion on whether the degraded-state precondition makes them rare enough to leave.
9. **Everything dated before 2026-07-25** — the loop recalibration changed what "minor" meant mid-stream, so older bundles were scoped under different rules than newer ones.

## Suggested first cut, if we want a sequence out of this

**Now:** P6-013 (report upstream — S, and it retires a class) · P6-089 (one gate step; its former partner P6-092 was dropped 2026-07-28) · P6-032 (one gate step, stops agents following a retired loop) · P6-059 (S, protects prod schema) · P6-102 (done 2026-07-28 — was not a deployed blocker; P6-010 left §1 on verification and is now a low-priority §3).

**Then:** §2 in one sweep — most are S, they cluster in `hosted.rs` and the move/teardown paths, and they're the ones that cost the user something they can't get back.

**Then:** P6-005+P6-103 (the add flow), P6-068 (diagnosed — now an M fix, no longer §1), and the §7 items that make everything after them trustworthy (P6-060, P6-027, P6-037, P6-004-bundle).

P6-105 sat outside this and was **split 2026-07-28**: the bulk load is already written, so `P6-108` is a supervised run (hours, not a week) and stays in Phase 6; stage 3, the daily incremental, is the actual build work and is parked in TODO.md as `P6-109` behind a user-visible-staleness trigger.
