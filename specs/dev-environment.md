# Dev environment

**Status:** draft
**Depends on:** —

## Problem

The stack is Rust-heavy (Leptos, Axum, Tauri, cargo-leptos) and the maintainer does not want that toolchain installed on the host. Development, builds, and the web target should run in a reproducible container instead. But the product also targets platforms a Linux container cannot fully serve: macOS desktop / iOS builds require macOS + Xcode, and Android apps must *run* on an emulator or device. The environment must draw a clear line between what runs in the container and what stays on the host.

## Scope

In: the devcontainer configuration (`.devcontainer/`), the definition and build of the project dev image (`dgoings/three-rings`), the host/container responsibility split, and local build/caching conventions. Out: CI (Phase 2), the contents of the Android SDK/NDK image layer (added when the architecture spike reaches the Android gate), and production release/signing/notarization.

## Design

Development runs inside a Docker devcontainer. The image `dgoings/three-rings` ([`.devcontainer/Dockerfile`](../.devcontainer/Dockerfile)) is layered on the maintainer's existing `dgoings/magic-assistant-dev` base (Debian 13, Rust stable + `wasm32-unknown-unknown`, clippy/rustfmt, Node/Bun/pnpm, zsh, `vscode` user) and adds the Leptos/Tauri CLIs: `cargo-leptos`, `cargo-generate`, `cargo-tauri` (v2), `leptosfmt`. `.devcontainer/build.sh` (re)builds and optionally pushes the image; [`.devcontainer/README.md`](../.devcontainer/README.md) is the operational reference.

`devcontainer.json` pins the image, forwards ports `3000` (Leptos SSR app) and `3001` (live-reload), mounts a project `.zshrc`, and reads `.devcontainer/.env` (gitignored; holds `DATABASE_URL` for Neon) via `--env-file`. An `initializeCommand` bootstraps `.env` from `.env.example`.

**What runs where:**

| Work | Where |
|---|---|
| Scaffold, edit, `cargo` build/test/clippy/fmt, `cargo leptos` | container |
| Web target (`server` binary, SSR + hydration), Neon connectivity | container |
| Android APK **build** | container (once the Android SDK/NDK layer is added) |
| Android **run** (emulator/device, adb) | host |
| macOS desktop / iOS build + run | host (or CI) |

Rationale: a Linux container cannot build or run a macOS `.app` (WebKit/AppKit + the macOS SDK + Tauri's macOS bundling are macOS-only), and running a Tauri app means executing it on its target OS — so macOS desktop and iOS are host- or CI-side. Android is cross-buildable on Linux, so the APK build is containerized while the emulator/device (the *run*) stays on the host. Consequence recorded in the architecture-spike Findings: macOS desktop (Phase 1) is deferred behind the Android gate.

## Open questions

- *(resolved during execution — architecture spike, task 4)* Contents of the Android toolchain image layer (JDK version; Android SDK/NDK/build-tools versions; rust android targets) and how the container-built APK reaches the host emulator (adb over TCP vs. `adb install` from the host).
- *(resolved during execution — before the Phase 1 macOS-desktop task)* macOS desktop / iOS build path: CI (macOS runner) vs. a minimal host Rust install.
- Reproducibility: the image installs the CLIs at their latest versions (`.devcontainer/Dockerfile` carries a TODO). Pin exact versions once the spike confirms the toolchain is stable.
- Multi-arch: the image is built for `aarch64` (Apple Silicon). If CI or an x86 host needs it, publish a multi-arch image.
