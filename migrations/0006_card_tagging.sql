-- Card tagging & deck boards (specs/card-tagging.md → Migration). Adds the two
-- annotation shapes uncovered in the 2026-07-15 review:
--   * boards — a quantity-bearing partition, as a `board` column folded into the
--     holdings/desires uniqueness keys (so "2 main + 1 side" is two rows);
--   * tags — a `tags` + `card_tags` many-to-many (system/account/deck scopes).
-- Retires the point-solution `deck_commanders` table (commander is now a built-in
-- tag). Expand-first: `board` defaults to 'main' on existing rows; deck_commanders
-- is empty. Grants follow 0005's convention — app_runtime gets SELECT by default,
-- so user-table migrations GRANT the writes explicitly.

-- 1. Boards -----------------------------------------------------------------
CREATE TYPE card_board AS ENUM ('main', 'side', 'maybe');   -- deck boards; 'main' = mainboard

ALTER TABLE holdings ADD COLUMN board card_board NOT NULL DEFAULT 'main';
ALTER TABLE desires  ADD COLUMN board card_board NOT NULL DEFAULT 'main';

-- Fold `board` into each uniqueness key so a deck can hold distinct per-board
-- quantities of the same card. The old inline UNIQUE constraints have
-- auto-generated (truncated) names, so drop them by lookup: each table has
-- exactly one unique constraint (contype 'u'); the primary key (contype 'p') is
-- left untouched.
DO $$
DECLARE cname text;
BEGIN
    SELECT conname INTO cname FROM pg_constraint
     WHERE conrelid = 'holdings'::regclass AND contype = 'u';
    EXECUTE format('ALTER TABLE holdings DROP CONSTRAINT %I', cname);
END $$;
ALTER TABLE holdings ADD CONSTRAINT holdings_uniq
    UNIQUE (collection_id, printing_id, finish, condition, language, board);

DO $$
DECLARE cname text;
BEGIN
    SELECT conname INTO cname FROM pg_constraint
     WHERE conrelid = 'desires'::regclass AND contype = 'u';
    EXECUTE format('ALTER TABLE desires DROP CONSTRAINT %I', cname);
END $$;
ALTER TABLE desires ADD CONSTRAINT desires_uniq
    UNIQUE NULLS NOT DISTINCT (collection_id, oracle_id, printing_id, board);

-- 2. Tags + card_tags -------------------------------------------------------
CREATE TABLE tags (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- NULL = system/built-in
    collection_id uuid REFERENCES collections(id) ON DELETE CASCADE,        -- non-NULL = deck-scoped
    name          text NOT NULL,
    builtin       text,        -- stable slug for system tags ('commander','companion'); NULL for user tags
    color         text,        -- optional UI accent
    created_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (collection_id IS NULL OR user_id IS NOT NULL),                 -- a deck tag is owned
    CHECK (builtin IS NULL OR (user_id IS NULL AND collection_id IS NULL)) -- builtins are system-scoped
);

-- scope-unique names (one partial unique index per scope):
CREATE UNIQUE INDEX tags_system_name  ON tags (name)                WHERE user_id IS NULL;
CREATE UNIQUE INDEX tags_account_name ON tags (user_id, name)       WHERE user_id IS NOT NULL AND collection_id IS NULL;
CREATE UNIQUE INDEX tags_deck_name    ON tags (collection_id, name) WHERE collection_id IS NOT NULL;

-- Seed the built-in system tags BEFORE forcing RLS below — an RLS-forced owner
-- cannot insert a `user_id IS NULL` row (it fails the tags_owner WITH CHECK).
INSERT INTO tags (name, builtin) VALUES ('commander', 'commander'), ('companion', 'companion');

CREATE TABLE card_tags (
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    oracle_id     uuid NOT NULL REFERENCES cards(oracle_id),
    tag_id        uuid NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    user_id       uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- denormalized (RLS)
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, oracle_id, tag_id)
);
CREATE INDEX card_tags_tag_idx        ON card_tags (tag_id);                 -- "which cards carry tag X"
CREATE INDEX card_tags_collection_idx ON card_tags (collection_id, oracle_id);

-- 3. RLS (after the seed) ---------------------------------------------------
-- tags: system rows (user_id NULL) are world-readable but not user-writable;
-- OR-combined permissive policies give SELECT = own+system, writes = own only.
ALTER TABLE tags ENABLE ROW LEVEL SECURITY;
ALTER TABLE tags FORCE  ROW LEVEL SECURITY;
CREATE POLICY tags_read ON tags FOR SELECT
    USING (user_id IS NULL OR user_id = current_setting('app.user_id', true)::uuid);
CREATE POLICY tags_owner ON tags FOR ALL
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

ALTER TABLE card_tags ENABLE ROW LEVEL SECURITY;
ALTER TABLE card_tags FORCE  ROW LEVEL SECURITY;
CREATE POLICY card_tags_owner ON card_tags
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- 4. Grants (app_runtime is SELECT-by-default per 0005; grant the writes) ----
GRANT INSERT, UPDATE, DELETE ON tags, card_tags TO app_runtime;

-- 5. Retire deck_commanders (empty; commander is now the built-in tag) -------
DROP TABLE deck_commanders;
