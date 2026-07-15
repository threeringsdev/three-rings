-- Data-model migration — COLLECTION group (specs/data-model.md → Migration plan,
-- group 2). The user-owned tree, the two holding tables (present/desired), the
-- move ledger, the owned aggregate view, and RLS. References Neon Auth (Better
-- Auth) neon_auth."user"(id) — a real in-database uuid-PK table with hard
-- deletes, so hard FKs with ON DELETE CASCADE are correct.
--
-- Two execution-deferred OQs from the spec are resolved here:
--   * Denormalize user_id onto holdings/desires/deck_commanders (over
--     EXISTS-on-collection RLS policies): simpler + faster direct policies; the
--     API keeps it consistent with the owning collection's user_id.
--   * Keep full finish + condition + language granularity on holdings/moves
--     (defaulted), rather than trimming v1 to finish-only: cheap now, painful to
--     retrofit into the uniqueness constraint later.
--
-- Grants at the foot assume the non-owner app_runtime role exists (see 0002).

CREATE TYPE collection_kind AS ENUM ('binder', 'deck');
CREATE TYPE card_condition  AS ENUM ('nm', 'lp', 'mp', 'hp', 'dmg'); -- physical grade; default 'nm'

CREATE TABLE collections (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- Neon Auth (Better Auth) user id
    parent_id  uuid REFERENCES collections(id) ON DELETE CASCADE,  -- NULL = top level
    kind       collection_kind NOT NULL,
    name       text NOT NULL,
    is_inbox   boolean NOT NULL DEFAULT false,
    position   numeric NOT NULL DEFAULT 0,     -- fractional index for O(1) drag-reorder among siblings
    format     text,                            -- decks only (e.g. 'commander','modern')
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (format IS NULL OR kind = 'deck')
);

-- exactly one Inbox per user
CREATE UNIQUE INDEX collections_one_inbox ON collections (user_id) WHERE is_inbox;
CREATE INDEX collections_user_parent_idx ON collections (user_id, parent_id);

CREATE TABLE deck_commanders (                  -- 0..2 commanders (partners); decks only, enforced in app
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    printing_id   uuid NOT NULL REFERENCES printings(id),
    user_id       uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- denormalized from the owning collection (RLS)
    PRIMARY KEY (collection_id, printing_id)
);

-- PRESENT: physical copies in a collection, at printing grain.
CREATE TABLE holdings (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    printing_id   uuid NOT NULL REFERENCES printings(id),
    user_id       uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- denormalized from the owning collection (RLS)
    finish        card_finish   NOT NULL DEFAULT 'nonfoil',
    condition     card_condition NOT NULL DEFAULT 'nm',
    language      text NOT NULL DEFAULT 'en',
    quantity      integer NOT NULL CHECK (quantity > 0),
    UNIQUE (collection_id, printing_id, finish, condition, language)
);

-- DESIRED: a target count in a collection, at ORACLE grain by default,
-- with an optional pin to a specific printing.
CREATE TABLE desires (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    oracle_id     uuid NOT NULL REFERENCES cards(oracle_id),
    printing_id   uuid REFERENCES printings(id),   -- NULL = any printing; set = "specific printing only" pin
    user_id       uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- denormalized from the owning collection (RLS)
    quantity      integer NOT NULL CHECK (quantity > 0),
    UNIQUE NULLS NOT DISTINCT (collection_id, oracle_id, printing_id)
);

CREATE TABLE moves (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id            uuid NOT NULL REFERENCES neon_auth."user"(id) ON DELETE CASCADE,  -- Neon Auth (Better Auth) user id
    printing_id        uuid NOT NULL REFERENCES printings(id),
    finish             card_finish   NOT NULL,
    condition          card_condition NOT NULL,
    language           text NOT NULL,
    from_collection_id uuid REFERENCES collections(id) ON DELETE SET NULL,  -- NULL = external intake (+Have)
    to_collection_id   uuid REFERENCES collections(id) ON DELETE SET NULL,  -- NULL = removal
    quantity           integer NOT NULL CHECK (quantity > 0),
    created_at         timestamptz NOT NULL DEFAULT now(),
    undone_at          timestamptz                                          -- set when reversed
);
CREATE INDEX moves_user_created_idx ON moves (user_id, created_at DESC);
CREATE INDEX moves_to_recent_idx    ON moves (to_collection_id, printing_id, created_at DESC);

-- OWNED (per user, per oracle card): a pure aggregate of present, never stored.
-- security_invoker so it runs with the querying app_runtime's RLS context (the
-- app.user_id GUC), not the owner's — the owner is RLS-forced but making the
-- invoker explicit keeps the per-user scoping unambiguous.
CREATE VIEW owned_by_card WITH (security_invoker = true) AS
SELECT c.user_id, p.oracle_id, sum(h.quantity)::int AS owned
FROM holdings h
JOIN collections c ON c.id = h.collection_id
JOIN printings   p ON p.id = h.printing_id
GROUP BY c.user_id, p.oracle_id;

-- Row-level security: on + FORCED (the migration owner would otherwise bypass)
-- as defense-in-depth beneath the hosted API terminus. Every request runs
-- SET LOCAL app.user_id = <verified Neon Auth uuid> in its transaction; the
-- policies scope each row to it. With no GUC set (anonymous), current_setting's
-- missing_ok=true yields NULL → NULL::uuid → no rows (safe deny-by-default).
-- All five user tables carry user_id directly (denormalized above), so every
-- policy is one direct comparison — no EXISTS-on-collection hop.

ALTER TABLE collections     ENABLE ROW LEVEL SECURITY;
ALTER TABLE collections     FORCE  ROW LEVEL SECURITY;
CREATE POLICY collections_owner ON collections
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

ALTER TABLE deck_commanders ENABLE ROW LEVEL SECURITY;
ALTER TABLE deck_commanders FORCE  ROW LEVEL SECURITY;
CREATE POLICY deck_commanders_owner ON deck_commanders
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

ALTER TABLE holdings        ENABLE ROW LEVEL SECURITY;
ALTER TABLE holdings        FORCE  ROW LEVEL SECURITY;
CREATE POLICY holdings_owner ON holdings
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

ALTER TABLE desires         ENABLE ROW LEVEL SECURITY;
ALTER TABLE desires         FORCE  ROW LEVEL SECURITY;
CREATE POLICY desires_owner ON desires
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

ALTER TABLE moves           ENABLE ROW LEVEL SECURITY;
ALTER TABLE moves           FORCE  ROW LEVEL SECURITY;
CREATE POLICY moves_owner ON moves
    USING (user_id = current_setting('app.user_id', true)::uuid)
    WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- The app runs as the non-owner, RLS-subject app_runtime role.
GRANT SELECT, INSERT, UPDATE, DELETE ON collections, deck_commanders, holdings, desires, moves TO app_runtime;
GRANT SELECT ON owned_by_card TO app_runtime;
