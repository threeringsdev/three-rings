# App UI — the real pages

**Status:** draft
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
long-press / `x`) affordances. **Deck variant** adds: format + commander(s)
rendered as a card in the header, cards grouped by type with counts, the needs
chip (`6 missing — 4 owned elsewhere · 2 to buy`), Want-led add default (binders
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
