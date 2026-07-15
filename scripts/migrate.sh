#!/usr/bin/env bash
#
# Run pending sqlx migrations against a Neon branch as the OWNER role.
#
# This is the "Option B" migration path (specs/data-model.md → Migration plan):
# the app itself runs as the non-owner `app_runtime` role and never executes DDL,
# so migrations are applied here, deliberately, from this trusted dev container —
# never from CI, never from the running web service. (A paid Render pre-deploy
# hook is the future upgrade once the project warrants it.)
#
# Usage:
#   scripts/migrate.sh            # migrate the DEV branch (default)
#   scripts/migrate.sh dev
#   scripts/migrate.sh prod       # migrate the PRODUCTION branch (prompts first)
#   scripts/migrate.sh prod -y    # ...skip the confirmation
#
# The owner connection string is read from .devcontainer/.env (gitignored) so the
# credential is never typed or pasted. Expected keys:
#   dev  -> MIGRATION_DATABASE_URL       (falls back to DATABASE_URL)
#   prod -> PROD_MIGRATION_DATABASE_URL
set -euo pipefail

target="${1:-dev}"
skip_confirm=""
[[ "${2:-}" == "-y" || "${2:-}" == "--yes" ]] && skip_confirm=1

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="$root/.devcontainer/.env"

if [[ ! -f "$env_file" ]]; then
  echo "error: $env_file not found — copy .devcontainer/.env.example and fill it in" >&2
  exit 1
fi

# Read a key's value from the env file WITHOUT sourcing it: connection strings
# contain '&', '?', ':' etc. that `source` would mis-parse as shell syntax.
# `cut -d= -f2-` keeps everything after the first '='; commented (`#…`) lines
# don't match the `^KEY=` anchor.
get_var() {
  grep -E "^$1=" "$env_file" | tail -n1 | cut -d= -f2- | tr -d '\r'
}

case "$target" in
  dev)
    url="$(get_var MIGRATION_DATABASE_URL || true)"
    [[ -z "$url" ]] && url="$(get_var DATABASE_URL || true)"
    missing="MIGRATION_DATABASE_URL (or DATABASE_URL)"
    ;;
  prod)
    url="$(get_var PROD_MIGRATION_DATABASE_URL || true)"
    missing="PROD_MIGRATION_DATABASE_URL"
    ;;
  *)
    echo "usage: $0 [dev|prod] [-y]" >&2
    exit 2
    ;;
esac

if [[ -z "$url" ]]; then
  echo "error: $missing is not set in $env_file" >&2
  exit 1
fi

# Host only (no credential) so you can see which branch you're about to touch.
host="$(printf '%s' "$url" | sed -E 's#^[^@]*@([^/?]+).*#\1#')"
echo "→ migrating [$target] @ $host"

if [[ "$target" == "prod" && -z "$skip_confirm" ]]; then
  read -r -p "Apply migrations to PRODUCTION ($host)? [y/N] " reply
  [[ "$reply" == "y" || "$reply" == "Y" ]] || { echo "aborted"; exit 1; }
fi

# Force the sqlx::migrate!("../migrations") macro (in app/src/db.rs) to re-embed
# the migrations directory. cargo doesn't reliably detect a *newly added* .sql
# file on its own, so without this a fresh migration can be silently skipped and
# `--migrate` reports "up to date" without applying it (observed 2026-07-15 on the
# devcontainer overlay fs). Touching the macro's source guarantees a re-embed.
touch "$root/app/src/db.rs"

MIGRATION_DATABASE_URL="$url" cargo run --quiet -p server -- --migrate
