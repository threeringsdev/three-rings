---
name: e2e-suite
description: Author and run the three-rings Playwright e2e suite — login fixture (email verification is ON; never sign up in a test), tier tags (@fast per task, full tier at stage boundaries), Android webview project, webkit-as-WKWebView rationale, :3000 server lifecycle, quarantine policy. Use when writing, running, or debugging any e2e test, or when a UI task reaches its e2e step.
---

# E2E suite — Playwright against the live dev server

Suite lives in `end2end/` (`npx playwright test` from there). The ad-hoc
`.mjs` probes (`hydration-check`, `bench-check`, `android-cdp-check`) are the
layer *beneath* the suite — cheap diagnostics, not tests.

## Server lifecycle

Tests hit `http://127.0.0.1:3000` — the `cargo leptos watch --features
component-bench` server (repo root; reads `.env` → Neon **dev** branch).
Check `lsof -i :3000` before starting another; the bind happens *after* the
first build, so a fresh start takes minutes. Never point tests at prod.

**Release-build clobber trap:** the validate gate's `cargo leptos build
--release` overwrites `target/site/pkg` while the debug watch server keeps
serving — debug SSR HTML + release wasm = a tachys "unreachable" hydration
panic on every page, and forms silently fall back to native POSTs (302s).
Touching a source file does NOT reliably restore the frontend half
(observed: server rebuilt, stale wasm still served). **Restart the watch
after any release build** before trusting any browser-driven result.

## The auth fixture trap (the one that hangs)

Email verification is **ON** (`require_email_verification`, OTP method): a
naive signup fixture hangs forever waiting for an emailed OTP. **Never sign
up in a test.** Use the pre-seeded verified test user on the Neon dev branch
(created by the e2e-baseline task; credentials in `end2end/.env`, gitignored).
The login fixture drives the real `/login` form once per worker and reuses
`storageState` (captures the `tr_session`/`tr_jwt` httpOnly cookies):

- `input[name=email]` + `input[name=password]` + `button[type=submit]`,
  then wait for the redirect — same sequence as `auth-e2e.mjs`.
- Sign-out invalidates server-side session state — a test that signs out
  must not share storageState with later tests (isolate it).

## Tiers

- `@fast` in the test title = the per-task tier: `npx playwright test
  --project=chromium --grep @fast`.
- Full tier (stage boundaries, or any overlay/positioning change):
  `npx playwright test` — chromium + firefox + **webkit**. Webkit stands in
  for WKWebView (desktop is untested in-loop); an overlay that breaks in
  webkit will break in the macOS shell.
- Android webview project: attach per the **android-smoke** recipe, then run
  the spec against the attached page. One page, one context — Android runs
  serialize (single worker) and share state; every spec must `goto` its own
  start URL (`http://tauri.localhost/<path>`, never JS `location.href` — it
  races the CDP session).

## Quarantine policy

Flake → one retry. Still flaky → tag `@flaky`, file a Findings entry in the
task's spec, move on. `@flaky` tests block the phase-final polish task — the
tag is a debt marker, not a mute button.
