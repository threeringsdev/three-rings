-- catalog-search indexes (specs/catalog-search.md → Query → SQL), per
-- data-model's delegation: "catalog-search refines" the base search indexes.
--
-- oracle_search_text closes the multi-face `o:` gap flagged by data-model's
-- 2026-07-14 shape review: multi-face layouts carry oracle text only inside
-- card_faces, so a top-level-only search would miss every back face. The
-- generated column concatenates top-level text with every face's text
-- (jsonb_path_query_array is IMMUTABLE — verified live), lowercased so the
-- engine can use plain LIKE with a lowered bind. Substring semantics via trgm
-- match the name search; a tsvector is the deferred relevance upgrade path.

ALTER TABLE cards ADD COLUMN oracle_search_text text GENERATED ALWAYS AS (
    lower(coalesce(oracle_text, '') || ' ' ||
          coalesce(jsonb_path_query_array(card_faces, '$[*].oracle_text')::text, ''))
) STORED;

CREATE INDEX cards_oracle_search_trgm_idx
    ON cards USING gin (oracle_search_text gin_trgm_ops);

-- `t:` filters are substring matches over type_line (combined across faces).
CREATE INDEX cards_type_line_trgm_idx
    ON cards USING gin (type_line gin_trgm_ops);

-- Deliberately NOT indexed yet: colors / card_faces GIN (most queries carry a
-- name/type/text term that already narrows via the indexes above; profile at
-- full catalog scale first — catalog-search Open questions).
