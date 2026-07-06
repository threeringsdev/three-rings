# 008: UI design phase

**Status:** draft
**Depends on:** — (can proceed in parallel with 007)

## Problem

Before building features we need to know what the app looks like and how it flows. Design decisions (navigation structure, card display density, search interaction) shape the data the API must serve.

## Scope

In: information architecture, wireframes for core screens, interaction design for the primary flows, visual direction within Rust/UI + Tailwind constraints. Out: final pixel-perfect design for every state; marketing/branding.

## Core screens to design

1. **Catalog search/browse** — the workhorse. Fuzzy name search, filters (set, color, type, rarity), card grid vs. list toggle, card detail view.
2. **Collection view** — the user's cards: totals, value, filtering, sorting. What summary stats matter on first load?
3. **Add-to-collection flow** — speed matters most here; users enter stacks of cards. Rapid search → quantity/printing/condition → next card, minimal clicks/keystrokes. Consider keyboard-first entry.
4. **Auth screens** — signup/login, minimal.
5. **App shell** — navigation, responsive behavior (desktop window vs. mobile vs. browser), platform conventions where they diverge.

## Process

1. Rough flows and wireframes (low fidelity — paper/excalidraw/pencil-tool level)
2. Validate the add-to-collection flow against real usage (time-to-enter-50-cards is the metric that matters)
3. Map wireframes to Rust/UI components; identify gaps needing custom components
4. Higher-fidelity passes only for the catalog and collection screens

## Open questions

- Card images: how prominent? (Drives layout and Scryfall image-loading strategy.)
- Mobile: same feature surface as desktop, or a focused subset (lookup + quick-add)?
- Keyboard-driven command palette for power users — v1 or later?

## Tasks

- [ ] Information architecture / nav structure
- [ ] Wireframe the five core screen groups
- [ ] Prototype the add-to-collection flow
- [ ] Component gap analysis vs. Rust/UI registry
