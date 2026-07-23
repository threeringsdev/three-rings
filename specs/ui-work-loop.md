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
milestone activity); CI e2e (local-loop concern — the possible follow-up of
promoting Playwright into CI was decided **against** 2026-07-23, see Findings).

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
   iterate with the fast tier (`npx playwright test --project=chromium`), then
   run the full three-browser tier (chromium + firefox + **webkit** — webkit is
   the WKWebView proxy since desktop is untested in-loop) **at the end of every
   task**. Revised 2026-07-20 (maintainer): the tier was originally scoped to
   stage boundaries, but the filter-rail task showed boundary-only running lets
   cross-browser breakage sit undetected across several tasks and then land as
   a pile — the shell task's full tier surfaced 8 pre-existing firefox/webkit
   failures at once. Full-tier green is now a precondition for `[x]`.
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
built); tier tags (`@fast` chromium-only while iterating; full three-browser
tier at the end of every task — revised 2026-07-20, see the per-task loop).
The ad-hoc probes (`bench-check.mjs`, `hydration-check.mjs`, `auth-e2e.mjs`)
stay as the probe layer beneath the suite.

## Open questions

- **Android e2e viability** — the spike's whole purpose; resolved by its
  outcome per the decision tree above. *(resolved during execution — the
  Android-e2e spike task records the outcome in Findings here)*
- **Playwright in CI** — once the suite stabilizes, should the fast tier join
  the merge gate (needs a served build + dev-branch DB access in CI, which
  contradicts the no-DB-creds-in-GitHub rule — would need a seeded local PG or
  a dedicated Neon branch token)? *(resolved 2026-07-23 — **no**: Playwright
  stays a local-loop precondition for `[x]`; the gate stays creds-free. Four
  grounds + explicit revisit conditions in Findings, "Playwright stays out of
  the merge gate".)*

## Findings

(appended per task — spike outcome, skill-building surprises, loop adjustments)

### Android dev-proxy limits: no authed flows over dev attach (2026-07-19)

Discovered during the app-shell task; constrains every future task's
on-device verification scope. The Tauri Android **dev** proxy (webview →
`http://tauri.localhost` → devUrl) mangles three things, verified directly
against the attached webview:

1. **Follows server 302s internally** — the webview gets the redirect
   target's HTML at the original URL. The app now self-recovers (the
   `data-ssr-path` stamp + `shell::hydrate_entry` replace shim, app-ui
   Findings), so this one is handled.
2. **Strips POST request bodies** — an argless server-fn POST returns 200,
   but a form-encoded POST reaches the server with an empty body ("missing
   field `email`"). The spike's counter-increment POST was argless, which is
   why this never showed before.
3. **Strips Cookie headers** — with valid `tr_session`/`tr_jwt` injected
   into the webview jar, `GET /api/me` → 401. (The dark-palette on-device
   theme check didn't catch this: the toggle also initializes client-side,
   masking the SSR miss.)

Loop consequence: **on-device dev-attach verification covers the anonymous
surface only** (navigation, layout, overlays, SSR/hydration, guard bounces).
Authed interactions stay on the web tiers (webkit = WKWebView proxy).
Whether the **release** protocol handler shares these behaviors is unproven —
a queue task before the phase-end release smoke must verify sign-in works in
the release APK at all. Hot-reload websockets also die through the proxy
("Live-reload stopped" immediately) — expected noise, not a failure.

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
  webview e2e every task; full three-browser tier at the end of every task
  (revised 2026-07-20 from stage-boundaries-only); one
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
- **Release-build clobber trap (major)** — **resolved 2026-07-23**: the validate gate's
  `cargo leptos build --release` overwrites `target/site/pkg` under the
  running debug watch — every page then hydration-panics
  (tachys `hydration.rs:163 unreachable`) and forms fall back to native
  POSTs. A source-file `touch` did *not* reliably rebuild the frontend half;
  **restart the watch after any release build**. Recorded in the e2e-suite
  skill; this cost ~30 min of diagnosis and will bite every loop iteration
  that verifies after gating. The gate now runs its release build under a
  dedicated `CARGO_TARGET_DIR=target/gate`, so it can no longer touch the
  watch's `target/site/pkg` — see "Gate build gets its own target dir" below.
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

### Playwright stays out of the merge gate (2026-07-23)

Resolves the "Playwright in CI" open question: **no** — the e2e tiers stay a
local-loop precondition for `[x]` (full three-browser tier every task, per the
2026-07-20 revision), and the merge gate stays creds-free and byte-identical
to the local validate suite. Four grounds, each checked against the current
suite rather than the OQ's original sketch:

1. **The standing no-Neon-creds-in-GitHub rule.** CLAUDE.md: "No
   `DATABASE_URL` in GitHub — no CI job talks to Neon." It was reaffirmed
   2026-07-16 when the ingestion cron chose Render *specifically because* the
   rule ruled out GitHub Actions (TODO Decisions log). Every variant that
   runs the *real* suite (authed, data-backed) ends at a Neon credential in a
   GitHub secret: the dev-branch `DATABASE_URL` directly, or a Neon API key
   to mint per-run branches — the OQ's "dedicated Neon branch token" is still
   a Neon credential in GitHub.
2. **The auth fixture hard-depends on the hosted Neon Auth service.**
   `playwright.config.ts` gives all three browser projects
   `dependencies: ["setup"]`, and `auth.setup.ts` signs in through the real
   `/login`, which proxies to the Neon-hosted Better Auth service
   (`NEON_AUTH_BASE_URL` — sign-in 500s without it). So the OQ's "seeded local
   PG" escape hatch cannot run the suite: a CI-local Postgres has no auth
   service and no Neon-managed `neon_auth."user"` sync, the setup project
   fails, and every spec fails with it (6 of 7 spec files use the authed
   storageState). A creds-free tier would mean standing up a bespoke
   better-auth deployment plus a fake `neon_auth` sync — substantial new
   infrastructure that then exercises a *different* auth stack than the one
   that ships.
3. **Concurrent CI runs race the suite's writes on shared mutable state.**
   The tests write as the single seeded e2e user (quick-add + undo,
   tree create/rename/delete/drag-reparent) against the shared dev branch.
   One push to a PR branch starts *two* runs — the `push` and `pull_request`
   events land in different concurrency groups (`refs/heads/<branch>` vs
   `refs/pull/<n>/merge`), so validate.yml's cancel-in-progress does not
   collapse them — and unrelated branches add more. Interleaved writes to one
   user's collections are structural flake, not retry-tunable noise. The
   isolation fix (ephemeral per-run Neon branches) is otherwise technically
   sound — Neon Auth is provisioned *per branch* (`.env.example` documents the
   branch-specific auth URLs), so a fresh branch carries its own isolated auth
   service and data and would exercise the shipped auth stack — but it circles
   back to ground 1: the rejection is the credential, not feasibility.
4. **Cost/latency lands on every push of the workflow auto-merge waits on.**
   Serving a build + installing Playwright browsers + running the tier adds
   minutes to a gate that runs twice per PR-branch push, in a repo whose
   runner budget is deliberately frugal (Blacksmith free tier; the artifact
   cadence is already split by cost). The marginal coverage is small: the
   loop already makes full-tier green a hard precondition for `[x]` on every
   task, with Codex mutation passes guarding assertion strength.

One creds-free Playwright shape *is* technically workable (Codex review of
this decision, accepted): an anonymous-only subset — `--no-deps` (or a
dedicated project without the setup dependency) skips `auth.setup.ts`, and an
unauthenticated `fetch_current_user` returns `None` before any upstream auth
call (app/src/account.rs), so `/`, the `/my`→`/login` guard bounce, `/login`,
and hydration all serve with no DB or auth credential. **Declined on marginal
value, not feasibility**: that tier covers only the anonymous shell — no
search results, no collections, none of the surfaces the suite exists for —
duplicating the per-task local probes (hydration-check.mjs + SSR curls) while
paying browser-install + serve minutes twice per PR-branch push. If a CI
browser check ever becomes wanted, this subset is the creds-free entry point.

What the gate *should* gain instead, creds-free: the queued headless
release-binary launch assertion (build the release server, assert it binds
and answers `GET /catalog` — the DB probe is non-fatal by design, so it needs
no `DATABASE_URL`). On cost grounds that task, not a Playwright tier, is the
right first CI runtime smoke; it would have caught the 2026-07-20
crash-on-launch that shipped to `latest`.

**Revisit conditions** (so "no" doesn't quietly rot into dogma): (a) the auth
stack gains a self-hostable/CI-reachable path, or Neon ships OIDC-federated
short-lived tokens for Actions (no long-lived secret at rest in GitHub);
(b) contributors outside the loop start pushing UI changes, so the local
tiers stop being a reliable invariant; (c) per-run write isolation becomes
possible without Neon API access. Any of those reopens the question; until
then it is settled, not deferred.

Residual risk, accepted: a change pushed outside the loop can auto-merge
unit-green with broken UI. Single-maintainer repo; the loop is the standing
discipline; the launch-assertion task narrows the worst case (a server that
won't boot).

Codex adversarial review of this decision (4 findings): (1) high —
"every variant needs a Neon credential / not Playwright" overstated; the
anonymous `--no-deps` subset is creds-free-workable → **accepted**, rationale
amended to decline it on value (above); (2) medium — per-run Neon branches
mischaracterizable as technically invalid when branchable per-branch auth
makes them sound → **accepted**, ground 3 now names it policy-not-feasibility;
(3) low — playwright.config.ts's tier comment still said "stage boundaries",
stale since the 2026-07-20 every-task revision this decision leans on →
**accepted**, comment fixed in this diff; (4) low — TODO still `[~]` while
the texts say resolved → **no change**, that is the loop's mid-task state;
the `[x]` flip lands in the task's final commit as always.

### Gate build gets its own target dir (2026-07-23)

Fixes the "Release-build clobber trap" structurally instead of by discipline.
`cargo leptos build` has no `--target-dir` flag, so the isolation rides an env
var plus a one-line `Cargo.toml` change: `site-root` became the
`CARGO_TARGET_DIR/site` **marker** that cargo-leptos resolves against the real
cargo target directory (`config/project.rs` `parse_raw`). Consequences:

- **Default (env unset)** → the marker resolves to `target/site`, byte-for-byte
  the old behavior. Every non-gate build keeps it because none of them set the
  var: `cargo leptos watch`, `cargo leptos serve`, the Docker/Render image
  (`COPY /app/target/site`), and the Tauri `beforeBuildCommand`. Confirmed live —
  the watch started under the new `Cargo.toml`, wrote `target/site/pkg`, and
  served :3000 normally.
- **Merge gate** runs `CARGO_TARGET_DIR=target/gate cargo leptos build
  --release`. `cargo metadata` honors `CARGO_TARGET_DIR`, so the target dir
  becomes `target/gate`; site → `target/gate/site`, wasm →
  `target/gate/front` (front_target_dir defaults to `CARGO_TARGET_DIR/front`),
  native artifacts → `target/gate/…`. Nothing the gate writes lands in the
  watch's `target/site/pkg`.

Only the release-build step is isolated; the clippy/test steps keep sharing
`target/` with the watch (they never write `site-root`, and sharing lets them
reuse the watch's compiled deps — cargo's per-target lock serializes them
safely). CI (`validate.yml`) is deliberately left unchanged: no watch server
runs there, and keeping the build under `target/` preserves the
`useblacksmith/rust-cache` paths.

**Verified** with a `cargo leptos watch --features component-bench` live on
:3000 throughout a full gate run (all 8 steps green, macOS host incl.
`three_rings`): the release build populated `target/gate/site/pkg` (2.1 MB
release wasm) while the watch's `target/site/pkg` stayed byte-identical (all
five files' md5 unchanged before/after), the served `/pkg/app.wasm` remained the
14 MB debug build, and `/login` + `/dev/components` still hydrated **CLEAN with
no watch restart** — the exact verify-after-gating scenario that used to force
one. Docs updated in lockstep: the validate skill (+`.agents` mirror),
CLAUDE.md/AGENTS.md Verify section, and the e2e-suite clobber-trap note (which
now flags that a *bare* `cargo leptos build --release` still clobbers).
