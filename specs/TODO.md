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
- [ ] One server function + one page rendering DB rows, using at least one Rust/UI component (specs: [architecture-spike](architecture-spike.md), [ui-components](ui-components.md))
- [ ] Build + run: macOS desktop target (embedded Axum) (specs: [architecture-spike](architecture-spike.md))
- [ ] Write up findings in architecture-spike.md; mark spec implemented (specs: [architecture-spike](architecture-spike.md))

## Phase 1b — UI design — parallel with Phase 1, human-led

- [ ] Information architecture / nav structure (specs: [ui-design](ui-design.md))
- [ ] Wireframe core screens (catalog search, collection, add-flow, auth, shell) (specs: [ui-design](ui-design.md))
- [ ] Prototype the add-to-collection flow (specs: [ui-design](ui-design.md))
- [ ] Component gap analysis vs. Rust/UI registry (specs: [ui-design](ui-design.md), [ui-components](ui-components.md))

## Phase 2 — foundations

- [ ] CI: fmt, clippy, test, web build (incl. Tailwind pipeline) on push
- [ ] Flesh out data-model spec using spike findings + designs; write initial migrations (specs: [data-model](data-model.md))
- [ ] Design the data-access trait split; remove spike-era direct DB access (prerequisite: Phase 1 complete) (specs: [data-access-backends](data-access-backends.md))

## Later / parked (not in the queue — promote to a phase before working)

- Bundled read-only catalog for offline browsing on desktop/mobile (deliberately deferred)
- Decks and sharing features
- Import/export (CSV, Moxfield)
- macOS desktop build path — CI (macOS runner) vs. host build — decide before the Phase 1 macOS-desktop task (the host gained rustup + tauri-cli at the Android gate, so a host build is now the low-friction option)

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
