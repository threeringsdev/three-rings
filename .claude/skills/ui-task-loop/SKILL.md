---
name: ui-task-loop
description: The Phase 5 per-task work loop — build → Codex adversarial review → platform verification (web + Android webview e2e) → e2e authored + mutation-checked → merge gate → PR. Use when starting any Phase 5 UI task from specs/TODO.md, when the user says "next UI task", "run the loop", or "work the queue" during Phase 5, or when resuming a [~] UI task.
---

# UI task loop — one Phase 5 task, end to end

The contract is [specs/ui-work-loop.md](../../../specs/ui-work-loop.md); this
skill is the operational sequence. Platform matrix (fixed by the spike,
2026-07-19): **web + Android webview e2e every task; desktop ignored in-loop;
one Android release smoke at phase end** (the polish task).

## Sequence

0. **Start** — first available Phase 5 task per specs/README.md "Working the
   queue". Branch off fresh `main`. Flip `[ ]`→`[~]`, commit exactly that:
   `start: <task summary>`. Read the task's spec sections *before* code.
1. **Build** — per the task's acceptance criteria in specs/app-ui.md;
   wireframes (design/wireframes.pen) govern detail. New primitives go through
   the **vendor-component** skill (bench section, same commit). Keep
   `cargo leptos watch --features component-bench` alive in the background
   (repo root; it binds :3000 after the first build — check `lsof -i :3000`
   before assuming it's down).
2. **Codex adversarial review** — the `/codex:*` slash commands are human-only
   (`disable-model-invocation: true`), so an autonomous run drives the
   **runtime script directly**. That is the whole trick, and it is not
   optional: `codex-rescue` dispatches a job and returns *before it finishes*,
   and messaging it for the result gets a refusal (its own instructions forbid
   follow-up work). **A refusal from `codex-rescue` does not mean Codex is
   unavailable.** Dispatch via the agent, then collect via:

   ```bash
   R="$HOME/.claude/plugins/cache/openai-codex/codex/<ver>/scripts/codex-companion.mjs"
   node "$R" status                 # job id + completion state
   node "$R" result <job-id>        # the findings
   ```

   Poll `status` until the job leaves `running`. Prompt it review-only: the
   task's acceptance criteria as focus, "return numbered findings (file:line,
   what breaks, severity), no fixes applied". **Verify every finding before
   acting** — never blind-apply; record disputed findings + rationale in
   app-ui Findings. Repeat until clean or all-disputed.
3. **Platform verification** — web: `node end2end/hydration-check.mjs <urls…>`
   + a page-specific SSR curl against :3000 (view-source markup present).
   **The probe takes URLs as arguments — running it bare exits 0 having checked
   nothing.** Probe the pages you touched *and* every page that renders a
   component you touched; a shared component means a page you didn't open can
   regress. (The filter-rail task skipped this step entirely and a real
   `/catalog` hydration bug went undetected for two tasks — app-ui Findings,
   "Cross-task audit".) A warning here is a finding, not noise: chase it to
   either a fix or a written reason it is safe.
   Android: the **android-smoke** skill's dev-attach recipe →
   `node end2end/android-cdp-check.mjs`, then run the task's e2e spec against
   the webview if it touches layout/input (overlay/positioning code always).
4. **E2E** — author specs per the **e2e-suite** skill (login fixture, tier
   tags). Iterate with the fast tier (`npx playwright test
   --project=chromium`), then run the **full tier at the end of the task**:
   `npx playwright test` (chromium+firefox+webkit — webkit is the WKWebView
   proxy). Full-tier green is a precondition for `[x]`, every task.
5. **Codex e2e pass** — same dispatch/collect mechanics as step 2, focused on
   the new test files: "which assertions still pass if the feature is broken;
   propose one mutation per test." Apply accepted mutations transiently,
   confirm the test fails, revert; note evidence in Findings. **Wait for the
   watch server to actually rebuild before judging a mutation** — poll the
   served wasm hash (`curl -s :3000/pkg/app_bg.wasm | md5`) until it changes,
   or you will test the old binary and record a false survival.
6. **Gate + land** — the **validate** skill. Final commit flips `[~]`→`[x]`
   **and** records Findings in the same commit. Conventional-commit PR title
   (it becomes the squash commit on main); enable auto-merge; confirm green.

## Failure policy (from the spec — not optional)

- A red platform check blocks `[x]`. No exceptions.
- Emulator unavailable → Findings entry "Android smoke deferred: emulator
  offline" + flag the maintainer. Never silently skip.
- E2E flake → one retry, then quarantine with `@flaky` tag + Findings entry.
  Quarantined tests block the phase-final polish task.
- Codex disagreements resolve by verification, not deference.
- Durable state = branch + TODO checkbox + spec Findings only. Codex job IDs
  and server PIDs are session-ephemeral — re-derive (`/codex:status`,
  `lsof -i :3000`), never persist.
- The Codex Stop-hook review gate stays **disabled** — the loop invokes Codex
  explicitly at steps 2 and 5.

## Operational gotchas

- `cargo tauri android dev` runs from the **repo root** (its beforeDevCommand
  `cd ..` resolves against the invocation dir; from src-tauri/ it dies with
  "manifest path `Cargo.toml` does not exist").
- The Android manifest at src-tauri/gen/android/.../AndroidManifest.xml gets a
  deep-link intent-filter injected at build time — `git checkout` it before
  committing; never commit the injected copy.
- The full three-browser tier runs at the end of **every** task (step 4), not
  at stage boundaries. A stage-boundary task additionally runs the Android
  **release** smoke via the android-smoke skill (at phase end only).
