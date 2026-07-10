# Project TODO — execution queue

**This file is the single source of truth for what to work on next.** The selection algorithm, spec gating, and definition of done are in [README.md](README.md) ("Working the queue"). State legend: `[ ]` available · `[~]` in progress · `[x]` done.

Phases execute top to bottom; tasks within a phase top to bottom. A task's `(specs: ...)` lists every spec it is gated on — all must be `accepted` (status is read from the spec files, not recorded here). Tasks without a specs annotation are ungated.

## Phase 0 — dev environment (devcontainer)

Keeps the Rust toolchain off the host; see the Decisions log and `.devcontainer/README.md`.

- [x] Author `.devcontainer/` and build the `dgoings/three-rings` dev image (v1: Rust/Leptos/Tauri CLIs — cargo-leptos, cargo-generate, cargo-tauri, leptosfmt); wire devcontainer (image, ports 3000/3001, env-file, zshrc mount)

## Phase 1 — architecture spike

Ordered riskiest-first; see the spec's Failure policy — if the Android gate fails, STOP the phase. Executed inside the devcontainer (Phase 0); macOS desktop is host/CI-side — see Decisions log.

- [x] Compare scaffold bases (start-tauri-fullstack vs. tauri-leptos-ssr); record choice + rationale in architecture-spike.md (specs: [architecture-spike](architecture-spike.md), [ui-components](ui-components.md))
- [x] Scaffold Cargo workspace from the chosen base; commit unmodified (specs: [architecture-spike](architecture-spike.md))
- [x] Verify web target: `server` binary locally, SSR + hydration (specs: [architecture-spike](architecture-spike.md)) — done early via devcontainer smoke (see Findings)
- [x] Build + run: Android (emulator OK) — the architecture gate; static page sufficient (specs: [architecture-spike](architecture-spike.md)) — **PASS** on the Flip 7 emulator, incl. hydration + server fns (see Findings)
- [x] Set up Neon project (free tier): one trivial table, seed rows, sqlx connectivity from the server path (specs: [architecture-spike](architecture-spike.md)) — verified in-container: `cards` migrated + seeded, probe read 3 rows (see Findings)
- [x] One server function + one page rendering DB rows, using at least one Rust/UI component (specs: [architecture-spike](architecture-spike.md), [ui-components](ui-components.md)) — `/cards` SSRs Neon rows in the vendored Rust/UI table (see Findings)
- [x] Build + run: macOS desktop target (embedded Axum) (specs: [architecture-spike](architecture-spike.md)) — **PASS**, host-built; dynamic port, SSR + `/cards` from Neon (see Findings)
- [x] Write up findings in architecture-spike.md; mark spec implemented (specs: [architecture-spike](architecture-spike.md)) — spec status → implemented; Phase 1 complete

## Phase 1b — UI design — parallel with Phase 1, human-led

- [x] Information architecture / nav structure (specs: [ui-design](ui-design.md)) — two modes (Catalog / My cards); see design/information-architecture.md + spec Findings
- [ ] Wireframe core screens (catalog search, collection, add-flow, auth, shell) (specs: [ui-design](ui-design.md))
- [ ] Prototype the add-to-collection flow (specs: [ui-design](ui-design.md))
- [ ] Component gap analysis vs. Rust/UI registry (specs: [ui-design](ui-design.md), [ui-components](ui-components.md))

## Phase 2 — delivery pipeline

Code leaves the laptop: CI validation, remote-checkable artifacts, agent self-sufficiency. Ordered by dependency; budget policy and design details in the spec.

- [~] GitHub: create a free org + public repo under it (Blacksmith requires org ownership; public because free-plan orgs can't gate private repos), push `main`, enable auto-merge, branch protection requiring the validate check (blocked on the validate workflow existing), install the Blacksmith GitHub app, add secrets (`ANDROID_KEYSTORE` + passwords; `RENDER_DEPLOY_HOOK` only if needed) — done through auto-merge (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] Replace template `ci.yml` with the validate workflow: fmt, clippy (ssr + wasm, incl. `src-tauri` via Tauri linux deps), test, `cargo leptos build --release`; rust caching (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] Android APK artifact job (`main` + `workflow_dispatch`, `paths-ignore` for docs-only pushes): NDK r28+ (closes the 16 KB finding), aarch64 APK, signed with the keystore secret (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] macOS `.dmg` artifact job (`workflow_dispatch` only — Blacksmith macOS at $0.08/min) (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] Rolling `latest` prerelease: artifact jobs publish APK + `.dmg` to stable URLs (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] Web deploy: multi-stage `Dockerfile` (server binary + site, `PORT`→`LEPTOS_SITE_ADDR` entrypoint) + Render service with GitHub integration; Neon branch split (project main → Render `DATABASE_URL`, new `dev` branch → `.devcontainer/.env`); verify `/` + `/cards` at the public URL (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] Agent self-sufficiency: root `CLAUDE.md` (commands, what-runs-where, queue conventions), in-container git/`gh` auth docs, update `.devcontainer/README.md` table (CI owns Android/macOS builds) (specs: [delivery-pipeline](delivery-pipeline.md))
- [ ] Prove the loop end-to-end: fresh container → agent does a trivial task → PR → auto-merge on green (zero human touch) → verify web URL + APK update away from the laptop, dispatch and check the `.dmg`; mark spec implemented (specs: [delivery-pipeline](delivery-pipeline.md))

## Phase 3 — foundations

- [ ] Flesh out data-model spec using spike findings + designs; write initial migrations (specs: [data-model](data-model.md))
- [ ] Design the data-access trait split; remove spike-era direct DB access — also the path to native builds using the deployed API instead of direct Neon (specs: [data-access-backends](data-access-backends.md))

## Later / parked (not in the queue — promote to a phase before working)

- Bundled read-only catalog for offline browsing on desktop/mobile (deliberately deferred)
- Decks and sharing features
- Import/export (CSV, Moxfield)

## Decisions log

- 2026-07: API-first on Neon chosen over offline-first Turso designs. Rationale in README.
- 2026-07: Architecture spike prioritized ahead of data model — architecture unproven.
- 2026-07: Spec numbering dropped; filenames are the stable IDs, this file owns execution order.
- 2026-07: Tasks gated on spec status via `(specs: ...)` annotations; only humans accept specs.
- 2026-07: Spike decisions: web = local run only; mobile = Android; mobile SSR failure = stop and reassess; no time-box; Android gate moved ahead of DB work (fail fast).
- 2026-07: Scaffold base = tauri-leptos-ssr (embedded in-process Axum matches the README); start-tauri-fullstack rejected (thin shell → external server, csr default). Rationale in architecture-spike.md Findings.
- 2026-07: Dev environment = Docker devcontainer. Image `dgoings/three-rings` (`.devcontainer/Dockerfile`, layered on `dgoings/magic-assistant-dev`) carries the Rust/Leptos/Tauri toolchain so the host stays toolchain-free. All Rust dev + the web target build/run in the container; the Android *build* is containerized; macOS desktop + iOS and the Android *run* (emulator/device) are host-side.
- 2026-07: Consequence of the devcontainer split — macOS desktop (Phase 1) is deferred behind the Android gate, built later via a CI macOS runner or a minimal host install (keeps the host toolchain-free until the architecture is proven). Android SDK/NDK are added to the image as a second layer just before the gate.
- 2026-07-07: **Android build moved host-side** (reverses the containerized-Android-build part of the devcontainer decision). Google ships no linux-arm64 NDK — official Linux tooling is x86_64-only — so the planned Android layer on the arm64 image cannot work; an amd64-under-Rosetta image variant was viable but was passed over for the host toolchain (Android Studio SDK/NDK/JBR + brew rustup + binstalled cargo-leptos/tauri-cli). Web dev stays in the container; the "toolchain-free host" goal is relaxed to "Rust via brew rustup, no ad-hoc curl installs".
- 2026-07-07: **Android architecture gate passed** on the Samsung Flip 7 emulator — release APK, embedded in-process Axum serving SSR + hydration + server fns. Android-specific fixes (cleartext/signing in gradle, APK-asset extraction, on_page_load navigation) recorded in architecture-spike.md Findings; `src-tauri/gen/android` is now committed since it carries the gradle config.
- 2026-07-07: **macOS desktop built host-side** (resolves the parked CI-vs-host question) — the toolchain was already on the host from the Android gate; Xcode supplies the platform bits.
- 2026-07-07: **Phase 1 complete — architecture spike passed on all targets**; architecture-spike.md marked implemented, carry-forward items in its Conclusion. Neon `DATABASE_URL` lives in `.devcontainer/.env` (host shell exports it for native desktop runs).
- 2026-07-09: **Web deploy platform = Render** (maintainer decision at spec review, supersedes the Railway draft choice below; Fly.io fallback).
- 2026-07-09: **Repo is public** (was: private in the draft) — discovered at setup: the free-plan org can't enforce branch protection or auto-merge on a private repo, and the merge gate depends on both; public unlocks them at zero cost. Side effects: the rolling release needs no GitHub login from the phone, and GitHub-hosted minutes would be free too (Blacksmith remains the runner choice for speed and its own free tier). Repo lives at `threeringsdev/three-rings`.
- 2026-07-09: **Artifact cadence split by cost, not target** — Android APK per `main` merge (plus dispatch; docs-only pushes skipped via `paths-ignore`) keeps the phone-checkable rolling release fresh; macOS `.dmg` is `workflow_dispatch`-only from the start ($0.08/min would exhaust the Blacksmith free tier in ~15 merges; its consumer is at a Mac anyway, and pre-merge desktop checks happen as local host builds). Per-target path filtering rejected: the shared `app` crate means most code changes alter every artifact.
- 2026-07-09: **CI runners = Blacksmith** (maintainer decision, supersedes the GitHub-hosted draft; drop-in `runs-on` labels, any job reverts to GitHub-hosted independently). Consequence: the repo must be owned by a free GitHub organization — Blacksmith doesn't install on personal accounts. Budget framing changes from GitHub's 2,000 min / 10×-macOS multiplier to Blacksmith's 3,000 free min/month (2 vCPU x64 basis; macOS M4 $0.08/min), plus GitHub's $0.002/min control-plane fee on all Actions minutes (in effect since 2026-03).
- 2026-07-07: **Phases reorganized — delivery pipeline before data model/UI** (new Phase 2, specs/delivery-pipeline.md; foundations becomes Phase 3). Goal: agents work autonomously from any host with the repo as the contract; results checkable remotely. Decisions: GitHub private repo; two-tier CI (validate per-push linux-only; artifacts on `main` merges + manual — private-repo minutes, macOS 10×); one keystore-as-secret for in-place APK updates; rolling `latest` prerelease; web deploy = Railway building a repo Dockerfile (Render fallback; zero Actions minutes); no `DATABASE_URL` in CI. Android-in-CI uses NDK r28+ (x86_64 runners sidestep the linux-arm64 NDK gap and close the 16 KB finding).
