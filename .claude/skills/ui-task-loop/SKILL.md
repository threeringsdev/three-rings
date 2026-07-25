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

Platform matrix: **chromium + Android webview every task; firefox and webkit
not run at all; desktop ignored in-loop; one Android release smoke at phase
end** (the polish task).

## What this loop is for (read before tuning anything)

Three-rings is an **early-stage MVP with a single developer**. It is not a
hardened production service, and the loop must not spend like one. The
calibration below is deliberate, set by the maintainer after one task cost
~2.5 h and 1.44M subagent tokens (2026-07-25, specs/ui-work-loop.md Findings):

- **Ship the feature working.** Correctness of what the user touches.
- **Do not chase craft.** Naming, comment accuracy, assertion elegance,
  redundancy, "this could be cleaner" — not now.
- **Minors are not fixed. They are filed.** Every non-major finding becomes a
  `[ ]` entry under **Phase 5 discoveries** in specs/TODO.md. Filing is the
  correct outcome, not a concession.
- **When in doubt, spend less.** A second look costs more than it returns at
  this stage.

## The loop break — when this ends

**The loop runs task after task until Stage 3 has no `[ ]` left, then stops.**
That is the only completion condition. On finishing a task the orchestrator goes
straight back to step 0 and starts the next one without asking; it stops early
only if the human says so, the queue is blocked (every remaining `[ ]` gated by a
`draft` spec), or a task cannot reach `[x]`.

This terminates **only** because of the next rule, which is therefore not
optional:

> ### Never add a task to Stage 3. Ever.
>
> Everything discovered while working — out-of-scope bugs, review minors,
> deferred follow-ups, quarantined tests, "we should also…" — goes into
> **`### Phase 5 discoveries`** under `## Later / parked`. Never into Stage 3,
> never into another stage, never into the stage list "just this once".
>
> This **overrides CLAUDE.md's** definition of done ("newly discovered follow-up
> work added as new `[ ]` tasks in the right phase") for the duration of this
> loop. During Phase 5 the right phase *is* the discoveries pen. A Stage 3 that
> grows while being worked never empties, and the loop never breaks.

The Stage 3 list is a **fixed backlog being drained**, not a living queue. Its
length only goes down.

Between tasks the orchestrator keeps its own context (queue state, host
processes, what shipped) and **discards the subagents** — every task gets a
fresh implementer and a fresh reviewer. Never carry a previous task's
implementation agent into a new task: its context is full of the last feature,
which costs tokens and leaks stale assumptions into the new one.

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
  orchestrator never edits code to resolve them itself. The orchestrator also
  overrides the reviewer's severity when it looks wrong — the major/minor call
  decides whether work happens now or gets filed.

## Sequence

0. **Start** — first available Phase 5 task per "Working the queue". Branch off
   fresh `main`, flip `[ ]`→`[~]`, commit exactly that. Read the task line and
   skim its `(specs:)` links only enough to write the dispatch prompt — deep
   spec reading is the implementer's job, in its own context.

   **If the task line bundles several surfaces, say so before dispatching.**
   A line like "binder view + deck variant + stepper + teardown + mobile +
   search + paging" is three tasks wearing one checkbox, and one agent
   absorbing it in one context is a top cost driver. Propose the split to the
   maintainer and wait.

1. **Implement** — dispatch an implementation subagent (general-purpose). The
   dispatch prompt contains, explicitly (the subagent inherits nothing from
   this conversation):
   - the task line **verbatim** from specs/TODO.md; the branch name; that a
     watch server with `component-bench` is already serving :3000;
   - required reading *before code*: the task's sections of specs/app-ui.md,
     every spec in its `(specs:)` annotation, and design/wireframes.pen;
   - skills to invoke: **vendor-component** for any new primitive (bench
     section, same commit), **e2e-suite** for authoring tests,
     **android-smoke** for the dev-attach recipe;
   - the evidence it must return: unit tests green; web probes run with URLs
     (`node end2end/hydration-check.mjs <urls…>` — **bare = checks nothing**;
     probe every page that renders a touched component, plus a page-specific
     SSR curl against :3000);
   - that it **authors** e2e tests now but runs only
     `--project=chromium --grep @fast` while iterating — **the e2e run is
     step 4, after review fixes**, and it must not run a full pass early;
   - constraints: never restart the :3000 watch; `git checkout` the
     build-injected AndroidManifest.xml before committing; commit its own work
     with conventional messages; do **not** touch specs/TODO.md or spec
     Findings (orchestrator's job); report back a summary, the evidence, and
     anything the spec should record.

2. **Review — exactly one round, code only** — dispatch a review subagent with:
   "Invoke the **adversarial-review** skill and follow it", the branch name,
   and the task's acceptance criteria as focus text. Fresh agent — never the
   implementer, and never done in the main session; the value is the
   independence.

   Hard limits, restated in the dispatch:
   - **One round. There is no re-review.** Whatever the fixes look like, the
     loop moves to step 3 and then step 4.
   - **Reads code; runs no tests.** No Playwright, no mutation pass, no
     rebuild cycles. Static reading plus, at most, a curl against the
     already-running :3000.
   - **Reports blockers and majors only.** A major is: wrong data, data loss,
     a broken user-facing path, a security or auth hole, or a crash. Everything
     else is a minor — listed separately to be filed, never fixed.

3. **Fix the majors** — forward the blockers/majors to the implementation
   agent (SendMessage — its context is intact) with the standing policy:
   **verify every finding before acting, never blind-apply**; dispute with
   evidence what doesn't hold. Minors go straight into the orchestrator's
   notes for the finishing commit — they are **not** sent to the implementer.
   If the implementer is gone (ended/overflowed), dispatch a fresh fix agent
   with the branch, the findings, and pointers to the spec sections.

4. **E2E — the single run, after the fixes** — one pass, chromium only:
   `npx playwright test --project=chromium`, plus the Android webview probe
   (`node end2end/android-cdp-check.mjs`, and the task's own probe when it
   touches layout or input). This is the only full e2e execution in the task.
   - A **major** failure (a genuinely broken behavior) → back to step 3 for
     that failure only.
   - A **minor** failure or a flake → one retry; still red → quarantine with
     `@flaky` and file it under Phase 5 discoveries.

5. **Gate** — dispatch a subagent to run the **validate** skill and return the
   per-step verdict table. Red → fix, then re-run the affected step only. Skip
   the gate only when the diff since the last green run touches no compiled
   sources (`.rs`/`.toml`/`.css`) — and say so explicitly when you skip.

6. **Land** — finishing commit flips `[~]`→`[x]` **and** records Findings in
   the linked spec in the same commit: surprises, the review's majors and how
   they resolved, disputed findings with rationale, evidence summary. **File
   every minor as a `[ ]` under Phase 5 discoveries in the same commit** — that
   is where the deferred work is preserved, so it is not optional.
   Conventional-commit PR title; enable auto-merge; confirm green.

   Then **go straight back to step 0** with the next Stage 3 `[ ]` — no check-in,
   no "shall I continue?". Report the task landed in a few lines and start the
   next. Stop only per **The loop break** above: no `[ ]` left in Stage 3, the
   human says stop, or the queue is blocked.

## Failure policy

- A red **chromium** e2e run or a red gate blocks `[x]`. Firefox and webkit are
  not run and never block.
- Emulator unavailable → Findings entry "Android smoke deferred: emulator
  offline" + flag the maintainer. Never silently skip.
- E2E flake → one retry, then quarantine with `@flaky` + a Phase 5 discoveries
  entry. Quarantined tests block the phase-final polish task.
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
- The e2e `tr_jwt` cookie expires in ~20 minutes. The symptom is a page
  rendering `unauthorized: invalid token`, which reads like a page bug — it is
  an expired cookie. `npx playwright test --project=setup` refreshes it.
- `cargo tauri android dev` runs from the **repo root** (its beforeDevCommand
  `cd ..` resolves against the invocation dir; from src-tauri/ it dies with
  "manifest path `Cargo.toml` does not exist"). Prefer the cheaper recipe: the
  already-installed debug APK plus `adb reverse tcp:3000 tcp:3000` gives live
  dev-server content in the real webview with no second watch fighting the
  first over `target/`. Match the CDP socket to the app's **own** pid
  (`webview_devtools_remote_$pid`), not `head -1` of the socket list.
- If you stop a subagent mid-flight, **check `git status` before committing** —
  a killed agent leaves its edits applied, including experimental ones.
