-- Data-model migration — CATALOG group (specs/data-model.md → Migration plan,
-- group 1). Scryfall-sourced, public/read-mostly catalog: sets, oracle-identity
-- cards, printings, latest-only prices, rulings, plus pg_trgm + the base search
-- indexes. No auth dependency. Population is catalog-ingestion's; this is the
-- empty schema.
--
-- Grants at the foot assume the non-owner `app_runtime` role already exists
-- (provisioned out-of-band on both Neon branches, 2026-07-12 — it carries a
-- login password we deliberately keep out of migrations). A missing role fails
-- the GRANT loudly, which correctly signals the ops prerequisite.

-- The spike's throwaway cards(id serial, name) (migration 0001) is unrelated to
-- the real oracle-identity cards below; drop it before recreating the name.
DROP TABLE IF EXISTS cards;

-- Scryfall's finish set. Used by printings.finishes here and holdings/moves in
-- the collection group. ALTER TYPE ... ADD VALUE later is cheap + non-blocking.
CREATE TYPE card_finish AS ENUM ('nonfoil', 'foil', 'etched');

CREATE TABLE sets (
    id           uuid PRIMARY KEY,          -- Scryfall set id
    code         text NOT NULL UNIQUE,      -- 'mh3', 'lea', …
    name         text NOT NULL,
    set_type     text NOT NULL,             -- 'expansion','core','commander',…
    released_at  date,
    card_count   integer,
    icon_svg_uri text
);

CREATE TABLE cards (                        -- ORACLE identity (one row per distinct card)
    oracle_id       uuid PRIMARY KEY,       -- Scryfall oracle_id; for layout='reversible_card' read from card_faces[0].oracle_id
    name            text NOT NULL,          -- combined "Front // Back" on multi-face layouts
    mana_cost       text,                   -- single-face only; NULL on multi-face → see card_faces
    cmc             numeric,                -- fractional for un-cards
    type_line       text,                   -- combined on multi-face
    oracle_text     text,                   -- single-face only; NULL on multi-face → see card_faces
    colors          text[] NOT NULL DEFAULT '{}',   -- single-face; per-face colors live in card_faces
    color_identity  text[] NOT NULL DEFAULT '{}',   -- whole-card (Scryfall aggregates across faces)
    color_indicator text[],                 -- frame color pip (e.g. Ancestral Vision); per-face on DFCs
    keywords        text[] NOT NULL DEFAULT '{}',
    power           text,                   -- '*'/'1+*' → text, not int; single-face, NULL on multi-face
    toughness       text,
    loyalty         text,
    produced_mana   text[],                 -- mainly catalog-search's `produces:` filter
    layout          text,                   -- 'normal','split','transform','modal_dfc','adventure','flip','meld','reversible_card','token','double_faced_token',…
    legalities      jsonb,                  -- flat {format: legal|not_legal|banned|restricted}; rendered on card page
    card_faces      jsonb,                  -- NULL single-face; else ordered per-face oracle data → see "Multi-face cards & relations"
    all_parts       jsonb,                  -- NULL unless related; token/meld/combo links → see "Multi-face cards & relations"
    reserved        boolean NOT NULL DEFAULT false,
    game_changer    boolean,                -- commander-bracket flag; renders near legalities
    edhrec_rank     integer
);

CREATE TABLE printings (                    -- a specific physical object collections point at
    id               uuid PRIMARY KEY,      -- Scryfall card id
    oracle_id        uuid NOT NULL REFERENCES cards(oracle_id),
    set_id           uuid NOT NULL REFERENCES sets(id),
    collector_number text NOT NULL,
    rarity           text NOT NULL,         -- 'common','uncommon','rare','mythic','special','bonus' (tokens/checklist carry 'common')
    finishes         card_finish[] NOT NULL DEFAULT '{}',  -- which finishes this printing exists in
    lang             text NOT NULL DEFAULT 'en',
    frame            text,
    frame_effects    text[] NOT NULL DEFAULT '{}',  -- 'showcase','extendedart','inverted',… distinguishes treatments within a set
    border_color     text,
    full_art         boolean NOT NULL DEFAULT false,
    textless         boolean NOT NULL DEFAULT false,
    promo            boolean NOT NULL DEFAULT false,
    promo_types      text[] NOT NULL DEFAULT '{}',   -- 'boosterfun','godzillaseries','buyabox',…
    flavor_name      text,                            -- displayed name for Godzilla/Dracula/etc. variants (name still holds the real card)
    artist           text,                            -- single-face; per-face in faces jsonb on DFCs
    flavor_text      text,                            -- printing-level; per-face on DFCs
    watermark        text,                            -- guild/clan/set watermark, when present
    security_stamp   text,                            -- 'oval','triangle','acorn',… anti-counterfeit stamp
    games            text[] NOT NULL DEFAULT '{}',    -- 'paper','arena','mtgo' this printing exists in
    digital          boolean NOT NULL DEFAULT false,  -- Arena/MTGO-only printing; label/filter in the printing picker
    released_at      date,
    image_uris       jsonb,                 -- single-face images; NULL on multi-face → see faces. Link-out (caching deferred → catalog-ingestion)
    faces            jsonb,                 -- NULL single-face; else per-printing per-face data → see "Multi-face cards & relations"
    UNIQUE (set_id, collector_number, lang)
);

CREATE TABLE prices (                       -- latest snapshot only; upserted by ingestion
    printing_id uuid PRIMARY KEY REFERENCES printings(id) ON DELETE CASCADE,
    usd         numeric(10,2),
    usd_foil    numeric(10,2),
    usd_etched  numeric(10,2),
    eur         numeric(10,2),
    eur_foil    numeric(10,2),
    tix         numeric(10,2),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE rulings (                       -- rendered on the card page; population is catalog-ingestion's
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    oracle_id    uuid NOT NULL REFERENCES cards(oracle_id),
    published_at date,
    source       text,                       -- 'wotc','scryfall'
    comment      text NOT NULL
);
CREATE INDEX rulings_oracle_idx ON rulings (oracle_id);

-- Base search indexes (the floor; catalog-search refines with oracle-text FTS,
-- filter indexes, etc.). Fuzzy name search + the printing join/filter indexes.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX cards_name_trgm_idx  ON cards USING gin (name gin_trgm_ops);  -- fuzzy name (both search surfaces)
CREATE INDEX printings_oracle_idx ON printings (oracle_id);
CREATE INDEX printings_set_idx    ON printings (set_id);

-- Catalog is public read for the app; RLS stays OFF (writes happen only through
-- the owner/ingestion role). The app connects as app_runtime and reads freely.
GRANT USAGE ON SCHEMA public TO app_runtime;
GRANT SELECT ON sets, cards, printings, prices, rulings TO app_runtime;
