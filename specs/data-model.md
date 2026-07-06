# Data model

**Status:** draft
**Depends on:** —

## Problem

Define the Postgres schema for the card catalog and user collections — the foundation every other spec builds on.

## Scope

In: catalog tables (cards, printings, sets, prices), user and collection tables, indexing strategy for card search. Out: decks, sharing, trade features (future specs).

## Design

To be worked out. Starting considerations:

- Catalog keyed on Scryfall IDs; a card (oracle identity) vs. a printing (set/collector number/finish) are distinct entities — collections reference printings.
- Price data is high-churn; consider a separate table (or partition) so catalog updates don't rewrite card rows.
- Full-text / trigram search on card names; likely `pg_trgm` for fuzzy matching.
- Collection rows: (user_id, printing_id, finish, condition, language, quantity) with a uniqueness constraint.
- RLS policies as defense-in-depth beneath API authorization — decide whether to enable from day one.

## Open questions

- Store full oracle text / rulings, or only what the UI needs and link out?
- How much price history to retain?

## Tasks

- [ ] Draft ERD
- [ ] Write initial sqlx migrations
- [ ] Decide on RLS from day one vs. later
