# App UI — the real pages

**Status:** implemented
**Depends on:** [ui-design](ui-design.md), [ui-components](ui-components.md), [ui-component-bench](ui-component-bench.md), [collection-api](collection-api.md), [catalog-search](catalog-search.md), [auth](auth.md), [ui-work-loop](ui-work-loop.md)

> **What `implemented` means here** (maintainer decision, 2026-07-27, at the Stage 3
> boundary): all nine wireframe screens exist and reconcile against their frames at their
> own widths, with every deviation recorded in Findings. **One drawn control is knowingly
> unbuilt** — the `+ Want` / `+ Have` quick actions in the hover preview and the card sheet
> (`Preview Actions` / `Sheet Actions` in the frames, named at
> [information-architecture.md:78](../design/information-architecture.md)) — filed as a `[ ]`
> under Phase 5 discoveries rather than held open. So this status asserts *every screen
> exists and every frame otherwise reconciles*, not *every drawn control is built*.

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
placeholder). The grid caps at `max-w-7xl`, left-flush (no `mx-auto`) — the
same cap `Table` already carries for the list view, so neither surface grows
past it on a wide monitor (P6-098). Every result carries `+ Want` / `+ Have` and the sticky
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
Findings.) **The chip also has a neutral state** (2026-08-13, P6-143):
`✓ All needs met`, `success`-toned, when the collection has desires and none
are missing — still linking to `/needs`, which is the only navigation path to
that page's own "All set" empty state. A collection with no desires at all
still gets no chip, either state — see Findings.

**Deck variant** adds: format + commander(s) rendered as a card in
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
   out in the gap analysis.) The pill counts **entries** (cards), not copies;
   how many copies of each move is chosen in the move's **which-copies picker**
   — one row per full grain, each with a `− n +` bounded by its stack, default
   one (P6-150 ruling, 2026-08-15; Findings). An entry whose stack holds exactly
   one copy skips the picker and moves directly.

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

### The which-copies step became the quantity-and-version picker (P6-150, 2026-08-15)

`app/src/my/move_selection.rs`, `app/src/lib.rs`, `end2end/tests/batch-move.spec.ts`
— the maintainer's P6-150 ruling, and the end of "one copy per entry".

**The ruling (2026-08-15).** A tray entry is a *card*; the move flow must let
the user choose **how many** copies move and **which version** (finish /
condition / language) when a row spans several. All-or-one-copy does not serve
the point of moving cards between collections. It settles the four questions
the old decision task bundled: the tray keeps counting entries; quantity is
chosen at **move time, per entry**; duplicate `/my`-vs-collection entries
collapse in the move step's stack resolution; and multi-grain rows stay
selectable, because the step **asks** which grain instead of refusing.

**The duplicate collapse, as shipped: a display merge with per-row
attribution.** `/my` and a collection page are two views of one shelf, so
selecting a card on each makes two tray entries over the *same copies*.
`split_skips` merges every askable refusal for one oracle into one `AskedCard`
holding **one `AskedEntry` per tray entry** (its key and its own refusal);
`card_choices` offers the union of what those keys addressed, each stack
appearing once (the payload is already one row per full grain, filtered in a
single pass). A `StackPick` then answers for **the entries whose copies that row
actually is** — `addressed_by`, the same containment the rows were selected with
— so a row shared by a `/my` entry and a collection entry retires both, and a
row belonging to only one of them retires only that one.

**Merging on the card and attributing on the row is the whole design, and the
first attempt got it wrong.** Retiring every entry in a section was right for
the case the ruling named (two views of one shelf) and silently wrong for the
one it did not: two `Held` entries of one card — a deck's mainboard and
sideboard rows, two binders, two printings — are *different copies*. A user who
zeroed one entry's rows and confirmed another's watched the zeroed entry leave
the tray having moved nothing and said nothing, which is the silent drop this
file's whole reporting path exists to prevent. Splitting them back into two
sections would have been honest too, but worse to use: one card should be one
question with one list. So the section stayed merged and the *bookkeeping* went
per row — and `unanswered` went per entry with it, so an entry nothing came out
of is still named with its own reason (which also means cancelling a merged
section now says both sentences instead of only the first). Mutation-verified:
attributing a pick to its whole section again makes the tray pill vanish
entirely in the two-board e2e. Two things fall out for free: the
toast counts **cards by oracle** rather than by tray token, so one card's copies
can no longer be reported as "2 cards"; and the wire stops carrying two items
with the same token, which the server's own outcome could not have told apart.
Merging is per oracle only — nothing merges across cards, and nothing merges
into a refusal the step cannot ask about.

**What changed, in one sentence.** The which-copies dialog stopped being a
disambiguation escape hatch and became the move's ordinary second step: its
rows split to the **full grain** — `(collection, printing, board, finish,
condition, language)` — and each carries a `− n +` stepper bounded by its own
stack, so "2 foil and 1 etched" is two rows with two counts instead of one
un-actionable row.

**The routing rule, and why it lives server-side.** A plain tray entry still
carries no quantity. Resolution moves it only when the stack it resolves to
holds **exactly one copy** — the one quantity that needs no answer — and
refuses anything larger as the new `SkipReason::Several(n)`, which is askable
and therefore opens the picker. So a batch of single-copy entries keeps the
direct path (one request, no dialog) and everything else asks, per entry,
without the client having to know a count it cannot keep fresh. The alternative
considered and rejected was routing on a copy count carried on `SelectedCard`:
the tray is long-lived by design, so that number is stale by construction, and
— the deciding reason — the client cannot tell `Several` from
`ManyCollections`, so every refusal toast the user sees on cancel would have
degraded to "you didn't pick anything". `SelectedCard` therefore keeps its
shape; the *chosen* quantity rides on `SelectionItem`, its wire form, where it
can be checked. (`shared::MoveItem` already carried `quantity`, and
`append_move`/`undo_one` already read it — the write layer needed no change at
all, which is why this story is a client-and-adapter story.)

**Quantity is validated, never clamped.** `SelectionItem` gained
`pick: Option<Pick>` (grain + count, `#[serde(default)]` so an unpicked entry
sends exactly the shape it always sent). The server re-resolves every pick
against its own fresh, ungrouped `holdings_of_oracle` read and refuses per
entry: `NotEnough(n)` naming the stack's real size when the ask exceeds it,
`NoneRequested` for a zero, `NoCopies` for a grain that has since emptied. A
clamp was rejected outright — it moves a different number of cards than the
dialog said, behind a success toast, which is this repo's recurring defect
shape. Skipped reporting and batch survival are unchanged.

**`SkipReason::Grain` is gone, and not by being suppressed.** It named a stack
of several grains with no default one, which the old rows could not tell apart.
Two copies of anything now exceed one, so that stack is `Several` first, and
the picker renders it as one row per grain — the variant had no reachable case
left. `is_ambiguous` was renamed `is_askable` for the same reason: `Several` is
a *how many* question, not an ambiguity.

**Two tokens for one row, or the outcome cannot be read.** The picker's rows
are finer than `SelectionKey::Held`, so two picks on one card can differ only
by finish and would report under the same token — the toast's copy count, the
tray reconciliation and the refusal naming all read that list.
`SelectionItem::token()` appends the grain when a pick is present and is
byte-identical to the tray token when it is not, so nothing outside the picker
changed.

**A `Held` entry is asked only about its own stack.** The stacks read answers
per *oracle*, so without a filter a row selected in one binder would offer to
move copies out of another the user never pointed at. `card_choices` now
narrows by the entry's key: a `Held` entry gets its own stack split by grain, a
`/my` entry gets everywhere.

**Every row opens at one copy** — the maintainer's small-blast-radius default,
carried over from the old fixed quantity. The tension is real and was accepted
deliberately: a card sitting in three stacks now says "Move 3 copies" on the
button before it is pressed, where the checkbox version started at zero.
Nothing is hidden (each row shows its count, the button names the total, and
Undo covers the batch), and the common case — one stack, "how many" the only
open question — is one press of the button instead of two.

**One toast sentence for both passes.** `moved_message(copies, cards, dest)`
replaced the pair: the batch's "Moved 3 cards (1 copy each)" was true only
while quantity was fixed, and `picked_message`'s "across 2 cards" existed
solely because the other one could not say it. Both counts come from what the
server reported *moved*, never from what was asked for.

**Not `CountStepper`, and the reason is the phone.** The collection view's
stepper commits on blur, raises its own undo toast, and hides its ± buttons
below `sm` (a phone taps the number and types). Inside a dialog whose commit is
its own button, all three are wrong — and on the one surface this story exists
to serve there would have been no visible control at all. The picker's `PickCount`
is the same shape with none of the persistence contract: 44 px targets at phone
width, `sm:` back to the dense size, and it writes one slot of the dialog's count
vector. E2E covers it at 390 px (targets, no sideways scroll, a real press).

**Two majors from adversarial review, both about a snapshot being trusted past
its moment:**

- **The picker claimed the copies were gone while its read was still in
  flight.** The dialog resolves its rows in an `Effect` rather than awaiting
  them (that is what lets a stepper's value be a plain `Vec<i32>` beside the
  rows), and `Resource::get()` hands back the *previously resolved* value
  during a refetch — where the first value this resource ever resolves is an
  **empty** payload, because its closure short-circuits while no dialog is
  open. So every card rendered "No copies left to move — reload the page" with
  the confirm disabled for the whole round trip, self-correcting when the real
  read landed. Two states that look alike and mean opposite things. The payload
  now carries the oracle ids it was produced for, and a payload for another
  question is treated as "still loading"; the row list carries
  `data-state=loading|failed|ready` so the distinction is assertable at all.
  Auto-retrying assertions cannot catch this class — they poll until it heals —
  so the e2e **stalls the read** with a route handler and asserts the in-flight
  state while it cannot resolve. Mutation-verified: disabling the check turns
  the panel `ready` under a stalled read and kills the test.
- **A batch could draw one stack twice and take the whole batch down with it.**
  Validation reads a card's holdings once per *oracle* (the tray can hold one
  card twice) and used to validate every entry against that unspent snapshot —
  so two entries over the same stack each passed on their own, and the second
  `holding_take` inside `move_batch`'s single transaction hit `Conflict` and
  rolled back **every card in the batch**. That is the per-entry-refusal
  contract failing from the inside. `resolve_item` now spends the snapshot as
  the batch consumes it, so the later entry validates against what is left and
  becomes an ordinary `NotEnough`/`NoCopies` refusal while everything else
  moves. It is also the honest model: inside one transaction the earlier item's
  copies really are gone. Reachable from the UI through the duplicate-entry
  case above, and from a hand-rolled POST regardless.

**Two view-macro traps, both silent, both cost real time:**

- **`slot=` on a component makes the node vanish.** leptos reserves it for its
  `#[slot]` composition mechanism, so `<PickCount … slot=i …/>` compiled
  cleanly, rendered nothing, and announced itself only as an "unused variable"
  warning on the value being passed. The prop is named `index`.
- **A bare `>` inside an attribute value ends the tag.**
  `attr:disabled=move || value.get() >= max` made `= max …` the button's *text*
  and left it permanently disabled — the `+` could never raise the count, which
  is how it was found (an e2e stepper that would not step). Parenthesised.

**A fixture helper that could not promise what its name said, fixed properly
this time.** `unownedCards` filtered against the first 200 rows of `/my`, and
today's bulk-loaded seed parks a large slice of the catalog in the dev user's
Inbox — so "owned nowhere" cards came back holding 51 copies in the Inbox, and
every single-place assertion in the file turned into a which-copies dialog.
(P6-151 recorded this and fixed one *assertion* around it.) It now verifies each
candidate with `GET /api/cards/{id}/holdings` and takes the first `n` that
really are held nowhere. This is the file's share of the known
fixture-pool failure class; the class itself is still owned by its own task.

Two follow-on fixture findings, both measured rather than guessed:

- **One search letter is not a fixture.** The helper searched `q=z` alone,
  picked once because the seed's own cards came from name-ordered searches. Its
  free pool went to **zero** between two runs of this suite (0 held-nowhere
  cards in the first 40 hits, against 27-32 for `q`, `x`, `vi`, `un`), which
  failed six tests at the higher offsets. It now tries several terms in order
  and says so when they are all exhausted.
- **Double-faced cards are not a fixture either.** A DFC's catalog name is
  `Front // Back` while `/cards/:id` heads with the front face alone, so a
  `toContainText(card.name)` on the detail page failed while showing exactly
  the right card. Filtered out of the candidate pool; nothing here needs one.
- The scan is also **windowed** per `skip` now, so verifying candidates cannot
  converge two concurrent tests onto the same card.

**The same assumption is now failing suite-wide, and it is not this task's to
fix.** `removal`, `needs`, `command-palette` and `collection-tree-manage` each
carry their own copy of "a catalog card this account owns nowhere", and in a
full serial pass **18 of 36 failures were that helper giving up** ("the fixture
has fewer than 6 catalog cards the dev user owns nowhere"; "every candidate up
to skip 40 is already held somewhere") — with two more classes behind it: four
`401`s late in the run (a session expiring around `session-fallback`'s
cookie-mangling tests) and a residue of timeouts. `batch-move.spec.ts` carried
no failures in that run — measured **after** the sweep below and on the pool it
left, which is the only pool these numbers describe — because its helper is the
verified, multi-term one above; that is the pattern the others need. Separately, the shared dev branch had accumulated **51
leftover `zz-e2e-*` collections** from earlier parallel runs (`w1`…`w16`, plus
seven from this task's own timed-out runs), which held cards the "owns nowhere"
scan was looking for and doubled several tree/palette lists; swept with the same
delete endpoint every test's `finally` calls, leaving the nine seeded
collections. It accounted for seven of the failures — the pool exhaustion is
real beyond it.

**Evidence.** `cargo test -p app --features hosted --lib` 392 passed / 0 failed
— the quantity validation trio (over the stack, zero, a vanished grain), the
per-grain take, the full-grain split, the `Held`-entry row filter, the
count-vector→picks mapping, the two-tokens-for-one-row case, the unified toast,
the batch-overdraw pair (one stack drained across entries; spending addresses
the grain it took from), and the collapse-and-attribution set (one question,
union of rows, a shared row retiring both entries, two board rows retiring only
their own with the other still named, the `/my`-plus-two-boards composite, and
no cross-card merge). Full `batch-move.spec.ts`
@fast **16/16** chromium, including the grain-split test (2 foil + 1 etched →
two rows → the foils move, the etched copy stays, read back by grain through the
API), the stalled-read test, the duplicate-entry test, the two-board attribution test
and the phone-width picker test. Android webview: 16/16 on the emulator via the bench section (see the task
report).

### Ambiguous batch-move refusals became a which-copies step (P6-151, 2026-08-13)

`app/src/my/move_selection.rs`, `app/src/lib.rs` — the refusal contract the two
entries below established, revised where it dead-ended.

**What changed, in one sentence.** A `/my` row whose copies are spread over
several collections, printings or boards used to end the batch's story for that
card with a toast telling the user to go open a collection page and select the
row there; it now opens a **which-copies step** — the concrete stacks behind
the card, one row per `(collection, printing, board)` with its count — and the
picked rows complete the move without leaving the page.

**The whole feature is a client step over the write that already existed.** No
new mutation, no new wire item, no change to `move_batch`, `MoveItem` or
`holding_take`. The refusals named exactly the dimensions
`SelectionKey::Held { collection_id, printing_id, board }` already carries, so a
picked row *is* a `Held` entry, and the second pass is the ordinary
`move_selection` batch: the server resolves it through `resolve_held` against
its own fresh `holdings_of_oracle` read, at one copy per item, exactly as a
collection-page row does. That is also why the step cannot loop — a `Held` key
has nothing left to be ambiguous about.

**What still refuses, and why each one is not a question with rows behind it:**

| Refusal | Now |
|---|---|
| `ManyCollections` / `ManyPrintings` / `ManyBoards` | **the step** (`SkipReason::is_ambiguous`, renamed `is_askable` by P6-150) |
| `Grain(n)` | still a toast — a stack with several finish/condition/language grains and no default one is **one** row on any list this app can render (the step's rows are `(collection, printing, board)`), so offering it as a choice would show rows the user cannot tell apart **(superseded by P6-150: the rows are full grain now, that stack is simply several of them, and the variant is deleted)** |
| `AlreadyThere` | still a toast — the copies are at the destination; there is nothing to choose between |
| `NoCopies` / `NoLongerNeeded` | still toasts — the fresh server read just proved the stack (or the gap) gone; no choice fixes that |

**Cancelling is still a refusal.** Every exit from the dialog — Cancel, the ✕,
the backdrop, Escape — raises the refusal toast the batch would have raised, and
the entries stay checked. The three ambiguous `phrase()`s were reworded from
"open one and select the row there" to "pick the copies to move", because the
sentence is now what a user sees after *declining* the offer, not an errand.

**The stack list is a second read, deliberately.** `crate::selection_stacks`
composes reads this app already had — `holdings_of_oracle` (the only one that
does not group board or grain away), `list_collections` for names, `card_detail`
for the set/number chip — with **no trait method, SQL or route added**. Three
things decided it against fattening `MoveOutcome` with the stacks the batch's
own resolution had in hand: the payload would carry display strings for a dialog
that usually never opens; a read taken when the user is *asked* cannot be older
than the question; and staleness past that point is already handled, since the
second pass re-resolves every pick and answers `NoCopies` for a stack that
emptied in between. The catalog read is skipped whenever a card's copies sit
under one printing — a set/number chip distinguishes nothing there — so the
common two-binders case costs no `card_detail` at all.

**One copy per ticked row, matching the pill.** Two ticks on one card are two
items and two copies; the same "the tray counts entries" rule the batch follows.
A count picker per row was considered and dropped: the tray's pill has never
counted copies, and a step that quietly could would make it lie. (**Reversed by
the P6-150 ruling, above**: the count picker is the feature, the pill still
counts entries, and the toasts count copies out loud so neither number has to
be inferred from the other.)

**But that is exactly where the batch's own toast stops being true, and review
caught it.** `moved_message`'s unit is the tray entry, where one entry is one
card is one copy — "Moved 3 cards (1 copy each)". A pick is a *stack*, so
ticking all three stacks of one Bolt is three copies of **one** card, and
reusing that sentence made the step's headline flow assert something false about
the user's collection. The second pass phrases itself instead
(`picked_message`): both numbers said out loud, neither inferred — "Moved 3
copies of 1 card → 🗂 Deck", "Moved 5 copies across 2 cards → 🗂 Deck" — and
counted from what the server reported *moved*, not from what was ticked, so a
refused pick is never claimed as a copy that landed. `MoveReport::moved_as`
takes the sentence and the drop-tokens from its caller for the same reason:
the two passes count in different units, and everything below that line is
identical. Pass 1's wording is untouched.

**Two more places the same premise leaked**, fixed with it:

- **A row said `Binder · 3 copies` while ticking it moves one.** It now reads
  `Binder · 1 of 3 copies` — the row is a checkbox, and labelling it with the
  stack's whole size invites the reading that ticking takes the stack. The
  dialog's description does say "one copy from each", but it sits above a
  scrolling list and cannot be the only place that is stated.
- **A partial submit told the untouched cards nothing.** Submitting with some
  asked cards unticked sets `answered`, which suppresses the cancel toast — so
  a user who ticked one of three heard about that one and nothing at all about
  the other two, while they stayed checked in the tray. `unanswered` is now the
  one sentence both exits share: cancelling is its no-picks case.

**Tray bookkeeping needed its own translation, and this is the bug that would
have shipped otherwise.** The second pass reports `held:` tokens; the tray holds
the `card:` entry those answer. `tokens_to_drop` matches tokens literally, so
without `answered_tokens` mapping picks back to their originating entry, a card
whose copies had just moved stayed checked in the pill forever. Mutation-verified
(below).

**Evidence.** `cargo test --workspace --exclude frontend --exclude three_rings`
358 passed / 0 failed (10 new pure tests: the skip split, the stack rollup, the
choices join, the pick→wire-item translation, the token translation, the row
label). Three mutations, three deaths, all in the browser:
`is_ambiguous → false` (the step never opens — killed both e2e tests),
`answered_tokens → []` (killed the tray assertion),
`StackPick::key → SelectionKey::Card` (the pick submits the oracle grain again —
killed the moved-copies assertions). Full `batch-move.spec.ts` +
`selection-tray.spec.ts` at `--workers=1`, with the base-parity triage recorded
in the task report.

**A fixture helper that could not promise what its name said.** `unownedCards`
asks `/my?limit=200` for "cards the user owns nowhere", which on today's
fixture is really "not in the first 200 rows" — a card came back free while
sitting in a third collection, and the new test's "how many places is this card
in" assertion read 3 where it had seeded 2 (correctly). Fixed in the test rather
than in the helper: `stackCount` derives the expected row count from
`GET /api/cards/{id}/holdings`, so the assertion tracks the database instead of
an assumption. Every other test in that file only counts copies per collection
and is unaffected.

### Stale selection-tray entries: pruned, not moved (P6-122, 2026-08-12)

`app/src/components/ui/selection_tray.rs`, `app/src/my/move_selection.rs`,
`app/src/shell.rs` — the staleness policy for a tray key that outlives what it
names, now that batch move writes.

**The critical finding: the server already handles a gone reference gracefully
for the threat model this task describes, and that was not new work.** The
"Batch move (2026-07-25)" entry above already built the pre-check this task
was scoped to add: `move_selection`'s server fn re-reads the caller's holdings
fresh (`holdings_of_oracle`) and resolves every entry against *that*, never
against anything the client cached, before `move_batch` ever runs. A key
naming a holding a stepper drove to zero, or a collection since deleted (its
holdings relocated by the delete's disposition, or — under Discard — hidden
with the soft-deleted collection; either way the deleted `collection_id`
no longer appears in `holdings_of_oracle`'s live rows), resolves to zero
candidates and is refused as `SkipReason::NoCopies`,
by name, never written. No wrong-write path exists either: every write is
addressed by the natural key (`collection_id, printing_id, finish, condition,
language, board`), never by a holding row's id, so nothing about a row being
deleted and recreated under a new id can misdirect a write. **The one gap that
genuinely needed closing was client-side reconciliation, not the server.**

**The residual TOCTOU race is deliberate, and re-closing it here was
rejected.** A microsecond window between the resolution read and `move_batch`'s
own write transaction can still abort the whole batch on one stale item rather
than skip it — "Batch move (2026-07-25)" already named this "deliberate"
(closing it needs a grain-aware batch write) and P6-114 already declined to
build the general fix. Re-opening either is out of scope for an S-sized task
revisiting client policy; the mitigations recorded there (a microsecond window,
an honest "nothing was moved" error, an intact selection the user can retry)
still stand.

**What actually changed, both client-side:**

1. **`tokens_to_drop`** (`move_selection.rs`) — post-move reconciliation used to
   drop only what moved (`remove_tokens(&outcome.moved)`), leaving every
   refusal checked "because it is still work to do." That reasoning holds for
   `Grain`/`ManyCollections`/`ManyPrintings`/`ManyBoards`/`AlreadyThere` — each
   names a real, still-actionable question. (P6-150 deleted `Grain` and added
   `Several`/`NotEnough`/`NoneRequested` on the same side of that line; the
   rule is unchanged.) It does not hold for `NoCopies`:
   the stack is provably gone, and there is no page to open that fixes it. The
   tray now drops `NoCopies` refusals alongside moves, so the pill stops
   counting something the server just proved gone.
2. **`SelectionState::prune_missing_collections`** (`selection_tray.rs`),
   wired at the shell — every time the sidebar's collection tree resolves (the
   initial load and every create/rename/move/delete refetch), any `Held` entry
   whose `collection_id` is no longer among the live ones is dropped. Free:
   the tree is fetched for the sidebar regardless, so this reads data already
   in hand rather than issuing a read of its own. `Card` entries are immune by
   construction — they name no collection.

**Deliberately not done: pruning a row driven to zero by the stepper.** This
already has a maintainer decision behind it — "Removed rows leave section
counts and selection immediately" (2026-08-10, P6-118) hoists `removed` to gate
only the *checkbox's* visibility, leaving the tray entry itself alone, "because
the tray already has a name and a toast for a selection that outlived its
copies... this is that case reaching the mechanism already built for it, not a
new one." Reversing that now would need a server read to validate a hidden
row honestly (the mechanism `holdings_revision` bumps for is a refetch
*trigger*, not a payload the tray itself can diff), which is the one thing this
policy avoids paying for. Left as documented, not silently re-litigated.

**Evidence.** `cargo test --workspace --exclude frontend --exclude three_rings`
324 passed, 0 failed (4 new: 2 pure prune tests in `selection_tray.rs`, 2 pure
`tokens_to_drop` tests in `move_selection.rs`, plus existing coverage). fmt
clean; clippy clean on all five gate lines (workspace-exclude, wasm frontend,
`app --features native`, `app --features hosted,component-bench`, `app
--features hydrate,component-bench` wasm). e2e: one new kill-verified test in
`batch-move.spec.ts` — selects a live pair plus a row the stepper's own
`POST /api/holdings/{id}/quantity` endpoint zeroes out *after* selection,
batch-moves, and asserts the pair moved, the dead entry was named and never
written, and the tray emptied rather than staying pinned at "1 card." Reverting
just the `tokens_to_drop` wiring reproduces the failure at the exact assertion
(`toHaveCount(0)` on the tray receives `1`); restoring it passes again. Full
serial run of `batch-move.spec.ts` + `selection-tray.spec.ts`: 12/14 passed.
The 2 failures (`a /my row held in one place resolves to that place and
moves`, `a /my row whose copies are all sideboarded moves off the sideboard`)
are pre-existing and unrelated — confirmed via the API that "Akki
Blizzard-Herder" (the card `unownedCards`' fixed skip offsets pick at those two
call sites) is already held via the permanent Inbox (5 copies, real dev-seed
data, not test residue), so the `/api/all-cards?limit=200` + `catalog/
search?q=z&limit=60` fixture-pool helper misclassifies it as "owned nowhere."
This is the same fixture-pool class already tracked in the e2e-suite skill's
baseline (batch-move `:315`/`:435` before this task's insertions shifted line
numbers to `:344`/`:464`); the dev DB carries dozens of `zz-e2e-*` collections
left over from unrelated past sessions, confirming a growing shared fixture
pool rather than anything this task's diff touches. No `zz-e2e-stale*`
collection from this task's own test was left behind.

### Catalog search: a facet click inside the query bar's debounce window (P6-086, 2026-08-12)

**The race.** `/catalog` has two writers of one `?q=`, and for ~250 ms after
every keystroke they disagree about what the query is. A keystroke arms the
query bar's debounce holding the box text *captured at that moment*; the URL
still holds the pre-typing query. The filter rail commits synchronously on
click, reading the URL — so a facet clicked inside that window navigated with
the pre-typing text, and then the timer fired and navigated again with the
pre-click text, silently undoing the facet edit a quarter-second later. The
box was left worse than the URL: the rail's navigation re-seeded it (rule 3 —
"the URL moved without us") to the facet-bearing query, and the timer's
navigation then set `self_pushed` to the typed text without re-seeding, so the
box read `bolt c:r` over a URL saying `bolt mv<=2`. Reset had the same shape
and was louder: it cleared the rail's terms and the debounce put every one of
them back.

Note the asymmetry that makes this one-directional. Rail commits *re-read* the
current query when they fire (before this fix directly off the URL; now via
`QueryBase::read`, which prefers the bar's pending text over the URL), so a
rail text field's own debounce rebases onto whatever landed while
it was armed — including a facet click. The query bar cannot do the same,
because its box text is the *whole* query string: re-reading it late would not
pick up a facet edit, it would overwrite one.

**Chosen semantics: reconcile, not cancel** (the task's option (b), done by
value rather than by a second navigation). A rail edit rewrites the query
bar's *pending* text when there is one, so the single navigation it already
performs carries both intents — what you typed plus what you clicked — and
then cancels the timer whose text it has absorbed. Cancelling alone (option
(a)) would have thrown away keystrokes the user had already made; option (c)
(merge at fire time) needs the same information one hop later and more
machinery to use it. Nothing flushes through a separate `navigate()`, so there
is no ordering hazard between two navigations and no extra history entry.

**Shape.** `PendingQuery` (`app/src/components/query_bar.rs`) — a `Copy`
newtype over `StoredValue<Option<{TimeoutHandle, String}>>`, provided by
`AppShell`. Shell-level is forced: context flows down the owner tree and the
two surfaces are on opposite sides of it (the bar is inside the `<Outlet/>`,
the rail inside `SidebarRail`). Only the *handle and text* cross the seam, never
the committer: `use_navigate`'s closure is neither `Send` nor `Sync`, but a
`TimeoutHandle` is a plain `i32` and a `String` is `Send + Sync`, so the shared
slot stays an ordinary `StoredValue` and no widget is pushed into local
storage. A `QueryBar` with no such context (a bench or test render) falls back
to a private slot and behaves exactly as before.

The rail reads it through `QueryBase` (`app/src/catalog/rail.rs`): `read()`
peeks without disarming, `consumed()` cancels. Two calls, not one, deliberately
— a rewrite over the pending text can *fail* (a half-typed `c:` in the box is
an ordinary mid-typing state), and a rail edit that refuses must leave the
user's keystrokes on their way to the URL rather than deleting them.

**Invariants preserved.** The box after a mid-window facet click shows the
merged query (`bolt mv<=2 c:r`) — via the existing re-seed rule, not a second
writer: the rail's navigation moved the URL without the bar, which is rule 3's
own case, the same one an ordinary (non-racing) facet click already relies on.
`text` still has exactly one writer, `QueryBar`; the caret rule (P6-068) is
untouched because no new write to `text` was added. History granularity is
unchanged (the rail's single navigation still decides push-vs-replace by
`was_searching`).

**Scope: catalog-only, verified.** `/my`, `/my/all` and `/my/collections/:id`
each pair a `QueryBar` with revision-driven writes, but nothing there is a
second writer of `?q=`: tray moves and tree mutations bump `holdings_revision`
/ `TreeManage::revision` and navigate to *paths*, never to a query. The only
other producers of a `?q=` URL on those pages are pager links, which carry the
payload's `q` forward unchanged. (Their own interaction with the debounce is
the P6-130 `displaced_by` family, which `/my`'s pager does not yet implement —
filed on the pager-extraction task `WB-01KZVHYHMG`, not folded in here.)

**e2e.** `filter-rail.spec.ts` — "a facet click survives the query bar's
debounce window @fast" and "Reset mid-debounce clears the filters instead of
restoring them @fast". Both assert they are genuinely inside the window (the
URL has not moved yet) before acting, which is what stops them passing
vacuously on a build where the debounce had already fired. Kill-verified on
base: the first ends at `?q=bolt mv<=2` with the facet gone, the second reverts
to the full pre-Reset query.

### The vendored Dialog traps Tab focus (P6-125, 2026-08-11)

`DialogContent` (`app/src/components/ui/dialog.rs`) gained a second `window`
keydown listener alongside Escape's, gated identically
(`overlay_stack::is_top`): while the dialog is open and topmost, Tab from the
last tabbable descendant wraps to the first, Shift+Tab from the first wraps
to the last, and — the cited symptom, `palette.rs`'s field-focus-on-open with
nothing installed to hold it — a Tab pressed while focus is still *outside*
the container (the trigger, focused by the click that opened it) is
redirected in rather than left to walk the page behind the scrim. Zero
tabbable descendants keeps focus on the container itself (now
`tabindex="-1"`, so `.focus()` has somewhere to land). One fix at the
`DialogContent` level covers all six current dialog instances across three
files with no per-consumer wiring: `app/src/my/tree_manage.rs`'s
create/rename/delete/move dialogs (four), `app/src/my/collection.rs`'s
`TeardownDialog` (one), and the command palette via `CommandDialog` (one).

**Mechanics.** The tabbable set is enumerated fresh on every keypress — never
cached, since dialogs re-render — via a standard selector
(`a[href]`/`button`/`textarea`/`input`/`select`/`[tabindex]`, each
`:not([disabled])`, tabindex explicitly excluding `-1`) filtered to elements
with real layout (`offsetWidth`/`offsetHeight` > 0). That filter turned out to
be load-bearing, not defensive: `CommandDialog` opens with
`show_close_button=false`, which only sets Tailwind's `hidden` class
(`display:none`) on the close button — the button is still in the DOM,
untabindexed, and would otherwise count as a real stop, breaking the
palette's "one tabbable" case below. The same filter also excludes a closed
sibling `Popover`'s native-hidden content (the delete dialog's two
disposition pickers), which matches the selector but carries no
`disabled`/tabindex marker of its own.

**Composition, the three points named in the task.**
- **`overlay_stack`/Escape**: the Tab listener shares Escape's exact gate, so
  a `Popover` opened on top of a `Dialog` (the delete confirm's pickers, per
  `P6-189`) keeps its own Tab order — the `Dialog` below is not topmost and
  does not intercept. The popover itself gets no trap of its own here (out of
  scope, "their own stories" per the task) — filed as a follow-up below.
- **The palette's autofocus** (`palette.rs`'s open `Effect`, focusing
  `#command-palette-input`): unchanged. The trap does not set initial focus;
  it only constrains Tab from wherever focus already is. `CommandDialog`'s
  palette has exactly one tabbable descendant (the search field —
  `CommandItem` rows carry `tabindex="-1"`, roving ↑↓ is their own navigation
  model, and the close button is filtered out per above), so this is also the
  trap's "one tabbable" edge case: Tab/Shift+Tab both keep focus on the field.
  Verified in `command-palette.spec.ts`.
- **`tree_manage.rs`'s four dialogs**: none fight the trap. Create/rename have
  no autofocus effect of their own at all (the trap's outside-container
  redirect is what puts focus inside on the first Tab); the move dialog's
  existing `focus_move_field` effect composes the same way the palette's
  does. Nothing in any of the four uses a custom `tabindex` that would
  conflict with the selector.

**Review round 1 caught a real hole: "focus inside the container but on
nothing tabbable" fell through to native Tab, uncaught — and it was routine,
not rare.** The first cut's boundary check only distinguished "focus outside
the container" (redirect in) from "focus inside, on a tracked tabbable, at a
boundary" (wrap) — it never asked what happens when focus is *inside* the
container but on something that isn't itself tabbable at all. That state
turned out to be reachable on every dialog by the most ordinary interaction
there is: clicking non-interactive chrome — a title, a description, plain
padding — has no focusable target of its own, so the browser's own focus
algorithm walks up the tree to the nearest focusable ancestor and lands there
instead, which is exactly `DialogContent`'s own `tabindex="-1"` (added by
this same task, for the zero-tabbables fallback). Repro: open "New binder
inside…", click the description line, Shift+Tab — focus walked backward onto
a tree link behind the backdrop. Fixed by recognizing that `idx: Option<usize>`
already carries everything needed to decide this, once containment is dropped
as a separate check: every element `tabbable_within` finds is, by
construction, a descendant of the container it was queried from, so
`idx.is_some()` already implies "within" — `None` correctly, and identically,
covers both "outside the container entirely" and "inside it, but not on a
tabbable." The `container.contains(...)` check is gone; `trap_tab` now
branches on `idx` alone.

**Surprise, found while writing the e2e test, not a regression.**
`DialogClose` sets `attr:aria-label="Close dialog"` unconditionally,
regardless of its children — so a dialog's "Cancel" button and its real close
(X) button share one accessible name. `getByRole('button', { name: 'Cancel'
})` and `getByRole('button', { name: 'Close dialog' })` both fail to
disambiguate them; the e2e tests below locate structurally instead (the close
button is `DialogContent`'s own direct child, `footer button` for the
footer's two). Pre-existing (this task did not add the `aria-label`), and out
of surgical scope — worth its own small a11y fix, filed below.

**Accepted papercut.** Clicking non-interactive chrome now blurs a
previously-focused field (the click lands on the container's own
`tabindex="-1"`, added by this task), so Enter-to-submit needs a re-click
into the field first if a stray click landed on the dialog's padding in
between — recorded on the `DialogClose` a11y follow-up task
(`WB-01KZSBB0GM`) rather than chased here.

**Bench.** `app/src/bench/dialog.rs`'s existing "Confirm move…" demo already
has three tabbable stops (close button, Cancel, Move) — enough to exercise a
forward wrap, a backward wrap, and an ordinary in-between Tab with no new
markup; extended with a one-line caption naming the sequence.
`end2end/bench-check.mjs`'s dialog section gained the trap assertions: open
via the trigger (leaving focus on it, *outside* `DialogContent` — the exact
"focus behind the scrim" scenario), Tab through all three stops in order,
Tab again to prove the forward wrap, Shift+Tab to prove the backward wrap.
`npm run probe:bench`: **CLEAN**.

**e2e.** Three new deterministic tests in `collection-tree-manage.spec.ts`
(`describe("Tab focus trap (P6-125)")`), against the create dialog — a Tab
cycle from an explicitly-focused Name field through Cancel → Create → wraps
to the close button; a Shift+Tab from the close button wrapping to Create;
and (added in review, pinning the round-1 fix above) "clicking non-interactive
dialog chrome does not leak focus past the scrim" — click the description
line, assert the container itself is focused, Shift+Tab, assert focus lands
on Create (not a control behind the backdrop), then the forward-Tab sibling
assertion (click the description again, Tab, assert focus lands on the close
button) — plus one composition test in `command-palette.spec.ts` confirming
the search field keeps the palette's own autofocus and neither Tab nor
Shift+Tab can move off it (the "one tabbable" case, for real, not just in the
bench). All four solo: **4/4 passed**.

**Regression runs, both consumer files, full file, `--workers=1`** (real dev
server, `http://127.0.0.1:3000`), re-run after the review-round fix added a
third test to `collection-tree-manage.spec.ts`:
- `collection-tree-manage.spec.ts`: 16/17 passed. The one failure ("Delete's
  card count is this collection's own, not the rolled-up subtree", `copiesIn`
  expected 1, got 2 the first run and 3 the re-run — consistent with a
  shared-pool count still drifting between runs, not a fixed value) is the
  skill's own enumerated residual — "tree-manage :419 (pool-growth count)" —
  at its original line 419 on `main`; this task's own new describe block
  shifts it to line 499 (two tests) then 542 (three tests, this diff's final
  state), same test, same fixture-pool cause, not a regression.
- `command-palette.spec.ts`: 16/19 passed (unaffected by the review-round
  fix — no lines added to this file since). The three failures ("Undo last
  move reverses the move another surface just made", "a move already undone
  from its toast is no longer `the last move`", "`Undo last move` after Empty
  deck reverses the teardown, not an older move") are the enumerated
  "command-palette :442/:484/:524" residuals — this task's own new test above
  them shifts each by the same +29 lines (442→471, 484→513, 524→553); all
  three fail identically, `unownedCards` reporting 0 free against the shared
  dev-branch pool, the documented fixture-pool class, not the trap.

Every observed failure across both full-file runs matches the skill's
baseline enumeration exactly (line-shifted by this diff's own insertions);
zero failures outside it.

**Real-app check.** Exercised against the real running dev server
(`cargo leptos watch --features component-bench` on `127.0.0.1:3000`), not
just unit-level: the bench probe (`npm run probe:bench`, driving a real
Chromium page over CDP) and the e2e runs above (same server, real login
session, real Neon dev-branch collections) both count — no separate manual
probe was needed beyond these.

**Verification.** `cargo fmt --all -- --check` clean; the gate's own clippy
lines all clean — `cargo clippy --workspace --exclude frontend --all-targets
-- -D warnings` (native workspace incl. `src-tauri`/`three_rings`, run for
real on this macOS host, not skipped the way the web-dev container would
skip it), `cargo clippy -p frontend --target wasm32-unknown-unknown -- -D
warnings` (wasm hydrate crate), `cargo clippy -p app --features native
--all-targets -- -D warnings` (native backend, masked by `hosted` in the
workspace line), `cargo clippy -p app --features hosted,component-bench
--all-targets -- -D warnings`, and `cargo clippy -p app --features
hydrate,component-bench --target wasm32-unknown-unknown -- -D warnings`
(bench code, both halves); `cargo test -p app --features hosted`: 285
passed, 0 failed, 4 ignored (DB-gated, untouched — no new Rust unit tests,
the trap is DOM-driven `hydrate`-only logic with no pure-function core to
extract, same reasoning `context_menu.rs`'s own keyboard roving-focus code
was never unit tested either).

**Follow-ups filed, not absorbed.** (1) `Popover` gets no Tab trap of its
own — a `Dialog`'s nested disposition pickers (the delete confirm) are
untrapped while open on top of an already-trapped dialog; explicitly out of
this task's scope ("their own stories"), but worth its own task once
`popover`'s other open items are picked up. (2) `DialogClose`'s
`aria-label="Close dialog"` collides with the real close button's — same
accessible name for two different controls on every dialog that uses
`DialogClose` with custom text ("Cancel", "Done", …). Pre-existing, found
incidentally while writing this task's e2e tests, not fixed here (out of
surgical scope) — worth a small follow-up giving `DialogClose` its own
default `aria-label` (or none, letting its children supply the name).

### Teardown toast gains Undo — the phone's only reversal path (P6-031, 2026-08-10)

The standing gap: "Empty deck…" wrote its receipt's `move_ids` straight to
`note_last_move` for ⌘K's `Undo last move` and then dropped them — the only
reversal was desktop-only (the palette does not exist below the `768px`/
`pointer:fine` gate, `DESKTOP_MEDIA` in `palette.rs`), so a phone had no way to
undo a teardown at all.

**Reused the tray's reversal, not a new one.** `TeardownDialog::submit`
(`app/src/my/collection.rs`) now keeps its own clone of `receipt.move_ids` and
adds `.action("Undo", …)` to the success toast when the vec is non-empty (an
empty teardown has nothing to undo, same guard `LastMoveState::note` already
applies). The action calls a new `undo_teardown` closure built on
`crate::undo_selection_move` — the one-transaction batch undo the tray's Undo
and ⌘K's fallback path both already call, since a teardown is N ledger rows
exactly like a tray move. Always the batch endpoint, never the single-move
`undo_move` ⌘K falls back to for a length-1 batch: unlike `undo_removal`,
teardown has no single row to rewire from a receipt, so there is no reason to
special-case a count of one.

**Review round 1 caught a real regression: the failed-undo path deleted the
only reversal affordance.** The first version called `forget_last_move`
unconditionally before dispatch and never restored it on failure — and the
part that made this a MAJOR rather than a missed nicety is that the toast
itself was never a fallback either: `Toaster` dismisses a toast the instant
its action button is clicked (`sonner.rs`'s `on:click` runs the callback,
then `dismiss`es, *before* the request even resolves). One flaky
`undo_selection_move` call and the teardown became unreversible from any UI
at all — strictly worse than before this task, when a desktop session at
least always had ⌘K. Fixed to mirror ⌘K's own `UndoLastMove` handler
(`palette.rs`) exactly: on `Err`, restore the record via
`LastMoveState::note` if nothing newer arrived meanwhile
(`state.0.get_untracked().is_none()`, the same guard ⌘K's own retry uses), so
a failed dispatch leaves ⌘K reachable as the fallback instead of stranding
the user. `LastMoveState::forget`'s own doc comment (`palette.rs`) repeated
the same false claim — "the toast that started it is still on screen with
its own button" — and is corrected alongside the code.

**On success it now raises its own toast, in the tray's voice** — `"Put 1
card back"` / `"Put them back"` (`move_selection::undo`'s exact phrasing, not
⌘K's "Undid the last move": the closer sibling, same batch-undo shape).
Silent success was the same class of bug as the error-path one above: this
closure is designed to run *after* its page is gone, so a caller that only
ever sees an off-page failure and never an off-page success has no way to
tell whether Undo did anything at all.

**On success it also refetches the tree and bumps `HoldingsRevision`, not
`view_res` directly** — a deliberate divergence from the forward `submit`
path immediately above it, which refetches `view_res` directly because it is
still running synchronously on the page that just tore the deck down. The
toast's Undo can fire *after* that page is gone (the exact "toast outlives its
row" scenario `undo_removal`'s own doc names): `view_res` is this page's own
`Resource` and would be disposed with it, while `tree` (shell-level context)
and `HoldingsRevision` (also shell-level, and already a source `view_res`
itself depends on) are always live. This mirrors what `undo_removal` and
`move_selection::undo` already do for the identical reason, not a new
pattern.

**Error path**: `"Couldn't undo: {message_of(&e)} — try ⌘K → Undo last
move"`, `ToastKind::Error`. Now names the fallback explicitly, since after
the fix above it is a real, working one rather than an empty promise.

**Web e2e**: extended `removal.spec.ts` (the file already covering both
teardown tests) with `Empty deck's own toast Undo restores the deck, and ⌘K
stops offering the reversal` — teardown → toast carries "Undo" → click →
`grainsIn`/`viewRows` read-backs (not the toast) prove the deck's contents
came back → ⌘K opens, "undo" filters to `Undo last move`, ⏎ raises "Nothing to
undo yet" (`forget_last_move` landed). Full file **9/9** at `--workers=1`,
twice. One pre-existing, unrelated flake surfaced during those runs:
`a stale count-change toast's Undo does nothing once the row has been
removed` hit its own 90s `test.slow()` timeout twice running the full file
sequentially (reproduced identically with this task's diff `git stash`ed, so
it predates P6-031 and is not caused by it — same class of shared-dev-branch/
count-stepper timing sensitivity this file's own Findings already documents
for P6-117). Left un-investigated, out of this task's scope; its own
`finally` never ran on either timeout, leaving `zz-e2e-rm-stale-toast-*`
scratch collections behind — cleaned up directly through the API rather than
left for the next run to trip over.

**Android — real, not bench-only, for the first time on this platform.**
Every prior authed-surface Android probe in this repo (`android-collection-
check.mjs` on down) hit "the dev proxy strips Cookie headers and POST
bodies" (ui-work-loop.md → "Android dev-proxy limits") and fell back to the
`/dev/components` bench. That limit is specific to Tauri's `tauri://` /
`http://tauri.localhost` scheme handler proxying `devUrl` — `cargo tauri
android dev` already runs `adb reverse tcp:3000 tcp:3000`, so the device's own
loopback can reach the host server directly. A `page.goto` straight to
`http://127.0.0.1:3000/...` never enters that scheme handler at all: signing
in over that plain origin landed on `/my` with real session cookies, on the
real device, first try — nothing about the mechanism is teardown-specific, so
this reopens on-device authed verification for every future UI task, not just
this one. **Scoped honestly**: this exercises the real device WebView, its
cookie jar, and its touch stack (`Input.dispatchTouchEvent` under a real
`.click()`) — it does *not* exercise Tauri's `tauri://` scheme handler or the
`native` backend at all, since the request goes straight over plain HTTP to
the same `hosted` dev server every web e2e test already talks to. New
`end2end/android-teardown-undo-check.mjs`
(`npm run probe:android-teardown-undo`) drives the real flow end to end: signs
in on-device, creates a scratch deck + destination via the API (the same
authed `page.request`, absolute URLs — a CDP-attached page has no `baseURL`),
tears the deck down through the real dialog, asserts the toast's Undo button's
real on-device bounding box against the repo's real `TAP = 44` standard
(`android-tap-targets-check.mjs`) — measured 44×24 px at (421,1143) in a
540×1260 viewport: on-screen, not clipped, width clears the standard, height
does not (warns rather than fails; see follow-up 4 below — a component-wide
gap this task did not introduce and a teardown-scoped fix cannot close), taps
it for real
(`Input.dispatchTouchEvent` under the hood, same as every other android-*
probe's `.click()`), and reads the deck's contents back through the API
(present=2, matching the fixture) rather than trusting the toast. Reproduced
twice, identically. Two environment fixes needed to get here, both host-only
(this worktree's `.devcontainer/.env`/`.env`/`end2end/.env` were copied over
from the main checkout, gitignored, not a repo change):
- `mkdir -p target/site/pkg` — `src-tauri/build.rs` hard-codes this relative
  path regardless of `CARGO_TARGET_DIR`, so the `CARGO_TARGET_DIR=target/wb`
  this task's build-hygiene rule requires left the real directory missing;
  already called out in CLAUDE.md's merge-gate reproduction steps, just not
  wired into `cargo tauri android dev`.
- **The host's system JDK is too new for this Gradle/Kotlin-DSL pin.**
  `java -version` resolved to Android Studio's bundled JBR 25.0.2 (`JAVA_HOME`
  pointed there), and Gradle 8.14.3's embedded Kotlin compiler's
  `JavaVersion.parse` throws `IllegalArgumentException: 25.0.2` configuring
  `:buildSrc` — a version string newer than that Kotlin/Gradle pairing
  recognizes at all. Fixed by installing `openjdk@21` via Homebrew and passing
  `JAVA_HOME=/opt/homebrew/opt/openjdk@21` to `cargo tauri android dev` for
  this session only (no shell profile, no global config touched). Worth a
  `.devcontainer/README.md`/android-smoke skill note for the next agent on a
  host where Android Studio's JBR is the only JDK.
Also hit, and reverted before committing (the android-smoke skill's own
documented gotcha): the dev build injects a deep-link `<intent-filter>` into
`src-tauri/gen/android/.../AndroidManifest.xml`.
One harness-only artifact, not a product defect: navigating the on-device
webview to the plain `127.0.0.1:3000` origin logs
`Uncaught TypeError: Cannot redefine property: postMessage` (and
`__TAURI_PATTERN__`/`metadata`/`path`/`__TAURI_EVENT_PLUGIN_INTERNALS__`)
through `pageerror`. Isolated with a two-line repro (`goto` the plain origin
twice, no login, no teardown, nothing this task touches): Tauri's own
WebView-level IPC bootstrap re-injects on every navigation and tries to
redefine globals a prior injection already made non-configurable — it fires
on the *first* plain-origin navigation already, so it is a property of the
technique (a Tauri webview navigating to a foreign origin at all), not
anything about auth, teardown, or this diff. Not filed as a bug; noted here so
the next task reusing this technique isn't surprised by console noise its own
`pageerror` listener would otherwise report as a false failure.
No scratch data left behind: the probe's `finally` deletes both collections
every run, verified by an API listing showing zero `zz-android-p6031-*` rows
after.

**Verification.** `cargo fmt --all -- --check` clean; `cargo clippy -p app
--features hosted --all-targets -- -D warnings` clean; `cargo clippy -p app
--features native --all-targets -- -D warnings` clean (lints the `native`
backend `TeardownDialog` and `undo_teardown` compile against too — the
workspace/`hosted` line masks this feature); `cargo clippy -p app --features
hydrate --target wasm32-unknown-unknown -- -D warnings` clean; `cargo test -p
app --features hosted`: **284 passed, 0 failed, 4 ignored** (DB-gated,
untouched by this diff). e2e: `removal.spec.ts` full file **9/9** at
`--workers=1` (the one failure across two runs was the pre-existing,
unrelated flake above). Android: dev attach, real authed flow, **PASS** —
toast Undo present/sized/tappable on-device (width clears `TAP=44`, height
warns — follow-up 4) and the round trip verified through the API, reproduced
twice; see the probe's own run above for exact numbers.

**Follow-ups filed, not absorbed.** (1) `android-smoke` skill /
`.devcontainer/README.md` should note the JDK-too-new failure mode and its
fix, so the next agent on a host where Android Studio's bundled JBR is the
only JDK doesn't re-derive it from a bare `IllegalArgumentException: 25.0.2`.
(2) The plain-origin (`adb reverse`-exposed `127.0.0.1:3000`) sign-in
technique this task found should be written up as the android-smoke skill's
documented path for *any* future authed on-device verification — every prior
Android probe's "bench only, dev proxy strips cookies" caveat is now
avoidable, not a hard platform limit; note it exercises the device WebView
and touch stack only, not Tauri's scheme handler or the `native` backend
itself. (3) The pre-existing
`a stale count-change toast's Undo does nothing once the row has been
removed` flake (90s `test.slow()` timeout, reproduces with this diff
stashed) is unchased — worth its own investigation or an `@flaky` tag.
(4) **Every toast action button is under the repo's 44 px touch-target
standard on its height axis** — `ui/sonner.rs`'s `ToastAction` button
(`py-1` + `text-xs`) measures ~24 px tall regardless of caller, confirmed
on-device at 44×24 px for this task's own Undo button. Component-wide (every
`Undo` this file and `move_selection.rs` already raise shares the same
markup), not introduced by and not fixable from `TeardownDialog` alone —
`android-teardown-undo-check.mjs` warns rather than fails on it and points
back here. Worth its own task against `ui/sonner.rs`.

### Partial pulls stay in the walk with their residual (P6-119, 2026-08-10)

`app/src/my/needs.rs:~793` — `PickRowView::toggle`'s decision on a tick was
`let moved = !outcome.pulled.is_empty();`, a boolean that could not
distinguish "moved everything this line asked for" from "moved something".
Since [`Pulled`]'s doc already promises the honest count ("reported rather
than inferred"), any nonzero `pulled` entry struck the line through and
folded its token into `done` — a line asking 4 whose source had only 2 left
was marked fully pulled, and the residual 2 vanished from the walk with no
record it was ever owed.

**Root cause, traced rather than assumed.** The pick list is a snapshot
(module doc): each line's displayed count is `allocate(gap, locations)` at
the moment "Pull all…" was clicked. Between then and a given line's own tick,
the server re-derives `want` fresh from a live `needs()` read (`pull_needs`'s
own doc: quantity is never the caller's) — but that fresh `want` can itself
be *less* than the client's stale snapshot if the source's real stock
dropped in between (another tab, the collection view's own count stepper, or
simply an earlier line in the same walk touching the same card by a
different path). The server then moves exactly what it can, honestly, and
reports it — the defect was never a missing wire fact, it was the client
inferring "the whole ask" from "not empty".

**Determined `outcome.pulled` already carries what it needed to: no DTO
change.** `Pulled { token, copies }`'s `copies` is the real moved count,
server-verified end to end by the existing `POST /api/pull_needs`
duplicate-line check (Findings, "Needs view + pick list…"). What was missing
was not on the wire, it was a decision function on the client — extracted as
[`pull_line_outcome(asked, moved) -> PullLineOutcome`] (`Full` /
`Partial { residual }` / `Zero`), a pure, unit-tested classification so the
three cases are a closed match rather than an inferred boolean and an
implicit else. `asked` is deliberately the caller's own already-known
snapshot count, not a second server round trip — re-deriving it live is
exactly the "re-derive the source" step the floor below declines for size S.

**Presentation shipped: the stated floor, not the ideal.** On `Partial`, the
line stays unstruck, its own `RwSignal<i32>` count (`remaining`, new —
previously `row.copies` was rendered as a bare, non-reactive value) updates
to the residual, and the shared `report()` toast states the shortfall by
name: `"Pulled {moved} of {asked} {label} — {residual} still owed"`. Reworded
from an earlier `"… not found at the source"` (review): that phrasing
asserted a *cause* the client cannot know — a `NoLongerNeeded` skip beside
this same toast means the *gap* closed, not the source, and that skip
already states its own reason — and named a *single* source, which is wrong
on `ElsewhereRow`'s path, whose `asked` spans every source its allocation
named. The shipped wording is cause-neutral and number-only. What was **not**
attempted: re-deriving *where else* the residual
might now be fillable from (a different, non-dry source in the same
collection tree) — that needs a fresh `needs()` read and a new pick-list
line, which is a materially bigger change than a size-S fix carries. The
line simply keeps naming its own (now-partially-dry) source rather than
pointing at one with copies left, which is the explicitly-sanctioned floor:
"line stays unstruck, count updates to the residual, toast names the
shortfall." Re-ticking the same line after a partial pull is still live
(the checkbox is not marked `done`) and asks the server fresh each time — on
a fully-dried source that resolves to `SkipReason::NoLongerNeeded` (the
existing "gap closed" reason, imprecise here since it is the *source*, not
the gap, that closed — a pre-existing wording nuance, not introduced by this
task and not fixed here).

**Undo reverts the residual too.** A partial pull is still one real
`move_batch` entry, so Undo is unaffected — its callback additionally resets
`remaining` back to the original `asked`, so a partial-then-undo does not
quietly leave the line asking for less than it originally did.

**`report()` grew a fourth outcome (`asked: Option<i32>`) worth a different
sentence, not a second toast.** Considered and rejected: firing the existing
"Pulled N copies of X" toast unchanged *and* a second "shortfall" toast for
the same tick — two toasts stacking for one action reads as noise the
existing single-toast-per-outcome design (the function's own doc: "must not
word them differently") argues against. `report()` now computes the same
`pull_line_outcome` over the outcome's own total when the caller supplies
`asked`, and only *changes wording* on a genuine shortfall; a full match
(the overwhelmingly common case, and every existing passing e2e assertion)
falls through to the original "Pulled N copies of X" text unchanged. Wired
into **both** call sites — `PickRowView` (the snapshot count) and
`ElsewhereRow`'s one-tap Pull button (`fillable = row.owned_elsewhere`, its
own already-computed total ask across every source the row's allocation
named) — a free, low-risk consistency extension once `report()` already knew
how, since the row button can partially fill exactly the same way and
previously said nothing about it either.

**`report()` crossed clippy's argument ceiling (7) at 8 with `asked`
added; fixed by bundling, not by `#[allow]`.** `tree` / `revision` /
`last_move` always travel together at both call sites (same three local
variable names at each), so they moved into a new `Copy` struct
(`ReportContext`) rather than suppressing the lint — the repo has no
precedent for `#[allow(clippy::too_many_arguments)]` and one more
already-grouped concept was the cheaper fix.

**Test coverage: unit for the decision, e2e for the whole walk.** Three unit
tests pin `pull_line_outcome`'s three arms plus their boundaries (`moved ==
asked` is `Full`, not `Partial { residual: 0 }`; `moved > asked` — the source
restocked between snapshot and tick — is still `Full`, never a negative
residual; a defensive negative `moved` still resolves to `Zero`). The e2e
addition (`end2end/tests/needs.spec.ts`, "a line that finds less than it
asked for stays in the walk with its residual") sets up a real shortage
**server-side**, deterministically: generate the pick list at 4-from-one-
source, then drain that source to 2 via the raw `POST
/api/holdings/{id}/quantity` route (the same one the count stepper uses)
*out of band* of the page under test — modeling "another tab" without a
second browser context — before ticking the line. Asserts the toast's exact
shortfall wording, the label's residual count, the checkbox's own
`data-state="unchecked"` (a struck label beside a checked box, or the
reverse, would be its own lie the two assertions catch independently), the
real holdings read back (0 at the source, 2 at the destination — not just
the toast's claim), and that a second tick on the now-dry line is still live
and refused by name rather than silently re-doing nothing.

**Fixture debris, again — `q=z&limit=60` in this file was already fully
exhausted, not just tight.** This task's own new test was what tripped it:
measured live, `q=z&limit=60` now returns **0** free (down from P6-117/
P6-118's own draws against the same shared pool). Applied the identical
file-scoped remedy P6-118 used in `removal.spec.ts` — switched
`unownedCards` to `q=n&limit=200` (measured 112 free before this task's runs,
99 free after, still ample headroom) — local to `needs.spec.ts`. This puts
`needs.spec.ts` on the **same** `q=n&limit=200` pool `removal.spec.ts`
already uses (P6-118), so the two are no longer isolated from each other's
draw-down either. Still on the exhausted `q=z`, untouched by this task:
`batch-move.spec.ts`, `command-palette.spec.ts`, `collection-tree-manage.spec.ts`
and `collection-undo-restore.spec.ts` (the latter two at `q=z&limit=200` —
P6-118's own comment in `removal.spec.ts` names them as having already made
the `limit=60→200` bump, but not the `q=z→q=n` letter switch). The systemic
fix (one query with real headroom shared by every file, instead of each file
drifting to its own patched term) is not done here — filed as follow-up,
same as P6-118 left it.

**Zero-pull already worked, verified rather than assumed.** A token that
appears in `outcome.skipped` instead of `outcome.pulled` (`AlreadyThere`,
`NoCopies`, `NoLongerNeeded`) was never inserted into `done` even before this
task — `moved` defaults to `0` when the token is absent from `pulled`
entirely (a zero-copy line is reported as a skip, never as a `Pulled{copies:
0}`, per that struct's own doc), and `pull_line_outcome(_, 0) == Zero`
changes nothing. Pinned by a unit test rather than left as an unstated
assumption.

**Two accepted edges, recorded rather than fixed (review).**

- **Two live partial-pull Undo toasts on one line can restore a stale ask.**
  Each tick's Undo closure captures its own `asked` at click time and, on
  success, `remaining.set(asked)`. If the same line is ticked twice inside
  one 5s auto-dismiss window (two partial pulls in quick succession — the
  checkbox stays live and clickable between them, since a partial pull is
  never marked `done`), two Undo actions are live at once; clicking the
  **older** one after the newer tick has already moved `remaining` on
  overwrites it with the older, now-stale `asked` rather than a value
  consistent with the newer tick's own effect. Same class as P6-118's
  superseded-`section_delta` note: the write is harmless (no data
  corruption — only the *displayed* residual on an already-partial line is
  briefly wrong) and the window is bounded by the same auto-dismiss timing;
  it self-corrects the next time the pick list is regenerated (the list has
  no independent refetch path of its own — module doc). Not worth a guard
  for the same reason P6-118 gave.
- **⌘K's "Undo last move" bypasses `PickRowView`'s own `on_undo`, and now
  also leaves `remaining` stale, not just `done`.** `palette.rs`'s
  `UndoLastMove` handler calls `undo_move`/`undo_selection_move` directly
  against the ids `note_last_move` recorded — it has no reference to the
  ticked row's local signals, which are component state, not reachable from
  a global command. This was already true of `done` before this task (a
  fully-pulled, struck-through line reversed via ⌘K stayed struck until the
  pick list was regenerated) and the same gap now covers `remaining`: a
  partially-pulled line reversed via ⌘K keeps showing its post-pull residual
  rather than reverting to the original ask. Same bounded, self-correcting
  window as the edge above — pre-existing scope, not fixed here.

Verified: `cargo fmt --all -- --check` clean; `cargo clippy -p app --features
hosted --all-targets -- -D warnings` and `--features hydrate --target
wasm32-unknown-unknown -- -D warnings` both clean; `cargo test -p app
--features hosted`: 277 passed (274 baseline +
`moving_everything_asked_is_a_full_pull` +
`moving_less_than_asked_is_a_partial_pull_carrying_the_honest_residual` +
`moving_nothing_for_this_token_is_zero_and_leaves_no_residual_claim`), 4
ignored (DB-gated, untouched); `cargo test -p shared`: 34 passed (no wire
types changed, run anyway). e2e: `needs.spec.ts` full file, chromium
`--workers=1`, 7/7, run twice for stability (a default-parallelism run first
surfaced a **pre-existing** cross-test race in this file's own
`unownedCards(request, n)` helper — concurrent workers computing the same
"first free card" before any of them had added holdings for it, corrupting
each other's fixtures — not a regression from this task; the skill's
`--workers=1` requirement for a task's e2e run exists for exactly this
reason and the file has never been safe to run at default parallelism).

### Removed rows leave section counts and selection immediately (2026-08-10)

P6-117 fixed the removed row's own stepper and id; P6-118 fixed the two
surfaces around it that still lied — the deck section header and the row's
own selection checkbox — both in `app/src/my/collection.rs`.

**Defect 1 — the section header summed static data.** `section_slots` (a deck
section's own `label · N` count) is computed once, from `row.present`, when
the payload loads inside `group_deck`. `here_delta` (the page header's own
optimistic adjustment) is page-global, so a removal or a stepper commit moved
the page header instantly but left the section header reading the pre-commit
number until an unrelated refetch — a screen where the row said "—", the page
header said one thing, and the section header said another.

**Fix chosen: a per-section delta, the section-scoped twin of `here_delta` —
but the unit pushed into it is slots, not copies.** `RwSignal<i32>`
`section_delta`, one per rendered `DeckSection`, created fresh each time the
deck's sections are built (which is to say: fresh every time `view_res`
resolves — see the module doc on why a commit never triggers that).
`HereCount` now takes an `Option<RwSignal<i32>>` (`None` in a binder, which
has no sections) and updates it at every point `here_delta` itself is
touched — `remove`, its error rollback, `undo_removal`, and `on_commit`'s
partial-edit path and its error rollback. The header renders
`section_slots_live(slots, section_delta.get())` — `slots + delta`, unit
tested — instead of the static `slots.to_string()`.

**Review round 1 caught a real bug here: the first cut pushed the raw copy
delta, and `section_slots` is not a copy count.** Per `(oracle, board)` it is
`held + max(desired - held, 0)`, which is exactly `max(held, desired)`
(`section_slots_count_a_split_card_once`'s "desire 4, three held → 4 slots"
already pinned this — it just wasn't connected to what the delta needed to
be). A raw copy delta corrupts the header whenever the row is wanted and
under-held, which is the *ordinary* deck row since decks are Want-led: a
4-held/4-desired row stepped to 2 pushed −2 and read "2" — the truth is
still 4 slots (2 held + 2 still lacking), and the header would have snapped
back to 4 on the next unrelated refetch. (The WANTED cell corroborates only
in the under-held variant; at 4-held/4-desired it collapses to "—".) The
withdrawn claim was that this delta mechanism "matches the page header
exactly" the way `counts_summary` deltas `totals.present`; that analogy does
not hold — `counts_summary` never mixes a delta into a desire-absorbing
figure, because `here`/`present` and `desired`/`missing` are reported as
separate numbers on the page header, never combined into one. The section
header combines both into a single `label · N`, so it needed its own correct
unit, not a borrowed simplification.

**The real rule: `section_slot_delta(old, new, desired) = max(new, desired) -
max(old, desired)`** (unit tested against the exact case above, its removal
and undo counterparts, an over-held case, and the `desired == 0` case where
the slot delta and the copy delta genuinely coincide — the case the first cut
implicitly assumed applied everywhere). `desired` is read from the row itself
at commit time (`CardRow::desired` is oracle-grained, so every row already
carries its group's true value) and threaded into `HereCount` as a new
required prop. **Deliberately per-row, not per-`(oracle, board)`-group**: a
row's own commit only ever changes its own `present`, so treating this row's
held count as standing in for the group's is exact whenever the row is the
group's only holding — overwhelmingly the common case — and is the same
approximation `section_slots` itself would already make if two printings of
one wanted card were edited independently in the same section; a fully
group-aware recompute would need every sibling row's live present threaded
up and re-summed on every keystroke for a rarer case than the one this bug
was in, so it was not pursued.

**Defect 2 — the checkbox was computed once, from pre-removal data.**
`selectable` in `CardTableRow` was `(present > 0).then(...)`, evaluated once
at render from the row's static `present`. A removed row kept its checkbox
live; ticking it selected a row with nothing left to move, and the tray's
`NoCopies` refusal ("has no copies left to move — reload the page") was the
first anyone heard about it.

**Fix chosen: hoist `removed` one level up, gate visibility with it.**
`HereCount` used to create its own `removed: RwSignal<bool>` internally
(needed for its own "—" fallback and for `stale_commit_should_be_dropped`,
P6-117). It now arrives as a required prop, created instead by `CardTableRow`
— the same signal, just owned one level higher so the row's checkbox can read
it too. `row_selectable(removed, present)` (`!removed && present > 0`, unit
tested) decides visibility reactively; whether a checkbox exists **at all**
stays the static, non-reactive `present > 0` check it always was (a
desire-only row never gets one, unchanged). Undo needs no extra wiring: it is
the same `removed` signal `undo_removal` already flips back to `false`, so
selectability returns the instant the stepper does.

**First cut used a second mount/unmount (`{move || selectable.get().map(...)}`,
a `Signal::derive` over `removed`) instead of a visibility toggle, and it
broke a passing e2e test.** `HereCount`'s own `<Show when=!removed.get()
fallback=...>` already disposes the count stepper's reactive scope on this
exact `removed` flip — that's the documented, pre-existing
`count_stepper.rs:416` disposal race P6-117's Findings already describes
("a genuine, pre-existing, unrelated wasm panic … reproduces identically
against unmodified `main`"). Landing a *second* structural mount/unmount in
the same reactive flush was measured to make that race fire far more often in
a standalone repro script (5/5 panics, vs. needing `page.route` trickery in
P6-117's own attempt) — enough to make `removal.spec.ts`'s pre-existing
"stale count-change toast" test look like a new, deterministic regression at
first. **It wasn't one**: `git stash`-ing this task's diff and running the
*real* Playwright test (not the standalone script, which turned out to have
different-enough timing to be a poor proxy) five times each way measured
comparable failure rates on both sides — 1/3 on unmodified `main`, 1/5 with
this task's `style:display` version — consistent with a pre-existing,
timing-sensitive flake neither version eliminates or reliably worsens, not a
regression either version introduces. The evidence is recorded here because
the standalone script *did* make it look deterministic, and a future reader
re-running just that script would draw the wrong conclusion. Switched to a
`style:display` toggle (`contents` ⇄ `none`) on a wrapper around an
always-mounted `SelectionCheckbox` regardless — it patches one style property
instead of tearing down and rebuilding a component subtree, which is the more
defensible choice even though it did not measurably change the pre-existing
rate. The `count_stepper.rs` race itself is unchanged, still filed under
P6-117, still out of this task's surgical scope.

**The already-selected-at-removal question, and the pattern it follows.**
What happens to an in-tray selection whose row gets removed out from under it?
Left alone — not force-cleared. `move_selection.rs`'s `SkipReason::NoCopies`
doc already names this exact case: "Nothing is held any more — the selection
outlived the copies (another tab, **the stepper**, or simply a tray left open
a long time)." The tray's own convention for a stale selection is "stays
checked, refused by name at move time" (`Skipped`/"Refusals are reported,
never dropped" — the same module's doc), never a silent client-side drop.
Making the checkbox reactive stops a **new** selection of an already-removed
row; it says nothing about an existing one, which is exactly right —
inventing a clear-on-removal path here would be a second, competing story for
the same "the row you selected is gone" case the tray already tells one way.
Covered by this task's own e2e test (select, then remove, then assert the
tray still reads "1 card").

**Two accepted edges, recorded rather than fixed (review round 1).** A
`section_delta` belonging to a render `view_res` has since superseded is a
signal nobody reads any more — Leptos still lets it be `update`d (the
`RwSignal` itself isn't disposed until the whole `Suspend` subtree is), so a
stale toast's Undo firing after an unrelated refetch moves `here_delta` (read
by the *current* page header) but writes into a `section_delta` the *current*
section header no longer renders. The screen briefly shows the page header
and the section header disagreeing again, exactly the defect this task fixes
— except here it self-corrects on the next real refetch rather than needing
one specifically to fix it, since nothing is ever left permanently wrong. Not
worth a guard: the write is harmless (an orphaned signal nobody reads), and
the window is bounded by the same auto-dismiss timing P6-117's stale-toast
defect already lives inside. Separately: hiding a removed row's checkbox
(`style:display: none`) rather than disposing it means an *already-selected*
removed row's entry can only leave the tray via the tray's own "Clear
selection" or a move attempt's `NoCopies` refusal — never a row-level
uncheck, since there is no interactive control left on that row to uncheck
it from. This is the same shape the "already-selected-at-removal" decision
above already accepts (the tray, not the row, owns clearing a stale
selection), stated here explicitly because it is the mechanism's direct
consequence rather than a deliberate design choice made for its own sake.

**Fixture debris, again — `q=z` itself is exhausted, not just under-limited.**
P6-117 bumped `removal.spec.ts`'s `unownedCards` from `limit=60` to
`limit=200` on the same `q=z` search; by this task, `q=z` was measured to
match only 132 cards **total** in this POC catalog (a `limit` bump cannot grow
a query's own universe), and this file's own earlier tests alone drove
free-at-200 to 0 before reaching this task's new test. Switched `q=z` to
`q=n` (measured 152 free of 200, real headroom) — local to this file, so it
does not draw down the same shared `q=z` pool `batch-move.spec.ts`,
`command-palette.spec.ts` and `needs.spec.ts` still lean on; those are
unchanged, same as P6-117 left them, still out of this task's scope. The
switch resets the pool but not a sibling derivation flaw (review round 1):
`unownedCards`' own `taken` set is read via `/api/all-cards?limit=200`, so
once this account holds more than 200 cards, a genuinely-owned card past the
200th would misread as free — needs a dedicated fixture-hardening follow-up,
not fixed here.

Verified: `cargo fmt --all -- --check` clean; `cargo clippy -p app --features
hosted --all-targets -- -D warnings` and `--features hydrate --target
wasm32-unknown-unknown -- -D warnings` both clean; `cargo test -p app
--features hosted`: 274 passed (270 baseline + `row_selectable_…` +
`section_slots_live_adds_the_pushed_delta` +
`section_slot_delta_holds_a_wanted_under_held_row_at_its_desired_count` +
`section_slot_delta_is_a_plain_copy_delta_when_nothing_is_desired`), 4 ignored
(DB-gated, untouched). e2e: `removal.spec.ts` full file, chromium
`--workers=1`, 8/8 — green on the second of two runs, the first having hit
the pre-existing `count_stepper.rs` flake described above on the
*unrelated*, pre-existing "stale count-change toast" test (not this task's
new test, which passed both times); per the e2e-suite skill's quarantine
policy ("Flake → one retry"), retried and green.

### Same-device undo-flow defects: a stale stepper id and a stale toast (2026-08-10)

P6-117. Two defects in `app/src/my/collection.rs`'s `HereCount` (the count
stepper's collection-view host — "Custom gap components" above), both reachable
on one device inside the removal toast's own 5s auto-dismiss window.

**Defect 1 — the stepper's captured `holding_id` went dead across an undo.**
`undo_removal` optimistically restored the row (`removed.set(false)`,
`value.set(copies)`) then bumped a revision that refetches `collection_view` —
but that refetch was also the *only* mechanism that ever gave the row a fresh
`holding_id`, because undoing a removal re-inserts the holding under a **new**
id (`hosted::undo_one` → `holding_add`'s upsert). Between the optimistic
restore and the refetch landing, a +/- addressed the dead pre-removal id and
failed `"not found: holding"`.

**Fix chosen: the server tells the client the new id.** `undo_one` now returns
the id `holding_add` wrote to (`RETURNING id`), threaded up through a new
`shared::UndoReceipt { restored_holding_id: Option<Id> }` — the trait's
`undo_move: ApiResult<UndoReceipt>` (was `ApiResult<()>`), both backends
(`hosted`'s direct write, `native`'s HTTP client), the `undo_move` server fn,
and `HereCount`'s own `holding_id`, now an `RwSignal<Id>` instead of a plain
value so `undo_removal`'s success arm can `holding_id.set(new_id)`
immediately — closing the window synchronously rather than waiting on the
refetch. The revision bump stays (WANTED/OWNED and the header's totals still
come from `collection_view`), it just no longer carries the id.

**Rejected: gating the stepper until the refetch lands.** Considered and set
aside — the receipt was cheap to thread (one `RETURNING id`, one wrapper
struct matching the existing `MoveReceipt`/`TeardownReceipt`/`QuickAddReceipt`
convention) and closes the window *exactly*, where a pending/disabled stepper
would still have a visible dead window, just a non-interactive one, and adds a
second signal (busy) to reason about for no real gain.

Two other callers of the trait method needed touching, both mechanically:
`undo_quick_add` (server fn) discards the receipt (`.map(|_| ())`) since a
quick-add's move has no origin to restore; ⌘K's `Undo last move`
(`palette.rs`) does the same to keep its
`if move_ids.len()==1 {...} else {...}` branches unifying with
`undo_selection_move`'s `Result<(), _>`. Neither changes behavior.

**Defect 2 — a stale count-change toast's own Undo outlived the row's
removal.** `CountStepper`'s built-in commit-toast guards its Undo only on
`value.try_get_untracked()` succeeding (the row not disposed) and
`cur != from` — but removal deliberately does **not** dispose the row (that is
what keeps the *removal's own* Undo toast reachable). So a "3 → 1 · Undo"
toast raised before a removal stayed live after it, and firing it re-committed
into the holding `remove_holding` had just deleted, surfacing "Couldn't save:
not found: holding" — the write always failed, but the attempt and the bogus
error were the bug.

**Fix chosen: guard the caller's `on_commit`, not the generic stepper.**
`HereCount::on_commit` — the boundary the component's own doc already calls
out as "the caller owns persistence" — now opens with
`if stale_commit_should_be_dropped(removed.get_untracked()) { return; }`, a
new pure predicate (unit-tested) reusing the `removed` signal the row already
owns. **Rejected:** extending `CountStepper` itself with a
`retired`/generation-counter prop, the shape considered first — the guard is
specific to *this* caller's "removed" semantics, and the component's contract
(`caller_reports`, the doc comments throughout) already puts write-eligibility
decisions on the caller's `on_commit`, not the shared primitive; adding
removal vocabulary to a reusable stepper felt like the wrong layer for a
one-consumer concern.

**A genuine, pre-existing, unrelated wasm panic was found (not fixed) while
trying to make defect 1's race deterministic in e2e.** `page.route`-holding
`collection_view` open across an Undo click reproduced "you tried to access a
reactive value... it has already been disposed" at `count_stepper.rs:416`
(`holds_focus`'s `container_ref.get_untracked()`, reached from the deferred
`set_timeout(0)` a stray `focusout` schedules) — and it reproduces
**identically against unmodified `main`** (confirmed by `git stash`-ing this
task's diff and re-running the exact same probe), so it predates this task and
is `commitZero`'s own Enter-key-commit path racing a fresh remount, not
anything this task touches. Filed as follow-up, not fixed — out of this task's
surgical scope. Because of it, the deterministic e2e for defect 1 was dropped;
defect 1 is covered instead by the existing "the page followed the database...
the row is back with a live holding id" e2e assertions (eventual consistency)
plus the type-level id threading through `shared::UndoReceipt` /
`hosted::undo_one`. The attempt and the reasoning are recorded in
`end2end/tests/removal.spec.ts`, above the teardown section.

**Fixture debris, again.** `removal.spec.ts`'s own `unownedCards` was still at
`q=z&limit=60`; the shared dev branch's pool is now exhausted at that limit
(measured live: 0 free at 60, 51 free at 200) — the same problem
`collection-undo-restore.spec.ts` and `collection-tree-manage.spec.ts` already
worked around. Bumped to `limit=200` here too. **Not fixed, same reason, in
`batch-move.spec.ts`, `command-palette.spec.ts`, `needs.spec.ts`** — out of
this task's scope; `command-palette.spec.ts`'s three "Undo last move" tests
were spot-checked and fail at the identical setup assertion, unrelated to the
`palette.rs` touch here (a type-erasing `.map(|_| ())`, exercised indirectly
through the same `undo_move` wire path the passing `removal.spec.ts` tests
cover).

Verified: `cargo fmt --all -- --check`; `cargo clippy` clean (`-D warnings`)
across `-p app --features hosted --all-targets`,
`--features hydrate --target wasm32-unknown-unknown`,
`--features native --all-targets`,
`--features hosted,component-bench --all-targets`, and
`--features hydrate,component-bench --target wasm32-unknown-unknown`.
`cargo test -p app --features hosted`: 270 passed, 4 ignored (DB-gated,
untouched by this diff, not run — no DB work this task). e2e:
`removal.spec.ts` full file (7/7) + `collection-undo-restore.spec.ts`'s `Undo`
describe block (1/1), chromium `--workers=1`.
`collection-undo-restore.spec.ts`'s `Restore` describe block has 2
pre-existing failures (a "Recently deleted" row not found) — a file this diff
does not touch at all; not investigated further, flagged here for the
maintainer rather than silently absorbed.

### Responsive audit + stage close (2026-07-27) — **the Stage 3 boundary**

All nine wireframe frames reconciled against the running app at their own widths (3 desktop
at 1440, 5 mobile at 390, plus the hover-preview overlay). Every frame now conforms or
deviates deliberately with the deviation recorded; **one gap remains and is deferred by
maintainer decision** (below).

**Four touch targets fixed, all measured on the real webview rather than reasoned about:**

| | Before | After |
|---|---|---|
| Selection checkbox | control box **16×16**, cell 32 px, card-detail link 16 px away | target **44×44** (drawn box still 16×16), link 8 px beyond `target.right`, a 3 px-in corner tap selects without navigating; desktop unchanged; **44×44 CSS px at 540 px/dpr 2 on-device** |
| Toast over the tray's clear `×` | @1440 the toast box wholly contained the `×`; @390 with *no* selection it covered **35 px of the bottom tab bar** | @1440 clears the pill by 10.5 px; @390 clears the tab bar by 21 px and the pill by 10.5 px |
| Tray dock centring | pill centre **720** vs content-column centre **840** — off by half the 240 px rail | pill centre **840**, ≤1 px from the column centre; still full-width below `md` |
| Rail toggle | **27.8×26** — and it is the *only* touch route into tree management, since a long-press raises no `contextmenu` on the webview | **44×44** |

**The kill-verification `app-ui.md:1198` left unfinished is now complete, and the caveat there
can be struck.** Re-applying the class-stripping mutation made the corrected wrapper-scoped
assertion **fail** (`Depth Box's table scrolls sideways inside its wrapper / Received: 112`).
Measured across all nine collections under that mutation, wrapper overflow read
**58, 83, 99, 101, 112, 119, 128, 131, 148 px while `document.documentElement` read exactly 0
on every one** — so the original document-level assertion could not have failed. It also
caught this task's *own* 2 px regression mid-flight, so it works on real defects and not only
synthetic ones.

#### Adding SPA click-through to the audit found a second live collision, then a third mechanism

PR #74's bug hid behind direct page loads: every probe, every hydration check and the whole
210-test tier passed it because **they all load pages directly**. So this audit reached each
frame by *clicking* as well, and asserted content rather than just the route.

**It immediately found an active collision on `main`:** from a collection page, clicking
**Catalog** rendered *the collection's own cards as catalog search results*, with a false "N
results" count and **zero `/api/search_catalog` requests**. `SearchResults { cards:
Vec<CardSummary>, next_cursor }` cross-decoded `CollectionView` because neither refused
unknown fields and `CardRow` is a structural superset of `CardSummary`. Deterministic on 4 of
9 collections — the tile count equalling the collection's row count was the signature — and
**collection-dependent exactly like #75**, which is why a spot check would have cleared it.
Fixed with a named-field `SearchPayload`; verified **0 wrong out of 20** (10 collections × two
entry points), and regression-tested **red then green** (the red failed on the request
assertion with `Received: 0`, the bug's exact fingerprint).

**Then review overturned the audit's own conclusion, and this is the durable lesson.** The
scan that concluded "two payloads remain exposed" was for `Result<T, _>` and **structurally
could not see the `Option<_>` family — which is strictly more promiscuous, because a bare
`null` deserializes into *every* `Option`-typed resource regardless of inner type.** Four of
the six slots anonymous `/catalog` leaves behind are bare `null`, and `CardDetailPage`'s
resource is `Resource<Option<Result<CardDetail, _>>>` whose `None` arm rendered *"That card id
isn't valid."* — so catalog → click a tile → `/cards/:id` could claim a real card does not
exist, with zero requests, on the most common click-through in the product. The real inventory
was **eight** raw `Result` and **four** `Option`-typed, not two.

**Measured outcome: the mechanism is armed but does not fire today.** 60+ real click-throughs
across six origin queries and six tile positions, anonymous and authed — every one fetched and
rendered correctly. Flooding `__RESOLVED_RESOURCES[0..199]` with `"null"` reproduces the bug
exactly, and the landing id was pinned by **injection** (the inverse of the removal trick that
found the first two) at **64 anonymous / 66 authed** against 13 / 19 slots serialized — ~50
slots of headroom that is *an accident of how many resources `/catalog` happens to build*, not
a designed margin.

**The cross-shape class is now closed, by a mix of wrappers and structural facts rather than by
wrapping everything.** Five payloads carry named fields (`CardDetailPayload`,
`CollectionViewPayload`, `QuickAddPayload`, `CollectionListPayload`, `SearchPayload` +
`AllCardsPayload`). The skips were **tested, not asserted**: shell-level resources (`shell.rs`,
`tree.rs`) are created once per document so their ids cannot drift; `catalog.rs`'s count and
`rail.rs`'s set facet are `Option`-typed but their `None` is the *correct* default, so a
collision degrades to the honest resting state; `shopping.rs` and `needs.rs` already
self-discriminate on their DTO field names (flood-tested 4/4). Three universal promiscuous
keys — bare `null`, `{"Ok":null}`, `{"Ok":[]}` — are empirically defeated on every edge tested
(20/20: 5 SPA edges × 4 payloads that have actually collided in this repo).

**One skip was wrong and was caught by measuring it** — `destination.rs`'s `list_collections`.
Under an `{"Ok":[]}` flood the picker rendered **0 of 10 collections, no error arm, no
request**, silently claiming the account has none: **an array payload is promiscuous the same
way `null` is**, since `{"Ok":[]}` decodes into an empty `Vec<T>` for any `T`. Wrapped.

Two structural notes carried from it: **a wrapper whose only field is an `Option` is
decorative**, because serde defaults a missing `Option` to `None` so the struct still accepts
`{}` — `CardDetailPayload`'s field is a plain enum for that reason; and wrapping
`selection_destinations` was **declined with the cost stated** (it makes
`Resource<Result<Wrapper,_>>` non-`Copy`, turning the `Suspend` closure into an `FnOnce` the
view macro rejects) and is safe only because that resource is never serialized — confirmed by
dumping every `/my/*` route's slots.

**`cards.rs`'s dishonest arm was fixed independently of any id arithmetic**, because a `None`
meaning *"not fetched"* must not render *"That card id isn't valid."* — the same class the
states sweep had just removed elsewhere. `CardIdOutcome` names the two facts the old `None`
conflated: the route carried no parseable id, versus nothing arrived.

**The new catalog→card-detail test is a standing guard, not a caught regression, and says so.**
Since the collision does not fire today it was not red-then-greened; what *is* kill-verified is
the fix, and more strongly than one mutation would show — flooding all 200 slots breaks the
pre-fix build and leaves the wrapped build correct, so **a wrapped payload is immune at every
id, not just today's**.

**Same-type collisions remain the one open hole**, unchanged and now the only one:
`list_collections` has two consumers of one type, `/my` and `/my/all` share `AllCardsPayload`,
and two collection pages each mount quick-add. Closing it needs the payload to echo the request
it answered and the consumer to reject a mismatch. The durable fix above all of this is still
upstream — `initial_value()` honouring `during_hydration()` — and is filed.

**Two pre-existing table overflows found and filed, not fixed**: below ~385 px (9–14 px at 375,
64–69 px at 320) and 30–34 px at exactly 768 px where `md` adds the Type column. Neither is
this task's: measured by reverting its classes in the live DOM, this branch **reduces** the
tables' intrinsic width by ~20 px at every width below `md`, and at ≥768 px resolves to exactly
the pre-task values. Both are outside the nine frames' widths.

**Re-measured and fixed, P6-001 (2026-08-13):** both reproduced, numbers essentially unchanged
from July despite the layout churn between — see "Table overflow re-measured and fixed" below
for the driver of each, the fix, and post-fix numbers (zero overflow at 320/375/390/768/800 on
every seeded collection, plus 430/639/640/700/767/1024/1440 spot-checked).

Three review minors were **fixed rather than filed** because all three were this branch's own
work: the `px-1` compensations now switch at `md` alongside the select column they pay for
(measured 0 overflow at 320/375/390/430/639/640/700/767/768/1024/1440), the click-through loop
no longer slices four collections in tree order, and an inverted fixture-filter comment was
corrected. Two review corrections were accepted in the other direction: a kill-verification is
a transient mutation and is *correctly* absent from a diff, and Playwright's `[n/n]` count
includes the `setup` project.

**Operational lesson worth carrying:** the watch server's rebuilds were slow enough that two
intermediate measurements read **stale** output before it was caught. A stale read is
indistinguishable from a passing test, so measurements now poll on a marker in the served HTML
rather than on elapsed time.

#### The stage-boundary call

**`ui-work-loop` → `implemented`.** Nothing in this stage contradicted it; its contract — spec
reading before code, kill-verification, probes, the platform matrix, mirror sync — held
throughout, and its recorded stopping rule is what kept the boundary task from sprawling.

**`app-ui` → `implemented`, with one gap recorded rather than hidden.** The implementing agent's
blunt assessment was that it could *not* honestly be flipped, on two grounds; the first (the
catalog collision) is fixed above. The second stands: **the hover preview and the card sheet
both omit the `+ Want` / `+ Have` quick actions their frames draw** (`Preview Actions`,
`Sheet Actions`), which `design/information-architecture.md:78` names explicitly, and on a
collection row the preview is the *only* place those actions could live. **Maintainer decision
2026-07-27: flip and file.** So `implemented` here means *every screen exists and every frame
reconciles except those two drawn controls* — not that every drawn control is built. The gap is
a `[ ]` under Phase 5 discoveries with the adapters it should reuse named.

**The Android release smoke is maintainer-owned and was not run by the loop.** `artifacts.yml`
is `workflow_dispatch` only and the maintainer handles Tauri releases manually, so no release
APK was built or triggered and **no release coverage is claimed**. What *is* proven on the real
webview is the dev-attach path: seven probes green at phone width — tap targets, selection
tray, `/my` root, states, header kebab, tree move, and the ⌘K desktop gate correctly reading
false.

Verified at the boundary: gate **8/8** on macOS incl. `three_rings`, with **four** cargo
fingerprint cache hits (steps 3–6, the three feature-split lines that matter most for five
changed wire payloads) each called out and re-run against a scratch target dir outside the repo
to force genuine from-scratch checks — 198/293/164 crates compiled, `app` genuinely checked in
every one. Workspace tests **256**. Full chromium tier **249/249** at `--workers=1` (7.0 min).
Hydration CLEAN anonymous and authed, at default width and `PROBE_WIDTH=390`. Bench CLEAN.
**24 table surfaces measured at 390 px with zero document and zero wrapper-local overflow.**
A **39-edge SPA sweep** at both widths compared each destination's rendered content against the
API's own JSON; the four edges it flagged all reproduced clean in isolation and were harness
timing, reported as such.

### Empty / error / loading arms across the nine screens (2026-07-26)

An audit-then-fix sweep, not a feature. `app/src/components/states.rs` is the centrepiece:
five surfaces had hand-rolled the same `border-destructive/40` banner and each offered a
different amount of nothing.

**The load-bearing idea: the banner takes the *error*, not a message.** `ErrorNote`
classifies the wire prefix four ways and picks affordances from the classification —
`Missing`/`Request` **withhold** the retry (it would re-send the same doomed request),
`Transport` offers it, `Session` offers neither and links `/login?next=<here>`. A uniform
"Try again" everywhere would be a lie on most pages. Review verified the classifier against
reality: `shared/src/lib.rs:50-72` gives exactly six `Display` prefixes, `ApiError::from_wire`
reconstructs the same six on the native backend, and every other `ServerFnError` variant
carries no known prefix — so the `Transport` default is right for them. `Failure::Missing` is
split from `Request` because `ApiError::NotFound` carries a bare noun across ~21 call sites,
so appending the detail produced *"Couldn't load this collection: collection"*.

**Amendment (2026-08-12, P6-083): typed dispatch, prefixes now the fallback.**
The server-fn wire used to carry only `ApiError`'s flattened `Display` string
(`ServerFnError<String>`), so the six `Display` prefixes below were the *only*
signal a consumer had — `describe`/`classify` had to parse them out of the
message. The wire now carries the typed `shared::ApiError` variant itself
(`ServerFnError<shared::ApiError>`, via `crate::api_err`), and
`components::states::describe` matches the variant directly
(`Failure::of_api_error`) before ever touching a string. The `classify(&str)`
prefix table described in this section is retired to a **fallback** — it
still fires, unchanged, for the `ServerFnError` variants that never carried a
typed `ApiError` (a dropped fetch, a deserialization failure), which is
exactly the case its `Transport` default already covered. No banner's message
or affordances changed; only what decides them did. See
`specs/phase-6-probes/batch-H-leptos.md`'s P6-083 Resolution note for the wire
mechanics and the serde landmine the e2e run caught along the way.

**The dishonest states found, which were the point of the task:**

- **`unwrap_or_default()` on the collection list in *two* shipped pickers** — a failed fetch
  asserted the account has no collections. On the native backend an offline phone is the
  *normal* failure, so this was the common case, not an exotic one.
- **`/cards/:id`'s `LoadFailed` was a page with no link or control at all.**
- **An expired session rendered raw `unauthorized: invalid token`** in a red box — the exact
  symptom the e2e-suite skill documents as reading like a page bug.
- **An existing test asserted the dishonesty and called it honest.**
  `selection-tray.spec.ts:120` expected "No collection to move to." with a comment reading
  *"the collection list is a session read the anonymous page cannot make, so the honest
  rendering is the empty state."* It was not — the read 401s. Test and comment both
  corrected, and the on-device probe asserting the same string was corrected with them.

**Review found the one surface where the collapse survived — and that the new docstring
claimed otherwise.** `tree_manage.rs:798` flattened `_ => Vec::new()` with no `failed` prop,
while `destination.rs:239-244` stated both the tray and the tree's `Move to…` were safe.
With a failed tree read, `MenuTarget::for_collection` deliberately degrades `forbidden` to
`{self}` so the kebab item stays live, `move_rows` renders its unconditional `⬆ Top level`
row, and `CommandEmpty` never fires because the registry is non-empty — so the dialog
silently asserted root was the only destination and offered a reparent as the only action:
**a real write on a false picture.** A docstring contradicting its own code is the more
dangerous half, because the next reader stops checking. **`⬆ Top level` was deliberately
kept**, because reparenting to root is the one destination that never needed the tree —
removing it would be the CSS-hidden-fallback mistake from the mobile-`/my` review in reverse.
What it must not be is *alone and unexplained*.

**Three retry buttons bypassed the classifier the task introduced** (orchestrator promoted
these from minors as scope completion, not new work — a component whose premise is that
affordances follow the classification cannot ship beside three surfaces contradicting it):
`PickerBody` printed the raw wire detail and offered an unconditional retry, so an
`unauthorized:` failure read *"Couldn't load your collections: invalid token"* over a retry
that 401s forever — verbatim the defect the component exists to prevent; `cards.rs`'s retry
was unconditional for every non-`not found:` class; and `tree.rs`/`root.rs` **disagreed with
each other about the same shell resource** — one offered a retry, the other offered none.
`tree::tree_retryable` is now the single decision point, because two consumers of one read
diverging is incoherent by construction rather than merely inconsistent.

#### `Failure::Session` is reachable on every `/my/*` route, and the reason is a two-credential mismatch

Review could find no app consumer of the `Session` arm — every `ErrorNote` surface sits
behind `RequireAuth`, which bounces an expired session before the page renders. That
contradicted an *unplanned* live observation, and reconciling the two found the mechanism.
**The app carries two credentials with different lifetimes and different fallbacks:**

- `fetch_current_user` (`account.rs:320-344`), which `RequireAuth` awaits, tries `tr_jwt` and
  **on failure falls back to `tr_session` and re-establishes the session** — so an expired
  JWT with a live session yields `Ok(Some(user))` and **the guard passes**.
- `user_id_from_headers` (`auth.rs:233-239`), which every *data* server fn uses, reads
  `tr_jwt` or a Bearer header **only, with no session fallback** — same request, same expired
  cookie, `unauthorized: invalid token`.

The refreshed JWT lands on the *response*, so data reads inside that same SSR pass still see
the stale request header. **The window is 15 minutes wide and any idle tab hits it.**
Observed twice on two routes (`/my/shopping`, `/my/all`), each rendering
`data-failure="session"` while `__RESOLVED_RESOURCES[0]` carried the fully resolved user. So
it is not a route the guard fails to cover — it is a route where the guard **succeeds** and
the data layer doesn't. Pinned in a unit test with the mechanism in the test comment,
precisely so a future reader does not call it unreachable again. **The underlying auth
mismatch was filed, not fixed, here**: it was an auth-request-lifetime change, not a
state-arm rendering, and did not belong in a states PR.

**Fixed 2026-08-12 (P6-010)**, for the hosted (web) backend: `collection_backend()`'s hosted
arm in `app/src/lib.rs` now resolves the user id through
`user_id_with_session_fallback` instead of calling `user_id_from_headers` directly. On a
`MissingToken`/`InvalidToken` failure it reads `tr_session`, mints a fresh JWT
(`auth::upstream::mint_jwt`) and verifies it — `fetch_current_user`'s own fallback logic
(`account.rs:320-344`) minus the cookie writes, since `fetch_current_user` already refreshes
`tr_jwt` on the same response. The same helper was also dropped into the catalog's
opportunistic backend construction (`catalog_backend_with_fallback`, `lib.rs`), so `/catalog`
and `/cards/:id` ownership blocks survive the window too instead of quietly degrading to the
anonymous view. Full detail, including why the fix lives in `collection_backend()` rather than
`auth.rs`/`routes.rs`, in `specs/phase-6-probes/P6-010.md` → Resolution.

**The native backend already had an equivalent for collection reads/writes**
(`backend/native.rs:132-144`, a silent re-mint + one retry on a hosted `401`), so for that
half this brings the hosted web path in line with it rather than introducing a new
mechanism. The native *catalog* half has no equivalent — catalog reads degrade to
anonymous with a `200`, which the 401-keyed re-mint never sees, so the desktop/mobile
shell still silently loses `/catalog` ownership blocks after the idle window (filed as a
follow-up task). `unauthorized: invalid token` is still reachable and
still non-retryable (the pinned unit test at `app/src/my/tree.rs` is unchanged) — it now means
a session that is genuinely gone, no live `tr_session` either, which really does need a
sign-in.

**Where the previously-unused token variants went** — three tones, three different claims,
mapped to families and never to hex (the recorded WCAG tuning means the tokens carry four
deliberate deviations from upstream, so hand-picking colours would discard that work):
`Resolved`→`success` for needs-empty and shopping-empty, because *that* nothing is an
achievement and the opposite claim from `/my/all`'s "you haven't added any cards yet" — same
blank table, opposite meanings; `Partial`→`warning` for the `/my` root fallback rows and the
failed sidebar tree, which matters most where the rows *look* complete; `Stale`→`info` for
the catalog's dimmed last-good page, which was previously unlabelled and `aria-hidden`, so
nothing said *why* results sat under an error. **`ButtonVariant::Warning`/`Success` were
deliberately left unused** — a retry is neutral and a way-out is a link, and "warning" on a
button reads as *this action is risky*; they belong to destructive-adjacent confirm flows.
The gate confirmed `bg-success-light`/`bg-warning-light`/`bg-info-light` are emitted into the
release CSS with resolved colours in both themes, so no badge ships invisible.

**Traps worth carrying:**

- **`CommandEmpty` can only ever speak about *filtering*.** It infers emptiness from the item
  registry, so zero registered items conflates not-fetched / failed / genuinely-empty — the
  same collapse the set picker's four arms exist to refuse, and three pickers were leaning on
  it to describe a failed fetch. It **cannot** be fixed in place: the ⌘K palette's "No
  matches" depends on exactly that inference.
- **A caller-side `failed` flag must be Effect-written, and that makes it SSR-blind.**
  `ResultsToolbar`'s recorded lesson (hydration *claims* the server's text without rewriting
  it) rules out reading a resource in render, and Effects do not run during SSR — so a flag
  is only safe on a surface that never server-renders. That is why the sticky picker got a
  structural fix inside its `Suspend` and the tray and move dialog got flags; review verified
  the tray genuinely cannot server-render (`SelectionState::new()` is an empty `RwSignal` with
  no cookie/localStorage restore, and the tray is inside `<Show when=!items.is_empty()>`).
- **Any client-constructed `ServerFnError` must speak the `ApiError` prefix vocabulary.** Two
  (`needs.rs`, `collection.rs`) carried none, so the classifier read them as *transport* and
  would have offered a retry that re-parses the same broken string forever.
- **No wireframe frame specifies an empty, error or loading state** — checked across every
  string in `design/wireframes.pen`, so judgement was not overriding a frame anywhere.
- **The needs-empty arm remained unreachable by navigation** (the needs chip was its only link
  and was absent when nothing is missing), so its test reached it by URL — the same trap
  already recorded for that route. **Closed 2026-08-13, P6-143:** the chip now renders a
  neutral `success`-toned state instead of vanishing when a collection's desires are all
  met, so `/needs`'s "All set" arm has a real link again. Still true and unchanged: a
  collection with no desires at all still gets no chip either way, so that shape of
  "nothing" still says nothing.
- **Fixture limit, stated rather than papered over:** the e2e user's shopping list has 4 rows,
  so `shopping-empty` — the second `Resolved` consumer — has **no** honest e2e; it is covered
  by the bench section and the unit-tested tone mapping only. Emptying it would mean mutating
  shared fixture data.

**Amendment (2026-08-13, P6-163): `CommandEmpty`'s `loading` world, unused until now, closed
the tree move dialog's own double-state bug.** `failed` shipped wired for all three
`DestinationList` consumers above; `loading` shipped on the primitive at the same time
(P6-011) but nothing used it. The tree's `Move to…` dialog (`TreeDialogs` in
`tree_manage.rs`) put its own "Loading collections…" line *inside* `DestinationList`'s
`children`, as a `Transition` `fallback` sitting next to `CommandEmpty`'s registry-inferred
`empty` line — and while the tree read was pending, the registry had zero items either way
(nothing had mounted yet), so **both lines rendered at once**: "Loading collections…" beside
"No collection to move into.", the identical not-fetched/failed/genuinely-empty collapse
`failed` already existed to end for the failure half. Fix: `DestinationList` now forwards a
`loading` signal onto `CommandEmpty` too, with its own `loading_children` slot carrying the
centralized sentence (same pattern as `failed_children`); the tree dialog's own `Transition`
fallback is now `|| ()` — `CommandEmpty` owns saying "loading" once, not two components saying
it in two different ways. `command.rs`'s own module doc and `DestinationList`'s doc comment
both carry the full account.

**e2e finding worth keeping: a full page load never triggers a client-side fetch for
`collection_tree`.** `AppShell` (which calls `provide_collection_tree()`) wraps every
catalog/my-cards page, and on a full navigation its `Resource` resolves *during SSR* — the
value ships already-baked into the initial HTML/hydration payload, so `page.route` on
`**/api/collection_tree*` catches nothing (measured: zero `request` events on a fresh full
load of `/my`). The one place a genuine client fetch happens is the *first* client-side (SPA)
navigation into an `AppShell`-wrapped route in a given browser session — `/dev/components` is
the one route outside `AppShell` entirely (module doc, `AppShell`: "Auth pages and the bench
stay outside it"), so landing there first and then routing in client-side (an injected,
clicked anchor, since there is no on-page link to an arbitrary just-created scratch
collection) is what makes the fetch, and so the hold, real. Pinned in
`collection-tree-move.spec.ts`'s "loading state (P6-163)" describe block, kill-verified
against the pre-fix dialog (both lines present, `toHaveCount(0)` on the empty line failing
with `Received: 1`).

**Folded in (2026-08-13, same task, maintainer's couple-line rule):** the finding above was
flagged as an open follow-up first, then folded into this same task rather than filed
separately — the fix is the identical two-line change `tree_manage.rs` got.
`app/src/my/move_selection.rs`'s tray "Move to…" picker had the identical shape:
`DestinationList` wrapping its own `Transition` with an inline "Loading collections…"
fallback, `failed` wired but `loading` not. `MoveSelection` now tracks both resources behind
the picker's rows (`collections`, the list itself, and `suggested`, the ranking hint the same
`Suspend` awaits) into a `load_loading` `RwSignal` — `loading` has to be true until *both*
resolve, since the `Transition` fallback doesn't clear until they do — and passes it to
`DestinationList` the same way; the `Transition`'s own fallback is now `|| ()`. No new e2e:
the mechanism is already pinned by the tree dialog's kill-verified test above, so this is
regression-covered by `selection-tray.spec.ts` + `batch-move.spec.ts` (13/15 passing,
`--workers=1`; the 2 failures are pre-existing — reproduced identically against the
pre-fold-in code, unrelated to this change, and match the e2e-suite skill's documented
fixture-pool-class baseline noise for `batch-move.spec.ts`).

**No mutation pass was run** (switched off in the loop), so each new test is instead anchored
on a `data-testid`/attribute the fix introduced — `destination-error`,
`collection-error-home`, `tree-error`, `data-failure`, `data-tone`, `state-retry` — none of
which exist on `main`, so no assertion keyed on them could have passed before. Review
assessed that substitution and found it holds for all six. The major's fix was additionally
**kill-verified** by dropping the `failed` prop and watching `destination-error` fail to
appear.

Verified: gate **8/8** on macOS incl. `three_rings` with **no** cargo cache hits (every
compiled step showed a real `Checking`/`Compiling app` line), workspace tests **254**, full
chromium tier **234/234** at `--workers=1` (7.6 min), hydration CLEAN anonymous ×5 / authed
×6 / ×6 again at `PROBE_WIDTH=390`, bench CLEAN, `probe:android-states` PASS (all four
failure classes rendering their own affordances and no others, a real touch on Try again
moving the counter 0→1, and all three tone badges resolving to real dark-theme oklch colours
— a token family with no CSS behind it would have been transparent), and
`probe:android-selection-tray` restored to PASS after its stale assertion was corrected.

The `.agents/skills/` ⇄ `.claude/skills/` mirror had drifted again during this loop
(`e2e-suite/SKILL.md`, by the probe rows and the `diag:resource-ids` paragraph added by
earlier tasks in the same session). Resynced here; `diff -rq` clean on all six. This is the
second occurrence, which strengthens the already-filed case for the merge-gate assertion.

### Collection-header kebab, and a Leptos resource-id collision (2026-07-26)

`HeaderKebab` in `app/src/my/collection.rs` — a real `<button>` in a new `Header Actions`
cluster, wrapped in a second `ContextMenu id="collection-header"` whose panel **is `TreeMenu`
itself**, so the two menus' offered *and withheld* sets cannot drift (an e2e asserts both
panels' full ordered label lists for the same collection). Review confirmed the invariant
holds, including the Inbox withholding move/rename/delete on both surfaces.

**The task's headline result is not the kebab. It is that a second consumer of a shared
menu is exactly where "the tree refetch is enough" stops being true** — and, following that
thread, a live data-correctness bug on `main`.

#### The `/my` empty-state regression: a Leptos resource-id collision

Found as an aside while building this surface, believed pre-existing, and proven to be a
**regression from the immediately preceding PR (#74, mobile `/my` root)**. Symptom: signed
in with 100 present copies, an **SPA navigation** into `/my` — the sidebar's `All cards` row
or the breadcrumb root — rendered "You haven't added any cards yet" with **zero network
requests**. A direct `goto('/my')` was correct.

Mechanism, verified in the vendored source rather than assumed:

- `leptos_server-0.8.6/src/resource.rs:399-427` — `initial_value()` calls
  `shared_context.read_data(id)` with **no `during_hydration()` check**, for every
  `Resource::new`, at any time. `hydration_context-0.3.1/src/hydrate.rs:133` is a bare
  `__RESOLVED_RESOURCES[id]` read that does not consume the slot. If the slot decodes,
  `is_ready = true` and **the fetcher never runs**. `during_hydration()` /
  `hydration_complete()` both exist and are maintained (`hydrate.rs:145`,
  `leptos-0.8.20/src/mount.rs:97`) — `initial_value` simply ignores them. The real fix is
  an upstream one-liner.
- An `SsrMode::Async` page renders **three times** server-side and serializes at three
  disjoint id ranges; the client consumes only the **first** during hydration, leaving
  unclaimed slots as landmines for resources created during client-side navigation.
- `shared::AllCardsView { cards, next_cursor }` and `shared::SearchResults { cards,
  next_cursor }` are **byte-identical when `cards` is empty**, so quick-add's closed-panel
  `{"Ok":{"cards":[],"next_cursor":null}}` cross-decoded into an empty All-cards view.
- **#74's authorship was measured, not argued.** Temporarily deleting `<MyRootNav />` from
  `AllCardsPage` made the bug disappear; pre-#74 the resource landed on id 11, an
  unserialized hole, so it fetched. `MyRootNav`'s `<Suspense>` sits *ahead* of
  `AllCardsBody` and shifts its id by exactly +1.
- **The colliding slot was id 12**, pinned by identification-by-removal (dropping 8 or 16
  left it broken; dropping 12 fixed it). The review's derived guess of 10 was wrong while
  its payload-stride fingerprint was right — worth remembering as the difference between a
  correct mechanism and a correct address.
- **It was collection-dependent, which is why it hid**: Trade Binder and Bulk Box broke
  while Shoebox and Commander Deck worked, on identical serialized slots — the difference is
  where the client id counter sits after each page creates its own resources. A spot check
  on the wrong collection would have cleared it.

Fixed with `AllCardsPayload { all_cards: Result<AllCardsView, ServerFnError<String>> }`
carrying `serde(deny_unknown_fields)` — **a named field, chosen over re-numbering ids
precisely because re-numbering leaves the next identical-shape collision waiting.**

**The honest limit, recorded because it is not fixed:** a decode-layer refusal cannot close
the **same-type** case. `/my` and `/my/all` both resolve `AllCardsView`, so a correctly
typed payload answering a *different* `?q=` passes any type tag; closing that needs the
payload to echo the request it answered and the consumer to reject a mismatch. Two
general fixes were investigated and rejected as unsafe from app code: clearing
`__RESOLVED_RESOURCES` at hydration-complete **races streaming** (late chunks write into the
same array, so out-of-order routes would lose legitimate values), and clearing on navigation
is **too late** (Effects run after the new route's components are built, so the new resource
reads the stale slot first). There is no keyed or opt-out `Resource` constructor. Both the
echo-comparison and the upstream report are filed.

`npm run diag:resource-ids` (`end2end/measure-resource-ids.mjs`) is kept and documented in
the e2e-suite skill — it is the tool that pinned slot 12 and will pin the next one.

**Two predictions I made were disputed with evidence and the disputes upheld:** the
`/my` → `/my/all?q=…` corollary **does not reproduce** (the mechanism permits it, but at
today's id layout the resource lands on a hole or an incompatible slot — latent, not active),
and `CollectionPage`'s `view_res` is safe in both directions of collection→collection SPA
navigation. Also corrected: the vendored crate is 0.8.6, not 0.8.7.

#### The two defects the second consumer exposed

- **A mutation was invisible on the page you performed it from.** Title, counts and folder
  rows come from `collection_view`, so no `tree.refetch()` could update them: renaming left
  a stale `<h1>` beside an already-updated breadcrumb, and `New binder inside…` added a
  folder row that never appeared. Both read as "the action did nothing". Fixed with
  `TreeManage::revision` as a `view_res` source — the same structural trick
  `HoldingsRevision` plays.
- **Deleting the collection you are viewing left a dead id on screen.** `route_after_delete`
  walks up to the parent (`/my` at top level) and covers the **cascade**, since any route
  inside the deleted subtree is equally dead. Fixed in the shared `submit_delete`, so the
  pre-existing tree-row path is fixed too.

**Review found the delete carve-out from `revision` was too broad** — a second major. The
exclusion was justified as "delete navigates away instead", but that only holds when
`route_after_delete` returns `Some`. Standing on parent `P` and deleting *child* `C` from the
sidebar returns `None`, so nothing refetched `collection_view` and **`C`'s folder row stayed
on the page linking to a deleted id**, with its copies still in the counts. `submit_delete`
now matches on `leaving`: `Some(to)` navigates and must *not* bump (the remount refetches,
and a bump would refetch a deleted collection), `None` bumps. The doc comment asserting the
old carve-out was corrected rather than left lying.

**`Empty deck…` deliberately stays a visible button, not a sixth kebab item.** The kebab's
five are collection *lifecycle*; teardown moves the cards *inside*, which is where the code
already splits. The frame drawing this kebab draws a **binder**, so it cannot be read as
relocating a deck action, and burying a primary destructive affordance is a discoverability
loss no frame asked for. Review judged this defensible against the frames.

**`design/information-architecture.md:37` is stale**: it calls the Inbox "undeletable,
renamable", but `hosted.rs:492` carries `AND NOT is_inbox` on rename, so the API refuses.
The UI now follows the server on two surfaces. Filed — one of the two must be reconciled.

`Row Kebab` turned out to belong to the **`Card Row`** frame, not the tree — the per-card-row
move affordance, unbuilt, correctly left out of scope, along with the `Hdr Kebab Spacer`
that reserves its column. **None of the four filed `ContextMenuItem` minors crossed from
imperfect into broken** on this surface, so none were touched; review verified the two
load-bearing claims (each `ContextMenuContent` owns its own `open` and `restore_focus`, and
the window ESC/pointerdown listeners early-return on `!open`, so the closed instance cannot
interfere; cross-instance focus hand-off is ordered correctly because `open_at` defers a
macrotask).

Deviations from the frames, all deliberate: `size-11` (44 px) hit area below `md` because the
frame's bare 18 px ellipsis is the *look*, not the target (measured 44×44 on-device);
`⋯` (U+22EF) rather than lucide `ellipsis` (no icon set vendored — already filed);
`text-muted-foreground` for both `$text-3` and `$text-2` (the app has no third text token);
`bg-background` rather than `#FFFFFF` (dark is the default theme); one element with
responsive classes rather than two (SSR cannot know the viewport); top-aligned rather than
`alignItems: center`, because the app's title group has three lines to the frame's two.

Verified: gate **8/8** on macOS incl. `three_rings` — with **four** cargo fingerprint cache
hits (steps 4, 5, 6, 7) each called out and re-run against a scratch target dir to force
genuine from-scratch work. Full chromium tier **223/223** at `--workers=1` (7.2 min),
workspace tests 245, hydration CLEAN authed ×4 and ×3 at `PROBE_WIDTH=390`, bench CLEAN,
authed SSR curls showing exactly one each of `tree-create/rename/delete/move` and **zero**
`role="menuitem"` until aimed (no cross-panel testid collision from two menus in one
document), `probe:android-header-kebab` PASS driving real touch — 44×44 target, aim-before-
open, clamped panel, item `on_select`. Both majors' tests confirmed **failing before** their
fix (`element(s) not found` for `all-cards-row`; the folder row still resolving) and passing
after. Eleven minors filed.

### Mobile `/my` root — the wireframe's collection drill-down (2026-07-26)

`app/src/my/root.rs`. `root_rows(&AssembledTree, all_cards_href) -> Vec<RootRow>` is the
whole projection and is pure, running over the **same** `AssembledTree` the desktop rail
renders — same shell-level fetch, same Inbox pin from `assemble`, same sibling order, same
rolled-up counts — so the list and the rail structurally cannot disagree. New route
`/my/all` (`AllCardsTablePage`) gives the aggregate table a URL a phone can reach.

**`/my` emits both markups and CSS picks; the width is never resolved in code.** List below
`md`, table at `md`+. This is forced, not preferred: desktop `/my` must keep the table, the
frame requires the list at 390 px, and **SSR cannot know viewport width** — resolving a
media query in Rust would make the server's markup disagree with what hydrates. So the
absence of a hydration mismatch is *by construction*, not by care, and the new `PROBE_WIDTH`
env on `hydration-check-authed.mjs` makes it checkable rather than argued (`/my` is the
first page whose layout switches on a media query, and the probe could not see phone width
at all before). Review confirmed independently that nothing in the diff resolves a width in
Rust or JS, and that the two subtrees' `data-testid`/`id` sets are disjoint.

**The review's one major was a regression this task introduced, and it is worth
remembering as a shape rather than an incident:** a CSS-hidden fallback is not a fallback.
When `collection_tree()` errored on a phone, `MyRootNav` rendered "Couldn't load
collections." and the All-cards table beside it was `display:none`, so the document
contained **no link to `/my/all`, none to `/my/shopping`, and none to any collection** — the
rail drawer reads the same failed resource and the bottom tab points back to `/my`, so
My-cards mode was a dead end on touch. Before the CSS switch the same backend failure
degraded gracefully, because the table's read is independent of the tree read. Fixed with
`fallback_rows(all_cards_href)`: the two rows that never needed the tree read, built through
the *same* `all_cards_row`/`shopping_row` helpers as the happy path so the two cannot
describe different rows, rendered after an `role="alert"` line that blames the collections
rather than the page. `RootRow::count` became `Option<i64>` and the fallback carries `None`
— both totals come from the read that just failed, and a `0` would be a number the app
cannot vouch for; an absent count **omits its cell** so a test cannot read a missing number
as a rendered one.

**`page.route` cannot induce a server-resolved resource failure, and a test that tries is
vacuous.** `/my` is `SsrMode::Async`, so the tree resource resolves in-process and is
serialized into the HTML — a `goto('/my')` makes **zero** browser requests beyond the
document, measured. The working mechanism: `AppShell` (which calls
`provide_collection_tree`) is not mounted on `/dev/components`, so an **SPA navigation**
from there into the shell fetches `/api/collection_tree` over the wire, giving exactly the
tree-Err/all_cards-Ok state. The test asserts the interception actually happened
(`treeReads > 0`) so it cannot pass by not inducing anything. It failed before the fix with
`element(s) not found` — not "hidden", *absent* — and passes after. Independently
corroborated mid-run when the e2e `tr_jwt` expired and produced the real failure
server-side: an SSR curl showed the error line plus exactly two rows, `/my/all` and
`/my/shopping`, with `counts: 0`.

**One forwarded finding was disputed and the dispute upheld.** `h1:visible` was said to
have lost the strict-mode catch for a second visible heading; mutating `MyRootNav` to drop
`md:hidden` showed the *existing* assertion already caught it
(`strict mode violation: locator('h1:visible') resolved to 2 elements`), because
`toHaveText(string)` requires a single match. The added `toHaveCount(1)` is therefore
redundant with current behavior — kept for explicitness and to survive a later switch to
`.first()`, but it closed no real gap.

**A pre-existing, unasserted 3 px overflow was found and fixed.** With Type and Mana
dropped, the All-cards table's intrinsic width was 359 px against a 356 px wrapper at
390×844, because WANTED and OWNED are sized by their own header words. It was a
*wrapper-local* scroll — invisible to a document-level assertion, the same trap recorded at
line 1198 — present since `/my` shipped, and unhit only because the table was desktop-only
in practice; `/my/all` makes it a phone surface. Fixed with `px-1 sm:px-2`, measured 356/356.
**The first cut of this task reproduced the same class of bug**: `Separator` is `w-full`, so
`class="mx-2.5"` made every divider 20 px wider than its container and gave `/my` 2 px of
document scroll. The frame's own `M Divider Wrap` — a padded wrapper around a fill-container
rule — is the correct shape and is what shipped. **Rule: never put `mx-*` on `Separator`.**

**The frame's `Binders`/`Decks` rows are ordinary user collections, not synthetic groups.**
They read like categories, but `design/information-architecture.md:21-34` has them as
top-level binders holding `Trade`/`Bulk` and `Grixis`. Building groups would have invented a
data model; a unit test now asserts each carries a real collection id and a
`/my/collections/{id}` href.

**The rail drawer stays, unchanged.** The list replaced its *navigation* job, not its
*management* job — create/rename/move/delete hang off a tree row's `⋯` button and no frame
specifies a touch path for them, so removing or gating the drawer would take all four off
touch, the exact defect the tree-move task fixed. `TreeDialogs` remains at the shell. Two
overlapping navigations below `md` is a smell this task did not resolve; a `⋯` on the list
rows was *not* invented, because the frame's row is icon/label/count/chevron and the
repo's precedent is not to invent unspecified UI.

Deviations from the frame, all deliberate: the 30 px avatar is omitted (the shell top bar
already carries the account avatar one row above at every width — two would be wrong);
**emoji stand in for lucide** because no icon set is vendored, and 🗂/📁 render
near-identically so the aggregate-vs-collection distinction leans on font weight plus the
divider (filed — a real icon set is the durable fix); row metrics are `min-h-11` + `px-2.5`
rather than the frame's literal 13/10 px paddings, because the requirement is the 44 px
touch target (measured 47.4 px on the real webview); Selection Tray and Tab Bar were not
rebuilt (existing shell chrome already renders there); and no search field was added
because the frame has none. Three pre-existing specs were adjusted, all legitimately —
notably `selection-tray.spec.ts`'s one mobile test had to move to `/my/all` because
`openMy`'s `waitFor()` defaults to `state: "visible"` and the checkboxes now sit in a
`display:none` subtree at phone width.

Review: **one major** (above) and **thirteen minors**, filed. Verified: gate **8/8** on
macOS incl. `three_rings` — with the workspace clippy step's cargo **fingerprint cache hit**
called out and re-run against a scratch target dir to force a genuine from-scratch check
(467 lines, `Checking app`, exit 0), and `three_rings` confirmed genuinely lint-checked via
`--message-format=json` rather than inferred from an absent `Checking` line. Full chromium
tier **210/210** at `--workers=1` (6.6 min), workspace tests 239, hydration CLEAN at default
width and at `PROBE_WIDTH=390`, bench CLEAN, authed SSR curls on `/my` (one `my-root-list`,
8 rows, *and* the hidden `all-cards-table` with 50 rows, two `<h1>`s) and `/my/all` (50
rows, **zero** `my-root-list`), `probe:android-my-root` PASS driving a real
`Input.dispatchTouchEvent` tap that drills in, with the overflow check **kill-verified**
(requiring `TableWrapper` on `/my` correctly reports it was never measured).

### Tree move — a keyboard and touch path (2026-07-26)

`Move to…` as a fourth action in the tree's context menu (`app/src/my/tree_manage.rs`):
`MoveReq` snapshotted on open (following `DeleteReq`), `MoveTarget { TopLevel, Into(Id) }`,
pure `move_destinations(rows, forbidden)` and `plan_move(rows, req, target)`, and a
fourth `tree-move` dialog hosting the shared `DestinationList`. It commits through the
existing `reparent_collection` + `reorder_collection` adapters — no new endpoint.

**The task's own premise about the Inbox was wrong, and the code is right.**
`hosted.rs:578` is `UPDATE collections SET parent_id = $2 WHERE id = $1 AND NOT is_inbox`
— `$1` is the **subject**. The Inbox cannot be *moved*; it has always been a legal
*parent*, and the drag path already allowed dropping into it (`drop_intent` collapses
its bands to `Into`). So the picker offers the Inbox as a destination and `Move to…` is
instead withheld from the Inbox's own menu. Review verified the SQL independently.

**Three surprises, each bigger than the task line implied:**

1. **There was no keyboard route into the context menu at all.** Rows are `<div>`s whose
   only focusable children are the link and the collapse chevron, and the menu panel is a
   `popover="manual"` with no focus management — so Tab from a row walks the *document*,
   not the panel. Adding only the menu item would have left the task's own premise
   unsatisfied. Added: a real `⋯` `<button>` (`opacity-0`, **not** `hidden`, at `md`+ so it
   stays tab-reachable, with `focus-visible` bringing it back), the `ContextMenu`/`Shift+F10`
   chord on the row and on `ContextMenuTrigger`, and focus-on-open + ↑↓/Home/End roving with
   wrap + ESC-closes-and-restores in `context_menu.rs` itself.
2. **A real long-press does NOT produce `contextmenu` on the Android webview** — the repo
   asserted the opposite in three code comments since the menu was vendored. The
   2026-07-20 "verified on the real webview" run had **dispatched a synthetic `contextmenu`
   event**, so it tested the handler, not the gesture. Re-measured: a 1.2 s held touch with
   a tracked contact id yields nothing, while a tap on the same page produces a click. All
   three comments corrected. **This is why the `⋯` button exists** — without an explicit
   trigger a phone has no way into the menu whatsoever.
3. **The `hidden md:block` gap was worse than filed.** It was not just "tree dialogs are
   invisible below `md`": the entire `aside` was `display:none`, so **a phone had no
   collection tree at all**, and create/rename/delete rendered into a hidden subtree and
   never appeared. Fixed in scope, because "no touch path" is half this task's title:
   `TreeDialogs` hoisted to the shell, and the rail turned into a CSS slide-over drawer
   (`invisible fixed -left-60` → `data-[open=true]:left-0`, `md:static md:visible`) with a
   `md:hidden` toggle shown in My-cards mode only, so Catalog keeps its one designed mobile
   filter path. **`left`, not `translate-x`** — a transformed ancestor becomes a containing
   block, and `DialogContent` is a plain `fixed` div with `translate-x-[-50%]`, so a
   transformed rail would have re-based every dialog. Review confirmed both the claim and
   that desktop layout is untouched (`-left-60`/`top-14`/`bottom-0`/`z-50` are all inert
   under `md:static`). **One `CollectionTreeNav` instance**, deliberately not following
   Catalog's `FilterSheet` precedent of mounting a second `RailBody` — a second would
   duplicate the ids its `ContextMenu` and `Collapsible`s key off.

**Scope decision, stated plainly:** `Move to…` covers reparenting (including out to top
level) and lands the collection **last among its new siblings** via `reorder_collection` —
a defined spot a bare reparent lacks, since it would otherwise carry its old `position`
into the new group and fall arbitrarily or tie on name. **Reordering among siblings you are
already among stays drag-only, and therefore mouse-only.** No ordering UI was invented
because no wireframe specifies one; the gap is filed as its own follow-up rather than left
in a code comment.

**The `command` ordering caveat does not apply to this picker**, verified independently by
review: `move_destinations` sorts the *data* before any item mounts, `move_rows` emits the
unconditional `Top level` row first and then the collections in one pass (no conditional
section that could swap — the shape that bit ⌘K), and typing only flips each
`CommandItem`'s `is_visible` memo rather than rebuilding the list. Pinned anyway by an e2e
that reads the visible rows' DOM order, presses `↓` once and `⏎`, and asserts the
collection that was **second on screen** received the move.

`destination.rs` was **extended, not forked** for its third consumer: `DestinationRow`
extracted out of `DestinationOption` (which is now a thin pass-through), because the tree's
list has one row that is not a collection (`Top level`) and so carries no `Destination` —
the alternatives were a sentinel `Id` or a second copy of the row markup. `DestinationList`
gained an optional `input_id`. Review checked both existing consumers (catalog quick
actions, selection tray) and found the emitted markup unchanged.

**New focus rule in `context_menu`:** an activated item suppresses the focus restore,
because the action owns focus from there. Without it the closing menu's restore races the
move dialog's field focus in the same effect flush and dead-ends the keyboard path; the
dialog also re-focuses on a `set_timeout(0)`.

Review: **CLEAN, zero majors** — including a check that `commit_move`'s two sequential
writes have an honest failure window (reparent lands, reorder fails ⇒ the node is in the
requested parent carrying its old `position`, which is exactly what "Moved, but couldn't
set its order" says, and `tree.0.refetch()` runs on both branches), that subtree exclusion
is complete rather than direct-children-only, and that a stale `MoveReq` snapshot is
backstopped by the server's recursive ancestor check surfaced inline as a 409. Ten minors
filed. Review also verified that nothing in this workspace enables `leptos/delegation`, so
`element_anchor`'s `current_target()` resolves to the bound element — had delegation been
on, both the new anchors and the pre-existing `drop_intent` would silently return `None`.

Verified: gate **8/8** on macOS incl. `three_rings`, full chromium tier **198/198** at
`--workers=1` (6.4 min), hydration CLEAN anonymous ×4 + authed ×5, bench CLEAN with the new
keyboard block **kill-checked** (flipping an expected label made it report
`context_menu ArrowUp did not wrap to the last item`), authed SSR curl of `/my` showing 11
row heads with `data-tree-row-actions`, exactly one `id="tree-move"` and one
`id="tree-create"` (no duplication from the shell hoist) and **zero** `destination-option`
(rows correctly unmounted while closed, so no testid collision with the other two pickers),
and `probe:android-tree-move` PASS on the real webview driving `Input.dispatchTouchEvent`
— tap opens the shared panel, the panel is clamped inside the phone viewport, tapping an
item closes the menu *and* runs its `on_select`, with the closed rail drawer off screen and
Catalog's own filter trigger as the positive control. `/my` is unreachable on-device (the
dev proxy still strips cookies, so it redirects to `/login?next=/my`), which is why the tap
path is driven on the bench and why `TapTrigger` was added there.

### Move dialog — honest exits (2026-08-12)

`plan_move`'s three `None` exits collapsed into one meaning at the call site
(`commit_move`): `manage.move_open.set(false)` with a comment naming only the
"already there" case. Two of the three are not that — a destination gone
forbidden or gone entirely between the dialog opening and the pick — and
closing silently on either told the user their move landed when nothing was
ever sent to the server (P6-121).

`plan_move` now returns `Result<(Option<Id>, Option<f64>), MoveBlocked>`
(`Forbidden` / `Gone` / `AlreadyThere`), table-tested per variant.
`AlreadyThere` still closes the dialog with no toast — the picker's own ✓
already told the user this is a no-op, the same standing as Cancel.
`Forbidden` and `Gone` keep the dialog open with `manage.error` set (the same
`data-tree-dialog-error` line the other three dialogs use), never a silent
close.

Same pass, two related exits that also went silent, both traced to
`app/src/my/tree_manage.rs`'s `commit_move`:

- **The tree resource reads `None`.** `commit_move` re-reads
  `tree.0.get_untracked()` live at commit time, and a `None` there previously
  hit the same blanket `else { return; }`. **Not** a mid-refetch case — an
  earlier draft of this note claimed `tree.refetch()` could catch the signal
  "between values" while this dialog is open, which is wrong per
  `reactive_graph`'s own semantics: a refetch holds the previous `Some` until
  the new fetch resolves (`ArcAsyncDerived` never regresses to `None`
  mid-flight). `None` is only the *never-resolved* state, and `open_move`
  can't fire until a tree row has already rendered once — so this arm is a
  defensive fallback for "the tree has somehow never loaded", not a
  transient this dialog realistically hits. Kept anyway, with honest
  wording ("Still loading — try again.") rather than a silent close, the
  same call as the adjacent failed-read arm ("Couldn't load your
  collections — try again.").
- **`busy` held by another dialog.** `busy` is one `RwSignal<bool>` shared by
  all four tree dialogs (`TreeManage::busy`), and `Dialog`'s ESC closes an
  overlay without cancelling its in-flight request — dismiss a slow Delete,
  open Move to…, and the picker looked completely idle while every pick
  silently no-opped. Deliberately **not** made per-dialog (out of scope for
  this task): the move list now renders `opacity-50 pointer-events-none` plus
  a "Working on another change — try again in a moment." line
  (`data-testid="tree-move-busy"`) while `manage.busy` is true, matching the
  other three dialogs' `attr:disabled=move || manage.busy.get()` on their
  submit `Button` — this dialog has no separate submit, so the row list
  itself carries the disabled state. `commit_move`'s own busy check keeps a
  message (not a silent return) as a backstop for the race between a click and
  that render landing.

**Finding, corrected mid-task: `move_rows`' picker list is not frozen.** The
first pass assumed `Suspend::new(async move { tree.await })` resolves once and
never rebuilds off a later `tree.refetch()`, which would make a deleted
destination stay stuck on screen for the rest of the dialog's life — the
obvious way to reach `MoveBlocked::Gone` from real clicking. Built an e2e test
on that premise (delete an unrelated collection through the UI, capturing its
undo-toast refetch as the trigger; raw-API-delete the picker's own selected
destination in between; click Undo to force a second refetch after the
deletion; click the stale row) and it disproved its own premise: once *any*
refetch lands, the picker's rendered rows update as fast as the sidebar's
own — Leptos's `Suspend` does resubscribe to the resource it awaits. The
`Gone`/`Forbidden` arms guard a real but sub-reactive-tick race (a click
landing in the same flush as a refetch, before the DOM commits) that this
suite cannot make deterministic without reaching into the page's internal
signals, which is out of bounds for an e2e test. `Forbidden` is narrower
still: the picker filters by the exact same frozen `req.forbidden` snapshot
`plan_move` checks against, so a row the check would reject is never rendered
at all — reaching that arm from a real click looks structurally impossible
given the current implementation, not merely hard to time. Both stay
worthwhile as defensive classification (correctly typed instead of silently
merged) and are table-tested directly against the pure function, which is
where their kill-verified coverage actually lives.

The e2e-reachable version of "destination deleted mid-dialog" is the plainer
one: delete it before *any* refetch has landed, so the still-rendered row
sends the pick to the server, which 404s. That path was already correct
before this task (`(Err(e), false) => manage.error.set(user_msg(&e))`) — the
new test pins it as a regression net so a future refactor can't silently
re-merge it into the closed-dialog branch, but it does not kill-verify this
diff's own new code. What does: a second e2e test for the `busy`-visible-state
markup, using `page.route` to hold a Delete's request open on cue (the same
pattern `collection-header-kebab.spec.ts`'s race test uses) — deterministic,
and confirmed to fail on the pre-fix code and pass on the fix by literally
running it both ways (`git stash` around `tree_manage.rs`).

**Review caught a major in the `busy`-visible-state addition itself, same day
(P6-121, second pass):** `move_busy = move_open && busy` reads `busy` without
asking *whose* write set it. `commit_move` sets `busy` for its own commit too
— the same shared signal every dialog's submit uses — so every ordinary
successful Move dimmed the row list and showed "Working on another change —
try again in a moment." for the span of the write the click had just made.
Factually wrong (it *was* this change) and actively bad advice (the pick was
already committing; there was nothing to retry). Fixed with a second signal,
`TreeManage::move_committing`, set/cleared alongside `busy` in `commit_move`
and nowhere else; `move_busy` (and the parallel "still working" branch inside
`commit_move`'s own foreign-busy check) now reads `busy && !move_committing`.
During the move dialog's own commit this renders exactly as it did before any
of this task's markup existed: no dimming, no message — there is no separate
submit control here whose disabled state could stand in for one, so "nothing
new" is the honest baseline, not a gap. Also reworded the `Gone` message
(minor, same review) from "…pick another destination" to "That collection was
just deleted — pick another destination.": the stale row can still be the one
visibly on screen for the sub-tick window this arm guards, so the wording
must hold up whether or not it has already vanished from the list.

New e2e coverage for the fix: extended the busy-visible-state test with an
ordinary-commit case — hold *this* move's own `reparent_collection` open via
`page.route` and assert the "another change" line never appears while it's in
flight, alongside the pre-existing foreign-busy case (holding an unrelated
Delete's request instead) still showing it. Kill-verified against the
pre-review commit (`8b4b405`, `git stash` around `tree_manage.rs`): the new
own-commit assertion failed there exactly as expected — the dimmed list and
the "another change" line for an ordinary Move it is possible to reproduce on
every single successful pick, not a rare race — and passes on the fix.

Verified: `cargo test -p app --features hosted` (320 passed, incl. the three
`MoveBlocked` unit tests), gate subset (fmt, workspace clippy, frontend wasm
clippy) clean, `collection-tree-move.spec.ts` full chromium `--workers=1`
**12/12**. `collection-tree-manage.spec.ts` in the same run: **26/29** (3
failures triaged as pre-existing shared-dev-branch data debris unrelated to
this change — a `genuinelyUnownedCard` pool exhaustion and an Inbox rollup
count off by the same kind of leftover `zz-e2e` debris the e2e-suite skill
already names as a known collision source; reproduced solo, no relation to
move-dialog code).

### ⌘K command palette (2026-07-26)

`app/src/components/palette.rs` — mounted once in `AppShell`, rendering nothing
unless **desktop and signed in**. `PaletteBody` owns the chord listener, the
recent ring, the place index and each row's action; `PaletteSurface`/
`PaletteContents` are the `CommandDialog` + field + grouped rows + footer, split
out so the bench can drive them (`command-dialog` — `CommandDialog` had no bench
coverage at all before this task).

**The ordering caveat is live on this surface, and it is discharged by
remounting — but the first two attempts to remount did not.** This is the
sharpest new fact about the caveat and it generalizes past this page:

1. The palette genuinely reorders the same rows as the query grows
   (`ranking_reorders_as_the_query_grows`: `e` → `[Shoebox, Depth Box]`,
   `eb` → `[Depth Box, Shoebox]`), so the exemption for "fully remounts" is the
   only thing standing between it and the bug.
2. **A plain dynamic closure is not a remount.** Rendering the rows from
   `{move || …}` left `document.contains(firstRow)` **true** after a keystroke:
   tachys diffs an unkeyed view collection *positionally* and reuses the DOM
   nodes while re-running the item registrations, so `command`'s registry grew
   while the nodes stayed put. Measured, not reasoned. The shape that actually
   remounts is `<For each=move || [rows.get()] key=RowSet::key>` — a `<For>` of
   **one** item keyed on the whole row set's identity — and the guard test pins
   a DOM-node reference with the `CommandInput` node as a positive control, so
   regressing to a positional diff (or to a per-row keyed `<For>`) fails it.
3. **Remounting was still not sufficient**, which the review caught: `CommandItem`
   registers into `CommandContext::items` from its *component body*
   (`command.rs:348-361`), which Leptos runs at **view-construction** time, not at
   DOM mount. `group_views` built both groups at their `let` bindings and only
   *then* chose DOM order, so places always registered first however they were
   drawn. Typing a single `n` (`New binder…` scores 11 at a prefix, `Inbox` 1
   mid-word) drew COMMANDS on top while `visible_ids()[0]` was `Inbox` — the
   highlight landing in the second group, `↑` clamping immediately, and the
   top-drawn rows unreachable upward. Verbatim the caveat's own failure. Fixed by
   deciding the order *before* constructing either group (a `[Option<AnyView>; 2]`
   built inside the `commands_first` branches), so construction order is
   structurally the mount order. **The `compareDocumentPosition` sort stays
   deferred**, now for a second reason beyond the original: `visible_ids()` is an
   already-measured O(n²) hot path and a DOM-ordering pass per call would put
   layout reads inside it. The construction-vs-insertion fact is recorded at the
   top of `visible_ids`'s doc, because it applies to any consumer building
   sections conditionally.

**Deviations from `design/command-palette.md`, each forced:**

- **`should_filter=false` with a hand-written fuzzy ranker.** The primitive's
  filter is a lowercase `contains` — it cannot match `trabin` → `Trade Binder`
  and cannot *rank*, while the design asks for fuzzy matching with the best match
  pre-selected. `score()` is subsequence matching with an anchor rule (each char
  after the first must be contiguous or at a word start), which keeps
  `cd` → `Commander Deck` while refusing `de` → `Undo last move`.
- **Groups are ranked by their top match** rather than fixed COLLECTIONS-then-
  COMMANDS. The design asserts both "grouped" and "best match pre-selected", and
  under a fixed order those conflict. Found by the e2e, whose scratch binder was
  named `zz-e2e-palette-undo-…`: typing `undo` pre-selected the collection over
  the command. `tra` still puts COLLECTIONS first, so the wireframe is unchanged.
  **This deviation is what exposed finding 3 above** — it is the only thing that
  makes DOM order variable.
- **Cold start is unspecified in the design.** With no history the group is
  labelled PLACES and offers All cards / Inbox / Shopping list; without it a
  first-ever `⌘K ⏎` would run `New binder…`.
- **Recents live in `localStorage`, not a cookie.** `tr_dest`/`tr_theme` are
  cookies *because* they must be SSR-readable; this list is read only by a
  surface that does not exist on the server.
  **(2026-08-13, P6-145)** The key is per-*user*, not per-origin:
  `tr_recent_places:{user_id}`, read/written only once `CurrentUserResource`
  resolves a signed-in user (nothing under the bare `tr_recent_places` key
  again). Before this fix the key was shared by every account on a browser,
  so a second sign-in inherited the first account's collection ids — `at_rest`'s
  index reconcile (dropping ids the live tree no longer has) was the only thing
  keeping a foreign id from rendering, not a real access boundary. Sign-out
  (`shell::UserMenu`) now also removes the current user's scoped key and the
  legacy bare key from `localStorage` explicitly, since it is a hard
  `location` navigation (P6-122) that does not otherwise touch storage; this
  is defense-in-depth once the key is scoped, and the only cleanup path for
  the legacy key, which is otherwise left to rot rather than migrated (a
  migration risked carrying one account's ids into another's ring).
- **Inbox is not added as a system place** — it is a tree row, so the flatten
  already produced it and adding it again would show two `Inbox` rows. `All cards`
  and `Go to My cards` both target `/my` and both are kept per the design, but
  only `All cards` is a recent-ring key so `/my` cannot appear twice in RECENT.
- Desktop is `(min-width: 768px) and (pointer: fine)`, resolved in an `Effect` and
  then **listened** on one `MediaQueryList` per document — deliberately unlike
  `CardPreview`'s per-card unlistened sample (already filed as a discovery).
  The same client-only gate is the hydration contract: `false` during SSR *and*
  during the hydration render, so the palette is absent from SSR markup for
  everyone.

**`Undo last move` is session memory, not an endpoint.** There is no
`undo_last_move` server fn and `lib.rs` records why (it races a second tab), so a
shell-level `LastMoveState` is written by every surface that already raises an
undo toast. **The review's two promoted majors were both this invariant** —
`LastMoveState` must name the most recent reversible move, or nothing:

- **Teardown recorded nothing**, so after emptying a deck `⌘K → Undo last move`
  reversed an *older, unrelated* move (a binder add from minutes earlier) and left
  the teardown standing — an unintended write to different data from a labelled
  global shortcut. `TeardownReceipt` now carries `move_ids: Vec<Id>` instead of
  `moves: i64`; the count is `len()`, not a second field that could disagree.
- **A toast's Undo did not clear the record**, so a later palette undo replayed a
  dead id, got an idempotent `Ok(())` from `undo_one` and raised a **false success
  toast over a no-op**. New `LastMoveState::forget(&[Id])` at all four toast-undo
  sites, **id-matched** via a pure `forgets()` — a toast outlives the row that
  raised it, so an older toast's Undo must not wipe a newer move's record.

An audit of every UI-reachable `moves`-row writer (quick-add Have, removal, batch
move, pull, teardown) found no third offender: `set_holding_quantity` writes no
ledger row (its undo is a re-commit, correctly excluded), tree reparent/reorder
write none, and `add_holding` has no UI caller. One judgement call recorded on the
method: `forget` runs when the reversal is **dispatched**, not when it succeeds —
the failure modes are asymmetric, and a stale record claiming success over a no-op
is worse than an over-eager forget saying "nothing to undo" while the toast that
started the reversal is still on screen.

Two other traps worth carrying: **reading a resource outside
Suspense/Transition/effect fails `hydration-check.mjs`** (the first version read
`CurrentUserResource` and the tree resource directly and the probe warned of a
hydration mismatch — both now go through `Effect`s), and the bench page grew a
**duplicate `id="command-palette"`** (section anchor + dialog) that only the SSR
curl dump exposed; the section is now `command-dialog`.

`provide_tree_manage` was hoisted from `CollectionTreeNav` to `AppShell` so
`New binder…` can open the tree's own create dialog from Catalog mode, where the
sidebar is not mounted; `TreeDialogs` still lives beside the tree, so a create
opened from Catalog mode appears when `/my` mounts. `CommandInput` gained an
optional `id` and `CommandDialog` now forwards `should_filter` — both additive,
documented in the vendored module doc.

Review: **one major** (finding 3), plus **two minors promoted to majors by the
orchestrator** (the `LastMoveState` pair — an unintended write from a labelled
shortcut clears the wrong-data bar). Eight minors filed under Phase 5 discoveries.
The Major-1 regression test was confirmed **failing before the fix** (the highlight
sat on the registered-first row while the premise assertions about draw order all
passed, so the failure is the bug and not a vacuous setup) and passing after; the
`forget` fix was mutation-checked by replacing its call with `let _ = last_move;`,
which made the test fail with the false success toast, then restored.

Verified: gate **8/8 green** on macOS incl. `three_rings` (app 195 tests + shared
28), full chromium tier **190/190** at `--workers=1` (5.9 min), hydration CLEAN
anonymous ×4 and authed ×5, bench CLEAN, SSR curls showing **zero** palette markup
authed, anonymous and on the bench with `id="tree-create"` and `Trade Binder` as
positive controls, Android CDP PASS + `probe:android-palette` CLEAN at 540 px
(`pointer: fine = false`, the desktop gate reads false, and `CommandDialog` itself
ranks `tra` → `[Trade Binder, Trade duplicates]` on the real webview as the
negative check's positive control).

### `CardSummary::owned` on catalog search (2026-07-26)

`HostedBackend::search` finished with `into_summary(None)`, so the tile's
"N owned" badge was dead code and **an authed catalog looked identical to an
anonymous one**. New `owned_by_oracle(&[Uuid]) -> Option<HashMap<Uuid, i32>>`:
`None` with no session, otherwise one
`SELECT oracle_id, owned FROM owned_by_card WHERE oracle_id = ANY($1)` inside
`scoped_tx`. `card_summary` was rewritten onto the same helper.

**The scoped and unscoped reads were kept separate, deliberately.** Folding a
`LEFT JOIN owned_by_card` into the paging query would force the *public,
cacheable, hottest* read into a transaction for a column anonymous callers never
see — or else duplicate the grammar/keyset/cursor construction across two paths.
Keeping them separate also means the tile and the detail page read ownership
through **one** helper and cannot drift, which was the binding constraint. Round
trips are a wash (join = begin/set/select/commit; separate = select +
begin/set/select/commit), and ownership stays out of the paging query so it
structurally cannot influence which rows a search returns.

`native.rs` needed no change, verified rather than assumed: `NativeBackend::search`
GETs `/api/catalog/search` with the caller's bearer token, that route resolves its
backend via the opportunistic `routes::catalog_backend`, and `owned` round-trips
as JSON. The native shell inherits the fix from the hosted terminus — which is
the point of the seam.

**Tenancy was verified against real multi-tenant data**, not from the code
comment. `migrations/0003_collections.sql:89` declares the view
`WITH (security_invoker = true)` (confirmed live in `pg_class.reloptions`); both
underlying tables carry `relrowsecurity` **and** `relforcerowsecurity`; the
policies are `USING (user_id = current_setting('app.user_id', true)::uuid)` on
role `public`; the pool role has `rolbypassrls = false`; and `scoped_tx` binds
the GUC transaction-locally with a bound parameter. Batching by `ANY($1)` does
not widen scope — neither the old single-id lookup nor the new one filters
`user_id`, both relying identically on RLS. The dev branch happens to hold two
oracle_ids owned by two different users, and for both the authed API returned
this user's count — not the other user's, not the cross-user sum.

**`Some(0)` and `None` render identically**, because the badge hides at zero. So
the authed/anonymous distinction is only observable **on the wire**, and a
page-only test cannot protect it. That is not a stylistic point: the mutation
making anonymous return `Some(0)` was killed *only* by the JSON-route assertion.

**The card-detail "Your copies" total is computed independently** — summed from
`card_detail`'s per-collection ownership rows, a different query from
`owned_by_card` — which makes it the only assertion that pins the actual count.
The mutation returning `n + 2` was killed only by that cross-check; comparing the
badge to the API's own `owned` would have survived, since both come from the
mutated helper. Review confirmed the two cannot diverge structurally: same inner
join under the same GUC, differing only in `GROUP BY` grain, and
`holdings.printing_id` is `NOT NULL` so the join to `printings` drops nothing.

**Both mutations were killed only by assertions that read as redundant.** That is
the reusable lesson, and it generalizes: an assertion with *independent
provenance* is the one with authority, even when it looks duplicative next to a
cheaper one.

**Two environment findings worth more than the fix.** `cargo leptos watch`
silently dropped a save that landed while a rebuild was in flight, and `touch`
did not retrigger it (it appears to hash content) — only a genuinely different
edit forced the rebuild. Any mutation pass that trusts a green run without
confirming the mutation actually compiled is measuring nothing. And the `{..}`
spread trap bit again: a component prop ending in a bare path immediately before
`{..}` parses as struct-update syntax (`E0797`), so the testid rides a wrapper
`<span>` in both views.

**Review: CLEAN, zero majors**, seven minors filed. Notable among them: `search`
now opens an RLS transaction it previously never needed, so a failure in the
ownership read turns a working catalog search into a 500 **for signed-in users
only**, with no fallback degrading `owned` to `None` while still serving results.
**Fixed 2026-08-12 (P6-135, was P6-038a):** `search`, `card_summary`, and
`card_detail` now degrade a failed ownership read to `None`/anonymous shape
instead of `?`-failing the request — see collection-api.md Findings.

**Evidence.** `cargo test --workspace --exclude frontend` 138 + 26 green; full
chromium e2e **153/153** at `--workers=1` (4.2 min); hydration CLEAN on three
authed URLs plus one anonymous; SSR curl on the same card shows `7 owned` in both
grid and list for an authed session and **zero** occurrences anonymously, with
`/cards/<id>` reading `Your copies · 7`; Android webview probe covers the
anonymous half (the dev proxy strips Cookie headers) and confirms 50 tiles, zero
badges, `owned: null` on all ten API hits.

### Set facet as a real picker (2026-07-26)

`app/src/catalog/rail.rs` + `CatalogStore::list_sets` (trait, both backends,
`GET /api/catalog/sets`, a thin GET server fn) + `SetSummary`/`SetQuery` in
`shared`. Resolves the "Set is a text input, not a picker" deferral above.
`RailState.set` became `Vec<String>`; `split_codes` and the comma round-trip are
gone. The route is deliberately anonymous — sets carry no ownership, so there is
no opportunistic-session arm.

**The selection is the query text, and is never intersected with the fetched
rows.** That is the whole design, and it is what prevents the widget silently
dropping part of a pasted query. `s:xyz` renders a chip, counts in the badge,
reflects on a shared link, survives an unrelated rail edit byte-for-byte, and is
removable — identically to `s:mh3`, and identically to a *real* code that happens
to fall outside the current 25-row window. There is no "unknown code" branch to
get wrong because recognition never happens: validating the selection against the
list is the only mechanism by which the widget could silently alter someone's
query, so the mechanism was removed rather than special-cased. No "use `xyz`
anyway" row either — the set list is complete, so an unmatched string is a typo,
and offering it would let typos in. Case and whitespace normalization stay in the
grammar's `csv()`, not against the list.

**Bounded server-side search, not a preloaded list — forced by a measured
O(n²).** `CommandItem`'s `highlighted` is a `Memo` calling
`CommandContext::visible_ids()`, which clones the whole items vec and `.get()`s
every item's `visible` signal: O(n) per item, O(n²) per invalidation — and
`Command`'s highlight-reset `Effect` invalidates all of them on **every
keystroke**. ~1050 sets is not viable. So `list_sets` takes `q` + `limit`
(default 25) and ranks exact-code-match first (typing `mh3` also matches
`amh3`/`tmh3`/`pmh3`, which would otherwise push MH3 out of the window), then
newest-first. Review independently confirmed the complexity claim.

`command`'s rows are built inside a `Suspend` keyed on the resource, so each
result set is a **full remount in document order** — the safe case for the
recorded `visible_ids()` ordering caveat. Nothing reorders in place, so ↑↓ visits
rows in visual order.

**The `Suspend` → signal → `Effect` commit hop.** A `Suspend`'s future and view
must both be `Send`, and `use_navigate`'s closure is neither, so a row click
writes a code into an `RwSignal` and an `Effect` outside the async boundary
commits. Still exactly one writer (`use_navigate_query`), reached through a
signal — which is also why the cursor drop comes for free on both the select and
deselect paths and on Reset. Review verified the `Effect` reads `codes` and
`query_map` **untracked**, so a URL change cannot re-fire it, it early-returns on
`None` so it does not fire on mount, and the `set(None)` before commit makes the
follow-up a no-op.

**The debounce race was reduced, not compounded.** The old Set widget was a
`RailTextField` that armed its own 250 ms navigate timer — a genuine participant.
The picker commits synchronously on click, so that writer is gone and a Set click
is now exactly a Color-checkbox click. Its only timer debounces the *read* and
never touches a navigate path.

**Review: one major.** Three distinct states — *not fetched*, *fetch failed*, and
*fetched and genuinely empty* — were all crammed into `Vec<SetSummary>`, so two
non-answers rendered as an authoritative claim about the user's catalog:
`sets.await.unwrap_or_default()` turned an `Err` into "No set matches." with no
error arm and no retry (and on native an **offline phone is the normal failure** —
`native.rs` maps a transport error to `Upstream` precisely so callers can tell),
while `if !expanded { return Ok(Vec::new()) }` made not-yet-fetched a *successful
empty*, so bare `/catalog` SSR'd `set-empty` with zero `Loading sets…` and — since
`Transition` holds previous children across a re-key — the first thing anyone ever
saw in the facet was "No set matches." for the length of the round trip.

Fixed by giving the resource `Option<Result<Vec<SetSummary>, _>>` and four arms
(`EitherOf4`): `None` → loading, `Some(Err)` → `role="alert"` with the message
from the shared `catalog::describe_error` (same vocabulary as the paging arm) plus
a **Try again** button that bumps an `attempt` counter in the resource's source
tuple, which is what makes it refetch an unchanged query; the search box stays
usable so typing is also a retry. The lazy fetch was kept — `/catalog` is the
most-loaded route and the rail renders twice. `Transition` was kept over
`Suspense` deliberately: `Suspense` would also make the fallback reachable but
would strobe the list away on every debounced keystroke. Verified: bare
`/catalog` SSR went from `set-empty × 2, Loading × 0` to `set-loading × 2,
set-empty × 0`.

**The failure was induced for real, not asserted around.** The adapter is a GET
server fn, so `page.route("**/api/list_sets*")` fulfilling a 500 produces the same
`Err` shape an offline phone does. Two mutations, each killing exactly its own
test and nothing else: collapsing the `Err` arm back into the empty state killed
only the error test; rendering the empty state for not-fetched killed only the
flash test. Liveness was proved before each run by `strings`-ing the served
`/pkg/app.wasm` for the testid anchors rather than trusting the watch — which
matters, since `cargo leptos watch` has been observed silently dropping a save
mid-rebuild.

**Evidence.** `cargo test --workspace --exclude frontend` 143 + 26 green; full
chromium e2e **162/162** at `--workers=1` (4.3 min); hydration CLEAN; SSR curl of
`?q=s:mh3,lea` renders both chips *and* the 25-row list while bare `/catalog`
renders zero rows (lazy fetch confirmed); Android webview rail probe 11/11
including chip reflect, search-by-name, multi-select and the badge.

**Fixed 2026-08-12 (P6-136):** `list_sets`' code/name `ILIKE` match now
escapes the term's `%`, `_`, and `\` before binding (the same
`crate::search::sql::escape_like` helper `/catalog` and `/my` use) with an
explicit `ESCAPE '\'`, so typed wildcard characters are literal rather than
LIKE metacharacters. Its `ORDER BY` was also sorting the `released_at::text AS
released_at` output alias — lexicographic, correct only by ISO-collation luck
— instead of the `date` column; it now sorts `s.released_at` directly. The
identical alias-capture pattern was found and fixed in the card-detail rulings
query.

**Truncation removed 2026-08-12 (P6-137), explicit maintainer ruling.** The
25-row default above silently truncated: searching "commander" matched 109
sets and showed 25 with no indication anything was cut off. The maintainer's
call was not to add a "showing 25 of N" indicator but to remove the cap
entirely — the picker is now a Scryfall-style scrollable dropdown listing
*every* match, including on a blank term (browse-all shows all 1,047 sets,
newest first). Typing still narrows server-side exactly as before; only the
window size changed.

The 25 lived in `SetQuery::limit()` (`shared/src/catalog.rs`): `self.limit
.unwrap_or(25).clamp(1, 200) as i64`. It is now:

```rust
match self.limit {
    Some(n) => n.clamp(1, 5_000) as i64,
    None => i64::MAX,
}
```

— an unrequested `limit` (`self.limit: Option<u32>` is `None`) carries no cap
at all; an explicit `limit` (the same field the public `GET
/api/catalog/sets?limit=` route reads) is still clamped, to `1..=5_000` rather
than `1..=200`. `i64::MAX` as a Postgres `LIMIT` bound is a no-op — it never
truncates a result set smaller than it, so this is "no `LIMIT` clause" without
maintaining a second SQL string. `list_sets`' only caller (`app/src/lib.rs`'s
`list_sets` server fn, used by both the `hosted` SQL path and the `native`
HTTP-client path, which itself calls the same `hosted` route) already passed
`limit: None`; grepping `SetQuery`/`list_sets`/`CATALOG_SETS` across the repo
found no other caller relying on the old 25 default, so the change applies
uniformly rather than needing a separate "uncapped" request shape.
`CommandList`'s container (`max-h-56 overflow-y-auto`, inherited from the
vendored `Command` primitive's own `overflow-y-auto`) already scrolled a
25-row list — 25 rows already exceeded 224px — so no CSS change was needed for
either the desktop rail or the mobile `FilterSheet` (both render the same
`SetPicker` via the shared `RailBody`).

**The O(n²) this section warned about was real, and it blocked shipping the
ruling as a pure limit change.** Measured with the picker's list fully mounted
(blank term, 1,047 rows) and then a single keystroke typed into the search
box: **~30.9s** to register that keystroke in a debug build (`page.keyboard
.type` — a raw CDP key event, not a Playwright actionability-gated action).
Root cause confirmed by inspection and by reverting the fix in isolation
against unmodified `command.rs` (same server, same fixture): `Command`'s
highlight-reset `Effect` fires on every keystroke and sets `ctx.highlight`;
`CommandItem::highlighted` is a `Memo` **per mounted item** that reads
`ctx.highlight` and therefore reruns for all N items on that one `Effect`, and
each rerun independently called `CommandContext::visible_ids()` — an O(n)
clone-and-filter of the whole item registry. N reruns × O(n) each = O(n²) per
keystroke; at n=1,047 that is ~1.1M signal reads through the reactive graph,
which is what a debug wasm build turns into 30+ seconds of a blocked main
thread.

Fixed in `app/src/components/ui/command.rs` (not just the set picker's calling
code) by hoisting the filtered-and-mapped id list into **one** `Memo` on
`CommandContext`, computed once per `items`/visibility change rather than once
per item, and having each item's `highlighted` Memo read it back with
`Memo::with` (a borrow, not a clone) plus an O(1) index comparison instead of
calling `visible_ids()` fresh. This is a pure algorithmic change — same
registration-order semantics, same `next()`/`prev()`/`activate_highlighted()`
behavior — verified equivalent by running `quick-add.spec.ts`,
`destination-picker.spec.ts`, and `command-palette.spec.ts` (the primitive's
other three consumers) before and after: identical pass/fail sets both times
(three pre-existing `command-palette.spec.ts` "Undo last move" failures,
confirmed present against *unmodified* `command.rs` too — shared-fixture
holdings-count flakiness against the live Neon dev branch, unrelated to this
change; not in this file's scope to fix). Touching the shared primitive rather
than only capping the set picker's request was a deliberate choice: a fallback
cap (this task's own contingency plan) would have meant permanently disobeying
"browse-all shows all 1,047" for a bug that was fixable in the primitive
itself, at a scope the review-verified equivalence made low-risk.

**Perf after the fix**, same scenario, same dev (unoptimized) build: opening
the picker on a blank term and reaching the full, stable 1,047-row list ≈
840ms (was ~2.1s pre-fix, since the O(n²) also taxed *mounting* — items push
onto the registry one at a time, and each push used to re-trigger every
already-mounted item's `visible_ids()` clone); typing "commander" and settling
on its 109-row narrowed result ≈ 670ms (includes the existing debounce);
clearing back to the full browse-all list ≈ 780ms; two `ArrowDown` + `Enter`
keyboard-nav picks ≈ 640ms combined, landing the pick correctly. All
comfortably responsive — no fallback cap needed.

**Round-1 review, fixed 2026-08-12 (P6-137): auto-open was still fetching the
full list, twice, on every set-filtered page load.** The truncation fix above
lifted `SetQuery::limit()`'s cap correctly, but conflated "the section is
open" with "the picker was engaged" — the same signal drove both. A
`?q=s:mh3` link auto-opens the Set section (`section_seeded_open`: any URL
already carrying a set seeds it open, so the chip is visible without a click),
and the old "fetch when open" rule fired on that seed with zero interaction.
Worse, this component renders **twice** per page — the desktop rail and the
mobile `FilterSheet`, and `SheetContent` mounts its children unconditionally,
off-screen via a CSS transform, rather than unmounting while closed (a
separate, pre-existing trap, filed rather than fixed here) — so every shared
or refreshed set-filtered link paid for SSR-rendering **~2,094 rows of
markup** (2 × 1,047) plus two full hydration payloads, before anyone touched
the picker. Measured directly: `curl "/catalog?q=s:mh3"` was **2,432,307
bytes** with 2,094 `data-testid="set-option"` occurrences (exactly 2 × 1,047,
confirming the double-render).

Fixed by splitting the one signal into two, in `SetPicker`
(`app/src/catalog/rail.rs`): `expanded` still seeds open/closed exactly as
before (the `<details>` state, the badge, the SSR'd chips — none of that
changed, a shared link still visibly reflects its selection) and a new
`engaged` signal — always starting `false`, even when `expanded` seeded open —
gates the row list. The resource's fetch fn now checks `engaged`, not
`expanded`; the row-rendering `Transition`/`Suspend` is wrapped in a `<Show
when=engaged>` so it is not even *constructed* (let alone SSR'd) while
un-engaged, replaced by a small, honest "Click or type to browse every set."
hint (`data-testid="set-unengaged"`) instead of the old `None`-resource arm's
misleading "Loading sets…" (nothing was loading — nothing had been asked).
`engaged` flips true on either a genuine disclosure toggle (an `Effect`
watching `expanded`'s *transitions*, not its seed value — the browser only
fires a real `toggle` event on an actual state change, never for the initial
SSR-open state, so the effect's own first run is exactly the run to ignore
via the `prev.is_some()` idiom) or on focusing/hovering the picker itself
(`on:focusin`/`on:pointerenter` on the `Command` root, spread via `{..}` —
covers keyboard-tab-in, mouse-hover-intent, and touch-tap-to-focus). Measured
after: the same `curl` is **359,697 bytes**, zero `set-option` rows — an 85%
reduction. `filter-rail.spec.ts`'s SSR test now asserts the negative directly
(chip present, `set-option` absent, `set-unengaged` present) and a new test
pins the same for the mobile sheet's independent `SetPicker` instance.

**Also fixed, same pass:** ↑↓ never scrolled the highlighted row into view
(`app/src/components/ui/command.rs`) — invisible at a handful of rows, a real
gap now that the set picker made a 1,047-row keyboard-reachable list the
point. `CommandItem` now calls `scroll_into_view_with_scroll_into_view_options`
(`block: "nearest"`, hydrate-only) when it becomes highlighted; verified
against `quick-add.spec.ts`, `destination-picker.spec.ts`, and
`command-palette.spec.ts` (the primitive's other keyboard-nav consumers) —
same pass/fail set before and after (the three pre-existing
`command-palette.spec.ts` "Undo last move" fixture-pool flakes, reproduced
against unmodified code too, are unrelated). Two doc-only fixes for accuracy:
`CatalogStore::list_sets`'s trait doc (`app/src/backend/mod.rs`) still claimed
the result was bounded; `hosted.rs`'s `ORDER BY` comment still framed the
three ranking tiers as gating *reachability*, which was true only under the
old cap. And a `shared/src/catalog.rs` unit test now pins `SetQuery::limit()`
directly: `None` → `i64::MAX`, an explicit value clamped to `1..=5_000` both
directions — the shape the maintainer flagged as worth a test of its own
rather than relying only on the e2e layer.

**Enter-vs-debounce race, fixed 2026-08-12 (P6-138).** `CommandInput` writes
the live box text synchronously on every keystroke, but the set picker's rows
— and the keyboard-nav registry Enter reads to decide what "activate the
highlighted row" means — are re-keyed by the picker's own 250ms-debounced
server fetch (the read-side debounce described above, under "The debounce race
was reduced, not compounded"). Typing toward `s:lea` ("limited edition alpha")
and pressing Enter inside that window could add `s:leb` instead: the box
already read "lea", but the mounted rows — and whatever they had highlighted —
still answered whatever term was fetched before the last keystroke's debounce
had fired. Wrong action taken on the user's behalf, silently.

**Chosen semantics: defer, not no-op, and not a synchronous flush.** Of the
three shapes considered — (a) no-op mid-window (simplest, but a keystroke that
visibly "did nothing" is surprising when the user's whole intent was to
finish and commit), (b) remember the Enter and act on the first row of the
*next* settled fetch, (c) flush the debounce synchronously and select from the
result — (c) is not actually reachable: the fetch is an async server round
trip, so "flush and select" still has to wait for a response, i.e. it
degenerates to (b) with a shorter window. The shipped behavior is that
shortened hybrid: Enter mid-race cancels the pending debounce timer and
re-keys the fetch *immediately* rather than waiting out the rest of the
250ms, then selects the first row of whatever comes back — but only if the
box still reads the way it did the moment Enter was pressed (typing more in
the meantime abandons that Enter; it is not anyone's business once the box has
moved on). "First row" needs no special-casing: P6-136's ranking already puts
an exact code match first, so finishing a code and hitting Enter — the exact
scenario the race breaks — lands on the right set as soon as the fresh fetch
answers.

**Where the gate lives: an opt-in pair on `CommandInput`, not a primitive-wide
behavior change.** `command.rs` is shared by the set picker, ⌘K, quick-add,
and the destination picker (module doc, "Vendoring" deviations list) — none of
the other three re-key their rows on an independent timer disjoint from the
box's own live text (⌘K and the destination picker filter synchronously on
`ctx.query` itself; quick-add's foreign-input path does not use `CommandInput`
at all), so none of them have this race, and none needed their behavior
touched. `CommandInput` gained two new optional props, both defaulting to
"off, behave exactly as before": `stale: Option<Signal<bool>>` and
`on_stale_enter: Option<Callback<()>>`. When `stale` reads `true` at Enter
time, `on_stale_enter` runs instead of the built-in
`activate_highlighted()` — otherwise Enter is untouched. Only `SetPicker`
(`app/src/catalog/rail.rs`) passes them.

**Mouse clicks are unaffected on purpose.** A click on a visible row — stale
or not — *is* the user's intent (they can see exactly what they are clicking),
so the gate lives entirely in `CommandInput`'s `Enter` branch; `CommandItem`'s
`on:click` was not touched.

**The freshness check needed a third signal, not just `search`.** The obvious
first cut compared the live box text against `search` (the debounced value
fed into the `sets` `Resource`) — but `search` flips the instant the debounce
timer fires, while the *rendered* rows (kept on screen by `Transition`) stay
the previous fetch's until the new one actually resolves, which can lag
`search` by a full network round trip on a slow connection. So the picker adds
`rendered_for: RwSignal<String>`, written only inside the `Suspend` block once
a fetch settles — and the resource's fetcher now echoes the term it used back
alongside its result (`Option<(String, Result<Vec<SetSummary>, _>)>`, the same
`(q, …)` idiom `catalog.rs`'s `SearchPayload` uses for its own `displaced_by`)
so the settlement can be matched to the request that produced it without
trusting `search`'s value to have stayed put across the `await`. `stale`
compares the live box text against `rendered_for`, not `search` — this is
what makes the e2e test's `page.route` hold (below) actually exercise the
gate: `search` re-keys the instant the debounce fires, well before the held
response ever lands, so a `search`-only comparison would have read "fresh"
throughout the entire held window and the test would have passed on
unpatched code too.

The pure comparison is `enter_targets_stale_rows(live: &str, fetched_for:
&str) -> bool` (`app/src/catalog/rail.rs`, unit-tested) — the same shape as
`catalog.rs`'s `displaced_by` (P6-130), collapsed to one disagreement instead
of two because the picker has only one rendered value to compare against
(`rendered_for`) rather than a URL plus a pending debounce.

**Verified with two real, adjacent sets, not a synthetic fixture.** Limited
Edition Alpha (`lea`) and Beta (`leb`) are both exact-code-match top hits for
their own search term (P6-136's ranking tiers), so "the stale list's top row"
and "what the box now asks for" are genuinely different sets — not a ranking
coincidence the test would need to hope for. `filter-rail.spec.ts`: establish
"leb" as the real, settled list; hold the *next* `**/api/list_sets*` request
open via `page.route` so the race window is deterministic instead of racing a
fast local round trip; type "lea" and wait past the 250ms debounce (the held
fetch has been launched but not answered); press Enter; assert no chip landed
at all (neither `leb` nor `lea` — nothing to act on yet). Release the held
request; assert `s:lea` — never `s:leb` — lands once the fresh answer
arrives. A companion positive-control test pins the ordinary, non-racing case:
once the debounce has settled for real, Enter still activates the highlighted
row exactly as before. **Kill-verified**: reverting `command.rs`/`rail.rs`
against the unmodified test reproduces the bug directly — the held-fetch test
fails with `leb` chip present (`toHaveCount(0)` receives `1`); the
positive-control test still passes on unpatched code, confirming it is not
accidentally exercising the fix.

Full `filter-rail.spec.ts` and the primitive's other three
consumers — `command-palette.spec.ts`, `quick-add.spec.ts`,
`destination-picker.spec.ts` — chromium, `--workers=1`, 65 run outcomes
across the four files: **62 passed, 3 failed**, the three failures being
exactly the pre-existing `command-palette.spec.ts` "Undo last move"
fixture-pool flakes already on record above (P6-137) and in
`.claude/skills/e2e-suite/SKILL.md`'s residual-failure enumeration —
reproduced directly against unmodified `command.rs`/`rail.rs` in this same
session (2 of the 3 reproduced in a targeted re-run; holdings-count polling
timeouts against the shared live Neon dev branch, unrelated to the primitive).

### Catalog paging via `?cursor=` (2026-07-25)

`app/src/catalog.rs` + `app/src/catalog/rail.rs` — the slice deferred from the
catalog-page task. The adapter always took a cursor and `SearchResults` always
carried `next_cursor`; the page simply never used either.

**A stale cursor is the defining failure mode**, so clearing it is a property of
the URL the existing writers build rather than a new writer watching `q`. Review
established by exhaustive grep that exactly **three** call sites navigate to a
`/catalog` URL carrying a query — `QueryBar::commit` via `to_url`,
`rail::use_navigate_query`, and `ViewSwitch::go` — plus the pager's own hrefs.
The first two pass `None`; `use_commit` and `reset_all` both funnel through
`use_navigate_query`; `RailBody`/`FilterSheet` navigate nowhere else. **No path
can change `q` while keeping `cursor`.**

**The view switch preserves the cursor deliberately** — relayouting is not a
query edit, and the keyset compares tuples so `view` is orthogonal to position.
That required the pager's hrefs to be reactive `move ||` closures reading
`list_view.get()` rather than baked at build time, because a layout switch
deliberately does not re-render the results block; a fixed href would page a
list-view reader back into the grid.

**The rail/debounce race was not compounded.** That filed defect (a facet click
inside the query bar's 250 ms window overwritten by the debounce firing with
captured text) now has a third participant in the same URL. Deliberately, no
timer-based writer and no `Effect` watching `q` were added: the pager and view
switch are click-driven and never write `q`, only add or remove `cursor`. An
armed debounce firing after a Next click therefore commits page one of the
newest typed text — the correct resolution rather than a hybrid. Review
confirmed the claim.

**One deviation from the `/my` pager, on purpose.** `/my`'s error arm is a dead
end. Paging is what makes an error reachable *with nothing to fix* — a shared
`?cursor=` link can be stale or corrupt, and the user who pasted it did nothing
wrong. So both the error and empty-page states offer "← Back to the start",
keeping `q` and dropping only the cursor. Verified: a corrupt cursor returns
HTTP 200 with an inline error and a way home; past-end and cross-query cursors
render "Nothing on this page." with the same. A deleted anchor row is a
non-case — the keyset compares `(name, oracle_id) > (…)` and needs no existing
row.

Forward-only, matching `/my` and for the same structural reason: a keyset cursor
means "everything after this row", so a real Previous needs a reverse-ordered
query plus a `before` cursor. Filed rather than half-built.

**Mutation testing found a survivor, which is the point of running it.** "The
last page offers no next" originally passed *with the cursor ignored entirely*,
because `bolt` fits on one page — the fixture could not distinguish the
behaviors. Strengthened to assert the card the cursor was taken after is absent;
it now dies under that mutation. Four mutations applied, four killed after the
strengthening. The same weakness survives in the Android probe's equivalent
step, filed.

**Known limitation, not a defect:** the count states this page's row count with
no page qualifier, so the last page of a 73-result search reads "23 results"
while mid-pages read "50+ results". Keyset has no offset, so a "51–73 of 73"
form needs a separate count query or a page ordinal in the URL.
*(Closed 2026-08-12 — see "Catalog paging honesty" below: the count is qualified
past a cursor rather than counted, so neither of those two costs is paid.)*

**Evidence.** `cargo test --workspace --exclude frontend` 138 + 26 green; full
chromium e2e **151/151** at `--workers=1` (4.1 min); hydration CLEAN on four
URLs including a cursored browse-all, a `?q=&cursor=`, a `?view=list&cursor=`
and a past-the-end cursor; SSR curl of a cursored URL renders 50 tiles with
page two's first card present and page one's absent, both pager controls in the
raw HTML; Android webview probe 11/11 including a 34 px tap target and a
deep-linked cursored page.

### Needs view + pick list + `/my/shopping` (2026-07-25)

`app/src/my/needs.rs` + `app/src/my/shopping.rs` — the two remaining unrouted
`/my` pages.

**The task line's composition was wrong, and the spec is corrected rather than
the code.** The queue said the pick list is client-composed from `move_cards` +
`suggested_destinations`. `suggested_destinations` ranks *destinations* for a
card; on this page the destination **is** the page, so the read actually needed
is source-side. The shipped composition is `needs` + `holdings_of_oracle` +
`move_batch`, and review confirmed it is complete for both Pull and Pull-all.
The spec line predates `NeedRow.locations` existing.

**Quantity is never the caller's.** `PullItem` carries only `oracle_id` +
`from_collection_id` — there is no quantity on the wire at all. `pull_needs`
re-reads `needs()` (itself behind `require_owned_collection`), re-runs the same
`allocate` over that fresh read, and accepts only *which* (card, source) lines
to apply. A source the user does not own cannot appear, because
`needs().locations` is built from RLS-scoped `holdings ⋈ collections`. A gap
closed since the page rendered yields no key and refuses as the new
`SkipReason::NoLongerNeeded` — the destination-side twin of `NoCopies`.

**The pick-list walk is order-independent**, which review proved rather than
assumed: `locations` is sorted quantity-desc, so draining any one line leaves
the others' greedy shares unchanged (a reordering would need `q_i > gap`,
contradicting the sort). Ticking rows in any order reproduces the same `want`
for the rest.

**The two buckets cannot diverge from the chip.** `needs()` and
`collection_view`'s totals use identical CTEs and both derive
`to_buy = missing − owned_elsewhere`; the headline reuses
`collection::needs_chip` directly. `sum(locations) == pe.elsewhere` under the
same RLS scope, so `sum(allocate(gap, locations)) == owned_elsewhere` is an
identity rather than a hope. A row appears in **both** buckets when part of its
gap is fillable and part is not — the split is per copy.

**Needs stayed board-blind, deliberately — reversed 2026-08-12 by P6-074.**
`desires` gained a `board` column in migration 0006, so the sideboard-want case
is real: a deck holding a card on `main` and wanting it on `side` produced no
need row. Keeping the blindness was chosen over fixing it because **a
board-aware need would manufacture rows whose Pull cannot work** —
`apply_move`/`move_holding` always land `to_board = main` (board relabel is
card-tagging's operation), so a sideboard need would survive every pull aimed at
it, forever. "Unfilled deck slot" and "missing copy" were treated as different
concepts and only the second was shipped. An e2e pinned the decision so it could
not be silently reversed.

**What actually happened, so this reads as history rather than current
behaviour:** P6-074 fixed the blocker itself — a pull now lands on the board
that wanted it — and only then made the read board-aware, in the same change.
The decision was re-taken, not drifted from, and the pinning e2e was *inverted*
rather than deleted, so it still fails loudly if the behaviour moves again. See
"Board-aware needs rows" below and the same-dated entry in
[collection-api](collection-api.md). The reasoning above is retained because it
is exactly why the pull path had to be fixed first.

The empty state was the honesty gap and was fixed in the 2026-07-25 review: it
read "Nothing missing — every card this collection wants is already here", an
unqualified claim about *slots* from a page that counted *copies*. It became
"Nothing to pull or buy — this collection holds every copy it wants. Unfilled
board slots aren't counted here." — and that last clause became false with
P6-074 and was rewritten again (below). The module doc and subtitle are not what
a user reads when the table is empty.

**Review: CLEAN, zero majors**, twelve minors, ten filed. Two were fixed rather
than filed: the empty-state wording above (an unmet requirement of the board
decision, not a discovered defect), and a hole in the quantity invariant —
duplicate `PullItem`s multiplied the pull, because `planned` is read per item
and the cached holdings were never decremented between items, so
`[{o,src},{o,src}]` against a gap of 2 moved 4 copies. Not UI-reachable
(`allocate` yields one item per source), but the whole point of the design is
that a caller cannot supply a quantity, and repetition supplied one. Deduped on
entry, with `SELECTION_MOVE_MAX` still counted against the **raw** list so
duplicates cannot buy headroom.

**A recorded test limitation, stated rather than papered over:** the dedupe unit
test pins the `dedupe` helper, not the `pull_needs` call site — deleting the
`dedupe(...)` call would leave it green. The adapter was bound by direct
evidence instead (`POST /api/pull_needs` with a line sent twice, gap 2, source
holding 4 → one ledger row, `copies: 2`, read back as destination 2 / source 2).
A standing guard needs an API-level e2e.

**Other shape notes.** The pick list mounts **outside** the `Transition`: a
control inside a resource-driven body dies when its own writes empty that
resource. The export is a read-only SSR'd `<textarea>` plus an `execCommand`
Copy (not `navigator.clipboard`, which prompts for permission in a webview), and
its content is the **shortfall**, not the desired total.

**Evidence.** `cargo test --workspace --exclude frontend` 149 + 28 green; full
chromium e2e **154/154** at `--workers=1` (4.8 min); hydration CLEAN on four
authed URLs; SSR 200 on both new routes with real rows and the export text in
the markup; Android webview probe green including a real tap on the pick-list
checkbox. Five mutations, five deaths — including `needs()` made board-aware,
which killed the decision-pin.

### Undoable removal + deck teardown (2026-07-25)

The third of the three split move-flow tasks, and the one carrying the
trait/backend work the first two were fenced from. Its acceptance criterion was
the standing discovery "**a card cannot be removed from a binder at all**"; that
is now false, undoably.

**Two ledger columns, not one.** `migrations/0009_move_boards.sql` adds
`from_board` *and* `to_board` to `moves`, both `DEFAULT 'main'`, expand-first
with no backfill. A move's ends need not agree — copies leaving a sideboard land
in a binder as ordinary copies — so one column could not say which end it
described, and undo must reverse both ends exactly. `to_board` is always `main`
today; board relabel is card-tagging's operation.

**The row cannot state its grain, so the server resolves it.**
`CardRow::holding_id` is `Some` exactly when one `holdings` row backs the cell,
but `present` is `GROUP BY printing_id, board`, so the rendered row knows no
finish/condition/language. New `CollectionStore::move_holding` (trait + hosted +
native + `POST /api/holdings/{id}/move`) is addressed by **holding id** and
reads grain, board, owning collection and quantity off that row `FOR UPDATE`
**inside the write transaction**. That is also what closes the row-path TOCTOU
the batch-move task had to leave open.

This is the direct application of the rule that task established — *anything
deciding whether a write is possible cannot use a read model that groups away
the write's addressing*. Here the write stopped using a read model at all.

**The `min=1` floor is gone and the defect behind it is genuinely gone, not
re-routed.** Review confirmed `set_holding_quantity(id, 0)` is now unreachable
from the UI (`on_commit` intercepts `c.to == 0` first), so the old
`DELETE FROM holdings`-behind-a-success-toast path has no caller. **Undo
re-creates the holding with a new id** — any UI holding a `holding_id` across an
undo must refetch, which the row does via `HoldingsRevision`; a stale id
surfaces as a visible `not found: holding` rather than touching another row
(UUIDs, no reuse).

**A latent bug fixed in passing:** `add_holding`'s intake move recorded no
board, so undoing a `+ Have` onto a sideboard took copies off the *mainboard*.

**Batch refusals became capabilities.** `movable` dropped to `quantity > 0` and
`CardSource::Move` carries a grain-complete `MoveSource`, so `SkipReason::Board`
is gone entirely. One refusal is deliberately kept: `Grain(n)` for a stack with
several grains and **no default one** (2 foil + 1 etched) — nothing about the
row says which the checkbox meant. `ManyCollections`/`ManyPrintings`/`ManyBoards`
remain, each a question the user answers by opening a page where the choice is a
visible row. (**Superseded on that last point by P6-151, above**: those three are
now asked in the move itself — the which-copies step — and only the toast the
user gets for *declining* it remains. **And on `Grain` by P6-150**: the step's
rows are the full grain now, so 2 foil + 1 etched is two rows with two
steppers, and the variant is deleted rather than kept as a refusal.)

**The batch-path TOCTOU stays open, deliberately, but is now attributable.**
Grain+board on `MoveItem` is the same addressing as a holding id
(`holdings_uniq` makes them equivalent), so adding an id would not narrow the
window; genuinely closing it means `move_batch` taking selection keys instead of
move items — a different API. Instead the residual whole-batch abort now names
the card: `move_batch` tags the failure with the item index, the adapter swaps
it for the entry's token, `name_batch_failure` swaps that for the name. Review
confirmed indices cannot shift (`lines[i]` and `moved[i]` are pushed in the same
match arm).

**Seven mutations, seven deaths** — the highest-value evidence in this task,
since its failure mode is silent corruption behind a success toast: undo
restoring `Board::Main` regardless of the ledger (killed the sideboard
round-trip test), `move_holding` using the default grain instead of the row's
(killed the foil/LP/Japanese test), teardown summing across boards, `min = 1`
restored (killed **four**), `caller_reports` disabled, `MoveItem` restating the
default grain, and `from_board: Main` instead of the resolved board. No test
survived its own mutation.

**Review: one blocker, zero code majors.** The blocker is operational ordering,
not code — see below. The grain/board round trip was traced end to end and no
path substitutes a default for a resolved grain or board; no `board = 'main'`
literal survives in any write; the migration's defaults reproduce prior undo
behavior exactly, because pre-0009 moves were all mainboard-addressed by
construction (`holding_take` pinned `'main'`, `holding_add` inserted `'main'`).

> **Deploy ordering is load-bearing for this change.** The new code names
> `moves.from_board` / `to_board` in three places — `add_holding`'s intake
> INSERT, `append_move`'s INSERT, and `undo_one`'s SELECT. Migration 0009 is
> expand-first only in the *old-code-against-new-schema* direction; the new code
> **cannot run against the unmigrated schema at all**. On an unmigrated
> database every write path (`+ Have`, every move, every removal, every
> teardown, every undo) fails `42703 column "to_board" does not exist` → 502,
> while reads are untouched — so the app looks alive while every write errors
> behind a toast. Nothing enforces the ordering: the merge gate never touches
> Neon, auto-merge ships on green, and Render deploys on push to `main`.
> `scripts/migrate.sh prod` must be run **before** the merge.

**Evidence.** `cargo test --workspace --exclude frontend` 138 + 28 green; full
chromium e2e **149/149** at `--workers=1` (4.5 min); hydration CLEAN on four
authed URLs; Android webview stepper + collection probes green; grain/board
round trips verified against the Neon dev branch read back through
`/api/cards/{id}/holdings` — the one read that does not group grain away —
including a foil/LP/Japanese stack, a sideboard→binder→undo cycle, a two-board
teardown writing one ledger row per board, and return-to-previous landing back
in the originating binder.

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
so the rest of the batch still moves. (**`Grain` and `Board` are both gone
now** — `Board` to the entry above, `Grain` to P6-150, which splits the picker's
rows to the full grain and asks instead.) `MoveItem`, `CardRow`, `holding_take` and
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
not lie about. (**Superseded by P6-150, above**: quantity is chosen per entry in
the which-copies picker and validated server-side against the caller's real
holdings; a stack of exactly one copy still moves unasked, and the toast counts
copies and cards separately.) The destination picker was *extracted* for sharing
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
route's DOM detach/re-attach (fixed since, P6-068). `Checkbox` is fully
controlled, so a re-mounted row re-derives its checked state from the shell
signal. Sign-out is a
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
returns `Ok`, then `toggle → closed`; (b) this page's whole subtree was removed
and re-inserted on every `?q=` change, and removing a *showing* popover hides it
**without** firing `toggle` (the HTML removing steps pass `fireEvents=false`), so
the Rust `open` signal stayed `true` while the panel was gone. An
absolutely-positioned panel has neither problem; Escape and outside-`pointerdown`
are handled in the panel instead. The destination picker keeps using `popover`
(button trigger, no navigation) — this is not a retreat from the primitive.
**(b) was fixed in P6-068** and, as recorded there, was never the router; (a)
alone still rules `popover` out here, so the deviation stands.

**This route churned its own DOM — and it was not the router** (corrected
2026-08-11, P6-068; the original wording is kept below because the *measurement*
was right and only the attribution was wrong). Measured on
`/my/collections/:id`: after each `?q=` navigation the page subtree was detached
for ~400 ms and re-attached (same nodes), blurring the field — without mitigation
the keystroke loop died after the first card. `/catalog` and `/my` did **not** do
this. The panel held the caret with a 120 ms interval gated on *panel open* and
on focus having landed on `<body>` rather than a real element; that interval is
now deleted, because the cause is gone. See the Findings entry
"`/my/*` stayed mounted once the page stopped reading its resource in setup".

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

**(2026-08-13, P6-147) `PresentSection` gates on a non-empty (post-trim)
`?q=`.** It previously rendered whenever `present` was non-empty, with no
regard to the query — since the collection read behind `present` returns the
destination's unfiltered first page for an empty `q`, the section filled with
the whole first page both when the panel first opened and in the instant after
every add cleared the field (the retained `QuickAddFacts` from P6-068 keep
`present` populated across that clear rather than going empty). The fix is a
render gate only (`present_visible` in `quick_add.rs`) — it does not touch
`QuickAddFacts`, `present_matches`, or the P6-068 retention behavior described
above.

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

**(2026-08-13, P6-148) Both minors above are fixed, plus two more in the same
keystroke loop.** The two the previous review re-checked and left as minors
turned out to compound with two others once the loop was walked end to end:

- **Escape-with-zero-rows** — `decode`'s `rows == 0` early return covered
  *every* key including Escape, so a panel open with nothing mounted (an
  empty query, or a query the catalog has no match for) could only be
  dismissed by an outside click. Escape is now checked first and
  unconditionally, before the row-count gate.
- **Stale count after a failed add** — `add`'s two synchronous early returns
  (no destination yet; a `Have` on a card with no printing) left `count` set
  after showing the failure toast, so the chip kept reading `× 4 ⏎` and a
  later bare ⏎ silently reused the abandoned quantity instead of implying 1.
  Both paths now reset `count` before returning, mirroring what the success
  path already did.
- **History spam** — the post-add `reset` prop's `clear()` commits `q = ""`
  through the same `commit()` every other `QueryBar` write uses, and
  `commit`'s `replace = was_searching && !q.is_empty()` is unconditionally
  `false` once `q` is empty — so *every* add pushed a fresh history entry, and
  Back walked through one intermediate cleared-query stop per add ever made in
  the session. `query_bar.rs` did not touch the general rule (still rule 4:
  refining replaces, starting or ending a search pushes) — the `reset` prop is
  quick-add's own field (only consumer), so it now bypasses `commit` for a
  dedicated always-`replace` navigation instead of changing what "ending a
  search" means generally.
- **Focus stuck open after Escape** — opening is `focusin`-driven (see the
  wrapper's `on:focusin` above), so a field Escape left focused could never
  re-fire that event; reopening needed a click away and back. Escape's close
  now blurs the field too, client-only (same shape as `catalog::
  focus_switch_item`).

Unit-extended (`decode` gets an explicit Escape-with-zero-rows case) and
kill-verified in `quick-add.spec.ts`: each of the four fixes was confirmed red
against the pre-fix code (`git stash` on the two touched files, one at a time,
server rebuilt between) before being confirmed green again. Full chromium
tier run for base-parity triage; no regression outside the suite's known
baseline.

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
  follow-up task rather than absorbed. *(Amended 2026-08-12: `N` / `N+` are
  page-one forms only. Past a cursor the page says `N results on this page` —
  "at least N" is false there, since the rows before the cursor are not
  counted. See "Catalog paging honesty".)*
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
    **Update (2026-08-12, P6-083):** the follow-up landed — `api_err` now
    returns `ServerFnError<shared::ApiError>` so the typed variant crosses
    this channel, and every consumer here classifies on the variant instead
    of the message prefix. The HTTP status itself is still a flat 500
    (`server_fn` 0.8.8 has no per-variant status hook without deeper
    surgery, confirmed while implementing) — this dispute's literal question
    stands unchanged, but the status was never what any consumer here read
    on this channel, so it stopped mattering.
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
  codes is the honest interim; filed. — **resolved 2026-07-26**, see the
  set-picker Findings below.

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

### Pinned "All cards" row: one honest target for the rail and the drawer (2026-08-13, P6-154)

`app/src/my/tree.rs` (`PinnedRow`'s `href`/`also` on the "All cards" row).
Size S. The rail and the mobile drawer are **one markup**
(`SidebarRail`/`CollectionTreeNav`, above) — a `md:hidden`/slide-over CSS
switch decides which *screen* it reads as, but CSS cannot change what a
shared `<a href>` points to. The pinned row shipped pointing at `/my`, which
is only the All-cards table at `md` and up (`AllCardsPage`,
`app/src/my/all_cards.rs`); below `md` the same route is the drill-down root
list (`app/src/my/root.rs`, P6-14x). So on a phone, tapping the drawer's "All
cards" row landed on a screen that looks like the drawer just closed onto
itself — a second tap into `/my/all` was still needed to reach the table.

**Fix: the row now targets `ALL_CARDS_PATH` (`/my/all`) at every width,
desktop rail included**, with `also="/my"` so the row still reads
`aria-current=page` when a caller lands on the table via `/my` some other
way (the desktop mode switch, the mobile bottom tab, breadcrumbs — none of
those changed). Chosen over rendering a different `href` per surface (a
prop/context threaded down to say "this render is the drawer"): nothing on
desktop actually depends on the row going to bare `/my` rather than
`/my/all` — both routes mount the identical `AllCardsBody`, and `/my`'s own
root list stays reachable from the mode switch, the bottom tab and every
breadcrumb, so pointing the shared row at the table everywhere costs
desktop nothing and fixes the phone case outright. **Desktop `/my` row
behavior, before → after:** before, clicking the row went to `/my` (already
correct there, since `/my` **is** the table at `md`+); after, it goes to
`/my/all` (the same table, at a route that also works below `md`) — visibly
identical on desktop, since `AllCardsTablePage`'s `back` link is `md:hidden`.

**e2e, updated deliberately.** Two tests in `responsive.spec.ts`'s "reaching
a screen by clicking, not by goto" block exist specifically to guard the
resource-id-collision class of bug from clicking into a page rather than
`goto`ing it (specs Findings above, `AllCardsPayload`) — both pinned the
rail's *literal* href, so both needed a call on whether they still cover
what they claim to after the target moved:
- `/my carries rows when reached by clicking All cards` → retargeted to
  `/my/all` (the sidebar row's new destination) and renamed accordingly;
  still proves a real fetch happens on a client-side landing at the row's
  target, now for the route that target actually is.
- `all-cards.spec.ts`'s "reached by a client-side navigation … shows the
  real rows" is a *different*, narrower guard: its regression was specific
  to `/my`'s own component order (`MyRootNav`'s `<Suspense>` ahead of
  `AllCardsBody` shifting the serialized resource id onto slot 12) —
  `/my/all` mounts no `MyRootNav` ahead of it, so retargeting this one would
  have stopped exercising the mechanism it documents. Left clicking into
  `/my`, but through the desktop mode-switch link instead of the sidebar row
  (the sidebar no longer reaches `/my` by a click at all).
- `collection-tree.spec.ts`'s pinned-rows test and `my-root.spec.ts`'s
  "the rail still marks All cards as where you are" both asserted the row's
  href literally; updated to `/my/all` to match.
- New: `responsive.spec.ts` → "the mobile drawer's All cards row lands on
  the table, not the drill-down list" (phone width, opens the real drawer
  via `rail-toggle`, taps the row, asserts `all-cards-table` visible after
  landing on `/my/all`; positive control confirms plain `/my` at phone width
  is still the root list). Kill-verified by hand: reverted the `tree.rs`
  change alone, confirmed this test fails (times out — no
  `a[href="/my/all"]` in the drawer), restored the fix, confirmed it passes
  again.

**Verification:** `responsive.spec.ts` + `collection-tree.spec.ts` full
(every test in both files carries `@fast`) 24/24 serial (`--workers=1`);
`my-root.spec.ts` + `all-cards.spec.ts` `@fast` 48/49 — the one failure
(`the location summary expands to the collections it names`) is a
pre-existing dev-seed data-shape gap ("dev seed should hold at least one
card in two collections"), reproduces solo against unmodified `main` too,
and touches none of this task's code (`all_cards.rs`'s location-summary
logic is untouched here). Gate: fmt clean; both clippy lines
(workspace-exclude native, `frontend` wasm) clean; `cargo test -p app
--features hosted` 360/360.

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

### `/my/*` stayed mounted once the page stopped reading its resource in setup (2026-08-11)

**The `/my/collections/:id` detach was the auth guard re-suspending, not the
router.** P6-068 (`specs/phase-6-probes/P6-068.md`) named the mechanism and this
task removed it. `CollectionPage` read `view_res` in its **setup body** — the
`here_delta` zeroing `Effect` and the `resolved` memo that fed the quick-add
panel — which is above every boundary in its own view. A `Resource` read
registers the *nearest `SuspenseContext` in the owner chain*, so those two reads
registered on `RequireAuth`'s `<Suspense>` (`app/src/shell.rs`), and every `?q=`
re-run of the resource made that boundary pending. With `TRANSITION = false` the
boundary swaps to its fallback on every re-suspension, and `EitherKeepAlive`
unmounts the whole `<Outlet/>` subtree and re-inserts the same nodes when the
fetch lands — hence "same nodes back", the blurred field, and a showing native
popover removed without a `toggle` (leaving `open` desynced).

**Three things that look like fixes and are not**, all recorded because each
costs a round to rediscover:

- **Wrapping the read in an `Effect` does nothing.** `in_effect_scope()`
  suppresses only leptos' debug warning; the suspense registration is
  unconditional. This is why `/catalog`'s `last_good` idiom does not port —
  `/catalog` is safe because `AppShell` provides no `SuspenseContext` above its
  `<Outlet/>`, not because its reads are in `Effect`s. Adding any `Suspense`
  above that `<Outlet/>` would hand `/catalog` and `/cards/:id` this defect for
  free.
- **A nearer boundary cannot be added.** Setup-body statements run under the
  component's own owner, an ancestor of every boundary its view contains.
- **Moving the consumers inside a boundary is ruled out by an existing
  constraint:** the query bar sits between the two `Transition`s deliberately, so
  a rebuild cannot rebuild the `<input>` under the caret.

**What shipped (Option B).** The header's `Transition` body writes two plain
signals from the payload it is about to render — `here_delta.set(0)` and a
`QuickAddFacts` value (destination, default kind, present matches, plus the
collection id) — and the out-of-boundary consumers read those. `RwSignal`'s
`try_read_untracked` does no `use_context::<SuspenseContext>()` lookup at all, so
a query refresh now *structurally* cannot reach the guard.

- **The header writes them, not the table.** The two boundaries await the same
  resource, so the writer picks the ordering, and `here_delta` admits only one:
  the delta corrects the header's totals, so it must be zeroed by the body that
  puts fresh totals on screen. The table writing it would leave a window with
  fresh totals and a stale delta still added on top — the teardown double-count
  the original `Effect` comment warns about. The reverse window is inert:
  nothing in the table *renders* from `here_delta`.
- **The quick-add destination is now retained across a re-search** instead of
  collapsing to `None` while the resource is in flight. Intended: `⏎` mid-search
  adds to the collection you are looking at, rather than hitting the "Still
  loading this collection" guard on the metric path. **The staleness that would
  be a real bug is gated by construction** — `live_facts` is a memo that returns
  the retained facts only while `?id` still parses to the collection they were
  built from, so a navigation to a *different* collection cannot leave a stale
  destination reachable in the window before its payload lands. A URL-keyed read
  rather than an `Effect` that clears, so there is no window at all.
- **The 120 ms focus keeper in `quick_add.rs` is deleted** — the subtree no
  longer unmounts, and an interval that steals focus back from `<body>` has a
  cost of its own (it fights any deliberate blur onto a non-focusable target).
- Option A (`RequireAuth` using `<Transition>`) was rejected though it is one
  token: it leaves the mis-wiring in place and only removes the surface it
  damages, and it becomes a real hazard the moment `CurrentUserResource` is made
  refetchable, since a Transition would then keep signed-in content on screen
  through a re-check.

**The rule this leaves behind**, now recorded on `RequireAuth` itself: a page
under `/my/*` must read its resources inside its own `Suspense`/`Transition`, and
anything a consumer outside one needs must arrive through a plain signal written
from inside it.

### Tables that name a collection went stale on a sidebar rename (P6-126, 2026-08-12)

`collection.rs`'s own module doc already names the fix (`TreeManage::revision`
as a resource source) but three other `/my` pages named a collection without
taking it: `/my`'s WHERE column (`all_cards.rs`), the needs page's "Owned
elsewhere" Where column and pick-list group names (`needs.rs`), and the
shopping list's "Wanted by" column (`shopping.rs`). None of their resources
referenced the tree at all — their sources were `(url_q, url_cursor,
holdings_revision)` or narrower — so a sidebar rename or delete left them
naming the old collection until an unrelated write (a search, a page turn, a
move) happened to bump one of the sources they *did* have. All three now take
`manage.revision` as an added source, the same trick and the same comment
shape `collection.rs` established. `recently_deleted.rs` (names only the
deleted collection's own row, which a live rename cannot reach) and
`root.rs`'s `MyRootNav` (reads the shell's `CollectionTreeResource` directly,
which a rename's own `tree.refetch()` already updates) were checked and left
alone — neither has the gap.

**e2e caught a pre-existing shared-fixture-pool trap while proving this.**
`collection-tree-manage.spec.ts`'s `unownedCards` helper verifies "owned
nowhere" against `/api/all-cards?limit=200`, which hard-caps at 200 rows; the
dev user now owns more than that, so a card past the cap can already be held
without showing up as "taken" — and every test in that file that relocates
its scratch collection's holdings on cleanup (the generic `deleteCollection`
helper's default `ToParent`, not `Discard`) leaves that card permanently
"owned" for the next run drawing from the same pool front. The new rename
test routes around it with its own `genuinelyUnownedCard` (re-verifies a
candidate against the unpaginated per-card holdings read before trusting it)
and cleans up with `Discard` rather than relocation. The underlying pool-drain
is pre-existing and out of scope here — it is the same fixture-pool class the
e2e-suite skill already tracks under `WB-01KZMVA2Y1` — but is worth restating
because it makes any test drawing from `unownedCards(request, 1)` at a low
`skip` fragile over time, not just this one.

### …and on the collection page that fix was too blunt (P6-127, 2026-08-12)

`TreeManage::revision` is bumped by **every** tree mutation, so taking it as a
whole-resource source means any of them refetches the whole payload. On the
three pages P6-126 fixed that is correct and cheap — their payloads are read-only
tables. On `/my/collections/:id` the same payload also carries the **card table**,
and rebuilding that table re-seeds every `CountStepper` from a fresh `value`
signal while disposing the one the count's own undo toast still points at. The
toast's `undo` closure bails on a disposed signal, so:

> commit a count → rename anything before the toast expires → Undo silently does
> nothing, and the write never happens.

That is the identical defect `collection.rs`'s module doc already records against
awaiting the *tree* resource in the table's boundary, reached from the other
direction — and the same whole-table-rebuild jank on an unrelated sidebar rename
that P6-068 had just removed.

**The narrowing.** `TreeManage` gained `content_revision`, bumped *in addition
to* `revision` by the subset of mutations that can move copies or move which
collection they roll up into: a delete (which relocates the node's holdings and
desires into the chosen destination), its undo, and a reparent (drag or picker).
Not a create (an empty collection), not a rename (a string), not a pure sibling
reorder (nothing moves anywhere). `collection.rs`'s `view_res` takes
`content_revision`; the /my pages keep `revision`.

**What a create or a rename changes there comes from the tree instead** — which
every tree mutation already refetches, and which this page already reads in
nested boundaries for the breadcrumb, the folder counts and the teardown
destinations. One more nested `<Suspense>` publishes a `TreeFacts { id, name,
children }` into a plain signal (the P6-068 write-inside/read-outside pattern),
and four consumers read it: the `<h1>`, the folder rows' identity, the quick-add
destination's name, and the header kebab's `menu_target` snapshot (subject name,
`parent_id` and child count). Every one falls back to the payload's own copy
when the tree does not know the node — a collection the cached tree predates, or
a failed tree read — so a broken tree leaves the page exactly as complete as it
was before. P6-111's ruling is honoured in its *failed*-tree half (the fallback);
its *stale*-tree half is deliberately traded away: a tree read in flight after a
delete/reparent can transiently disagree with the fresher payload (child count,
folder rows) until the round trip lands. See the amendment in
specs/collection-deletion.md.

Three consequences worth knowing before editing that file again:

- **The publisher is the *last* child of the page view, deliberately.** The
  route is `SsrMode::Async`, which renders in document order once every resource
  has resolved, so a publisher above its consumers would put tree-derived names
  and rows in the server HTML while the client's first pass — where the signal
  starts `None` — renders the payload's. Publishing last makes both passes read
  the payload and lands the correction one tick later, client-side only.
- **"Is the table empty" became a live question.** Folder rows are tree-derived
  now, so `New binder inside…` can add the *first* row to a collection whose
  payload still says it is empty. `CollectionBody` decides between `EmptyState`
  and the table off a `Memo<bool>`, not a raw read: that closure rebuilds the
  card table when it re-runs, which is the exact thing this task exists to
  prevent, and deduping to the boolean means only a genuine empty↔non-empty flip
  can do it (and in that branch there are no card rows to lose).
- **`here_delta`'s zero-write did not move** and its ordering argument is
  unchanged — it still lives in the header's `Transition` body, one statement
  before the header is built from the same payload. It is now *only* reached by a
  payload that genuinely changed, which is strictly tighter than before. On a
  rename the header is not re-rendered at all, so the delta is not zeroed and
  the committed-but-un-refetched count keeps agreeing with the HERE cells.

**Per-page calls.** `collection.rs` — hazard present (the only `CountStepper`
call site in the app), narrowed as above. `all_cards.rs`, `needs.rs`,
`shopping.rs` — no hazard, simple `manage.revision` source kept: none hosts a
stepper, and the one undo any of them raises (needs's pull toast) addresses the
server by `move_id` rather than a client-held baseline signal, so a refetch that
rebuilds the row cannot turn the undo into a no-op.

Pinned by `collection-tree-manage.spec.ts` → "a rename mid-toast leaves the
stepper's Undo working", kill-verified against the pre-fix code (the count stuck
at 5 after Undo).

### Catalog paging honesty (2026-08-12)

`app/src/catalog.rs` + `app/src/catalog/rail.rs` — four defects from the catalog
paging review (P6-130…133), batched because they are one defect wearing four
hats: **`<Transition>` keeps the previously-resolved page on screen while a
newer search runs, so the URL and the rendered results routinely disagree, and
every one of these read the URL.**

**The fix is structural, not four patches.** `SearchPayload` now echoes the
request it answered (`q`, `cursor`) alongside the result, and everything that
describes *the page on screen* — which page it is, how many rows it holds,
whether its pager can be clicked — reads the payload. The URL is still the
source of truth for *what to fetch*; it stopped being the source of truth for
what is displayed. (This is also half of the payload-echo the file's own
`initial_value` note asks for against same-type cross-decode; the rejection half
is still not built.)

- **P6-130 — a stale "Next page →" reverted typed text.** The old pager's href
  carries the old `(q, cursor)`, and an anchor click goes around
  `QueryBar::commit`, so the query bar's re-seed effect sees the URL move
  without it and rewrites the box. Both pager links are now **inert while the
  results under them no longer answer the box** — `aria-disabled`, dimmed, and a
  click that calls `preventDefault` (which `leptos_router`'s window bubble
  listener honors — load-bearing on `tachys/delegation` being OFF in this
  build; see `PageLink`'s doc for the `on:click:undelegated` escape hatch if
  that ever changes). *Inert, not removed*: the results deliberately stay on screen
  during a search, so dropping the control — or its `href`, which is the same
  thing to the tab order — would flicker the pager and lose keyboard focus
  mid-navigation. Staleness is `rendered q ≠ url_q ∨ rendered q ≠ box text`; the
  second disjunct covers the ~250 ms before the debounce fires, the first covers
  the flight after it.
- **P6-131 — `last_good` leaked a page-N set across queries.** It kept whatever
  resolved OK last, including a cursored page, and the error arm rendered it
  dimmed under the next grammar error — rows 2..n of a search the reader had
  left, labelled "Previous results". Now **only page one is retained and paging
  away forgets it**, and the kept page is tagged with its query: the error arm
  shows it only when one query is a prefix of the other (`same_search`), which
  is what "still editing this search" looks like as a string. The behavior the
  set exists for — `bolt` → `bolt pow>3` keeping `bolt`'s page dimmed — is
  unchanged and still pinned by its own test.
- **P6-132 — the count misstated page N as the whole set.** Now `count_label`:
  `23 results` / `50+ results` on page one, `50 results on this page` past a
  cursor. **No count query and no page ordinal**, deliberately — a count query
  behind a search-as-you-type box runs per keystroke, and an ordinal is a new
  URL parameter every writer of a catalog URL must thread and keep in sync with
  a cursor that can also arrive from a shared link with no ordinal beside it.
  The qualifier needs no new state to be true. The mobile sheet's footer renders
  the same phrase (`Show 50 results on this page`), because it was the same
  claim from the same number.
- **P6-133 — two pager rendering edges.** (a) "Am I past the start?" is the
  rendered payload's cursor, not the URL's, so page one no longer grows a "Back
  to the start" the instant Next is clicked. (b) `<nav aria-label="Pagination">`
  is not rendered at all when there is nothing to page — it used to wrap an
  empty `<span>`, which is a named landmark announced as navigation that then
  contains nothing.

**`/my` shares the bug class but not the component.** `all_cards.rs:612` and
`collection.rs:2186` each carry their own copy of `Pager` (three near-identical
components, no shared one), and both read a `paged: Memo<bool>` off the URL and
render the same empty `<nav>` on a single-page set. Not touched here — the brief
scoped this to `/catalog`, and the honest fix for `/my` is to extract the one
pager rather than patch two copies. Filed rather than absorbed.

**Evidence.** `cargo test -p app --features hosted` 298 green (3 new unit tests:
the count phrase, the pager-landmark decision, the refinement predicate); fmt +
clippy (workspace, wasm, `native`, `hosted,component-bench`) clean; six new
`catalog.spec.ts` tests, **all six kill-verified** in one stash cycle against
the pre-fix build (26 pre-existing catalog tests stayed green in that same run,
so the six fail for their own reasons); `@fast` catalog + filter-rail + states
68/68; hydration CLEAN on `/catalog`, `?q=bolt`, an error query, a cursored
browse-all and `?q=bolt&view=list`.

**The in-flight window is now testable.** `holdSearches(page)` gates
`**/api/search_catalog*` behind a promise the test releases, which is the only
way to stand inside "old results, new URL, nothing resolved". Two of the six
tests are meaningless without it. One trap it cost: `page.unroute` while a
handler is mid-`continue` fulfils the route itself and the `continue` then
throws "Route is already handled" — the handler stays installed instead.

### Board-aware needs rows (2026-08-12, P6-074)

The needs page's rows are now per `(oracle, board)`, because `NeedRow` is
(collection-api Findings, same date, carries the query and pull-path reasoning).
This section records only what the *page* does with the new field.

**The board is shown the way the deck page shows it: by silence for the
mainboard.** `group_deck` already labels a deck's sections "Instants" vs
"Sideboard · Instants" — main is the unmarked case. The needs rows follow that
exactly rather than inventing a second vocabulary: `my::collection::board_label`
was extracted from the same `BOARD_ORDER` table and is now the one source both
read, so the two pages cannot call the same board different things. A binder's
desires are all `main`, so the label never appears outside a deck, and inside one
it appears only where it distinguishes two rows of the same card. Rows carry
`data-board` alongside `data-oracle`, matching `collection-row`. The pick-list
lines carry it too — a card wanted on two boards produces two lines in the same
group, and without the tag they would read as a duplicate.

**The chip's semantics did not change, only its inputs.** `needs_chip` still
formats `CollectionTotals`, and `totals_of` still sums per-row gaps; a card
missing on two boards simply contributes two rows. The header's totals and
`read_needs_rows` no longer merely *agree* on grain — since the review they are
the same read (`read_need_gaps`) folded by the same function, so there is no
second copy left to drift.

**A row offers only its own share of a card's elsewhere copies.** The pool is
per-card and shared between that card's board rows, so it is apportioned across
them (mainboard first) rather than offered whole to each; a row's pull lines are
allocated from `owned_elsewhere`, not from its raw gap, through the one
`offers_of` the pick list, the row button and the server planner all call. The
visible consequence: a card wanted on two boards with only one copy elsewhere
shows **one** pullable line and a Short row, not two pullable lines — and the
chip says "1 owned elsewhere · 1 to buy" instead of claiming both are covered.

**Two page texts were false after the change and were rewritten, not left.** The
empty state's "Unfilled board slots aren't counted here" — added in the
2026-07-25 review precisely to keep the board-blind claim honest — now says the
remaining true caveat: moving a copy you *already hold* between boards is a
relabel, not an acquisition. The subtitle gained "board by board". `states.spec.ts`
asserted on the old wording and moved with it.

### Pull honesty: a reconciled pick list, a stated empty outcome (2026-08-13, P6-141)

Two silent paths in `/my/collections/:id/needs`'s Pull, both closed without
touching the deliberate snapshot-outside-`Transition` design (module doc,
`app/src/my/needs.rs`) that the 2026-07-25 review put there.

**(A) A row-level Pull closing a need left the open pick list holding a dead
line.** The checklist is generated once, from `Owned elsewhere`'s own rows, and
deliberately does not refetch as the table above it does — that is the whole
point of living outside the `Transition`. But the *table* has a second control
the snapshot cannot see either: `ElsewhereRow`'s one-tap Pull button, which
closes the same need through a completely different write. Before this task, a
line whose need closed that way stayed on the walk looking exactly as pullable
as a live one; ticking it could only ever land
[`SkipReason::NoLongerNeeded`](../app/src/my/move_selection.rs) — a real toast,
not a crash, but a dead-end the checklist itself should never have offered.

The fix is client-side pruning at the moment of proof, not a wider snapshot.
[`drop_closed_need`](../app/src/my/needs.rs) mirrors the selection tray's own
reconcile-after-write policy (P6-122, `SelectionState::remove_tokens`): once a
write proves a key is spent, prune it rather than wait for the next tick to
discover the same thing the hard way. It is safe to call only from
`ElsewhereRow`'s row button, and only when that call's own outcome is
[`PullLineOutcome::Full`](../app/src/my/needs.rs) against the row's *whole*
`owned_elsewhere` — the row button's `items` names every source the row's
allocation offers in one request, so `Full` there proves the need is closed for
every source, not just the one this call happened to draw from. A pick-list
*tick* cannot reuse the same check: it only ever asks for one line's own share,
and a `Full` tick on one line of a multi-source gap says nothing about the
sibling lines still open on other sources
(`the_pick_list_groups_by_the_collection_you_walk_to` pins exactly that split).
Lines already ticked are left alone by `drop_closed_need` — they are the record
of what was actually pulled, not a stale offer — and a group emptied by the
prune is dropped with its lines, so no "walk to this collection" heading
survives everything it named.

**(B) An outcome with nothing moved and nothing refused raised no toast at
all.** `report()` had exactly two arms — a success toast when `move_ids` is
non-empty, a `SkipReason`-phrased toast per entry in `skipped` — and a call
whose `items` resolved to neither fell through both in silence, which reads as
an unremarkable success rather than the refusal it is.
[`PullOutcome::is_empty`](../app/src/my/needs.rs) names the shape explicitly
and `report()` now states it: "Nothing to pull — *card* had nothing to move".

**Recorded rather than papered over: this path is not reachable from either
current UI caller today.** `PickRowView`'s tick always sends exactly one item;
`ElsewhereRow`'s row button always sends `offers_of(&row)`, which cannot be
empty for a row rendered in the `Owned elsewhere` bucket (`owned_elsewhere > 0`
guarantees at least one location with positive quantity — the same identity
`a_pick_list_adds_up_to_the_owned_elsewhere_bucket` pins). `dedupe` never drops
below its input length, either, so a non-empty `items` always yields a
non-empty union of `pulled`/`skipped` server-side
(`a_pull_with_no_items_plans_nothing_and_names_no_refusal`,
`app/src/backend/pull_plan.rs`, pins the *empty*-items case that motivates the
guard). The fix is verified two ways instead: a pure unit test on
`PullOutcome::is_empty` (`app/src/my/needs.rs`) and a Playwright test that
rewrites the outgoing `pull_needs` request to carry `items: []` before it
reaches the server (`needs.spec.ts`, same pattern `page.route` already uses for
the P6-140 in-flight test) — genuine browser-level proof of the toast, on a
shape the UI cannot construct on its own. A caller that ever *can* produce
empty `items` — none exists today — inherits the honest toast for free rather
than needing its own fix.

**Evidence.** `cargo test -p app --features hosted` 333 passed (26 new/changed
in `my::needs`, 1 new in `backend::pull_plan`), 0 failed. Full chromium
`needs.spec.ts` at `--workers=1` (this file races itself — see its own header):
**11/11 on the fix branch minus 6 known-environmental** = 5 passed, 6 failed —
the same 6 (`the two buckets split the gap…`, `Pull moves the copies it
names…`, `Pull all groups the walk by source…`, `a line that finds less than
it asked for…`, `the shopping list states the shortfall…`, `one copy elsewhere
cannot cover two boards…`) reproduced byte-for-byte on base with the fix
stashed out, confirmed unrelated to this task (traced live to a stray holding
of tens of copies on an oracle `unownedCards` believed free — its own doc
already records the "owned nowhere" check is a first-200-rows read, not a
complete one). **Both new tests kill-verified**: on base (fix stashed), "a
row-level Pull that closes a need…" fails on the dropped-line assertion (the
stale `pick-row` stays, count 1 not 0) and "a pull whose items resolve to
nothing…" fails on the toast assertion (no `Nothing to pull`, element not
found — genuinely silent, exactly as filed); with the fix restored both pass
and the failure set returns to exactly the same known 6.

**Addendum: Undo on a reconciled row-level Pull was itself a silent drop
(review caught, fixed same task).** The first cut of (A) dropped the stale
line but never put it back — the row-level Pull's toast carries the same
Undo every pull's toast does, and Undo's generic `undo_selection_move` +
`revision.bump()` already makes the *table* row reappear, but nothing
restored the checklist. An open pick list would silently omit a live need for
the rest of the session after an Undo — two representations of one need
disagreeing, the same honesty failure this whole task exists to close, just
moved one step later.

Fixed by making the drop reversible rather than final.
[`drop_closed_need`](../app/src/my/needs.rs) now returns what it removed —
[`DroppedPick`], the line plus enough group identity to reinsert it —
alongside the pruned groups. [`restore_dropped`] performs the reinsertion
(recreating a group if the drop took its last row), and
[`restore_dropped_if_current`] is the actual on-undo gate: it refuses to
splice into a checklist that has moved on since the drop — closed, reopened,
or simply regenerated by a second "Pull all…" click — by comparing a
`picks_generation` counter (`NeedsPage`) bumped only on a fresh
`pick_list()` call, never on a reconcile-prune. `ElsewhereRow`'s `pull`
captures the dropped lines and the generation at drop time and wires them as
`report()`'s `on_undo` — the same slot the pick-list tick's own Undo already
uses to un-tick, mirrored rather than reinvented. The glue condition
(`PullLineOutcome::Full` deciding whether to reconcile at all) was pulled out
as its own named [`row_pull_closed_the_need`] so it has a unit test
independent of the e2e proof.

Evidence: `cargo test -p app --features hosted` 336 passed (3 more than the
first cut — the round-trip, the since-regenerated-checklist refusal, and the
glue condition), 0 failed. `needs.spec.ts` extended with an Undo leg on the
reconcile test, kill-verified against the *first* fix commit (3e94104, which
has the drop but not the restore): fails there exactly as predicted — the
checklist's `pick-row` count stays 0 after Undo instead of returning to 1.
With the restore-fix applied, full chromium `needs.spec.ts` at `--workers=1`
returns to the same 5 passed / 6 failed (the identical known-environmental
six), both P6-141 tests green including the new Undo leg.

**Addendum: the sibling gap this task named but did not close — ⌘K's own
Undo (2026-08-13, P6-144).** (A)/(B) above closed the `Owned elsewhere`
table's row-level Pull as a reconcile source. There was a second one: ⌘K's
`Undo last move` (`app/src/components/palette.rs`) reverses a pull
server-side through `undo_selection_move` + `revision.bump()` — the exact
same primitives the toast's own Undo uses — but it is wired at the *shell*,
not the page, and never touches `picks`/`done` at all. A line ticked through
the checklist stayed struck-through after ⌘K put its copies back, and
`toggle` refused to re-tick it (deliberately: a tick is one-way, unticking
would have to reverse a move and that reversal already has a name). The
toast's own Undo button already worked, because `PickRowView`'s `toggle`
wires it directly to a per-line `on_undo` closure that removes the token
from `done`; ⌘K has no such closure to call — it does not know this page, or
any page, exists.

**Mechanism: revalidate `done` against a fresh needs read, not a second event
channel.** `reopen_done` (`app/src/my/needs.rs`) re-runs [`pick_list`] — the
*same* function that built the checklist in the first place — over a fresh
`NeedRow` slice, and drops from `done` any token that function offers again.
`PickListPanel` wires it to `needs_res.get()` in an `Effect`: `needs_res`
already refetches whenever the holdings revision bumps (one of its own
`Resource::new` sources), so this piggybacks on a fetch that was already
happening rather than opening a second one or teaching the palette about
page state it has no business owning. An event channel carrying move ids
from ⌘K to the needs page was the alternative on the table and was not
needed — the fresh-read comparison turned out sound on its own (next
paragraph), and a channel would have had to be re-derived for every future
undo path this page does not yet know about (a second command, a keyboard
shortcut, anything), where the revision-based reconcile covers all of them
for free by construction.

**Why a normal tick's own just-closed line survives unscathed — the thing
that had to be proven, not assumed.** A revision bump follows a tick that
*actually* closed a need just as much as it follows ⌘K reversing one — the
reconcile cannot (and does not try to) tell those two apart by cause. What
it reads instead is what each leaves behind: a tick that really moved the
copies leaves that token's source spent in the very next read, so
`pick_list` over the fresh rows no longer offers it, and `reopen_done`
correctly leaves it in `done`. The server's own `needs()` filters a closed
need out of the read entirely (`desired > present_here`) rather than handing
back a zero-gap row, so there is no row left to accidentally re-offer from
either. Only an actual reversal — of a tick, or of anything else that puts
copies back on that exact `(oracle, from_collection, board)` — makes the
token reappear, which is exactly the condition that un-sticks it. Pinned by
two unit tests rather than argued in prose alone:
`a_normal_ticks_own_line_survives_reconcile` (fresh read empty — closed and
absent, as a real server read would show) and
`an_undone_ticks_line_is_reopened` (fresh read is the pre-tick shape again).
A third, `reopen_done_leaves_other_ticked_lines_alone`, pins that two ticked
lines in one session are judged independently — only the one whose need
actually reopened un-sticks.

**Evidence.** `cargo test -p app --features hosted`: 340 passed (4 new —
`an_undone_ticks_line_is_reopened`, `a_normal_ticks_own_line_survives_reconcile`,
`reopen_done_leaves_other_ticked_lines_alone`, plus one from the surrounding
diff), 0 failed. `fmt --check` clean; every clippy gate line (workspace
excl. `frontend`/`three_rings`, wasm `frontend`, `app --features native`,
`app --features hosted,component-bench`, `app --features
hydrate,component-bench` on wasm) clean.

New e2e test in `needs.spec.ts`: "⌘K's Undo last move un-sticks the pick
list's own ticked line" — ticks a pick-list line (asserts `data-state`
struck to `"pulled"`, checkbox `"checked"`, holdings moved), opens the
palette and runs `Undo last move` (not the toast's own button — a control
the pick list's on-undo wiring never sees), asserts the line returns to
`data-state="todo"`/unchecked, and — the base-bug proof, not just a
rendering check — re-ticks it and asserts a *second* real pull succeeds
(since `toggle` refuses a checked box outright, this is the only way to
prove the line is genuinely un-stuck rather than merely drawn unstruck).
**Kill-verified** against base (the two Rust files stashed, `needs.spec.ts`
kept): fails exactly as predicted, on the un-tick assertion — `data-state`
stays `"pulled"` where `"todo"` was expected. With the fix restored, the
same test passes standalone.

**Full-file base-parity, and an honest note on live conditions.** A serial
`needs.spec.ts` run at `--workers=1` against the fix returned **7 failed / 7
passed** — one more failure than the file's own documented "6
known-environmental" baseline, the extra one being this task's own new test.
Run again immediately against base (Rust files stashed, same test file, same
running server) under the same conditions: **byte-identical 7 failed / 7
passed**, same seven test names including the new one — proof the extra
failure is not this fix's doing. Root-caused live: the shared `q=n` catalog
pool `unownedCards` (and this task's own `trulyUnownedCard`, added because
this test needs a card with provably zero holdings anywhere — see its own
doc comment on why a stray holding elsewhere would draw the pick list's
allocation off the collection this test built, reading as this test's bug
instead of the pre-existing, already-filed
`unownedCards`-is-a-first-200-rows-check-not-a-complete-one gap this same
Findings section named above) was down to single digits at run time — most
likely concurrent draw-down from other sessions against the same shared Neon
dev branch, not a regression. `command-palette.spec.ts` full serial run: 3
failed (the file's own known fixture-pool class), 16 passed; the palette.rs
diff for this task is a comment only, so no base-parity run was needed there
— it cannot change runtime behavior by construction.

**Open question, recorded rather than guessed at:** whether the `q=n`/`q=z`
per-file pool convention needs a real systemic fix (a shared, much larger
term, or a pool that gets minted rather than searched for) is still the
follow-up this Findings section already flagged in the first P6-141 entry —
this task hit the same wall harder than usual, not a new one.

### ⌘K respects open overlays (P6-149, 2026-08-13)

Before this fix `PaletteBody`'s chord handler (`app/src/components/palette.rs`)
toggled `open` unconditionally: `open.update(|o| *o = !*o)` on every ⌘K/Ctrl+K,
with no regard for whatever else was already on screen. Opening a tree dialog
(create/rename/delete/move, all `Dialog`-hosted) and then pressing ⌘K stacked
the palette's own `CommandDialog` on top of it — two modals, one scrim, the
underneath dialog's focus trap and Escape/Tab handling both still wired but
now unreachable behind the palette.

**The fix reads the overlay stack the palette already pushes onto,
rather than tracking a second copy of "is something else open".**
`CommandDialog` wraps the vendored `Dialog` (`command.rs`), so `PaletteSurface`
already registers `PALETTE_ID` on `super::ui::overlay_stack`
(`app/src/components/ui/overlay_stack.rs`, P6-125/P6-189 lineage — the same
stack `Dialog`, `Sheet` and `Popover` push/pop, and that gates their own
Escape/Tab) every time it opens or closes. The new pure decision function
[`palette_chord_target`] (`palette.rs`) takes `(currently_open, palette_is_top,
stack_is_empty)` and returns `Option<bool>` — `None` meaning "swallow the
chord, change nothing":

- **Closed → open** only when the stack is empty. A non-empty stack means some
  other overlay (a tree dialog, a `Sheet`, a `Popover`) is genuinely showing,
  and opening on top of it is exactly the bug this task closes.
- **Open → close** only while the palette is still the *topmost* overlay —
  mirroring the "topmost only" rule `dialog.rs` already applies to Escape and
  Tab, for the same reason: if something else somehow opened above the
  palette, ⌘K should not yank the palette out from under it.

[`overlay_stack::is_empty`] is new — the stack already had `is_top` (the ESC
gate) and that alone cannot answer "is anything open", since a *closed*
palette has already popped its own id and every id `is_top` would ever be
asked about is gone by the time the closed branch needs an answer.

**Known gap, recorded rather than silently worked around: the quick-add panel
is not on this stack.** `quick_add.rs`'s own module doc explains why it is
deliberately *not* built on `Dialog`/`Popover` (measured light-dismiss and
same-page-navigation failures in the native Popover API — see that file's doc
for the details) — it is a plain absolutely-positioned panel with its own
`open: RwSignal<bool>`, created fresh inside `QuickAddPanel` per collection
page and never exposed through a context provider the shell-level palette
could read. `overlay_stack` has no way to know quick-add is open, and this
task's gate has no way around that without either (a) wiring quick-add through
`overlay_stack` itself — a real behavior change to a surface this task did not
otherwise touch — or (b) growing a second, bespoke channel just for this one
caller. Neither was in scope for an S-sized fix; ⌘K over an open quick-add
panel still stacks the palette on top of it today. **Not yet filed as a
Workbook task** — this task's instructions were explicit no-`workbook`; a
follow-up task still needs filing by whoever picks this back up.

**Evidence.** `cargo test -p app --features hosted`: 348 passed (2 new —
`a_closed_palette_opens_only_onto_an_empty_stack`,
`an_open_palette_closes_only_while_it_is_still_topmost` — pure unit tests on
`palette_chord_target`, no wasm/DOM needed). New e2e test in
`command-palette.spec.ts`, "⌘K does not stack over an already-open tree
dialog, and works again once it closes": opens the tree's own create dialog
directly (the same background-right-click path
`collection-tree-manage.spec.ts` uses), types into its name field, presses
⌘K, and asserts the palette dialog stays `data-state="closed"` while the tree
dialog stays `data-state="open"` with its typed value intact (not merely
`open` — genuinely untouched); closes the tree dialog without submitting, then
confirms ⌘K opens the palette normally with nothing else open. **Kill-verified**
against base (palette.rs and overlay_stack.rs stashed, test file kept): fails
exactly as predicted, on the closed-state assertion — the palette dialog reads
`data-state="open"` where `"closed"` was expected, i.e. it stacked. With the
fix restored the same test passes standalone.

Full serial `command-palette.spec.ts` run (`--workers=1`) with the fix:
**4 failed / 17 passed** (21 total incl. the auth-setup step). Base-parity: the
same 3 failures this file's own header already documents as the fixture-pool
class (the three `Undo last move` tests, which write through the shared
`unownedCards` catalog pool against the shared Neon dev branch), plus one
more — "a no-match query says so instead of erroring" — reproduced
byte-for-byte on base with this task's Rust changes stashed (same failure,
same assertion, same locator not found), so it predates this task and is not
this fix's doing. The `palette.rs`/`overlay_stack.rs` diff cannot be
implicated in either class: neither touches ranking, the empty-state markup,
or the undo-ledger code the four failures exercise.

### Catalog grid cap at wide viewports (P6-098, 2026-08-13)

`GRID_CLASS` (`app/src/catalog.rs`) topped out at `xl:grid-cols-6` with
nothing capping the container, so a card tile kept growing with the window
past `xl`'s 1280px breakpoint — measured comically large (~500×700px cards,
one row filling the screen) at 3440px wide.

**Investigated whether a content-column cap already exists (it does, and it
isn't what "tray centres on the content column" means).**
`responsive.spec.ts`'s "tray centres on the content column, not on the
window" test (and `shell.rs`'s `SelectionTrayDock` doc comment) uses "content
column" for the area right of the sidebar rail (`<main>`, `flex-1`,
`RAIL = 240` px at `md`+) — that area is *not* width-capped, it is simply
"whatever's left of the viewport after the rail". `<main>` carries no
`max-w`, and neither does any `/my` page's own outer container
(`app/src/my/all_cards.rs` et al. — checked directly): those pages don't cap
their page shell either.

**What actually caps something in this app already: `Table`.**
`components/ui/table.rs`'s `Table` carries `max-w-7xl` baked into its `clx!`
definition (`"w-full max-w-7xl text-sm caption-bottom"`), with no `mx-auto` —
left-flush, not centered. So `/catalog`'s own list view (`ResultsList`, which
renders through `Table`) and every `/my` table page were already capped and
don't share this defect at all; only the grid view was unbounded. The fix
adds the same `max-w-7xl` (no `mx-auto`, matching `Table`'s left-flush
convention) to `GRID_CLASS`, so the grid caps the identical way the app's one
existing wide-viewport cap already works, rather than inventing a new
convention or centering the page. Below the cap (viewport content width <
1280px, e.g. desktop 1440px minus the rail) nothing changes; above it the
grid simply stops growing.

**Verified visually.** Playwright screenshots of `/catalog` at 1440, 2560 and
3440 px, before (`git stash`, forced a rebuild, confirmed the served HTML
reverted to the old class list) and after: at 1440 the two are pixel-similar
(cap not yet reached); at 2560 and 3440 the "before" grid keeps growing card
size with viewport width (one row of six huge cards at 3440), the "after"
grid holds card size constant between 2560 and 3440 (identical column
geometry) with the extra width as trailing whitespace. Confirmed the
`max-w-7xl` rule is actually emitted, not just referenced: `grep` on both the
dev-server's live `tailwind.css` and the `--minify`d release `app.css`
(`CARGO_TARGET_DIR=target/gate cargo leptos build --release`) both show
`.max-w-7xl{max-width:var(--container-7xl)}`.

**Evidence.** `cargo test -p app --features hosted`: 360 passed, 0 failed.
`npx playwright test --project=chromium --workers=1 --grep @fast
tests/catalog.spec.ts tests/responsive.spec.ts`: 47 passed, including "the
tray centres on the content column, not on the window" — unaffected, since
`<main>` itself carries no cap and this change is scoped to the grid's own
class list. Release build (`CARGO_TARGET_DIR=target/gate cargo leptos build
--release`) succeeded and its minified CSS carries the new rule.

### Batch move's two gaps on `/cards/:id` and the tray picker (P6-152, 2026-08-13)

Two small, unrelated defects sharing one root cause — the batch move's
client-side propagation was built for `/my` and its collection view and never
extended to the third holdings-rendering surface, `/cards/:id`.

**A — `/cards/:id`'s "Your copies" block did not hear a move.**
`HoldingsRevision` (this file, "Other decisions", 2026-08-11) is provided by
the shell and consumed as a resource *source* by `all_cards.rs` and
`collection.rs` — that sentence should now read **three** pages, not two.
`cards::CardDetailPage`'s own `detail_res` never took it, so a batch move
performed while parked on a card's detail page left "Your copies" naming the
pre-move collection until an unrelated reload — silent staleness, not a crash,
which is exactly the class this mechanism exists to close everywhere else.
Fixed the same way `all_cards.rs`/`collection.rs` already do it:
`crate::my::move_selection::holdings_revision()` (the public helper, not the
`HoldingsRevision` context type directly) joins `oracle_id` in the resource's
source tuple. The helper is what makes the `Option`-context question moot —
it degrades to a constant `0` signal outside the shell (the bench) rather than
requiring every caller to `use_context` and branch, so `/cards/:id` stays
mountable there unchanged.

**B — the tray picker's empty line spoke past what it could see.**
`MoveSelection`'s `DestinationList` overrode `empty="No collection to move
to."`, and — per `DestinationList`'s own doc, "`empty` can only ever speak
about *filtering*" — that string fires for a *filtered* zero too, not only a
failed or genuinely-empty read. Typing a search term that matched nothing
therefore claimed the user had nowhere to move copies, which was false: their
collections were sitting right there, unfiltered. Fixed by dropping the
override — `DestinationList`'s own default ("No collection matches.", the
catalog picker's own wording) is simply the true sentence here too, and the
1-line diff is the whole fix.

**The zero-collections case this override used to (over-)cover cannot happen
in the tray.** `CollectionStore::list_collections` — the read behind
`collection_list()`, which is what backs this picker — calls `ensure_inbox`
before it returns rows (`hosted.rs`; collection-api.md → "Inbox provisioning":
lazy on first authed load, idempotent via `collections_one_inbox`). So the
very request that populates this list provisions the caller's undeletable
Inbox row as a side effect; the registry it renders from is never really
empty, only ever filtered down to nothing. (The tree's own `Move to…` picker
in `tree_manage.rs` carries the identical override with the identical
overreach — flagged, but out of scope: it is a different feature/dialog and
the task named only the tray.)

Both gaps are one line each in `app/src/cards.rs` / `app/src/my/move_selection.rs`
respectively (plus updating the one bench doc comment in
`app/src/bench/selection_tray.rs` that quoted the old wording).

**e2e.** Two new tests in `batch-move.spec.ts`. (A): select a row on `/my`,
follow its own link into `/cards/:id` — a real SPA navigation, not `page.goto`,
since the in-memory tray selection has to ride along (P6-122) — move through
the tray *parked on the detail page*, and assert "Your copies" updates with no
navigation between the write and the read. Kill-verified: with `cards.rs`
stashed back to base the same test fails exactly as predicted (`your-copies`
still names the pre-move and Inbox collections, never the destination);
restored, it passes. (B): open the tray picker, type a term nothing matches,
assert "No collection matches." (not "No collection to move to."). Kill-
verified the same way against `move_selection.rs`.

**A fixture surprise surfaced building (A), reinforcing rather than
contradicting the existing P6-126 Findings note on this file's own
`unownedCards` helper.** Today's dev seed's bulk catalog load has grown to the
point that the Inbox now holds real, nonzero quantities of *nearly the entire
q=z catalog slice* this suite draws candidates from — not merely "more than
200 distinct printings" (the original caveat's framing) but close to all of
it. A first attempt at (A) assumed a `genuinely unowned` card could still be
found by walking further into the candidate list re-verified against the
unpaginated per-card holdings read (the same guard
`collection-tree-manage.spec.ts`'s rename test already uses); at `n=48` past
`skip=30` **zero** candidates were genuinely free. The test does not depend on
finding one: it adds a copy to a fresh scratch collection regardless of
what else the seed already holds, then drives *whichever* path the tray
actually takes — a direct resolve, or the which-copies step (P6-151) — rather
than assuming single-place resolution, and answers the step by ticking only
the scratch collection's own row when it appears. Not filed as a new Workbook
task (this task's instructions were explicit no-`workbook`, the same
constraint P6-149's Findings entry above recorded) — a follow-up on the
fixture-pool class (already tracked under `WB-01KZMVA2Y1`) should fold this
observation in when picked back up.

**Evidence.** `cargo fmt --all -- --check` clean. `cargo clippy --workspace
--exclude frontend --all-targets -- -D warnings` and `cargo clippy -p frontend
--target wasm32-unknown-unknown -- -D warnings` both clean (this host builds
`three_rings` directly — no container exclusion needed here). `cargo test -p
app --features hosted`: 360 passed, 0 failed, 5 ignored. e2e, full serial run
of `batch-move.spec.ts` + `card-detail.spec.ts` + `selection-tray.spec.ts` +
`destination-picker.spec.ts` (`--workers=1`, both fixes in place): **38/40
passed.** The 2 residual failures are exactly the base-parity class the task
brief named in advance ("2 known fixture misclassifications in batch-move") —
"a /my row held in one place resolves to that place and moves" and "a /my row
whose copies are all sideboarded moves off the sideboard", both sunk by the
same Inbox-bulk-load contamination described above (their own `unownedCards`
candidates are no longer genuinely unowned either) — reproduced on base
independent of this task's diff, since both assume the same single-place
resolution the fixture can no longer promise. Every other test in all four
files passed, including the two new ones and the pre-existing bench test that
already asserted the *failed*-read arm never regresses to the retired empty
string (`selection-tray.spec.ts`'s `"Move to…" opens the destination picker`).


### Table overflow re-measured and fixed (P6-001, 2026-08-13)

The two overflows P6-098's review found and filed in July (line 1439 above) — re-measured
first, per this task's own instructions, because the loop had touched table columns
repeatedly since. Live server (this worktree, `.env` × 3 from the main checkout), Playwright
at 320/375/390/768/800 on `/my/collections/:id` (Depth Box, Commander Deck, Bulk Box — the
three the mobile no-scroll test already names) and `/my/all`, measuring
`document.scrollingElement` and the `TableWrapper`'s own `scrollWidth − clientWidth` (the
wrapper is `overflow-auto`, so a too-wide table is a wrapper-local scroll the document check
alone misses — the same trap recorded at line 3349).

**Both reproduced, magnitudes essentially unchanged from July:**

| width | collection (worst of 3) | all-cards |
|---|---|---|
| 320 | 64px (Bulk Box) | 40px |
| 375 | 9px (Bulk Box) | 0px |
| 390 | 0px | 0px |
| 768 | 30px (Bulk Box) | 6px |
| 800 | 0px | 0px |

Depth Box and Commander Deck overflowed less at 320/768 (5–20px, 0–10px) — the drivers below
are content-sized, so the worst case varies by collection.

**768px driver: the Type column returning at `md` has no room, and the amount depends on
content, not a fixed gap.** Three things land at the same breakpoint: the select column's
`w-11→w-8` shrink (−12px), HERE/WANTED/OWNED's `px-1→px-2` bump (+24px across three cells),
and Type itself (0–30px, driven by the longest unbroken word in the row's longest type line —
table cells wrap by default, so it is the longest *word*, not the longest line). Net, the
freed 12px does not cover the other two.

Tried and reverted: capping the Type cell with `max-w-[7rem] truncate` plus a `title`
attribute (the pattern used elsewhere — catalog.rs's card name, for one). This made every
width *worse* (Depth Box's 0px at 768 became 33px; 800px picked up 6–8px it never had) — under
`table-layout: auto`, a `max-width` on a table cell is not purely a cap. Browsers appear to
use it as the cell's preferred-width contribution when computing the column's auto width, so a
short type line that used to yield a narrow column was now forced to the full 7rem regardless
of content. Reverted rather than chased further.

**Fix: Type's breakpoint moved from `md` to `lg`** (`collection.rs`'s `CollectionTable` header
and data cell, the matching folder-row cell, and `all_cards.rs`'s `CardsTable` header and data
cell — four sites, all `hidden md:table-cell` → `hidden lg:table-cell`). Mana stays at `sm`;
only Type moves. At `lg` (1024px) there is enough room regardless of content — measured 0px on
every seeded collection, where `md` was not (see the table above).

**320/375px driver: the HERE count-stepper's ± buttons, and the WANTED/OWNED header words —
both fixed costs independent of any card's data.**
`CountStepper`'s reveal buttons (`app/src/components/ui/count_stepper.rs`) were
`opacity-0` until hover/focus — invisible, but `opacity` does not remove a box from layout, so
the collection table's HERE cell was paying for two 24px buttons on every row whether or not
they could ever be revealed. Below `sm` there is no hover at all (touch), so they were dead
width, not a dead-but-recoverable affordance: `REVEAL` gained `hidden sm:inline-flex`, leaving
them out of layout entirely below `sm`. Keyboard ± is unaffected (`on_keydown` reads focus on
the stepper's container div, not button visibility) and tap-to-edit is unaffected (`enter_edit`
is the number itself, not a button) — no functionality lost, only an unusable-below-`sm`
control's footprint. This alone took the collection page from 57–64px to 0–20px at 320px.

The remaining gap, all on `/my/all` (which has no stepper — its HERE-equivalent column is
`LocationSummary`, plain text) and the collection page's residual 5–20px: a `TableHead`'s own
word sets its column's min-width under `table-layout: auto`, same mechanism as Type above, and
"Wanted"/"Owned" were doing that on every row regardless of content. Abbreviated to "Want"/"Own"
below `sm`, full words back at `sm`+ (both `collection.rs` and `all_cards.rs`) — not data loss,
a header label, and the abbreviation reads fine at a glance. Diagnosed column-by-column
(`getBoundingClientRect` per `<th>`/`<td>`) rather than guessed: at 320px pre-fix, all-cards'
"Wanted" header alone was 58.5px wide (padding + the word "Wanted") against a 286px content
budget, `text: "—"` in the cell contributing nothing. The abbreviation closed the collection
page to 0px on its own; all-cards needed one more notch — its Card cell's padding
(`p-2` → `px-1 py-2 sm:p-2`), the last 7px of the original 40px.

**Post-fix: 0px at every measured width (320/375/390/768/800) on every seeded collection, both
pages.** Spot-checked at the breakpoint transitions too (430/639/640/700/767/1024/1440) — 0px
throughout. Screenshots before/after at 320 and 768 (collection view, Depth Box) confirm
visually: before-320 crops the OWNED column off the right edge; after-320 fits HERE/WANT/OWN;
before-768 and after-768 differ only in Type's presence, no overflow in either (Depth Box was
already 0px at 768 pre-fix — Bulk Box is the width that shows the mechanism, at 30–40px before
and 0px after).

**Evidence.** `cargo test -p app --features hosted`: 360 passed, 0 failed. Full serial run
(`--workers=1`) of `collection-view.spec.ts` + `all-cards.spec.ts` + `responsive.spec.ts`:
50 passed / 2 failed, both pre-existing and unrelated to this diff — `all-cards.spec.ts:270`
is the documented fixture-pool baseline failure (e2e-suite skill), and
`responsive.spec.ts:253` ("a real toast occupies its container's bottom edge") fails only
because it drives `/dev/components`, and the shared dev server this task measured against was
started without `--features component-bench` (confirmed: the route 404s regardless of this
diff). Neither touches table layout or count-stepper code.

**Open question, not blocking:** the Type-column `max-w`+`truncate`-forces-preferred-width
behavior under `table-layout: auto` is worth knowing the next time a table cell needs
truncation in this codebase — the working pattern elsewhere (catalog.rs) is inside a *flex*
container (`min-w-0` + `truncate`), not a table cell directly. A table cell that truly needs
truncation (not just a later breakpoint) would need either `table-layout: fixed` with explicit
column widths, or the `width: 1px; min-width: 100%` block-in-cell trick — neither attempted
here since the breakpoint move and header abbreviation closed both overflows without it.


### `/login` + `/signup` match the "Desktop — Sign in" frame (P6-008, 2026-08-13)

`specs/phase-6-probes/batch-I-responsive.md`'s P6-008 entry confirmed
`app/src/auth_pages.rs`'s cards had drifted from `design/wireframes.pen`'s
"Desktop — Sign in" frame (the pencil MCP is down; read the frame straight out
of the `.pen` file's JSON instead — it is plain JSON, `grep -n '"name": "Desktop'`
finds it at line 2830): no brand line, the heading read "Sign in" instead of
the frame's "Sign in to your collection", every field was a bare `placeholder`
with no `<label>` anywhere in the file, and the sign-up line read "No account?
Sign up" instead of the frame's "New here?" / "Create account".

**What the frame actually specifies (read from the `.pen` JSON, not prose).**
The "Auth Card" is `icon(circle-dashed) + "Three Rings"` (the "Auth Logo"
group) above a muted, normal-weight `"Sign in to your collection"` heading,
then an **Email Group** and **Password Group** each pairing a small
medium-weight **Label** (`"Email"` / `"Password"`) with its own input frame —
label and input both drawn on screen together, not a floating-placeholder
pattern where the label text only ever appears inside the input. Below the
card, `"New here?"` + `"Create account"` (link). No frame for `/signup`
exists in the file (`grep '"name": "Desktop'` finds only the sign-in frame),
so the signup card got the structural half of the same treatment — brand
line, visible labels — without inventing copy the design never specified.

**Applied, `app/src/auth_pages.rs` + `app/src/shell.rs`:**
- **Brand line.** The shell header's own wordmark (`<a href="/"
  class="text-sm font-semibold tracking-tight">"Three Rings"</a>`) was
  factored out of `AppShell` into `pub fn Wordmark()` (`shell.rs`) and reused
  on both cards — reusing the app's one existing brand element rather than
  inventing a second, and giving the shell itself the same component instead
  of duplicating markup. No `circle-dashed` icon: this app already substitutes
  emoji glyphs for lucide icons where a wireframe specifies one
  (`app/src/my/root.rs`'s `ICON_*` constants) and has no icon set wired up;
  adding one for a single decorative mark was out of scope for an S task, so
  the wordmark ships text-only, same as the shell.
- **Heading.** `/login`: `"Sign in"` → `"Sign in to your collection"`,
  verbatim from the frame. `/signup`: left as `"Create account"` — no frame
  authority for signup copy, and it already matches the sign-in frame's own
  "Create account" link text.
- **Labels.** Real, visible `<label for=… >` elements above each input
  (`LABEL` class: `block text-sm font-medium text-muted-foreground`, sized to
  the frame's small/medium-weight/muted look), each paired with a matching
  `id` on its input — the frame draws the label and a filled input
  side by side, not a floating-placeholder pattern, so this is the "real
  visible `<label>`" case, not the "keep placeholders, just wire ARIA" one.
  Placeholders were kept only where they now carry non-redundant content: an
  example value (`you@example.com`) or a genuine hint (`8+ characters` for the
  signup password, no longer prefixed with the now-redundant word
  "Password"). The bare `"Password"` placeholder on `/login` was dropped
  outright — the label already says it and there's no hint left to add.
- **Sign-up line.** `/login`: `"No account? "` + `"Sign up"` →
  `"New here? "` + `"Create account"`, verbatim from the frame (the link
  still points at `/signup`). `/signup`'s own `"Already have an account? Sign
  in"` line was left alone — no frame governs it and nothing flagged it as a
  deviation.

**Deliberately out of scope (still deviations, not touched here — this task's
brief named only the four items above).** `BackHome`'s `"← Back to home"`
text vs. the frame's `"Browse the catalog without an account →"` (shared by
`/login`, `/signup`, the reset and OTP cards — touching it has a wider blast
radius than this S task's brief covered) and the desktop sidebar rail's `w-60`
(240px, `shell.rs`) vs. the frame's drawn 280px both remain as filed in the
P6-008 probe entry; neither was in this task's named list of elements to
align.

**e2e selector risk, checked before touching markup.** `end2end/tests/
auth.setup.ts`, `smoke.spec.ts`, `command-palette.spec.ts`, and
`seed-e2e-user.mjs` all drive the forms via `input[name=email]` /
`input[name=password]` / `input[name=name]` — untouched by this change.
`getByRole("heading", { name: "Sign in" })` (three call sites in
`smoke.spec.ts` / `catalog.spec.ts`) matches by case-insensitive substring by
default, so it still matches the new `"Sign in to your collection"` heading
without edits.

**Verified.** Playwright screenshots of `/login` and `/signup`, before
(`git stash`, forced a rebuild, confirmed the served HTML reverted to the
plain-placeholder markup) and after — brand line, heading, labels, and the
sign-up line all visibly changed as described. `cargo test -p app --features
hosted`: 360 passed, 0 failed. `npx playwright test --project=chromium
--workers=1 tests/session-fallback.spec.ts tests/smoke.spec.ts`: 11 passed —
including `auth.setup.ts`'s fixture sign-in (the critical regression: it
fills the real form and waits for the `/my` redirect) and `smoke.spec.ts`'s
"login honors next after sign-in", which drives the real login form through
the new label markup end to end.


### Two DFC swap gaps: keywords and preview flip persistence (P6-164, 2026-08-13)

Two small, unrelated defects in `/cards/:id`'s flip control, both pre-existing
since the original DFC back-face flip task ("DFC back-face flip", above).

**A — the keywords row didn't swap with the rest of the face-dependent block.**
`CardDetail::keywords` is card-level: Scryfall's top-level `keywords` array is
already the union of both faces' ability words, and there is no per-face
equivalent anywhere on the wire to swap in instead — confirmed at the source,
not assumed. `app/src/ingest/extract.rs`'s `ORACLE_FACE_KEYS` (the allowlist
that decides what goes into `cards.card_faces`) deliberately excludes
`keywords`, and Scryfall's own Card Face object has no `keywords` key to begin
with (only the top-level Card object does). So a flipped-to back face was
showing the *front* face's keywords beside the *back* face's oracle text —
data that was never wrong on the wire, just paired with the wrong face on
screen. The honest-minimal fix (data doesn't exist per-face, so don't invent
it): gate the row on `!flippable || face.get() == 0` — it shows only
alongside the front face and disappears entirely once flipped, rather than
being relabeled "card-level" (considered; rejected as more UI for a single-S
fix to carry, and "gone" is the same information as "not this face's" for a
reader who can flip back to see it). `data-testid="card-keywords"` added —
the row had none before, which was awkward for tests that need to assert an
absence.

**B — a preview's flip state survived closing and reopening it.**
`PreviewBody`'s own doc comment already said "each starting at the front";
the *first* open did, but the code let the DFC flip ride along on every
subsequent reopen. Root cause is the lazy-mount latch just above it
(`CardPreview`'s `hovered`/`sheet_seen`, "Both bodies mount lazily" doc
comment): `PreviewBody` mounts once and is never torn down, so its `face`
`RwSignal` is one long-lived piece of state, not something recreated per
open — matching "each starting at the front" needed an explicit reset on
reopen, not the accidental byproduct of a fresh component instance.
**Decision: reset-on-reopen**, not persistence — no comment, spec passage, or
commit message anywhere in this codebase gives persistence a rationale, and
the module doc already stated the opposite intent, so this closes the gap the
doc always claimed rather than opening a new design question.

The mount latches themselves are unmount-averse on purpose (unmounting empties
the sheet mid-slide, or thrashes the hover card every `mouseleave` — both
still true, both left alone). So the fix doesn't touch them: `PreviewBody`
gained an `open: Signal<bool>` prop — the affordance's *real* visible state,
distinct from the latch — and an `Effect` that resets `face` to `0` every time
`open` flips true. The sheet already had a live signal for this
(`sheet_open`, an `RwSignal<bool>` the `Sheet` component itself writes to on
close). The hover card did not: its `open` signal lives entirely inside
`HoverCard`'s own context, invisible to callers. Added `on_open_change:
Option<Callback<bool>>` to `HoverCard` (mirrors `open` out via an `Effect`,
defaults to `None`/no-op) rather than exposing the signal itself or
restructuring the context — the smallest change that gives a caller a way to
observe a genuine open/close transition without owning the popover state.

**Both gaps are UI-only; no wire or DB change.** `CardDetail`/`CardSummary`
are unchanged — (A) reads fields the client already had, and (B) is
client-side signal wiring. `shared`'s tests were not re-run for this reason
(no source in that crate changed).

**e2e.** Three new tests in `card-detail.spec.ts`, all kill-verified by
stashing `app/src/cards.rs` + `app/src/components/ui/hover_card.rs` back to
base and confirming each new test fails for the reason it claims, then
restoring: (A) `"Kruin Outlaw"` (a transform DFC whose union keywords are
non-empty — `DFC_QUERY`'s "Agadeem's Awakening" has none, so it can't
distinguish the fix from "never renders") shows `card-keywords` on the front
face, hides it entirely after a flip, and shows it again flipped back to
front. Base fails on the first assertion (`card-keywords` doesn't exist at
all pre-fix — it had no `data-testid`), which proves the row changed but not
specifically that the *gating* is what the test pins; a second, narrower
mutation isolated that — testid kept, `is_front` hardcoded `true` — and the
*same test* then fails exactly on the post-flip `toHaveCount(0)` assertion
instead, which is the one that actually exercises the fix. (B), two tests, hover and sheet: flip
to the back, close (mouse-away past the 150 ms hover-intent timer; `Escape`
for the sheet), reopen the same trigger, assert the front face again. Base
fails both — the reopened body still shows the back face's name.

**Evidence.** `cargo fmt --all -- --check` clean. `cargo clippy --workspace
--exclude frontend --all-targets`, `-p frontend --target
wasm32-unknown-unknown`, `-p app --features native --all-targets`, `-p app
--features hosted,component-bench --all-targets`, and `-p app --features
hydrate,component-bench --target wasm32-unknown-unknown` all clean, `-D
warnings` (run on this host, which builds `three_rings` directly — needed
`mkdir -p target/site/pkg` first, the same Tauri build-script requirement the
verify section already names). `cargo test -p app --features hosted`: 360
passed, 0 failed, 5 ignored — unchanged from P6-152's count, since neither fix
added Rust-level logic (both are component wiring, covered by e2e instead).
e2e: `card-detail.spec.ts` full chromium 20/20 (includes the 3 new tests, all
kill-verified per above); `responsive.spec.ts` full chromium 34/34 — the one
DFC/preview-adjacent file besides `card-detail.spec.ts` itself (only these two
reference `card-preview`/`card-flip`/`card-keywords` test ids). One `--
workers=1` run of both files together hit a single failure on an unrelated
toast-positioning test (`#sonner` never appeared); traced to the dev server
having been started without `--features component-bench` (that test drives
`/dev/components`) rather than to this task's diff — restarting the server
with the flag made it 35/35 with no other change. No base-parity failures.



**Closed, P6-020 (2026-08-13): the data-dependence caveat above.** Everything measured just
above used the seeded fixture's own names, so the P6-001 fix was only proven 0px-everywhere for
*those* names. Any card-holder-chosen collection name (or, less likely given the catalog's
naming, a card name) longer or less breakable than what the seed happens to carry can still
reopen the same overflow — `/my/all`'s WHERE cell renders `"{n} in {collection_name}"` with a
user-chosen name, and `collection.rs`'s folder rows render a user-chosen name directly, neither
bounded by anything the app controls.

**Root cause, confirmed by measurement:** the `all_cards.rs` WHERE cell rendered plain text —
no `truncate` at all — so under `table-layout: auto` a long name simply set the column's
min-content width to the name's full rendered width (as P6-001 already knew: table cells wrap by
default, so a column's floor is its longest unbreakable *token*, and a user-chosen name has no
bound on that token's length the way a catalog word does). Adding `truncate` alone would not
have fixed it either, and this task confirms *why*, closing the open question above with a real
measurement rather than a guess: `truncate` sets `white-space: nowrap`, which removes every wrap
opportunity from that text — its min-content width becomes equal to its max-content width (the
whole string), so a nowrap span *anywhere* inside a table cell forces the column to fit the full
text regardless of any `overflow`/`text-overflow` also set. This is a different, stronger trap
than the "forced to `max-w`'s exact value" one P6-001 measured on the Type column — that one
under-grew short content up to a fixed cap; this one over-grows arbitrarily with content length,
because nowrap text has no line-break opportunity for the intrinsic-sizing pass to use.

**Fix: the `max-width: 0; width: 100%` block-in-cell trick, applied to the `<TableCell>` (`<td>`)
itself,** not a nested child — the documented, cross-browser pattern for "one column takes the
table's leftover width and clips its own content" under auto layout. `max-w-0` caps this cell's
own contribution to the column's min/max-content computation at zero regardless of what nowrap
text lives inside it (unlike the Type column's `max-w-[7rem]`, zero cannot force short content
wider — it can only fail to *widen* the column, never inflate it); `w-full` is the auto-layout
hint that the column should still claim the table's remaining width once every other column has
what it needs. The nested content then does the actual visual truncation once the column has a
real, data-independent width: `all_cards.rs`'s WHERE cell and `collection.rs`'s folder-name cell
both got `max-w-0 w-full` on the `<TableCell>`, and `truncate` (`overflow-hidden`,
`text-overflow: ellipsis`, `white-space: nowrap`) plus a `title` attribute carrying the untruncated
text on the element that actually renders the name. This is the "worth knowing" case the P6-001
open question named — now attempted, measured working, and folded into a real fix rather than an
open question.

**Cells hardened:**
- `all_cards.rs`'s WHERE cell (`LocationSummary`): the single-location `"{n} in {name}"` line is
  rendered and truncated as *one* string rather than splitting the count from the name — an
  end-ellipsis on the whole string can only ever cut into the tail (the name), never the count,
  which is always first, so there was no need for a separate fixed-width prefix span. The
  multi-location dropdown's per-collection `"{n} · {name}"` list items got the same `truncate` +
  `title` treatment for visual correctness once expanded (the TD-level `max-w-0 w-full` already
  contains the *layout* risk regardless of what is nested inside, but an untruncated line inside
  a narrow column would still visually clip mid-character without the ellipsis).
- `collection.rs`'s `FolderTableRow` name cell: icon and name were plain text in one `<a>`
  (`flex items-center gap-2`); the icon got `shrink-0` and the name moved into its own
  `min-w-0 truncate` span with a `title`. The `max-w-0 w-full` on the TD is what stops the
  column from growing past the width card rows in the same column already establish (folder rows
  share the "Card" column with `CardTableRow`, which was left alone — see below); `min-w-0` on
  the flex item is what lets *that* span actually shrink and ellipsize at real layout time once
  the column has its real width, the ordinary flex-truncation idiom used everywhere else in this
  codebase (`tree.rs`, `root.rs`, `move_selection.rs`).
- **Card-name cells left alone, on purpose:** `all_cards.rs`'s `CardsRow` and `collection.rs`'s
  `CardTableRow` render the card name plain, no truncation. Checked, not fixed: a card name comes
  from the Scryfall catalog, not the account holder, and every card name in this catalog contains
  at least one space (`"Fire // Ice"`, double-faced names, everything) — the existing
  wrap-by-default behavior already bounds that column's floor to the longest *word*, the same
  mechanism P6-001 used deliberately for the Type column. "Card-name cells: check whether they
  already truncate" (this task's brief) is answered here: they don't, and the invariant this task
  closes — *no viewport-width dependence on **user** data length* — does not reach them, because
  a card name is not user-chosen data. If the catalog data source ever changes to allow spaceless
  card names, this reasoning needs revisiting; not expected, not seen.

**Evidence.** `cargo test -p app --features hosted`: 360 passed, 0 failed (same count as
P6-001 — no Rust unit tests changed). One new, kill-verified e2e test
(`all-cards.spec.ts`, "mobile — a long collection name does not widen the table"): creates a
`zz-e2e`-prefixed scratch binder whose name is a single ~65-char token with no spaces or hyphens
(nothing for default line-breaking to grab), holds one card in it via `/api/collections/:id/have`,
loads `/my/all?q=…` at 390×844, and asserts the `TableWrapper`'s `scrollWidth − clientWidth` is
`≤ 1` (the P6-001 "measure the scroll container, not the document" discipline), the document-level
check too, the WHERE cell's own `title` equals the untruncated `"1 in {name}"`, and the WHERE
cell's own `scrollWidth − clientWidth` is `> 0` (the "base: overflow > 0" half — proof the
truncation CSS is actually clipping this text, not merely that a short name happened to fit).
Kill-verified by reverting the two Rust files' hunks (`git stash`) and rerunning against the
pre-fix code: the wrapper-overflow assertion failed at **334px** of overflow (not the title
assertion, which would have failed trivially since `title` is new either way — the wrapper check
was moved ahead of it specifically so the kill-run exercises the real layout regression); restoring
the fix and rebuilding turned it green again. Full serial run (`--workers=1`) of
`all-cards.spec.ts` + `collection-view.spec.ts`: 37 passed / 1 failed — `all-cards.spec.ts:270`
is the documented fixture-pool baseline failure (e2e-suite skill), unrelated to table layout,
present before this task's changes too.

### Bare `/my` ships the list, not the hidden table (2026-08-13, P6-166)

`app/src/my/all_cards.rs` (`AllCardsPage`, new `AllCardsPending` /
`AllCardsHeading`), new `app/src/components/viewport.rs`, and
`app/src/components/palette.rs` (`desktop_signal` is now one line over the
shared helper). Size M. This amends the "Mobile `/my` root" section above,
which described `/my` as emitting both markups at every width.

**The defect.** `/my` is `SsrMode::Async` and rendered *both* the drill-down
root list (`md:hidden`) and the All-cards table (`hidden md:flex`), letting CSS
pick. A phone therefore waited on the aggregate `all_cards` read it would
display none of, and then downloaded the whole table — including Leptos's
serialized copy of the resource, which is the bulk of it. Measured against the
dev seed, signed in, on the watch server:

| | bytes | `all-cards-row` `<tr>`s | SSR (warm, ×3) |
|---|---|---|---|
| `/my` before | 576,473 | 50 | 862 / 814 / 821 ms |
| `/my` after | 143,580 | **0** | 492 / 475 / 467 ms |
| `/my/all` (untouched) | 548,047 → 548,154 | 50 | ~830 / 900 ms |
| `/my/shopping` (shell baseline) | 125,940 | — | — |

−432,893 B (−75.1%) and −~345 ms of blocking. The shell baseline is the number
that says what is left: `/my` now sits ~18 KB over a plain shell page, where it
used to sit ~450 KB over one. Two caveats on reading the absolute figures, both
of which leave the deltas intact: these are watch-server responses (`cargo
leptos watch` injects hot-reload comment markers, so a release build is smaller
on every row), and the e2e suite writes to this same user, so all four rows were
captured back-to-back on one fixture state — re-measuring after a suite run
moved `/my/all` by ~54 KB while `/my` did not move at all, which is itself the
finding: `/my` no longer carries the aggregate payload that changes. The
`batch-move` parity check above happens to supply a **second, independent**
before/after pair, taken hours later on a fixture the suite had grown:
597,488 B / 50 rows reverted → 164,488 B / **0** rows restored, −72.5%.

**Chosen: the server ships one markup that is correct at every width, and the
table's subtree mounts client-side.** `/my` SSRs the root list plus this page's
own heading and row skeleton — constant size — and `AllCardsBody` is mounted
behind a `Show` gated on `media_signal("(min-width: 768px)")`. That signal is
`false` during SSR *and* during the hydration render and is corrected in an
`Effect` afterwards, so **no width is resolved on the server** and the markup
the server sends is still the markup that hydrates. CSS remains the display
authority: the mounted body keeps its `hidden md:flex` and the query names the
same 768 px line, so the gate decides only whether the subtree *exists*.

The mechanism is not new here — the ⌘K palette has used exactly it since
2026-07-26 (`desktop_signal`, gated on width *and* `pointer: fine`). It is now
factored into `components/viewport.rs` with one implementation and two callers,
so the two surfaces cannot drift on when a width is allowed to be known. The
listener half matters and is kept: a tablet rotated into landscape crosses the
line, and a sampled-once read would leave it on the skeleton forever.

**What it costs, stated plainly.** A **full document load** of `/my` at desktop
width no longer arrives with rows in the HTML: it paints the heading and the row
skeleton, then fills in one round trip after hydration. Every *in-app* arrival is
unaffected — a client-side navigation always mounted and fetched this table, so
the mode switch, the breadcrumbs, the palette and the rail row cost exactly what
they did before. The residual surface is the post-login `/` → `/my` redirect,
a bookmark, and a refresh. The SSR-complete table is `/my/all`, which renders it
at every width; the "the table SSRs every row server-side" contract moved there
with the e2e test that pins it.

**Rejected, with the evidence.**

- **Client-hint cookie** (a script records a viewport class; SSR reads it and
  skips the table for a phone). It is the only option with *zero* desktop cost,
  and it was still turned down: it makes the server's markup a **guess**, which
  is the one thing this page's design has refused since the list shipped. A
  stale hint (widen the window, then hard-load `/my`) SSRs a list that is
  `md:hidden` — a blank content column — so it needs a client-side recovery
  path, which is the deferred mount anyway. That makes it the chosen mechanism
  *plus* a cookie, a `Vary`-shaped caching hazard on an authed landing page, and
  a resize listener writing `document.cookie`. More machinery for a first-paint
  optimisation on one route.
- **Two documents — `/my` the list at every width, `/my/all` the table —
  retiring the CSS switch.** Attractive on paper (and the shape P6-154's rail
  retarget points at), and rejected on what desktop `/my` would then be:
  `root_rows` is a *strict subset* of the sidebar rail, projected off the same
  `AssembledTree` (that is its stated design goal). Desktop `/my` would be a
  240 px rail beside a content column repeating it — and `/` → `/my` after
  login is the desktop landing, so this is not a corner. There is no designed
  desktop hub in `design/wireframes.pen`; the frame that produced the list is
  *Mobile — My cards root*. Retargeting desktop's entry points at `/my/all`
  does not save it either: `/` → `/my` cannot know the width, so `/my` has to
  remain good at both.
- **Streaming the table island instead of blocking on it** (`PartiallyBlocked`
  / out-of-order for the route). Fixes the blocking half only: under
  out-of-order the rows still ship, as a `<template>` plus a hoisting script.
  The task's requirement is neither block *nor* ship.

**Not done, and why — two links of P6-154's family, filed rather than
absorbed.** The command palette's `All cards` place and the collection view's
`All cards` breadcrumb both point at `/my`, which is the table only at `md`+;
on a phone they land on the drill-down list. That is the same defect P6-154
fixed on the rail's pinned row, on two more shared, width-agnostic links, and
this change neither creates nor worsens it (both are client-side navigations,
so neither pays the payload this task is about). It wants P6-154's ruling
applied, not a payload fix.

**e2e, updated deliberately.** Nothing here was retargeted to make a test go
green; each move is a contract that changed route.

- **New, and the assertion this task exists for:** `all-cards.spec.ts` →
  "bare /my ships the list, not the hidden table". Request-level, not
  page-level, because a `display: none` subtree is still bytes on the wire and
  only the raw response can tell "hidden" from "not sent". Counts
  `all-cards-row` occurrences (0, against a base of 50) rather than
  `not.toContain`, so a failure says how many leaked. Two positive controls:
  the root list is present in the same document, and `/my/all` in the same run
  does carry rows — so the zero is about `/my` and not about an empty account.
  **Kill-verified**: reverting `AllCardsPage` to the pre-fix shape alone and
  rebuilding fails it (`bare /my shipped all-cards rows it never displays:
  expected 0, received 50`) and passes again restored.
- `all-cards.spec.ts` → "the table SSRs every row server-side" now requests
  `/my/all`. The contract belongs to whichever route renders the table at every
  width, and that is `/my/all`; it is `SsrMode::Async` for exactly the reason
  the test states and mounts the identical `AllCardsBody`. Same for the raw-HTML
  halves of "quick search filters by name and rides the URL" and "?cursor= is
  honored on a cold load" — both keep their page halves on `/my`, which is where
  a desktop reader actually lands with those URLs.
- `states.spec.ts` → the "works with no JS at all" half of the stale-cursor test
  moved to `/my/all` for the same reason: a shared `?cursor=` link that must
  survive a dead JS bundle is `/my/all`'s to carry. Its page half stays on `/my`.
- `all-cards.spec.ts` gained `settled(page)` — `hydrated()` now proves only that
  the *document* took over, and `/my` can be hydrated with a skeleton where the
  table will go. Retrying locators absorbed that on their own; the one-shot
  `$$eval` in `renderedCells` did not, and read an empty page *deterministically*
  (so `toPass` retried to the same answer). `settled` waits for whichever of
  table / empty / error the body resolves to — all three, so it waits for
  settled and never for correct.
- `my-root.spec.ts`, mobile: the table's absence at 390 px is now
  `toHaveCount(0)`, where it was `toHaveCount(1)` + `toBeHidden()`. That is a
  strictly stronger assertion and it is the point of the change; the same swap
  in "a failed tree read still leaves a way out of My cards" strengthens that
  test's state control. The same test also now asserts **zero `all_cards`
  requests** at phone width — the client half, and not redundant with the
  request-level one: deferring the table to hydration would be a poor trade if
  the deferred mount then ran at every width, since the read is the expensive
  part. Its positive control is the desktop test's mirror-image assertion that
  the very same request *was* made.
- `my-root.spec.ts`, desktop: "/my is still the All-cards table" now also
  asserts real rows, no empty state, **and a non-zero `all_cards` request
  count**. This is new risk the change introduces, not padding: the table's
  resource is now created *after* hydration on a document load, and
  `initial_value()` reads `__RESOLVED_RESOURCES[<next id>]` whatever the
  hydration flag says (see `AllCardsPayload` above) — so the wrong-payload class
  of bug gains a document-load instance, where before it had only the
  client-side-navigation one. `AllCardsPayload`'s named field is what closes it;
  this asserts it stays closed.
- `responsive.spec.ts` needed no change. Its two `/my` click-path tests reach
  the page by client-side navigation, which is the path this change does not
  touch — worth stating, since "the click-path tests still pass" is otherwise
  easy to read as coverage of the thing that moved.

**Verification, chromium, `--workers=1`.**
`responsive.spec.ts` + `my-root.spec.ts` + `all-cards.spec.ts` **41/42**;
`my-root.spec.ts` + `states.spec.ts` + `session-fallback.spec.ts` +
`collection-tree.spec.ts` **32/32** after the phone no-fetch assertion landed.
Two failures triaged against base, both **pre-existing**:

- `all-cards.spec.ts` → "the location summary expands to the collections it
  names" — the dev seed gap P6-154 already recorded. It fails on the **API
  payload** (`dev seed should hold at least one card in two collections`) before
  the page is touched, and `GET /api/all_cards` confirms it directly: 50 cards,
  **0** with more than one location. Nothing in this change can reach it.
- `batch-move.spec.ts` → "a /my row held in one place resolves to that place and
  moves" and "a /my row whose copies are all sideboarded moves off the
  sideboard" — no `Moved 1 card` toast after a move committed from `/my`. These
  *are* `/my` tests, so they were parity-checked properly rather than argued
  about: `AllCardsPage` was reverted to its pre-fix shape, the watch server
  rebuilt and confirmed serving the old markup (`/my` back to 50 rows), and the
  same two tests fail with the identical signature and the same sibling
  (`an ambiguous /my row asks which copies, and moves exactly the stack picked`)
  still passing. Restored after. Not this task's, and filed rather than absorbed.

**The full chromium tier was not run to completion, deliberately and this is a
gap.** At `--workers=1` against the remote Neon dev branch this box measured
~1.5 min/test across 305 tests — five hours, most of it in families
(`batch-move`, `collection-tree-manage`) with no causal path to a change that
only moves *when* one subtree mounts. The specs that touch `/my`'s table or
request it raw were run instead, which is where the change can reach; the
untouched remainder is the honest caveat, and CI runs the gate regardless.

Gate: `cargo fmt --all --check` clean; all five clippy lines clean
(`--workspace --exclude frontend --exclude three_rings --all-targets`,
`-p frontend --target wasm32-unknown-unknown`, `-p app --features native`,
`-p app --features hosted,component-bench`, `-p app --features
hydrate,component-bench --target wasm32-unknown-unknown`); `cargo test -p app
--features hosted` **360 passed**.

**Resilience trade, stated (P6-166 review):** desktop `/my`'s main content now
requires wasm + `matchMedia` to mount — a hydration failure that previously left
a readable SSR'd table leaves a permanent skeleton. Accepted for the payload
win; `/my/all` remains the full-SSR table if resilience is ever needed at a
bookmarkable address.

### Hover-preview mouseout flash — leftover class fixed, symptom not reproduced (2026-08-15)

`app/src/components/ui/hover_card.rs` (`HoverCardContent`), `end2end/tests/all-cards.spec.ts`
(new regression test). Bug report (Workbook WB-01M031SABTFM1FXYAF6EG2CEKV): on
`/my`, a card row's mouseout shows "a brief flash of content that appears to be
the card preview rendering inside the row instead of a hovering modal, causing
the row to resize to hold the card image, then... the row resizes again back to
normal."

**The lead.** `#148`'s `PopoverContent` fix (`popover.rs`, same day, merged
14:07) removed a leftover `relative` Tailwind class copied from the upstream
(non-native-popover) registry source: on a `popover`-attribute element promoted
to the top layer, an author-set non-`static` `position` overrides the UA's
`position: fixed`, corrupting `anchor()`-based positioning on a page taller than
the viewport. `hover_card.rs`'s `HoverCardContent` carried the identical
leftover class, found during that fix and left out of scope — the obvious
hypothesis for this bug, filed as a follow-up the same day.

**Confirmed: the class is a real defect here too, but with no reproducible
visual effect.** Verified in real (headed) chromium via direct DOM manipulation
on a genuinely open panel: `getComputedStyle(el).position` reads `"absolute"`
with the class present, `"fixed"` immediately after `el.classList.remove
("relative")` — so the same top-layer-position-override mechanism is active.
But unlike `PopoverContent` (raw `anchor()` calls in `left`/`right`/`top`/
`bottom`), `HoverCardContent` positions itself via the `position-area`
shorthand, and that rendered **pixel-identical** with the class present or
removed — checked scrolled 2249px down a 4026px document and unscrolled, panel
below the anchor and flipped above it. No mispositioning was reproducible from
this class for this component in current chromium.

**The row-height flash itself was not reproduced, despite extensive attempts.**
Eight separate Playwright scripts (real headed chromium, not just headless):
single hover cycles and 30+ varied-dwell hover/unhover cycles (including
sub-150ms flickers that interrupt the open-intent timer) across single and
multiple rows simultaneously; scrolled and unscrolled; a continuous
`requestAnimationFrame` collector sampling the row's `getBoundingClientRect()`
every frame across the whole close transition rather than at one instant (a
one-shot before/after check would miss a one-frame spike). Direct
`getComputedStyle` sampling showed the panel's `display` and `position` change
**simultaneously** on close (`block`/`absolute` → `none`/`relative` in the same
sampled frame, both with and without the leftover class) — i.e. no captured
frame ever had the panel both rendered and in-flow, which is the necessary
condition for the reported row growth. Row height never deviated from baseline
in any run. Console was clean (no errors) across all runs.

**Fixed anyway.** The leftover class is a genuine, confirmed correctness bug
matching an already-accepted sibling fix — worth removing so `HoverCardContent`
does not keep relying on `position-area` happening to compensate for a
spec-incorrect `position` value, a property of this chromium build/version and
not a guarantee. No consumer regressed: catalog list-view hover preview,
`/cards/:id`'s own hover card, and DFC flip-in-preview all reverified by hand
and via the existing `card-detail.spec.ts` suite (30/30 relevant tests green).

**Open question, for whoever picks this back up if the flash persists after
this ships:** the reported mechanism could not be confirmed. Candidates not yet
ruled out — a browser-version-specific Popover-API/CSS-transition interaction
this chromium build's synced `display`/`position` change doesn't exhibit; or
something entirely outside `hover_card.rs`'s own CSS (Leptos DOM-construction
ordering during a row re-render, though the architecture — signals updating in
place, `<Show>` mounted once and latched — gives no obvious trigger for one on
mouseout). Get a screen recording and the exact browser/version/row from the
reporter before spending more automated-repro budget on this; every angle
scripted-Playwright could try has now been tried.

**Verification.** `end2end/tests/all-cards.spec.ts` gained "the hover preview
closes without ever changing the row's height @fast" — hovers a row, then
samples `getBoundingClientRect().height` every animation frame for 700ms after
mouseout (covering the 150ms hover-intent delay plus the ~200ms CSS close
transition), asserting every sample stays within 1px of baseline. Stable across
6 consecutive local runs. `card-detail.spec.ts` + `all-cards.spec.ts` +
`catalog.spec.ts` fast tier: 92/93 (one new test added), the sole failure
("the location summary expands to the collections it names") confirmed
pre-existing via `git stash` against unmodified branch — a documented dev-seed
gap (e2e-suite skill), not a regression. `cargo fmt --all --check` clean;
`cargo clippy -p app --features hydrate,component-bench --target
wasm32-unknown-unknown`, `--features hosted,component-bench --all-targets`, and
`--features native --all-targets` all clean.
