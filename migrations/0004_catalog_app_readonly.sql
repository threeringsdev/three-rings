-- Data-model migration — enforce catalog read-only for the app role.
--
-- Finding (verified against the dev branch during this task): a pre-existing Neon
-- default privilege — pg_default_acl, granted by neondb_owner when the app_runtime
-- role was provisioned (2026-07-12) — auto-grants app_runtime CRUD (arwd) on every
-- table neondb_owner creates. So the catalog tables from 0002 landed *writable* by
-- app_runtime, and 0002's explicit GRANT SELECT was redundant.
--
-- The spec requires catalog writes to happen ONLY through the owner/ingestion role
-- (specs/data-model.md → Row-level security: "Writes happen only through the
-- ingestion role"). The runtime web server connects as app_runtime and only ever
-- READS the catalog; ingestion runs as the owner. So strip app_runtime's write
-- access on the catalog tables, leaving the SELECT from 0002. Table-scoped REVOKE
-- (not a role-wide ALTER DEFAULT PRIVILEGES change) — future catalog tables repeat
-- this pattern. User tables are untouched: app_runtime needs CRUD there (RLS-gated).
REVOKE INSERT, UPDATE, DELETE, TRUNCATE
    ON sets, cards, printings, prices, rulings
    FROM app_runtime;
