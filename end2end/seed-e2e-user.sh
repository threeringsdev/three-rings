#!/usr/bin/env bash
#
# Seed the verified e2e test user on the Neon DEV branch (specs/ui-work-loop.md
# → E2E baseline reset; the e2e-suite skill).
#
# - generates end2end/.env (E2E_EMAIL / E2E_PASSWORD) if missing; when the
#   creds are freshly generated, any pre-existing e2e user row is deleted
#   first (its old password is unknowable), so the script is idempotent from
#   ANY state — fresh checkout included
# - signs the user up through the real /signup form on :3000 (the dev server,
#   which points at the Neon dev branch)
# - flips neon_auth."user"."emailVerified" via the owner credential
#   (MIGRATION_DATABASE_URL in .devcontainer/.env — the migrate.sh convention)
#
# Never point this at production.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
base="${E2E_BASE:-http://127.0.0.1:3000}"

# -- owner credential → PG* env scoped to each psql call only, so the secret
#    never appears in argv (process tables are world-readable) and is never
#    inherited by the node child. The e2e email rides a psql variable
#    (:'email'), never string interpolation. Assumes Neon's URL shape:
#    postgresql://user:pass@host[:port]/db?sslmode=require...
mig_url="$(grep -E '^MIGRATION_DATABASE_URL=' "$root/.devcontainer/.env" | cut -d= -f2-)"
if [[ -z "$mig_url" ]]; then
  echo "MIGRATION_DATABASE_URL missing from .devcontainer/.env" >&2
  exit 1
fi
s=${mig_url#postgres*://}
creds=${s%%@*} rest=${s#*@} hostport=${rest%%/*} dbq=${rest#*/}
port=""
[[ "$hostport" == "${hostport%%:*}" ]] || port=${hostport#*:}
owner_sql() {
  PGUSER=${creds%%:*} PGPASSWORD=${creds#*:} PGHOST=${hostport%%:*} \
    PGDATABASE=${dbq%%\?*} PGSSLMODE=require ${port:+PGPORT=$port} \
    psql -v ON_ERROR_STOP=1 -qtA -v email="$E2E_EMAIL" -f - <<<"$1"
}

# -- credentials
env_file="$here/.env"
fresh_env=""
if [[ ! -f "$env_file" ]]; then
  pw="$(openssl rand -hex 10)"
  printf 'E2E_EMAIL=three-rings-e2e@example.com\nE2E_PASSWORD=%s\n' "$pw" >"$env_file"
  fresh_env=1
  echo "created $env_file"
fi
# shellcheck disable=SC1090
source "$env_file"

# -- fresh creds + existing user = unknowable old password: recreate the user.
#    Deleting cascades the account row and any collections (FK ON DELETE CASCADE);
#    this is the purpose-built e2e account on the dev branch, owned by this script.
if [[ -n "$fresh_env" ]]; then
  deleted="$(owner_sql "DELETE FROM neon_auth.\"user\" WHERE email = :'email' RETURNING email")"
  [[ -n "$deleted" ]] && echo "recreating $E2E_EMAIL (stale user row deleted — old password unknown)"
fi

# -- dev server up?
if ! curl -sf -o /dev/null "$base/"; then
  echo "no server at $base — start 'cargo leptos watch --features component-bench' first" >&2
  exit 1
fi

# -- sign up through the real form
(cd "$here" && node seed-e2e-user.mjs "$base" "$E2E_EMAIL" "$E2E_PASSWORD")

# -- flip verified
flipped="$(owner_sql "UPDATE neon_auth.\"user\" SET \"emailVerified\" = true WHERE email = :'email' RETURNING email")"
if [[ -z "$flipped" ]]; then
  echo "no neon_auth user row for $E2E_EMAIL — signup did not land?" >&2
  exit 1
fi
echo "seed complete: $flipped verified"
