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
   `/api/me` probe route (401 without a valid token). Still to wire in this task:
   `SET LOCAL app.user_id` (arrives with the data-model per-request tx) and the
   cookie token source (step 6, path B). 401 on missing/invalid confirmed.
6. Sign-in/up/refresh proxy + httpOnly cookie session (path B); minimal Leptos
   `/login` + `/signup` screens per the wireframes.
7. **Tauri token spike (host-side):** confirm the cookie session round-trips through
   the embedded Axum server in the webview on desktop + Android.
8. Decide the deferred toggles (email verification, password reset, OAuth social) and
   record in Findings.

## Open questions

Accepted with these deferred to this task's execution; none blocks acceptance.

- Spike (still needed): session/token behavior inside Tauri webviews on all
  platforms vs. the browser, for the chosen frontend path (B). *(resolved during
  execution — this task)*
- Frontend fork **(A) client-side Better Auth + Bearer JWT** vs. **(B) server-side
  proxy + our own httpOnly cookie** — leaning B; settle with the Tauri spike.
  *(resolved during execution — this task)*
- Email verification / password reset — Better Auth covers these (currently off:
  `verify_email_on_sign_up=false`); decide what to turn on for the prototype.
  *(resolved during execution — this task)*
- OAuth social login — Google is available (shared credentials) out of the box;
  worth enabling for v1? *(resolved during execution — this task)*
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
