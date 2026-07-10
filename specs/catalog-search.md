# Catalog search

**Status:** draft
**Depends on:** [ui-design](ui-design.md), [catalog-ingestion](catalog-ingestion.md)

## Problem

Catalog mode's search has two input surfaces — a query bar and a filter rail — and their relationship needs a defined contract. The catalog dataset comes from Scryfall, so users will arrive knowing Scryfall's query syntax (`t:instant c:ur cmc<=2`); the query bar should honor as much of it as practical. The rail, by contrast, is deliberately a curated everyday subset (name, text, set, color, type, rarity, mana value — per the Phase 1b wireframes), not a reproduction of Scryfall's full advanced-search form.

## Scope

In: which Scryfall query-syntax subset the query bar supports; the sync contract between query bar and filter rail; how combined search state serializes into the URL (the IA doc requires shareable/restorable searches). Out: the ingestion pipeline (catalog-ingestion), search backend/indexing implementation (data-access-backends / collection-api), and the rail's visual design (ui-design wireframes).

## Design

Proposed model — **one filter state, two views over it**:

- The rail and the query bar both edit a single underlying search state.
- Rail edits rewrite their corresponding term in the query text (checking Blue+Red keeps exactly one `c:` term in sync).
- Query-bar terms the rail understands ("matched" terms: name, text, set, color, type, rarity, mana value) reflect back into rail state — checkboxes, badges.
- Query-bar terms the rail has no widget for (e.g. `is:commander`, `year<=2003`) are preserved verbatim as opaque terms — they simply never appear in the rail. A small "N advanced terms" indicator on the rail can acknowledge them.
- Consequence: editing the query never destroys rail state and vice versa; the query text is always the complete serialization of the search (and is what goes in the URL).

## Open questions

- Which Scryfall syntax subset ships in v1? (Full grammar including `or`/negation/parens is a real parser; a flat `key:value` term list covers most usage.)
- Does unrecognized-term preservation need the "N advanced terms" rail indicator, or is silence fine?
- Server-side: does the query translate to SQL against the ingested catalog, or proxy to Scryfall's search API as a stopgap?
- Debounce/latency budget for live results-as-you-type (the wireframe promises live updates on both rail edits and typing).
