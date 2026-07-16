# Data-access backends (hosted vs. native)

**Status:** implemented
**Depends on:** [data-model](data-model.md), [auth](auth.md)

## Problem

The `app` crate's server functions run in two very different trust environments:

- **Hosted (web):** our infrastructure. Holding Postgres credentials is fine; this process is the authorization boundary.
- **Native (Tauri desktop/mobile):** the user's machine. The embedded Axum server must never hold direct Postgres credentials — anything shipped in the binary or its config is extractable.

Server functions must work identically in both, or the shared-crate architecture loses its point.

## Design

**Per-domain data-access traits** (`CatalogStore`, `CollectionStore`, plus more as decks/sharing land) that server functions program against. Two implementations of each:

| | Hosted impl | Native impl |
|---|---|---|
| Transport | In-process sqlx against Neon | HTTPS to the hosted API |
| Credentials | `DATABASE_URL` from environment | The user's Better Auth JWT (`tr_jwt`), forwarded as `Authorization: Bearer` |
| Authorization | Enforced here (the terminus) | Delegated — hosted API enforces |

### Compile-time selection

Selected at compile time via the cargo feature split, so the native binary contains no sqlx/Postgres code path at all. **This requires decomposing today's `ssr` feature**, which currently *implies* sqlx: [`app`'s `ssr`](../app/Cargo.toml) pulls in `dep:sqlx` and the direct `DATABASE_URL` pool ([`app/src/db.rs`](../app/src/db.rs)), and [`src-tauri`](../src-tauri/Cargo.toml) builds `app` with `features = ["ssr"]` — so **the native shell already ships the Postgres path today** (spike debt this task retires; `db.rs`'s header flags itself as such). The split:

- Keep `ssr` = the router, `leptos_axum`, Axum, serde — everything the embedded server needs regardless of backend.
- Move `dep:sqlx`, `db.rs`, and the sqlx `*Store` impls behind a new **`hosted`** feature. The web `server` binary enables `ssr + hosted`.
- The native **`native`** feature carries the HTTPS client impls. `src-tauri` enables `ssr + native` — never `hosted` — so `dep:sqlx` is not in its dependency graph at all.

### Key rules

- **One terminus for data authorization.** All *row/data* authorization and DB-credential-holding happens in the hosted deployment. The hosted impl does NOT call the API over loopback — that would add a serialization round-trip for zero security gain, since enforcement lives in the same process either way.
  - **Identity is the deliberate exception.** The native app already talks *directly* to Better Auth for sign-in/OTP/Google and holds the resulting httpOnly cookies on its embedded `127.0.0.1` server (see [auth](auth.md)) — auth does **not** funnel through the hosted API. So "single terminus" scopes to *data access*, not to identity; the native embedded server is already a first-class auth client.
- **The native impl reuses the token the terminus already accepts — no new auth surface.** The hosted API's `AuthUser` extractor verifies the Better Auth **15-min EdDSA JWT** against Neon's JWKS. The native impl forwards that same `tr_jwt` as `Authorization: Bearer`, and on a `401` re-mints it from `tr_session` via auth's existing silent-refresh path. There is no bespoke native↔hosted API token to design — data-access rides the auth the hosted API was already built to verify.
- **Config: the hosted API base URL must be baked into release builds.** Finder-launched `.app`s and APKs get *no* environment (a lesson auth already paid — it bakes `NEON_AUTH_BASE_URL`/`TR_WEB_ORIGIN` release defaults). The native impl reuses **`TR_WEB_ORIGIN`** as the hosted API origin (exported env still wins, so a dev build can point at a local/dev deployment). This closes auth.md's explicit handoff ("Fuller native config plumbing still belongs to data-access-backends").
- The hosted API surface and the native impl's client are generated from/checked against the same shared types so the two backends cannot drift. **This spec owns the `shared/` types crate** — it is the drift guarantee's home, so it lives here rather than in [collection-api](collection-api.md); collection-api's endpoints import their request/response types from it. The crate does not exist yet (workspace is `app`/`frontend`/`server`/`src-tauri`), so standing it up is part of the data-access trait-split task.
- Native impl owns: token attachment + refresh (above), retry/timeout policy, and mapping HTTP errors into the same error type the sqlx impl produces.
- **Trait granularity: per-domain traits, one backend struct per target.** Callers depend on the narrow trait they use, so the trust boundary is visible in the type system — `CatalogStore` is anonymous-safe (the IA public routes), `CollectionStore` is session-scoped (the per-request `app.user_id` transaction). But *one* `HostedBackend` and *one* `NativeBackend` struct each implement every trait, so the native cross-cutting machinery above (token attach/refresh, retry/timeout, error mapping) has a single home rather than being duplicated per domain. Trait count (a caller concern) and impl-struct count (an implementer concern) are deliberately decoupled — the per-domain split costs nothing at the impl layer. Each trait maps 1:1 onto [collection-api](collection-api.md)'s endpoint domains (catalog search/browse vs. collection CRUD), so the native impl of a store is literally the client of that domain's endpoints.

## Alternatives considered

- **Native talks to Postgres directly with per-user credentials + RLS.** Rejected: per-user DB credential provisioning is heavy, connection limits on serverless Postgres punish many direct clients, and RLS becomes the sole authorization layer.
- **Everything (hosted included) calls Neon's Data API (PostgREST + JWT + RLS).** Removes the terminal API server entirely — every deployment makes authenticated calls to Neon. Genuinely uniform, but all authorization moves into SQL policies, business logic that needs transactions gets awkward, and we couple to PostgREST query semantics. Revisit only if maintaining the hosted API becomes the dominant cost.
- **Hosted also calls its own API (full uniformity).** Rejected: loopback HTTP hop per request, no security benefit; something must hold credentials regardless.

## Open questions

- ~~Error-type unification: one error enum both impls map into.~~ **Resolved
  (implemented 2026-07-16):** `shared::ApiError` (`NotFound`/`Unauthorized`/
  `Forbidden`/`Conflict`/`Validation`/`Upstream`), each variant carrying a
  message, with `http_status()`/`code()` and a `{error:{code,message,details?}}`
  wire envelope (`to_wire`/`from_wire`). The hosted routes serialize it; the
  native client reconstructs it from status + body. Shape per
  [collection-api](collection-api.md) §Error model; the enum lives in the
  `shared/` crate. See Findings.
- ~~Trait granularity: one big store trait vs. per-domain traits.~~ **Resolved (2026-07-14 review):** per-domain traits with one backend struct per target (see Key rules). The prior citation to "collection-api's per-domain stores" was circular — `*Store` is this spec's vocabulary; collection-api has a per-domain *endpoint* split (catalog vs. collection), which is the real 1:1 the traits mirror.
- **Still open (deferred to the collection-api native-client work):** does
  SSR-on-first-load in the native app degrade gracefully when the embedded
  server is up but the hosted API is unreachable (rather than white-screen)? And
  the native impl's `401`-refresh needs a defined behavior for *unreachable* vs.
  *auth-error*. The trait layer maps a transport failure to `Upstream` and a
  `401` to `Unauthorized` today; the **silent JWT re-mint from `tr_session` on
  `401`** is stubbed (a `TODO` in `native.rs`) because the cookie-jar plumbing it
  needs lands with the session-scoped endpoints — filed as a Phase 4 follow-up.
- ~~Session token storage in Tauri (keychain vs. encrypted file)~~ **Resolved by [auth](auth.md):** the session lives as httpOnly cookies (`tr_session`/`tr_jwt`) on the embedded `127.0.0.1` server — the webview carries them like a browser. No keychain/encrypted-file store; the native data-access impl reads `tr_jwt` from that same cookie jar.

## Findings

- 2026-07-14 — **Spec reconciled with shipped auth (review, not implementation).**
  The draft predated the auth task (now done) and had drifted:
  - **The native binary already ships the sqlx path** — `src-tauri` builds `app`
    with `features = ["ssr"]`, and `ssr` implies `dep:sqlx` + `db.rs`. The
    feature split is now spelled out concretely (decompose `ssr`; sqlx moves
    behind a new `hosted` feature; native gets `ssr + native`).
  - **"Exactly one terminus" was absolute and now-false** — auth has the native
    app talking directly to Better Auth. Rescoped to *data* authorization, with
    identity named as the deliberate exception.
  - **Token forwarding is settled, not open** — the native impl forwards the
    Better Auth `tr_jwt` the hosted `AuthUser` extractor already verifies (JWKS,
    15-min EdDSA), refreshing from `tr_session` on `401`. No new native↔hosted
    token to design.
  - **Config baking added** — native reuses `TR_WEB_ORIGIN` for the hosted API
    origin, closing auth.md's explicit "native config plumbing belongs here"
    handoff.
  - **Struck OQ#4** (Tauri token storage) — resolved by auth's httpOnly-cookie
    session. Flagged that the `shared/` types crate (the drift guarantee's
    backbone) does not yet exist and needs an owner agreed with collection-api —
    ownership settled in the review below.

- 2026-07-14 — **Granularity resolved and `shared/` ownership settled (maintainer spec review).**
  - **OQ#1 (trait granularity) resolved: per-domain traits, one backend struct
    per target.** Callers get interface segregation and a type-visible trust
    boundary (`CatalogStore` anonymous-safe, `CollectionStore` session-scoped); a
    single `HostedBackend`/`NativeBackend` struct implements every trait so the
    cross-cutting native machinery lives once. The old OQ's "matches
    collection-api's per-domain stores" was a circular citation (collection-api
    has no stores — it has a per-domain *endpoint* split); corrected in Key rules.
  - **`shared/` types crate ownership → this spec** (was an open standoff with
    collection-api). It's the drift guarantee's home; collection-api imports its
    endpoint types. Standing it up folds into the trait-split task.
  - **collection-api scheduled.** It was load-bearing but had zero queue tasks
    while four specs (data-model + ui-design — both *accepted* — plus
    catalog-search and this one) defer real decisions to it. A Phase 4 "flesh out
    collection-api" task was added, co-designed with this spec (native client ⇄
    endpoints; `shared/` types). Dependency direction made canonical one-way:
    **collection-api `Depends on:` this spec** (for the `shared/` types + trait
    seam); the reverse coupling (this spec's native impl is a client of
    collection-api's endpoints) stays prose here, not a `Depends on:`, so there
    is no mutual dependency to deadlock queue gating.

- 2026-07-16 — **Trait layer implemented; the seam is real and proven.**
  Built the whole seam and retired the spike DB access. What shipped:
  - **`shared/` crate** (new workspace member, package `shared`) — the DTOs
    (`CatalogCount`, `CollectionSummary`/`CollectionKind`) and `ApiError` (+ the
    `{error:{code,message,details?}}` wire envelope). Deliberately platform-neutral
    (serde/uuid/serde_json/thiserror only): it compiles for the wasm hydrate
    frontend *and* both server backends, so it carries no sqlx/axum — the hosted
    side maps `ApiError::http_status()` (a plain `u16`) onto its status type.
  - **`ssr` decomposed into a substrate + two backends.** `ssr` now = the router,
    `leptos_axum`, Axum, the auth core (JWKS verify + the Better Auth cookie
    proxy), serde. `dep:sqlx` + `db.rs` moved behind a new **`hosted`** feature;
    the HTTPS client is behind **`native`**; both imply `ssr`. `server` builds
    `app` with `hosted`, `src-tauri` with `native` — so **the Tauri binary no
    longer links sqlx at all** (the spike debt this task retired). A
    `compile_error!` makes `ssr` without a backend fail loud.
  - **Per-domain traits** `CatalogStore` (anonymous) / `CollectionStore`
    (session-scoped), each implemented by **one** `HostedBackend` and **one**
    `NativeBackend`. The backend struct carries the per-request session credential
    (hosted: the verified `user_id`; native: the forwarded `tr_jwt`), so trait
    methods take no credential argument.
  - **Per-request `SET LOCAL app.user_id` transaction** on the hosted side:
    `scoped_tx` opens a tx and runs `set_config('app.user_id', $1, true)` (the
    bound, injection-safe `SET LOCAL` form) before every session-scoped query, so
    data-model's RLS policies scope the rows beneath the API terminus. An
    anonymous handle answers a `CollectionStore` call with `Unauthorized`.
  - **Endpoints are trait methods projected to HTTP.** The web UI calls Leptos
    server fns (`catalog_count`, `list_collections`) that dispatch to the backend
    **in-process** (honoring "one terminus"); the native client is the HTTP client
    of explicit hosted JSON routes (`GET /api/catalog/count`, `GET /api/collections`)
    mounted only in the hosted deployment. Route paths live in one `paths` module
    both sides share, so they can't drift.
  - **Config baking:** the native client's hosted-API origin is `TR_WEB_ORIGIN`
    (exported env wins) falling back to the baked production origin — mirroring
    auth's release defaults.
  - **Spike removed:** `app/src/db.rs`'s `card_count` probe, the `Card`/`get_cards`
    server fn, and the server-startup Neon probe are gone; the `/cards` page now
    renders the catalog count through the seam.

  **Verified.** Full merge gate reproduced in-container (fmt, all clippy lines
  incl. the two new ones — see below — test, `cargo leptos build --release`).
  Live against the Neon **dev** branch: `GET /api/catalog/count` → `{"cards":0}`
  (hosted `CatalogStore` → Neon); `GET /api/collections` with no token → `401`;
  the UI server fn `POST /api/catalog_count` → `{"cards":0}`; and a temporary
  in-process probe ran `HostedBackend::for_user(<real user>).list_collections()`
  end to end (begin → `set_config` → RLS SELECT → commit → `Ok`), then reverted.
  (Row-level correctness of the RLS scoping was already proven at the SQL level in
  the data-model migration task.)

  **CI note — the `native` arms need their own lint line.** `cargo clippy
  --workspace` unifies `app`'s features to `hosted + native`, and the backend
  selection resolves `#[cfg(all(native, not(hosted)))]` in favor of `hosted` — so
  the workspace line never compiles the native-only server-fn arms (they ship in
  the APK/`.dmg`). Added a dedicated `cargo clippy -p app --features native
  --all-targets` gate line; the bench line moved from bare `ssr` to `hosted`
  since a real server always carries a backend. Both are in `validate.yml` +
  `CLAUDE.md`.

  **Neon production migrated** (2026-07-16): `scripts/migrate.sh prod` applied
  `0002`–`0006` to the production branch — verified identical to dev (migrations
  1–6, the seven real tables, spike `cards.id` gone). Done before this PR merged
  (maintainer's call), so the still-deployed spike `/cards` degrades to its
  graceful error until Render redeploys this code — expected, home page
  unaffected. **Deferred (follow-up filed in TODO):** the native `401`
  silent-refresh from `tr_session` (stubbed — needs the session cookie-jar
  plumbing that lands with collection-api's native client) and the
  offline-degradation behavior (OQ#3). sqlx gained the `uuid` feature so the
  hosted backend decodes ids natively.
