# Auth

**Status:** accepted
**Depends on:** —

## Problem

Users need accounts; the API must authenticate every request. Clients never hold database credentials.

## Scope

In: signup/login, session or token management, API middleware. Out: authorization rules for sharing (future spec).

## Design

**Provider decided (2026-07-11, maintainer): [Neon Auth](https://neon.com/docs/neon-auth/overview),
which is [Better Auth](https://www.better-auth.com/) hosted by Neon** (verified
against the live project — *not* Stack Auth, as an earlier draft of this spec
assumed). A hosted provider, not roll-our-own. Rationale: it lives on the Neon
project we already use, keeps its tables **directly** in a `neon_auth` schema in our
database (so the data model references users with a normal `uuid` FK), and hands us
signup/login/session/verification/social-login off the shelf. **It is already
provisioned** on both the `production` and `dev` branches (see Findings).

- **Users table = `neon_auth."user"`** (Neon/Better-Auth-managed; do **not** migrate
  it — quote the name, `user` is reserved). Contract the data model depends on
  ([data-model](data-model.md) → *Users & the auth boundary*): `id uuid` PK, `email`,
  `name`, `emailVerified`, `image`, `createdAt`, `updatedAt` (+ admin `role`/`banned`
  fields). It's the **live** table (same DB/branch/transaction), so a hard FK with
  `ON DELETE CASCADE` is correct — no async-sync race, no soft delete. Sibling tables
  in the schema: `session`, `account`, `verification`, `jwks`, `organization`.
- **The hosted Axum API stays the authorization terminus** (per
  [data-access-backends](data-access-backends.md)). The API validates the Better Auth
  session and sets the `app.user_id` GUC that RLS reads — we do **not** adopt Neon's
  Data API / JWT-RLS path, which would move enforcement out of the API.
- **Token strategy across clients.** Confirm the session works identically in the
  browser and inside Tauri webviews on every platform (cookie handling differs in
  Tauri) — the spike below.
- **RLS app role.** Provision a non-owner application role for the runtime (owners
  bypass RLS); wiring `DATABASE_URL` to it is an ops step shared with data-model.

### Integration architecture

Better Auth issues **standard JWTs** (a JWKS is served per branch), so the two halves
of our stack integrate differently:

- **Backend verification (Axum) — Rust-native, no JS.** Requests to `/my/*` and
  mutations carry the user's JWT; middleware verifies it **locally against the
  branch's JWKS URL** (issuer = the Neon Auth base-URL origin), pulls `sub` (=
  `neon_auth."user".id`, a uuid), and opens the per-request transaction that
  `SET LOCAL app.user_id = sub` for RLS. Crates: `jsonwebtoken` + a cached JWKS
  client, or `axum-jwks`. Tokens are short-lived → refresh required. This is the
  confident part.
- **Frontend token acquisition (Leptos) — the real design fork.** Better Auth is a
  JS-first, cookie-session library exposing a REST API at its **base_url** (a
  *different origin* from our app — e.g. `…neonauth….neon.tech`), plus a JWT-plugin
  `/token` endpoint that mints the JWT our backend verifies. Standard Better Auth
  routes (confirm against the live base_url): `POST /sign-up/email`,
  `POST /sign-in/email`, `GET /get-session`, `GET /token`, `POST /sign-out`,
  `/sign-in/social` for OAuth. Two viable paths:
  - **(A) Client-side Better Auth** — the browser talks to base_url (CORS +
    credentials; needs our origins in `trusted_origins`), gets a JWT via `/token`,
    sends it Bearer to our Axum API. Fast, but cross-origin cookies + a client-held
    token, and SSR/Tauri get awkward.
  - **(B) Server-side proxy + our own cookie (recommended, pending spike)** — our
    Axum server calls Better Auth server-to-server, then sets its **own httpOnly
    session cookie** on our origin and refreshes server-side. Browser and Tauri
    webview carry only *our* cookie; the frontend stays pure Rust and SSR-friendly.
    OAuth still redirects through Better Auth's hosted flow. Cost: we implement the
    sign-in/up/refresh proxy + cookie session.
  - **Recommendation (B)** — it collapses the Tauri problem into the one cookie
    question we must answer anyway, rather than a second cross-origin token store.
- **Config (all non-secret, per branch).** Neon Auth exposes a **base_url** (→ JWT
  issuer) and a **jwks_url** — different per branch (dev vs. production), mapping onto
  our existing Neon branch split (`.devcontainer/.env` → dev; Render → production).
  No publishable/secret app key is surfaced (Neon hosts the auth service), so basic
  sign-in + JWKS verification need **no committed secret**. `trusted_origins` is
  currently empty and must be set to our app origins before cross-origin browser
  calls work; `allow_localhost` is already on.

### Implementation plan

1. ~~Enable Neon Auth~~ **Done** — already provisioned on both branches (2026-07-06
   production, 2026-07-11 dev). Config captured in Findings.
2. ~~Create the non-owner app role~~ **Done** — `app_runtime` on both branches
   (2026-07-12). Credential still to be set in the Console + placed in Render/local env.
3. ~~Set `trusted_origins`~~ **Done** — Render URL on production; `allow_localhost`
   covers dev. Add the per-branch `base_url`/`jwks_url` to app config (`.env.example`
   has the dev URL; Render gets production).
4. ~~Move migrations out of the app~~ **Done** (PR #3): `MIGRATOR.run` dropped from
   startup; `server --migrate` step added (reads owner `MIGRATION_DATABASE_URL`).
   Delivery is **Option B** — [`scripts/migrate.sh`](../scripts/migrate.sh) run
   manually from the dev container (Render free tier has no pre-deploy hook; owner
   cred stays off Render + CI). See [data-model](data-model.md) → Migration plan.
   Still pending (maintainer, ops): set the `app_runtime` passwords and rotate
   Render's `DATABASE_URL` → `app_runtime`.
5. ~~Axum JWKS-verify middleware (EdDSA)~~ **Verification core done** (2026-07-12,
   `app/src/auth.rs`): fetch+cache the branch JWKS, verify EdDSA signature +
   issuer + expiry, extract `sub` uuid, expose an `AuthUser` bearer extractor;
   `/api/me` probe route (401 without a valid token). **Correction 2026-07-13:**
   this core silently 401'd every *real* token (`InvalidAudience` — see
   Findings); fixed in step 6. `SET LOCAL app.user_id` still arrives with the
   data-model per-request tx.
6. ~~Sign-in/up/refresh proxy + httpOnly cookie session (path B); minimal Leptos
   `/login` + `/signup` screens~~ **Done 2026-07-13** — `app/src/auth/upstream.rs`
   (Better Auth REST client), `app/src/auth/cookies.rs` (our `tr_session` /
   `tr_jwt` / `tr_challenge` httpOnly cookies), `app/src/account.rs` (server
   fns at stable `/api/*` endpoints), `app/src/auth_pages.rs` (`/login`,
   `/signup`, OTP step, home-footer status), `/auth/callback` route (Google).
   All flows verified live against the dev branch — see Findings.
7. **Tauri token spike (host-side)** — desktop `.app` built and run: the
   embedded Axum serves the auth server fns on its dynamic `127.0.0.1` port and
   the upstream accepts that origin for the password/OTP flows (verified by
   driving the embedded server directly); webview cookie round-trip result in
   Findings. Android leg deferred (CI builds it; same webview family as the
   spiked desktop path).
8. ~~Decide the deferred toggles~~ **Decided 2026-07-13 (maintainer):** email
   verification **on** (`require_email_verification=true`, OTP method, both
   branches — flipped via Console), Google OAuth **on** (was already enabled,
   shared credentials, both branches). Password reset not yet wired — queued
   as a follow-up task in TODO.md.

## Open questions

Accepted with these deferred to this task's execution; none blocks acceptance.

- ~~Spike: session/token behavior inside Tauri webviews vs. the browser~~
  **Resolved (2026-07-13, desktop):** same-origin httpOnly cookies on the
  embedded `127.0.0.1` server work in the webview — the full gated
  sign-up → OTP → auto-sign-in flow passes in the app window. Google is
  unavailable *inside* the webview (upstream rejects `127.0.0.1` callback
  URLs; Google blocks OAuth in embedded webviews) — the desktop path is the
  system browser + loopback callback to the embedded server, designed in
  Findings and queued as a Phase 3 task.
- ~~Frontend fork (A) vs. (B)~~ **Resolved: (B) server-side proxy + our own
  httpOnly cookies**, implemented and verified — including Google, which stays
  path B via the verifier/challenge exchange (no cross-origin cookies
  anywhere; see Findings).
- ~~Email verification / password reset~~ **Resolved:** verification **on**
  (OTP; `require_email_verification=true` on both branches). Password reset
  exists upstream (`/forget-password`) but isn't wired — follow-up task.
- ~~OAuth social login~~ **Resolved:** Google **on** (shared credentials, both
  branches); web flow verified live. Own Google credentials only needed if we
  outgrow the shared ones (branding/quotas).
- Google-first account + email/password: signing up (or in) with an email that
  already has a Google identity fails `User already exists`, with no way to
  attach a password. What does the hosted service expose for adding a
  credential to a social-only account — the email-otp `forget-password` reset
  on a credential-less user, a set-password call, or account-linking config?
  Probe live, like everything else here. *(queued as a Phase 3 task alongside
  the password reset flow)*
- Self-hosting / lock-in: Neon Auth is hosted, but the engine is **Better Auth**
  (MIT, fully self-hostable) and the schema lives in our own DB — a real exit path.
  *(accepted risk — Better Auth OSS is the fallback)*
- ~~Hard FK vs. soft reference to the users table~~ **Resolved:** `neon_auth."user"`
  is a live uuid-keyed table with hard deletes → hard FK + `ON DELETE CASCADE`
  (the async-sync/soft-delete concern was a Stack Auth artifact and doesn't apply).

## Findings

- 2026-07-11 — **Integration architecture researched** (task start; desk research,
  before Neon MCP access). Conclusion that the backend verifies JWKS-issued JWTs and
  the frontend is the open fork **still holds**, but the specifics were **superseded**
  the same day (below): this entry assumed **Stack Auth** (text `users_sync.id`,
  `/api/v1/auth/...` REST, publishable/secret keys) — Neon Auth is actually **Better
  Auth**, so those details are wrong. Kept for history. Sources were the Neon/Stack
  Auth docs.
- 2026-07-11 — **Ground truth via Neon MCP (supersedes the above).** Connected the
  Neon plugin and inspected the live project `steep-scene-29832344` ("three rings",
  PG 18). Findings:
  - **Neon Auth = Better Auth**, and **already provisioned on both branches** —
    `production` (`br-icy-haze-atbrqruq`, since 2026-07-06, at project creation) and
    `dev` (`br-empty-butterfly-atqr162u`, since 2026-07-11). Each branch has its own
    auth service + URLs. Nothing to "turn on".
  - **Schema** (`neon_auth`, verified): `"user"` (**`id uuid`**, email, name,
    emailVerified, image, createdAt, updatedAt, role/ban fields), plus `session`,
    `account`, `verification`, `jwks`, `organization`/`member`/`invitation`,
    `project_config`. It's the **live** table — corrects the earlier text-id /
    `users_sync` / async-sync / soft-delete assumptions (all Stack Auth artifacts).
    Data-model updated: `user_id uuid REFERENCES neon_auth."user"(id) ON DELETE CASCADE`.
  - **Config** (non-secret, per branch): base_url → JWT issuer, jwks_url → verify key.
    - dev: `https://ep-dry-cell-atj9rpc2.neonauth.c-9.us-east-1.aws.neon.tech/neondb/auth`
    - production: `https://ep-curly-pond-atsb6fgp.neonauth.c-9.us-east-1.aws.neon.tech/neondb/auth`
    - `email_password` enabled, `allow_sign_up=true`, email verification **off**;
      Google OAuth available (shared creds); `trusted_origins=[]` (must be set);
      `allow_localhost=true`. **No app secret key surfaced** — Neon hosts the service.
  - **Remaining** (task stays `[~]`): the non-owner RLS role (step 2, ops), set
    `trusted_origins` (step 3), then the Axum JWKS middleware + cookie proxy + Tauri
    spike (steps 4–6). None blocked on the maintainer except the DB role.
- 2026-07-11 — **Ops progress + JWKS finding.**
  - **`trusted_origins`** — added the Render URL `https://three-rings-6p5o.onrender.com`
    to the production branch (CSRF + redirect allowlist); `allow_localhost=true`
    already covers `http://localhost:3000` for dev.
  - **App role** — `neondb_owner` has `USAGE`/`SELECT`/`REFERENCES` on
    `neon_auth."user"`, so the hard FK is confirmed workable. Non-owner
    `app_runtime` role **created 2026-07-12 on both branches** (LOGIN, CRUD +
    default privileges on `public`, RLS-subject; no superuser/bypassrls/DDL),
    initially **without a password**. Set the credential via the branch **SQL
    Editor** — `ALTER ROLE app_runtime PASSWORD '…'` — **not** the Console's
    role UI: Neon's control plane can't set/reset the password of a role created
    in SQL (the Console shows the role's connection string but with no password),
    so the SQL Editor is the way. Then paste the password into the
    Console-provided string (`postgresql://app_runtime:<pw>@<host>/neondb?sslmode=require&channel_binding=require`;
    drop to `channel_binding=prefer` if sqlx's SCRAM trips). Never the chat.
    Grants keep the current `public` schema working so the rotation is safe.
  - **Migrations move out of the app** (decided 2026-07-12, replacing the earlier
    dual-URL idea): the app never runs DDL. `MIGRATOR.run` leaves startup; migrations
    run as a **Render pre-deploy step** under `neondb_owner` (owner URL in Render env
    + local dev only, never GitHub). The web server runs as `app_runtime` with one
    credential. See [data-model](data-model.md) → Migration plan.
  - **JWKS algorithm = EdDSA / Ed25519** (`kty:OKP`, one key, `kid` present) on both
    branches → verify with `jsonwebtoken` (v9, EdDSA) building a `DecodingKey` from
    the OKP `x`; cache the JWKS and refresh on unknown `kid`. RSA-only JWKS helper
    crates (and possibly `axum-jwks`) won't handle OKP, so use `jsonwebtoken`
    directly. Middleware config: issuer = base-URL origin, `sub` = user uuid.
- 2026-07-12 — **JWKS middleware built** (`app/src/auth.rs`, ssr-only; step 5).
  `Verifier` fetches `<base_url>/.well-known/jwks.json`, caches by `kid`,
  refreshes lazily on an unknown `kid` (covers rotation), and verifies
  `Algorithm::EdDSA` + issuer + `exp`; `DecodingKey::from_ed_der(&x)` takes the
  raw base64url `x` (32 bytes) despite the `_der` name. An `AuthUser`
  `FromRequestParts` extractor yields the `sub` uuid; a `/api/me` probe route
  returns it (401 otherwise). New deps (all ssr-optional): `jsonwebtoken`,
  `reqwest` (rustls, no OpenSSL), `base64`, `uuid`. Live dev JWKS confirmed to
  match the parser: one key `{kty:OKP, crv:Ed25519, alg:EdDSA, kid, x}`.
  - **Unconfirmed until a real token exists:** the exact `iss` claim value.
    Better Auth may set it to the full base_url *or* its origin, so the verifier
    accepts **both** for now; narrow it to the observed value during step 6
    (sign-in flow), when a live token is available to inspect.
  - Deliberately **not** wired yet: `SET LOCAL app.user_id` (needs the
    data-model per-request transaction) and the httpOnly-cookie token source
    (step 6, path B) — the extractor reads `Authorization: Bearer` today, which
    the cookie proxy will feed later. End-to-end signature verification against a
    real signed token is deferred to step 6 for the same reason.
- 2026-07-13 — **Path B shipped and verified live (step 6); toggles decided
  (step 8); desktop spike run (step 7).** The whole surface was probed against
  the live dev auth service before implementation; everything below is
  observed behavior, not documentation.
  - **The step-5 verifier rejected every real token** — `jsonwebtoken` v9
    fails tokens carrying an `aud` claim unless an expected audience is
    configured (`InvalidAudience`), and live tokens carry `aud`. Both `iss`
    and `aud` are the base URL's **origin** (no `/neondb/auth` path; live
    token inspected), so the verifier now validates both against the origin.
    Real-token verification is exercised on every sign-in (the proxy verifies
    each JWT it mints). JWT lifetime is **15 min**; sessions are **7 days**;
    claims carry `email`/`name`/`emailVerified` (used for `CurrentUser`).
  - **Upstream mechanics (all verified by probe):** every state-changing call
    must send an `Origin` header the service trusts (`MISSING_ORIGIN`
    otherwise) — the proxy derives it from the request's
    `x-forwarded-proto`/`host`; sessions arrive only as the
    `__Secure-neon-auth.session_token` Set-Cookie (body `token` is the
    unsigned form; bearer replay is *not* accepted — cookie replay is), and
    `GET /token` mints the JWT. OTP endpoints:
    `POST /email-otp/send-verification-otp` + `POST /email-otp/verify-email`.
  - **Google stays pure path B.** Mechanism read from Neon's own server SDK
    (`neon-js` `packages/auth/src/server/middleware/oauth.ts`) and confirmed
    live: `POST /sign-in/social` returns the hosted-flow URL *and* a
    `session_challange` cookie (upstream's typo) that we re-host as our
    httpOnly `tr_challenge`; the browser returns to
    `/auth/callback?neon_auth_session_verifier=…`; the server exchanges
    verifier + challenge via `GET /get-session?…` for the session. No
    cross-origin cookies at any point (the naive "fetch `/token` from the
    browser after callback" bridge would die on partitioned/blocked
    third-party cookies — this is why the SDK flow was worth excavating).
    **Verified end-to-end with a real Google consent** (web, dev branch).
  - **Our cookies** (httpOnly, SameSite=Lax, `Secure` when forwarded-proto is
    https): `tr_session` (upstream session value, 7 d), `tr_jwt` (15 min),
    `tr_challenge` (10 min, OAuth window only). `AuthUser` reads Bearer first,
    then `tr_jwt`; `current_user` re-mints an expired JWT from `tr_session`
    transparently (the refresh path — verified: session-only request returns
    the user and a fresh `tr_jwt`).
  - **Verification toggles:** the Console's toggle maps to
    `require_email_verification` (the API's `verify_email_on_sign_up` stays
    `false` — behaviorally irrelevant: with require on, sign-up creates the
    account, issues **no** session, and **auto-mails the OTP**). Verified on
    the gated config: sign-up → `VerificationRequired` → mailed code →
    `verify-email` → auto-signed-in with cookies; unverified sign-in →
    `EMAIL_NOT_VERIFIED` → we re-send a code and surface the OTP step.
    OTP method beats link for this stack: the code is entered in-app, so the
    flow is identical in browser and Tauri webviews.
  - **Tauri desktop spike — PASS (2026-07-13, maintainer-confirmed):**
    release `.app` runs the embedded Axum on a dynamic `127.0.0.1` port; the
    full gated flow works *inside the WKWebView* — sign-up → "check your
    email" card → OTP → auto-signed-in footer — so the webview carries our
    httpOnly cookies exactly like a browser. **Google inside the webview is a
    dead end** — upstream rejects `127.0.0.1` callback URLs
    (`INVALID_CALLBACKURL`; literal `localhost` is accepted) and Google
    itself blocks OAuth in embedded webviews (`disallowed_useragent`, the
    same policy that makes every desktop app open the system browser) — the
    login page says so instead of no-op'ing. **Desktop Google plan (queued,
    TODO Phase 3):** open the flow in the *system browser* (Tauri v2 opener)
    with `callbackURL = http://localhost:<embedded port>/auth/callback` —
    both halves verified viable (upstream trusts `localhost` callbacks;
    Google allows loopback redirects for native apps). The one real change:
    the embedded single-user server must hold the OAuth *challenge* in
    memory between start and callback (the system browser never has our
    cookie); webview refetches auth state on window focus. Also noted:
    OAuth-created accounts have no password credential, so the same email
    can't password-sign-up ("User already exists") — Better Auth account
    linking exists if that UX ever matters (parked). Packaged release apps get
    no environment at all (Finder-launched `.app`, Android APK) — surfaced in
    the field as "Google sign-in isn't available right now" (the server fn
    500s with no `NEON_AUTH_BASE_URL`) — so the Tauri shell now bakes the
    **production** auth base URL as a release-build default (non-secret;
    exported env still wins, so terminal launches can point at dev). Fuller
    native config plumbing still belongs to data-access-backends.
  - **Regression caught: a unit `Suspense` fallback breaks hydration
    app-wide.** The first `AuthStatus` used `<Suspense fallback=|| ()>`;
    under leptos 0.8's out-of-order streaming that SSRs a `<!--<() />-->`
    marker which desyncs the hydration cursor — the wasm then panics
    ("expected a marker node", reported at the unrelated `Stylesheet`) and
    **all** interactivity dies on every route, not just the one rendering the
    component. Fix: any real element as fallback (`<span>"…"</span>`).
    Found/verified with a headless console-error probe
    (`end2end/hydration-check.mjs`); the full login→footer→sign-out UI drive
    is `end2end/auth-e2e.mjs` (playwright, ad-hoc — not wired into CI).
    Rule of thumb recorded: never use `()` as a `Suspense` fallback.
  - **Ops:** Render got `NEON_AUTH_BASE_URL` (production base URL, non-secret,
    set by maintainer). Host-side note: `cargo leptos watch` reads a root
    `.env`, but the bare server binary does not — and a `.env` with `&` in
    `DATABASE_URL` can't be shell-`source`d (parse it like
    `scripts/migrate.sh` does). Dev-branch test users left behind:
    `tr-spike-1@example.com`, `dylan.goings+tr-dev{1,2,3}@atomicobject.com`
    (harmless; clear from the Neon Auth console at will).
- 2026-07-13 (later) — **Production shakedown fixes + desktop Google built.**
  Maintainer testing on the deployed app surfaced four issues; root causes
  and fixes:
  - **First-time Google users bounced back signed out** (second attempt
    worked). No callback error was ever logged and `/auth/callback` was never
    reached — Better Auth routes *new* social users via `newUserCallbackURL`,
    which we didn't send, so first-timers skipped our exchange entirely.
    `social_start` now sends `callbackURL`, `newUserCallbackURL` (same URL),
    and `errorCallbackURL` (`/login?error=google`). Needs a fresh Google
    account (or a deleted user row) to re-verify in production.
  - **Sign-up had no Google option** — the button is now a shared
    `GoogleButton` component on both `/login` and `/signup` (social sign-in
    creates the account on first use, so one flow serves both).
  - **No way off `/login`//`/signup` without browser chrome** — both cards
    (and the OTP step) got a "← Back to home" link; native apps have no back
    button.
  - **Desktop/Android Google implemented** (the queued system-browser task):
    the Tauri shell exports `TR_EMBEDDED_ORIGIN=http://localhost:<port>`;
    `google_sign_in` then uses that for the upstream Origin + callback URL
    and parks the OAuth challenge in process memory
    (`app/src/auth/native.rs` — safe: the embedded server is single-user);
    the webview opens the flow in the **system browser** via the shell
    plugin (`withGlobalTauri` + `shell:allow-open`, called through
    `js_sys::Reflect`) and polls `current_user` every 2 s (≤3 min); the
    callback lands in the external browser on the embedded server, exchanges
    verifier + memory-challenge, parks the session, and shows "return to the
    app"; the poll claims the session and re-hosts it as webview cookies.
    Verified by simulation against the live dev service (localhost callback
    accepted; memory-challenge exchange fired upstream; bogus verifier
    correctly rejected `VERIFICATION_NOT_FOUND`), then **confirmed end-to-end
    on the desktop app by the maintainer** (browser opens → consent →
    "return to the app" → footer signed-in via the poll). Android rides the
    identical code path (loopback callback + opener Intent) — check on device
    from the next rolling APK.
  - **Getting the browser to open took three Tauri v2 lessons**, recorded so
    nobody relearns them: (1) `window.__TAURI__.shell.open` accepted the call
    and rejected the promise *silently* — surfaced by attaching a rejection
    handler that reports into the UI; (2) app-defined commands are ACL-gated
    like plugin commands ("Command open_url not allowed by ACL") — permissions
    must be *generated* via `tauri_build::AppManifest::commands` and allowed
    in the capability; (3) the actual root cause of every denial: our pages
    live on `http://127.0.0.1:<port>` / `http://localhost:3000`, which Tauri
    classifies as **remote** origins — capabilities don't apply there until
    the capability opts in (`remote.urls`). Also swapped the deprecated shell
    `open` for `tauri-plugin-opener` (the deprecation would fail CI's
    `clippy -D warnings`), and the server now loads a workspace `.env` at
    startup (dotenvy — host-side `cargo tauri dev`/`cargo leptos watch` get
    `DATABASE_URL`/`NEON_AUTH_BASE_URL` without shell gymnastics; dev-mode
    Google needs `TR_EMBEDDED_ORIGIN=http://localhost:3000` exported so the
    watch server holds the handoff state).
- 2026-07-13 (Android follow-up) — **Android Google return rebuilt as a deep
  link: a frozen backgrounded app cannot serve its loopback callback.**
  - **Diagnosis.** On-device test: the browser opened, Google authenticated,
    the upstream redirected to `http://localhost:36265/auth/callback?…verifier`
    — and Chrome *timed out*. A killed app would have refused the connection
    instantly; a timeout means the kernel completed the TCP handshake on the
    listening socket but the app never served the request. That is Android's
    cached-app freezer suspending the backgrounded app while Chrome was
    foreground. Desktop is unaffected (macOS doesn't freeze the app; its flow
    keeps working) — this is exactly RFC 8252's split: loopback redirects for
    desktop, app-claimed links for mobile.
  - **Rejected: custom scheme as the upstream callback.** Probed live:
    `POST /sign-in/social` with `callbackURL: "three-rings://auth/callback"`
    → `INVALID_CALLBACKURL`; the service only takes http(s). Verified App
    Links were also passed over — they need `assetlinks.json` + the signing
    cert fingerprint for no real gain here.
  - **Design shipped.** Android's `google_sign_in` (runtime
    `cfg!(target_os = "android")`, so host clippy still checks it) sends the
    *public web origin*'s `/auth/app-return` as both callback and error URL
    (probe-verified: the prod upstream accepts the Render callback with a
    `localhost` Origin header). That page (static HTML in `build_router`, no
    server state, query forwarded client-side from `location.search` so
    nothing user-controlled is interpolated) bounces onto
    `three-rings://auth/callback?…verifier` — JS auto-attempt plus a visible
    "Open the app" link, since Chrome may demand a user gesture for custom
    schemes. The deep link foregrounds the app (manifest already
    `singleTask`); `tauri-plugin-deep-link`'s `on_open_url` extracts the
    verifier and `app::auth::native::complete_google_return` runs the
    verifier + parked-challenge exchange in-process, parking the session for
    the webview's existing `current_user` poll. The intent-filter is injected
    into `gen/android`'s manifest at build time by the plugin from
    `tauri.conf.json` (`plugins.deep-link.mobile`), so CI's APK claims the
    scheme with no manifest edit.
  - **Notes.** The web origin is baked like the auth URL
    (`TR_WEB_ORIGIN` env wins; Render default in `account.rs`). Scheme
    squatting is harmless: a rogue app catching the verifier lacks the
    challenge, which never leaves our process. If Android *kills* (not
    freezes) the app mid-flow the parked challenge is lost and the exchange
    fails with a logged "start over" error — acceptable, the flow is
    restartable. The bounce page ships with the web deploy and the APK builds
    on the same merge, so both surfaces update together. Desktop keeps the
    loopback path untouched (the deep-link handler is registered there too
    but never exercised).
- 2026-07-13 (verified) — **Android deep-link return confirmed on device by
  the maintainer**: Continue with Google → Chrome → bounce page → app
  foregrounds signed in, on the *first* pass. Since the prod user row had been
  deleted beforehand, this same run re-proved the first-time-Google fix
  (`newUserCallbackURL`) in production. The auth surface is now verified on
  web, desktop, and Android; remaining auth work is queued in TODO Phase 3
  (password reset; adding a password to a Google-first account).
