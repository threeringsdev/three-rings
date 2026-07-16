# syntax=docker/dockerfile:1
#
# dgoings/three-rings — dev image for the Three Rings project.
# Layers the Rust/Leptos/Tauri toolchain onto the shared web-dev base.
#
# Base already provides: Debian 13, Rust stable + wasm32-unknown-unknown target,
# clippy/rustfmt, Node/Bun/pnpm, zsh, and the `vscode` user
# (CARGO_HOME=/home/vscode/.cargo, RUSTUP_HOME=/home/vscode/.rustup).
#
# Rebuild / push:  .devcontainer/build.sh  [--push]
#
# SCOPE (v1 = web-dev): the CLIs installed here unblock scaffolding and the web
# target (server binary, SSR + hydration). Android builds happen on the HOST —
# Google ships no linux-arm64 NDK, so an Android layer here is a dead end (see
# specs/TODO.md Decisions log). macOS desktop / iOS builds also stay on the host.
FROM dgoings/magic-assistant-dev:latest

USER vscode
ENV CARGO_HOME=/home/vscode/.cargo \
    RUSTUP_HOME=/home/vscode/.rustup \
    PATH=/home/vscode/.cargo/bin:${PATH}

# cargo-binstall fetches prebuilt binaries (only falling back to a source compile
# when a target has none), keeping image builds fast. tauri-cli resolves to v2,
# matching the Tauri v2 scaffold.
# TODO(reproducibility): pin exact versions once the scaffold is confirmed to build.
RUN curl -L --proto '=https' --tlsv1.2 -sSf \
      https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
 && cargo binstall -y \
      cargo-leptos \
      cargo-generate \
      tauri-cli \
      leptosfmt \
 && rm -rf "${CARGO_HOME}/registry/cache" "${CARGO_HOME}/git"

# Claude Code CLI (native installer → ~/.local/bin). Auth comes from the
# runtime env (CLAUDE_CODE_OAUTH_TOKEN in .devcontainer/.env) or a persisted
# /login — never baked in (image is public on Docker Hub). Project plugins/MCP
# servers come from the repo's .claude/settings.json + .mcp.json at run time.
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH=/home/vscode/.local/bin:${PATH}

# GitHub CLI (gh) — used for git-over-HTTPS auth (gh as the credential helper)
# and for opening PRs from the container. Installed from GitHub's apt repo; needs
# root, so switch user around it. Auth is runtime-only (GH_TOKEN in
# .devcontainer/.env, never baked in). The devcontainer `postStartCommand` runs
# .devcontainer/setup-git-auth.sh on each start to point git at this gh and strip
# any host-copied credential helper (see that script for the why).
USER root
RUN mkdir -p -m 755 /etc/apt/keyrings \
 && curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
      -o /etc/apt/keyrings/githubcli-archive-keyring.gpg \
 && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
 && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
      > /etc/apt/sources.list.d/github-cli.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends gh \
 && rm -rf /var/lib/apt/lists/*
USER vscode
