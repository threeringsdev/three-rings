# Architecture spike

**Status:** implemented
**Depends on:** —

## Problem

The proposed architecture (shared `app` crate with Leptos SSR + embedded Axum, shipped as web app and Tauri-wrapped native apps, talking to Neon) hasn't been tried by us. Before investing in the data model or features, prove the skeleton end-to-end — riskiest assumption first.

## Scope

In: project scaffold, a trivial schema (one table, e.g. `cards(id, name)` with a few seed rows), one server function reading it, one page rendering it, building and running on every target. Out: real data model, auth, UI polish, deployment to real infrastructure — anything not needed to prove the pipeline.

## Success criteria

The same `app` crate, with a one-table Neon database behind it, demonstrably:

- [x] Runs as an Android app (Tauri, embedded Axum — **the architecture gate**, see Failure policy)
- [x] Runs as a macOS desktop app (Tauri, embedded Axum, dynamic port)
- [x] Serves SSR + hydration via the `server` binary running locally (real deployment is out of scope)
- [x] Renders one Rust/UI component (proves Tailwind pipeline in both build paths)
- [x] Dev loop works: `cargo tauri dev` / `cargo leptos watch` with hot reload (watch verified end-to-end; see Findings for the `tauri dev` caveat)

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
- **Included `LICENSE`** as rendered (MIT © 2026 Dylan Goings). ✅ Confirmed 2026-07-07: keeping MIT.

### Web target verified — SSR + hydration (task 7, pulled early, 2026-07-07)

Built and ran the web target in the devcontainer (`cargo leptos build` + `cargo leptos serve`) against the scaffold's demo app. **Pass.**
- **Build:** OK — frontend → wasm32 via wasm-bindgen 0.2.103; `server` binary built; Tailwind CSS pipeline ran (arbitrary-value classes appear in the output HTML).
- **SSR:** server returns fully server-rendered HTML (~2.6 KB) — `<!DOCTYPE html>`, `<title>Welcome to Three Rings</title>`, and the counter UI rendered server-side (not an empty shell).
- **Hydration:** Leptos bootstrap present — `modulepreload /pkg/app.js`, `preload /pkg/app.wasm`, and the `<script type=module>` that imports `app.js` and calls `mod.hydrate()`. Assets serve: `GET /pkg/app.js` → 200 (50 KB), `/pkg/app.wasm` → 200 (3.9 MB, unoptimized debug), `/pkg/app.css` linked.
- **To confirm at task 6:** an ad-hoc `POST /api/get_count` returned 404 — likely a wrong path/verb in the probe (Leptos server-fn URL scheme), not necessarily a defect. Interactive hydration (wasm executes, counter increments) is verifiable via the bundled Playwright e2e for deeper confidence.

Proves the full web toolchain (cargo-leptos, wasm, Tailwind, Axum SSR) end-to-end in the container.

- **Dev environment:** built and run inside a Docker devcontainer (image `dgoings/three-rings`) to keep the Rust toolchain off the host — see TODO.md Decisions log + `.devcontainer/README.md`. Bearing on this spike: the web target (task 7) runs in-container; the Android *build* is containerized while the *run* (emulator) is host-side; macOS desktop (task 3) is deferred behind the Android gate (CI or minimal host install). *(Superseded 2026-07-07: the Android build moved host-side — see the Android gate finding.)*
- Where do DB credentials live during the spike? (Native builds talking directly to Neon is acceptable *for the spike only* — data-access-backends removes this before any real user data.) *Partially answered at task 5: container/server path reads `DATABASE_URL` from `.devcontainer/.env` (gitignored). Native tauri builds are still open — decide at task 6.*

### Android gate (task 4, 2026-07-07) — PASS

Release APK (aarch64/arm64-v8a) on the Samsung Flip 7 AVD (Android 16, API 36). The embedded in-process Axum pattern works on Android, beyond the "static page" bar:

- **Embedded server:** bound `127.0.0.1:<os-assigned port>` inside the app process — verified via `/proc/net/tcp` (LISTEN socket owned by the app's uid), not just logs.
- **SSR:** the server returns fully rendered HTML (verified by curling it through `adb forward` from the host).
- **Hydration + server fns:** the counter increments on tap — wasm hydrated the server-rendered DOM and the `increment_count`/`get_count` server functions round-trip through the in-process server.
- **Gate honesty:** release profile (embedded server active; `devUrl` unused). A static fallback cannot masquerade as a pass: SSR ships no `index.html`, so the asset protocol shows "asset not found" (observed before the navigation fix, below).

**Build environment (decision reversed — host-side, not containerized).** Google ships no linux-arm64 NDK (x86_64 only; ARM Linux hosts explicitly not on their roadmap), so the planned Android layer on our arm64 image was a dead end. An amd64 image variant under Rosetta was viable (the base image is multi-arch on Docker Hub) but heavier and slower; chose the host: Android Studio's SDK/NDK/JBR (already installed) + brew `rustup` (keg-only) + `aarch64-linux-android`/`wasm32-unknown-unknown` targets + binstalled `cargo-leptos` 0.3.7 / `tauri-cli` 2.11.4. Versions: Tauri 2.11.2, NDK 27.1, compileSdk 36, minSdk 24. Web dev stays in the container.

**Three Android-specific fixes were required (all in-tree, none a fallback):**

1. **Cleartext + signing** (`src-tauri/gen/android/app/build.gradle.kts`): Android blocks cleartext HTTP in release builds, and the generated release buildType is unsigned (uninstallable). Release now sets `usesCleartextTraffic=true` (TODO post-spike: scope to 127.0.0.1 via `networkSecurityConfig`) and signs with the auto-generated debug keystore (fine until Play distribution). This is why `src-tauri/gen/android` is now committed (was gitignored by the template).
2. **Resources are not files on Android** (`src-tauri/src/lib.rs`): bundled resources live inside the APK; `resource_dir()` returns the non-filesystem URI `asset://localhost/`, so the template's `get_configuration(<resource Cargo.toml>)` + Axum `ServeDir` cannot work. The Android branch extracts the embedded `frontendDist` assets via Tauri's `AssetResolver` into `<app-data>/site` at launch and configures Leptos from env vars (no Cargo.toml on disk). Re-extracts every launch (~4 MB) — add a version check before real releases. Desktop path unchanged.
3. **navigate() races webview creation** (`lib.rs`): Android creates its webview asynchronously; the template's `window.navigate(http://127.0.0.1:<port>)` during `setup` fires before the webview's initial load and loses — the app sticks on "asset not found: index.html". Fixed by stashing the port in managed state (`ServerPort`) and navigating from `on_page_load` once the initial load finishes (webview provably exists). Desktop keeps the setup-time navigate.

**Known warts (non-blocking, pre-release TODOs):**

- **16 KB page size:** `libapp_lib.so` is 4 KB-aligned (NDK r27 default); Android 16 shows a compatibility dialog and runs the app in compat mode. Fix with NDK r28+ (16 KB default; r30 is already installed host-side) or `-Wl,-z,max-page-size=16384`. Play requires 16 KB alignment for new submissions.
- **applicationId sanitization:** `tauri android init` silently mapped the identifier `com.three-rings.dev` → `com.three_rings.dev` (hyphens are illegal in Android app IDs — and underscores are illegal in iOS bundle IDs). Pick a canonical alphanumeric-and-dots identifier before any store distribution or iOS work.

### Neon + sqlx from the server path (task 5, 2026-07-07) — PASS

Free-tier Neon project (human-created); `DATABASE_URL` lives in `.devcontainer/.env` (gitignored, template in `.env.example`). Verified inside the devcontainer: the `server` binary logged `neon: connected, cards table has 3 rows` at startup — migration applied and seed rows read over TLS.

- **Wiring:** sqlx 0.8 (`runtime-tokio`, `tls-rustls`, `postgres`, `macros`, `migrate`; default features off — rustls avoids system OpenSSL in the container). Optional dep of `app` behind the `ssr` feature, so the wasm/hydrate build never sees it. Shared pool + embedded migrations in `app::db` (tokio `OnceCell`; migrations run on first pool use). Trivial schema in `migrations/0001_create_cards.sql` — `cards(id, name)`, three seed rows.
- **Direct endpoint, not the pooler:** Neon's connection pooler is PgBouncer in transaction mode, which breaks sqlx's migration advisory locks. Revisit pooling when data-access-backends introduces the trait boundary.
- **No compile-time-checked `query!` macros** — builds and CI need no live database.
- **Probe is non-fatal:** without `DATABASE_URL` the server logs the failure and still serves, so the plain web demo keeps working.

### Server fn + page + Rust/UI component (task 6, 2026-07-07) — PASS

Verified in the devcontainer: `GET /cards` returns server-rendered HTML containing all three seeded card names inside the full Rust/UI table structure (`data-name="TableWrapper|Table|TableHeader|TableRow|TableCell|…"`), and the emitted `app.css` maps the theme utilities (`.bg-card` → `var(--card)`, etc.). Pieces:

- **Server fn:** `get_cards()` (`#[server(prefix = "/api")]`) reads `cards` through `app::db::pool()`; `CardsPage` at `/cards` renders it via `Resource` + `Suspense` (linked from the home page).
- **Rust/UI adoption per ui-components.md:** the registry `table.rs` is copied to `app/src/components/ui/` (ours now; trimmed unrelated Card helpers, fixed a malformed `TableCell` class). Theme tokens: minimal shadcn-style set (`:root` vars + Tailwind v4 `@theme inline` mapping) trimmed into `style/input.css` — real palette/dark-mode is ui-design's call.
- **Caveat — `leptos_ui` force-enables `leptos/nightly`:** Rust/UI components import `clx!` from the `leptos_ui` crate, which would flip the whole workspace to nightly-feature leptos and break the stable build. The macro is ~30 lines and stable-safe, so it's vendored (`app/src/components/ui/clx.rs`); only `tw_merge` was added as a real dependency (plus `serde` for the row struct). Recorded in ui-components.md.
- Tailwind v4 was already correctly wired by the scaffold (`tailwind-input-file` + `@import "tailwindcss"`); the same `cargo leptos build` runs in the web path and the Tauri `beforeBuildCommand`, so the component/CSS pipeline is common to both.

### macOS desktop (task 3, executed last, 2026-07-07) — PASS

Host-built with `cargo tauri build --bundles app` (resolves the parked CI-vs-host question in favor of the host — the toolchain was already there from the Android gate; Xcode supplies the platform bits). Launched from the shell with `DATABASE_URL` exported: embedded Axum bound a **dynamic OS-assigned port** (`127.0.0.1:54633` in the verification run), `/` served SSR, and `/cards` returned the Neon rows inside the Rust/UI table. The desktop path still uses the template's original mechanism (bundled resources + `Cargo.toml` config from `resource_dir()`), untouched by the Android-specific fixes — both branches of the platform split are now proven.

### Dev loop (2026-07-07) — PASS

`cargo leptos watch` on the host: source edit → automatic rebuild → server serves the change (verified in both directions with a title edit and revert). Caveat for the criterion's other half: `cargo tauri dev` drives this same watch via `beforeDevCommand` with the window pointed at `devUrl`, and in debug builds the embedded server intentionally does not start — so native dev iterates against the watch server, not embedded Axum. Fine for development; release builds exercise the embedded path (as Android and macOS above did). Exercise `cargo tauri dev` explicitly when native-shell dev work actually begins.

### Conclusion (2026-07-07) — spike complete, architecture proven

Every success criterion is met: the one shared `app` crate, backed by a one-table Neon database, runs as an Android app (release APK, in-process Axum SSR — the gate), a macOS desktop app (dynamic port), and a local `server` binary with SSR + hydration, rendering a Rust/UI component through the common Tailwind v4 pipeline, with a working hot-rebuild dev loop. The failure policy was never triggered.

Carry-forward items (recorded above, none architecture-threatening):
- 16 KB page alignment for Android release (NDK r28+/linker flag) — required for Play.
- Scope Android cleartext to `127.0.0.1` via `networkSecurityConfig`; add a version check to the asset extraction.
- Pick a canonical app identifier (no hyphens/underscores) before store/iOS work.
- Where native builds get `DATABASE_URL` (desktop inherits the shell env today; Android has no env) — moot once data-access-backends puts the API boundary in front of the database.
- Neon pooler (PgBouncer transaction mode) vs. sqlx migration locks — revisit with data-access-backends.
- `leptos_ui` upstream forces `leptos/nightly` — keep vendoring `clx!` (ui-components.md).
