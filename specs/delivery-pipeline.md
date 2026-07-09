# Delivery pipeline

**Status:** draft
**Depends on:** [architecture-spike](architecture-spike.md), [dev-environment](dev-environment.md)

## Problem

Code and artifacts exist only on the laptop. Two needs, one pipeline:

1. **Autonomous agents.** An agent should be able to work on this repo from *any* host — docker on the laptop left unattended, a cloud devcontainer, a hosted agent session — with the repo itself as the complete contract (environment, commands, conventions, verification).
2. **Remote verification.** The human should be able to judge the result from anywhere — open a deployed web URL, install an updated APK, download a desktop build — without being at the laptop driving.

CI build steps are needed as the app evolves regardless; this phase builds them now, ahead of the data model and UI work.

## Scope

In: GitHub repo + secrets, CI validation workflow, Android/macOS artifact builds, a rolling release, web deployment to Render, the agent self-sufficiency contract (root `CLAUDE.md`, in-container auth docs), and one end-to-end proof of the loop.

Out (explicit non-goals for this phase):

- **Native builds reaching the database.** The released APK's `/cards` fails politely until [data-access-backends](data-access-backends.md) puts an API boundary in front of Neon (native → deployed API is exactly that later work). The web deploy is the DB-connected artifact for now.
- Store distribution (Play, App Store), macOS signing/notarization, iOS.
- Production-grade infra (custom domains, CDN, observability beyond platform defaults).

## Design

### Platform and repo

- **GitHub, private repo** (`three-rings` on the maintainer's personal account). Actions budget: 2,000 min/month free, linux 1×, macOS 10× — the CI design below exists to respect this.
- **Secrets:** `ANDROID_KEYSTORE` (+ passwords); `RENDER_DEPLOY_HOOK` only if Render's GitHub auto-deploy needs supplementing. **No `DATABASE_URL` in GitHub** — no CI job talks to Neon; the deployed app reads it from Render's own environment variables.
- **Merge policy — agents open PRs; auto-merge on green.** Branch protection on `main` requires the validate workflow; repo enables auto-merge. The validate suite is therefore the de facto reviewer — a wrong-but-green change ships itself to the rolling release and Render, and the human reviews *outcomes* (artifacts, deployed app) rather than diffs. Accepted deliberately for maximum autonomy; tighten to human-tap-merge later if trust breaks. This makes validate quality (clippy `-D warnings`, tests) load-bearing.

### CI: two tiers (budget policy)

The template's shipped `ci.yml` tests the *template* (it `cargo generate`s a copy); it is replaced, keeping its apt-dependency list and job shapes as reference.

- **Validate** — every push and PR, linux only: `cargo fmt --check`, clippy over the workspace with `-D warnings` (ssr and wasm targets), `cargo test`, `cargo leptos build --release` (exercises the Tailwind pipeline). CI installs the Tauri linux system deps, so `src-tauri` is linted here even though the devcontainer excludes it. Rust caching (e.g. Swatinem/rust-cache) throughout.
- **Artifacts** — merges to `main` plus `workflow_dispatch` only (agent iteration on branches stays cheap; merging is what mints checkable artifacts):
  - **Android APK**: x86_64 linux runner (the NDK works there — CI sidesteps the linux-arm64 NDK gap that pushed local Android builds host-side). Use **NDK r28+**, which defaults to 16 KB page alignment and closes that spike finding. `cargo tauri android build --apk --target aarch64`.
  - **macOS `.dmg`**: macos runner (10× minutes — this job is the reason artifacts don't run per-push). Unsigned/ad-hoc; opened via right-click → Open.

### APK signing: one keystore, stored as a secret

Generate a debug-grade keystore once; store it (base64) as a GitHub secret and sign CI release builds with it. If each run generated its own keystore, every install would demand uninstall/reinstall (signature mismatch); one persistent key makes rolling builds update in place on the phone. The local `~/.android/debug.keystore` stays for host builds — device installs from mixed sources still conflict; prefer CI builds on the phone.

### Rolling release

Artifact jobs upload the APK and `.dmg` to a single rolling **`latest` prerelease** (replaced each time). Stable URLs, reachable from a phone browser (GitHub login — private repo). Real version tags come later, when there is something to version.

Every publish stamps provenance: the release body carries the source commit SHA + timestamp, and the Android build sets `versionName` to include the short SHA (with a monotonically increasing `versionCode`, e.g. the workflow run number) — so "which build am I holding?" always has an answer, and in-place APK upgrades keep working.

### Web deploy: Render from a Dockerfile

- Multi-stage `Dockerfile` at the repo root: build stage runs `cargo leptos build --release`; runtime stage is a slim Debian image holding the `server` binary and `target/site`. The Dockerfile doubles as documentation of the runtime environment.
- Entrypoint maps Render's injected `PORT` to `LEPTOS_SITE_ADDR=0.0.0.0:$PORT`.
- Render's GitHub integration builds and deploys on push to `main` **on Render's infrastructure** — zero Actions minutes. `DATABASE_URL` lives in Render environment variables.
- **Neon branch split:** the Neon project's **main branch** backs the Render deployment; a child **`dev` branch** backs laptop/container development (`.devcontainer/.env` repointed to it). Agent experiments and dev migrations can't disturb the deployed app's data, and the dev/prod pattern is established before there is real data. Free tier covers both branches.
- Live proof: the Render URL serves `/` (SSR + hydration) and `/cards` (Neon rows).
- Render is the maintainer's platform choice (2026-07-09, superseding the design-time Railway draft). Nothing here depends on Render specifics beyond "PaaS that builds a Dockerfile from GitHub"; Fly.io is the named fallback.

### Agent self-sufficiency contract

The repo alone (plus documented secrets) must get an agent from clone to verified push, on any host:

- **Root `CLAUDE.md`**: build/test/verify commands per target, what-runs-where (container vs CI vs host), the TODO-queue conventions (selection, spec gating, definition of done), commit conventions.
- **In-container git/GitHub auth**: documented pattern for supplying credentials to a container (e.g. `GH_TOKEN` env var / mounted config; never baked into the image). `.devcontainer/README.md` gains this plus an updated what-runs-where table — with CI owning Android/macOS builds, host builds become optional local development, not the delivery path.
- The devcontainer image ([dev-environment](dev-environment.md)) stays the canonical environment; anything an agent needs that the image lacks is an image bug.

## Success criteria

- [ ] Push a branch → validation verdict (fmt, clippy, test, web build) visible on GitHub from any device
- [ ] Merge to `main` → rolling `latest` release carries a fresh APK (installs in place over the previous one) and macOS `.dmg`
- [ ] Merge to `main` → Render URL serves SSR `/` and `/cards` with Neon rows
- [ ] A fresh container started from the repo alone (plus documented secrets) lets an agent build, test, run the web target, and push a branch
- [ ] The loop proven once end-to-end: agent does a trivial task in a fresh container → pushes → merge → all three artifacts checked away from the laptop

## Open questions

- Actions minutes burn rate in practice — if the Android job proves slow/expensive even on merges, demote it to `workflow_dispatch`-only and revisit caching.
- Render free-tier fit for an always-on SSR process — free web services spin down after idle (cold start on next request); validate whether that's tolerable for "check the deployed app from anywhere", upgrade to the paid always-on instance if not. Fly.io is the fallback platform.
- *(resolved 2026-07-07)* ~~Where PR review fits once agents are pushing branches~~ — **PR + auto-merge on green**, for maximum autonomy; the validate workflow is the merge gate and the human reviews outcomes via artifacts. Recorded in Design; revisit toward human-tap-merge only if it misbehaves.
