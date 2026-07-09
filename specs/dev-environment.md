# Dev environment

**Status:** implemented
**Depends on:** —

## Problem

The stack is Rust-heavy (Leptos, Axum, Tauri, cargo-leptos) and the maintainer does not want that toolchain installed on the host. Development, builds, and the web target should run in a reproducible container instead. But the product also targets platforms a Linux container cannot fully serve: macOS desktop / iOS builds require macOS + Xcode, and Android apps must *run* on an emulator or device. The environment must draw a clear line between what runs in the container and what stays on the host.

## Scope

In: the devcontainer configuration (`.devcontainer/`), the definition and build of the project dev image (`dgoings/three-rings`), the host/container responsibility split, and local build/caching conventions. Out: CI and remote artifact delivery ([delivery-pipeline](delivery-pipeline.md)), and production release/signing/notarization. *(An Android SDK/NDK image layer was originally in scope here; it was abandoned during the spike — see the resolved questions below.)*

## Design

Development runs inside a Docker devcontainer. The image `dgoings/three-rings` ([`.devcontainer/Dockerfile`](../.devcontainer/Dockerfile)) is layered on the maintainer's existing `dgoings/magic-assistant-dev` base (Debian 13, Rust stable + `wasm32-unknown-unknown`, clippy/rustfmt, Node/Bun/pnpm, zsh, `vscode` user) and adds the Leptos/Tauri CLIs: `cargo-leptos`, `cargo-generate`, `cargo-tauri` (v2), `leptosfmt`. `.devcontainer/build.sh` (re)builds and optionally pushes the image; [`.devcontainer/README.md`](../.devcontainer/README.md) is the operational reference.

`devcontainer.json` pins the image, forwards ports `3000` (Leptos SSR app) and `3001` (live-reload), mounts a project `.zshrc`, and reads `.devcontainer/.env` (gitignored; holds `DATABASE_URL` for Neon) via `--env-file`. An `initializeCommand` bootstraps `.env` from `.env.example`.

**What runs where:**

| Work | Where |
|---|---|
| Scaffold, edit, `cargo` build/test/clippy/fmt, `cargo leptos` | container |
| Web target (`server` binary, SSR + hydration), Neon connectivity | container |
| Android APK **build** | host locally (Android Studio SDK/NDK + brew rustup); CI for delivery artifacts ([delivery-pipeline](delivery-pipeline.md)) |
| Android **run** (emulator/device, adb) | host |
| macOS desktop build + run | host locally (`cargo tauri build`, Xcode); CI `.dmg` for delivery ([delivery-pipeline](delivery-pipeline.md)) |
| iOS build + run (future) | host or CI macOS runner |

Rationale: a Linux container cannot build or run a macOS `.app` (WebKit/AppKit + the macOS SDK + Tauri's macOS bundling are macOS-only), and running a Tauri app means executing it on its target OS — so macOS desktop and iOS are host- or CI-side. Android *is* cross-buildable on Linux — but only on x86_64: Google ships no linux-arm64 NDK, which killed the planned container layer on this Apple Silicon host (an amd64-under-Rosetta image variant was viable but passed over). x86_64 CI runners have no such gap, so delivery APKs come from CI.

**Host toolchain relaxation (2026-07-07):** the original "toolchain-free host" goal is relaxed to *"Rust via brew rustup, no ad-hoc curl installs"* — the Android gate needed a host build, and macOS desktop rides the same install. Host carries: brew `rustup` (keg-only) with `aarch64-linux-android`/`wasm32-unknown-unknown` targets, binstalled `cargo-leptos`/`tauri-cli`, plus Android Studio (SDK/NDK/JBR) and Xcode, which were host requirements regardless. The container remains the canonical environment for everyday development and for agents ([delivery-pipeline](delivery-pipeline.md) makes it the self-sufficiency contract).

## Open questions

- *(resolved 2026-07-07, architecture spike task 4)* ~~Contents of the Android toolchain image layer~~ — **there is no image layer.** Google ships the NDK for linux x86_64 only (arm64 Linux hosts are explicitly off their roadmap), so an Android layer on this arm64 image cannot work with official tooling. The Android build moved to the host: Android Studio's SDK (NDK r27.1, JBR as JAVA_HOME, compileSdk 36/minSdk 24) + brew rustup + binstalled tauri-cli. The APK reaches the emulator via plain `adb install` from the host. Details in architecture-spike.md Findings; delivery builds move to x86_64 CI runners (NDK r28+) in [delivery-pipeline](delivery-pipeline.md).
- *(resolved 2026-07-07, before the Phase 1 macOS-desktop task)* macOS desktop build path: **host build** (`cargo tauri build`) — the Rust toolchain was already on the host from the Android decision, and Xcode supplies the platform bits. CI macOS runners produce the delivery `.dmg` in [delivery-pipeline](delivery-pipeline.md); iOS remains future work on the same host/CI split.
- Reproducibility: the image installs the CLIs at their latest versions (`.devcontainer/Dockerfile` carries a TODO). The spike is done and the toolchain confirmed working (cargo-leptos 0.3.x, tauri-cli 2.11.x, Leptos 0.8.2/Tauri 2.11 in `Cargo.lock`) — pinning is now actionable at the next image rebuild.
- Multi-arch: the project image is `aarch64`-only (the `dgoings/magic-assistant-dev` base is already multi-arch on Docker Hub). CI does not consume the image (GitHub runners bring their own toolchains), but x86_64 *agent hosts* (e.g. Codespaces) would need an amd64 variant — becomes relevant with delivery-pipeline's any-host self-sufficiency goal.
