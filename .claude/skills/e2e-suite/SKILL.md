---
name: e2e-suite
description: Author and run the three-rings Playwright e2e suite — login fixture (email verification is ON; never sign up in a test), chromium-only tiers (@fast while iterating; one full chromium run per task after review fixes; firefox and webkit are never run), Android webview project, :3000 server lifecycle, assertions that lie, quarantine policy. Use when writing, running, or debugging any e2e test, or when a UI task reaches its e2e step.
---

# E2E suite — Playwright against the live dev server

Suite lives in `end2end/` (`npx playwright test` from there). The ad-hoc
`.mjs` probes (`hydration-check`, `bench-check`, `android-cdp-check`) are the
layer *beneath* the suite — cheap diagnostics, not tests. The ones a task may
need are `npm run` scripts in `end2end/package.json` so they are discoverable
rather than folklore:

| Script | What it proves |
|---|---|
| `npm run probe:hydration -- <url…>` | no console errors/warnings, anonymous |
| `npm run probe:hydration-authed -- <url…>` | same, signed in (needs `--project=setup` first) |
| `npm run probe:bench` | `/dev/components` SSR + hydration + tokens |
| `npm run probe:paging [limit]` | `/my` keyset paging walked at a small page size |
| `npm run probe:android-collection` | `/my/collections/:id` on the real Android webview (needs the android-smoke attach) |
| `npm run probe:android-quick-add` | quick-add's ↑↓/⏎/⌥⏎ registry navigation on the real Android webview, via the bench (the panel itself is authed, so unreachable through the dev proxy) |
| `npm run probe:android-selection-tray` | selection tray + row checkbox on the real Android webview, via the bench (both hosting pages are authed) |
| `npm run probe:android-rail` | the filter sheet on the real Android webview — badge, facet click, scroll lock, and the Set picker's search + multi-select pick (the rail is only a sheet at phone width) |
| `npm run probe:android-needs` | `/my/collections/:id/needs` + `/my/shopping` route guards through the redirect shim, and the pick list's checkbox tap, on the real Android webview |
| `npm run probe:android-palette` | the ⌘K desktop gate reads `false` at phone width on the real Android webview, with `CommandDialog` itself proven working there (the negative check's positive control) |

**A probe covers what the browser tier structurally cannot.** `probe:paging`
exists because page size is fixed in the UI, so only the JSON route can ask for
a page small enough to iterate; it walks the whole set asserting no duplicate,
no skipped row, stable order, and exactly one terminal cursor. If you add a
probe, add its script line here — an unregistered probe is one nobody runs.

## Server lifecycle

Tests hit `http://127.0.0.1:3000` — the `cargo leptos watch --features
component-bench` server (repo root; reads `.env` → Neon **dev** branch).
Check `lsof -i :3000` before starting another; the bind happens *after* the
first build, so a fresh start takes minutes. Never point tests at prod.

**Release-build clobber trap (isolated for the gate):** a `cargo leptos build
--release` that writes to the default `target/site` overwrites `target/site/pkg`
while the debug watch server keeps serving — debug SSR HTML + release wasm = a
tachys "unreachable" hydration panic on every page, and forms silently fall back
to native POSTs (302s). Touching a source file does NOT reliably restore the
frontend half (observed: server rebuilt, stale wasm still served). The **validate
skill's gate command now runs the release build under
`CARGO_TARGET_DIR=target/gate`** (`site-root` is the `CARGO_TARGET_DIR/site`
marker in `Cargo.toml`), so it writes to `target/gate/site` and can't touch the
running watch — verify-after-gating no longer needs a restart. A **bare**
`cargo leptos build --release` still clobbers `target/site`: give it the same env
var, or restart the watch afterward.

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

**Chromium only. Firefox and webkit are not run at all** (maintainer decision,
2026-07-25 — this is an MVP with one developer; cross-browser compatibility is
deferred wholesale, and WKWebView gets covered by desktop later). Never invoke
a bare `npx playwright test` — it fans out across all three projects and is the
single biggest cost sink in the loop.

- `@fast` in the test title = the **iteration** tier, run as often as you like
  while building: `npx playwright test --project=chromium --grep @fast`.
  Implementers and reviewers use **only** this.
- The task's e2e run = **once per task, after the adversarial-review fixes are
  in** (ui-task-loop step 4):
  `npx playwright test --project=chromium --workers=1`. One pass, not one per
  round. A task is not `[x]` until it is green.

  **`--workers=1` is deliberate, not caution.** `hydrated(page)` waits on the
  global `<html data-hydrated>` stamp, which does not imply a *streamed*
  Suspense island is interactive; under worker pressure a click lands before
  the island is wired and is silently swallowed. Four tests flake on this —
  `smoke.spec.ts:92` and `:114`, `collection-tree.spec.ts:65`,
  `selection-tray.spec.ts:239` — a different subset each run, all passing in
  isolation. Measured 2026-07-25: default workers 2 failed / 141 passed in
  1.4 min, then 143/143 in 3.8 min single-worker. Since the parallel run needs
  a retry to be believed, it is not actually faster, and it costs a real
  signal. Revert this when the hydration-aware click helper lands (filed under
  Phase 5 discoveries).
  A **major** failure goes back to the implementer; a minor failure or flake is
  quarantined `@flaky` and filed under Phase 5 discoveries. (Stage boundaries
  add the Android **release** smoke, not a different browser tier.)
- Android webview project: attach per the **android-smoke** recipe, then run
  the spec against the attached page. One page, one context — Android runs
  serialize (single worker) and share state; every spec must `goto` its own
  start URL (`http://tauri.localhost/<path>`, never JS `location.href` — it
  races the CDP session).

## Assertions that lie

- **`toBeVisible()` on a `Sheet` is always wrong.** `SheetContent` slides via a
  transform and keeps its box when closed, so a *closed* sheet — and everything
  nested in it — is "visible" to Playwright. Assert
  `toHaveAttribute("data-state", "open"|"closed")` on the `[data-name=SheetContent]`
  element, not on a child.
- **`{..}` spreads land a `data-testid` on the backdrop as well as the panel.**
  Pair the testid with `[role=dialog]` or the locator resolves to two elements.
- **A closed `popover` is in the DOM.** `toBeHidden()` passes whether or not its
  children rendered, so it cannot test lazy mounting — assert on the content
  (e.g. a name appearing exactly once) instead.
- Both of the first two shipped as green-but-meaningless assertions and were
  only caught by mutation testing. **Mutation passes are switched off in the
  loop** (maintainer decision, 2026-07-25 — assertion strength gets its own
  sweep after the spec work lands), so these three patterns are now the
  checklist you apply *while writing* the assertion instead of a check that
  catches you afterward.
- A test can only distinguish two behaviors the **fixture** distinguishes. If
  no seeded collection has a rollup, a nested folder with children, or a
  repeated printing, then assertions about those are green no matter what the
  code does. Check the fixture actually contains the shape you are asserting
  on — this produced three vacuous tests in one task.
- Measure the **scroll container**, not the document, for overflow. An
  `overflow-auto` wrapper (e.g. `TableWrapper`) absorbs the overflow so
  `document.documentElement` never moves.

## Quarantine policy

Flake → one retry. Still flaky → tag `@flaky`, file a Findings entry in the
task's spec, move on. `@flaky` tests block the phase-final polish task — the
tag is a debt marker, not a mute button.
