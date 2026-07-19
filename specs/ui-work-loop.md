# UI work loop — the per-task agent loop for Phase 5

**Status:** accepted
**Depends on:** [app-ui](app-ui.md), [delivery-pipeline](delivery-pipeline.md)

## Problem

Phase 5 is ~15 page/component tasks, each meant to be executed end-to-end by an
agent with minimal supervision. The merge gate (validate) proves code compiles
and unit tests pass, but nothing today proves a UI feature *works*: no runtime
verification loop, no adversarial review step, no e2e authoring convention, and
no automated native-platform check. This spec defines that loop — the mechanism,
the per-task step sequence, the failure policy, and the supporting skills — so
every Phase 5 task ships through the same discipline.

## Scope

**In:** the loop mechanism and step contract; the supporting skills
(`ui-task-loop`, `e2e-suite`, `android-smoke`) and the permission allowlist they
need; the e2e baseline reset (auth fixture, tiers) and the Android-e2e spike
that fixes the platform matrix.

**Out:** changing the merge gate (validate stays byte-identical to CI); desktop
macOS in-loop verification (debug Tauri points at the web dev server — it would
re-test the web path in a native window; release desktop checks remain a manual/
milestone activity); CI e2e (local-loop concern for now; promoting Playwright
into CI is a possible follow-up once the suite stabilizes).

## Design

### Mechanism: a repo skill, not a Workflow script

`.claude/skills/ui-task-loop/SKILL.md` (mirrored in `.agents/skills/`),
orchestrating in the **main session**. Rationale: the Codex plugin's review
commands (`/codex:adversarial-review`, `/codex:status`, `/codex:result`) are
main-session slash commands; the loop must hold long-lived host state (the
`cargo leptos watch` server, the Android emulator) across steps; and durable
checkpointing already exists (TODO states, branch, spec Findings) so a Workflow
engine adds nothing. Repetition across tasks is the human's call (e.g.
`/loop /ui-task-loop`), with human-in-the-loop recommended for the first few
iterations.

### Platform matrix — fixed by the Android-e2e spike (first task)

What's proven: the release APK runs on the emulator (spike 2026-07, manual
verification via taps + `adb forward`). What's unproven: *automated* e2e against
the Android webview. The spike attempts: Tauri **debug** builds enable webview
remote debugging → `adb forward tcp:<port> localabstract:webview_devtools_remote_<pid>`
→ Playwright `connectOverCDP` (or Playwright's experimental `_android` webview
API). Decision tree (maintainer, 2026-07-17):

1. Android webview e2e works → **web + Android e2e every task**; no stage
   boundaries; desktop ignored.
2. Only release-build smoke is viable → **tiered**: web e2e every task; Android
   release install/launch/adb smoke at stage boundaries (shell, catalog
   complete, my-cards complete, final); desktop ignored.
3. Neither → **web only** in-loop; native verified manually at milestones.

The spike's outcome is recorded in Findings here and baked into the skills.
Note for path 1: debug Android builds point at the host dev server (devUrl), so
webview e2e exercises real Android rendering/input over the same server the web
tests hit — embedded-Axum coverage still needs the release-smoke check at least
once before phase end.

### The per-task loop

0. **Start** — pick the first available Phase 5 task per "Working the queue";
   `[ ]`→`[~]`, commit `start: <summary>`; work on a branch.
1. **Build** — implement per the task's acceptance criteria in
   [app-ui](app-ui.md) (wireframes govern detail). TDD where logic warrants;
   any new primitive goes through the `vendor-component` skill (bench section,
   same commit). Keep `cargo leptos watch --features component-bench` alive in
   the background for live verification.
2. **Codex adversarial review** — `/codex:adversarial-review --background`,
   focus text = the task's acceptance criteria; poll status, read result.
   Verify every finding before acting — never blind-apply; disputed findings
   recorded with rationale in app-ui Findings. Heavy mechanical fixes may be
   delegated to the `codex-rescue` agent. Repeat until clean or all-disputed.
3. **Local build verification** — web: hydration probe
   (`end2end/hydration-check.mjs`) + a page-specific SSR curl against :3000;
   Android: per the spike outcome (webview e2e, or `android-smoke` at stage
   boundaries). Desktop: none in-loop.
4. **E2E** — author specs per the `e2e-suite` skill (auth fixture, tier tags);
   run the fast tier (`npx playwright test --project=chromium`). Full
   three-browser tier (chromium + firefox + **webkit** — webkit is the WKWebView
   proxy since desktop is untested in-loop) at stage boundaries or whenever
   overlay/positioning code changed.
5. **Codex e2e adversarial pass** — `/codex:adversarial-review` focused on the
   new test files: "which assertions still pass if the feature is broken;
   propose one mutation per test." Accepted mutations are applied transiently,
   the test confirmed to fail, then reverted; evidence noted in Findings.
6. **Gate + land** — the `validate` skill; final commit flips `[~]`→`[x]` and
   records Findings in the same commit; conventional-commit PR; auto-merge on
   green.

### Failure policy

- A red platform check means the task cannot reach `[x]`.
- Emulator unavailable → record "Android smoke deferred: emulator offline" in
  Findings and flag the maintainer — never silently skip.
- E2E flake → one retry, then quarantine with a `@flaky` tag + a Findings
  entry. Quarantined tests block the phase-final polish task.
- Codex review disagreements are resolved by verification, not deference —
  disputed findings ship only with recorded rationale.
- Durable state is only: the branch, the TODO checkbox, and spec Findings.
  Codex job IDs and server PIDs are session-ephemeral — re-derive
  (`/codex:status`, `lsof -i :3000`), never persist.
- The Codex Stop-hook review gate stays **disabled** — the loop invokes Codex
  explicitly at the two points it belongs.

### Supporting skills (each earns its place with operational gotchas, not doc duplication)

| Skill | Owns |
|---|---|
| `ui-task-loop` | The loop above: Codex invocation split (slash commands vs. rescue agent), background polling, dispute policy, TODO/Findings bookkeeping |
| `e2e-suite` | Playwright authoring/running: the Better-Auth fixture trap (email verification is ON — a naive signup fixture hangs on OTP; use the pre-seeded verified test user, login helper captures `tr_session`/`tr_jwt`), :3000 server lifecycle, tier tags, webkit-as-WKWebView rationale, quarantine policy |
| `android-smoke` | Emulator boot (`adb devices` else `emulator -avd <AVD> &` + `wait-for-device`), debug-devUrl vs. release-embedded-Axum trap, install/launch/`adb forward` probe sequence, logcat crash grep, CDP attach recipe if the spike lands it |

Plus a permission allowlist in `.claude/settings.json` so the loop doesn't stall
on prompts: `Bash(cargo leptos *)`, `Bash(cargo tauri *)`, `Bash(cargo test *)`,
`Bash(adb *)`, `Bash(emulator *)`, `Bash(npx playwright *)`,
`Bash(node end2end/*)`. The settings diff is surfaced to the maintainer for
approval when it lands.

Deliberately **not** skills: the server-fn adapter pattern (a code convention —
exemplar in app-ui) and extending validate (the merge gate mirrors CI exactly;
smoke is a separate concern).

### E2E baseline reset

The current suite (`end2end/tests/example.spec.ts`) tests the counter being
deleted. Reset: remove it; add the auth fixture (pre-seeded **verified** test
user on the Neon dev branch — mechanism recorded in the `e2e-suite` skill when
built); tier tags (`@fast` chromium-only per task; full tier at boundaries).
The ad-hoc probes (`bench-check.mjs`, `hydration-check.mjs`, `auth-e2e.mjs`)
stay as the probe layer beneath the suite.

## Open questions

- **Android e2e viability** — the spike's whole purpose; resolved by its
  outcome per the decision tree above. *(resolved during execution — the
  Android-e2e spike task records the outcome in Findings here)*
- **Playwright in CI** — once the suite stabilizes, should the fast tier join
  the merge gate (needs a served build + dev-branch DB access in CI, which
  contradicts the no-DB-creds-in-GitHub rule — would need a seeded local PG or
  a dedicated Neon branch token)? Deferred; revisit at phase end.

## Findings

(appended per task — spike outcome, skill-building surprises, loop adjustments)

### Android e2e spike — PASS, decision-tree path 1 (2026-07-19)

Automated Playwright e2e against the Tauri debug webview on the Android
emulator **works end-to-end**. Evidence (Samsung_Flip_7 AVD, Android 17 arm64,
app `com.three_rings.dev`):

- **Attach**: debug builds ship a debuggable webview; the devtools socket
  appears as `@webview_devtools_remote_<pid>` in `/proc/net/unix` on app
  launch. `adb forward tcp:9222 localabstract:webview_devtools_remote_<pid>`,
  then Playwright `chromium.connectOverCDP('http://127.0.0.1:9222')` attaches
  (webview = Chrome 145, CDP 1.3). Playwright's experimental `_android` API
  was not needed.
- **Drive**: locators, clicks, `page.evaluate`, `page.goto`, and screenshots
  all work. Asserted: the scaffold counter incremented 20 → 21 through a real
  click (wasm hydration + server-fn round trip on device), and the bench
  dark-mode toggle flipped the `.dark` class (pure client-side wasm state).
- **Live dev-server content**: `cargo tauri android dev` runs
  `adb reverse tcp:3000 tcp:3000` itself and proxies the devUrl behind the
  stable origin `http://tauri.localhost` — the page URL never shows `:3000`.
  Proof the content is live rather than stale bundled assets: `/dev/components`
  (component-bench-gated, dev-server-only) renders on the device.

**Platform matrix fixed: path 1 — web + Android webview e2e every task; no
per-stage Android smoke tier; desktop ignored in-loop.** Embedded-Axum
coverage (release APK) still needs one release smoke before phase end — it
rides the phase-final polish task, which already carries "Android release
smoke".

Operational constraints for the skills (to be baked into `e2e-suite` /
`android-smoke` by the skills task):

- The socket name embeds the app pid — re-discover and re-forward on every app
  launch; never persist the port mapping.
- One webview page, one context: Android runs serialize (single worker) and
  share page state across tests; each spec must `goto` its own start URL.
- Navigate with `page.goto('http://tauri.localhost/<path>')` — same-page JS
  `location.href` races the CDP session (execution context destroyed).
- Emulator boot: `adb devices` shows a device, else
  `emulator -avd Samsung_Flip_7` + `adb wait-for-device` + poll
  `getprop sys.boot_completed` until `1`.
- `cargo tauri android dev` must run from the **repo root** — its
  `beforeDevCommand` (`cd .. && cargo leptos watch …`) resolves against the
  invocation directory; from `src-tauri/` it lands outside the workspace and
  dies with "manifest path `Cargo.toml` does not exist".

Probe layer: `end2end/android-cdp-check.mjs` (attach + page inventory +
evaluate) joins the `.mjs` probes.

### Work-loop skills + permissions (2026-07-19)

The three skills landed shaped by the spike's path-1 outcome:

- `ui-task-loop` — the six-step loop with the matrix baked in (web + Android
  webview e2e every task; full three-browser tier at stage boundaries; one
  Android **release** smoke at phase end, riding the polish task). Codex
  command names verified against the installed plugin
  (`/codex:adversarial-review [--wait|--background] [--base] [--scope] [focus]`,
  `/codex:status`, `/codex:result`).
- `e2e-suite` — login-fixture trap (verification ON → never sign up in a
  test; storageState off the real `/login` form), tier tags, webkit-as-
  WKWebView rationale, Android single-worker/shared-page constraints.
- `android-smoke` — emulator boot, dev CDP attach recipe, and
  `scripts/smoke-android.sh` for the phase-end release smoke (debug builds
  skip embedded Axum entirely — only release proves that path).

All mirrored into `.agents/skills/` (byte-identical copies — the established
mirror mechanism). Permission allowlist added to `.claude/settings.json`
exactly as specced. **Maintainer attention: that settings diff ships in this
PR** — revert the `permissions.allow` block if any entry is unwanted.

Drive-by fix, same commit: the `validate` skill's clippy lines had drifted
from validate.yml (missing the dedicated `--features native` backend line;
bench line still said `ssr` where the gate uses `hosted`) — realigned, and
the report template gained the native-backend row.

### E2E baseline reset (2026-07-19)

Counter suite deleted; tiered config + login fixture + baseline smoke landed;
fast tier 3/3 and full three-browser tier 7/7 green against the dev server.

- **Test user**: `three-rings-e2e@example.com`, seeded on the Neon **dev**
  branch by `end2end/seed-e2e-user.sh` (idempotent): signup through the real
  `/signup` form, then `emailVerified` flipped via the owner credential
  (`MIGRATION_DATABASE_URL`, the migrate.sh convention). Two findings baked
  into the script: the OTP send to a non-deliverable address can error
  UI-side *after* the account exists (so the DB row, not the screen, is the
  success criterion), and sign-in-gating reads the same `neon_auth."user"`
  row the app joins on — the mirror flip is sufficient.
- **Fixture**: `tests/auth.setup.ts` drives `/login` once, saves
  `storageState` (`tr_session`/`tr_jwt` ride along); authed tests opt in via
  `test.use({ storageState: AUTH_STATE })` from `tests/helpers.ts`
  (Playwright forbids importing one test file from another). The setup test
  carries `@fast` in its title or `--grep @fast` filters the dependency away
  and every authed test fails on a missing state file.
- **Release-build clobber trap (major)**: the validate gate's
  `cargo leptos build --release` overwrites `target/site/pkg` under the
  running debug watch — every page then hydration-panics
  (tachys `hydration.rs:163 unreachable`) and forms fall back to native
  POSTs. A source-file `touch` did *not* reliably rebuild the frontend half;
  **restart the watch after any release build**. Recorded in the e2e-suite
  skill; this cost ~30 min of diagnosis and will bite every loop iteration
  that verifies after gating.
- **Codex invocation path**: the review slash commands are human-only
  (`disable-model-invocation: true` in the plugin), so autonomous loop runs
  route reviews through the `codex-rescue` agent with a review-only prompt —
  ui-task-loop skill updated accordingly; mechanism proven on this task's
  own diff. Wrinkle: the rescue subagent is a fire-and-forget forwarder (it
  refuses to poll), so the main session polls the companion runtime itself
  (`codex-companion.mjs status/result <task-id>`).
- **Codex review of this task** (4 findings): (1) owner credential in psql
  argv → **fixed**, URL parsed into `PG*` env vars, secret never in the
  process table; (2) fresh-checkout password drift (lost `.env` + existing
  user = unknowable password, fixture permanently broken with a misleading
  "seed complete") → **fixed**, freshly generated creds delete + recreate
  the purpose-built e2e user, script idempotent from any state (verified by
  running exactly that scenario); (3) browser h1-check doesn't prove SSR →
  **fixed**, request-level assertion on the raw HTML (no JS) added;
  (4) fixed :3000 could hit a stale/foreign server → **disputed**: the loop
  deliberately owns the watch-server lifecycle (a Playwright `webServer`
  block would fight the long-lived watch and its minutes-long builds);
  single-developer risk accepted, a build-stamp `/health` route noted as a
  possible future hardening.
