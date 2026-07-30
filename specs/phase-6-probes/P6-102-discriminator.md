# P6-102 follow-up — discriminating the embedded server from the hosted web server
Probed 2026-07-28 against 2cd1b6d.

## Answer

Five candidate discriminators exist. Ranked by whether they actually solve the
local-dev defect:

1. **Move the env var out of the shared `.env` and into `cargo tauri dev`'s
   `beforeDevCommand`.** — **WORKS, cheapest.** No code change, no new
   discriminator needed: the var stops reaching the plain `cargo leptos watch`
   web dev server in the first place, while `cargo tauri dev` (which *is* the
   desktop dev flow) still gets it. One-line edit to
   `src-tauri/tauri.conf.json:9` plus deleting the line from the root `.env`.
2. **Which arm supplied the OAuth challenge — cookie vs. in-memory stash.** —
   **WORKS as a code discriminator, with one caveat.** Already computed inside
   the handler (`app/src/lib.rs:1491-1492`). A challenge that came from the
   `tr_challenge` cookie means the *same* browser that started the flow is
   landing the callback → web branch (303). A challenge that came from
   `native::take_challenge()` means the requester is a bystander system browser
   that never had our cookie → native branch (terminal page). This is the only
   candidate that separates the two cases when a *single* process must serve
   both, which is exactly the `cargo tauri dev` situation.
3. **A `#[cfg(all(feature = "native", not(feature = "hosted")))]` guard.** —
   **BREAKS the desktop dev flow.** Correct for shipped binaries, wrong for
   `cargo tauri dev`: in dev the server serving the Tauri webview is the
   `server` bin built with `hosted`, not `three_rings` built with `native`. See
   Evidence.
4. **Request-time host / `x-forwarded-*` / loopback check.** — **BREAKS
   locally.** In `cargo tauri dev` the webview *and* the plain web dev browser
   *and* the system browser all talk to the same `http://localhost:3000`
   process. There is no header difference to read.
5. **An explicit marker on the callback URL round trip (e.g. `?native=1`).** —
   **CAVEAT — unverified.** Would be unambiguous, but depends on Better Auth
   preserving extra query params on `callbackURL`; I could not probe the live
   service. See Open.

## Evidence

### 1 — Feature split (compile-time discriminator)

- `server` bin enables `app` with `features = ["hosted"]`, `default-features = false` —
  `server/Cargo.toml:18`.
- `src-tauri` (`three_rings`) enables `app` with `features = ["native"]`,
  `default-features = false` — `src-tauri/Cargo.toml:35`.
- `native = ["ssr"]`, `hosted = ["ssr", …]`; `ssr` alone is the substrate and a
  `compile_error!` demands exactly one backend — `app/Cargo.toml:66-70`,
  `app/src/backend/mod.rs:48-53`.
- Nothing else enables `native`. Only two places name it: `src-tauri/Cargo.toml:35`
  and the merge-gate lint line `cargo clippy -p app --features native`
  (`.github/workflows/validate.yml:100`). `frontend` does not.
- **`/auth/callback` is compiled into both builds.** The handler carries only
  `#[cfg(feature = "ssr")]` (`app/src/lib.rs:1475`) and is mounted
  unconditionally in `build_router` (`app/src/lib.rs:1628`); only the hosted JSON
  routes are cfg-gated (`app/src/lib.rs:1634-1635`).
- The repo's established idiom for "this is the native-backend build" is
  `#[cfg(all(feature = "native", not(feature = "hosted")))]`, not a bare
  `#[cfg(feature = "native")]` — `app/src/lib.rs:254, 312, 367, 415, 844`. The
  `not(hosted)` half exists because `cargo clippy/test --workspace` unifies both
  features into one `app` build. A bare `#[cfg(feature = "native")]` guard on the
  `if native` branch would therefore be **wrong under the workspace gate lines**
  as well as wrong in dev.
- **Why the cfg guard breaks desktop dev.** The Tauri embedded server only exists
  in release: the whole setup block that binds the loopback listener and calls
  `std::env::set_var("TR_EMBEDDED_ORIGIN", …)` sits under
  `#[cfg(not(debug_assertions))]` — `src-tauri/src/lib.rs:54`, set at `:194`.
  In debug, `cargo tauri dev` points the webview at
  `devUrl = http://localhost:3000` and runs `cargo leptos watch` as
  `beforeDevCommand` — `src-tauri/tauri.conf.json:8-9`. So the process answering
  `/auth/callback` during desktop dev is the **`hosted`** `server` binary. Gate
  the branch on `native` and the desktop dev Google flow loses the terminal page
  and the in-memory session stash: the 303 would set cookies in the *system
  browser*, and the webview's `current_user` poll would never claim a session.
  That is precisely why the maintainer keeps `TR_EMBEDDED_ORIGIN` in `.env`.
  → **cfg guard: correct for shipped binaries, BREAKS `cargo tauri dev`.**

### 2 — Who reads `TR_EMBEDDED_ORIGIN`

Three read sites and one write site, all in `app`/`src-tauri`; **`server` never
touches it by name**:

- `app/src/auth/native.rs:24-26` — `embedded_origin()`, the only reader of the var.
- `app/src/lib.rs:1486` — the `/auth/callback` branch (the defect).
- `app/src/account.rs:387` — `google_sign_in`: picks the upstream `Origin` and the
  callback URL, and at `:413-415` decides whether to *also* stash the challenge in
  memory. **This is the second half of the same switch** — it is the reason the
  in-memory challenge is even populated on the dev web server.
- `app/src/auth/native.rs:63` — `complete_google_return` (Android deep-link leg).
- `src-tauri/src/lib.rs:194` — the only writer, `std::env::set_var(...)` with the
  dynamic loopback port, release-only.

Because the shell sets the var **itself at runtime**, the root `.env` entry is
dead weight for release desktop builds; it exists purely for the debug
`cargo tauri dev` case.

### 3 — How the root `.env` reaches the process

- **`dotenvy::dotenv().ok()` in `server/src/main.rs:10`** (dep declared at
  `server/Cargo.toml:28`). That is the whole mechanism. `cargo leptos watch` runs
  the `server` bin with the workspace root as cwd, so it picks up the root `.env`.
  Nothing else in the workspace calls dotenv — `src-tauri` does not, and the
  `frontend`/`app` crates do not.
- Confirmed against the *currently running* watch server (pid 15971 on :3000):
  `ps eww` shows **zero** matches for `TR_EMBEDDED_ORIGIN=` in its launch
  environment, i.e. the var is not exported by the shell or the devcontainer — it
  is being injected in-process by dotenvy from the root `.env`.
- Both env files exist. **Keys only** (no values read out):
  - root `.env`: `LEPTOS_SITE_ADDR`, `LEPTOS_RELOAD_PORT`, `DATABASE_URL`,
    `MIGRATION_DATABASE_URL`, `PROD_MIGRATION_DATABASE_URL`, `NEON_AUTH_BASE_URL`,
    **`TR_EMBEDDED_ORIGIN`**, `RENDER_API_KEY`.
  - `.devcontainer/.env`: `LEPTOS_SITE_ADDR`, `LEPTOS_RELOAD_PORT`, `DATABASE_URL`,
    `MIGRATION_DATABASE_URL`, `PROD_MIGRATION_DATABASE_URL`, `RENDER_API_KEY`,
    `CLAUDE_CODE_OAUTH_TOKEN`, `GH_TOKEN`, `INGEST_DATABASE_URL`,
    `PROD_INGEST_DATABASE_URL`. **No `TR_EMBEDDED_ORIGIN`** — so the container's
    web dev server is unaffected; this is a host-only (macOS) defect.
- Deployed path is clean: the runtime image sets only `LEPTOS_*`
  (`Dockerfile:123-126`) and there is no `render.yaml` in the repo. Consistent
  with the established fact that Render is correct.
- `dotenvy` never overrides an already-set variable (`server/src/main.rs:8-9`), so
  an exported `TR_EMBEDDED_ORIGIN` wins over the file — which is what makes the
  `beforeDevCommand` option work.

### 4 — Request-time signals

- `cookies::request_origin` reads `x-forwarded-proto` / `x-forwarded-host`,
  falling back to `Host` and then the literal `"localhost:3000"` —
  `app/src/auth/cookies.rs:57-68`. `request_is_secure` is `x-forwarded-proto ==
  https` — `:72-78`.
- **Neither separates the two locally.** On Render the values are
  `https` + `three-rings-6p5o.onrender.com` (asserted at
  `app/src/auth/cookies.rs:101-111`), but locally the desktop dev webview, the
  system browser completing OAuth, and a plain web dev browser all hit the *same*
  `http://localhost:3000` process with the same `Host`. A loopback / peer-address
  check fails for the same reason. → **BREAKS locally.**
- **The one request-time signal that does work: challenge provenance.**
  `app/src/lib.rs:1491-1492` already does
  `cookie_value(&headers, CHALLENGE_COOKIE).or_else(native::take_challenge)`.
  `google_sign_in` sets `tr_challenge` unconditionally on *both* paths
  (`app/src/account.rs:416-421`), and the cookie is `SameSite=Lax`
  (`app/src/auth/cookies.rs:33`), so Google's top-level GET redirect does carry it
  back to the browser that started the flow. The Tauri system browser is a
  different cookie jar and has no such cookie, so it necessarily falls through to
  the memory stash. Tracking *which arm produced the value* and branching on that
  instead of on `embedded_origin().is_some()` is a correct discriminator even
  when one process serves both.
  **Caveat:** the cookie arm is checked first, so a stale `tr_challenge` left in
  the system browser at `localhost:3000` from an earlier *web* login would be
  preferred over the fresh memory stash and the exchange would fail. That hazard
  already exists today at `:1491`; a fix branching on provenance should also try
  the memory stash on a cookie-challenge exchange failure, or prefer the memory
  stash when one is present.

## What a fix would touch

- **Config-only fix (recommended, smallest):** `src-tauri/tauri.conf.json:9`
  (prefix the `beforeDevCommand` with `TR_EMBEDDED_ORIGIN=http://localhost:3000`;
  it already runs through a shell — it uses `cd .. &&`), plus deleting the line
  from the untracked root `.env` (maintainer's own file, not in the repo). Zero
  code. Also update `specs/auth.md:384`, which currently *instructs* exporting the
  var for dev-mode Google and is the origin of the whole trap.
- **Code fix (provenance branch):** `app/src/lib.rs:1484-1526` — replace the
  `native` boolean with a challenge-source enum. Bigger than it looks because
  `google_sign_in` (`app/src/account.rs:387, 398, 413-415`) is the *other* half of
  the same switch: it uses `embedded_origin()` to choose the upstream `Origin`,
  the callback URL, the Android deep-link bounce, *and* whether to stash the
  challenge at all. If `TR_EMBEDDED_ORIGIN` stops being set for the web dev
  server, no memory challenge is ever stashed there and the provenance branch
  falls out for free — meaning the config fix and the code fix largely address
  the same switch from opposite ends. Changing only `:1486` without `account.rs`
  leaves the sign-in side still taking the native origin path.
- **Do not** use a bare `#[cfg(feature = "native")]`: the repo idiom is
  `#[cfg(all(feature = "native", not(feature = "hosted")))]` because of workspace
  feature unification (`app/src/lib.rs:254` et al.), and either spelling breaks
  `cargo tauri dev` as shown above.
- The Android leg (`app/src/auth/native.rs:53-71`, `app/src/lib.rs:1574-1594`)
  returns via `/auth/app-return` + deep link and never reaches the `if native`
  branch, so it is unaffected by any of these.

## Open — needs a check I could not run

- **Does the upstream Better Auth service preserve extra query parameters on
  `callbackURL`?** Needed to judge discriminator #5. Exact check: a live
  `POST {NEON_AUTH_BASE_URL}/sign-in/social` with
  `callbackURL: "http://localhost:3000/auth/callback?tr_native=1"` and confirm the
  parameter survives to the landed redirect (the spec log at `specs/auth.md`
  records a comparable probe rejecting `three-rings://` with
  `INVALID_CALLBACKURL`, so the endpoint does validate this field). Requires
  network access to the auth service; out of scope for a read-only probe.
- **Runtime confirmation that the running :3000 server currently takes the native
  branch.** The branch is only reachable on a *successful* verifier+challenge
  exchange, so an unauthenticated `curl http://localhost:3000/auth/callback` hits
  `google_error_redirect` (`app/src/lib.rs:1597`) either way and proves nothing. A
  real check means completing a Google sign-in in a browser against the local
  watch server and observing whether the tab shows the terminal "Signed in" page
  instead of landing on `/`. Not performable read-only.
