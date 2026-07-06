# Auth

**Status:** draft
**Depends on:** —

## Problem

Users need accounts; the API must authenticate every request. Clients never hold database credentials.

## Scope

In: signup/login, session or token management, API middleware. Out: authorization rules for sharing (future spec).

## Design

To be worked out. Starting considerations:

- Decide: roll our own (Axum middleware + argon2 + sessions) vs. a hosted provider (e.g. Clerk, Auth0) vs. an OSS self-hosted option.
- Token strategy must work identically for Tauri webview clients and the browser (cookie handling differs in Tauri — verify early).
- Refresh/long-lived sessions for desktop app UX.

## Open questions

- Email verification and password reset infrastructure — needed at prototype stage?
- OAuth social login worth it for v1?
