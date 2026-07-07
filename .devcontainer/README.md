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

## Run the web app
From inside the container (VS Code → "Dev Containers: Reopen in Container", or `docker exec`):
```bash
cargo leptos watch      # build + serve with hot reload on :3000
```
Open http://localhost:3000 (VS Code forwards it automatically). **SSR** = view-source shows
rendered HTML, not an empty `<body>`; **hydration** = the counter's buttons actually work (the
wasm has taken over the server-rendered DOM).

Without VS Code, from the repo root on the host:
```bash
docker run --rm -it -v "$PWD":/workspaces/three-rings -w /workspaces/three-rings \
  -p 3000:3000 -p 3001:3001 --env-file .devcontainer/.env \
  -u vscode dgoings/three-rings:latest bash -lc 'cargo leptos watch'
```
(First build is cold — several minutes. The VS Code container persists build caches between runs; the `--rm` one-liner does not.)

## Rebuild / push the image
```bash
.devcontainer/build.sh          # build + tag dgoings/three-rings:latest
.devcontainer/build.sh --push   # also push to Docker Hub (after `docker login`)
```
