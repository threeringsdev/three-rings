# UI work loop — the per-task agent loop for Phase 5

**Status:** implemented
**Depends on:** [app-ui](app-ui.md), [delivery-pipeline](delivery-pipeline.md)

> Flipped at the Stage 3 boundary, 2026-07-27. The loop ran all of Stage 3; its contract —
> spec reading before code, kill-verification, the probe layer beneath the suite, the
> platform matrix, and skill-mirror sync — held throughout, and its recorded stopping rule
> is what kept the boundary task from sprawling. The **Android release smoke stays
> maintainer-owned** (`artifacts.yml` is `workflow_dispatch` only), so the loop records it
> rather than claiming release coverage it does not produce.

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
orchestrating in the **main session**. Revised 2026-07-25 (maintainer): the
main session is now a pure **orchestrator** — implementation and adversarial
review each run in their own subagent with a clean context, and Codex is out
of the loop entirely (see Findings, "Loop goes fully Claude-driven"). What
stays main-session, and why the mechanism is still a skill rather than a
Workflow script: the loop must hold long-lived host state (the
`cargo leptos watch` server, the Android emulator) across steps; queue
bookkeeping and arbitration between the implementer and reviewer are
sequential judgment calls; and durable checkpointing already exists (TODO
states, branch, spec Findings) so a Workflow engine adds nothing. Repetition
across tasks is the orchestrator's own loop (step 5 → step 0), with
human-in-the-loop recommended for the first few iterations.

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

### The per-task loop (revised 2026-07-25 — subagent split; Codex removed)

0. **Start** (orchestrator) — pick the first available Phase 5 task per
   "Working the queue"; `[ ]`→`[~]`, commit `start: <summary>`; work on a
   branch. The orchestrator reads only enough to write the dispatch prompt.
1. **Implement** (subagent, clean context) — one implementation subagent
   receives the task line, the branch, the required spec reading
   ([app-ui](app-ui.md) sections + `(specs:)` links + wireframes), and the
   skills to invoke (`vendor-component`, `e2e-suite`, `android-smoke`). It
   builds per the acceptance criteria (TDD where logic warrants), runs the
   platform verification itself — web hydration probe
   (`end2end/hydration-check.mjs` with URLs) + page-specific SSR curl;
   Android webview check per the spike outcome — and **authors** the e2e per
   `e2e-suite`, running only `--project=chromium --grep @fast` while
   iterating. It does not run a full e2e pass; that is step 4. It commits its
   own work and reports evidence; it never touches TODO.md or spec Findings.
2. **Adversarial review** (subagent, clean context) — **exactly one round, code
   only.** A fresh reviewer invokes the `adversarial-review` skill: acceptance
   criteria as claims to test, plus the repo trap list. It **runs no tests** —
   no Playwright, no mutation pass, no rebuild cycles — and reports **blockers
   and majors only**, with everything else in a separate minors list.
   Review-only by contract — findings, never fixes. There is no re-review.
3. **Fix the majors** (orchestrator arbitrates) — blockers/majors go back to the
   implementation agent: verify every finding before acting — never
   blind-apply; disputed findings recorded with rationale in app-ui Findings.
   **Minors are never sent to the implementer** — the orchestrator files each
   as a `[ ]` under "Phase 5 discoveries" in TODO.md at landing time.
4. **E2E** (once, after the fixes) — a single `npx playwright test
   --project=chromium` plus the Android webview probe. A major failure returns
   to step 3 for that failure only; a minor failure or flake is retried once,
   then quarantined `@flaky` and filed.
5. **Gate** (subagent) — the `validate` skill, per-step verdict table
   required. Skippable only when the diff since the last green run touches no
   compiled sources, stated explicitly.
6. **Land** (orchestrator) — final commit flips `[~]`→`[x]`, records Findings,
   and files the round's minors as Phase 5 discoveries, all in the same commit;
   conventional-commit PR; auto-merge on green; loop to the next task.

### Loop break — the termination condition

The loop runs task after task and **ends when Stage 3 holds no `[ ]`**. The
orchestrator does not check in between tasks; it lands one and starts the next.
It stops early only when the human says so, when every remaining `[ ]` is gated
by a `draft` spec, or when a task cannot reach `[x]`.

This terminates only because **nothing is ever added to Stage 3 while it is
being worked**. Every discovery — out-of-scope bugs, review minors, deferred
follow-ups, quarantined tests — is filed under `### Phase 5 discoveries` in
`## Later / parked`. For the duration of this loop that rule **overrides
CLAUDE.md's** "newly discovered follow-up work added as new `[ ]` tasks in the
right phase": during Phase 5 the right phase is the discoveries pen. A stage
list that grows while being drained never empties.

Between tasks the orchestrator keeps its own context and discards the
subagents — a fresh implementer and reviewer per task, never a carried-over
agent whose context is full of the previous feature.

### Calibration — MVP, one developer

Revised 2026-07-25 after the binder/deck task cost ~2.5 h and 1.44M subagent
tokens. This project is an early-stage MVP with a single developer, not a
hardened production service, and the loop is tuned to that:

- **Chromium only.** Firefox and webkit are not run at all; cross-browser
  compatibility is deferred wholesale, with WKWebView to be covered by desktop
  later. A bare `npx playwright test` fans out across all three and is
  forbidden.
- **One e2e run per task**, after the review fixes — not one per round.
- **One review round**, code-only, majors-only.
- **No mutation passes.** Assertion strength gets its own sweep once the spec
  work is done.
- **Minors are filed, not fixed.** Craft — naming, comment accuracy, assertion
  elegance, redundancy — is explicitly out of scope right now.

Extended 2026-07-25 after the quick-add task, once the first calibration's
numbers came in and showed the test matrix was never the cost (see Findings):

- **The implementer does not run the gate.** No `cargo leptos build --release`,
  no `cargo fmt --check`, no clippy sweep — step 5 owns all of them. The
  implementer gets `cargo test` plus one `clippy -p app` for feedback.
- **Required reading is handed over as line ranges**, not section names. The
  orchestrator greps; the implementer reads what it is pointed at.
- **Probes are capped** — hydration on at most four URLs, one SSR curl per
  changed surface.
- **Nobody runs the gate locally as a pre-check.** CI runs it on every PR,
  branch protection requires it, auto-merge only fires on green. The laptop is
  macOS and the devcontainer can't build `three_rings` at all, so a local green
  is not evidence of the green that gates. Local `validate` is a *debugging*
  tool for a red CI run, not a ritual before pushing.

### Failure policy

- A red **chromium** e2e run or a red gate means the task cannot reach `[x]`.
  Firefox and webkit are not run and never block.
- Emulator unavailable → record "Android smoke deferred: emulator offline" in
  Findings and flag the maintainer — never silently skip.
- E2E flake → one retry, then quarantine with a `@flaky` tag + a Findings
  entry. Quarantined tests block the phase-final polish task.
- Review disagreements are resolved by verification, not deference — disputed
  findings ship only with recorded rationale.
- Durable state is only: the branch, the TODO checkbox, and spec Findings.
  Subagent IDs and server PIDs are session-ephemeral — re-derive
  (`lsof -i :3000`), never persist.
- The Codex plugin stays installed for human-invoked use but is out of the
  loop (2026-07-25); its Stop-hook review gate stays **disabled**.

### Supporting skills (each earns its place with operational gotchas, not doc duplication)

| Skill | Owns |
|---|---|
| `ui-task-loop` | The orchestration above: what the main session owns vs. delegates, the dispatch-prompt recipes, the fix-loop/dispute policy, TODO/Findings bookkeeping |
| `adversarial-review` | The reviewer subagent's contract: review-only and run-nothing discipline, the major/minor rule, the repo trap hunt list, the findings output format |
| `e2e-suite` | Playwright authoring/running: the Better-Auth fixture trap (email verification is ON — a naive signup fixture hangs on OTP; use the pre-seeded verified test user, login helper captures `tr_session`/`tr_jwt`), :3000 server lifecycle, chromium-only tiers, assertions that lie, quarantine policy |
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
built); tier tags (`@fast` while iterating; one full **chromium** pass per task
after the review fixes — revised 2026-07-25 to chromium-only, see Calibration).
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

### The implementer is the cost, not the test matrix (2026-07-25)

The recalibration worked — the quick-add task (PR #58) ran **887k subagent
tokens against the binder/deck task's 1.44M**, on a comparable diff. But its
per-step timings falsify the assumption the first calibration was built on.
Measured:

| Step | Wall clock | Tokens |
|---|---|---|
| Implementation subagent | **69 min** (51 + 18 after an API error) | 680k |
| Review subagent | 10.5 min | 153k |
| Validate gate subagent | 2.0 min | 53k |
| Full chromium tier (131 tests) | **52 s** | — |
| Android probes + cookie refresh | ~15 s | — |

The full e2e tier — the thing the first calibration spent its effort cutting
from ~6 runs to 1 — costs **52 seconds**. The implementer is 85% of wall clock
and 77% of tokens. Further tuning of the test matrix is optimizing the 1%.

**The one large, verified waste: the implementer ran the entire eight-step merge
gate itself**, including `CARGO_TARGET_DIR=target/gate cargo leptos build
--release` — full Tailwind + wasm, cold, the most expensive command in the repo.
The step-5 gate agent then replayed steps 2–6 and 8 from cargo fingerprints in
under a second each, which is the proof the implementer's run was pure
duplication. Nothing in the dispatch prompt forbade it; a conscientious agent
runs the gate to be safe. The fix is one paragraph in the dispatch contract, now
in the skill and in Calibration above.

Two smaller levers landed with it: required reading handed over as **line
ranges** (specs/app-ui.md is ~1,700 lines, and "read the section plus its
`Depends on:` specs" makes an agent read most of it), and a **cap of four**
hydration URLs (agents left to choose probe eight).

**Then step 5 went away entirely.** Checking the workflow rather than assuming
showed `validate.yml` already runs the same eight steps on every PR, in **2–3
minutes**, and branch protection requires it — the local run was duplicating a
cloud run that was about to happen anyway. Worse, it is *not the same check*:
CI is linux with Tauri's system libs, the laptop is macOS, and the devcontainer
cannot build `three_rings` at all, so a local green was never evidence of the
green that gates. Nothing can ship broken by dropping it, because auto-merge
only fires on the cloud green; the only cost is finding out about a red gate
~2 minutes later. Local `validate` is now a debugging tool for a red CI run.

Found while checking: a bare `on: push` alongside `on: pull_request` fired
**two identical runs for every push to a PR branch** — visible in the run
history as matching `push` and `pull_request` entries on the same commit,
2 minutes each. Scoped to `on: push: branches: [main]`, halving validate
minutes for no loss of coverage.

Not cut, deliberately: the review round. 10.5 min and 153k for an independent
read of a write path is proportionate, and it produced 12 filed findings even at
zero majors.

### Loop recalibrated for MVP cost (2026-07-25)

The binder/deck task (`/my/collections/:id`, PR #56) cost **~2.5 h wall and
1.44M subagent tokens** — implementation 60 min / 454k, review 1 30 min / 198k,
fix round 26 min / 523k, review 2 36 min / 269k — and was still returning
findings when the maintainer stopped it. Measured drivers, in order:

1. **The full three-browser tier ran ~6 times for one task.** It grows every
   task (196 → 355 → 367 tests over three stories) and *every* actor ran it:
   the implementer at the end of implementation and again after fixes, review 1
   once, review 2 three times. This was the compounding cost — it worsens every
   task regardless of story size.
2. **The mutation pass was uncapped** — 12 mutations in round 1, 10 in round 2,
   each an edit → rebuild → wait-for-serve → run → revert → rebuild cycle.
3. **Review rounds were unbounded**, and returns fell off a cliff between them.
   Round 1 found a genuine data-loss blocker (committing HERE to 0 deleted the
   holding while the offered Undo 404'd). Round 2's best finding was a vacuous
   test; the rest was craft — "the fix works but the comment explaining it is
   wrong", assertion elegance, flake-proneness.
4. **The task line bundled three surfaces** (binder + deck + stepper + teardown
   + mobile + search + paging) into one agent's context.

**Maintainer decision.** Three-rings is an early-stage MVP with a single
developer building toward a working product, not a hardened production service.
The loop was spending like the latter. Changes, all now in the Design section
above and the three skills:

- **Chromium only, full stop.** Firefox and webkit are not run at all.
  Cross-browser is deferred wholesale; WKWebView will be covered by desktop
  later. This drops the webkit-as-WKWebView rationale that had justified the
  three-browser tier since 2026-07-20.
- **One e2e run per task**, after the review fixes land — not per round, and
  not by the implementer or reviewer, who use `@fast` chromium only.
- **One review round**, reading code and running nothing, reporting blockers
  and majors only.
- **No mutation passes.** Assertion strength gets a dedicated sweep after the
  spec work is done, rather than a per-task tax.
- **Minors are filed, never fixed** — a `[ ]` under Phase 5 discoveries.

The trade is accepted knowingly: fewer vacuous tests will be caught at the
moment they are written, and cross-browser breakage will land as a pile to be
paid down later. Both were judged cheaper than the current per-task cost.
Two lessons from round 2 were kept as *authoring* checklist items in
`e2e-suite` rather than as a verification pass — a test can only distinguish
behaviors the **fixture** distinguishes, and overflow must be measured on the
scroll container, not the document.

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

### Loop goes fully Claude-driven — Codex removed, orchestrator + subagents (2026-07-25)

Maintainer decision, restructuring the loop's mechanism (steps unchanged in
*what* they prove, changed in *who* runs them). Grounds:

1. **The Codex review path forced implementation-level work into the main
   session.** The `/codex:*` review commands are human-only
   (`disable-model-invocation`), and the `codex-rescue` agent is a
   fire-and-forget forwarder that refuses to poll — so autonomous runs had the
   orchestrator driving `codex-companion.mjs status/result` itself and holding
   the whole task's context besides. The card-detail task already recorded the
   failure mode: Codex step-2 output could not be read in an autonomous run
   and a substitute reviewer was used.
2. **Clean contexts per role.** Implementation and review now each get a fresh
   subagent (the reviewer via the new `adversarial-review` skill, dispatched
   with the acceptance criteria as focus). The main session only orchestrates:
   queue/git bookkeeping, host-process lifecycle, the fix-loop arbitration —
   so one session can span many tasks without accumulating each task's build
   context.
3. **The review discipline is preserved, not relaxed**: review-only (findings,
   never fixes), verify-before-acting, disputes recorded with rationale, and
   the mutation pass with its rebuild-wait mechanics all moved verbatim into
   `adversarial-review`. Fresh reviewer every round keeps the independence the
   Codex step provided.

Drive-by fix while porting: the old skill's mutation rebuild-wait polled
`:3000/pkg/app_bg.wasm`, a file that does not exist in `target/site/pkg` (the
served binary is `app.wasm`) — the poll could never see a hash change. The
`adversarial-review` skill polls `/pkg/app.wasm`.

The Codex plugin remains installed for human-invoked use (`/codex:*`); the
permission allowlist is unchanged. TODO.md's Phase 5 intro updated to describe
the new loop shape.

Skill shakedown (same day, before landing): two subagent reviews of the
DFC-flip commit (`c088006` — the task whose Codex review scored **zero**
findings). The skill-dispatched reviewer invoked `adversarial-review` through
the **Skill tool** (the mechanism the loop depends on — confirmed available to
subagents), followed the output contract exactly (numbered file:line findings
with severities, hunted-and-cleared evidence, verdict, tree-state check), and
returned 4 verified minor findings. A second reviewer dispatched *bare*
converged on the same contract shape — not a clean baseline, the skill file
was visible in the working tree — and additionally executed the unit-level
mutation pass, exposing one kill-verified vacuous test (a surviving mutation
in `CardFaceSummary::build`'s fail-closed guard). Both respected review-only:
every mutation reverted byte-identical, neither touched the orchestrator's
server. The six distinct findings across the two runs are queued as a Phase 5
follow-up task.

