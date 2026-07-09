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

- **GitHub, public repo** (`threeringsdev/three-rings`) — owned by a free GitHub **organization**, not the personal account: Blacksmith (below) installs only on organizations. Public rather than private (2026-07-09): free-plan orgs can't enforce branch protection or auto-merge on private repos, and the merge gate below depends on both — public unlocks them at zero cost. (Public also makes GitHub-hosted minutes free; Blacksmith stays the runner choice for speed and its own free tier.)
- **CI runners: [Blacksmith](https://www.blacksmith.sh/)** — drop-in replacement for GitHub-hosted runners; each job opts in via its `runs-on` label and nothing else changes. Budget: 3,000 free Blacksmith minutes/month (denominated in 2 vCPU linux-x64 minutes), then pay-as-you-go (linux x64 $0.004/min, macOS M4 $0.08/min); GitHub separately charges a $0.002/min control-plane fee on all Actions minutes (since 2026-03) regardless of runner. The two-tier CI design below exists to respect this — macOS still costs ~20× the linux rate. Maintainer choice (2026-07-09, superseding the GitHub-hosted draft); any job reverts to a GitHub-hosted label independently if Blacksmith misbehaves.
- **Secrets:** `ANDROID_KEYSTORE` (+ passwords); `RENDER_DEPLOY_HOOK` only if Render's GitHub auto-deploy needs supplementing. **No `DATABASE_URL` in GitHub** — no CI job talks to Neon; the deployed app reads it from Render's own environment variables.
- **Merge policy — agents open PRs; auto-merge on green.** Branch protection on `main` requires the validate workflow; repo enables auto-merge. The validate suite is therefore the de facto reviewer — a wrong-but-green change ships itself to the rolling release and Render, and the human reviews *outcomes* (artifacts, deployed app) rather than diffs. Accepted deliberately for maximum autonomy; tighten to human-tap-merge later if trust breaks. This makes validate quality (clippy `-D warnings`, tests) load-bearing.

### CI: two tiers (budget policy)

The template's shipped `ci.yml` tests the *template* (it `cargo generate`s a copy); it is replaced, keeping its apt-dependency list and job shapes as reference.

- **Validate** — every push and PR, linux only (`blacksmith-4vcpu-ubuntu-2404`): `cargo fmt --check`, clippy over the workspace with `-D warnings` (ssr and wasm targets), `cargo test`, `cargo leptos build --release` (exercises the Tailwind pipeline). CI installs the Tauri linux system deps, so `src-tauri` is linted here even though the devcontainer excludes it. Rust caching throughout via `useblacksmith/rust-cache` (Blacksmith's sticky-disk fork of Swatinem/rust-cache).
- **Artifacts** — never per-push on branches (agent iteration stays cheap; merging is what mints checkable artifacts). Cadence splits by job *cost*, not by target — a UI change lands in the shared `app` crate and alters every artifact, so per-target path filtering would rarely skip a build honestly:
  - **Android APK** — merges to `main` plus `workflow_dispatch`, with `paths-ignore` skipping docs-only pushes (`specs/**`, `**.md`). x86_64 linux runner (`blacksmith-4vcpu-ubuntu-2404` — must stay x64: Blacksmith's arm64 shapes would reintroduce the linux-arm64 NDK gap that pushed local Android builds host-side). Use **NDK r28+**, which defaults to 16 KB page alignment and closes that spike finding. `cargo tauri android build --apk --target aarch64`. Roughly 30 free-tier minutes per merge — cheap enough to keep the phone-checkable rolling release always fresh.
  - **macOS `.dmg`** — `workflow_dispatch` only, from the start. Blacksmith macOS runner (`blacksmith-6vcpu-macos-latest`, Apple Silicon M4) at $0.08/min — ~20× the free-tier basis rate — would exhaust the tier in ~15 merges if it ran per-merge. The gate costs little: the `.dmg`'s consumer is a human at a Mac (who can click "Run workflow" right there), and pre-merge desktop checks happen as local host builds anyway. Unsigned/ad-hoc; opened via right-click → Open.

### APK signing: one keystore, stored as a secret

Generate a debug-grade keystore once; store it (base64) as a GitHub secret and sign CI release builds with it. If each run generated its own keystore, every install would demand uninstall/reinstall (signature mismatch); one persistent key makes rolling builds update in place on the phone. The local `~/.android/debug.keystore` stays for host builds — device installs from mixed sources still conflict; prefer CI builds on the phone.

### Rolling release

Artifact jobs upload the APK and `.dmg` to a single rolling **`latest` prerelease** — each job replaces its own asset, and since the `.dmg` builds on dispatch cadence the two assets may come from different commits. Stable URLs, reachable from a phone browser (public repo — no login needed). Real version tags come later, when there is something to version.

Every publish stamps provenance: the release body carries the source commit SHA + timestamp per asset, and the Android build sets `versionName` to include the short SHA (with a monotonically increasing `versionCode`, e.g. the workflow run number) — so "which build am I holding?" always has an answer, and in-place APK upgrades keep working.

### Web deploy: Render from a Dockerfile

- Multi-stage `Dockerfile` at the repo root: build stage runs `cargo leptos build --release`; runtime stage is a slim Debian image holding the `server` binary and `target/site`. The Dockerfile doubles as documentation of the runtime environment.
- Entrypoint maps Render's injected `PORT` to `LEPTOS_SITE_ADDR=0.0.0.0:$PORT`.
- Render's GitHub integration builds and deploys on push to `main` **on Render's infrastructure** — zero Actions minutes. `DATABASE_URL` lives in Render environment variables.
- **Neon branch split:** the Neon project's **main branch** backs the Render deployment; a child **`dev` branch** backs laptop/container development (`.devcontainer/.env` repointed to it). Agent experiments and dev migrations can't disturb the deployed app's data, and the dev/prod pattern is established before there is real data. Free tier covers both branches.
- Live proof: the Render URL serves `/` (SSR + hydration) and `/cards` (Neon rows).
- Render is the maintainer's platform choice (2026-07-09, superseding the design-time Railway draft). Nothing here depends on Render specifics beyond "PaaS that builds a Dockerfile from GitHub"; Fly.io is the named fallback.
- **Free tier, spin-down accepted** (2026-07-09): idle services cold-start on the next request, which is fine for remote checking. Move to the paid always-on instance only if it grates in practice.

### Agent self-sufficiency contract

The repo alone (plus documented secrets) must get an agent from clone to verified push, on any host:

- **Root `CLAUDE.md`**: build/test/verify commands per target, what-runs-where (container vs CI vs host), the TODO-queue conventions (selection, spec gating, definition of done), commit conventions.
- **In-container git/GitHub auth**: documented pattern for supplying credentials to a container (e.g. `GH_TOKEN` env var / mounted config; never baked into the image). `.devcontainer/README.md` gains this plus an updated what-runs-where table — with CI owning Android/macOS builds, host builds become optional local development, not the delivery path.
- The devcontainer image ([dev-environment](dev-environment.md)) stays the canonical environment; anything an agent needs that the image lacks is an image bug.

## Success criteria

- [ ] Push a branch → validation verdict (fmt, clippy, test, web build) visible on GitHub from any device
- [ ] Merge to `main` (non-docs) → rolling `latest` release carries a fresh APK (installs in place over the previous one); docs-only merges skip the build
- [ ] `workflow_dispatch` of the macOS job → fresh `.dmg` on the same rolling release
- [ ] Merge to `main` → Render URL serves SSR `/` and `/cards` with Neon rows
- [ ] A fresh container started from the repo alone (plus documented secrets) lets an agent build, test, run the web target, and push a branch
- [ ] The loop proven once end-to-end: agent does a trivial task in a fresh container → pushes → merge → web URL + APK checked away from the laptop, `.dmg` dispatched and checked

## Open questions

- *(resolved 2026-07-09)* ~~Blacksmith free-tier burn rate in practice~~ — defused in Design: the macOS job (the fast drain) is `workflow_dispatch`-only from the start, and docs-only pushes skip the Android job (~30 free-tier min/merge otherwise) — comfortably inside 3,000 min/month. Revisit caching only if the Android job misbehaves.
- *(resolved 2026-07-09)* ~~Render free-tier fit for an always-on SSR process~~ — free-tier spin-down (cold start after idle) is **accepted for now**; a cold start is tolerable for "check the deployed app from anywhere". Recorded in Design; upgrade to the paid always-on instance only if it grates in practice. Fly.io remains the fallback platform.
- *(resolved 2026-07-07)* ~~Where PR review fits once agents are pushing branches~~ — **PR + auto-merge on green**, for maximum autonomy; the validate workflow is the merge gate and the human reviews outcomes via artifacts. Recorded in Design; revisit toward human-tap-merge only if it misbehaves.
