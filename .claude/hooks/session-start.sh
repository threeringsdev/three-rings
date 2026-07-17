#!/bin/bash
# SessionStart hook — provisions the Three Rings web-dev toolchain for
# Claude Code on the web.
#
# This reproduces what the `dgoings/three-rings` devcontainer image layers on
# top of its base (see .devcontainer/Dockerfile): the wasm target plus the
# Leptos/Tauri cargo CLIs. The cloud image already ships Rust stable, clippy,
# rustfmt, and Node/pnpm/bun, so this only fills the gaps.
#
# NOTE ON INSTALL METHOD: the Dockerfile uses cargo-binstall to pull prebuilt
# binaries from GitHub releases. In the cloud environment the egress proxy
# blocks GitHub release downloads (403), so this installs from crates.io source
# via `cargo install --locked` instead — crates.io is on the proxy allowlist.
# Same resulting CLIs, just compiled rather than downloaded.
#
# Scope matches the container: web-dev only. The Tauri shell crate (three_rings)
# is lint-checked in CI, not built here, so no Tauri Linux system libs are
# installed. See CLAUDE.md "Verify" and specs/dev-environment.md.
#
# Idempotent and non-interactive: safe to re-run; every step is a no-op once
# satisfied, so resumed/cached sessions cost nothing.
set -euo pipefail

# Only provision the remote (Claude Code on the web) environment. Local runs
# already use the devcontainer image and should be left untouched.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export PATH="$CARGO_HOME/bin:$HOME/.local/bin:$PATH"

echo "[session-start] provisioning Three Rings web-dev toolchain..."

# 1. wasm target — the frontend/hydrate crate and `cargo leptos build` compile
#    for wasm32-unknown-unknown.
if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
  echo "[session-start] adding wasm32-unknown-unknown target"
  rustup target add wasm32-unknown-unknown
else
  echo "[session-start] wasm32-unknown-unknown already installed"
fi

# 2. Leptos/Tauri cargo CLIs — the same set the image installs, compiled from
#    crates.io (see NOTE above). tauri-cli is pinned to the v2 series to match
#    the Tauri v2 scaffold; the rest track the latest release like the image.
#    Only the missing ones are installed, so re-runs are no-ops.
install_cli() {
  # $1 = binary name to probe, $2 = crate name, $3 = optional --version arg
  local bin="$1" crate="$2" ver="${3:-}"
  if command -v "$bin" >/dev/null 2>&1; then
    echo "[session-start] $bin already installed"
    return 0
  fi
  echo "[session-start] installing $crate ${ver:+($ver) }from crates.io (compiles from source)"
  if [ -n "$ver" ]; then
    cargo install --locked "$crate" --version "$ver"
  else
    cargo install --locked "$crate"
  fi
}

install_cli cargo-leptos   cargo-leptos
install_cli leptosfmt      leptosfmt
install_cli cargo-generate cargo-generate
install_cli cargo-tauri    tauri-cli   "^2"

# The Tauri build script expects this dir to exist even when only the web target
# is built (see CLAUDE.md "Verify").
mkdir -p "${CLAUDE_PROJECT_DIR:-.}/target/site/pkg"

echo "[session-start] toolchain ready."
