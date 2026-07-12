# syntax=docker/dockerfile:1
#
# Web target delivery image (specs/delivery-pipeline.md → "Web deploy: Render
# from a Dockerfile" + "Build performance (deploy time)"). Render builds this
# on push to `main`; it also doubles as the documented runtime environment for
# the SSR `server` binary.
#
# cargo-chef stage layout: dependencies compile in layers keyed only on the
# workspace manifests + Cargo.lock (distilled into recipe.json), so code-only
# pushes reuse them and recompile just the workspace crates (app/frontend/
# server). Payoff requires the builder's Docker layer cache to be warm — see
# the spec's caveat on Render cache persistence.

# ---- Toolchain base (chef) --------------------------------------------------
FROM rust:1-bookworm AS chef

# wasm target for the `frontend` hydrate crate.
RUN rustup target add wasm32-unknown-unknown

# mold: fast linker for the native server binary — the link is a serial tail
# of every rebuild, and matters more now that the server-release profile turns
# LTO off. Scoped to the native linux triples via $CARGO_HOME/config.toml so
# the wasm target is untouched. The same rustflags apply to `chef cook` and
# the real build below, keeping cargo fingerprints (and so the dep cache)
# identical between them.
RUN apt-get update \
    && apt-get install -y --no-install-recommends mold \
    && rm -rf /var/lib/apt/lists/* \
    && printf '[target.x86_64-unknown-linux-gnu]\nrustflags = ["-C", "link-arg=-fuse-ld=mold"]\n\n[target.aarch64-unknown-linux-gnu]\nrustflags = ["-C", "link-arg=-fuse-ld=mold"]\n' \
       >> "$CARGO_HOME/config.toml"

# cargo-leptos pinned to the version that builds this workspace on the host and
# in CI; cargo-chef pinned for reproducible dep layers. binstall fetches
# prebuilt binaries for the build arch (no source compile).
RUN curl -fsSL https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -y --locked cargo-leptos@0.3.7 cargo-chef@0.1.77

# The three external tools cargo-leptos otherwise fetches on every build
# (frontend pipeline: wasm-bindgen CLI, wasm-opt, tailwindcss). cargo-leptos
# prefers tools already on PATH (`which` lookup) over downloading, so pinned
# copies here remove the per-build GitHub "latest" API checks + downloads — a
# rate-limit/flakiness risk on shared PaaS builders, and a reproducibility win.
# WASM_BINDGEN_VERSION must stay equal to the wasm-bindgen crate pin in
# Cargo.toml (the CLI hard-errors on a schema mismatch, so drift fails loudly).
ARG TARGETARCH
ARG WASM_BINDGEN_VERSION=0.2.103
ARG BINARYEN_VERSION=version_123
ARG TAILWIND_VERSION=v4.2.1
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) wb_triple=x86_64-unknown-linux-musl; bo_arch=x86_64; tw_arch=x64 ;; \
      arm64) wb_triple=aarch64-unknown-linux-gnu; bo_arch=aarch64; tw_arch=arm64 ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/wasm-bindgen-${WASM_BINDGEN_VERSION}-${wb_triple}.tar.gz" \
      | tar -xz -C /usr/local/bin --strip-components=1 "wasm-bindgen-${WASM_BINDGEN_VERSION}-${wb_triple}/wasm-bindgen"; \
    curl -fsSL "https://github.com/WebAssembly/binaryen/releases/download/${BINARYEN_VERSION}/binaryen-${BINARYEN_VERSION}-${bo_arch}-linux.tar.gz" \
      | tar -xz -C /opt; \
    ln -s "/opt/binaryen-${BINARYEN_VERSION}/bin/wasm-opt" /usr/local/bin/wasm-opt; \
    curl -fsSL -o /usr/local/bin/tailwindcss \
      "https://github.com/tailwindlabs/tailwindcss/releases/download/${TAILWIND_VERSION}/tailwindcss-linux-${tw_arch}"; \
    chmod +x /usr/local/bin/tailwindcss; \
    wasm-bindgen --version; wasm-opt --version; tailwindcss --help >/dev/null

WORKDIR /app

# ---- Planner ----------------------------------------------------------------
# Distills the workspace manifests + lockfile + .cargo/config.toml into
# recipe.json. This COPY re-runs on any source change, but the recipe's content
# only changes when the manifests do — so the cook layers below stay cached for
# code-only pushes (BuildKit keys the builder's COPY on recipe.json content).
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---- Builder ----------------------------------------------------------------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Dependency layers. Each cook mirrors one of the two builds that
# `cargo leptos build --release` runs (invocations verified against the
# cargo-leptos 0.3.7 source) — a profile/target/feature mismatch wouldn't
# break the build, but the deps would silently recompile in the final step:
#   native bin: cargo build -p server --bin=server --no-default-features
#                 --profile=server-release            → target/server-release/
#   wasm lib:   cargo build -p frontend --lib --target-dir=target/front
#                 --target=wasm32-unknown-unknown --no-default-features
#                 --release                → target/front/wasm32-.../release/
# Scoping with -p also keeps src-tauri's dependency tree out of the cook —
# its Linux system libraries are deliberately absent from this image.
RUN cargo chef cook --recipe-path recipe.json \
      --package server --bin server --no-default-features --profile server-release
RUN cargo chef cook --recipe-path recipe.json \
      --package frontend --no-default-features --release \
      --target wasm32-unknown-unknown --target-dir target/front

# App code — from here down, only the workspace crates recompile.
COPY . .

# Produces target/server-release/server + target/site. The server binary lands
# in target/server-release/ (not target/release/) because cargo-leptos builds
# the bin with the `server-release` profile — see bin-profile-release in
# Cargo.toml and specs/delivery-pipeline.md → "Build performance".
RUN cargo leptos build --release

# ---- Runtime stage ----------------------------------------------------------
# Slim Debian carrying only the server binary + static site. Same Debian
# release (bookworm) as the build stage, so the binary's glibc matches.
FROM debian:bookworm-slim AS runtime

# ca-certificates: sqlx (rustls + ring) validates Neon's TLS cert against the
# system trust store. The server links nothing else dynamically beyond glibc.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/server-release/server /app/server
COPY --from=builder /app/target/site /app/site

# Leptos reads these at runtime — get_configuration(None) has no Cargo.toml to
# fall back on in the runtime image, so site-root/output-name come from the env.
ENV LEPTOS_OUTPUT_NAME=app \
    LEPTOS_SITE_ROOT=/app/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_ENV=PROD

# Documentation only; Render injects the real port via $PORT at runtime.
EXPOSE 3000

# Map Render's injected $PORT (fallback 3000 for a plain `docker run`) onto the
# address Leptos binds, then hand PID 1 to the server.
CMD ["/bin/sh", "-c", "LEPTOS_SITE_ADDR=0.0.0.0:${PORT:-3000} exec /app/server"]
