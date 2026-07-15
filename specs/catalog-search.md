# Catalog search

**Status:** draft
**Depends on:** [ui-design](ui-design.md), [data-model](data-model.md)

The queryable field vocabulary and the base search indexes come from
[data-model](data-model.md) — which says *"the query→SQL translation is
catalog-search's"* and *"Data-model provides the base; catalog-search
refines"*; the two-surface UX and the rail's curated vocabulary come from
[ui-design](ui-design.md). **[catalog-ingestion](catalog-ingestion.md) is a
runtime sibling, not a design dependency** — both are downstream of data-model,
and search returns *real* results only once ingestion has loaded the catalog.
[collection-api](collection-api.md)'s search endpoint executes this query
grammar and already settled the backend as **SQL against our ingested catalog**
(not a Scryfall proxy) — which is what makes catalog-ingestion real Phase 4 work
(see its TODO tasks).

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

**Multi-face note (from data-model's 2026-07-14 Scryfall shape review):** for
multi-face layouts the top-level `oracle_text` is `NULL` — the per-face text lives
in `cards.card_faces jsonb`. So the `tsvector` backing `o:` (oracle text) must
concatenate `card_faces[].oracle_text`, or back-face text (e.g. a transform card's
reverse) won't match. Scryfall's "match if any face qualifies" semantics apply to
`t:`/`pow`/`tough` too. Newly stored fields are available as filter inputs when this
spec defines its rail/query vocabulary: `produced_mana` (`produces:`), `legalities`
(`f:`/`banned:`/`legal:`), `frame_effects`/`promo_types`/`full_art`/`textless`
(`is:`), and `digital`/`games` (`game:`/`is:digital`).

## Open questions

- Which Scryfall syntax subset ships in v1? (Full grammar including `or`/negation/parens is a real parser; a flat `key:value` term list covers most usage.)
- Does unrecognized-term preservation need the "N advanced terms" rail indicator, or is silence fine?
- Server-side: does the query translate to SQL against the ingested catalog, or proxy to Scryfall's search API as a stopgap?
- Debounce/latency budget for live results-as-you-type (the wireframe promises live updates on both rail edits and typing).
