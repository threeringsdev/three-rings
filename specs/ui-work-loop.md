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
