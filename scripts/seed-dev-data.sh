#!/usr/bin/env bash
#
# Build the e2e test user's dev seed tree on the Neon DEV branch
# (specs/app-ui.md → Dev seed data). Thin orchestration:
#
#   1. resolve the e2e user's uuid from neon_auth."user" (owner credential,
#      scoped to the single psql call via PG* env — never argv, never
#      inherited by later processes; email bound as a psql variable)
#   2. `server --seed-dev <uuid>` — the actual seeding runs through the real
#      CollectionStore methods against DATABASE_URL (app_runtime, RLS-subject).
#      Debug builds only; release binaries don't carry the arm.
#
# Prereqs: the e2e user exists (end2end/seed-e2e-user.sh) and the POC catalog
# is ingested on the dev branch. Idempotent: a sentinel collection short-
# circuits a re-run; a mid-seed failure cleans up its own roots. Never point
# this at production.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

email="${E2E_EMAIL:-}"
if [[ -z "$email" && -f "$root/end2end/.env" ]]; then
  email="$(grep -E '^E2E_EMAIL=' "$root/end2end/.env" | cut -d= -f2-)"
fi
[[ -n "$email" ]] || { echo "no E2E_EMAIL (run end2end/seed-e2e-user.sh first)" >&2; exit 1; }

mig_url="$(grep -E '^MIGRATION_DATABASE_URL=' "$root/.devcontainer/.env" | cut -d= -f2-)"
[[ -n "$mig_url" ]] || { echo "MIGRATION_DATABASE_URL missing from .devcontainer/.env" >&2; exit 1; }
s=${mig_url#postgres*://}
creds=${s%%@*} rest=${s#*@} hostport=${rest%%/*} dbq=${rest#*/}
port=""
[[ "$hostport" == "${hostport%%:*}" ]] || port=${hostport#*:}

# One psql call with the credential in ITS environment only; the email rides a
# psql variable (:'email'), never string interpolation into owner-privileged SQL.
uuid="$(PGUSER=${creds%%:*} PGPASSWORD=${creds#*:} PGHOST=${hostport%%:*} \
  PGDATABASE=${dbq%%\?*} PGSSLMODE=require ${port:+PGPORT=$port} \
  psql -v ON_ERROR_STOP=1 -qtA -v email="$email" -f - \
    <<<"SELECT id FROM neon_auth.\"user\" WHERE email = :'email'")"
[[ -n "$uuid" ]] || { echo "no neon_auth user for $email — run end2end/seed-e2e-user.sh" >&2; exit 1; }

echo "seeding dev data for $email ($uuid)"
cd "$root" && cargo run --quiet -p server -- --seed-dev "$uuid"
