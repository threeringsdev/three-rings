# Collection API

**Status:** draft
**Depends on:** data-model, auth

## Problem

Clients need endpoints to search the catalog and manage a personal collection.

## Scope

In: catalog search/browse endpoints, collection CRUD, shared request/response types crate. Out: decks, sharing, import/export (future specs).

## Design

To be worked out. Starting considerations:

- REST-ish JSON API via Axum; shared `shared/` crate defines request/response types used by both server and Leptos clients.
- Catalog search: name (fuzzy), set, color, type filters; paginated.
- Collection endpoints join collection ↔ catalog server-side; clients receive denormalized rows.
- Bulk add flow matters (users enter many cards at once — consider a batch endpoint from day one).

## Open questions

- Pagination style (cursor vs. offset) for 100K-row catalog search.
- CSV import (Moxfield/Archidekt formats) — v1 or later?
