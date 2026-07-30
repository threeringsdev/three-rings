# Batch J — card surfaces / DFC / catalog visuals / perf

Read-only triage pass. All line numbers reflect `grep` at time of writing (2026-07-30); re-grep before trusting them later.

## P6-005 + P6-103 — `+ Want` / `+ Have` missing from hover preview, mobile sheet, and card detail page

**Verdict:** CONFIRMED (bundle)

**Evidence:**
- `app/src/cards.rs` — zero hits for `Want`/`Have`/`QuickAddButton`/`raise_add_toast`; `PreviewBody` (cards.rs:164) and `CardDetailBody` (cards.rs:634) render art/name/mana/type/stats/ownership only, no add affordance.
- `app/src/catalog.rs:951` — `fn QuickAddButton(name, oracle_id, printing_id, kind) -> impl IntoView` exists but is **module-private** (no `pub`), reads `destination::current_destination()` (catalog/destination.rs:572) and `expect_context::<ToastHandle>()`.
- `app/src/shell.rs:169` — `provide_destination_state()` is called at the **shell** level (app-wide), not scoped to the catalog page. So `current_destination()` and the toast/tree contexts QuickAddButton needs are already reachable from `cards.rs` without new plumbing — the wiring cost is lower than "needs its own context stack."
- `app/src/cards.rs:634-654` — `CardDetailBody` destructures `CardDetail` with `..`, dropping `oracle_id`; it does keep `printings: Vec<PrintingSummary>` and already computes `hero_printing` (a `PrintingSummary` with `.id`), so a printing id for `+Have` is available, oracle_id needs to be captured.
- `PreviewBody` (cards.rs:164) is the **single shared body** for both the hover card and the mobile sheet (`CardPreview`, cards.rs:266-401 calls it from both branches) — wiring it once covers P6-005's two surfaces at once.

**Size:** M (matches triage's own bundle estimate) — make `QuickAddButton` `pub(crate)`, thread `oracle_id`/`printing_id` into `PreviewBody` (covers hover + sheet) and into `CardDetailBody` (covers detail page, needs to stop dropping `oracle_id` and use `hero_printing.map(|p| p.id)`), plus each surface's own optimistic/pending state.

**Disposition:** KEEP as one bundled unit — this is already how TODO-Phase-6-triage.md groups it (line 133) and is called out as the top of the missing-capability class. No new spec work needed (`app-ui`/`ui-design` are both `implemented`, ungated).

**Blocked-by / duplicate-of:** none.

---

## P6-016 — Per-card-row kebab is unbuilt

**Verdict:** CONFIRMED

**Evidence:**
- `design/wireframes.pen:158` — `"name": "Row Kebab"`, a 16px lucide `ellipsis` icon, sibling to the row's `Cell Owned`/`Cell Wanted` cells (i.e. under `Card Row`, not the tree).
- Only a **collection-header** kebab exists in code: `app/src/my/collection.rs:860` `pub(crate) fn HeaderKebab(...)`, doc comment at :822 ties it explicitly to `Header Kebab` in the wireframes, and :835 says its five actions are deliberately **collection lifecycle only** (not row-level).
- `specs/app-ui.md:1710` — "deck-only, the spec's per-row move affordance is unshipped… Filed as a Phase 5 [task]" — the spec itself already records this as outstanding.
- No `kebab`/`ellipsis` symbol anywhere in `app/src/cards.rs` or `app/src/catalog.rs`.

**Size:** M — needs a new per-row menu component plus reuse of the existing "move to…" destination-picker plumbing (`tree_manage.rs`) at row grain; distinct trigger from the header kebab, which is intentionally collection-scoped.

**Disposition:** KEEP.

**Blocked-by / duplicate-of:** none.

---

## P6-072 — DFC flip follow-ups (a–f)

### (a) vacuous test on `CardFaceSummary::build`'s `faces.len() < 2` guard

**Verdict:** CONFIRMED

**Evidence:** `shared/src/catalog.rs:200` — `CardFaceSummary::build` has its own `if faces.len() < 2 { return vec![] }`, separate from `CardDetail::flip_faces`'s guard at line 142. The only boundary test for "exactly one face element" is `flip_faces_is_empty_on_missing_or_malformed_jsonb` (line 367-376), which exercises `CardDetail::flip_faces`, **not** `CardFaceSummary::build`. The three `summary_faces_*` tests (lines 379-408) cover: 2-face zip, missing images, and off-allowlist/`None` layouts — none feed an on-allowlist layout with exactly one parseable face into `CardFaceSummary::build`. So that guard's leg is genuinely untested, confirmed by reading test coverage (not by running the suite, per read-only constraint).

**Size:** S. **Disposition:** KEEP (add the missing regression test), bundle with (b) per triage.

### (b) no test asserts the mana-cost/type-line/stats swap

**Verdict:** CONFIRMED

**Evidence:** `app/src/cards.rs:87` `fn stats_line(...)` has no direct references anywhere outside its two call sites (`PreviewBody`/`CardDetailBody`); no test in the repo names `stats_line`. The claim that the e2e fixture card has no P/T is a claim about `e2e/` fixtures, not verifiable by static grep alone without deeper fixture inspection — treated as asserted, consistent with the existing test-coverage gap being the load-bearing fact.

**Size:** S. **Disposition:** KEEP, bundled with (a) (`P6-072a + P6-072b`, triage line 255).

### (c) keywords badge row is card-level, doesn't follow the face swap

**Verdict:** CONFIRMED

**Evidence:** `app/src/cards.rs:634-654` — `keywords` is destructured once from the top-level `CardDetail` (outside the per-face `panel: Signal<FacePanel>`), and rendered unconditionally as its own `Badge` row (~line 763-778) regardless of which face is showing. `FacePanel` (cards.rs:75-85) has no `keywords` field at all, so the flip control never touches it.

**Size:** S. **Disposition:** KEEP.

### (d) preview flip state survives close/reopen, contradicting the doc comment

**Verdict:** CONFIRMED

**Evidence:** `app/src/cards.rs:160-163` doc comment: "Face state is per-affordance: the hover card and the sheet are separate `PreviewBody` instances, **each starting at the front**." But `hovered`/`sheet_seen` (cards.rs:305-306) are one-way latches — set `true` on first interaction and never reset back to `false` anywhere in the file — and the `<Show when=move || hovered.get()>` / `<Show when=move || sheet_seen.get()>` wrappers (cards.rs:357-358, 388-390) therefore mount `PreviewBody` exactly once per `CardPreview` instance and never unmount it. `PreviewBody`'s `face` `RwSignal` (cards.rs:177) is created at that single mount, so re-hovering (or re-opening the sheet on) the same row preserves whatever face was last flipped to — it does not restart at the front. This is a real discrepancy between the comment and the runtime behavior, not a bug in the strict sense (the latch behavior is deliberately documented for a different reason — avoiding remount thrash).

**Size:** S. **Disposition:** RESCOPE — either reset `face` on the transition that hides the affordance again (mouseleave for hover, sheet-close for the sheet) or correct the comment to describe actual persistence; open question for the maintainer on which behavior is wanted (record in spec before implementing).

### (e) flip-control a11y

**Verdict:** CONFIRMED

**Evidence:** `app/src/cards.rs:106-150` `FlipButton` — `aria-label="Flip card"` (line 124) is a static string literal; no `aria-pressed`, no dynamic label naming the now-visible face, and no live region/announcement on swap (`face.update(...)` at line 131 is the only state change, purely visual).

**Size:** S. **Disposition:** KEEP.

### (f) `jsonb_array_elements(p.faces)` hard-errors on a non-array

**Verdict:** CONFIRMED — and already tracked elsewhere

**Evidence:** `app/src/backend/hosted.rs` has three occurrences (lines 159, 859, 2489) of `FROM jsonb_array_elements(p.faces) WITH ORDINALITY AS t(f, ord)`, none shape-guarded (old code used a shape-tolerant `->0` projection). `specs/TODO-Phase-6.md:138` (`P6-108`) already states: **"Prereq: `P6-072f`"** and explains it's unreachable today only because the POC subset (2,976 printings) happens not to produce a non-array `faces`, and a 116K-row bulk load is exactly what would surface it. `specs/TODO-Phase-6-triage.md:123` independently classifies it: "Prerequisite of `P6-108`, not a sibling."

**Size:** S. **Disposition:** PROMOTE — split out of the P6-072 bundle into its own small task that gates `P6-108` (bulk Scryfall load), since it's already documented as a hard prerequisite there and shipping the bundle as one lump would make `P6-108`'s dependency harder to track discretely.

**Blocked-by / duplicate-of (whole P6-072 bundle):** (f) is `blocked-by:` nothing but is itself a `blocks: P6-108`.

---

## P6-090 — `CardPreview`'s pointer-type detection isn't hoisted

**Verdict:** CONFIRMED

**Evidence:** `app/src/cards.rs:284-297` — inside `CardPreview` (one component instance per card row/tile), an `Effect` calls `window().match_media("(pointer: coarse)")` and sets a per-instance `coarse: RwSignal<bool>`. No `MediaQueryList::add_event_listener`/`onchange`/`addListener` call anywhere in `cards.rs` — grepped, zero hits. `CardPreview` is instantiated from three call sites: `app/src/catalog.rs:720,809` (grid + list results), `app/src/my/all_cards.rs:416`, `app/src/my/collection.rs:1383` — so the per-card, per-page-load `match_media` call and its lack of a change listener is real and reproduced at every one of those surfaces, matching "Cost multiplies as Stage 3 surfaces adopt `CardPreview`."

**Size:** S — hoist to one signal provided once (e.g. alongside `provide_destination_state()` in `shell.rs`), add the missing `addEventListener('change', ...)`.

**Disposition:** KEEP.

**Blocked-by / duplicate-of:** none.

---

## P6-097 — Catalog first load is slow

**Verdict:** PARTLY / UNVERIFIABLE for magnitude. **Runtime check needed:** browser DevTools network waterfall on a cold `/catalog` GET (SSR + hydration + image fetches), plus `EXPLAIN ANALYZE` on the `search`/`catalog_count` queries against the dev Neon branch. No such measurement was taken here (out of scope for triage / no server started).

**Evidence (what governs the behavior today, confirmed by reading, not profiling):**
- `app/src/catalog.rs:206-243` (`CatalogPage`) — the common cold/browse-all load fires **two concurrent server round trips**: the `results` Resource (`crate::search_catalog`, the paged search) and a separate `count` Resource (`crate::catalog_count`) gated on `url_q.read().is_empty()` — i.e. exactly the first-load case pays for both.
- `app/src/backend/hosted.rs:263-313` (`search`) — each row runs a `LEFT JOIN LATERAL` (`REPRESENTATIVE_PRINTING_JOIN`, hosted.rs:2484-2494) that does its own `ORDER BY ... LIMIT 1` per card to pick a representative printing, plus a **second** query afterward (`owned_by_oracle`) for the owned-count badges.
- `app/src/catalog.rs:730-740` (`CardTile`) — renders Scryfall's `normal` image size (`image_uris->>'normal'`, up to 488×680) with `loading="lazy"` but no `srcset`/`sizes`, so every above-the-fold tile (default grid is `xl:grid-cols-6`, i.e. dozens of tiles) requests a full-size image with no responsive downscaling.
- Indexes exist: `migrations/0002_catalog.sql:105-106` (`cards_name_trgm_idx`, `printings_oracle_idx`) and `migrations/0008_search_indexes.sql` (additional trgm indexes) — so this isn't an obviously-missing-index problem at the current ~3K-printing POC scale (`P6-108`'s own numbers).

**Size:** M, measure first (matches triage's own sizing) — the fix shape depends entirely on where the time actually goes (network image weight vs. DB round trips vs. Neon cold-start vs. wasm/hydration), which static reading cannot resolve.

**Disposition:** PARK — trigger: get one real profile (network waterfall + `EXPLAIN ANALYZE`) of a cold `/catalog` load before scoping any fix; without it, work here is a guess.

**Blocked-by / duplicate-of:** none.

---

## P6-098 — Catalog card size uncapped at large widths

**Verdict:** CONFIRMED

**Evidence:** `app/src/catalog.rs:670` — `const GRID_CLASS: &str = "grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6"`. No breakpoint beyond `xl` bumps the column count further, and grepping `app/src/catalog.rs` for `max-w`/`container` returns nothing — no page- or grid-level width cap wraps the results grid. `app/src/shell.rs`'s only `max-w-3xl` is on an unrelated toast/dialog wrapper, not page content. So on any viewport wider than the `xl` breakpoint, the grid's 6 columns keep splitting the full (uncapped) container width evenly, and each tile (`aspect-[5/7]`) grows without bound.

**Size:** S — either add a `max-w-*` wrapper around the results grid or add another breakpoint step (e.g. `2xl:grid-cols-8`).

**Disposition:** KEEP.

**Blocked-by / duplicate-of:** none.

---

## P6-099 — Black background too dark, card borders blend in

**Verdict:** CONFIRMED as a real, locatable token pairing (the "too dark" judgment itself is a design call, not something code-verifiable).

**Evidence:** `style/input.css` — `.dark { --background: oklch(0.18 0 0); ... }` (near-black background, ~line 51) paired with `--border: oklch(1 0 0 / 10%)` (white at 10% alpha, ~line 83). Dark is the app's **default** theme (file header comment, style/input.css:1-7: "Dark is the DEFAULT... maintainer decision 2026-07-17"). A white border at 10% opacity composited over an L=0.18 background is a low-contrast combination, consistent with "borders blend in."

**Size:** S — token tuning (raise `--background` lightness and/or `--border` alpha/lightness in `.dark`).

**Disposition:** KEEP, but route through a design pass rather than an arbitrary engineering tweak — this is a perceptual/contrast call, not a functional bug.

**Blocked-by / duplicate-of:** none.

---

## P6-100 — DFC flip unavailable on the catalog page and everywhere except hover view

**Verdict:** CONFIRMED

**Evidence:** `app/src/catalog.rs:720` — `CardTile` calls `<CardPreview card=preview hover=false>` ("the tile is already the card art, so a hover preview would just repeat it smaller" — comment at line 718). With `hover=false`, `CardPreview` (cards.rs:266) never mounts the hover-card branch at all; `PreviewBody` (which owns the only `FlipButton` besides the detail page's) only mounts when `sheet_seen` latches true, i.e. only on a touch tap. The tile's own always-visible art (cards.rs — `CardTile`'s bare `<img>`, catalog.rs:730-740) has no flip control. So on desktop, browsing the grid/list, there is **no** way to flip a DFC without opening the detail page — matches the claim exactly ("everywhere the card is rendered besides hover view", and hover itself is off for tiles).

**Size:** S–M (matches triage) — needs a flip affordance directly on `CardTile`/list rows, independent of the hover/sheet gating that `CardPreview` otherwise controls.

**Disposition:** KEEP.

**Blocked-by / duplicate-of:** none.

---

## P6-101 — No back button from card detail view to catalog page

**Verdict:** CONFIRMED

**Evidence:** `app/src/cards.rs:598` and `:612` — `"Back to the catalog"` links to `/catalog` exist, but only inside `LoadFailed` (error state, ~line 570-606) and `NotFound` (~line 607-619). The **successful** render path, `CardDetailBody` (cards.rs:634-820), has no back link, breadcrumb, or any `<a href="/catalog">` anywhere in its view — confirmed by grepping the whole function body for `back`/`Back`/`breadcrumb`/`<a href`.

**Size:** S — add the same (or a shared) back-to-catalog affordance to the success path.

**Disposition:** KEEP.

**Blocked-by / duplicate-of:** none.
