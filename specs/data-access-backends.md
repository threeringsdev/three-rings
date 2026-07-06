# Data-access backends (hosted vs. native)

**Status:** draft
**Depends on:** data-model, auth

## Problem

The `app` crate's server functions run in two very different trust environments:

- **Hosted (web):** our infrastructure. Holding Postgres credentials is fine; this process is the authorization boundary.
- **Native (Tauri desktop/mobile):** the user's machine. The embedded Axum server must never hold direct Postgres credentials — anything shipped in the binary or its config is extractable.

Server functions must work identically in both, or the shared-crate architecture loses its point.

## Design

A data-access trait (per domain area, e.g. `CatalogStore`, `CollectionStore`) that server functions program against. Two implementations:

| | Hosted impl | Native impl |
|---|---|---|
| Transport | In-process sqlx against Neon | HTTPS to the hosted API |
| Credentials | `DATABASE_URL` from environment | User's session token (from auth) |
| Authorization | Enforced here (the terminus) | Delegated — hosted API enforces |

Selected at compile time via the existing cargo feature split (`ssr` + a `hosted`/`native` feature pair), so the native binary contains no sqlx/Postgres code path at all.

Key rules:

- **Exactly one terminus.** All authorization and credential-holding happens in the hosted deployment. The hosted impl does NOT call the API over loopback — that would add a serialization round-trip for zero security gain, since enforcement lives in the same process either way.
- The hosted API surface and the native impl's client are generated from/checked against the same shared types (see collection-api) so the two backends cannot drift.
- Native impl owns: token attachment, retry/timeout policy, and mapping HTTP errors into the same error type the sqlx impl produces.

## Alternatives considered

- **Native talks to Postgres directly with per-user credentials + RLS.** Rejected: per-user DB credential provisioning is heavy, connection limits on serverless Postgres punish many direct clients, and RLS becomes the sole authorization layer.
- **Everything (hosted included) calls Neon's Data API (PostgREST + JWT + RLS).** Removes the terminal API server entirely — every deployment makes authenticated calls to Neon. Genuinely uniform, but all authorization moves into SQL policies, business logic that needs transactions gets awkward, and we couple to PostgREST query semantics. Revisit only if maintaining the hosted API becomes the dominant cost.
- **Hosted also calls its own API (full uniformity).** Rejected: loopback HTTP hop per request, no security benefit; something must hold credentials regardless.

## Open questions

- Error-type unification: one error enum both impls map into — shape TBD.
- Trait granularity: one big store trait vs. per-domain traits (leaning per-domain).
- Does SSR-on-first-load in the native app work offline-tolerantly enough (embedded server up, but API unreachable) to degrade gracefully rather than white-screen?
- Session token storage in Tauri (keychain vs. encrypted file) — overlaps with auth.
