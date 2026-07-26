-- Boards on the move ledger (specs/collection-api.md → Move / Undo / Teardown).
--
-- `moves` recorded printing + finish + condition + language but no board, so
-- every move was implicitly a *mainboard* move: `holding_take` pinned
-- `board = 'main'` and `holding_add` inserted `'main'`. Two consequences, both
-- data-integrity bugs rather than missing features:
--
--   * a deck's sideboard/maybeboard stack could not be moved or removed at all
--     (the write addressed a mainboard row that did not exist), and
--   * anything that *did* touch a non-mainboard stack — `+ Have` at
--     `board = 'side'` appends an intake move — could not be undone correctly,
--     because the reversal had no record of which board to put the copies back
--     on and would have taken/returned them on the mainboard.
--
-- Two columns, not one, because a move's two ends are not the same board: the
-- source board is a property of the stack the copies were taken *from*, while
-- the destination board is where they landed (today always `main` — moving a
-- sideboard card into a binder makes it an ordinary binder card; re-labelling a
-- board is card-tagging's separate quantity-preserving op, not a move). Undo
-- needs both, and one column could not say which end it described.
--
-- Expand-first: both default to 'main', which is exactly what every existing
-- row meant, so no backfill is needed and the pre-migration server keeps working
-- against the migrated schema.

ALTER TABLE moves ADD COLUMN from_board card_board NOT NULL DEFAULT 'main';  -- board the copies left (NULL `from` = intake: unused)
ALTER TABLE moves ADD COLUMN to_board   card_board NOT NULL DEFAULT 'main';  -- board they landed on (NULL `to` = removal: unused)
