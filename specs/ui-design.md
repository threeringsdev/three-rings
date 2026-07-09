# UI design phase

**Status:** draft
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

- **Desktop:** persistent left sidebar with the collection tree — nested, collapsible, drag to reparent/reorder, rolled-up present-count badge per collection, Inbox pinned at top with an unsorted-count badge. Main nav: **Search** (catalog), **Collections**, **All cards**, **Shopping list**.
- **All cards:** a virtual root aggregating every collection — the "everything I own" view, searchable and filterable. Needed because the tree can have several top-level collections.
- **Mobile:** same feature surface; navigation collapses (Collections as a root tab with drill-down, Inbox badge on the tab) rather than features being cut.

## Core screens

1. **Catalog search/browse** — the workhorse. Fuzzy name search, filters (set, color, type, rarity), card grid vs. list toggle, card detail view.
2. **Collection view** (one binder or deck) — child collections as folder-style rows on top, own cards below (file-explorer convention). Every card row shows the three counts compactly, present first: `3 here · 4 wanted · 7 owned`; wanted appears only when set (> 0) and different from present, and owned collapses when it equals present, so discrepancies stand out and the common case stays visually quiet. Present is editable in place: stepper on hover/tap, click-to-type, +/− on a focused row.
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

## Open questions

- Theming: dark mode from day one, or light-only v1? (Moved from ui-components — the component system supports either.)
- Card images: how prominent? (Drives layout and Scryfall image-loading strategy.)
- Keyboard-driven command palette for power users — v1 or later?
- Pick-list ergonomics: does checking items one-by-one beat a single "confirm all pulled" action at the shelf? (Validate in low-fi testing.)
