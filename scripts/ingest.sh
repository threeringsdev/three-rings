#!/usr/bin/env bash
#
# Run the catalog-ingestion bulk pipeline against a Neon branch as the
# least-privilege `catalog_ingest` role (specs/catalog-ingestion.md).
#
# Usage:
#   scripts/ingest.sh                 # POC subset against the DEV branch
#   scripts/ingest.sh dev poc
#   scripts/ingest.sh dev bulk        # full bulk load / rebuild / price true-up
#   scripts/ingest.sh prod bulk       # ...against PRODUCTION (prompts first)
#   scripts/ingest.sh prod bulk -y    # ...skip the confirmation
#
# Connection strings are read from .devcontainer/.env (gitignored), same
# discipline as scripts/migrate.sh:
#   dev  -> INGEST_DATABASE_URL
#   prod -> PROD_INGEST_DATABASE_URL
set -euo pipefail

target="${1:-dev}"
mode="${2:-poc}"
skip_confirm=""
[[ "${3:-}" == "-y" || "${3:-}" == "--yes" ]] && skip_confirm=1

case "$mode" in poc|bulk) ;; *) echo "usage: $0 [dev|prod] [poc|bulk] [-y]" >&2; exit 2;; esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="$root/.devcontainer/.env"

if [[ ! -f "$env_file" ]]; then
  echo "error: $env_file not found — copy .devcontainer/.env.example and fill it in" >&2
  exit 1
fi

# Read a key's value without sourcing (connection strings contain '&', '?', …).
get_var() {
  grep -E "^$1=" "$env_file" | tail -n1 | cut -d= -f2- | tr -d '\r'
}

case "$target" in
  dev)  url="$(get_var INGEST_DATABASE_URL || true)";      missing="INGEST_DATABASE_URL" ;;
  prod) url="$(get_var PROD_INGEST_DATABASE_URL || true)"; missing="PROD_INGEST_DATABASE_URL" ;;
  *)    echo "usage: $0 [dev|prod] [poc|bulk] [-y]" >&2; exit 2 ;;
esac

if [[ -z "$url" ]]; then
  echo "error: $missing is not set in $env_file" >&2
  exit 1
fi

host="$(printf '%s' "$url" | sed -E 's#^[^@]*@([^/?]+).*#\1#')"
echo "→ ingest [$mode] into [$target] @ $host"

if [[ "$target" == "prod" && -z "$skip_confirm" ]]; then
  read -r -p "Ingest into PRODUCTION ($host)? [y/N] " reply
  [[ "$reply" == "y" || "$reply" == "Y" ]] || { echo "aborted"; exit 1; }
fi

INGEST_DATABASE_URL="$url" cargo run --quiet -p server -- --ingest "$mode"
