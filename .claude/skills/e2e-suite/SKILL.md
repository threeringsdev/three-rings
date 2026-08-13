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
| `npm run probe:catalog-paging [limit]` | `/catalog`'s keyset paging walked end to end via `GET /api/catalog/search` — browse-all cross-checked against `GET /api/catalog/count`, plus one filtered query (`t:creature`) asserting no-dup-ids and monotonic `(name, oracle_id)` order (no independent count exists for an arbitrary query) |
| `npm run probe:android-collection` | `/my/collections/:id` on the real Android webview (needs the android-smoke attach) |
| `npm run probe:android-quick-add` | quick-add's ↑↓/⏎/⌥⏎ registry navigation on the real Android webview, via the bench (the panel itself is authed, so unreachable through the dev proxy) |
| `npm run probe:android-selection-tray` | selection tray + row checkbox on the real Android webview, via the bench (both hosting pages are authed) |
| `npm run probe:android-rail` | the filter sheet on the real Android webview — badge, facet click, scroll lock, and the Set picker's search + multi-select pick (the rail is only a sheet at phone width) |
| `npm run probe:android-needs` | `/my/collections/:id/needs` + `/my/shopping` route guards through the redirect shim, and the pick list's checkbox tap, on the real Android webview |
| `npm run probe:android-palette` | the ⌘K desktop gate reads `false` at phone width on the real Android webview, with `CommandDialog` itself proven working there (the negative check's positive control) |
| `npm run probe:android-my-root` | the My-cards root drill-down list on the real Android webview — the frame's row shape, 44 px touch targets and no sideways scroll on the real engine, and a real `Input.dispatchTouchEvent` tap on a row navigating to its `href` (via the bench; `/my` is authed and unreachable through the dev proxy), plus the anonymous `/my` guard bounce through the redirect shim |
| `npm run probe:android-header-kebab` | the collection-header `⋯` on the real Android webview — its 44 px tap target at phone width, a real `Input.dispatchTouchEvent` tap opening the shared `context_menu` (having aimed it first), the panel clamped inside the viewport, and a real tap on `Move to…` running its `on_select` (via the bench; `/my/collections/:id` is authed and unreachable through the dev proxy) |
| `npm run probe:android-states` | the shared error banner on the real Android webview — all four failure classes rendering the affordances their class warrants and no others, a real `Input.dispatchTouchEvent` tap on `Try again` running its callback, no sideways scroll at phone width, and the `success`/`warning`/`info` tone badges resolving to real computed colors (a token family with no CSS behind it emits classes that do nothing). Via the bench: every surface these arms live on is authed, and an *expired session* arm cannot be induced on one at all — the auth guard bounces the load first |
| `npm run probe:android-tap-targets` | the responsive audit's touch targets on the real Android webview — the row select control's own box (44 CSS px against a 16 px drawn checkbox), a real `Input.dispatchTouchEvent` on the *padded ring* outside that box selecting the row, a touch just outside the target not selecting (positive control), the bottom tab bar docked to the viewport floor with 44 px tabs, and the toaster's bottom edge clearing that tab bar. Via the bench for the row control (every page hosting one is authed) and `/catalog` for the shell chrome |
| `npm run probe:android-tree-move` | the tree's touch path on the real Android webview — a real tap on an `⋯`-shaped trigger opens the shared `context_menu` and a real tap on an item runs it, via the bench (the tree itself is authed); also measures whether a real long-press yields `contextmenu` (it does not) and that the rail drawer stays off screen at phone width |

One **diagnostic** rather than a probe (it reports, it does not pass/fail):
`npm run diag:resource-ids -- <collection-id> [slot…]` prints the serialized
`__RESOLVED_RESOURCES` slots a page carries, then does a real SPA navigation into
`/my` and reports whether it fetched. Reach for it whenever a client-side
navigation shows **stale or empty data with zero requests**: leptos'
`initial_value()` reads `__RESOLVED_RESOURCES[<next monotonic id>]` for every
`Resource::new` without checking `during_hydration()`, so a resource created
during a navigation can decode a payload belonging to the page you left. Passing
slot numbers `delete`s them before navigating, which identifies the colliding slot
by removal — that is how id 12 was pinned (see `AllCardsPayload` in
`app/src/my/all_cards.rs`).

**A probe covers what the browser tier structurally cannot.** `probe:paging`
exists because page size is fixed in the UI, so only the JSON route can ask for
a page small enough to iterate; it walks the whole set asserting no duplicate,
no skipped row, stable order, and exactly one terminal cursor. If you add a
probe, add its script line here — an unregistered probe is one nobody runs.

## Server lifecycle

Tests hit `http://localhost:3000` — the `cargo leptos watch --features
component-bench` server (repo root; reads `.env` → Neon **dev** branch)
still *binds* `127.0.0.1:3000` (`Cargo.toml` `site-addr`; do not touch that —
it's server config, not a test-suite concern) and `localhost` resolves to it,
so this is a client-side-only distinction.
Check `lsof -i :3000` before starting another; the bind happens *after* the
first build, so a fresh start takes minutes. Never point tests at prod.

**Must be `localhost`, not `127.0.0.1` (maintainer ruling, 2026-08-11).**
Neon Better Auth's "Allow Localhost" setting is ON for the Neon **dev**
environment and matches the literal `localhost` hostname — a request whose
`Origin` is `http://127.0.0.1:3000` now gets `403 INVALID_ORIGIN` on sign-in.
Every `end2end/` origin hardcode (`playwright.config.ts`'s `baseURL`, every
`.mjs` probe, `seed-e2e-user.sh`) was repointed to `http://localhost:3000`
accordingly (P6-060) — a probe or test still hitting `127.0.0.1:3000` will
either 403 on its own sign-in or, if it reuses the login fixture's
`storageState`, silently send no cookie at all (cookies are host-scoped, and
the fixture's cookies are now scoped to `localhost`, not `127.0.0.1`). New
probes must use `localhost:3000`, never `127.0.0.1:3000`.
**Caveat, so nobody "fixes" this backward:** prod (Render, Neon's main
branch) keeps Allow Localhost **OFF**. This is irrelevant to e2e, which never
targets prod (never point tests at prod, above) — recorded only so the
distinction doesn't get "corrected" into matching prod's setting.

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
  in** (ui-task-loop step 4): `npx playwright test --project=chromium
  --workers=1` — **serial remains the full-pass mode** (12.4m measured 2026-08-11). P6-060's
  helper retires the hydration-click mechanism, but post-bulk-load (116,695
  printings vs the 2,976 the suite grew up on) parallel full runs fail ~29
  tests from shared-dev-server contention — a different subset each run, every
  sampled failure green solo. The filed contention task (WB-01KZRZ0TT7) owns changing this
  policy; until it lands, parallel full passes prove nothing. **Serial full
  pass is the verification mode (~13 min). Post-reconciliation baseline
  (2026-08-11): 247/259. The 12 residual failures, enumerated — fixture-pool
  class (WB-01KZMVA2Y1): batch-move :242/:315/:366/:435, command-palette
  :442/:484/:524, all-cards :270 (location summary), tree-manage :419
  (pool-growth count). Order-sensitive, proven green solo, owned by the
  flake task (WB-01KZNPJC9S): undo-restore :208, removal :369, tree-manage
  :538 (Escape/picker). Judge a task's run against that enumeration: a NEW
  failure outside it is the task's problem; these twelve are not. Delete
  this note when both owning tasks land.**

  **The hydration-click flake is handled now, test-side — `retryUntil` /
  `clickUntil` in `end2end/tests/helpers.ts`.** `hydrated(page)` waits on the
  global `<html data-hydrated>` stamp, which does not imply a *streamed*
  `<Suspense>`/`<Transition>` island, or a global listener (⌘K's document
  keydown, the router's click-delegate), is wired yet; under worker pressure
  an interaction can land before its handler is attached and is silently
  swallowed — Playwright reports a normal, successful action (element
  visible/enabled/stable) while the app never reacts. `clickUntil(locator,
  check)` retries the interaction itself against its own declared effect
  instead of clicking once and waiting passively on an effect that may never
  arrive; `retryUntil` is the general form for non-click actions. Deliberately
  test-side, not an app-side readiness marker — see the helper's own doc
  comment for why (the swallowed sites span mechanisms too different for one
  app stamp to cover honestly). Applied to the four originally-flaky sites
  (`smoke.spec.ts`'s Mode-switch/bottom-tabs tests, `collection-tree.spec.ts`'s
  tree-badge/chevron/row-click tests, `selection-tray.spec.ts`'s row-select
  tests) plus obviously-identical neighbors in those same three files — the
  convention + those sites, not a suite-wide sweep. Honest scope note: in the
  2026-08-11 measurement runs the four named sites failed in NO parallel run —
  the dominant parallel-failure class had already shifted to post-bulk-load
  server contention (see the policy bullet above) — so the helper is shipped
  as the standing convention for island interactions, not as the fix for
  today's parallel failures. Evidence in specs/ui-work-loop.md Findings,
  "Hydration-aware island clicks" (P6-060).

  **Why parallel still fails (both filed):** (a) the **contention** class —
  post-bulk-load pages query 40× more data, so 9 workers push the shared dev
  server into timeouts suite-wide (~29 failures/run, disjoint subsets, all
  green solo); and (b) the **fixture-pool** class: many spec files write
  through the *same single* seeded e2e user against the *same* shared Neon
  dev branch (holdings, collections, `LastMoveState`), so concurrent workers
  race each other's writes to the same rows — data problems
  `retryUntil`/`clickUntil` cannot fix, since retrying a click faster does
  not make two workers agree on one row's count.
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
