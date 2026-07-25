# App UI — the real pages

**Status:** accepted
**Depends on:** [ui-design](ui-design.md), [ui-components](ui-components.md), [ui-component-bench](ui-component-bench.md), [collection-api](collection-api.md), [catalog-search](catalog-search.md), [auth](auth.md), [ui-work-loop](ui-work-loop.md)

## Problem

Every backend spec is `implemented` and every design artifact exists, but the app
still serves the scaffold counter on `/`. The catalog, collections, auth, search,
and tagging surfaces are reachable only via raw JSON routes. This spec defines
the construction of the real UI — the nine wireframed screens, the app shell,
and the three custom gap components — page by page, so each task ships
independently through the merge gate.

Design authority (all Phase 1b deliverables; this spec distills, never
overrides): [`design/information-architecture.md`](../design/information-architecture.md)
(route map, shells, navigation), [`design/wireframes.pen`](../design/wireframes.pen)
(9 screens + overlays + add-flow storyboards),
[`design/add-flow-prototype.md`](../design/add-flow-prototype.md) (keyboard-first
quick-add, time-to-enter-50-cards metric),
[`design/command-palette.md`](../design/command-palette.md) (⌘K),
[`design/component-gap-analysis.md`](../design/component-gap-analysis.md)
(27 primitives: 20 direct, 4 composites, 3 gaps). When a task's acceptance
criteria here feel thin, the wireframes are the source of truth.

## Scope

**In:**
- The full v1 route map (below) replacing the counter, on web and both Tauri shells.
- The app shell: desktop (top-bar mode switch, sidebar rail, docked selection
  tray) and mobile (bottom tabs, drill-down, slide-over filter sheet).
- Component vendoring per the gap analysis (three batches) and the three custom
  gap components: collection tree, in-place count stepper, selection tray.
- Thin per-screen server-fn adapters over the existing `CatalogStore` /
  `CollectionStore` trait methods (the adapters collection-api deliberately
  deferred here).
- Dark palette + migration of existing pages off hardcoded hex onto theme tokens.
- Dev seed data for the test user so `/my/*` screens are buildable.
- The ⌘K command palette (desktop, logged-in).
- E2E coverage per feature (the loop contract lives in
  [ui-work-loop](ui-work-loop.md)).

**Out (parked, per TODO.md Later/parked):** decks-sharing, import/export
(CSV/Moxfield), buy-link integration, format legality, offline bundled catalog,
full-catalog ingest (UI builds against the ~3K-printing POC subset; quick-add
disambiguation realism and list-perf findings are recorded as deferred), app
update delivery.

## Design

### Route map

| Route | Page | Access |
|---|---|---|
| `/` | redirect: authed → `/my`, anon → `/catalog` | public |
| `/catalog` | catalog search/browse | public |
| `/cards/:id` | card detail | public (ownership section authed) |
| `/my` | All cards (My cards landing) | auth |
| `/my/collections/:id` | collection view (binder / deck) | auth |
| `/my/collections/:id/needs` | needs view + pick list | auth |
| `/my/shopping` | shopping list | auth |
| `/login`, `/signup` | auth screens (exist; restyle onto tokens) | public |

Two top-level modes — **Catalog** ("what exists?") and **My cards** ("what do I
have and where?") — switched in the top bar (desktop) / bottom tabs (mobile).
The sidebar rail is mode-filled: filter rail in Catalog, collection tree in My
cards. The selection tray docks at the bottom and survives mode switches.

### Per-page acceptance criteria (distilled; wireframes govern detail)

**`/catalog`** — filter rail (name, card text, set, color, type, rarity, mana
value; multi-selects serialize to comma-OR terms) + query bar above results.
Query text is the canonical state, in the URL (`?q=…&cursor=…`); rail edits
rewrite their term, recognized terms reflect into widgets, unrecognized terms
preserved verbatim (catalog-search contract). Live typing: ~250 ms debounce, one
in-flight request, stale-response discard, first page SSR when the URL carries
`q`. Grid/list toggle; tiles lead with the image (lazy-loaded, skeleton
placeholder). Every result carries `+ Want` / `+ Have` and the sticky
destination picker (`Adding to: 📥 Inbox ▾`, persists across searches).
Logged-out: quick actions prompt sign-in. Mobile: filter rail becomes a
slide-over sheet with an active-filter badge count.

**`/cards/:id`** — full detail: printings, rulings, "your copies & locations"
when authed. Desktop hover on any row/tile opens a lightweight `hover_card`
preview (no URL change); touch tap opens a bottom `sheet` with a "Full
details →" expansion. Multi-face printings must render an image (see the
projection fix below).

**`/my`** — everything-view aggregating all collections incl. Inbox; same row
treatment as collection view but the HERE column is replaced by an expandable
location summary (`7 across 3 collections`). Quick search input, keyset paging.

**`/my/collections/:id`** — child collections as folder rows on top, cards
below. Three right-aligned numeric columns under one header: HERE / WANTED /
OWNED (WANTED only when set and different; OWNED collapses when equal to HERE;
rolled-up child counts italic + dimmed). HERE is editable in place via the count
stepper. Persistent in-collection quick-search/type-ahead in the header (`/`
focus hint) that filters this collection and inline-adds catalog matches — the
intake path. Per-row move (kebab / swipe / `m`) and select (checkbox /
long-press / `x`) affordances. The needs chip
(`6 missing — 4 owned elsewhere · 2 to buy`) sits on **any** collection's
header, not only a deck's — `design/information-architecture.md` line 41, the
authority this spec distills, puts it on "a deck **or collection** header", and
`/my/collections/:id/needs` is a route for any collection. (This bullet
previously listed the chip under the deck variant; corrected 2026-07-25, see
Findings.) **Deck variant** adds: format + commander(s) rendered as a card in
the header, cards grouped by type with counts, Want-led add default (binders
and Inbox are Have-led), and the "Empty deck…" teardown action (single
destination or "Return to previous locations").

**`/my/collections/:id/needs`** — two buckets: **Owned elsewhere** rows show
where copies live + a one-tap **Pull** (pre-filled move); **Pull all** generates
a pick list (checklist grouped by source collection; checking records the
move). **Short** rows feed the shopping list. Pull/pull-all are client-composed
from `move_cards` + `suggested_destinations` (collection-api Findings).

**`/my/shopping`** — one row per card: shortfall count + which collections want
it; text export.

**App shell** — desktop: slim top bar (mode switch `Catalog | My cards`, user
menu/avatar), sidebar rail, main panel, selection tray docked bottom, undo toast
on every move. Mobile: two tabs `[📖 Catalog] [🗂 My cards •N]` (badge = Inbox
unsorted count); My cards is drill-down (root mirrors the sidebar; back walks
up); tray docks above the tab bar and survives tab switches.

**⌘K palette** — desktop, logged-in only. Places (flattened collections with
parent-path meta + system places + mode jumps) and a fixed 3-command registry
(`New binder…`, `New deck…`, `Undo last move`); at rest RECENT + COMMANDS,
first row pre-selected. Client-side filter over a preloaded index.

**Quick-add panel** — the central intake composite (`command` + `popover` +
`input` + `kbd`). Keystroke contract from the storyboards:
`↑↓ navigate · ⏎ add 1 here · ⇧⏎ set count · ⌥⏎ want instead`; desktop steady
state ≈ 5–7 keystrokes/card, zero pointer. Deck context flips the default to
Want. E2E asserts the keystroke contract.

### Custom gap components (bench section required, like any vendored component)

1. **Collection tree** — the largest: nesting, per-node collapse, pinned system
   rows (All cards / Inbox / Shopping list), selection, rolled-up count badges,
   drag reparent/reorder, context-menu management. Built in two tasks
   (read-only, then management) on `collapsible`/`item`/`badge`/`context_menu`.
2. **Count stepper** — hover/focus-revealed `− n +`, click-to-type, keyboard ±
   on the focused row, commit-on-blur; optimistic update + undo toast. Composed
   from `button` + `input`.
3. **Selection tray** — docked thumbnail stack + count + "Move to…" + clear;
   cross-view selection state. (Registry `action_bar` was evaluated and ruled
   out in the gap analysis.)

### Conventions established by this spec

**Thin server-fn adapters.** No per-op Leptos server fns exist (collection-api
deferred them here). Each page task adds only the adapters it needs, as thin
projections of trait methods — no business logic. Exemplar shape:

```rust
#[server]
pub async fn search_catalog(q: String, cursor: Option<String>) -> Result<SearchResults, ServerFnError> {
    let headers: http::HeaderMap = extract().await?;
    let backend = server_backend(&headers); // anonymous or session-scoped
    backend.search(SearchQuery { q: Some(q) }, Page { cursor, limit: None })
        .await
        .map_err(api_error_to_server_fn)
}
```

**`/my/*` auth guard.** No client-side guard exists today. Pattern: a shared
wrapper component holding a `Resource` on `fetch_current_user`; anon →
`use_navigate` to `/login?next=<current>`; `/login` honors `next` after
sign-in. Server fns underneath still enforce auth independently
(`user_id_from_headers`) — the guard is UX, not security.

**Tokens, not hex.** All new UI uses the theme-token utilities
(`bg-background`, `text-muted-foreground`, …). A `.dark` block is added to
`style/input.css` (OKLCH values for every token) and the existing hardcoded-hex
pages (`HomePage` remnants, `auth_pages.rs`) migrate. Theme class rides `<html>`
with persistence (model per Open question below). The counter and its
`get_count`/`increment_count` server fns + `storage` module are deleted with the
shell task.

**Vendoring.** Via the `vendor-component` skill: bench section in the same
commit, runtime verification checklist including native webviews. The six
interactive components (dialog/popover/command/hover_card/sheet/sonner) are
"vendor markup + CSS, rewire behavior in Leptos" — deterministic caller-supplied
IDs (no `use_random_id`), Leptos-owned open state, verified CSS-anchor
positioning fallback for WKWebView / Android WebView. `command` is the shared
core of quick-add, destination picker, and ⌘K — its reactive rewiring happens
once, in the vendoring batch, not per-feature.

### Known defect folded in

**Multi-face card images**: `HostedBackend` projections read
`image_uris->>'normal'`, which is NULL for `transform`-layout printings
(Scryfall nests `image_uris` per face) — blank tiles today. Fix in the
card-detail task: `COALESCE(p.image_uris->>'normal', p.faces->0->'image_uris'->>'normal')`
in the summary/detail projections. Images hotlink Scryfall's CDN (policy-fine
at this scale); no image pipeline this phase.

## Open questions

None — all resolved at spec review (maintainer, 2026-07-17):

- **Theme persistence** — **dark mode is the default**; an explicit toggle
  override is persisted as a saved user preference. The dark-palette task wires
  accordingly (default `dark` class on `<html>`, override stored and re-applied
  SSR-safely).
- **Sonner engine** — **small native Leptos toaster**, not upstream's vendored
  JS engine (undo-on-toast wants first-class Leptos state; upstream's own
  `_sonner_leptos_only_later/` points the same way). Accepted deviation from the
  vendor-as-is convention.
- **POC catalog** — **deferred, confirmed**. Quick-add disambiguation realism
  and list performance are explicitly not goals of this phase; the phase's goal
  is validating the infrastructure already built (API, ingestion, design system,
  auth, search). Data-scale issues are addressed after the full ingest (parked
  task + Later/parked note in TODO.md).

## Findings

(appended per task by the work loop — decisions, surprises, disputed review
findings with rationale, deferred items)

### Batch move (2026-07-25)

`app/src/my/move_selection.rs` — the tray's "Move to…" wired to a real write.
Second of the three split move-flow tasks.

**The generalizable rule, and the round's one major:** *anything deciding
whether a write is possible cannot use a read model that groups away the
write's addressing.*

The collection view's `present` CTE is `GROUP BY printing_id, board`, so finish,
condition and language are collapsed; a row holding only foils reports
`present = 3` and gets a checkbox. `holding_take` matches the **full** grain
plus `board = 'main'`. Every `MoveItem` was built at the default grain and
nothing checked that the source actually held it, so such a row returned
`Conflict("no copies to move")` — and because `move_batch` is one transaction,
that rolled back the *entire* batch behind an error naming no card. A 20-card
batch died to one entry, diagnosable only by bisecting the selection.

Reachable on real data: Trade Binder's `Aatchik, Emerald Radian` is foil-only
(qty 3) and renders as an ordinary selectable row. The same blindness hit
boards on the `/my` path — `CardDetail::ownership` groups by
`(collection_id, printing_id)`, so `resolve_card` saw neither grain nor board,
making the module's own "boards are refused, not silently mainboarded" true
only for `SelectionKey::Held`.

The implementer filed this itself as a follow-up ("loud, never wrong — but
annoying"); the orchestrator **promoted it to major**, on the grounds that it is
loud but *unattributable*, fires on data present today, and breaks the feature's
primary path. That call was accepted on re-verification: "never wrong" held,
"loud" did not survive the fact that the failure isn't scoped to the bad entry.

**The fix refuses; it does not widen the write.** New read
`CollectionStore::holdings_of_oracle -> Vec<HoldingLine>` (trait + hosted +
native + `GET /api/cards/{id}/holdings`) returns holdings **ungrouped**, and
resolution became two pure functions over `&[HoldingLine]` that decide
*movable* — mainboard, default grain, quantity > 0 — before any `MoveItem`
exists. It also superseded the `card_detail` call resolution previously made,
so the path got cheaper while getting correct. New per-entry refusals `Grain`,
`Board` (now reachable on the `/my` path) and `NoCopies` (the stale-tray gap,
previously a batch abort) are named in the toast and stay checked in the tray,
so the rest of the batch still moves. `MoveItem`, `CardRow`, `holding_take` and
the ledger are untouched — the third task's fence held.

**The residual TOCTOU case is deliberate.** If another tab moves the copies
between the resolution read and `move_batch`, `holding_take` still returns
`Conflict` and still rolls back with an unattributable error. Pre-judging it is
impossible from outside the write transaction; closing it means a grain-aware
batch write, which is the third task. Mitigations: the window shrank from
"however long the tray sat open" to microseconds inside one server fn, the
pre-judgeable stale case now refuses as `NoCopies`, and the toast says nothing
was moved so the intact selection can be retried.

**Other decisions.** `move_batch` (already trait/hosted/native/route-complete)
was used rather than a new `move_cards` path — it is collection-api's actual
"Move (batch)" and is what makes the write atomic. A true batch undo needed a
new `undo_moves(Vec<Id>)` trait method: the ledger has no batch id, so N writes
mean N `moves` rows, and looping the per-move undo would be N transactions with
a partial-revert failure mode — the exact defect shape this repo already hit
(committing 0 ran `DELETE FROM holdings` behind a success toast). Quantity is
server-fixed at one copy per entry, and the toast says "(1 copy each)", because
the tray's pill counts entries and one copy each is the only quantity it does
not lie about. The destination picker was *extracted* for sharing
(`DestinationList`/`DestinationOption`/`DestinationChoice`), not forked.
`HoldingsRevision` is provided by the shell and consumed as a resource *source*
by both holdings-rendering pages, so a move made from the shell invalidates the
page it affected by construction.

**Authorization was checked and is sound.** `moves`, `holdings` and
`collections` all carry `ENABLE` + `FORCE ROW LEVEL SECURITY` keyed on
`app.user_id`; `scoped_tx` binds that GUC transaction-locally; `undo_one`'s
lookup therefore returns no row for another user's move id. The
client-supplied `oracle_id` on `SelectionItem` is safe by construction, not by
trust: it only selects which of the caller's own RLS-scoped holdings to read,
and every path re-checks the named collection/printing/board, so a wrong oracle
yields `NoCopies` rather than a write elsewhere.

**A fixture that cannot express a distinction cannot test code that depends on
it.** The grain path was untestable across the whole suite by construction:
`addHave` posted only `{printing_id, quantity}`, so every fixture holding was
`nonfoil/nm/en/main`. `addHave` now takes a grain. The fix is mutation-verified
— reverting `movable` to the pre-fix `quantity > 0` fails both new tests, the
foil one with the whole-batch death that was originally reported.

**Evidence.** `cargo test --workspace --exclude frontend` 136 + 26 green; full
chromium e2e **143/143** at `--workers=1` (3.8 min); hydration CLEAN; Android
webview 16/16. The reviewer's exact row driven through the real UI: rendered
`present=3 board=main` with a checkbox, refused by name, tray retains it,
Trade Binder still 3, destination still 0 rows.

### Selection tray, read-only (2026-07-25)

`app/src/components/ui/selection_tray.rs` — custom gap component №3, built on
the `count_stepper.rs` precedent (a custom component, not a vendored port, but
carrying a bench section like any other). First of three split tasks; the batch
move and the undoable removal follow it.

**The tray is the pill; the shell owns the dock.** `SelectionTray` renders the
wireframe's pill (thumbnail stack capped at 3, count, inert "Move to…", clear)
and nothing at all at zero selection — `Show`, so it is absent from the DOM
rather than transparent. `shell::SelectionTrayDock` is the fixed positioning
(`bottom-16 md:bottom-0`, above the mobile tab bar). That split mirrors the
wireframe's own Tray Wrap / Selection Tray frames and is what lets the bench
render the pill inline.

**The selection key is an enum, and that is the load-bearing decision:**

```rust
Held { collection_id, printing_id, board }   // /my/collections/:id rows
Card { oracle_id }                            // /my rows
```

A `/my` row genuinely cannot be `Held`-shaped: it aggregates every collection
per *oracle* card, and `CardSummary::printing_id` is the has-art-first
**representative** printing, which the user may own zero of. So neither "from
where" nor "which printing" is answerable from that row.

The enum was chosen over `from_collection_id: Option<Id>` deliberately. `None`
in `shared::MoveItem` means *external intake* — copies appearing from outside
the system. A struct-with-`Option` key would let the batch-move task pipe `None`
straight through and silently conjure copies; the enum makes that a compile
error and forces an explicit resolution step. **The batch-move task inherits
that resolution: it cannot pipe a `/my` selection into `move_cards`.**

`board` rides in the key even though `move_cards`/`holding_take` are hardcoded
to `board = 'main'`, because a deck's mainboard and sideboard rows for the same
printing are two rows on screen and must be two checkboxes or one lies about the
other. The move that consumes it cannot honor board yet — same constraint
already recorded at the `CountStepper` call site.

Selectability is `present > 0` on collection rows and `owned > 0` on `/my`;
desire-only rows render no checkbox, since offering to move copies that do not
exist is a lying control. Review confirmed this matches what the rows render:
`CardRow::present` counts *this collection's* holdings only and `present_rollup`
is a separate column, so a rollup-only row correctly gets the dimmed `+n` and no
checkbox.

**Cross-view survival was verified structurally, not just behaviorally.**
`provide_selection()` is called exactly once, in `AppShell`, which is a single
`ParentRoute` view above `/catalog`, `/cards/:id` and the whole `/my` subtree —
so no navigation among them re-runs it, including the `/my/collections/:id`
route's ~800 ms DOM detach/re-attach. `Checkbox` is fully controlled, so a
re-mounted row re-derives its checked state from the shell signal. Sign-out is a
`hard_navigate`, so a selection cannot outlive a session. The selection is
in-memory only: it survives every SPA navigation and mode switch but not a
document load, which the spec does not require.

**Review: CLEAN — zero majors.** Nine minors filed under Phase 5 discoveries.
Three were checked by the orchestrator rather than accepted on the reviewer's
severity call, since the major/minor line decides whether work happens now:

- *16×16 px checkbox tap target on mobile* — confirmed real (`size-4` on the
  control, padding on the `<td>`, adjacent to the card-detail link) and the
  closest to promotion. Held at minor because a mis-tap lands somewhere
  recoverable and the selection survives the navigation.
- *A toast covers the tray's clear "×"* — geometry confirmed (sonner `z-[200]`
  at `bottom-6 right-6` vs the dock's `z-50`; ~44 px overlap at 1440 px). Held
  at minor: transient, and rows can still be unchecked.
- *The count reads entries, not copies* — "1 card" for a selected row holding 4.
  Internally consistent today (one entry, one thumbnail); filed because it is
  the minor that most constrains the batch-move task's quantity semantics.

**A reported test-infrastructure failure did not reproduce.** The implementer
reported `smoke.spec.ts:92` timing out reproducibly at the default worker count
once the seven new tests were added, passing only at `--workers=3`, and proposed
either lowering the suite's local worker count or rewriting that test's
`waitForURL`. The task's single full chromium run went **138/138 in 52.3 s at
the default workers** (`workers: undefined` locally = half the cores), with that
exact test green. No config change made and none warranted; treated as a
suspected load flake and filed as such. One green run does not prove it never
flakes — but it does refute "reproducible", which is what the proposed fix
rested on.

**Evidence.** `cargo test --workspace --exclude frontend` green; full chromium
e2e 138/138 (52.3 s); hydration CLEAN on four authed URLs; SSR curl non-empty on
`/my`, a collection, and `/dev/components`, with collection rows keying e.g.
`held:<collection>:<printing>:main`; Android webview 12/12 on the new
`probe:android-selection-tray` plus the baseline CDP check.

### Quick-add panel (2026-07-25)

`app/src/components/quick_add.rs` — the intake composite on
`/my/collections/:id`, wrapping the page's existing `QueryBar` in a `Command`
whose candidates come from `search_catalog` keyed on the committed `?q=`. The
keystroke contract is a pure `decode(key, shift, alt, rows, counting) -> Action`
(12 unit tests); `command` gained a `use_command_nav()` / `CommandNav` seam so a
*foreign* input can drive its item registry, `query_bar` gained `on_key` and
`reset` props, and `quick_add` gained a server-clamped `quantity`
(`clamp_quick_add_quantity`, `QUICK_ADD_MAX = 99`).

**Deviation: the panel is not the vendored `popover`,** though the task line
names it in the composite. Two measured browser behaviors defeat it for a
field-anchored surface on a page that navigates as you type: (a) `popover="auto"`
light-dismisses on the same `pointerup` that focuses the field — `showPopover`
returns `Ok`, then `toggle → closed`; (b) the router removes and re-inserts this
page's whole subtree on every `?q=` change, and removing a *showing* popover
hides it **without** firing `toggle` (the HTML removing steps pass
`fireEvents=false`), so the Rust `open` signal stayed `true` while the panel was
gone. An absolutely-positioned panel has neither problem; Escape and
outside-`pointerdown` are handled in the panel instead. The destination picker
keeps using `popover` (button trigger, no navigation) — this is not a retreat
from the primitive.

**The router churns this route's DOM.** Measured on `/my/collections/:id`:
~800 ms after each `?q=` navigation the page subtree is detached for ~400 ms and
re-attached (same nodes), blurring the field — without mitigation the keystroke
loop dies after the first card. `/catalog` and `/my` do **not** do this. The
panel holds the caret with a 120 ms interval gated on *panel open* and on focus
having landed on `<body>` rather than a real element. That is a workaround at one
call site; the cause is upstream and is filed.

**`command`'s ordering caveat needed no fix here** — the candidate list is
rebuilt inside a `Suspend` per query, so every result set is a full remount in
document order, which is the case the caveat explicitly exempts. The
`visible_ids` comment was rewritten per-consumer rather than sorted.

**`quick_add` taking a quantity** deliberately narrows the earlier "quantity 1 by
construction" finding: `⇧⏎ set count` is a shipped keystroke, and four separate
adds would need four undos. Clamped `1..=99` server-side; the UI caps entry at
two digits, so the clamp is unreachable by typing. Undo reverses the full move
quantity, so a playset undoes as a playset.

**`IN THIS COLLECTION` rows are context, not targets** — storyboard S2
pre-highlights the best *catalog* match with the present row above it
unhighlighted. They render as links to `/cards/:id`; adding more copies of a card
you already own goes through the catalog section.

**time-to-enter-50, recorded:** scripted run into a scratch binder, 60 ms per
keystroke, 6-character prefixes, `⏎` on the pre-highlighted match — **50 cards /
50 copies verified in the collection, 350 keystrokes (7.0 per card), 1 pointer
action for the whole session, 97.3 s (1.95 s/card)**. Typing is 0.36 s of that;
the remaining ~1.6 s/card is the 250 ms debounce plus the catalog and add round
trips.

**POC-catalog realism — confirmed deferred, now quantified.** Of 30 six-character
probes, one ("shock") returns *zero* matches in the ~3K-printing subset and
several return the 10-row page cap, so real ↓ disambiguation cost is unmeasurable
here. The metric run used verified prefixes and spent zero disambiguation
keystrokes; a full catalog will move that number.

**Deliberately deferred:** the storyboard's in-field `esc` chip (the footer
carries the contract and Escape works); the post-add per-row `✓ wanted 1`
confirmation (the toast covers it); set codes in the candidate meta line
(`CardSummary` carries no set — mana cost with a type-line fallback instead);
`aria-activedescendant` (`CommandItem` emits no ids). **No bench section for the
panel itself** — it is a Composite in the gap analysis, not one of the three
custom gap components, following `QueryBar`'s precedent; the *primitive* it
depends on got the bench coverage instead, which is what made the Android probe
possible at all.

**Review: one round, zero majors.** The reviewer verified all six flagged claims
end-to-end — the popover replacement's dismissal/escape/stacking/focus, the focus
keeper's lifecycle (cleared on unmount, no-ops when closed, cannot take focus
from a real element), the quantity clamp and undo path, the Want-vs-Have memo
sharing `add_default` with the page's own quick-action row so the two cannot
drift, `use_command_nav` being purely additive for the destination picker and
bench, and `decode` against the contract. Twelve minors were filed rather than
fixed, per the loop's calibration. Two were re-checked by the orchestrator before
accepting the severity: the stale-count-after-a-failed-add path renders a visible
`× 4` chip and any keystroke clears the count, and the Escape-with-zero-rows gap
retains outside-click dismissal — neither reaches the major bar.

**Verified:** merge gate all 8 steps green (135 tests); hydration CLEAN on 3 anon
+ 5 authed URLs; bench probe CLEAN including the new foreign-nav assertions; SSR
curls on binder (`Adding here: ⏎ Have`) and deck (`Adding here: ⏎ Want`), both
with a server-side panel count of 0; full chromium tier **131/131** (8 new, 0
regressions); Android CDP probe PASS and `android-quick-add-check` CLEAN on the
live webview.

### `/my/collections/:id` binder/deck view (2026-07-25)

`app/src/my/collection.rs` — the binder and its deck variant on one page, over a
reworked `CollectionStore::collection_view`. Two adversarial review rounds
(8 findings then 6): round 1's fixes are `85ac7cc`, and round 2's single
code-level fix rides in this task's finishing commit. Round 2's remaining
findings were parked as Phase 5 discoveries by maintainer decision — the loop
had already spent ~2.5 h on this task and round 2 had drifted from correctness
to craft (see "Loop cost" below).

**`collection_view` returned no desire-only rows.** It inner-joined `holdings`,
so a card a deck *wants* and does not hold did not exist in the view at all —
while the needs chip in the same header counted it. The same `FULL OUTER JOIN`
correction `/my`'s `all_cards` needed; desire-only rows borrow the
representative printing. `CardRow` also gained `holding_id` (the stepper's write
target) and `faces` (so a collection row reuses `CardPreview` — hover card,
touch sheet, DFC flip), and `collection_view` gained `q`.

**A committed HERE of 0 is a deletion this view cannot undo — the blocker of
round 1.** The stepper offers Undo on every commit, but `set_holding_quantity(id, 0)`
runs `DELETE FROM holdings WHERE id = $1`, and the undo re-POSTs the now-dead
id: HTTP 500, `not found: holding`, copies gone behind a success toast. The
floor here is now `min=1`, which makes the destructive commit *unreachable*
rather than reachable-and-lying. That is a scope boundary, not a preference: an
undoable removal is a `move_cards(to = None)` addressed by **grain**, and
`CardRow` carries no finish/condition/language while `move_cards`/`holding_take`
are hardcoded to `board = 'main'` — a move-based write from a sideboard row
would silently hit the mainboard. **Consequence to carry forward: there is now
no way at all to remove a card from a binder in this view** (teardown is
deck-only, the spec's per-row move affordance is unshipped). Filed as a Phase 5
discovery for the move-flows task.

**An optimistic delta must be scoped to the payload it corrects, not the URL.**
`here_delta` keyed on the URL survived a same-URL `refetch()` — teardown left the
header reading `1 here` on an emptied deck — and was cleared too early on
navigation. Gating the reset on the resource having *landed* fixes both.
Round 2 refuted the *rationale* recorded for it, which is worth keeping: in
Leptos 0.8 `ArcAsyncDerived` never clears `value` on re-run, so `.get()` returns
`Some(old)` throughout a refetch, and `<Transition>` renders **nothing** during a
re-fetch rather than the stale payload. The guard is therefore inert as written —
removing it survives the spec — but the commit-during-in-flight-fetch window it
was reaching for is unreachable anyway, because nothing is on screen to click.

**A tree refetch was remounting the stepper rows and silently disarming Undo.**
Both page blocks awaited the shell's `CollectionTreeResource`; a stepper commit
refetches it for the sidebar badges, re-running the whole `Suspend` and
re-seeding each `CountStepper` from the stale fetched count, so `cur != from` was
false and Undo did nothing while the header delta stayed applied. Rule:
**nothing large awaits the tree** — breadcrumb, folder counts and teardown
destinations each await it in their own nested `Suspense`. Kill-verified.

**Three render rules were unfalsifiable against the fixture, not
under-asserted.** No seeded collection had `present_rollup > 0`, a nested folder
with children, or a card held under two printings, so mutations to
`rolled_up_of`, `owned_cell` and `show_wanted` survived the whole e2e suite
(`cargo test` caught them). Fixed in `app/src/seed.rs` with the
`Depth Box → Depth Shelf → Depth Drawer` block, plus tests that *guard* the
shape so a re-seed cannot quietly remove it.

**An `overflow-auto` wrapper hides overflow from a document-level assertion.**
The mobile no-horizontal-overflow test measured `document.documentElement`, but
`TableWrapper` is `overflow-auto`, so table overflow is a wrapper-local scroll
the document never sees: stripping the progressive-column classes produced
92–128px of sideways scroll at 390×844 and the test stayed green. Measure the
scroll container — the corrected test asserts on the `data-name="TableWrapper"`
ancestor's `scrollWidth − clientWidth` and keeps the document check as a cheaper
second net. **Caveat, recorded deliberately:** the corrected test was confirmed
passing on unmutated source, but its kill verification (re-applying the
class-stripping mutation and watching it fail) was **cut short by maintainer
decision** to stop the loop. The fix is reason-verified, not kill-verified.

**The needs chip is not deck-only — round 1 finding disputed and upheld.** The
code renders it on any collection with `missing > 0`;
`design/information-architecture.md:41` puts the chip on "a deck **or
collection** header", and this spec distills that document and never overrides
it. The spec's own deck-variant bullet was the line that read wrong and has been
corrected above. The test weakness behind the finding was real and is fixed
(`Depth Box` is a binder with wants, so the chip's presence is now pinned there;
the absence test was retargeted at genuinely deck-only elements).

**Other decisions.** `min(uuid)` avoided — `holding_id` is
`(array_agg(id))[1]` under `CASE WHEN count(*) = 1`, so it is NULL exactly when
the cell sums several finish/condition/language grains, and the stepper is
withheld rather than writing to an arbitrary holding. Deck slot counts and the
WANTED column dedupe on `(oracle, board)` because `desired` is oracle-grained and
repeats across printings. OWNED collapses against `present + present_rollup`, not
`present`. Folder rows take identity from `view.children` and counts from the
tree, so a folder badge and the sidebar badge cannot disagree. `commanders_in`
was extracted so the deck header and the standalone read cannot drift — which
cost the commander e2e its independence, since both sides became the same query;
the witness is now `card_tags` via `/tags` → `/tags/{id}/cards`.

**Playwright treats `aria-disabled="true"` as not-enabled** and will hang on
`.click()`. Useful: the refusal is itself evidence the control is announced
inert, and `{ force: true }` then proves it does nothing.

**A `<Suspense>` fallback inside a `<select>` is a second `<option value="">`**
and made a strict-mode locator ambiguous. The teardown dialog also re-introduced
the read-in-render hydration panic (`tachys` "expected a marker node") — the same
trap the cross-task audit recorded — fixed by mounting it only for decks inside
the resolved header and building its options inside a `Suspend`.

**Loop cost, recorded deliberately.** Implementation 60 min / 454k tokens,
review 1 30 min / 198k, fix round 26 min / 523k, review 2 36 min / 269k —
~2.5 h and 1.44M subagent tokens for one task. Drivers: the full three-browser
tier ran ~6 times (it grows every task — 196 → 355 → 367 tests) because
implementer *and* reviewers each ran it; the mutation pass is uncapped at ~10–12
mutations, each costing a rebuild cycle; and this task line bundles three
surfaces. Round 2's findings had already drifted from correctness to craft. The
loop needs a stopping rule — see ui-work-loop.

**On-device coverage is the anonymous half again**, by the fixed matrix — the dev
proxy strips Cookie headers, so the table is unreachable; the probe covers the new
route's guard bounce through the redirect-swallowing shim plus the
stepper/breadcrumb on the bench.

**Deliberately not absorbed.** The quick-search box filters but does not yet
inline-add catalog matches (the quick-add panel task owns that; `add_default(kind)`
is exported for it), and per-row move/select affordances belong to the
selection-tray task.

### `/my` All cards view (2026-07-24)

`app/src/my/all_cards.rs` — the everything-view, over a reworked
`CollectionStore::all_cards`.

**The row is a catalog row, by construction.** "Same row treatment as
collection view" is enforced by rendering the *same DTO*: `AllCardsRow` now
carries a whole `CardSummary`, so the name cell reuses `CardPreview` (hover
card, touch sheet, DFC flip) and the SQL reuses `summary_select()` — which
grew a `summary_select_with(extra_cols)` sibling so a projection can add its
own columns without re-listing the shared ones. Two fields were *removed*
rather than kept: `owned` is `card.owned` and `in_collections` is
`locations.len()`, both derived by `impl AllCardsRow`, because a stored copy of
either can disagree with the list it summarizes. `NeedLocation` was renamed
`CardLocation` — the needs view and this one wanted the identical shape.

**The everything-view includes cards you only *want*.** `all_cards` had been an
inner join on `holdings`, so a card desired-but-held-nowhere did not exist in
it — invisible in `/my` while sitting on the shopping list. The aggregate is now
`held FULL OUTER JOIN wanted`, and that row renders owned `—` / no locations /
WANTED n. The e2e asserts the row's *existence*, not just its numbers, since
that is the regression shape.

**Quick search is deliberately not the catalog grammar.** `/my`'s box filters a
list you already own, so it is a plain name substring — but through
`crate::search::sql::pattern` (now `pub(crate)`), so a typed `%` is literal in
both places. Asserted on-page: `?q=%` matches nothing.

**HERE → WHERE, in three shapes.** The spec's `7 across 3 collections` phrasing
only works in the plural, so: no locations → `—` with no control; exactly one →
`3 in Trade Binder`, linked (a disclosure would expand to the sentence it is
already showing); several → the spec's summary, expandable. Recorded as a
refinement, not a deviation.

**A `collapsible`'s padding leaks when closed.** `CollapsibleContent`'s `class`
lands on the *inner* div, and `min-h-0` zeroes its content box but not its
padding, so `class="pt-1"` left ~4 px of open track under every collapsed row.
Spacing belongs on the child. Worth knowing for the next consumer — the vendored
component's prop reads like it is inside the clipped region, and it is, but not
below zero.

**`/catalog`'s query bar became a shared component.** `app/src/components/
query_bar.rs`, used by both pages. The debounce, the `self_pushed` URL⇄field
guard, the per-search-session history granularity and the timer cleanup are four
separately-earned behaviors; copying them into `/my` would have meant
re-earning them. It is an app-level composite over vendored primitives, not a
registry component, so it carries no bench section — its behavior is asserted by
the `/catalog` and `/my` specs and by the on-device probe. Note the view-macro
trap it re-triggered: the prop immediately before `{..}` must not end in a bare
path (`placeholder=placeholder {..}` parses as struct-update and the spread
vanishes, reported as "no field `aria`" on the props builder).

**Keyset paging is forward-only.** A cursor describes "everything after this
row", so Previous would need a reverse-ordered query and a `before` cursor.
Browser Back already walks the pages you came through (each Next is a real
history entry); the pager adds "Back to the start". Editing the search drops the
cursor — a new filter has no page two yet.

**The dev seed grew a bulk box, because the fixture could not reach page two.**
`/my` asks for 50 rows and the seed held ~19 distinct cards, so the "Next page"
link was unreachable by *any* browser test — the Codex review's second finding,
and correct: every paging assertion was deep-linking a cursor obtained from the
JSON route, so `Pager`'s href could have pointed anywhere. `app/src/seed.rs` now
has a second, independently-sentinelled block (`BULK` / `BULK_CARDS = 60`) that
an already-seeded user picks up on a plain re-run. It **skips whatever is
currently short**: filling it blindly owned the two cards the tree deliberately
wants and holds nowhere, and the whole "short → shopping list" leg of the
fixture evaporated on the first run. `/my/collections/:id` will want the same
headroom.

**Codex adversarial review: zero findings in the feature code** (all six focus
areas explicitly clear), three in the tests, all three accepted:

1. *WANTED was verified against itself* — the expectation came from the same
   projection the page renders, so a regression that summed wrong would agree
   with itself. Now cross-checked against `/api/shopping-list`, which computes
   the same cross-collection desire total through its own CTEs. Note the trap
   found while wiring it: "on the shopping list" means `desired > owned`, **not**
   "held nowhere" — the held-nowhere half filters on `owned === 0`.
2. *The Next-page link was never clicked* — fixed by the seed change above; the
   spec now clicks it and asserts the URL, the rows, and the way home.
3. *The paging probe was unregistered* — no npm script, no doc reference, so a
   duplicate/skip/cursor regression could pass the suite unless someone
   remembered the command. `end2end/package.json` now carries `probe:*` scripts
   for every probe a task may need, and the e2e-suite skill lists them with the
   rule that an unregistered probe is one nobody runs.

**Keyset paging is proved by a probe, not by the browser tier.** Page size is
fixed in the UI, so only the JSON route can ask for a page small enough to
iterate. `end2end/all-cards-paging-check.mjs` walks the whole set at limit 3 and
7 and asserts no duplicate, no skipped row, order stable across boundaries,
exactly one terminal cursor, and that a filter survives paging. Its reference
read is `limit=200` (`Page::limit`'s clamp) *and asserts it came back terminal* —
the page's own adapter cannot serve as the reference, because its 50-row default
is itself a page now.

**The e2e reads rows another spec writes.** `destination-picker.spec.ts` fires
real `+ Have`/`+ Want` at the same dev user in a parallel worker, and `/my`
aggregates every collection, so an API snapshot and the render compared against
it can straddle a concurrent write. It surfaced as a firefox-only failure on the
first full-tier run and is not browser-specific at all. Every API-cross-checked
assertion now re-reads *and* re-renders inside `expect(...).toPass()`: a real bug
fails every attempt, a mutation that lands mid-attempt is gone by the next. This
is the "concurrent runs racing the seeded e2e user" hazard the Playwright-in-CI
decision names, met inside a single run.

**On-device coverage is the anonymous half, and it is the right half.** `/my` is
authed and the dev proxy strips Cookie headers, so the table is unreachable on
the emulator. What `end2end/android-all-cards-check.mjs` does cover is the
shared `QueryBar` (typing → debounce → navigation → field survives its own
commit → clear → Back re-seeds) and the anonymous `/my` → `/login?next=/my`
bounce, which on this platform goes through the `data-ssr-path` shim rather than
a browser-followed 302. 10/10 on Chrome 145.

**Mutation pass: 4 analyzed, 2 real gaps closed and kill-verified, 1 already
covered elsewhere, 1 documented as residual.**

- *`take(1)` in `CardsTable` survived the SSR test* — `toContain('…all-cards-row')`
  plus the first card's name cannot tell one row from fifty. The test now
  extracts every `data-oracle` from the raw HTML and compares the **list** to
  the API's. Mutation applied, test failed, reverted.
- *`rows.truncate(limit - 1)` survived every paging assertion* — the page and the
  test's oracle both read the same backend, so they agree on a 49-row page and
  walk the whole set consistently. Fixed in the probe, which now asserts each
  non-terminal page holds exactly the number of rows it **asked for** — the one
  expectation nothing server-side computes. Mutation applied, probe failed 33
  checks, reverted. (It also made the probe throw on `cursor=null`; guarded, so
  a broken server produces findings rather than a stack trace.)
- *`0 AS wanted` survived "three columns agree"* — true, and the reason is worth
  naming: that test's oracle **is** the projection under test, so it can only
  prove projection-to-DOM fidelity, never aggregate correctness. The suite does
  kill this mutation, via the shopping-list cross-check added for the earlier
  review finding.
- **Residual, queued:** no fixture card is desired in *two* collections, so
  "WANTED is a sum" and "WANTED is a max" are indistinguishable against this
  seed. The SQL is a plain `sum(quantity) GROUP BY oracle_id` and the
  cross-check catches a dropped/zeroed total, but the sum-vs-max distinction is
  currently unobservable. Closing it needs a third seed block wanting one card
  from two collections — filed as a follow-up rather than a third re-seed inside
  this task.

**Anonymous `/my` answers 200, not 302, to a request without `Accept: text/html`.**
Noticed while curl-probing and chased rather than assumed: `leptos_axum`'s
redirect only applies on the HTML render path, so a bare `curl` (which sends
`Accept: */*`) gets the page and a browser gets the 302. Not a regression — `/`
behaves the same way and always has. Probe `/my` with a browser-like `Accept`
header or the result means nothing.

### Count stepper (custom gap component) (2026-07-23)

`app/src/components/ui/count_stepper.rs` — custom gap component №2 (the first
custom one built; collection-tree was the other, across two tasks). Composes
the vendored `Button` + `Input`; the interaction logic is the work. Bench
section in `app/src/bench/count_stepper.rs` (a happy-path stepper and a
failing-save stepper exercising the caller-revert contract).

**Contract.** `value: RwSignal<i32>` is caller-owned; the stepper writes each
commit into it optimistically and fires `on_commit(StepperCommit { from, to })`
**after** the write. The caller owns persistence — on failure it sets `value`
back and toasts (the bench's failing stepper demonstrates this). The stepper
mounts no `Toaster`; it `expect_context`s one, so a host must provide it.

**One editing session, one commit.** ± steps and typed edits accumulate in a
`pending`/`text` session shown immediately; the session commits **once** — on
blur out of the stepper, or ⏎. ⎋ cancels. This is the "commit-on-blur" the
collection-view spec wants, not per-keystroke writes.

**Three engine/lifecycle traps hit while building:**
- *The blur-commit must be deferred.* The display⇄edit element swap unmounts a
  *focused* node, which fires a `focusout` with no `relatedTarget` — read
  synchronously that's indistinguishable from focus leaving the stepper, so an
  immediate commit closes edit mode the instant a click opens it. Fix: a
  `focusout` that looks like an exit *schedules* the commit (0ms macrotask);
  it commits only if focus genuinely ended up outside. Removing the defer
  breaks click-to-type entirely — mutation-verified (the input never stays
  open; the "commits on Enter" test fails).
- *WebKit doesn't focus a `<button>` on click.* The ± buttons `preventDefault`
  their `pointerdown` (no focus steal) and, when nothing inside holds focus
  yet, programmatically focus the count element so blur-commit has an anchor in
  every engine. Verified across the full three-browser tier (webkit = the
  WKWebView proxy) and on the real Android webview.
- *Built on the vendored `Input`, not a raw `<input>`.* Per the queue note and
  the `bind:value`-SSR-seed finding: `Input` seeds the `value` *attribute* from
  the bound signal (PR #43). The stepper's edit field is only ever mounted
  client-side (never SSR'd, since editing starts false), so the SSR-empty trap
  doesn't strictly bite here — but using `Input` keeps the seed and the e2e
  asserts the `value` attribute (not just the property) so a regression to a
  bare element is caught.

**Codex adversarial review (step 2): 4 of 6 accepted and fixed, 2 addressed in
e2e.**
1. (high) The deferred blur callback fires from a raw timer that outlives the
   reactive owner; if a parent unmounts the row (a list refetch) first,
   `commit_session` read disposed signals. **Fixed** — guarded with
   `try_get_untracked` on entry; bail if disposed.
2. (high) `label` (component-owned) was read *after* `on_commit`; a caller that
   removes the row synchronously on a committed 0 (the component itself defines
   0 as deletion) disposes `label` first. **Fixed** — read `label` / build the
   toast message before `on_commit`, and fire `on_commit` last (after the
   optimistic write and the toast).
3. (med) The edit-mode number input carried `min` but not `max`, so native
   ArrowUp overshot the upper bound until commit clamped. **Fixed** — `max`
   added via the attribute spread (`Input`'s `max` prop can't take an `Option`).
4. (med) The ± buttons announced `aria-disabled` at a bound but stayed live,
   opening a same-value pending session on click. **Fixed** — `step()` no-ops
   when the clamp doesn't change the value, so the click is genuinely inert.
5. (med) bench-check's SSR marker check couldn't catch a dropped `Input` seed.
   **Addressed** — the e2e/bench now assert the mounted input's `value`
   *attribute*.
6. (med) bench-check is Chromium-only and never blurred an active edit input to
   an external target. **Addressed** — the Playwright spec adds a
   blur-to-external-target commit case and runs the **full three-browser tier**
   (webkit).

**Codex e2e mutation pass (step 5): most assertions solid; two real gaps
strengthened, one documented.**
- *Commit cardinality* (items 2, 12): the "commit once" tests recorded only the
  *last* event, so a duplicate identical `on_commit(3→5)` was undetectable.
  **Strengthened** — the bench harness exposes a commit *count*
  (`bench-stepper-count`); the spec + bench-check assert exactly one commit per
  session (and that Undo is the second). Mutation-verified: duplicating the
  `on_commit.run` call now fails the count assertion.
- *Optimistic-first on failed save* (item 17): bench-check checked only the
  eventual reverted value, which passes even if `value.set(to)` were skipped.
  **Strengthened** — it now asserts the optimistic value appears *before* the
  simulated rejection lands.
- *Min-bound "opens no dead session"* (items 8, 16): **documented, not further
  instrumented.** A dead `pending = Some(min)` session at the bound is
  unobservable through commits (it never commits either way, since `to == from`)
  and has no user-visible effect. The meaningful contract — the click causes no
  value change and no commit, and the control announces `aria-disabled` — is
  asserted; distinguishing the harmless dead session would need bench-only
  session-state instrumentation not worth its weight.

**Platform verification.** Web: hydration CLEAN on `/`, `/login`, `/catalog`,
`/dev/components` (anon) and `/catalog`, `/my` (authed); bench-check CLEAN with
the new stepper assertions. Android: dev-attach over CDP, `android-stepper-check.mjs`
PASS on Chrome 145 (hover-reveal, accumulate+blur commit, undo, click-to-type
seed, Enter commit). Full three-browser e2e tier green (250/250; one pre-existing
drag-reorder flake in `collection-tree-manage.spec.ts` passed on the one-retry).

**Deferred / carried forward.** The stepper ships as a standalone bench
component; wiring it into the `/my/collections/:id` collection view (the real
HERE-column editor over `set_holding_quantity`) is that task's job — this task
built and proved the component in isolation, per the queue entry.

### Catalog page `/catalog` (2026-07-19)

`app/src/catalog.rs` — query bar, results grid/list, view switch, anonymous
quick actions; `search_catalog` server fn in `app/src/lib.rs`. The rail,
destination picker, and mobile filter sheet are their own queued tasks.

- **`search_catalog` is the adapter exemplar, and it is a `GET`**
  (`input = leptos::server_fn::codec::GetUrl`) rather than the server-fn POST
  default. Two reasons, both load-bearing: it's a pure read whose arguments
  belong in a cacheable URL, and the Tauri Android dev proxy strips POST
  bodies (ui-work-loop Findings), which would have made the on-device search
  unverifiable. **Verified on-device**: typed search returns results through
  the proxy. Later read adapters should copy this; write adapters can't.
- **Opportunistic auth reuses `routes::catalog_backend`** (promoted to
  `pub(crate)`) instead of re-deriving the rule. Two callers disagreeing about
  when a catalog read is session-scoped is exactly the drift the seam exists
  to stop. The `native` arm expresses the same rule via
  `NativeBackend::authed` with possibly-absent session material.
- **Stale-discard is the reactive layer's, not ours** — claim verified against
  the source, not assumed: `Resource` is an `ArcAsyncDerived`, which stamps
  each run with a monotonic version and drops a resolved future whose version
  is no longer latest (reactive_graph 0.2.14 `arc_async_derived.rs`,
  `if latest_version == this_version`). So catalog-search's "no stale results
  ever render over newer input" holds independent of the debounce. **Debounce
  closed at 250 ms as proposed** (catalog-search open question). What we do
  *not* have is cancellation: an overtaken request is discarded on arrival,
  never aborted — the debounce is what limits request volume.
- **History is per search session, not per keystroke.** Refining replaces,
  starting/ending a search pushes. Replace-always (the obvious reading of
  "one entry per session") was implemented first and caught in probing: Back
  from the first typed search walked straight off the site.
- **View mode rides `?view=list`.** It is not search state and never enters
  the query text, but keeping it in the URL makes it SSR correctly (no
  post-hydration flip) and survive reload/share.
- **Grid/list is a real `radiogroup` with roving focus** — the behavior V1's
  vendoring parked here. `toggle_group`'s hardcoded `tabindex="-1"` became a
  prop (deviation noted in-file); arrow-key selection + focus movement stays
  feature-side, with catalog.rs as the reference wiring and a new bench
  assertion pinning "exactly one tab stop, on the selected item".
- **Two Leptos traps, both cost a debug cycle.** (1) `{..}` spread after a
  path-valued prop (`bind_value=query_text {..}`) parses as *struct-update
  syntax* and the spread silently vanishes into the value — put a
  paren/string-terminated prop last. (2) Reading a signal in a `#[component]`
  **body** bakes the value in at construction; the layout switch stopped
  working until the `list_view` read moved into a closure. The regression
  test written minutes earlier is what caught it.
- **Result counts are `N` / `N+`, not a total.** The endpoint is keyset-paged
  and deliberately runs no `COUNT` (catalog-search), so the wireframe's
  "128 results" is not obtainable; "at least N" is the honest rendering.
  Paging beyond the first page (`?cursor=`) is **deferred** — filed as a
  follow-up task rather than absorbed.
- **Codex adversarial review — 2 fixed, 1 accepted-as-documented, 1 disputed:**
  - *Grammar errors blanked the result set* (high) — **confirmed by probe**
    (grid count 1 → 0 on `bolt pow>3`) and **fixed**: the last OK page is
    retained and rendered dimmed/inert under the error, since half-typed
    queries hit the term-naming 422 constantly. Regression test added.
  - *URL⇄field sync could clobber newer typing* (high) — **not reproducible**
    (the window is between `navigate()` and the effect flush, sub-millisecond;
    timed-keystroke probing at 240 ms spacing never hit it) but the mechanism
    is real: the effect had no way to tell our own navigation from an external
    one. **Fixed** by tracking the last self-pushed query and re-seeding the
    field only on external URL movement.
  - *No cancellation of in-flight requests* (medium) — **accepted, not fixed.**
    The "no stale render" guarantee holds via the version counter (Codex
    verified this rather than refuting it); true abort isn't exposed through
    the server-fn client. The overstated "one in-flight request" wording in
    our own doc comment was corrected instead.
  - *`ApiError::Validation` reaches the UI as HTTP 500, not 422* (medium) —
    **disputed.** This is the pre-existing, documented behavior of the shared
    `api_err` helper (lib.rs: "the transport channel carries the message;
    richer status semantics are collection-api's"). The status-correct channel
    is the JSON API (`ApiError::http_status`), which is what the native backend
    consumes; the Leptos server-fn channel is UI-internal and reads the
    message. Making it status-accurate means a custom error type across every
    server fn — filed as a follow-up task, not smuggled into this one.
- **E2E mutation pass — 11/11 kills**, and it found two tests that passed
  vacuously before it: the lazy-image assertion was wrapped in
  `if (await img.count())` (a page rendering no images at all would have
  skipped it), and "a signed-in visitor gets no sign-in prompts" would have
  passed if the quick actions were deleted outright. Both strengthened, plus
  the retained-results test now asserts the *actual cards* survive and a new
  test pins `&`/`+` (the characters a naive encoder splits or eats) through a
  full type→URL→reload round trip. Mutations confirmed killed: debounce
  250 ms→31 s, `replace` forced false, `last_good` never retained, lazy→eager,
  `authed` forced false, validation prefix renamed, encoder ignoring its
  input, focus index pinned to 0, view switch dropping `q`, clear not
  committing, and `url_q` ignoring `?q` — the last also kills the SSR test,
  which is what proves that test asserts *`q` drove the search* rather than
  merely that SSR happened.
- **Verification**: web SSR asserted at request level (rendered results in raw
  HTML for `?q=`), hydration CLEAN on 4 URLs (browse-all, search, list view,
  error), bench-check CLEAN, fast tier 25/25, Android debug webview 8/8
  on-device (incl. touch view-switch and zero horizontal overflow at phone
  width).
- **Known-cosmetic**: transform-layout printings still render the skeleton
  instead of an image (`image_uri` NULL until the card-detail task's
  `COALESCE` fix) — the tile degrades to skeleton + name rather than breaking.

### App shell + routing (2026-07-19)

The shell (`app/src/shell.rs`): top bar (brand, desktop mode switch, theme
toggle, user menu popover), sidebar rail frame, mobile bottom tabs, route
skeletons for the full map, `/` redirect, `/my/*` guard. Counter +
`get_count`/`increment_count`/`storage` deleted; `/cards` placeholder folded
into `/catalog` (keeps the seam-proving `catalog_count` read).

- **Server-side redirects are real 302s.** `/` and the `/my/*` leaves run
  `SsrMode::Async` so `leptos_axum::redirect` can still set status before
  streaming; `<Redirect/>` covers SPA navigation. Gotcha: leptos_axum only
  emits 302 when the request's `Accept` contains `text/html` — curl's default
  `*/*` gets 200 + `Location` + `serverfnredirect` headers instead. Probe with
  `-H "Accept: text/html"`.
- **Webview redirect-swallowing shim.** The Tauri Android webview fetches
  documents through an in-process proxy that follows 302s internally, so the
  webview receives the redirect *target's* HTML at the *original* URL —
  hydrating panics (router renders the URL's route against the target's DOM;
  reproduced on-device at `/` and `/my`). `shell()` stamps
  `data-ssr-path` on `<html>` (outside the hydrated root, like the theme
  class); `shell::hydrate_entry` compares pathnames before hydrating and
  `location.replace`s to the stamp on mismatch. Real browsers never mismatch.
- **One shared current-user resource** (`CurrentUserResource` in context) for
  redirect + guard + user menu. Consequence: auth transitions must be
  **full-page loads** (`redirect_browser`/`hard_navigate`), not SPA
  navigation — the login fixture caught sign-in dispatching on the stale
  anonymous resource and landing on /catalog. All five success paths
  (password, signup, reset, OTP, Google-Tauri poll) and sign-out do document
  loads now.
- **Guard reads location untracked.** A tracked read re-ran the guard's
  Suspend mid-redirect and compounded `next=/login%3Fnext%3D…` (found
  on-device, applies to web SPA nav too). E2E now pins the single bounce.
- **Codex review** (commit 4fb0241): 1 high — `next=/\evil.com` open
  redirect (browsers normalize `\`→`/`) — fixed by also rejecting `/\`;
  1 medium — Google sign-in loses `?next` (web OAuth callback 303s to `/`,
  Tauri poll hard-loads `/`) — deferred as a queue task, sign-in still lands
  correctly via the `/` redirect, just not on the guarded page; 6 explicit
  clean confirmations (route map, mode-switch/tabs breakpoints, counter
  deletion, stale-resource paths, hydration-safety conventions).
- **Codex e2e mutation pass**: 5 assertion strengthenings applied (exact
  `<h1>Catalog</h1>` SSR regex, post-settle `?next` stability re-assert on
  the SPA-bounce test, `aria-current` asserts on the mode switch, exact
  email in the user-menu assert, cookie-presence check on the saved
  storageState). All 10 tests then demonstrated kills in three transient
  mutation rounds: A (catalog h1, guard next-path) killed the SSR, guard
  and SPA-bounce tests; B (root-redirect target, mode-switch link,
  login-redirect hardcode, bottom-tab link) killed the 302, mode-switch,
  next-honoring and tabs tests; C (authed target, user-menu text, post-auth
  default) fails the login fixture itself, blocking the suite — no
  decorative tests. All mutations reverted; suite green after.
- **Android dev-proxy strips POST bodies and Cookie headers** (verified
  directly: argless server-fn POST → 200, form POST arrives with empty body →
  "missing field email", valid injected session cookies → `/api/me` 401).
  Authed flows are unverifiable over the dev-attach webview; on-device
  verification covered the anon surface (redirect, tabs, guard bounce, shim
  recovery — 5/5 PASS) and the authed surface is covered by the web tiers.
  Release-path auth is unproven → queue task before the phase-end smoke.
  Details in ui-work-loop Findings.

### Vendor batch V3 — command / hover_card / sonner (2026-07-19)

The interactive core of the app's central surfaces. All three carry
data-name markers and their bench sections in the same commit.

- **`command` fully reactive** (the gap analysis's headline rewrite): the
  parallel vanilla-JS keyboard+filter script — which fought the reactive path
  by *also* writing item visibility — is deleted. Filter is a per-item
  `Memo`; ↑↓/⏎ navigation is a Leptos **item registry** (each `CommandItem`
  registers on mount into a shared `RwSignal<Vec<ItemReg>>`, deregisters on
  cleanup), and `CommandInput` drives a `highlight` index over the *visible*
  subset. `CommandEmpty` reacts to "no item visible". This is the shape
  features extend with ⇧⏎/⌥⏎/count-entry by reading modifiers in their own
  handlers. `CommandDialog` wraps it in the vendored `dialog` (deterministic
  caller id, Leptos open state) — no inline script.
- **`hover_card`**: native Popover API + CSS anchor positioning kept;
  hover-intent is a cancelable Leptos `TimeoutHandle` (150 ms open on
  enter/focus, 150 ms close on leave/blur, cancel-close while over the
  content) — upstream's inline `<script>` gone. No JS position fallback (a
  hover preview is never the sole affordance; cosmetic if mispositioned).
- **`sonner` is a native Leptos toaster, not a vendored copy** (maintainer
  decision, Open questions): upstream's Rust side is markup that triggers a
  separate `sonner.js` engine we don't ship. Ours: a `Toaster` mounted once
  provides a `ToastHandle` via context; `handle.show(ToastOptions…)` fires
  programmatically with an optional **action button** (the undo-on-toast the
  move flow needs) and auto-dismiss. API shape follows the registry so
  callers read familiarly.
- Bench-check extended: command reactive filter + ↑↓/⏎ selection, toast fire
  + undo-dismiss, hover-intent open/close. Snag caught in-loop: the bench
  first rendered `CommandInput` without a `<Command>` ancestor →
  `expect_context` panicked in SSR and killed the server thread; wrapping in
  `<Command>` fixed it (a good reminder the context is mandatory).
- **Codex review** (9 findings) — sonner cleared entirely (native design
  accepted; auto-dismiss/keys/id-allocation all verified correct):
  - **hover_card trigger→content handoff broken** (#3, high) — trigger and
    content held *separate* `HoverTimer`s, so moving onto the card didn't
    cancel the trigger's pending close and it shut ~150 ms later. **Fixed**:
    one timer shared through the context; both endpoints cancel/reschedule
    the same handle. The bench now moves onto the content and asserts it
    stays open (was untested — finding #6), plus `on_cleanup` cancels a
    pending timer on unmount (#4).
  - **command highlight not clamped on shrink** (#2, medium) — a stale
    highlight above a shrunk visible set rendered no selection. **Fixed**:
    the highlight memo and Enter both clamp to the last visible row.
  - **command registry vs DOM order after in-place keyed reorder** (#1) —
    **documented as a bounded limitation** (module doc + here): all three
    consumers are append-only (static client-filtered lists; full remounts
    on new server results), none reorder persistent items in place, so the
    `compareDocumentPosition` sort is deferred until one does. Not a live
    bug for any current consumer.
  - Remaining bench-depth findings (#5, #7–#9) acknowledged; the `.mjs`
    probe stays diagnostic-grade (behavioral depth is the per-feature e2e's
    job) — the two that pointed at real defects (#6 handoff) are now
    covered.

### Vendor batch V2 — overlay foundations (2026-07-19)

`scroll_lock` + `dialog` + `popover` + `sheet`, markup/CSS vendored from
rust-ui@43e1e32, behavior rewired to Leptos (the gap analysis's
"vendor markup + CSS, rewire behavior" plan):

- **scroll_lock is the pure-Rust registry hook** (not the JS asset):
  hydrate-gated implementation + no-op SSR stubs, and the `window.ScrollLock`
  JS-interop registration dropped — no inline scripts remain to need the
  global. Let-chains rewritten for the 2021 edition.
- **Deterministic caller-supplied `id`s everywhere** (the `use_random_id`
  SSR-counter hydration bug from the gap analysis is structurally gone).
- **One `RwSignal<bool>` owns each overlay**: trigger/close/backdrop/ESC all
  drive it; callers can pass their own signal for programmatic open (the
  `m`-key move flow) — proven in the bench via a programmatic-open button.
  ESC listeners are `window_event_listener` handles removed on cleanup
  (upstream leaked per-instance `document` listeners). Closed panels get
  `inert` (upstream's closed overlays stayed keyboard-focusable —
  `pointer-events-none` only blocks the mouse).
- **Popover keeps the native Popover API + CSS anchor positioning** and
  gains two-way sync: signal→`showPopover`/`hidePopover` in an Effect,
  native `toggle` events→signal via `:popover-open` (DOM types through
  leptos's own web_sys re-export — compiles in every build). The
  close-on-CommandItem inline script is gone; compositions use
  `use_popover_open`.
- Sheet's open/closed transform is a reactive class (upstream mutated
  classList from its script); direction enum hand-written (no strum).
- **Anchor positioning verified on the Android webview on-device** (Chrome
  145, `CSS.supports` true, panel 9 px off the trigger). A **JS positioning
  fallback** lands anyway (spec requirement): `web_sys::css::supports` gates
  it; when anchors are absent the panel is fixed-positioned under the trigger
  (flipping above on viewport overflow). The installed WebKit build also
  reports support, so the fallback is defensive — exercised by construction,
  not observed firing.

**Codex review (9 findings + 1 extra) — a genuinely valuable pass, 8 fixed:**
- **Stacked-overlay ESC closing everything** (#1) + **ESC via
  stop_immediate_propagation**: new `overlay_stack` module (client-only,
  ssr-stub); each overlay pushes its id on open, and the ESC handler acts
  only when it's the topmost — *and* calls `stop_immediate_propagation` so a
  synchronous signal-flush of the stack can't let the next overlay's window
  listener fire on the same keypress. Proven: sheet+dialog open, one ESC
  closes only the dialog, a second closes the sheet.
- **Scroll lock not reference-counted** (#2) + **unlock-delay reopen race**
  (#3): `scroll_lock` gained an owner count + a generation counter. Stacked
  overlays share one lock (last-out unlocks); a delayed restore no-ops if the
  generation moved (close-then-reopen keeps the DOM locked) or an owner
  remains. Bench asserts body `overflow:hidden` engages on open, survives
  while a second overlay is open, and releases only when the last closes.
- **Popover JS fallback absent** (#4): added (above); `show_popover` failure
  now re-syncs the signal from the DOM instead of silently drifting.
- **Closed overlays keyboard-focusable** (folded into #8/aria): `inert` on
  closed dialog/sheet panels; `aria-label` prop added (Titles alone gave the
  overlays no accessible name — the extra finding).
- Bench (#5–#7, #9) strengthened: horizontal popover overlap check, the
  stacked-ESC scenario, scroll-lock body-style assertions, and a two-render
  ID-stability diff. **Disputed**: #8's "children always instantiated" is
  inherent to this always-mounted overlay pattern (upstream's too); revisited
  only if a specific overlay's content proves expensive — noted, not changed.

### Vendor batch V1 — static primitives (2026-07-19)

Eleven components vendored from rust-ui@43e1e32 (button, badge, input,
input_group, kbd, separator, checkbox, label, toggle_group, breadcrumb,
skeleton, card), each with a bench section in the same commit. Batch-wide
decisions (per-file details in each header):

- **`variants!` hand-expanded.** Upstream's 457-line `leptos_ui::variants!`
  macro (plus its `paste` dep and `TwClass`/`TwVariant` derives) is replaced
  with plain enums + `match` arms carrying identical class strings — zero new
  dependencies, and the token trap surfaces at review time instead of
  silently emitting no CSS.
- **Undefined-token variants dropped**: button `Warning`/`Success`/
  `Bordered`, badge `Success`/`Warning`/`Info` (they reference `warning`/
  `success`/`info`/`*-light`/`*-dark` tokens style/input.css doesn't define).
  Re-add variants together with their tokens if a screen needs them.
- **`void!` joined `clx!`** in the vendored clx.rs (same leptos_ui source).
- **Icons inlined** (checkbox check, breadcrumb chevron/ellipsis — Lucide
  paths, ISC) rather than adopting the registry's icons crate.
- **Upstream bug fixed as deviation**: label's runtime-formatted named-peer
  classes (`peer-disabled/{for}:…`) can never have CSS generated for them —
  replaced with the static `peer-disabled:` pair.
- **`strum` avoided** (input's type enum → hand-written `as_str`);
  **`InputGroupTextarea` dropped** (no textarea vendored, no wireframe use).
- Component attr pass-through convention: `attr:aria-label=…` etc. on the
  component tag (the `{..}` spread form mis-parses hyphenated attrs in this
  leptos version).
- Verified: bench-check extended (SSR marker per family + checkbox/
  toggle-group interaction + the html-level bench toggle) — CLEAN; fast tier
  4/4; **Android webview on-device** (all families render, checkbox
  interacts). ID stability N/A (no generated IDs); assets N/A (none
  referenced).
- **Codex review** (9 findings): 1–4 ("`attr:` on components can't compile /
  won't forward") **disputed with hard evidence** — both clippy halves green
  and all five spot-checked attributes (`href`, `data-slot`, `aria-label`,
  `role`, `aria-current`) present in the served SSR HTML; `attr:` on a
  component is Leptos 0.8's documented root-attribute pass-through, and
  upstream's own breadcrumb uses it on clx components. 5 **accepted**:
  `aria-checked` added to ToggleGroupItem (deviation noted in-file); roving
  focus/keyboard is feature-side, lands with the catalog switch. 6–7
  **accepted**: bench demo now exercises the label↔checkbox `for`/`id`
  association and the probe asserts it plus the toggle item's `data-state`.
  8–9 (probe depth) **disputed**: the `.mjs` probe layer is cheap
  diagnostics by design; behavioral depth belongs to the per-task e2e specs
  (ui-work-loop's tier contract).

### Dark palette + token migration (2026-07-19)

- **Token set**: `style/input.css` now carries the full Rust/UI standard set
  (background/foreground, card, popover, primary, secondary, muted, accent,
  destructive + foregrounds, border, input, ring) in `:root` *and* `.dark`
  (upstream OKLCH values, charts/sidenav trimmed), plus
  `@custom-variant dark` and a base `body { bg-background text-foreground }`.
  Full set added now so Stage 1 components land without token churn.
- **Dark is the default**: `shell()` reads the `tr_theme` cookie from the
  request `Parts` in context and stamps `class="dark"` (absence of cookie or
  any non-`light` value = dark) on `<html>` during SSR — right before any
  wasm runs, no flash. `<html>` attrs live outside the hydrated root, so the
  client toggle owns them post-hydration; no mismatch by construction.
- **theme_toggle vendored** (deviations in its header): upstream's `icons`
  crate inlined as two SVG paths; `use_theme_mode` hook replaced with
  app-owned state — toggle flips the class and persists
  `tr_theme=light|dark` (1 year, SameSite=Lax). Bench section is live
  against the real page theme (unlike the bench-local toggle).
- **Hex migration**: HomePage + auth_pages fully on tokens (auth_pages was
  conveniently constant-driven). The scaffold teal CTA became `bg-primary`
  (the wireframes are grayscale; a brand accent is a later design decision).
  Deliberately NOT migrated: the two standalone bounce/callback HTML strings
  in lib.rs (raw documents served without the stylesheet — tokens can't
  reach them).
- Verified live: 6/6 theme-probe checks (dark default SSR, toggle flips
  class+cookie, both overrides survive reload SSR-side, raw no-JS SSR honors
  the cookie), hydration clean on 4 routes, fast tier 4/4, **and the Android
  webview** (dark default + toggle flip on-device over CDP — matrix path 1).
- **Codex review** (3 findings, all accepted + fixed): production had no
  toggle mount until the shell lands → interim footer mount on HomePage (two
  lines the shell task deletes); light-override icon flash → the signal now
  initializes from the cookie on BOTH sides (`cookie_theme_is_dark()`: Parts
  SSR-side, `document.cookie` client-side — the cookie is deliberately not
  httpOnly), removing the corrective Effect entirely; the bench-local toggle
  couldn't show light under the dark `<html>` default (container-scoped
  class can't override ancestor variables) → the bench control now drives
  the `<html>` class directly, session-only, no cookie. All re-verified
  (6/6 probe, hydration clean, fast tier 4/4).

### Dev seed data (2026-07-19)

`app/src/seed.rs` (hosted-only) + the `server --seed-dev <uuid>` CLI arm
(mirroring `--ingest`) + `scripts/seed-dev-data.sh` (resolves the e2e user's
uuid owner-side, then runs the seed as `app_runtime`). Decisions:

- **Real methods only** — every write goes through `CollectionStore` /
  `CatalogStore` (search → card_detail → first printing), so the seed
  exercises the same paths the `/my/*` screens read back, including the lazy
  Inbox provision, RLS under `scoped_tx`, and intake `moves` rows.
- **Shape**: Inbox (4 arrivals) · Trade Binder (6 cards, one foil playset) ·
  Shoebox ▸ Rares (nested) · "Commander Deck" (format=commander; commander
  system-tagged; 7 mainboard + 1 sideboard; 2 wants held in Trade Binder →
  the owned-elsewhere needs bucket, 2 wants held nowhere → short/shopping) ·
  1 explicit move (Trade→Shoebox) for undo/pull history.
- **Idempotency = sentinel** ("Trade Binder" exists → no-op). Chosen over
  delete-and-rebuild: re-seeding from scratch is `end2end/seed-e2e-user.sh`
  with a fresh `.env` (recreates the user; collections cascade). Verified:
  first run wrote {4 collections, 20 holdings, 4 desires, 1 move}; re-run
  no-oped; dev-branch SQL shows 5 collections / 29 copies / 4 desires /
  21 moves / 1 commander tag.
- Seed queries fail loudly (`found x/n — is the POC catalog ingested?`)
  rather than building a partial tree.
- **Codex review** (7 findings): partial-tree-behind-sentinel + non-atomicity
  → **fixed** with cleanup-on-error (created roots deleted best-effort; a
  wrapping tx is impossible through the store methods, deliberately);
  `--seed-dev` shipping in the release binary (unlike `--ingest`, no
  dedicated credential) → **fixed** with `#[cfg(debug_assertions)]` — release
  binaries don't carry the arm at all; owner-credential SQL interpolation +
  PG env inheritance in the scripts → **fixed** (psql `:'email'` variable via
  stdin — note `-c` never interpolates variables — and per-invocation PG*
  env), same hardening retrofitted to `seed-e2e-user.sh`; name-based
  sentinel spoofable → **disputed**: the e2e account is purpose-built and
  script-owned by contract. All fixes re-verified live (idempotent no-op
  path + fresh-user path).

### Filter rail + query↔rail sync (2026-07-19)

`app/src/catalog/rail.rs` — the pure `read`/`rewrite`/`reset` layer plus the
widgets; `FilterRail` fills the shell's Catalog-mode sidebar and `FilterSheet`
is the mobile slide-over. The rail holds **no state of its own**: it reads `?q=`
and every edit rewrites that string and navigates, so a rail edit and a typed
edit are the same operation.

- **The grammar moved to `shared/src/search.rs`** — the move catalog-search's
  Parser section predicted ("may motivate moving the *parser* (not the SQL) to
  `shared/`"). Both halves of the two-surface UX need it now: the hosted backend
  translates terms to SQL, and the rail reads the query in wasm *and* in SSR
  under either backend. Only `sql.rs` stayed behind `hosted`.
- **`parse_tokens` is the new entry point, and the raw text is the point.**
  Re-serializing from the AST would rewrite a user's `type:` to `t:` and drop
  their quotes, so "unrecognized terms preserved verbatim" is implemented by
  re-emitting each unowned token's original characters, never by pretty-printing.
- **Ownership is one rule, shared by `read` and `rewrite`** (`owns_every_match`).
  Only the name box owns a run — bare words are collectively one field; every
  keyed facet owns just its first term, because a repeat like `c:u c:r` is an
  AND its single widget cannot express. The first version had `read` showing
  only the first while `rewrite` deleted them all, silently dropping the second
  (Codex, high) — the shared predicate is what keeps the two honest.
- **`t:` gained comma-OR** (grammar + `ILIKE ANY` in sql.rs). The wireframe's
  Type facet is a multi-select and flat syntax has no other way to say "instant
  OR sorcery" — exactly the case catalog-search's comma micro-extension exists
  for; it was specified on `s:`/`r:` only because those were the facets that
  existed then. Recorded as a deviation in catalog-search.
- **Colors concatenate, everything else comma-separates.** `c:` means "has all
  of these", so its values are one letter-set (`c:ur`), not an OR list. This is
  the one facet where the comma rule does *not* apply, and it survived the first
  mutation pass unnoticed — it now has its own unit test.
- **`c:colorless` counts without a checkbox.** It is a real Color filter the
  wireframe's five-box facet cannot draw. Counting it 0 hid the Reset button and
  the mobile badge on a filtered query (Codex, medium).
- **The name box shows a value it can write back**, not the raw token: `name:bolt`
  displays as `bolt`, because echoing the raw form would re-serialize on the next
  edit as the literal `"name:bolt"` — a different search (Codex, medium).
  Conversely anything typed *into* that box that would parse as a keyed term or a
  negation is force-quoted, so "Card name" can never become a type filter.
- **A rejected query makes the rail inert, not empty.** There is no honest way to
  reflect an unparseable query into widgets, and rewriting one term of it means
  guessing which term is broken; empty-but-clickable boxes would eat the user's
  text on the next click.
- **Sections are `<details>`**, seeded once (wireframe defaults, plus "open if it
  already has filters") and then left to the user. Deriving openness reactively
  would slam a section shut under someone mid-click. Consequence: a filter typed
  into the query bar for a collapsed section shows only as the summary badge.
- **History matches the query bar's rule** — first filter on a bare `/catalog`
  pushes, refinements replace. The two surfaces edit the same string, so Back
  must not depend on which one you used last. The first version replaced
  unconditionally and walked straight off the site.

**Regression found while building this:** `bind:value` is a client-side binding
and renders **no `value` attribute**, so every SSR'd input — including the
catalog task's own query bar — came back empty on a shared `?q=` link and only
filled in after wasm landed. Fixed here for the query bar and all four rail
fields with a one-time `value=` alongside the binding (set once, not reactively:
after hydration the property is what shows, and a reactive attribute would race
the binding).

**Hydration test seam:** `data-hydrated` on `<html>`, stamped by an `Effect` in
`app/src/lib.rs` (Effects don't run during SSR, so the attribute *is* the
definition of hydrated). Added because the rail e2e specs flaked under parallel
load: typing into an SSR'd input before hydration drops the input silently. It
also fixed **8 pre-existing firefox/webkit failures** in `catalog.spec.ts` /
`smoke.spec.ts` — those had never been run at the full tier — and cut the
three-browser tier from 49 s to 18 s by replacing implicit retry waiting with a
deterministic gate.

**`#![recursion_limit = "512"]` on the `app` crate.** A Leptos view tree is one
deeply-nested generic type per page, and the rail's seven stacked sections
crossed rustc's 128 default — *but only for `aarch64-linux-android`*. The host
targets built fine, so nothing caught it until the Android build ran. Worth
knowing for the rest of Phase 5: the per-page type grows as screens gain
sections, and the failure surfaces on the target you compile last.

**Verification:** unit 20/20 (`catalog::rail`) + 22 in `shared::search`; e2e
118/118 across chromium/firefox/webkit, stable over three consecutive runs;
Android on-device via CDP (`end2end/android-rail-check.mjs`) — badge count,
sheet open, facet click rewriting the query, and body scroll-lock, all on the
device's Chrome 145. Codex e2e mutation pass: 10 mutations applied transiently,
10 caught (the colour-serializer one only after adding the test it exposed).

**Disputed / deferred:**

- **Codex (medium): rail edit vs a pending query-bar debounce.** Real — type in
  the box, click a facet inside 250 ms, and the debounce fires last with its
  captured text, losing the facet edit. Not fixed here: the two surfaces live in
  different subtrees (the rail is in the shell, the bar is in the page), so
  sharing the pending-timer handle needs a context that `use_navigate`'s
  non-`Send` closure can't ride. Filed as its own task rather than bolted on.
- **Set is a text input, not a picker.** No `list_sets` adapter exists, and
  adding one is a server-fn of its own. `s:mh3,lea` typed as comma-separated
  codes is the honest interim; filed.

### Card detail `/cards/:id` + previews (2026-07-20)

**The multi-face projection fix, widened.** The spec named "the summary/detail
projections"; the same bare `image_uris->>'normal'` appeared at **six** sites in
`hosted.rs` (detail printings, `card_summary`, `search`, `collection_view`, and
two tagged-card reads). Fixing only the two named would have left DFCs
imageless in the catalog grid and every collection view, so all six took the
`COALESCE(…, faces->0->'image_uris'->>'normal')` fallback. Measured on the dev
POC catalog: **128 of 2,976 printings** had NULL top-level `image_uris` with
`faces` populated, and all 128 resolve after the change; zero previously-working
rows changed. `catalog.spec.ts`'s "transform layouts legitimately have no image"
caveat is now obsolete.

The four correlated subqueries additionally gained
`AND COALESCE(...) IS NOT NULL ORDER BY id LIMIT 1`. That one is **defensive,
not a fix for anything observed** — an unordered `LIMIT 1` could pick an artless
printing while a sibling has art, but zero cards in the POC catalog currently
have that mix. It matters at full-catalog scale (Scryfall carries artless
placeholder rows).

**`/cards/:id` needed `SsrMode::Async`, and the test that "proved" SSR couldn't
tell.** The route inherited Leptos's default out-of-order streaming, which ships
the whole `<Transition>` as a `<template>` plus a hoisting script while the
in-place markup stays the skeleton. A `request.get(...).toContain(...)`
assertion passes either way, because the template content *is* in the body — so
the original SSR test was vacuous. Confirmed by counting unclosed `<template>`
tags before the content (1 before, 0 after). The test now asserts the skeleton's
`aria-label` is **absent**, which is what actually distinguishes the two.

**Previews are lazily mounted, and that was a correctness fix, not an
optimization.** Rendering both preview bodies up-front put every card's name and
art into the DOM two extra times per row. That broke three *pre-existing*
catalog tests: `getByText("Lightning Bolt").first()` began resolving to a hidden
copy inside a closed popover. Both bodies now mount on the interaction that
reveals them (latched, so they stay mounted after) — the sheet keys on its own
`sheet_seen` latch rather than `sheet_open`, because gating on the live signal
empties the panel on the same tick its 300 ms slide-out begins.

**Grid tiles deliberately opt out of the hover preview** (`hover=false`) — a
deviation from the spec's "any row/tile". A tile *is* the card art, so a hover
card there is a smaller copy of what you are already looking at. The touch sheet
stays on everywhere, since a tap still wants an alternative to navigating.

**`hover_card` gained a `disabled` prop** (vendored-component deviation, bench
section in the same commit). Touch browsers fire a synthetic `mouseenter` on
tap, so without it a tap opened the sheet *and* the hover card. Two subtleties
found by review: the disable must **cancel the pending timer**, not just clear
`open` (an already-scheduled open fires 150 ms later and undoes it), and the
timer callback re-checks `disabled` on fire because the flag can flip mid-delay
— which is precisely the hydration window in `CardPreview`, where the pointer
type resolves in an Effect after listeners are attached.

**Mutation pass: 5 mutations, 5 kills — but two only after strengthening the
tests.** Two assertions were vacuous and survived their mutation:

- *Removing `disabled=coarse` did not fail the touch test.* The test tapped, and
  the sheet's backdrop steals the pointer — the resulting `mouseleave` cancels
  the pending hover open, so the hover card stayed shut for an unrelated reason.
  The scenario `disabled` actually guards is a coarse pointer **travelling over**
  a row without tapping (scrolling a list), where nothing would dismiss the card
  afterwards. Rewritten to hover without clicking; it now kills.
- *`toBeVisible()` on the sheet proved nothing.* `SheetContent` slides via a
  transform and stays in the layout when closed, so a **closed** sheet is
  "visible" to Playwright too. All sheet assertions moved to
  `data-state=open|closed`.

**Verification.** Full three-browser tier 148/148 (chromium + firefox + webkit)
— run at the end of this task under the revised policy, not at a stage boundary.
Android dev-webview: a new `end2end/android-card-detail-check.mjs` drives the
real WebView over CDP, 11/11, and is the only place `(pointer: coarse)` is
decided by an actual device rather than Playwright emulation. `bench-check`
CLEAN (extended with the `disabled` assertions). Hydration CLEAN on the detail
page, the multi-face page, and the malformed-id page. Merge gate green, 8/8.

**Disputed / deferred:**

- **Review (medium): `PreviewBody`'s "N owned" badge is unreachable.** Correct,
  and it turned out to be a **pre-existing** hole rather than one this task
  introduced: `HostedBackend::search` selects no `owned` column at all, so
  `CardSummary::owned` is `None` on every search hit and `CardTile`'s identical
  badge has never rendered either. Kept the branch rather than deleting it (it
  mirrors the existing tile and goes live when the projection is fixed) and
  filed the projection as its own task. It under-reports rather than
  misinforming. This also forced the authed e2e to locate seeded holdings by
  walking `t:creature` detail pages instead of filtering on `owned`.
- **Review (low): `coarse` is sampled once, per card.** No `MediaQueryList`
  change listener, and one signal + one `match_media` per `CardPreview` (~60 on
  a catalog page) for what is a global fact. No correctness impact short of a
  convertible flipping pointer modes mid-session; a shared context is the right
  shape and is deferred rather than bolted on here.
- **Codex review: retrieval, and a correction.** The first pass through this
  task recorded that the Codex step "cannot run unattended". **That was wrong.**
  The `/codex:*` slash commands are indeed `disable-model-invocation: true`, and
  the `codex-rescue` subagent is a one-shot forwarder that returns before its
  job finishes — but both commands are thin wrappers over
  `scripts/codex-companion.mjs`, which is callable directly:

  ```
  node "$CLAUDE_PLUGIN_ROOT/scripts/codex-companion.mjs" status
  node "$CLAUDE_PLUGIN_ROOT/scripts/codex-companion.mjs" result <job-id>
  ```

  Job `task-mrt56mi7-6w478m` was retrieved that way and its six findings are
  folded in above. Convergence with the independent reviewer was high: Codex
  independently flagged the `disabled` timer race, the `first()` hero art, and
  — notably — **both** of the vacuous assertions the mutation pass had already
  caught. It found two things the substitute missed: the hybrid-pointer gap and
  the unasserted per-row quantities, both fixed here.

  Lesson for the loop, and the reason ui-task-loop now says this: **the
  autonomous path is the runtime script, not the subagent.** `codex-rescue`
  dispatches and returns; polling it for the result gets a refusal, because its
  own instructions forbid follow-up work. Do not read that refusal as "Codex is
  unavailable".

### Cross-task audit (2026-07-20, during the card-detail task)

A sweep of every prior Phase 5 task's deferred/disputed items, looking for debt
that was recorded in Findings but tracked nowhere. Nine items were filed as
TODO tasks; three things are worth stating here.

**The `/catalog` hydration warning was not benign, and its comment said it
was.** `ResultsToolbar` derived the mobile sheet's "Show N results" footer by
reading the results `Resource` in render, with a comment justifying it as a
deliberate escape from the suspense boundary. The goal was right; the mechanism
was not. SSR ran the closure before the resource resolved and emitted "Show
results"; hydration *claimed* that text node without rewriting it; the label
then stayed countless until the next query change. Reproduced on
`/catalog?q=bolt` — two results, label reading "Show results". Fixed by moving
the derivation into an Effect-written signal, which keeps the non-blocking
property (Effects don't run in SSR, so SSR still renders the `None` branch
deterministically) while making the post-hydration update a real signal change.
`/catalog` is hydration-CLEAN again, and a regression assertion in
`filter-rail.spec.ts` fails against the old shape. This was the only
read-in-render of a resource in the codebase; the other four all go through
`Suspend`.

**How it survived two tasks is the more useful finding.** `git blame` puts the
line on the filter-rail commit, and that task's Verification block lists unit
tests, e2e, Android CDP and a mutation pass — **but no hydration probe**, which
step 3 of the loop requires. The card-detail task then probed the detail pages
it had touched, not `/catalog`. So a required step was omitted once, silently,
and nothing in the loop's failure policy catches an omitted verification step.
The ui-task-loop skill now names the probe's scope explicitly (the pages you
touched **and** the pages that render components you touched).

**Two vacuous assertions of the same shape as the card-detail ones were still
live in `filter-rail.spec.ts`.** `await expect(sheet).toBeVisible()` on a
closed `SheetContent` passes, for the reason recorded above — and the locator
was pointed at the rail body nested *inside* the panel rather than the panel
itself, so it could not have read the open state either way. Retargeted to
`[data-name=SheetContent]` + `data-state`. Worth generalizing: **`toBeVisible()`
is never the right assertion for this Sheet**, on any surface.

### `bind:value` SSR seed, moved into the primitive (2026-07-20)

`bind:value` is a client-side binding: it drives the DOM property and emits no
`value` attribute, so every SSR'd input came back empty and filled in only once
wasm landed. The filter-rail task found this and patched its call sites; by the
time it was looked at again there were **three hand-written copies of the same
workaround** (the query bar and two rail fields), each capturing
`get_untracked()` and passing a one-shot `value` through the `{..}` spread.

`Input` now seeds the attribute itself from `bind_value`, and the three call
sites dropped their copies. The reasoning for fixing the primitive rather than
documenting the workaround: the failure is invisible (the field just looks
empty for a beat, on a shared link), it is re-inherited by every SSR'd form,
and three Stage 3 tasks each ship one. `InputGroupInput` needed no change — it
renders through `Input`.

Locked by a request-level assertion in `filter-rail.spec.ts` covering **both**
render paths into the primitive (`Input` directly and via `InputGroupInput`),
since they are separate call chains. Mutation-checked: removing the seed fails
the test.

Not fixed, deliberately: `command`'s item registry is ordered by mount rather
than document position, so an in-place keyed *reorder* of persistent items
would make ↑↓ visit rows out of visual order. That one is filed rather than
fixed — no current consumer reorders in place, and writing DOM-ordering code
with nothing to verify it against is worse than the latent bug. Read that task
before building the destination picker, quick-add, or ⌘K.

### Destination picker + Want/Have quick actions + undo toasts (2026-07-20)

`app/src/catalog/destination.rs` (picker + shell-level state), the
`QuickAddButton`/`raise_add_toast` half of `app/src/catalog.rs`, and the
`quick_add` / `undo_quick_add` adapters in `app/src/lib.rs`. Stage 2's last
task, and the first UI task that **writes**.

- **`+ Want` is not undoable, and the toast says so by omitting the button.**
  This is the task's one real gap and it is an API-shape finding, not a UI
  choice. Undo is the `moves` ledger's `undone_at` flag
  (specs/collection-api.md), and `add_desire` writes no ledger row; the trait
  exposes no desire-quantity operation to compensate with either (`desires` can
  be added, board-relabeled, or cascade-deleted with their collection — nothing
  else). So a Want can be confirmed but not reversed. Offering a dead Undo
  would be worse than offering none. **Filed as a follow-up** — it needs
  `set_desire_quantity` on the trait, both backends, and a route, which is
  collection-api's surface, not a UI task's.

- **`+ Have` goes through `move_cards`, not `add_holding`.** Both write the
  same thing — `add_holding` appends its own intake `moves` row — but only
  `move_cards` *returns* that row's id, and undo targets a specific move id.
  Routing a Have through the intake form of a move (`from = None`) is how the
  toast gets an undo handle without widening the trait. `undo_last_move` was
  the alternative and was rejected: it races a second tab or a fast second
  click, so the toast could undo a *different* add than the one it names.
  Note this also means the task's original "adapters over batch-add" framing
  did not survive contact — `batch_add` returns `Vec<LineResult>`, which
  carries no ids at all, so it cannot back an undoable single-card add.

- **`CardSummary` gained `printing_id`, because a catalog row could not be
  Had.** Holdings are per-printing; the catalog is per-oracle. The projections
  already chose a representative printing to source `image_uri` from, but
  discarded its id. Both per-oracle projections (search, card summary) now
  share one `REPRESENTATIVE_PRINTING_JOIN` lateral so they cannot drift on
  which printing a row stands for. The lateral orders by
  `(image IS NULL, id)`, which returns the *same* printing the previous
  image-only correlated subquery did, while also populating an id for a card
  whose printings all lack art — that card can now be Wanted and Had rather
  than being silently unaddable.

- **The server-fn POST default is URL-encoded, not JSON, and it silently
  mangles nested DTOs.** The first cut passed the caller's whole `AddLine`
  (an internally-tagged enum) and got
  `invalid type: string "1", expected i32` on `quantity` — every field
  flattened to a string. Worth knowing before the next adapter takes a struct:
  either declare `input = Json` or keep the arguments scalar. This one ended up
  scalar for an unrelated reason (below), so it needs no codec override.

- **Adapter arguments are scalars and the DTO is built server-side** — Codex
  review, accepted. Taking the caller's `AddLine` let anything holding a
  session POST `quantity: 20`, a printing-pinned Want, or a non-default board
  at an endpoint whose whole contract is "one copy, default grain". Severity
  was argued down from the reviewer's *medium*: it is **not** a privilege
  escalation, since the same caller can already reach
  `POST /api/collections/{id}/have` with any quantity on their own
  collections, and `moves` carries `ENABLE`+`FORCE` RLS with an owner policy
  (verified — a cross-user `undo_move` finds no row and 404s). It was fixed
  anyway because an adapter whose wire contract is wider than its name is a
  trap for the next caller. Quantity 1 is now true by construction.

### Collection tree, read-only (2026-07-20)

`app/src/my/tree.rs` (assembly + `CollectionTreeNav`), the vendored
`collapsible` + `item`, the `CollectionStore::collection_tree` read
(shared DTOs, hosted SQL, native client, `GET /api/collections/tree`,
`collection_tree` GetUrl adapter), and the shell wiring (rail My-mode arm,
mobile tab badge). Stage 3's opener; management (drag, context menus) is the
next task.

- **The read returns own-counts; the client rolls up.** `list_collections`
  carries no counts and nothing else provides per-collection aggregates in one
  call, so the tree got its own read model rather than N+1 `collection_view`
  calls: flat rows (each `CollectionSummary` + `SUM(holdings.quantity)` for
  that collection alone) plus the shopping-short count. The client already
  reassembles nesting from `parent_id` (the DTO's documented contract), so
  rolled-up badges, the All-cards total, and the Inbox tab badge are the same
  walk. Assembly is pure and unit-tested (Inbox pin, sibling order, orphan
  surfaces at top level, a parent cycle neither renders nor hangs).
- **`ensure_inbox` extracted and shared.** The spec pins lazy Inbox
  provisioning to "the first `/my` request" — `collection_tree` is exactly
  that, so the INSERT moved out of `list_collections` into a helper both call
  rather than being duplicated.
- **The shopping badge is a COUNT over `shopping_list`'s own short rule**
  (total desired − owned > 0 per oracle) — same CTE shape, so the badge and
  the page it advertises cannot disagree.
- **Vendoring deviations** (headers + gap analysis carry them): `collapsible`
  gained `aria-expanded`/`aria-controls` (caller-supplied content id, the
  deterministic-ID convention) and **`inert` when closed** — the grid
  animation keeps collapsed content in the DOM, which would leave collapsed
  tree links tab-reachable. `item`'s `variants!` was hand-expanded (V1
  convention), `support_href` became a real `href` prop rendering an `<a>`,
  and upstream's `[a]:`-arbitrary-variant hover classes moved onto that arm
  as plain utilities (the `[a]:` form resolves to no usable CSS here).
- **One shell-level resource, refetched by quick-add.** The desktop rail and
  the mobile tab badge share a `CollectionTreeResource` (the
  `CurrentUserResource` pattern); catalog `+ Have`/Undo refetch it so the
  badges don't go stale on the page where adds happen. Anonymous shells skip
  the fetch entirely (`None`) instead of 401ing on every public page view —
  e2e-asserted at the request level.
- **Codex review: 2 findings, both accepted and fixed.** (1) *Medium*:
  `reparent_collection` never rejected the Inbox, so a raw API call could
  nest it and defeat the pinned-first rendering (the pin only applies among
  roots). Fixed at the API — `AND NOT is_inbox` + the same
  `absent_or_inbox` disambiguation rename/delete use; verified live (409 on
  the Inbox, legal reparents unaffected). The IA calls Inbox *renamable* but
  collection-api ships it unrenamable — reparent now sides with the
  shipped protections; if renamable is ever honored, reparent stays
  protected regardless. (2) *Low*: selection used exact URL equality, so a
  collection lost its highlight on its own `/needs` subpage — tree rows now
  prefix-match (pinned rows stay exact, since `/my` prefixes everything).
- **The `.pen` wireframes were unreachable this session** (no Pencil editor
  open — the MCP needs one). Design authority fell back to
  design/information-architecture.md's sidebar wireframe + this spec's text,
  which specify the read-only tree completely (pins, delimiters, counts,
  collapse). Flag for the maintainer if visual detail diverges from the DCol
  frame.
- **Verification:** unit 5/5 (assembly) + the suite's 84; SSR curls (authed
  `/my` renders the full tree server-side; rollups internally consistent —
  All cards 31 = 6+14+1+2+8 with Shoebox 3 = 1 own + 2 child); hydration
  probes CLEAN anonymous ×4 and authed ×3; bench-check CLEAN with new
  collapsible/item assertions (SSR markers, toggle, aria-expanded, inert,
  href arm); fast tier 8/8; **full three-browser tier 196/196**; Android
  dev-attach probe `android-tree-check.mjs` PASS on the real webview
  (anonymous shell + bench collapse/inert — the tree itself is authed and
  the dev proxy strips cookies, per the fixed platform matrix).
- **Mutation pass: 7/7 kills** (one proposed mutation per test, each applied
  transiently against the rebuilt binary — wasm hash polled per the
  ui-task-loop rule — confirmed failing, reverted): skeleton-instead-of-tree
  (SSR test), rollup → own-count (badge loop), `inert` dropped (collapse
  test), selection back to exact-match (subpage assertion), row href →
  `/my` (navigation), tab badge hardcoded 0 (mobile test), anonymous
  client-side tree fetch injected (request-listener test). One analysis
  subtlety worth keeping: the anonymous no-fetch assertion watches *browser*
  requests, and an SSR-side guard regression fetches in-process where no
  request listener can see it — the injected-`Effect` mutation is the
  client-visible form of that regression, which is the half the test can
  honestly own. Codex also noted per-assertion overlap in the multi-concern
  tests (each behavior still has a killing assertion; no test was vacuous).

- **The picker re-resolves against every collection list, not just the first**
  — Codex review, accepted and real. The state is the *shell's*, so it outlives
  the widget; seeding once meant a collection renamed or deleted between two
  mounts left a stale label, or an id every add would `NotFound` on. The
  module doc had already claimed the label was "always resolved from the live
  list", so the code was contradicting its own contract. `reconcile` now keeps
  the chosen id, refreshes its name/flag, and falls back when it is gone.

- **`command`'s mount-order registry is safe for this consumer, and the
  reasoning is worth keeping.** The queue task carried the V3 caveat forward.
  The picker sorts its collections (Inbox pinned, then by name) *before* any
  item mounts, and typing *hides* rows rather than reordering them, so
  registration order still equals document order. No `compareDocumentPosition`
  sort was needed. The caveat stands unchanged for quick-add and ⌘K.

- **Persistence is a cookie, matching `theme_toggle`, not localStorage.**
  `tr_dest` is readable during SSR *and* in the wasm, so the server renders the
  chosen destination instead of a placeholder that a corrective effect rewrites
  a frame later. It stores the id only — the label always comes from the live
  list, which is what makes a rename or delete degrade gracefully.

- **Verification gap that nearly shipped: the hydration probe runs
  anonymously.** The picker only renders for a session, so
  `hydration-check.mjs` walked `/catalog` and reported CLEAN having never
  instantiated the component under test. Added
  `end2end/hydration-check-authed.mjs`, which reuses the Playwright login
  fixture's storageState; `/catalog`, `?q=`, `?view=list` and `/my` are CLEAN
  authed. Any future authed-only surface needs this probe, not the other one.

- **Android on-device coverage is anonymous-only here, by policy, not by
  omission.** ui-work-loop's spike fixed the matrix: the dev proxy strips POST
  bodies and Cookie headers, so authed interactions stay on the web tiers
  (webkit = the WKWebView proxy). Everything this task added on the authed side
  — picker, adds, toasts — is therefore unverifiable on the emulator until the
  already-queued "Android release auth check" task runs. The anonymous
  `/catalog` surface (sign-in-prompt quick actions, no picker) was checked
  on-device.

- **Operational trap: `cargo tauri android dev` and the container's
  `cargo leptos watch` fight over `target/`.** Running the Android dev build
  while the web e2e tier was in flight failed the login fixture on a 15 s
  navigation timeout. Same family as the release-build clobber already
  documented in the e2e-suite skill, different trigger: two watch servers, one
  target dir. Sequence the platforms; never run them concurrently.

- **These e2e tests write to the Neon dev branch.** Every `+ Have` the suite
  makes is undone by the test that made it, so holdings return to their prior
  state. `+ Want` has no undo to call, so its desire row's quantity grows by
  one per suite run against a single upserted row — bounded rows, growing
  count, on a throwaway test user. Acceptable for now; it resolves itself when
  the Want-undo follow-up lands.

- **Mutation pass: 6/6 kills, and it caught a vacuous undo test.** The review's
  most useful finding was that "+ Have … the toast undoes it" passed with
  `undo_quick_add` stubbed to `Ok(())` — the 200 and the "Removed" toast were
  both still produced, so nothing asserted the *database* had moved. The test
  now brackets the add with reads of `GET /api/collections/{id}/view` (the
  machine route; `page.request` shares the context's session cookies) and
  asserts `present` goes `n → n+1 → n`. That also pins quantity to exactly one.
  Two more assertions were strengthened as conditionally vacuous: `data-chosen`
  could have been hard-coded on row 0 (now a non-chosen row is asserted too),
  and the filter test proved nothing on an Inbox-only fixture (now skips below
  two collections). Mutations killed: undo no-op, quantity 1→2, picker rendered
  for anonymous, Want handed an undo id, `data-chosen` hard-coded,
  `remember_destination` removed.

- **A mutation run's first result was a false survival** — the exact trap the
  ui-task-loop skill warns about, hit anyway. Three of four batch-A mutations
  "survived" because Playwright started while cargo-leptos had finished the
  wasm but not yet restarted the *server* binary; the wasm hash had already
  changed, so waiting on it was not sufficient. Re-running against the settled
  server killed all four. **Wait for `Serving` in the watch log, not just a new
  wasm hash**, before believing any mutation result.

- **Left in the dev DB deliberately: 2 Lightning Bolt copies** in the e2e
  user's Inbox, from the killed mutation runs (a mutation that breaks undo
  necessarily leaks the copy the test made). `end2end/cleanup-mutation-leftovers.mjs`
  reports and, with `--apply`, removes them. It was *not* applied: the arithmetic
  of what the runs should have leaked (+3) does not match what is there (2), so
  the rows cannot be confidently attributed to this task rather than to
  `seed-dev-data.sh`, and deleting shared dev-branch rows on a guess is worse
  than leaving two spare cards in a test user's Inbox.

### Collection tree, management (2026-07-20)

`app/src/my/tree_manage.rs` (the shared context menu, three confirm dialogs,
and the drag commit layer) plus the drag/menu wiring on the rows in
`app/src/my/tree.rs`; the newly vendored `context_menu`; and five thin
server-fn adapters in `app/src/lib.rs` (create/rename/delete/reparent/reorder).
The backend trait already had every method — this task is entirely UI + thin
adapters. Stage 3's second task; completes the collection-tree gap component.

- **`drag_and_drop` (registry) evaluated and rejected; the drag layer is
  custom.** The registry primitive reorders by mutating the live DOM during
  `dragover` (`insert_before` on real nodes) — under a hydrated Leptos view
  those nodes belong to the reactive graph, so the next signal update renders
  against a DOM Leptos no longer owns. It is also flat-list-only (Y-sort within
  one container; no drop-*onto* for reparent) and reports nothing back. Ours is
  signal-driven HTML5 DnD on the row heads: `dragstart` stamps a `DragState`
  (the node, its parent, and its forbidden-target set), `dragover` classifies
  the pointer's Y-band into `Before`/`Into`/`After` and paints a `data-drop-hint`,
  and `drop` calls a **pure** `plan_drop` that returns the writes to make. The
  fractional-index math is unit-tested in isolation (9 cases) because it is the
  part most prone to off-by-one; the server returns siblings `ORDER BY position,
  name`, so the neighbor lookup can trust document order.

- **The Inbox never drags and only accepts `Into`.** It is pinned first
  client-side, so ordering relative to it is meaningless; `drop_intent` collapses
  its bands to `Into`, and its `dragstart` is cancelled (its row is an `<a>`, so
  the native link-drag had to be suppressed explicitly).

- **Cycle prevention is client-first, server-backstopped.** The dragged node's
  `forbidden` set (itself + every descendant, from `subtree_ids`) makes its own
  subtree undroppable in the UI — no request is even sent. The unchanged
  `reparent_collection` 409 is the backstop for anything that bypasses the
  client. The e2e pins *both*: it asserts the drag sends **no** reparent request
  (the client refusal) *and* that a direct API cycle returns 409 — because
  asserting only the end-state can't tell "client refused" from "client sent,
  server rejected" (both leave the tree unchanged). That distinction was a Codex
  mutation-pass finding; without the no-request assertion, dropping `subtree_ids`'
  recursion survived.

- **`context_menu` rewired to `popover="manual"` after `"auto"` failed the
  right-click.** The obvious port used `popover="auto"` (top layer + light
  dismiss + ESC for free). It broke: a right-click's own trailing pointerup is
  read as an outside interaction and dismisses the auto popover the instant it
  opens — engine-dependent (one of chromium/firefox/webkit kept it, two didn't;
  observed as a `closed->closed` toggle). The fix is two parts: `popover="manual"`
  (no automatic dismissal) with our own `window` pointerdown-outside + ESC
  listeners, and **deferring the open one macrotask** so the opening gesture
  finishes before the menu enters the top layer. Verified on all three web
  engines *and* the real Android webview (long-press → `contextmenu`). One shared
  menu serves all N rows via `use_context_menu()`; the right-click sets a
  `menu_target` signal that the panel reads.

- **The `ContextMenu` provider had to move inside the `Suspense`.** First wiring
  put `<ContextMenu>` in `CollectionTreeNav`, wrapping the `<Suspense>`. The rows
  render inside `Suspend::new(async {…})`, and a context provided by the
  `<Provider>` component *above* that async boundary does not reach
  `use_context_menu()` calls *inside* it — the menu's content populated (that is
  driven by `menu_target`, provided in the component body, which does cross) but
  never opened (the open signal, from the provider, resolved to `None`). Moving
  the wrapper into `assembled_view` (a `TreeBody` child reads the handle) puts
  the provider and the rows in one synchronous owner. `TreeManage` stays provided
  in `CollectionTreeNav` because dialogs live outside the menu wrapper.

- **Codex review: 4 findings, all resolved.** (1) *high* — the delete confirm
  reread the live `menu_target` at submit, while create/rename snapshot their
  subject; a right-click landing elsewhere while the dialog was open would delete
  the wrong row. Now snapshotted into `delete_req` on open (regression test:
  open delete for A, dispatch `contextmenu` on B behind the modal backdrop,
  confirm still deletes A — verified it kills the un-snapshotted mutation). (2)
  *med* — the deferred open used an uncancelled timeout, so a `close` racing the
  macrotask could revive a dismissed menu; a generation stamp now invalidates a
  pending open on `close`/re-open. (3) *high→med* — a cross-parent edge-drop is
  two writes (reparent, then position) with no combining trait op; on a
  reparent-ok/reorder-fail the node *did* move parent, so the toast no longer
  claims "Couldn't move" — it says "Moved, but couldn't set its order." (4) *med*
  — fractional-index position collisions: real but inherent and unreachable at
  this scale (integer seed positions; needs ~50 midpoint inserts between one
  pair). Queued as a follow-up rather than building a rebalancer now.

- **Mutation pass: 12 analyzed, 9 killed outright, 3 gaps — 2 real,
  strengthened.** (a) bench-check *claimed* outside-click coverage in a comment
  but only tested ESC; an empty `pointer_outside` survived. Added the actual
  outside-click assertion (kill-verified). (b) the cycle-guard no-request
  assertion above. The third "gap" (the menu-visibility test doesn't click
  Rename) is covered at the suite level — the "Rename edits the name" test
  exercises that callback end-to-end, so the mutation dies there.

- **Delete-confirm copy counts holdings, not desires.** "This permanently
  deletes N nested collections and M cards" — M is the rolled-up `present`
  (holdings). The cascade also drops desires, which are not surfaced in the
  count; "cards" reads as the meaningful number and the copy already warns it is
  irreversible. Left as-is.

- **These e2e tests mutate the dev branch and self-clean.** Every test creates
  uniquely-named `zz-e2e-…` scratch collections via the API and deletes them in a
  `finally` (delete cascades the subtree, so one delete per created root). Names
  are worker-index + a per-file counter — no wall-clock — so parallel workers and
  the three browser projects don't collide. A crashed test can leak a
  `zz-e2e-…` root; they are harmless and greppable.

- **Verification.** Unit 79 (9 new `plan_drop` cases + the assembly suite);
  SSR authed curl shows the management markup server-side (`data-tree-root`,
  `data-tree-row-head`, the `role="menu"` panel); hydration CLEAN anon `/catalog`
  + `/dev/components` and authed `/my`; bench-check CLEAN with the new
  context-menu block (open, item-select, ESC, outside-click); **full
  three-browser tier 223→ (14 new management tests × 3)**; Android webview
  `android-tree-manage-check.mjs` PASS (open, on-screen positioning, item tap,
  outside-tap dismiss) on Chrome 145; Codex review + mutation pass both clean
  after fixes.

### Status-token variants re-added (2026-07-23)

V1's dropped variants restored: button `Warning`/`Success`/`Bordered`, badge
`Success`/`Warning`/`Info`, with the full upstream `success`/`warning`/`info`
token families (base/foreground/light/dark, both modes) from rust-ui
`style/tailwind.css` @ 43e1e32 in `style/input.css` + `@theme inline`
mappings, mirrored into the bench theme panel. Bench rows for every new
variant; bench-check gained a token-variant section asserting computed
backgrounds resolve (non-emission → transparent is the failure mode),
text utilities emitted (color ≠ inherited), family distinctness, and the
Bordered border (width / transparency / currentcolor-fallback equality).

- **Upstream's status colors fail WCAG AA — four value deviations, recorded
  in input.css comments.** White text on the 0.65 L light bases is 2.9–3.3:1
  (AA needs 4.5). And the `hover:bg-*/90` idiom alpha-composites over the
  page background, so light-mode hovers *lighten* — the base must carry
  headroom for the hover state too. Deviations: light `--success`/`--warning`
  0.65→0.48 L (base 5.81/6.58, hover-over-white 4.77/5.36); dark `--warning`
  0.65→0.67 L (upstream hover composite was 4.45); dark `--info-foreground`
  white→dark text like its siblings (white was 3.10). Plus the class-level
  deviation: `Bordered` swaps upstream's hardcoded `border-zinc-200` for the
  token border ("Tokens, not hex"; fixed light zinc reads wrong in dark).
  Method note: composite in **gamma-encoded sRGB** (what browsers do), then
  WCAG luminance on the decoded result — Codex's linear-space compositing
  overestimated the hover drop and *missed* the dark-warning 4.45 failure;
  the gamma-space numbers were confirmed by Codex's own recompute to four
  decimals in the final round (verdict CONFIRMED, all enabled pairs ≥ 4.5:1
  including badge pairs 6.9–9.9 and Bordered text 4.73/7.26).
- **Codex review, three rounds, 4/4 findings confirmed + fixed:** (1) the
  base contrast failures above; (2) the probe originally checked only
  backgrounds — a missing `*-foreground`/`*-dark` mapping silently inherits,
  so a text-≠-inherited assertion was added; (3) the Bordered check could
  false-pass on the `currentcolor` fallback when `border-border` fails to
  emit — border-color-≠-text-color added; (4) round 2 caught the hover
  composite gap. Known limit, accepted: computed-style assertions catch
  **non-emission**, not wrong-but-plausible values — value correctness is
  pinned by the reviewed token list itself.
- **Mutation pass 4/4 kills**, one per assertion class, each verified
  against the rebuilt CSS/wasm (`--color-warning` mapping deleted → bg
  assertion fired; `--color-warning-foreground` deleted → text assertion;
  `--color-info-light` aliased to success-light → distinctness; `border`
  dropped from the Bordered arm → width check, full wasm rebuild awaited).
  Codex enumerated 15 candidate mutations: the executed four cover each
  distinct assertion code path (the rest are per-token repetitions of the
  same loop assertions); its two `--color-border` mutations were analyzed
  statically, not executed — that mapping is app-wide and pre-existing, and
  round 2 confirmed the currentcolor equality catches its removal.
- **Verification.** bench-check CLEAN on the final values (and thrice
  during mutation cycling); hydration CLEAN anon (`/`, `/login`, `/catalog`,
  `/catalog?q=`) + authed (`/my`, `/catalog`, a card page); SSR curl carries
  the variant classes and the compiled CSS the utilities; fast tier 76/76;
  **full three-browser tier 226/226**; Android webview dev-attach:
  `android-cdp-check.mjs` PASS + a variant drive on `/dev/components`
  observing the final token oklch values computed on-device (Chrome 145).
  No app screen uses the variants yet — the polish task's error/empty
  states are the intended consumers.

### DFC back-face flip (2026-07-24)

The queued defect task: `/cards/:id`, the hover preview, and the touch sheet
rendered face 0 only, with the combined `Front // Back` heading — the back
face was unreachable. Landed as projection + DTO + UI, no ingestion work
(the task's own scope note held: both faces' `image_uris` were already in
`printings.faces`, per-face oracle data in `cards.card_faces`).

- **The layout allowlist widened by two.** The task named
  `transform`/`modal_dfc`/`reversible_card`; the dev catalog shows
  `double_faced_token` and `art_series` also carry true per-face
  `image_uris` on every printing face (and ≥2 well-formed `card_faces`),
  while `split`/`flip`/`adventure`/`prepare` printing faces carry **no**
  `image_uris` at all — same defect, same fix, so all five layouts flip
  (`shared::BACK_FACE_LAYOUTS`). One list drives the SQL gate
  (`summary_select()` interpolates it), the server-side face build, and the
  UI control, so the three cannot drift.
- **Flippability is decided server-side.** `CardSummary.faces` is non-empty
  only for allowlisted layouts (the SQL also CASE-gates `card_faces` off the
  wire for everything else), so clients key the control off
  `faces.len() >= 2` without re-deriving layout rules. On the detail page the
  same gate is `CardDetail::flip_faces()` (parse fails closed → no control).
- **The heading swaps, the combined name stays.** `card-name` shows the
  current face; the canonical `Front // Back` identity moved to a
  `card-combined-name` subtitle — which is also what keeps the pre-existing
  SSR test (`html.toContain(card.name)`) honest.
- **Two different printings can back the two surfaces.** Previews render the
  representative printing (has-art-first, lowest id); the detail hero is the
  oldest printing with art. The e2e therefore asserts **exact** art equality
  in previews (same printing as the API summary) but the Scryfall
  `/front/`↔`/back/` path segment on the detail page — pinning exact URLs
  there would over-constrain.
- **Codex review (step 2): zero findings** — explicitly cleared SQL
  ordering/NULLs/injection surface, SSR/hydration init, DTO version skew
  (`serde(default)` on both new fields), modulo/clamp safety, event
  propagation, the face-0 fallback, and single-face regressions.
- **Mutation pass: 6 analyzed, 4 applied, 4 killed — but only after
  strengthening 3 tests + the Android probe.** Codex correctly showed the
  art-pairing class survived: every art assertion checked "src changed and
  is Scryfall", so swapping the face→art index (detail) or pinning previews
  to the front image passed. Added the exact-equality / path-segment
  assertions above; all four mutations (index swap, front-pinned previews,
  `adventure` added to the allowlist, `flippable = true`) then failed their
  test and were reverted. The allowlist and flippable mutations were
  kill-verified live rather than taken on Codex's yes.
- **Flake note (not this task's):** the full tier's first run failed
  `collection-tree-manage` "drop on a row's lower edge reorders among
  siblings" on chromium+firefox in the same run; it passed 4/4 on targeted
  retry and the full rerun was 265/265. Looks like parallel-run contention
  on the seeded tree, worth an eye if it recurs.
- **Verification.** Shared unit tests 26 (7 new: allowlist, parse, zip,
  fail-closed paths); hydration CLEAN anon + authed (DFC page, catalog
  list/grid); bench CLEAN; SSR curls (flip control + face-0 heading +
  combined subtitle in raw HTML); fast tier 17/17; **full three-browser
  tier 265/265**; Android webview dev-attach `android-dfc-check.mjs`
  **12/12 on-device** (Chrome 145) including the strengthened art-pairing
  checks and the adventure negative; merge gate 8/8 green.
