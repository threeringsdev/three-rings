# Information architecture & navigation

**Deliverable of:** Phase 1b task "Information architecture / nav structure" — see [specs/ui-design.md](../specs/ui-design.md)
**Approved:** 2026-07-10, maintainer, section by section. Revisions to the accepted spec draft are listed under [Deviations](#deviations-from-the-spec-draft) and mirrored in the spec's Findings.

## Overview: two modes

The app has exactly two top-level modes, switched at the topmost level of navigation:

| Mode | Universe | You're asking |
|---|---|---|
| **Catalog** | every card that exists (Scryfall data) | "what exists?" |
| **My cards** | every card you own | "what do I have and where is it?" |

There is no generic "Search" destination and no top-level shopping list — search is two context-specific surfaces (below), and the shopping list lives inside My cards.

## My cards mode

The collection tree carries the entire IA, keeping the file-explorer metaphor end to end:

```
┌─ My cards sidebar ────┐
│ 🗂 All cards    (812) │  ← pinned virtual view: everything, Inbox included; landing view
│ ───────────────────── │  ← delimiter: aggregate above, actual collections below
│ 📥 Inbox         (7)  │  ← pinned first in the tree; unsorted-count badge
│ ▾ Binders       (640) │
│   • Trade       (120) │
│   • Bulk        (520) │
│ ▾ Decks         (172) │
│   • Grixis      (100) │
│ ───────────────────── │
│ 🛒 Shopping list (2)  │  ← pinned system row; badge = distinct cards short
└───────────────────────┘
```

- **All cards** is a pinned virtual view at the very top, separated from the tree by a delimiter. It aggregates every collection — Inbox included (Inbox's 7 are part of the 812) — and is the mode's landing view. It is not a tree node: the collections below aren't visually nested under it, only accounted within it.
- **Inbox** is a real collection (undeletable, renamable) pinned first in the tree, above the user's collections; the pin targets its collection route.
- **Shopping list** is a pinned system row below the tree — always one click away in the mode where you act on it, without being a top-level destination.
- Tree behaviors per the spec: nested, collapsible, drag to reparent/reorder, rolled-up present-count badges.
- Tree management (create / rename / delete / move) happens in place via context menus — there is no separate "manage collections" page.
- **Needs views and pick lists are contextual only**: reached from the needs chip on a deck or collection header, never from global nav.

## Catalog mode

The Scryfall-caliber query surface. **Filters live in the mode's sidebar** — the same panel slot the collection tree occupies in My cards — with the query field above the results in the main panel. Filter edits apply immediately: results update live, no explicit submit. Query text and filter state both serialize into the URL (`?q=…` plus filter params) so any search is shareable and restorable.

Catalog has no collection context, so adding needs a target: a **sticky destination picker in the results toolbar** (`Adding to: 📥 Inbox ▾`) that persists across searches and defaults to Inbox. Every result carries the spec's `+ Want` / `+ Have` quick actions against that target. For scattered destinations, the selection tray is the batch alternative — gather results, pick a destination once.

```
┌─ Catalog ─────────────────────────────────────────┐
│ Filters      │ [query: t:instant c:ur cmc<=2    ] │
│ ──────────   │ Adding to: 📥 Inbox ▾ [grid | list]│
│ Set      ▾   │ ┌────┐ ┌────┐ ┌────┐               │
│ Color    ▾   │ │card│ │card│ │card│  each: +Want  │
│ Type     ▾   │ └────┘ └────┘ └────┘  +Have ⋯      │
│ Rarity   ▾   │   …results update live as          │
│              │    filters are edited…             │
└──────────────┴────────────────────────────────────┘
```

## Two search surfaces

| | In-collection quick search | Catalog mode |
|---|---|---|
| Lives | inside every collection view (My cards) | top-level mode |
| UI | persistent type-ahead field | query builder + filters |
| Query shape | card name lookup | arbitrary catalog queries |
| Role | filter this collection's cards; inline-add catalog matches not present | browse/discover; add via sticky destination picker |
| Add default | `+ Have` here (deck context leads `+ Want`, per spec) | `+ Want` / `+ Have` to picked target |
| Flow served | time-to-enter-50-cards; intake | discovery, decklist research |

The in-collection surface is part of the collection view, not a destination — type a name, hit enter, the card lands here. It is the keyboard-first rapid-add path the spec's metric targets.

## Card detail: preview + page

Two tiers, one component:

- **Hover preview (desktop):** hovering a card row or grid tile opens a lightweight overlay — image, key info, quick actions, your copies. Ephemeral; no URL change.
- **Dedicated page:** clicking navigates to `/cards/:id` — full detail (printings, rulings, your copies & locations), deep-linkable and SSR-able. Mode-neutral: reachable from both modes, renders catalog info plus your ownership.
- **Touch mapping (no hover):** tap opens the preview as a bottom sheet; an expand affordance in the sheet goes to the full page.

## Desktop shell

A slim top bar carries the mode switch (`Catalog | My cards`) and the user menu. Both modes share the same layout skeleton — a sidebar rail plus a main panel — and each mode fills the rail with its own content: My cards the collection tree, Catalog the filter panel. The selection tray docks at the bottom of the window, persists across both modes, and every move fires an undo toast.

## Mobile shell

Two tabs, matching the modes: `[📖 Catalog] [🗂 My cards •7]` (badge = Inbox unsorted count).

- **Catalog tab:** query field on top, results beneath; the filter rail collapses into a slide-over sheet, and results update live the same way.
- **My cards tab** is a drill-down: the root screen mirrors the sidebar — All cards pinned at top above a delimiter, then Inbox first among the top-level collections, shopping list pinned at bottom; tapping pushes into a collection, back walks up the tree.
- The selection tray docks above the tab bar and survives tab switches.
- Same feature surface as desktop — navigation collapses, features don't (per spec).

## Route map

One Leptos route table, identical across web and the Tauri WebView.

| Route | View | Access |
|---|---|---|
| `/` | redirect → `/my` (authed) or `/catalog` (anonymous) | public |
| `/catalog` | filter rail + live results; `?q=…` + filter params hold search state | public |
| `/cards/:id` | dedicated card page | public |
| `/my` | All cards (the everything view; My cards landing) | auth |
| `/my/collections/:id` | collection / deck view (Inbox included) | auth |
| `/my/collections/:id/needs` | needs view; pick list opens from here | auth |
| `/my/shopping` | global shopping list | auth |
| `/login`, `/signup` | auth screens, outside the shell | public |

**Auth model:** Catalog and card pages are public — browsable and shareable logged out, with `+ Want` / `+ Have` and the destination picker replaced by a login prompt. Everything under `/my/*` requires a session; hitting it logged out redirects to `/login`. The Catalog shell therefore has logged-in and logged-out variants; My cards has one. Ownership UI (the selection tray, a card page's "your copies & locations") exists only logged in — anonymous views simply omit it.

## Feeds into

- **Wireframes (next Phase 1b task):** screen-level layout for each route above; deck-view internals (type grouping, commander header) are deliberately unspecified here.
- **data-model:** nothing new beyond the spec's ripples; note that Inbox-as-real-collection and the All-cards view need no special tables — All cards is virtual, Inbox is a flagged row.
- **collection-api:** public vs. authed route split implies catalog/card endpoints are anonymous-safe; collection endpoints are session-scoped.

## Deviations from the spec draft

Maintainer-approved 2026-07-10 during this task; recorded in the spec's Findings:

1. The four-item main nav (Search / Collections / All cards / Shopping list) is replaced by the two modes.
2. All cards is a pinned virtual view above the tree (its counts include Inbox), not a nav destination beside Collections.
3. The shopping list is a pinned system row inside My cards, not top-level.
4. "Search" splits into the two surfaces above; neither is a nav destination.
5. Card detail is hover-preview + dedicated page (tap → sheet → expand on touch).
6. Catalog and card pages are public; `/my/*` is auth-gated.
