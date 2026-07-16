# CLAUDE.md — agent self-sufficiency contract

This repository is the **complete contract**. An agent with a clone plus the
documented secrets should get from **clone → build → test → verify → push** on
*any* host — the laptop's Docker container, a cloud devcontainer, or a hosted
agent session — with no other information required. If something an agent needs
isn't here (or in a file this links), that's a bug in the repo, not a gap for
the agent to improvise around.

Authoritative sources, in order:

- [`README.md`](README.md) — what the product is and the crate architecture.
- [`specs/README.md`](specs/README.md) — how to pick the next task ("Working the queue").
- [`specs/TODO.md`](specs/TODO.md) — the execution queue (single source of truth for *what next*).
- [`specs/`](specs/) — one spec per feature; the linked spec is required reading before its task.
- [`specs/delivery-pipeline.md`](specs/delivery-pipeline.md) — CI, artifacts, deploy, this contract.
- [`specs/dev-environment.md`](specs/dev-environment.md) + [`.devcontainer/README.md`](.devcontainer/README.md) — the environment and in-container auth.

## The environment

Everyday development runs inside the **`dgoings/three-rings` Docker
devcontainer** ([`.devcontainer/`](.devcontainer/)) — a Debian image carrying
Rust stable (+ `wasm32-unknown-unknown`), clippy/rustfmt, Node, and the
Leptos/Tauri CLIs (`cargo-leptos`, `cargo-generate`, `cargo-tauri` v2,
`leptosfmt`). This container is the **canonical environment**: anything an agent
needs that it lacks is an image bug, not something to install ad hoc.

The container is **web-dev only** — it deliberately omits Tauri's Linux system
libraries, so the Tauri shell crate (`three_rings`, in `src-tauri/`) does **not**
build there. That crate is lint-checked in CI (which installs those libs) and
built for real on hosts/CI, never in this container.

Environment variables come from `.devcontainer/.env` (gitignored; VS Code's
devcontainer `initializeCommand` auto-creates it from
[`.devcontainer/.env.example`](.devcontainer/.env.example) on first start — the raw
`docker run` path has no such hook, so copy it yourself there). It holds
`DATABASE_URL` for the Neon **`dev`** branch (the deployed app uses Neon's main
branch via Render). The DB probe is non-fatal: with no `DATABASE_URL` the web
target still serves and `/cards` fails politely.

## Crates

Cargo workspace, four members (see [`Cargo.toml`](Cargo.toml)):

| Crate | Package | Role |
|---|---|---|
| `app/` | `app` | the product — Leptos UI + server fns + Axum router + the data-access trait seam (shared lib) |
| `frontend/` | `frontend` | wasm hydrate lib (cargo-leptos **lib-package**, target `wasm32-unknown-unknown`) |
| `server/` | `server` | thin SSR web binary (cargo-leptos **bin-package**); builds `app` with the `hosted` backend |
| `shared/` | `shared` | cross-backend contract crate — DTOs + the one `ApiError` enum both backends map into (specs/data-access-backends.md); wasm-safe, no sqlx/axum |
| `src-tauri/` | `three_rings` | Tauri v2 desktop/mobile shell embedding the same router; builds `app` with the `native` backend |

**`app` feature split** (specs/data-access-backends.md): `ssr` is the
embedded-server substrate (router + auth core); a server build layers on exactly
one backend — `hosted` (sqlx against Neon, the authorization terminus) or
`native` (HTTPS client to the hosted API, no sqlx). `hydrate` is the wasm client.

cargo-leptos config lives in `Cargo.toml` (`[[workspace.metadata.leptos]]`):
name `app`, `site-root = target/site`, `site-pkg-dir = pkg`,
`tailwind-input-file = style/input.css`, `site-addr = 127.0.0.1:3000`,
`reload-port = 3001`.

## Commands per target

### Web (container — canonical)

```bash
cargo leptos watch          # dev: build + serve, hot reload on :3000 (live-reload :3001)
cargo leptos build --release  # production build (server binary + target/site, runs Tailwind)
cargo leptos serve          # serve a release build
cargo leptos watch --features component-bench   # dev incl. /dev/components — the UI component bench
```

The `component-bench` cargo feature gates the `/dev/components` bench page
(specs/ui-component-bench.md): on for dev (the flag above; `cargo tauri dev`'s
`beforeDevCommand` carries it already), off in every release/CI build.
**Vendoring a new UI component includes adding its bench section in the same
commit** — registry and convention in `app/src/bench/`.

SSR is confirmed when view-source shows rendered HTML (not an empty `<body>`);
hydration is confirmed when the counter buttons work (wasm took over the DOM).

### Native desktop (host — optional local dev only)

The delivery `.dmg` is built by **CI**, not here (see below). Local desktop
builds are for development, not the release path.

```bash
cargo tauri dev             # debug: drives `cargo leptos watch`, window points at devUrl
cargo tauri build           # release: embedded Axum on a dynamic 127.0.0.1 port, bundles the app
```

Note (from the spike): in **debug** builds the embedded server intentionally
does not start — `cargo tauri dev` iterates against the `cargo leptos watch`
server via `devUrl`. Only **release** builds exercise embedded Axum.

### Android (host — optional local dev only)

The delivery APK is built by **CI**. Google ships no linux-arm64 NDK, so this
does not work in the arm64 container; it needs the host Android Studio toolchain.

```bash
cargo tauri android dev                               # debug on emulator/device (devUrl fallback)
cargo tauri android build --apk --target aarch64      # release APK (embedded Axum SSR)
```

## Verify — reproduce the merge gate before pushing

The merge gate is [`.github/workflows/validate.yml`](.github/workflows/validate.yml)
(linux, every push and PR). Branch protection on `main` requires it and
auto-merge ships on green, so **this suite is the de facto reviewer** — a
wrong-but-green change ships itself. Reproduce it exactly:

```bash
mkdir -p target/site/pkg                                                # Tauri build script needs this dir
cargo fmt --all -- --check
cargo clippy --workspace --exclude frontend --all-targets -- -D warnings   # native workspace incl. src-tauri
cargo clippy -p frontend --target wasm32-unknown-unknown -- -D warnings     # wasm hydrate crate
cargo clippy -p app --features native --all-targets -- -D warnings          # native backend (masked by hosted in the workspace line)
cargo clippy -p app --features hosted,component-bench --all-targets -- -D warnings          # bench code, hosted server half
cargo clippy -p app --features hydrate,component-bench --target wasm32-unknown-unknown -- -D warnings  # bench code, wasm half
cargo test --workspace --exclude frontend
cargo leptos build --release                                            # full Tailwind + wasm pipeline
```

The `app` crate has a three-way feature split (specs/data-access-backends.md):
`ssr` is the embedded-server substrate; a complete server build must also enable
exactly one **backend** — `hosted` (web: sqlx against Neon; what `server` builds)
or `native` (Tauri shell: HTTPS client, no sqlx; what `src-tauri` builds). Two
gate lines exist because of it: the **native backend** line lints the
`native`-only code that the workspace line masks (feature unification there makes
`hosted` win the backend cfg), and the **component-bench** line uses `hosted`
(not bare `ssr`) since a real server always carries a backend.

**In the web-dev container**, add `--exclude three_rings` to the native workspace
clippy and test commands — the container omits Tauri's Linux libs, so the Tauri
shell is lint-checked in CI, not locally. The `-p app --features native` line
*does* run in-container (it's the `app` crate, no Tauri libs) and is how you lint
the native backend locally. Everything else runs in-container as written.

## What runs where

CI now owns the delivery artifacts; host builds are optional local development,
**not** the delivery path.

| Work | Where |
|---|---|
| Edit, `cargo` build/test/clippy/fmt, `cargo leptos` | **container** (canonical) |
| Web target run (SSR + hydration), Neon connectivity | **container** |
| Validate suite (fmt, clippy ssr+wasm, test, `leptos build`) | **CI** every push/PR — the merge gate (reproducible in container) |
| Android APK (delivery artifact, signed, rolling release) | **CI** — `workflow_dispatch` only (`android` input, default on) |
| macOS `.dmg` (delivery artifact, rolling release) | **CI** — `workflow_dispatch` only (`macos` input, default off) |
| Android / macOS desktop **build** (local iteration) | host (optional) |
| Android / macOS / iOS **run** (emulator, device, `.app`) | host |
| Web deploy (`/` + `/cards` catalog count via Neon) | **Render** — builds the root `Dockerfile` on push to `main` (zero Actions minutes) |

## Working the queue

Full rules in [`specs/README.md`](specs/README.md) ("Working the queue"); the
queue itself is [`specs/TODO.md`](specs/TODO.md). Summary:

- **Selection.** Phases run top→bottom, tasks within a phase top→bottom. The
  next available task is the **first `[ ]`** in the topmost phase containing one,
  skipping **blocked** tasks. A task is blocked if a listed prerequisite isn't
  `[x]`, or any spec in its `(specs: ...)` annotation is not `accepted` or
  `implemented` (spec status is read from the spec file header, never duplicated
  in TODO.md). Tasks without a `(specs: ...)` annotation are ungated.
- **`[~]` = in progress.** Don't start one unasked, and don't skip past its
  phase — work the next `[ ]` within it.
- **All `[ ]` blocked by a `draft` spec** → the real next action is spec review:
  report which specs block, offer to resolve their open questions, and wait for
  the human to flip status to `accepted`. **Never** set a spec to `accepted`
  yourself.
- **Before starting:** change the task's `[ ]` to `[~]` and commit that alone with
  message `start: <task summary>`.
- **Read the linked spec and its `Depends on:` specs** before writing any code.
- **Definition of done — ALL of:** work committed (conventional message); the
  task's `[~]` → `[x]` in the *same commit* as the final work; findings/decisions/
  surprises recorded in the linked spec (Findings / Open questions); newly
  discovered follow-up work added as new `[ ]` tasks in the right phase (never
  silently absorbed).
- **Ambiguous after reading the spec?** Stop and ask — do not guess. Record the
  question in the spec's Open questions first.

State legend: `[ ]` available · `[~]` in progress · `[x]` done.

## Commit convention

Conventional Commits — `type(scope): summary`, imperative and lowercase; scope
optional. Types in use in this repo: `docs`, `design`, `chore`, plus `feat` /
`fix` as code lands (e.g. `docs(specs): …`, `design(wireframes): …`,
`chore(design): …`). Two repo-specific rules:

- Flipping a task to in-progress is its own commit: `start: <task summary>`.
- The commit that finishes a task flips its `[~]` → `[x]` in TODO.md **in that
  same commit**.

PRs are **squash-merged**, so the PR *title* becomes the commit message on
`main` — format it as a conventional commit too. (History shows leaked GitHub
defaults like `Docs/spec review data layer (#18)`; don't add more.)

## CI, artifacts, secrets, deploy — where they live

- **Runners: Blacksmith** (drop-in for GitHub-hosted, chosen per job via
  `runs-on`): linux `blacksmith-4vcpu-ubuntu-2404`, macOS (Apple Silicon M4)
  `blacksmith-6vcpu-macos-latest`. Rust caching via `useblacksmith/rust-cache@v3`.
- **[`.github/workflows/validate.yml`](.github/workflows/validate.yml)** — the
  validate suite (above), every push + PR, linux only. The merge gate.
- **[`.github/workflows/artifacts.yml`](.github/workflows/artifacts.yml)** — the
  Android APK and the macOS `.dmg`, both **`workflow_dispatch` only**. One
  dispatch carries two boolean inputs selecting the targets: `android` (default
  on, cheap linux runner) and `macos` (default off — macOS minutes are ~20×
  costlier). Both publish to a single rolling **`latest`** prerelease at stable
  URLs, reachable from a phone (public repo).
- **Secrets** (GitHub repo settings, `threeringsdev/three-rings`) — the four
  Android signing secrets, generated and explained by
  [`scripts/gen-android-keystore.sh`](scripts/gen-android-keystore.sh):
  - `ANDROID_KEYSTORE` — base64 of the `.keystore`/`.jks`
  - `ANDROID_KEYSTORE_PASSWORD`
  - `ANDROID_KEY_ALIAS`
  - `ANDROID_KEY_PASSWORD`

  One persistent keystore signs every CI build so rolling APKs upgrade in place
  on the phone (a fresh key per run would force uninstall/reinstall).
  **No `DATABASE_URL` in GitHub** — no CI job talks to Neon; the deployed app
  reads it from Render's own environment variables.
- **Web deploy: Render**, building the root `Dockerfile` on push to `main` on
  Render's infrastructure (zero Actions minutes). `DATABASE_URL` lives in Render
  env vars and points at Neon's **main** branch; the container's `.env` points at
  the Neon **`dev`** branch.

## Git / GitHub auth in the container

Never bake credentials into the image (it is public on Docker Hub) and never
commit them. Supply a token at runtime via a `GH_TOKEN` env var (or a mounted,
read-only host git/gh config). Full pattern in
[`.devcontainer/README.md`](.devcontainer/README.md) → "Git / GitHub auth".
Once `GH_TOKEN` is set, `gh auth setup-git` makes `git push` work over HTTPS.
