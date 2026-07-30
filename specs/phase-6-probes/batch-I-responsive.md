# Batch I — responsive / a11y / design deviations

Fast triage verification pass over `P6-001`, `P6-006`, `P6-007`, `P6-008`,
`P6-009` (bundle a–g), `P6-012` (bundle a–g), `P6-019`, `P6-020`, `P6-021`
(bundle a–f), against code on `docs/phase-6-triage` (atop `f9639d6`),
2026-07-30. Read-only — no files touched other than this report. Verbatim
entry text pulled from `specs/TODO-Phase-6.md` and the prior classification
rows in `specs/TODO-Phase-6-triage.md`. Line numbers below are current greps,
not the (drifted) numbers in the source files.

## P6-001 — two pre-existing table overflows (375 px, 768 px)

**Claim:** below ~385 px (9–14 px scroll at 375, 64–69 px at 320) and 30–34 px
at exactly 768 px, where `md` adds the Type column back before the extra
width is available. Predates the responsive audit; deliberately not fixed by
it.

- **verdict:** UNVERIFIABLE — the specific pixel magnitudes require a live
  render. **Check:** load `/my/collections/:id` (or `/my/all`) in a real
  browser/Playwright at 320, 375, and 768 px and compare
  `document.scrollingElement.scrollWidth` vs `clientWidth`.
- **evidence (structural, supports the claim's mechanism):**
  `app/src/my/collection.rs:1195` and `app/src/my/all_cards.rs:352` both carry
  `<TableHead class="hidden md:table-cell">"Type"</TableHead>` — the Type
  column reappears at exactly the `md` (768 px) breakpoint, which is the
  mechanism the claim describes for the 768 px overflow. Did not independently
  re-measure the 320/375 px numbers.
- **size:** M
- **disposition:** KEEP — runtime check named above must run before this can
  be resolved either way.

## P6-006 — mobile filter sheet slides left; frame draws a bottom sheet

**Claim:** the frame draws a bottom sheet with a grabber; the filter sheet
uses `SheetDirection::Left`. `SheetDirection::Bottom` already exists and the
card sheet uses it, so the fix is small but unrecorded, needing a ruling.

- **verdict:** CONFIRMED
- **evidence:** `app/src/catalog/rail.rs:463` —
  `direction=SheetDirection::Left` on the filter `SheetContent`.
  `app/src/cards.rs:376` and `app/src/bench/sheet.rs:26` both use
  `direction=SheetDirection::Bottom` for the card-preview sheet.
  `SheetDirection` (with a `Bottom` variant) is defined at
  `app/src/components/ui/sheet.rs:47`.
- **size:** S — decision first
- **disposition:** KEEP, **needs a maintainer design ruling** (bottom-vs-left)
  before any code changes.

## P6-007 — sub-44 px touch targets sanctioned by their own frames

**Claim:** catalog tile `+ Want`/`+ Have` measure 58.7×28 against the frame's
own 26; filter-sheet footer measures 283.5×36 against the frame's 44.

- **verdict:** CONFIRMED (height figures are direct code facts; the exact
  widths, and the frame's own numbers, are runtime/design-file-only — see
  check below)
- **evidence:** `app/src/catalog.rs:1024-1025` — `QuickAddButton` is
  `size=ButtonSize::Sm class="h-7 px-2 text-xs"`; `h-7` = 28 px, matching the
  claimed 28. `app/src/catalog/rail.rs:465-473` — the sheet's `Show N results`
  footer is a `<SheetClose variant=ButtonVariant::Default ...>` with no `size`
  override, so it takes `ButtonSize::Default` = `"h-9 …"` = 36 px
  (`app/src/components/ui/button.rs:61`), matching the claimed 36.
  **Check for the exact widths / frame values:** measure both elements live
  (`getBoundingClientRect()`) at the design's mobile frame width, and read the
  frame's own drawn sizes out of `design/wireframes.pen`.
- **size:** S — decision first
- **disposition:** KEEP, **needs a maintainer ruling**: does the 44 px
  guideline override a frame that draws smaller?

## P6-008 — `/login` copy/chrome deviates from `Desktop — Sign in`

**Claim:** no logo line, "Sign in" not "Sign in to your collection",
placeholders not labels, "No account? Sign up" not "New here? Create
account", `← Back to home` not "Browse the catalog without an account →",
shared `BackHome` also serves OTP/reset, desktop rail 240 px vs frame's 280.

- **verdict:** CONFIRMED
- **evidence:** `app/src/auth_pages.rs:292` — `<h1 …>"Sign in"</h1>`.
  `app/src/auth_pages.rs:298,305,380,385,392,482,491,529,589` — all fields are
  `placeholder=` with no `<label>` anywhere in the file (grep for `<label`
  returns nothing). `app/src/auth_pages.rs:333-334` — `"No account? "` +
  `<a href="/signup">"Sign up"</a>`. `app/src/auth_pages.rs:236-240` —
  `BackHome` renders `"← Back to home"`, and is reused at lines 336, 410, 613
  (signup, reset, resend-verification cards) — confirms the shared-component
  claim. Rail width: `app/src/shell.rs:447` — the sidebar rail is `w-60`
  (15rem = 240 px).
- **size:** S
- **disposition:** KEEP — straightforward copy/markup fix, no ruling needed
  (unlike P6-006/P6-007). Touching `BackHome`'s label affects OTP and reset
  cards too, per the entry.

## P6-009 (bundle a–g) — responsive-audit review-round minors

Umbrella: "none reaching the major bar." Three of the seven no longer
reproduce — the code already reads as the *fixed* version, with comments
that explicitly document the fix. This looks like review feedback that was
partly incorporated before the PR merged as a single squashed commit
(`7649d80`, the only commit ever touching these files' responsive classes).

### (a) `responsive.spec.ts` `.slice(0, 4)` misses reproducing collections

- **verdict:** STALE — already fixed
- **evidence:** No `.slice(0, 4)` exists in this loop anymore.
  `end2end/tests/responsive.spec.ts:409-425` (`smallHolders`) collects *every*
  qualifying collection, and line 438-442 carries a comment explicitly
  documenting the change: "Every qualifying collection, not the first four in
  tree order: this bug and both of its predecessors were collection-dependent
  … so a slice can miss every reproducing collection … Nine collections at
  ~1.5 s each is a price worth paying for that." The loop at line 443 iterates
  `await smallHolders(request)` with no slicing.
- **size:** S
- **disposition:** DROP — nothing to do, already resolved.

### (b) `px-1` savings switch back at `sm` while the select column persists to `md`

- **verdict:** STALE / WRONG as currently stated
- **evidence:** `app/src/my/collection.rs:1191-1199` and
  `app/src/my/all_cards.rs:348-356` — both tables' `Where`/`Wanted`/`Owned`
  `TableHead`s are `class="px-1 text-right md:px-2"` (or `px-1 md:px-2`) —
  the padding recovers at **`md`**, the *same* breakpoint as the select
  column (`w-11 md:w-8`), not at `sm` as claimed. No `sm:px-2` appears
  anywhere in either file. The described 640–767 px gap does not exist in the
  current code.
- **size:** S
- **disposition:** DROP — already resolved.

### (c) `toaster_offset`'s unit test asserts shape, not magnitude

- **verdict:** CONFIRMED
- **evidence:** `app/src/shell.rs:614-642`
  (`toaster_offset_clears_whatever_is_docked_below_it`) checks `assert_ne!`
  between the two arms, that the phone-width term `starts_with("bottom-[")`
  and isn't `"bottom-6"`, and that the tray arm `.contains("md:")` — string
  shape only. No assertion compares the actual `rem` magnitude against the
  measured table in the function's doc comment.
- **size:** S
- **disposition:** KEEP

### (d) selection-tray tap target and a11y target are different elements

- **verdict:** CONFIRMED
- **evidence:** `app/src/components/ui/selection_tray.rs:220-235`
  (`SelectionCheckbox`) — the 44 px hit area is a
  `<span class="flex size-11 … md:size-4" on:click=…>` with no `role` or
  `tabindex`; the accessible control is the `<Checkbox>` inside it, which
  carries `role`/`aria-checked`/focus per the doc comment above it ("the span
  owns the toggle and the checkbox has no `on_checked_change` at all").
- **size:** S
- **disposition:** KEEP

### (e) `results.get().map(|p| p.search)` clones per reactive read, twice

- **verdict:** CONFIRMED
- **evidence:** `app/src/catalog.rs:228` (`last_good`'s `Effect`) and
  `app/src/catalog.rs:307` (`count_after_hydrate`'s `Effect`) both execute
  `if let Some(Ok(r)) = results.get().map(|p| p.search) { … }` — two separate
  `Effect`s each cloning the `Resource`'s value out on every reactive read.
- **size:** S
- **disposition:** KEEP

### (f) inverted reasoning in the 50-card comment

- **verdict:** STALE — already fixed
- **evidence:** `end2end/tests/responsive.spec.ts:403-408` already states the
  claimed-correct reasoning verbatim: "the assertion below is `tiles >
  collectionCards`, and a catalog page holds 50. At 50 cards the comparison is
  `50 > 50` — false — so a full-size collection … makes the test fail in
  *both* renderings, the correct one included. The filter exists to keep the
  assertion meaningful, not because a big collection would pass either way."
  No inverted version exists in the current file.
- **size:** S
- **disposition:** DROP — already resolved.

### (g) orphaned `zz-e2e-inb-src-w1-9` collection on Neon dev

- **verdict:** UNVERIFIABLE — DB-only claim. **Check:** query the Neon `dev`
  branch for a collection named `zz-e2e-inb-src-w1-9` and compare against
  current e2e fixture-naming prefixes. Not checked this pass (read-only,
  no-DB constraint).
- **size:** S
- **disposition:** MERGE→`P6-065` (prior triage row already proposed this
  pairing; both are Neon dev-branch fixture cleanup).

**UNITS** (P6-009 survivors):
- Strengthen `toaster_offset` unit test to assert magnitude, not shape (c) — S
- Split selection-tray tap target from its a11y target (add role/tabindex or
  move the handler) (d) — S
- Stop double-cloning the search payload in `catalog.rs`'s two Effects (e) — S
- Investigate/clean up orphaned Neon dev-branch fixture, with P6-065 (g) — S,
  MERGE→P6-065
- (a), (b), (f): DROP, no unit — already fixed, nothing to schedule.

## P6-012 (bundle a–g) — states-sweep review-round minors

Umbrella: "none reaching the major bar." Six of seven still reproduce; one
(f) does not.

### (a) auth-page `role="alert"` text is not a pre-existing live region

- **verdict:** CONFIRMED
- **evidence:** `app/src/auth_pages.rs:320,404,541,611` — all four
  `<p role="alert" class=ERROR_TEXT>` are created together with their text
  content, each inside a `<Show when=move || error.get().is_some()>` —
  the element and its text both appear atomically, rather than an
  always-present empty live region being filled in.
- **size:** S
- **disposition:** KEEP

### (b) `describe_error` duplicates `classify`'s prefix table; search-error banner has no `data-failure`

- **verdict:** CONFIRMED
- **evidence:** `app/src/catalog.rs:132-141` (`describe_error`) independently
  does `raw.strip_prefix("validation: ")`. `app/src/components/states.rs:100-113`
  (`classify`) has its own, separate prefix table (`not found: `, `conflict: `,
  `forbidden: `, `validation: `, `unauthorized: `, `upstream: `) — two copies
  of overlapping logic, confirmed. `app/src/catalog.rs:494-505` — the
  `search-error` banner has `role="alert"` and `data-testid="search-error"`
  but no `data-failure` attribute; classes are hand-rolled
  (`border-destructive/40 bg-destructive/10 …`) rather than routed through
  the shared classifier/banner.
- **size:** S
- **disposition:** KEEP

### (c) `/my/collections/<malformed>/needs` back link points at the error page itself

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/needs.rs:321-330` — the `validation:` arm's
  comment: "The way out is already on the page: `NeedsHeader` sits *outside*
  this boundary … which is why this is the one error banner here that needs
  no `children`." `app/src/my/needs.rs:389-411` (`NeedsHeader`) builds
  `href = format!("/my/collections/{id}")` from the *same* `url_id` — i.e.
  the same malformed id string — so the back link targets
  `/my/collections/<malformed>`, which is itself the id-parse error page.
- **size:** S
- **disposition:** KEEP

### (d) destination-picker docstring claims it SSRs for any session; it's gated on sign-in

- **verdict:** CONFIRMED
- **evidence:** `app/src/catalog/destination.rs:393-395` — comment: "the one
  that SSRs (`/catalog` renders it for any session)." But
  `app/src/catalog/destination.rs:295-307` (`DestinationPicker`) renders
  `PickerBody` only when `matches!(user.await, Ok(Some(_)))` — signed-in
  callers only.
- **size:** S
- **disposition:** KEEP (doc fix)

### (e) `LoadError` docstring overclaims "the only thing on the page"

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/collection.rs:396-398` — doc comment: "the
  header's failed arm — … the *only* thing on the page when the read
  fails." `QuickAddPanel` (imported line 67, rendered line 310) and the
  shell's rail/tabs are driven by other resources/components and are not
  gated by this header boundary, so they remain on screen when `LoadError`
  renders. The docstring's literal "only thing on the page" is an overclaim.
- **size:** S
- **disposition:** KEEP (doc fix)

### (f) `err.locator("..")` hops to the parent needlessly

- **verdict:** STALE — not found in current code
- **evidence:** `grep -n 'err.locator' end2end/tests/states.spec.ts` shows
  only `err.locator('[role="alert"]')` (line 203) and
  `err.locator("[data-tone]")` (line 205); no `err.locator("..")` /
  parent-hop pattern exists anywhere in the file.
- **size:** S
- **disposition:** DROP — already resolved (or never landed as described).

### (g) `btn` dereferenced with no null check in `android-states-check.mjs`

- **verdict:** CONFIRMED
- **evidence:** `end2end/android-states-check.mjs:138-146` —
  `const btn = document.querySelector('[data-testid="bench-error-transport"] [data-testid="state-retry"]');`
  is immediately followed by `const r = btn.getBoundingClientRect();` inside
  `page.evaluate`, with no null/undefined guard on `btn` in between.
- **size:** S
- **disposition:** KEEP

**UNITS** (P6-012 survivors):
- Convert the four auth-page error `<p role="alert">`s (+ `ErrorNote`'s
  cold-SSR case) into pre-existing live regions (a) — S
- Unify `validation:` prefix parsing onto `states.rs::classify` and give
  `/catalog`'s search-error banner a `data-failure` seam (b) — S
- Fix the needs-page malformed-id back link so it doesn't point at itself
  (c) — S
- Fix two stale doc comments: destination-picker SSR claim, `LoadError`
  "only thing on the page" claim (d + e) — S
- Null-check `btn` in the Android states probe before use (g) — S
- (f): DROP, no unit — already resolved.

## P6-019 — no vendored icon set; emoji glyphs collide

**Claim:** 🗂 (All cards) and 📁 (a collection) render near-identically at
15 px; the distinction rests on `font-semibold` + a divider. Wireframes
specify distinct lucide glyphs (`layers`/`folder`).

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/root.rs:50,52` —
  `const ICON_ALL_CARDS: &str = "🗂";` / `const ICON_COLLECTION: &str = "📁";`.
  `app/src/my/root.rs:178,290` — the differentiator is `font-semibold` on the
  "My cards" `<h1>` / row label, plus a `divider_before` flag (`root.rs:130-149`).
  No `icons`/`lucide` crate dependency exists anywhere in `Cargo.toml` or
  `app/Cargo.toml` (grep returns nothing) — `specs/ui-components.md:20`
  mentions `icons` only as a planned supporting crate, never adopted.
- **size:** M, pays off everywhere (matches prior triage row)
- **disposition:** KEEP

## P6-020 — All-cards 390 px fit is data-dependent

**Claim:** no cell carries `whitespace-nowrap`; the WHERE column renders
`"{n} in {collection_name}"` with a user-chosen name, so one space-free name
(or long card name) can re-exceed the 356 px wrapper.

- **verdict:** CONFIRMED
- **evidence:** `grep -n 'whitespace-nowrap' app/src/my/all_cards.rs
  app/src/my/collection.rs` returns nothing — no cell is protected.
  `app/src/my/all_cards.rs:485` —
  `{format!("{} in {}", loc.quantity, loc.collection_name)}` — exactly the
  cited format, with `collection_name` user-chosen and unbounded.
- **size:** S
- **disposition:** KEEP

## P6-021 (bundle a–f) — mobile-`/my`-root review-round minors

Umbrella: "none reaching the major bar." Five still reproduce; (f) is
explicitly already-fixed-during-the-task per the entry's own text, confirmed
in current code.

### (a) `MyRootNav` assembles the tree on desktop too; three `assemble()` calls per load

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/root.rs:163-176` (`MyRootNav`) is wrapped in
  `<div class="flex min-w-0 flex-col md:hidden" …>` — CSS-only hiding
  (`display:none` at `md`+), so the component still mounts, and its
  `Suspend`/`assemble(dto)` (line 188) still runs, on desktop.
  `app/src/my/all_cards.rs:132` — `<super::root::MyRootNav />` is rendered
  unconditionally inside `AllCardsBody`, i.e. on every `/my` load regardless
  of viewport. Three separate `assemble()` call sites confirmed: root list
  (`root.rs:188`), rail (`app/src/my/tree.rs:156`), and the bottom-tab badge
  (`app/src/shell.rs:505`, `BottomTabs`, `crate::my::tree::assemble(dto).inbox_count`).
- **size:** S
- **disposition:** KEEP

### (b) phone's bare `/my` blocks under `SsrMode::Async` on the aggregate read

- **verdict:** CONFIRMED
- **evidence:** `app/src/lib.rs:130` —
  `<Route path=StaticSegment("") view=my::all_cards::AllCardsPage ssr=SsrMode::Async />`
  for the bare `/my` path. `app/src/my/all_cards.rs:148`
  (`AllCardsBody`) backs this route and renders the full (hidden-on-mobile)
  table alongside `MyRootNav`.
- **size:** M, needs a client hint or two documents (matches prior triage row)
- **disposition:** KEEP

### (c) rail's pinned "All cards" row links to `/my`, not the table

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/tree.rs:266-274` — `<PinnedRow href="/my" … label="All cards" …/>`,
  with a comment noting the rail is "on screen there too at `md` and up"
  (i.e., the same `Rail`/`PinnedRow` markup backs the mobile drawer).
  Tapping it inside the mobile drawer therefore lands on `/my`'s root list,
  not `/my/all`'s table.
- **size:** S
- **disposition:** KEEP

### (d) `<nav aria-label="My cards">` duplicates the bottom tab bar's name

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/root.rs:267` —
  `<nav aria-label="My cards" class="flex flex-col …" data-testid="my-root-list">`.
  `app/src/shell.rs:465-523` (`BottomTabs`) — the `/my` tab link's visible
  text is `<span>"My cards"</span>` (line 522), giving the same accessible
  name to a second landmark in the same document.
- **size:** S
- **disposition:** KEEP

### (e) `list_skeleton`'s `aria-busy`/`aria-label` sit on a roleless `<div>`

- **verdict:** CONFIRMED
- **evidence:** `app/src/my/root.rs:254-257` (`list_skeleton`) —
  `<div class="space-y-2 px-2 py-1" aria-busy="true" aria-label="Loading your collections">`,
  a plain `<div>` with no `role` attribute.
- **size:** S
- **disposition:** KEEP

### (f) e2e shape lesson: `toBeGreaterThan(1)` could not detect an unmeasured `TableWrapper`

- **verdict:** CONFIRMED as already-fixed (the entry itself says "fixed
  during the task, but the shape is worth remembering" — historical note,
  not an open defect)
- **evidence:** `end2end/tests/my-root.spec.ts:328` —
  `if (!el.clientWidth) continue;` is present, matching the described fix
  (skip unmeasured/`display:none` elements rather than letting the count
  alone satisfy the assertion).
- **size:** — (not a task)
- **disposition:** DROP — no code action; already resolved, kept only for
  institutional memory (possibly worth folding into the `e2e-suite` skill's
  "assertions that lie" guidance, but that's a docs suggestion, not a P6
  task).

**UNITS** (P6-021 survivors):
- Stop `MyRootNav` from assembling/rendering its subtree on desktop (gate
  behind viewport or lazy-mount the `Suspend`) (a) — S
- Avoid the full aggregate `SsrMode::Async` block on phone-width `/my` (needs
  a client hint or split into two documents) (b) — M
- Point the rail's pinned "All cards" row at the table for mobile-drawer taps
  (c) — S
- De-duplicate the "My cards" accessible name between the root `<nav>` and
  the bottom tab bar (d) — S
- Give `list_skeleton`'s busy/label div a role (e) — S
- (f): DROP, no unit — already fixed, historical note only.
