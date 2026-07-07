# Three Rings dev container

Development happens inside a Linux container so the Rust/Tauri toolchain stays off the host.
The image is **`dgoings/three-rings`**, defined by [`Dockerfile`](./Dockerfile) (layered on
`dgoings/magic-assistant-dev`).

## What's inside
- **Base** `dgoings/magic-assistant-dev` — Debian 13, Rust stable + `wasm32-unknown-unknown`,
  clippy/rustfmt, Node/Bun/pnpm, zsh, the `vscode` user.
- **Added here (v1, web-dev):** `cargo-leptos`, `cargo-generate`, `cargo-tauri` (v2), `leptosfmt`.
- **Not yet included:** Android SDK/NDK + rust android targets — added in a later image layer
  when we reach the architecture gate. macOS desktop + iOS builds run on the host (or CI).

## What runs where
| Work | Where |
|---|---|
| Scaffold, edit, `cargo` build/test/clippy/fmt, `cargo leptos` | container |
| Web target (`server` binary, SSR + hydration), Neon connectivity | container |
| Android APK **build** | container (after the Android layer is added) |
| Android **run** (emulator/device) | host |
| macOS desktop / iOS build + run | host (or CI) |

## Ports
`3000` — Leptos app (SSR) · `3001` — live-reload.

## Env
`docker` reads `.devcontainer/.env` (gitignored) via `--env-file`; it's auto-created from
[`.env.example`](./.env.example) on first container start. Put `DATABASE_URL` (Neon) here.

## Rebuild / push the image
```bash
.devcontainer/build.sh          # build + tag dgoings/three-rings:latest
.devcontainer/build.sh --push   # also push to Docker Hub (after `docker login`)
```
