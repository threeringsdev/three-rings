-- Soft deletion for collections (specs/collection-deletion.md → Data model),
-- step 2 of that spec's sequencing: the column, its index, and the *one* place
-- the owned-per-oracle filter belongs. Deliberately inert — nothing sets
-- `deleted_at` yet, so every filter added alongside this migration is a no-op
-- and the risky part (the delete operation itself) lands separately.
--
-- Expand-only and safe to apply before the code that reads the column: a
-- pre-migration server never selects it, and a post-migration server sees NULL
-- (= live) on every existing row.
--
-- The `ON DELETE CASCADE` on `collections.parent_id`, `holdings`, `desires` and
-- `card_tags`, and the `ON DELETE SET NULL` on the two `moves` collection
-- columns, all **stay**. Soft delete never fires them, which is exactly what
-- stops the ledger rewriting history into intakes and removals. The
-- `collections_one_inbox` unique index is likewise left alone: the Inbox cannot
-- be deleted, so a soft-deleted Inbox cannot exist.

ALTER TABLE collections ADD COLUMN deleted_at timestamptz;  -- NULL = live

-- The hot path is "live collections for this user", so index that specifically
-- (the unconditional collections_user_parent_idx from 0003 stays for the reads
-- that do not care).
CREATE INDEX collections_user_parent_live_idx
    ON collections (user_id, parent_id) WHERE deleted_at IS NULL;

-- Owned-per-oracle's filter lands **exactly once**, here in the view, rather
-- than scattered across hosted.rs: P6-039 collapsed the three inline
-- re-derivations of this aggregate onto `owned_by_card`, and a structural test
-- (`owned_definition_guard`) keeps them collapsed. A card held only inside a
-- soft-deleted collection therefore stops being "owned" everywhere at once —
-- search/card-summary badges, the everything-view, the shopping list, the
-- collection view's `owned` column and the tree's shopping-short badge.
--
-- CREATE OR REPLACE (not DROP + CREATE) so the `app_runtime` GRANT from 0003
-- survives; the column list, order and types are unchanged, which is what
-- REPLACE requires. `WITH (security_invoker = true)` is restated deliberately —
-- reloptions are set from what this statement says, and silently dropping the
-- invoker flag would make the view run as its RLS-exempt owner.
CREATE OR REPLACE VIEW owned_by_card WITH (security_invoker = true) AS
SELECT c.user_id, p.oracle_id, sum(h.quantity)::int AS owned
FROM holdings h
JOIN collections c ON c.id = h.collection_id
JOIN printings   p ON p.id = h.printing_id
WHERE c.deleted_at IS NULL
GROUP BY c.user_id, p.oracle_id;
