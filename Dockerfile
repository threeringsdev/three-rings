# syntax=docker/dockerfile:1
#
# Web target delivery image (specs/delivery-pipeline.md → "Web deploy: Render
# from a Dockerfile"). Render builds this on push to `main`; it also doubles as
# the documented runtime environment for the SSR `server` binary.

# ---- Build stage ----------------------------------------------------------
# Runs the same `cargo leptos build --release` the merge gate runs: compiles
# the SSR `server` binary and the hydration bundle (wasm/JS/CSS + Tailwind)
# into target/site.
FROM rust:1-bookworm AS build

# wasm target for the `frontend` hydrate crate.
RUN rustup target add wasm32-unknown-unknown

# cargo-leptos, pinned to the version that builds this workspace on the host and
# in CI. binstall fetches a prebuilt binary for the build arch (no source compile).
RUN curl -fsSL https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -y --locked cargo-leptos@0.3.7

WORKDIR /app
COPY . .

# Produces target/server-release/server + target/site. The server binary lands in
# target/server-release/ (not target/release/) because cargo-leptos builds the bin
# with the `server-release` profile — see bin-profile-release in Cargo.toml and
# specs/delivery-pipeline.md → "Build performance". cargo-leptos downloads the
# standalone tailwindcss binary for the build arch on first run (needs network).
RUN cargo leptos build --release

# ---- Runtime stage --------------------------------------------------------
# Slim Debian carrying only the server binary + static site. Same Debian release
# (bookworm) as the build stage, so the binary's glibc matches.
FROM debian:bookworm-slim AS runtime

# ca-certificates: sqlx (rustls + ring) validates Neon's TLS cert against the
# system trust store. The server links nothing else dynamically beyond glibc.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /app/target/server-release/server /app/server
COPY --from=build /app/target/site /app/site

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
