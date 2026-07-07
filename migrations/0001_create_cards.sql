-- Spike schema (architecture-spike task 5): one trivial table + seed rows,
-- just enough to prove Neon + sqlx through the server path. The real schema
-- comes from the data-model spec (Phase 2).
CREATE TABLE cards (
    id   serial PRIMARY KEY,
    name text NOT NULL
);

INSERT INTO cards (name) VALUES
    ('Sol Ring'),
    ('Lightning Bolt'),
    ('Counterspell');
