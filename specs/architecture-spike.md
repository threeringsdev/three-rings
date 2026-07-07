# Architecture spike

**Status:** accepted
**Depends on:** —

## Problem

The proposed architecture (shared `app` crate with Leptos SSR + embedded Axum, shipped as web app and Tauri-wrapped native apps, talking to Neon) hasn't been tried by us. Before investing in the data model or features, prove the skeleton end-to-end — riskiest assumption first.

## Scope

In: project scaffold, a trivial schema (one table, e.g. `cards(id, name)` with a few seed rows), one server function reading it, one page rendering it, building and running on every target. Out: real data model, auth, UI polish, deployment to real infrastructure — anything not needed to prove the pipeline.

## Success criteria

The same `app` crate, with a one-table Neon database behind it, demonstrably:

- [ ] Runs as an Android app (Tauri, embedded Axum — **the architecture gate**, see Failure policy)
- [ ] Runs as a macOS desktop app (Tauri, embedded Axum, dynamic port)
- [ ] Serves SSR + hydration via the `server` binary running locally (real deployment is out of scope)
- [ ] Renders one Rust/UI component (proves Tailwind pipeline in both build paths)
- [ ] Dev loop works: `cargo tauri dev` / `cargo leptos watch` with hot reload

## Plan

Ordered to test the riskiest assumption (embedded SSR on mobile) before investing in the rest.

1. Pick scaffold base: [start-tauri-fullstack](https://github.com/rust-ui/start-tauri-fullstack) (Rust/UI pre-wired) vs. [tauri-leptos-ssr](https://github.com/codeitlikemiley/tauri-leptos-ssr) — evaluate both, pick one (see ui-components).
2. Scaffold workspace; commit before modifications.
3. Build/run macOS desktop (should work — the templates prove this pattern).
4. **Build/run Android (emulator acceptable): does the embedded Axum + WebView pattern work at all on mobile?** Static page is sufficient; no DB needed for this gate.
5. Set up Neon free-tier project; single migration; connect via sqlx from the server path.
6. One server function + one page listing rows, using one Rust/UI component.
7. Verify web target: `server` binary locally, SSR + hydration.
8. CI: fmt, clippy, test, and web build on push.

## Failure policy

If embedded SSR does not work on Android (step 4), **stop the spike and reassess the architecture** — do not proceed to steps 5–8, do not silently fall back to CSR. Write up findings, then revisit the README architecture with the human before any further work.

## Time-box

None. The spike runs until success criteria are met or the failure policy triggers.

## Findings

(Record here as the spike proceeds — this section becomes the input to data-access-backends and future specs.)

### Scaffold base decision (task 1, 2026-07-06)

**Chosen: [tauri-leptos-ssr](https://github.com/codeitlikemiley/tauri-leptos-ssr).** Scaffold from it; mine [start-tauri-fullstack](https://github.com/rust-ui/start-tauri-fullstack) as *reference* (Rust/UI components, sqlx wiring, mobile config) at the relevant later steps.

**Key finding: the two bases implement different architectures, not two versions of one.**

- **tauri-leptos-ssr embeds Axum in-process — this README verbatim.** `src-tauri/src/lib.rs` binds `TcpListener::bind("127.0.0.1:0")` (OS-assigned port, `[::1]:0` fallback), spawns `axum::serve` as a `tauri::async_runtime` task, waits for readiness, then `window.navigate("http://127.0.0.1:{port}")` (task aborted on window close). `src-tauri/Cargo.toml` depends on `app` (`features = ["ssr"]`) + `axum` + `tokio`. Layout is `app/frontend/server/src-tauri` (our planned layout). Versions current and coherent: Leptos 0.8.2, Tauri 2.5.0, axum 0.8.4. Distributed as a `cargo generate` template.
- **start-tauri-fullstack is a thin shell pointed at an external server.** Its `run()` embeds nothing; `src-tauri/Cargo.toml` links only `tauri` + `tauri-plugin-opener` (no `app`/`axum`/`tokio`, so it *cannot* embed). WebView → fixed `localhost:3000` (desktop) and, on Android, → the dev machine's LAN IP (`tauri.android.conf.json` = `http://192.168.1.101:3000`, rewritten by `just run_android` to `ipconfig getifaddr en0`). The `app` crate defaults to the `csr` feature and the frontend is not bundled as a resource. So its "mobile support" proves only that an Android WebView can load a *networked* SSR site — not that embedded Axum SSR runs *inside* the Android process, which is our architecture gate. Adopting it as-is would be the silent CSR/remote-server fallback the Failure policy forbids.
- This corrects ui-components.md's parenthetical "they're the same pattern" — they are not.
- start-tauri-fullstack's real advantages (Rust/UI pre-copied, sqlx + migrations, iOS/Android configs) map to later spike steps (Neon = step 5) or ui-components work we do regardless — reusable as reference from the chosen base. Maturity also favors tauri-leptos-ssr (22★ / 31 commits / CI / cargo-generate vs. 13★ / 4 commits).

**Caveat that shapes the Android gate (task 4):** tauri-leptos-ssr's embedded-server block is gated behind `#[cfg(not(debug_assertions))]` (release only); dev falls through to `devUrl` + `cargo leptos watch` (a sibling process). Because `cargo tauri android dev` builds debug, proving embedded SSR on Android will require a *release/bundled* build **or** lifting that `cfg` so the embedded server also runs in debug. Decide this when executing task 4 — do not let a debug build's `devUrl` fallback masquerade as a passing gate.

### Scaffold generated (task 2, 2026-07-07)

Generated with `cargo generate --git https://github.com/codeitlikemiley/tauri-leptos-ssr --name three-rings --silent` inside the devcontainer, then copied into the repo. Verified: placeholder substitution correct (`three_rings` crate, `com.three-rings.dev` id, MIT © Dylan Goings, Leptos 0.8.2, `src-tauri` → `app` with `ssr` + axum + tokio), and the workspace parses (`cargo verify-project` + `cargo metadata --no-deps` OK; members app/frontend/server/three_rings). Not built yet — that is task 3+.

Deliberate deviations from a pristine `cargo generate` (everything else copied unmodified):
- **Kept our `README.md`** (the template emits none — stripped via its `.genignore`) and **our `.gitignore`** (merged in the Playwright/`node_modules` ignores; dropped the `Cargo.lock` ignore so this app workspace commits its lockfile).
- **Excluded `CHANGELOG.md`** — it is the *template's* sidecar→in-process migration history, not ours.
- **Excluded `.vscode/`** — gitignored here; the devcontainer supplies editor settings.
- **Included `LICENSE`** as rendered (MIT © 2026 Dylan Goings). ⚠️ Confirm this is the intended project license, or change/remove it.

### Web target verified — SSR + hydration (task 7, pulled early, 2026-07-07)

Built and ran the web target in the devcontainer (`cargo leptos build` + `cargo leptos serve`) against the scaffold's demo app. **Pass.**
- **Build:** OK — frontend → wasm32 via wasm-bindgen 0.2.103; `server` binary built; Tailwind CSS pipeline ran (arbitrary-value classes appear in the output HTML).
- **SSR:** server returns fully server-rendered HTML (~2.6 KB) — `<!DOCTYPE html>`, `<title>Welcome to Three Rings</title>`, and the counter UI rendered server-side (not an empty shell).
- **Hydration:** Leptos bootstrap present — `modulepreload /pkg/app.js`, `preload /pkg/app.wasm`, and the `<script type=module>` that imports `app.js` and calls `mod.hydrate()`. Assets serve: `GET /pkg/app.js` → 200 (50 KB), `/pkg/app.wasm` → 200 (3.9 MB, unoptimized debug), `/pkg/app.css` linked.
- **To confirm at task 6:** an ad-hoc `POST /api/get_count` returned 404 — likely a wrong path/verb in the probe (Leptos server-fn URL scheme), not necessarily a defect. Interactive hydration (wasm executes, counter increments) is verifiable via the bundled Playwright e2e for deeper confidence.

Proves the full web toolchain (cargo-leptos, wasm, Tailwind, Axum SSR) end-to-end in the container.

- **Dev environment:** built and run inside a Docker devcontainer (image `dgoings/three-rings`) to keep the Rust toolchain off the host — see TODO.md Decisions log + `.devcontainer/README.md`. Bearing on this spike: the web target (task 7) runs in-container; the Android *build* is containerized while the *run* (emulator) is host-side; macOS desktop (task 3) is deferred behind the Android gate (CI or minimal host install).
- Where do DB credentials live during the spike? (Native builds talking directly to Neon is acceptable *for the spike only* — data-access-backends removes this before any real user data.)
