# UI design phase

**Status:** implemented
**Depends on:** —

## Problem

Before building features we need to know what the app looks like and how it flows. Design decisions (navigation structure, card display density, search interaction) shape the data the API must serve.

The app's differentiating feature is **multi-collection physical card tracking**: users organize cards into nested binders and decks and the app tracks where every physical card actually lives. The UX of recording and moving physical cards must feel intuitive and fast, requiring minimal interaction per move — this is the experience to get right.

## Scope

In: information architecture, the collection/counts concept model as it appears in the UI, wireframes for core screens, interaction design for the primary flows (especially card moves), visual direction within Rust/UI + Tailwind constraints. Out: final pixel-perfect design for every state; marketing/branding; buy-link integrations; format legality checking.

## Concept model

### Collections

- Two collection types: **binder** and **deck**, organized in a nested, file-folder-like tree.
- Mixed nesting: any collection can contain both its own cards and child collections. There is no separate folder type — a binder with no cards of its own acts as one.
- One built-in **Inbox** collection: undeletable but user-renamable; the default landing spot for cards recorded without filing.

### Three counts

Per card entry, per collection:

| Count | Set by | Meaning |
|---|---|---|
| **Present** | user | Copies physically in this collection. Collections with children display present = own cards + all descendants (rollup), with the rolled-up portion visually distinguished. |
| **Desired** | user | Copies this collection should contain (a deck's decklist quantity, a binder's target). Rolls up for display like present. |
| **Owned** | computed | Sum of present across every collection in the app. Never user-set; identical everywhere a card appears. |

### Card identity

Present attaches to a specific printing (finish/condition per data-model). Desired defaults to the oracle card — a deck wants 4 Lightning Bolts, any printing — with an optional "specific printing only" pin for collectors. Needs matching happens at oracle level by default.

### Needs

Where desired > present in a collection, the gap splits into two actionable buckets:

- **Owned elsewhere** — copies exist in other collections; a move fixes it. All copies elsewhere are listed with their locations; the user judges what can be pulled.
- **Short** — total desired across all collections exceeds owned; feeds the global shopping list. Shortfall per card = sum of desired across collections − owned, floored at zero.

## Information architecture

Designed and approved in Phase 1b — full detail (shells, route map, auth split) in [design/information-architecture.md](../design/information-architecture.md). This section reflects the approved design; the revisions from the draft are recorded in Findings.

- **Two top-level modes:** **Catalog** (the public card universe — Scryfall-caliber queries, browsing, discovery) and **My cards** (the collection tree, counts, needs, moves). No generic Search destination; no top-level shopping list.
- **My cards:** the tree carries the whole IA — **All cards** (the virtual everything-view, Inbox included in its counts) pinned at the very top above a delimiter, **Inbox** pinned first in the tree below it, the **shopping list** pinned at the bottom as a system row. Desktop: persistent sidebar — nested, collapsible, drag to reparent/reorder, rolled-up present-count badges. Tree management is in-place (context menus); needs views stay contextual, off needs chips.
- **Search is two surfaces:** an in-collection type-ahead (filters the collection + inline-adds catalog matches — the time-to-enter-50-cards path) and Catalog mode with a sticky `Adding to:` destination picker on results.
- **Card detail:** hover opens a preview overlay everywhere; click navigates to a dedicated card page (`/cards/:id`). Touch: tap → bottom sheet → expand to page.
- **Mobile:** two tabs (Catalog / My cards, Inbox badge on the tab); My cards is drill-down; same feature surface, navigation collapses rather than features being cut.
- **Auth split:** Catalog and card pages are public (add actions prompt login); everything under `/my/*` is session-gated.

## Core screens

1. **Catalog search/browse** — the workhorse. Fuzzy name search, filters (set, color, type, rarity), card grid vs. list toggle, card detail view.
2. **Collection view** (one binder or deck) — child collections as folder-style rows on top, own cards below (file-explorer convention). The three counts render as right-aligned numeric columns (here / wanted / owned) under a single column header — no per-row labels; wanted appears only when set (> 0) and different from present, and owned collapses when it equals present, so discrepancies stand out and the common case stays visually quiet. Rolled-up child-collection counts render italic and dimmed. Present is editable in place: stepper on hover/tap, click-to-type, +/− on a focused row.
3. **Deck view** — collection view plus: a header with declared format and commander(s) (commander rendered as a card, not a row), cards grouped by card type with group counts, and a **needs chip** (`6 missing — 4 owned elsewhere · 2 to buy`) that opens the needs view.
4. **All cards** — same row treatment; present is replaced by a location summary (`7 across 3 collections`, expandable), since "here" has no meaning at the root.
5. **Needs view & shopping list** — see Primary flows.
6. **Add-to-collection flow** — speed matters most; keyboard-first, time-to-enter-50-cards is the metric that matters.
7. **Auth screens** — signup/login, minimal.
8. **App shell** — navigation, responsive behavior (desktop window vs. mobile vs. browser), platform conventions where they diverge.

## Primary flows

### Moving cards — selection is the tray

One primitive unifies single moves and batches: a selection that persists across views.

- **Single card (the frequent case):** every row has a move affordance (kebab / swipe on mobile / `m` on a focused row) opening the destination picker directly — no selection ceremony. Picker order: **suggested destinations** first (collections whose desired > present for this card — the app already knows where it's wanted), then recent destinations, then type-ahead over the whole tree. Quantity defaults to what the destination needs, capped at what's present here; adjustable in the same dialog. The common case is two taps: move → destination.
- **Batch:** selecting a card (long-press / checkbox / `x` on a focused row) enters selection mode and docks a **tray bar** at the bottom (`4 cards · Move to… · Clear`) with a thumbnail stack so it's never mystery state. The selection survives navigation — gather cards from several collections, then pick one destination once.
- **Every move is logged and instantly undoable** (toast with Undo). Move history powers teardown.

### Needs-driven moves and pick lists

The per-collection needs view (opened from the needs chip) splits missing cards into the two buckets:

- **Owned elsewhere** rows show where copies live (`2 in Trade Binder`) and a **Pull** button — a pre-filled move, one tap to confirm.
- **Pull all** generates a **pick list**: a checklist grouped by source collection (open Trade Binder, grab these 3; open Bulk Box, grab this 1). Checking off an item records the move. This is the phone-at-the-shelf mode: the app names the physical container to open and what to take from it.
- **Short** rows feed the global **shopping list**: one row per card with shortfall count and which collections want it; exportable as text. Buy-link integration is out of scope for v1.

### Deck teardown

Deck-level **Empty deck…** action: choose one destination for everything, or **Return to previous locations** (uses move history; cards without history fall back to Inbox). Shows a preview grouped by destination before confirming.

### Intake

Rapid-add new cards into Inbox (add-flow speed goals apply), then batch-select outward. Suggested destinations do the routing work, since Inbox cards are usually wanted somewhere.

### Add-to-collection

Adding from search happens in a collection context, with two quick actions per result: **+ Want** (desired++) and **+ Have** (present++, arriving in this collection). Default emphasis flips by context: deck context leads with Want (writing a decklist), binder/Inbox context leads with Have (recording cards in hand). Same rapid keyboard-first flow, one extra bit of intent.

## Process

1. Rough flows and wireframes (low fidelity — paper/excalidraw/pencil-tool level); move flows first
2. Validate the add-to-collection flow (time-to-enter-50-cards) and the single-card move (taps-per-move) against real usage
3. Map wireframes to Rust/UI components; identify gaps needing custom components
4. Higher-fidelity passes only for the catalog, collection, and move/needs screens

## Ripples into other specs

Flagged here, designed there:

- **data-model:** collections table (parent, type, name), per-entry desired + present counts, owned as a computed aggregate, move-history table, desired at oracle-vs-printing granularity. (The current draft models a single flat collection and excludes decks.)
- **collection-api:** move endpoints (single + batch), needs/shopping-list computation, collection-tree CRUD.

## Findings

- 2026-07-10 — **Core-screen wireframes complete** (Phase 1b task 2); deliverable: `design/wireframes.pen` — 9 screens + hover-preview overlay (desktop: collection view, catalog search, add flow, sign-in; mobile: My cards root, collection drill-down, catalog search, filter sheet, card sheet), built on three reusable components (Tree Item, Card Row, Card Tile). Maintainer-approved revisions to this spec as accepted: (1) the inline `3 here · 4 wanted · 7 owned` row format became three right-aligned numeric columns under a single HERE/WANTED/OWNED column header — label words repeated on every row read as noise; the collapse rules (wanted blank when unset, owned blank when equal) and italic rolled-up counts carried over. (2) The catalog filter rail gained card-name and card-text text fields. (3) A header "Add cards" button was cut — nothing in the spec covers it; adding happens only through the two search surfaces. Mobile additions per the IA doc: the filter rail collapses to a bottom sheet whose trigger badge shows the count of rail-matched query terms (the [catalog-search](catalog-search.md) contract made visible), and card detail is a tap bottom sheet with a "Full details →" expand. Card-images question resolved: catalog results and card detail lead with the image; collection rows stay text-only for density, with the hover preview / card sheet supplying the image.
- 2026-07-10 — **Catalog search interaction split out** (during Phase 1b wireframes): the filter rail is a curated everyday subset, not Scryfall's full advanced-search form; the query bar targets Scryfall syntax as fully as practical. The query↔rail sync contract (only recognized terms reflect into the rail; unrecognized terms preserved verbatim) is proposed in [catalog-search](catalog-search.md) (draft) — behavioral contract lives there, wireframes stay structural.
- 2026-07-10 — **Add-to-collection flow prototyped** (Phase 1b task 3); deliverable: storyboard region in design/wireframes.pen (`Proto — Add flow · Desktop / Deck context / Catalog / Mobile` — states M1/S1–S5/M2, D1–D2, C1–C3, Mb1–Mb3), built per [design/add-flow-prototype.md](../design/add-flow-prototype.md). Pencil has no interactive links, so "click-through" is a captioned state walk with explicit input-cost accounting: desktop steady state ≈ 5–7 keystrokes/card, zero pointer (50 cards ≈ 250–350 keystrokes); deck context changes nothing but the default (⏎ = want); catalog adds are 1 click/result after a once-per-session destination pick; mobile ≈ 1 tap + ~5 chars/card. Risks flagged for real-usage validation: disambiguation frequency at 4–6 typed characters, and whether ⇧⏎ set-count stays cheap for playset entry. Build note: the former standalone `Desktop — Add flow (type-ahead)` screen was adjusted into milestone M2 inside the desktop storyboard (per the design doc), so the core-screens set now references the storyboard for that screen.
- 2026-07-10 — **Global ⌘K command palette designed** (Phase 1b follow-up task); deliverables: [design/command-palette.md](../design/command-palette.md) + `⌘K — Command palette` region in design/wireframes.pen (frames P1 at-rest / P2 typed + trigger note). v1 scope per maintainer: places (all collections with parent paths, system rows, mode jumps) + a three-command registry (New binder…, New deck…, Undo last move — Sign out considered and dropped); no card matches (the palette is places-and-commands only, never a third card-search surface); logged-in desktop only (`/` stays the quick-add binding). Empty query = RECENT + COMMANDS with first row pre-selected, so ⌘K ⏎ returns to the last collection. UI rides the vendored `command` core shared with the quick-add panel (per the gap analysis, with its keyboard layer rewritten in Leptos).
- 2026-07-10 — **Component gap analysis complete** (Phase 1b task 4); deliverable: [design/component-gap-analysis.md](../design/component-gap-analysis.md) — 27 primitives cataloged against rust-ui/ui `43e1e32`: 20 direct, 4 composite, 3 gaps (collection tree, in-place count stepper, selection tray — `action_bar` read and ruled out for the tray). SSR code review of dialog/popover/command/hover_card/sheet/sonner: SSR-safe throughout, but adoption is "vendor markup + CSS, rewire behavior" (counter-based ID hydration bug, inline-JS behavior layer, ScrollLock global, `leptos_ui` nightly dep). Maintainer decisions: the global ⌘K command palette is **v1** (the registry's `command` component carries the palette UI and the quick-add panel alike — follow-up design task queued); the runtime SSR spot-check was replaced by a standing component-bench page ([ui-component-bench](ui-component-bench.md), draft).
- 2026-07-10 — **Wireframe hygiene pass** (Phase 1b follow-up task): the four disabled ghost `/`-hint frames (desktop keyboard-hint copies stranded in mobile quick-search bars — Mobile — Collection view plus the three Proto Mobile frames) are deleted from design/wireframes.pen; a problems-only layout scan now reports exactly one issue, the intentional Filter Sheet clip in Mobile — Catalog filter sheet. Notes: the pre-deletion scans no longer flagged the ghosts (the pollution was intermittent), so they were deleted on identification; the disabled Row Icon slot inside the reusable Card Row component was deliberately kept — it is the component's optional-icon slot toggled per instance, not a ghost.
- 2026-07-10 — **IA / nav structure designed and approved** (Phase 1b task 1); deliverable: [design/information-architecture.md](../design/information-architecture.md). Maintainer-approved revisions to this spec as accepted: (1) the four-item main nav (Search / Collections / All cards / Shopping list) became two modes — Catalog and My cards; (2) All cards is a pinned virtual view above the tree, its counts including Inbox, not a sibling nav destination; (3) the shopping list is a pinned system row inside My cards, not top-level; (4) search is two context-specific surfaces (in-collection type-ahead; Catalog query builder with a sticky destination picker), not a destination; (5) card detail is hover-preview plus a dedicated public `/cards/:id` page; (6) Catalog and card pages are public, `/my/*` is auth-gated. The Information architecture section above was rewritten to match.

## Open questions

- *(resolved 07-08-2026)* Theming: dark mode from day one 
- Card images: how prominent? (Drives layout and Scryfall image-loading strategy.) *(resolved during execution — Phase 1b wireframes; see Findings: images lead in catalog/detail, collection rows text-only)*
- Keyboard-driven command palette for power users — v1 or later? *(resolved 2026-07-10 — v1, maintainer decision at gap-analysis design review; see Findings and the Phase 1b palette design task)*
- Pick-list ergonomics: does checking items one-by-one beat a single "confirm all pulled" action at the shelf? (Validate in low-fi testing.) *(resolved during execution — Phase 1b move-flow prototype validation)*
