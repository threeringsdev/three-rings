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
# target (server binary, SSR + hydration). The Android SDK/NDK + rust android
# targets are added in a later layer when we reach the architecture gate — see
# specs/architecture-spike.md. macOS desktop / iOS builds stay on the host.
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
