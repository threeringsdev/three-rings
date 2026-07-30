# Batch E — catalog / paging / rail

Triage verification pass over: `P6-038` (a–g), `P6-041`, `P6-042` (a–f), `P6-043`,
`P6-044`, `P6-086`, `P6-087`, `P6-096`. Read-only; no code touched.

## P6-038

Owned-badge minors from its review round, seven sub-items.

### (a) `search` opens an RLS transaction it never needed; no `owned`-degrade fallback

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:91-107` (`owned_by_oracle`) opens
  `scoped_tx()` whenever `self.session.is_some()`; `search` at
  `hosted.rs:263-325` calls it unconditionally at line 314 and propagates any
  error via `?` (line 314's `self.owned_by_oracle(&ids).await?`) with no
  fallback to `None`. A failed ownership read (tx error) now 500s a
  previously-working, signed-in catalog search; anonymous callers skip the
  branch entirely (`owned_by_oracle` short-circuits `Ok(None)`).
- **size**: S
- **disposition**: KEEP — small, well-scoped fix (catch the ownership-read
  error and degrade to `None` rather than failing the whole search).

### (b) negative badge assertion is vacuous if the tile is absent

- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/catalog.spec.ts:601`
  `await expect(ownedBadgeFor(page, none!.oracle_id)).toHaveCount(0)` —
  `ownedBadgeFor` (`catalog.spec.ts:522-529`) filters `results-grid li` by an
  `href` match then chains `.getByTestId("owned-badge")`; if the `li` itself
  doesn't match (0 results), the chained locator is also count 0 and the
  assertion passes without ever having found the tile.
- **size**: S
- **disposition**: KEEP, bundle with (c)+(d) as one test-hardening task
  (assert the tile locator itself has count 1 before asserting the badge is
  absent inside it).

### (c) `top`/`none` picked from `limit=15` while the page renders default `limit=50`, undeclared coupling

- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/catalog.spec.ts:561`
  `const mine = await search(page.request, { q: OWNED_QUERY, limit: 15 })`,
  while the page navigation at line 589 (`/catalog?q=...`) carries no
  `limit=`, defaulting to `Page::limit()`'s 50
  (`shared/src/collection.rs:368-371`, `unwrap_or(50)`). Soundness rests
  entirely on `ORDER BY c.name, c.oracle_id` (`hosted.rs:282`) making the
  first-15 a prefix of the first-50 — true today, undeclared in the test.
- **size**: S
- **disposition**: KEEP, bundled with (b)+(d).

### (d) SSR pin is a whole-page substring check

- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/catalog.spec.ts:591-592`
  `expect(html).toContain('data-testid="owned-badge"'); expect(html).toContain(`${top.owned} owned`)` —
  neither line scopes to `top`'s specific tile; any other card sharing
  `top.owned`'s count anywhere on the SSR'd page satisfies it. The
  tile-specific check (`ownedBadgeFor(...).toHaveText(...)`) only runs after
  `hydrated(page)` at line 594-598, post-hydration.
- **size**: S
- **disposition**: KEEP, bundled with (b)+(c).

### (e) `EXPLAIN` shows `holdings` side as a Seq Scan per page

- **verdict**: UNVERIFIABLE (exact runtime check: `EXPLAIN ANALYZE SELECT
  oracle_id, owned FROM owned_by_card WHERE oracle_id = ANY($1)` against a dev
  branch seeded to catalog-scale (~100K holdings rows) — out of scope for this
  read-only, no-DB pass)
- **evidence**: `app/src/backend/hosted.rs:91-107` is exactly the query named
  (`owned_by_oracle`); irrelevant at today's ~101-row dev seed per the entry's
  own claim.
- **size**: 0 now
- **disposition**: MERGE → already-filed `specs/TODO.md:108` ("Large-collection
  aggregate performance — profile owned / present-rollup / needs … promote
  `owned_by_card` to a materialized view"), which is exactly this query and
  exactly this profiling work. Not a new queue task.

### (f) `ResultsList`'s `ml-2` moved off `Badge` onto a wrapper `<span>`

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:823-829` — `<span class="ml-2"
  data-testid="owned-badge"><Badge variant=BadgeVariant::Secondary
  size=BadgeSize::Sm>...</Badge></span>` in `ResultsList` (table/list view),
  vs. the grid tile's version at `catalog.rs:752-759` which puts positioning
  classes on the wrapper `<span>` too but never puts margin directly on
  `Badge`. `BADGE_BASE` (`app/src/components/ui/badge.rs:56`) includes
  `inline-flex`, which is why wrapper-span margin and on-`Badge` margin render
  identically today (both inline-participating boxes) — the equivalence is
  incidental, not declared.
- **size**: S
- **disposition**: KEEP, bundled with (g) as one styling-consistency task.

### (g) Android owned-badge probe covers only the anonymous half by design

- **verdict**: CONFIRMED
- **evidence**: `end2end/android-owned-badge-check.mjs:5-9` — its own header
  comment states the dev proxy strips Cookie headers so the webview can only
  ever be unauthenticated; lines 40-42 assert `badges === 0` (no owned badge)
  and lines 47-56 assert `owned` is `null` on every hit. The authed half is
  explicitly deferred to `catalog.spec.ts`'s chromium tier (comment, line 9).
- **size**: S
- **disposition**: KEEP as documentation/acknowledgment only (already correct
  by design, per the probe's own comment) — bundled with (f); no code change
  needed for (g) itself.

## P6-041 (bundled with P6-096 per triage doc)

**The native backend splices `cursor` into the upstream query string
unencoded, in three call sites.**

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/native.rs` — `search` (`qs.push(format!("cursor={cursor}"))`
  at line 222, `q` alone goes through `urlencode` at line 219); `collection_view`
  (line 377, same pattern, `q` encoded at line 374); `all_cards` (line 448,
  same pattern, `q` encoded at line 445). All three leave `cursor` raw. A new
  comment at `native.rs:369-370` on `collection_view` ("The cursor is
  base64url (already URL-safe), so no escaping is needed") documents the
  *assumption* the bug violates — a well-formed cursor from `encode_cursor` is
  indeed URL-safe, but a hand-pasted/malformed one (the entry's scenario,
  e.g. `cursor=x%26limit%3D200`) is not filtered before reaching this splice,
  so the comment doesn't invalidate the finding. `search`'s call site remains
  reachable from the public, unauthenticated `/catalog` page.
- **size**: S
- **disposition**: MERGE → one task with P6-096 (below); fixing the cursor
  encoding is naturally done by routing all three call sites through the
  consolidated encoder.
- **duplicate-of / bundled-with**: P6-096

## P6-096 (bundled with P6-041 per triage doc)

**Three near-identical percent-encoders exist, undocumented as a set.**

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:112-123` (`encode_query_value`, `pub(crate)`,
  unreserved set `A-Za-z0-9-_.~`) and `app/src/backend/native.rs:256-267`
  (`urlencode`, private, byte-identical unreserved set and logic — functionally
  a duplicate) and `app/src/shell.rs:76-87` (`encode_path_for_query`, same
  unreserved set plus `/` kept literal, has its own unit test at
  `shell.rs:646-655`). No shared module; the three differ only in visibility
  and whether `/` is kept literal, and nothing states which is for what.
- **size**: S (as part of the merged task with P6-041)
- **disposition**: MERGE → consolidate into one module with the two variants
  (value-encoder / path-encoder) named, and use it to fix P6-041's cursor
  encoding at the same time.
- **duplicate-of / bundled-with**: P6-041

## P6-042

Catalog-paging minors from its review round, six sub-items.

### (a) result count states this page's row count with no page qualifier

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:322-326` —
  `let n = r.cards.len(); let more = if r.next_cursor.is_some() {"+"} else {""}; format!("{n}{more} results")`
  — no total/page context. The same signal feeds the mobile sheet: `result_count`
  (built from the identical `r.cards.len()` at `catalog.rs:305-311`) is passed
  into `<rail::FilterSheet result_count />` at `catalog.rs:315`. Keyset paging
  (no offset) means a "51–73 of 73" form needs a separate count query or a
  page ordinal, as the entry states.
- **size**: M
- **disposition**: KEEP as standalone task (rescope: needs either a count
  query or a page-ordinal param — a real design decision, not a one-line fix).

### (b) stale "Next page →" navigates to `(old_q, old_cursor)`, silently reverting typed text

- **verdict**: CONFIRMED
- **evidence**: `Results`' `<Transition>` (`app/src/catalog.rs:462-555`) keeps
  the previously-rendered `Pager` (with its `href` built from the `q` captured
  at that render, `catalog.rs:469` `let q = url_q.get()`) on screen while a
  newer search resolves. A click on that stale anchor navigates directly
  (bypassing `QueryBar::commit`), moving `url_q` back to `old_q`. `QueryBar`'s
  re-seed effect (`app/src/components/query_bar.rs:114-120`) then fires because
  `from_url != self_pushed.get_value()` (self_pushed having already advanced to
  the newly-typed text via an earlier debounce commit), reverting `text` to
  `old_q` — undoing what the user just typed. Matches the module doc's own
  description of behavior 3's guard rail (`query_bar.rs:14-17`), which this
  race falls outside of (a real anchor nav, not a `commit()` call).
- **size**: S
- **disposition**: KEEP as standalone task, most user-visible of the bundle.

### (c) `paged` flips as soon as the URL moves, growing a stale "Back to the start"

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:218` `let paged = Memo::new(move |_| !url_cursor.read().is_empty());`
  reacts to the URL immediately on navigation. Its consumers (`<Show
  when=move || paged.get()>` in `Pager` at `catalog.rs:614` and in the error
  arm at `catalog.rs:514`) are independent reactive closures inside content
  the outer `<Transition>` (`catalog.rs:462`) is still displaying from the
  prior resolution — so the still-on-screen page one grows the control before
  the new page's `Suspend` body replaces it.
- **size**: S
- **disposition**: KEEP, bundled with (d).

### (d) empty `<nav aria-label="Pagination">` renders on a single-page result set

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:613-639` (`Pager`) always renders
  `<nav aria-label="Pagination">`; when `paged.get()` is false the `<Show>`
  fallback is `|| view! { <span></span> }` (line 614) and `next` is `None` so
  `{next.map(...)}` (line 623) emits nothing — the nav lands in the DOM
  wrapping only an empty `<span>`.
- **size**: S
- **disposition**: KEEP, bundled with (c).

### (e) `last_good` retains page-N rows across an unrelated fresh-query grammar error

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:226-231` — `last_good` is set only on
  success (`if let Some(Ok(r)) = ...`), never cleared on a new query or a
  cursor reset; the error arm (`catalog.rs:483-551`) unconditionally renders
  whatever `last_good.get_untracked()` holds (line 485) as the dimmed "stale"
  set, regardless of whether it came from a different search's later page.
- **size**: S
- **disposition**: KEEP as standalone task (clear/rekey `last_good` on `url_q`
  change, not just on cursor change).

### (f) Android paging probe's step 4 has the exact survivor weakness fixed elsewhere

- **verdict**: CONFIRMED
- **evidence**: `end2end/android-catalog-paging-check.mjs:91-103` — fetches
  `bolt.next_cursor` via `limit=1` (line 93), navigates to the deep-linked
  cursored URL, and asserts only `page-first` present (line 98) and
  `page-next` absent (line 101). It never checks that `bolt.cards[0]` (the
  pre-cursor card) is absent from the rendered page, unlike
  `android-catalog-paging-check.mjs:79-81`'s own step 2 which does exactly
  that check. A cursor-ignoring build would re-render all of "bolt"'s results
  (still ≤50, so no next cursor) and still pass.
- **size**: S
- **disposition**: KEEP as standalone probe fix (add the same pre-cursor-card
  assertion step 2 already uses).

## P6-043

No catalog equivalent of `probe:paging`; search keyset covered only by an
`#[ignore]`d test; corrupt-cursor error mislabeled as a query error.

- **verdict**: CONFIRMED
- **evidence**: `end2end/package.json:12` defines `"probe:paging": "node
  all-cards-paging-check.mjs"` for `/my`'s `all_cards`, with no catalog
  counterpart script in the same file. `app/src/backend/hosted.rs:2875-2899`
  — `#[cfg(test)] mod search_live { ... #[ignore = "hits the live dev catalog
  (DATABASE_URL required)"] async fn query_engine_against_dev_poc_data() }` is
  the only keyset-walking coverage, and it's ignored by default. A corrupt
  cursor: `decode_cursor` (`hosted.rs:2803-2809`) returns
  `ApiError::Validation("invalid cursor")` on a bad base64/JSON payload,
  propagated by `search`'s `?` at `hosted.rs:270`; `catalog.rs:483-509`'s error
  arm renders any `Validation` as a query-grammar error in the `search-error`
  box (comment at `catalog.rs:509` even names "invalid cursor" as this exact
  case) — confirming the mislabeling.
- **size**: S
- **disposition**: KEEP.

## P6-044

Reverse paging (Previous) unbuilt on both `/my` and `/catalog`.

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog.rs:586-592` — `Pager`'s doc comment: "**Forward-only,
  matching `/my`'s pager** — a keyset cursor describes 'everything after this
  row', so Previous would need a second, reverse-ordered query and a `before`
  cursor. Browser Back already walks the pages you came through ... 'Back to
  the start' is the jump home". `Pager`'s implementation (`catalog.rs:598-641`)
  offers only "Back to the start" and "Next page →", no Previous control.
- **size**: M
- **disposition**: KEEP (build once, share across `/my` and `/catalog` if it's
  worth building — as the entry itself frames it).

## P6-086

Rail edit vs. pending query-bar debounce race, losing the rail edit.

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog/rail.rs:952-956` — comment on an *unrelated*
  facet-search debounce explicitly names this exact issue as still open: "The
  rail's known race (a facet click swallowed by the query bar's pending
  debounce, filed separately) is between *writers*; this adds none". The
  query bar's debounce timer lives in `app/src/components/query_bar.rs:90`
  (`let pending = StoredValue::new(...)`), private to that component; rail
  commits go through `use_navigate_query()` (`rail.rs:406-432`), a separate,
  unrelated write path with no shared timer handle. No context/shared state
  connecting the two exists anywhere in either file.
- **size**: M
- **disposition**: KEEP, deferred (matches entry's own "Codex review medium on
  the filter-rail task, deferred").

## P6-087

Rail sections don't spring open when a filter arrives from the query bar.

- **verdict**: CONFIRMED
- **evidence**: `app/src/catalog/rail.rs:738`
  `let open = section_seeded_open(default_open, count.get_untracked());` —
  computed once, non-reactively, at component creation, then set as a static
  `open=open` attribute on `<details>` (line 742). Doc comment at
  `rail.rs:721-724` confirms this is deliberate: "Openness is seeded once ...
  and then left to the user — re-deriving it reactively would slam a section
  shut under someone mid-click." So a filter term arriving later via the query
  bar (e.g. typing `r:rare`) updates the section's live `count` badge but
  cannot reopen an already-seeded-closed `<details>`.
- **size**: S, decision first
- **disposition**: PARK (trigger: a maintainer decision on whether a
  first-time-populated section should spring open, given the explicit
  mid-click-safety tradeoff already coded — this is a design call, not a bug
  fix).
