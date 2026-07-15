# Three Rings dev container

Development happens inside a Linux container so the Rust/Tauri toolchain stays off the host.
The image is **`dgoings/three-rings`**, defined by [`Dockerfile`](./Dockerfile) (layered on
`dgoings/magic-assistant-dev`).

## What's inside
- **Base** `dgoings/magic-assistant-dev` — Debian 13, Rust stable + `wasm32-unknown-unknown`,
  clippy/rustfmt, Node/Bun/pnpm, zsh, the `vscode` user.
- **Added here (v1, web-dev):** `cargo-leptos`, `cargo-generate`, `cargo-tauri` (v2), `leptosfmt`.
- **Deliberately excluded:** Android SDK/NDK — Google ships no linux-arm64 NDK, so Android
  builds run on the host instead (Android Studio toolchain + brew rustup; see the TODO
  Decisions log). macOS desktop + iOS builds also run on the host (or CI).

## What runs where
CI owns the delivery artifacts (Android APK, macOS `.dmg`); host builds are now
**optional local development, not the delivery path** — see
[`specs/delivery-pipeline.md`](../specs/delivery-pipeline.md).

| Work | Where |
|---|---|
| Scaffold, edit, `cargo` build/test/clippy/fmt, `cargo leptos` | container |
| Web target (`server` binary, SSR + hydration), Neon connectivity | container |
| Validate suite (fmt, clippy ssr+wasm, test, `cargo leptos build --release`) | CI on every push/PR — the merge gate (reproducible here¹) |
| Android APK (delivery artifact, signed, rolling release) | CI (`main` merges + `workflow_dispatch`) |
| macOS `.dmg` (delivery artifact, rolling release) | CI (`workflow_dispatch` only) |
| Android APK **build** for local iteration (`cargo tauri android build --apk --target aarch64`) | host, optional (no linux-arm64 NDK exists) |
| macOS desktop / iOS **build** for local iteration | host, optional |
| Android / macOS / iOS **run** (emulator, device, `.app`) | host |

¹ In this web-dev container, add `--exclude three_rings` to the native clippy/test
commands — the container omits Tauri's Linux libs, so the `three_rings` (`src-tauri`)
shell is lint-checked in CI, not locally. Everything else in the Validate suite runs
in-container as written.

## Ports
`3000` — Leptos app (SSR) · `3001` — live-reload.

## Env
`docker` reads `.devcontainer/.env` (gitignored) via `--env-file`; put `DATABASE_URL`
(Neon) here. **VS Code only:** its devcontainer `initializeCommand` auto-creates the file
from [`.env.example`](./.env.example) on first container start. The raw `docker run` path
below has no such hook, so create it yourself first:
```bash
test -f .devcontainer/.env || cp .devcontainer/.env.example .devcontainer/.env
```

## Git / GitHub auth
The image ships **no credentials** — never bake a token into it (it is public on
Docker Hub) and never commit one. Supply auth to the container at runtime, two ways:

- **Token via env (simplest).** Add a line to `.devcontainer/.env` (gitignored,
  already `--env-file`'d in): `GH_TOKEN=ghp_xxx`. `gh` reads `GH_TOKEN`
  automatically; run `gh auth setup-git` once so `git push` works over HTTPS.
  ```bash
  gh auth setup-git      # use gh as git's credential helper for github.com
  gh auth status         # confirm the token is picked up
  ```
- **Mounted config (alternative).** Bind-mount the host's `gh`/git config
  read-only instead of putting a token in `.env`:
  ```jsonc
  // devcontainer.json → "mounts"
  "source=${localEnv:HOME}/.config/gh,target=/home/vscode/.config/gh,type=bind,readonly"
  ```
  or with the `docker run` one-liner below: `-v "$HOME/.config/gh:/home/vscode/.config/gh:ro"`.

A minimally-scoped token (`repo`, `workflow`) is enough to clone, push branches,
and open PRs. The token stays out of the image and out of git.

## Claude Code in the container

The image ships the `claude` CLI (native installer → `~/.local/bin`), and the
repo carries the config that recreates the host experience in any fresh
container — nothing to set up by hand except the two tokens below.

**Auth — two layers, use both:**

- **`CLAUDE_CODE_OAUTH_TOKEN` in `.devcontainer/.env` (stateless).** Run
  `claude setup-token` on the host (Pro/Max subscription; one-year,
  inference-only token) and paste the `sk-ant-oat01-...` value into `.env`.
  Every container — including the throwaway `docker run --rm` path — starts
  authenticated, terminal and VS Code extension alike. Treat like a password.
- **Named volume `three-rings-claude` → `/home/vscode/.claude` (stateful).**
  `devcontainer.json` mounts it, so anything written under `~/.claude` —
  `/login` credentials, installed plugins, history — survives rebuilds on this
  machine. One caveat: claude.ai **cloud connectors** (Gmail, Linear, …) are
  only guaranteed to load under an interactive `/login`, not the env token; if
  you want them in-container, `/login` once and the volume keeps it.

**Plugins** are pinned in the checked-in [`.claude/settings.json`](../.claude/settings.json)
(`enabledPlugins`, all from the built-in `claude-plugins-official` marketplace:
neon, rust-analyzer-lsp, typescript-lsp, frontend-design, superpowers,
context7, skill-creator). A fresh container auto-installs them on first run
after a one-time trust prompt; the volume then caches them. `rust-analyzer`
is already in the image for the LSP plugin, and `ENABLE_LSP_TOOL=1` is set in
the same settings file.

**MCP servers** live in the repo-root [`.mcp.json`](../.mcp.json) (project
scope, auto-approved via `enableAllProjectMcpServers` in settings.json):

- **render** — deploy status/logs over HTTP; needs `RENDER_API_KEY` in
  `.devcontainer/.env` (the checked-in config only references
  `${RENDER_API_KEY}`, never the key itself).
- **neon** — comes from the neon plugin above, not `.mcp.json`.
- Host-only, deliberately not ported: **pencil** (darwin-arm64 binary tied to
  the host VS Code app) and **auggie** (needs the Augment CLI + its own auth;
  install and `auggie login` in-container if you ever want it there).

Personal preferences (model, output style, vim mode, permission allowlist)
belong in `.claude/settings.local.json` — gitignored but inside the
bind-mounted workspace, so they follow you into every container automatically.

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
# --env-file needs .env to exist (no VS Code initializeCommand hook here):
test -f .devcontainer/.env || cp .devcontainer/.env.example .devcontainer/.env
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
