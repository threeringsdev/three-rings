---
name: ui-task-loop
description: Use when starting any Phase 5 UI task from specs/TODO.md, when the user says "next UI task", "run the loop", or "work the queue" during Phase 5, when resuming a [~] UI task, or when asked to keep working tasks in sequence.
---

# UI task loop — orchestrate one Phase 5 task end to end

The contract is [specs/ui-work-loop.md](../../../specs/ui-work-loop.md); this
skill is the operational sequence. The main session is the **orchestrator**: it
owns queue bookkeeping, git state, long-lived host processes, dispatch, and
arbitration — and does **not** implement or review. Implementation and review
each run in their own subagent with a clean context; context spent doing their
work in the main session is context the loop can't spend on the next task.

Platform matrix (fixed by the spike, 2026-07-19): **web + Android webview e2e
every task; desktop ignored in-loop; one Android release smoke at phase end**
(the polish task).

## Orchestrator owns (never delegate)

- **Queue**: task selection per specs/README.md "Working the queue";
  `[ ]`→`[~]` committed alone as `start: <task summary>`; the final
  `[~]`→`[x]` + spec Findings in the finishing commit.
- **Git**: branch off fresh `main`; the PR (conventional-commit title — it
  becomes the squash commit on main); auto-merge; green confirmation.
- **Host state**: `cargo leptos watch --features component-bench` running in
  the background (repo root; it binds :3000 after the first build — check
  `lsof -i :3000` before assuming it's down) and the Android emulator.
  Subagents *use* these; only the orchestrator starts or restarts them.
- **Arbitration**: review findings route back to the implementer; the
  orchestrator never edits code to resolve them itself.

## Sequence

0. **Start** — first available Phase 5 task per "Working the queue". Branch off
   fresh `main`, flip `[ ]`→`[~]`, commit exactly that. Read the task line and
   skim its `(specs:)` links only enough to write the dispatch prompt — deep
   spec reading is the implementer's job, in its own context.

1. **Implement** — dispatch an implementation subagent (general-purpose). The
   dispatch prompt contains, explicitly (the subagent inherits nothing from
   this conversation):
   - the task line **verbatim** from specs/TODO.md; the branch name; that a
     watch server with `component-bench` is already serving :3000;
   - required reading *before code*: the task's sections of specs/app-ui.md,
     every spec in its `(specs:)` annotation, and design/wireframes.pen;
   - skills to invoke: **vendor-component** for any new primitive (bench
     section, same commit), **e2e-suite** for authoring/running tests,
     **android-smoke** for the dev-attach recipe;
   - the evidence it must return: unit tests green; web probes run with URLs
     (`node end2end/hydration-check.mjs <urls…>` — **bare = checks nothing**;
     probe every page that renders a touched component, plus a page-specific
     SSR curl against :3000); Android webview check
     (`node end2end/android-cdp-check.mjs` + the task's spec against the
     webview when it touches layout/input); e2e authored per e2e-suite, fast
     tier while iterating, **full three-browser tier green at the end**
     (`npx playwright test`);
   - constraints: never restart the :3000 watch; `git checkout` the
     build-injected AndroidManifest.xml before committing; commit its own work
     with conventional messages; do **not** touch specs/TODO.md or spec
     Findings (orchestrator's job); report back a summary, the evidence, and
     anything the spec should record.

2. **Review** — dispatch a review subagent with: "Invoke the
   **adversarial-review** skill and follow it", the branch name, and the task's
   acceptance criteria as focus text. Both passes (implementation +
   test-strength). Fresh agent — never the implementer, and never done in the
   main session; the value is the independence.

3. **Fix loop** — forward the findings to the implementation agent
   (SendMessage — its context is intact) with the standing policy: **verify
   every finding before acting, never blind-apply**; dispute with evidence
   what doesn't hold. Then re-dispatch a *fresh* review subagent scoped to the
   findings and the files that changed. Repeat until the verdict is CLEAN or
   everything remaining is disputed-with-rationale. If the implementer is gone
   (ended/overflowed), dispatch a fresh fix agent with the branch, the
   findings, and pointers to the spec sections — not a paraphrase of the task.

4. **Gate** — dispatch a subagent to run the **validate** skill and return the
   per-step verdict table. Red → back to step 3 with the failing output.

5. **Land** — finishing commit flips `[~]`→`[x]` **and** records Findings in
   the linked spec in the same commit: surprises, review rounds + score,
   disputed findings with rationale, evidence summary (from the subagents'
   reports). Conventional-commit PR title; enable auto-merge; confirm green.
   Then loop to step 0 for the next task — stop only at a phase boundary, on a
   blocked queue, or when the human said one task.

## Failure policy (from the spec — not optional)

- A red platform check blocks `[x]`. No exceptions.
- Emulator unavailable → Findings entry "Android smoke deferred: emulator
  offline" + flag the maintainer. Never silently skip.
- E2E flake → one retry, then quarantine with `@flaky` tag + Findings entry.
  Quarantined tests block the phase-final polish task.
- Review disagreements resolve by verification, not deference — disputed
  findings ship only with recorded rationale.
- Durable state = branch + TODO checkbox + spec Findings only. Subagent IDs
  and server PIDs are session-ephemeral — re-derive (`lsof -i :3000`), never
  persist. A resumed `[~]` task means: inspect the branch, re-dispatch from
  whatever step the evidence shows incomplete.

## Dispatch gotchas

- Subagent prompts are self-contained — name files, branches, and skills
  explicitly; "the task we discussed" means nothing to a clean context.
- Subagents share the host: they can use the running watch server, adb, and
  the emulator, but must not own their lifecycle (restart requests come back
  to the orchestrator).
- `cargo tauri android dev` runs from the **repo root** (its beforeDevCommand
  `cd ..` resolves against the invocation dir; from src-tauri/ it dies with
  "manifest path `Cargo.toml` does not exist").
- The full three-browser tier runs at the end of **every** task, not at stage
  boundaries. A stage-boundary task additionally runs the Android **release**
  smoke via the android-smoke skill (at phase end only).
