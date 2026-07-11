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
