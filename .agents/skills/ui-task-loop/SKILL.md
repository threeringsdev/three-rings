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
2. **Codex adversarial review** — `/codex:adversarial-review --background`
   with the task's acceptance criteria as focus. Poll `/codex:status`, read
   `/codex:result`. **Verify every finding before acting** — never
   blind-apply; record disputed findings + rationale in app-ui Findings.
   Heavy mechanical fixes may go to the `codex-rescue` agent. Repeat until
   clean or all-disputed.
3. **Platform verification** — web: `node end2end/hydration-check.mjs` + a
   page-specific SSR curl against :3000 (view-source markup present).
   Android: the **android-smoke** skill's dev-attach recipe →
   `node end2end/android-cdp-check.mjs`, then run the task's e2e spec against
   the webview if it touches layout/input (overlay/positioning code always).
4. **E2E** — author specs per the **e2e-suite** skill (login fixture, tier
   tags). Run the fast tier: `npx playwright test --project=chromium`.
   Full tier (chromium+firefox+webkit — webkit is the WKWebView proxy) at
   stage boundaries or whenever overlay/positioning code changed.
5. **Codex e2e pass** — `/codex:adversarial-review` focused on the new test
   files: "which assertions still pass if the feature is broken; propose one
   mutation per test." Apply accepted mutations transiently, confirm the test
   fails, revert; note evidence in Findings.
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
- A stage-boundary task additionally runs the full three-browser tier and (at
  phase end only) the Android **release** smoke via the android-smoke skill.
