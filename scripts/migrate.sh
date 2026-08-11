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

# The sqlx::migrate!("../migrations") macro (app/src/db.rs) is embedded at
# compile time; app/build.rs gives cargo a real dependency edge on the
# migrations/ directory (`cargo:rerun-if-changed`) where previously there was
# none at all, so a newly added or edited .sql file reliably triggers a
# re-embed on a normal build (see specs/phase-6-probes/P6-059.md — the
# previous defense here was an mtime `touch` on app/src/db.rs, which was a bet
# on ordering, not a dependency cargo itself understood). `--migrate` also now
# verifies its result against the database's applied-migrations table rather
# than trusting a bare `Ok(())`, catching DB-vs-binary drift.
#
# Neither of those catches the binary itself being stale — a build that,
# for whatever reason, never re-embedded a newly added .sql file: DB and
# binary would still agree with each other while disagreeing with what's
# actually in migrations/ on disk, and --migrate would honestly report "up to
# date" from inside that stale binary's own — wrong — idea of what's embedded.
# So: independently count migrations on disk (distinct version prefixes, so a
# future reversible pair's .up/.down files still count once) and compare it
# to the `embedded=N` the binary reports having compiled in.
migration_count() {
  find "$root/migrations" -maxdepth 1 -type f -name '*.sql' -exec basename {} \; \
    | sed -E 's/^([0-9]+)[^0-9].*/\1/' \
    | sort -u | wc -l | tr -d ' '
}
disk_count="$(migration_count)"

# Capture output (so it can be both shown and parsed) without losing --migrate's
# own exit code to `set -e` swallowing a failed command substitution silently.
set +e
output="$(MIGRATION_DATABASE_URL="$url" cargo run --quiet -p server -- --migrate 2>&1)"
rc=$?
set -e
printf '%s\n' "$output"
[[ $rc -ne 0 ]] && exit "$rc"

embedded_count="$(printf '%s\n' "$output" | grep -oE 'embedded=[0-9]+' | tail -n1 | cut -d= -f2)"
if [[ -z "$embedded_count" ]]; then
  echo "error: could not find an 'embedded=N' line in --migrate's output — can't verify the binary against disk" >&2
  exit 1
fi
if [[ "$embedded_count" != "$disk_count" ]]; then
  echo "STALE EMBED: disk has $disk_count migrations, binary embedded $embedded_count — rebuild happened without the new file(s); do not trust this run" >&2
  exit 1
fi
