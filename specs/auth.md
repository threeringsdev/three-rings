# Auth

**Status:** accepted
**Depends on:** —

## Problem

Users need accounts; the API must authenticate every request. Clients never hold database credentials.

## Scope

In: signup/login, session or token management, API middleware. Out: authorization rules for sharing (future spec).

## Design

**Provider decided (2026-07-11, maintainer): [Neon Auth](https://neon.com/docs/neon-auth/overview)**
(Stack Auth-backed) — a hosted provider, not roll-our-own. Rationale: it lives on
the Neon project we already use, provisions and maintains a `neon_auth.users_sync`
table *inside our database* (so the data model can reference users with a normal
FK), and hands us signup/login/session/verification/social-login off the shelf. The
rest of this section is the starting shape; the Phase 3 auth task fleshes it out
and this spec must reach `accepted` before it.

- **Users table = `neon_auth.users_sync`** (Neon-managed; do **not** migrate it).
  Contract the data model depends on ([data-model](data-model.md) → *Users & the
  auth boundary*): `id text` PK, `email`, `name`, `created_at`, `updated_at`,
  `deleted_at` (soft delete), `raw_json`. Two consequences it must honor: the id is
  **text** (all `user_id` FKs are text), and sync is **asynchronous** + deletes are
  **soft** — so a new user can act before their row lands and `ON DELETE CASCADE`
  never fires. Hard FK (handle the race) vs. soft reference is this task's call.
- **The hosted Axum API stays the authorization terminus** (per
  [data-access-backends](data-access-backends.md)). The API validates the Neon Auth
  session and sets the `app.user_id` GUC that RLS reads — we do **not** adopt Neon's
  Data API / JWT-RLS (`pg_session_jwt`) path, which would move enforcement out of
  the API.
- **Token strategy across clients.** Verify the Neon Auth / Stack Auth session works
  identically in the browser and inside Tauri webviews on every platform (cookie
  handling differs in Tauri) — the long-standing spike below, now scoped to the
  Neon Auth SDK.
- **RLS app role.** Provision a non-owner application role for the runtime (owners
  bypass RLS); wiring `DATABASE_URL` to it is an ops step shared with data-model.

### Integration architecture (researched 2026-07-11 — see Findings)

Neon Auth issues **standard JWTs**, so the two halves of our stack integrate very
differently:

- **Backend verification (Axum) — Rust-native, no JS.** Every `/my/*` and mutating
  request carries the user's access token; middleware verifies it **locally against
  the Neon Auth JWKS endpoint** (issuer = the Neon Auth URL origin), pulls `sub`
  (the `neon_auth.users_sync.id`), and opens the per-request transaction that
  `SET LOCAL app.user_id = sub` for RLS. Crates: `jsonwebtoken` + a cached JWKS
  client, or `axum-jwks`. Access tokens are short-lived (~10–15 min) → refresh is
  required (below). This is the confident part.
- **Frontend token acquisition (Leptos) — the real design fork.** Neon Auth's
  first-class SDKs are JavaScript (`@stackframe/js`/React); we're Rust/WASM. Stack
  Auth also exposes a **REST API** (`POST /api/v1/auth/password/sign-{up,in}`
  returning access + refresh tokens + user id; refresh at
  `POST /api/v1/auth/sessions/current/refresh` with an `x-stack-refresh-token`
  header; project id + publishable client key sent as headers). Two viable paths:
  - **(A) JS SDK island** — mount `@stackframe/js` for the sign-in UI + token
    lifecycle, hand the access token to the Leptos app. Fastest, but drops JS into
    an otherwise-Rust frontend and owns tokens client-side.
  - **(B) Server-side REST proxy (recommended)** — our Axum server drives the Stack
    Auth REST endpoints and sets an **httpOnly session cookie**, holding/refreshing
    the Stack refresh token server-side. The browser and the Tauri webview then
    carry only *our* cookie; the frontend stays pure Rust, refresh is centralized,
    and it's SSR-friendly. OAuth social login still redirects through Stack Auth's
    hosted pages. Cost: we implement the sign-in/up/refresh proxy + cookie session
    ourselves.
  - **Recommendation (B)**, pending the spike — it makes the Tauri story a cookie
    question we already have to answer, rather than a second token store.
- **Config & secrets (from enabling Neon Auth).** Neon Auth yields a project URL
  (→ JWT issuer + JWKS URL), a project id, a publishable client key, and a secret
  server key. Non-secret values bake into the image `ENV`; the secret server key
  becomes a Render env var + a `.devcontainer/.env` entry — **never committed**
  (mirrors the `DATABASE_URL` split).

### Implementation plan

1. **[maintainer / ops]** Enable Neon Auth on the Neon project (provisions
   `neon_auth.users_sync`); capture project URL / id / publishable key / secret key;
   add them to Render env + `.env.example` (+ local `.env`). *Gates everything
   below — no Neon control-plane access from the container.*
2. **[maintainer / ops]** Create the non-owner application role; point the runtime
   `DATABASE_URL` at it (migrations still run as owner). Shared with data-model.
3. Axum JWKS-verify middleware → `sub` → `SET LOCAL app.user_id`; 401 on
   missing/invalid.
4. Server-side sign-in/up/refresh proxy + httpOnly cookie session (path B); minimal
   Leptos `/login` + `/signup` screens per the wireframes.
5. **Tauri token spike (host-side):** confirm the cookie session round-trips through
   the embedded Axum server in the webview on desktop + Android (the standing spike).
6. Decide the deferred toggles (email verification, password reset, OAuth social)
   and the hard-FK-vs-soft-ref question; record in Findings.

## Open questions

Accepted with these deferred to this task's execution; none blocks acceptance.

- Spike (still needed, now Neon-Auth-scoped): session/token behavior inside Tauri
  webviews on all platforms vs. the browser. *(resolved during execution — this task)*
- Hard FK to `neon_auth.users_sync(id)` vs. soft reference, given async sync + soft
  delete (shared with data-model). *(resolved during execution — this task)*
- Email verification / password reset — Neon Auth covers these; decide which to turn
  on for the prototype. *(resolved during execution — this task)*
- OAuth social login — Neon Auth offers it; worth enabling for v1? *(resolved during
  execution — this task)*
- Self-hosting / lock-in: Neon Auth is hosted-only; acceptable, or keep an exit
  path? Stack Auth (the engine) is OSS and self-hostable — recorded as the exit path.
  *(accepted risk — Stack Auth OSS is the fallback)*

## Findings

- 2026-07-11 — **Integration architecture researched** (task start; desk research,
  no live Neon Auth instance yet). Established that Neon Auth = Stack Auth issuing
  standard JWTs, so the **backend is Rust-native**: verify the access token locally
  against the Neon Auth **JWKS endpoint** (issuer = Neon Auth URL origin, `sub` =
  `users_sync.id`, ~10–15 min expiry), extract `sub`, set the `app.user_id` RLS GUC
  — crates `jsonwebtoken` (+ cached JWKS) or `axum-jwks`. The **frontend** is the
  open fork because Neon Auth's SDKs are JS-only; Stack Auth's **REST API**
  (`/api/v1/auth/password/sign-{up,in}` + `/api/v1/auth/sessions/current/refresh`,
  authenticated with project id + publishable client key headers) makes a Rust-only
  path possible. Chose **server-side REST proxy + httpOnly cookie (path B)** as the
  lead approach so browser and Tauri carry only our cookie — pending the Tauri
  spike. Sources: Neon Auth [JWT](https://neon.com/docs/auth/guides/plugins/jwt) /
  [backend integration](https://neon.com/docs/neon-auth/concepts/backend-integration)
  docs; Stack Auth [backend integration](https://docs.stack-auth.com/docs/concepts/backend-integration)
  + [REST refresh](https://docs.stack-auth.com/rest-api/client/sessions/refresh-access-token).
- 2026-07-11 — **Blocked on Neon Auth enablement.** Implementation steps 3–6 need
  the project URL / id / keys that only enabling Neon Auth produces, and the
  container has no Neon control-plane access — so step 1 is a maintainer action.
  Flagged to the maintainer; task stays `[~]`.
