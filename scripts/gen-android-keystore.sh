#!/usr/bin/env bash
#
# gen-android-keystore.sh — generate the one debug-grade Android signing keystore
# for Three Rings' CI, then print the exact `gh secret set` commands to register it.
#
# Why one persistent keystore: the artifacts workflow signs every CI APK with the
# SAME key so rolling `latest` builds upgrade IN PLACE on the phone. A fresh key
# per run would change the signature and force uninstall/reinstall each time.
# See specs/delivery-pipeline.md → "APK signing: one keystore, stored as a secret".
#
# This script:
#   1. Generates an RSA-2048, ~10000-day keystore with `keytool` OUTSIDE the repo
#      tree (default: ~/three-rings-release.keystore). It REFUSES to write inside
#      the repo, and it never overwrites an existing keystore (re-running is safe
#      and idempotent — pass --force to deliberately regenerate).
#   2. Prints the four `gh secret set` commands, using the exact secret names the
#      workflows expect: ANDROID_KEYSTORE, ANDROID_KEYSTORE_PASSWORD,
#      ANDROID_KEY_ALIAS, ANDROID_KEY_PASSWORD.
#
# It does NOT set the secrets for you and does NOT print your passwords. Copy the
# printed commands and run them yourself.
#
# ⚠️  NEVER commit the .keystore (or its base64) to the repo. The safeguard is
#     that it lives OUTSIDE the working tree by design (default: $HOME); this
#     script refuses to write it anywhere under the repo root.
#
# Usage:
#   scripts/gen-android-keystore.sh [-o PATH] [-a ALIAS] [-v DAYS] [--force] [-h]
#
#   -o, --out PATH     keystore output path (default: $HOME/three-rings-release.keystore)
#   -a, --alias ALIAS  key alias           (default: three-rings, or $ANDROID_KEY_ALIAS)
#   -v, --validity N   validity in days    (default: 10000)
#       --force        regenerate even if the keystore already exists (changes the
#                      signing key — breaks in-place APK upgrades; use only when you
#                      are also rotating the ANDROID_KEYSTORE secret)
#   -h, --help         show this help
#
# Passwords (never passed as CLI flags, to keep them out of shell history / `ps`):
#   ANDROID_KEYSTORE_PASSWORD   env var; prompted (hidden) if unset
#   ANDROID_KEY_PASSWORD        env var; defaults to the keystore password if unset
#
set -euo pipefail

# --- defaults -----------------------------------------------------------------
KEYSTORE="${HOME}/three-rings-release.keystore"
ALIAS="${ANDROID_KEY_ALIAS:-three-rings}"
VALIDITY=10000
FORCE=0
DNAME="CN=Three Rings Debug, OU=three-rings, O=threeringsdev, C=US"

usage() {
  cat <<'USAGE'
gen-android-keystore.sh — generate Three Rings' Android signing keystore (once)
and print the `gh secret set` commands to register it.

Usage:
  scripts/gen-android-keystore.sh [-o PATH] [-a ALIAS] [-v DAYS] [--force] [-h]

  -o, --out PATH     keystore output path (default: $HOME/three-rings-release.keystore)
  -a, --alias ALIAS  key alias           (default: three-rings, or $ANDROID_KEY_ALIAS)
  -v, --validity N   validity in days    (default: 10000)
      --force        regenerate even if the keystore already exists (changes the
                     signing key — breaks in-place APK upgrades; only when you are
                     also rotating the ANDROID_KEYSTORE secret)
  -h, --help         show this help

Passwords (env vars — never CLI flags, to stay out of shell history / `ps`):
  ANDROID_KEYSTORE_PASSWORD   prompted (hidden) if unset
  ANDROID_KEY_PASSWORD        defaults to the keystore password if unset

Generates an RSA-2048, ~10000-day keystore OUTSIDE the repo tree, refuses to
write inside the repo, and never overwrites an existing keystore without --force.
Secret names used: ANDROID_KEYSTORE, ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS,
ANDROID_KEY_PASSWORD. See specs/delivery-pipeline.md.
USAGE
}

# --- arg parsing --------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o|--out)      KEYSTORE="${2:?--out needs a PATH}"; shift 2 ;;
    -a|--alias)    ALIAS="${2:?--alias needs a value}"; shift 2 ;;
    -v|--validity) VALIDITY="${2:?--validity needs a number of days}"; shift 2 ;;
    --force)       FORCE=1; shift ;;
    -h|--help)     usage; exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; echo "run with --help for usage." >&2; exit 2 ;;
  esac
done

# --- resolve output to an absolute path & refuse to write inside the repo -----
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

out_dir="$(dirname "$KEYSTORE")"
if ! out_dir_abs="$(cd "$out_dir" 2>/dev/null && pwd)"; then
  echo "error: output directory does not exist: $out_dir" >&2
  exit 1
fi
KEYSTORE_ABS="$out_dir_abs/$(basename "$KEYSTORE")"

# Trailing slash on both sides so a sibling like <repo>-release.keystore is NOT
# mistaken for a path inside <repo>.
case "$KEYSTORE_ABS/" in
  "$REPO_ROOT"/*)
    echo "error: REFUSING to write the keystore inside the repository." >&2
    echo "       target:    $KEYSTORE_ABS" >&2
    echo "       repo root: $REPO_ROOT" >&2
    echo "       The signing keystore is a secret — keep it OUTSIDE the tree." >&2
    echo "       Re-run with -o pointing somewhere like \$HOME." >&2
    exit 1
    ;;
esac

# --- tooling check ------------------------------------------------------------
if ! command -v keytool >/dev/null 2>&1; then
  echo "error: 'keytool' not found on PATH." >&2
  echo "       It ships with the JDK (e.g. Android Studio's JBR, or a system JDK)." >&2
  echo "       Point JAVA_HOME at a JDK, or add its bin/ to PATH, then re-run." >&2
  exit 1
fi

# --- generate (idempotent) ----------------------------------------------------
if [[ -f "$KEYSTORE_ABS" && "$FORCE" -ne 1 ]]; then
  echo "keystore already exists — leaving it untouched (this is the safe path):"
  echo "  $KEYSTORE_ABS"
  echo
  echo "Re-running only reprints the secret commands below. To deliberately"
  echo "regenerate (new signing key — breaks in-place phone upgrades), pass --force."
  echo
else
  if [[ -f "$KEYSTORE_ABS" && "$FORCE" -eq 1 ]]; then
    echo "⚠️  --force: regenerating an EXISTING keystore. The new key will not match"
    echo "    APKs signed with the old one; installed builds must be uninstalled, and"
    echo "    you must update the ANDROID_KEYSTORE* secrets from this new file."
    rm -f "$KEYSTORE_ABS"
  fi

  # Resolve passwords without ever putting them on the command line.
  STOREPASS="${ANDROID_KEYSTORE_PASSWORD:-}"
  if [[ -z "$STOREPASS" ]]; then
    read -r -s -p "Keystore password (min 6 chars): " STOREPASS; echo
    read -r -s -p "Confirm keystore password:      " STOREPASS2; echo
    [[ "$STOREPASS" == "$STOREPASS2" ]] || { echo "error: passwords did not match." >&2; exit 1; }
  fi
  [[ ${#STOREPASS} -ge 6 ]] || { echo "error: keystore password must be at least 6 characters." >&2; exit 1; }
  KEYPASS="${ANDROID_KEY_PASSWORD:-$STOREPASS}"

  echo "Generating keystore:"
  echo "  path:     $KEYSTORE_ABS"
  echo "  alias:    $ALIAS"
  echo "  key:      RSA 2048, validity ${VALIDITY} days"
  echo

  # -storetype JKS is deliberate: on JDK 9+ keytool defaults to PKCS12, which
  # does NOT support a key password distinct from the store password (it silently
  # forces keyPassword == storePassword). We advertise a separate
  # ANDROID_KEY_PASSWORD, so force JKS to honor a distinct -keypass. Android's
  # release signing reads JKS keystores fine.
  keytool -genkeypair \
    -alias "$ALIAS" \
    -keyalg RSA -keysize 2048 -validity "$VALIDITY" \
    -keystore "$KEYSTORE_ABS" -storetype JKS \
    -storepass "$STOREPASS" -keypass "$KEYPASS" \
    -dname "$DNAME"

  chmod 600 "$KEYSTORE_ABS" || true
  echo
  echo "✅ Created $KEYSTORE_ABS"
  echo
fi

# --- print the exact `gh secret set` commands ---------------------------------
# Platform-appropriate base64 command (macOS BSD base64 vs GNU coreutils).
if [[ "$(uname)" == "Darwin" ]]; then
  B64_CLIP="base64 -i \"$KEYSTORE_ABS\" | pbcopy"
  B64_FILE="base64 -i \"$KEYSTORE_ABS\" -o \"$KEYSTORE_ABS.b64\""
else
  B64_CLIP="base64 -w0 \"$KEYSTORE_ABS\" | { command -v xclip >/dev/null && xclip -selection clipboard || cat; }"
  B64_FILE="base64 -w0 \"$KEYSTORE_ABS\" > \"$KEYSTORE_ABS.b64\""
fi

cat <<BANNER
────────────────────────────────────────────────────────────────────────────
Register the four GitHub secrets (repo: threeringsdev/three-rings)

Run these yourself — this script does NOT touch your secrets. Requires an
authenticated \`gh\` (\`gh auth status\`) with access to the repo. From ANY dir:

1) ANDROID_KEYSTORE  — the keystore, base64-encoded

   # Option A: base64 to a sidecar file, then pipe it into gh:
   $B64_FILE
   gh secret set ANDROID_KEYSTORE -R threeringsdev/three-rings < "$KEYSTORE_ABS.b64"
   rm "$KEYSTORE_ABS.b64"          # don't leave the encoded key lying around
BANNER

if [[ "$(uname)" == "Darwin" ]]; then
cat <<BANNER

   # Option B (macOS): copy to clipboard, then paste at gh's prompt:
   $B64_CLIP
   gh secret set ANDROID_KEYSTORE -R threeringsdev/three-rings   # paste, then Ctrl-D
BANNER
fi

cat <<BANNER

2) ANDROID_KEYSTORE_PASSWORD  — the keystore password (typed at the prompt)
   gh secret set ANDROID_KEYSTORE_PASSWORD -R threeringsdev/three-rings

3) ANDROID_KEY_ALIAS          — the key alias (not secret, but stored as one)
   printf '%s' '$ALIAS' | gh secret set ANDROID_KEY_ALIAS -R threeringsdev/three-rings

4) ANDROID_KEY_PASSWORD       — the key password (typed at the prompt)
   gh secret set ANDROID_KEY_PASSWORD -R threeringsdev/three-rings

Verify:
   gh secret list -R threeringsdev/three-rings

⚠️  Keep $KEYSTORE_ABS safe and OUT of the repo. If you lose it, in-place APK
    upgrades on installed phones break until you reinstall from the new key.
────────────────────────────────────────────────────────────────────────────
BANNER
