---
name: adversarial-review
description: Use when dispatched to adversarially review a three-rings change — a task branch, commit, or diff — by reading code for blockers and majors. Also use when the user asks for an adversarial, red-team, or hostile review of changes. Review-only — findings, never fixes. Runs no tests.
---

# Adversarial review — blockers and majors, by reading

Auto-merge ships on green in this repo, so this review is the de facto human
reviewer. The job is to find what's **broken**, not to approve and not to
polish.

## Calibration — read this first

Three-rings is an **early-stage MVP with a single developer**, not a hardened
production service. This review is deliberately cheap and deliberately narrow:

- **One round.** There is no re-review. Say everything you have to say now.
- **Read code. Run nothing.** No Playwright, no test execution, no mutation
  pass, no rebuild cycles. You may `curl` the already-running :3000 and read
  anything on disk. That is the whole toolkit.
- **Report blockers and majors only.** Everything else goes in a short,
  separate minors list that the orchestrator files as future work — it will
  not be fixed now, and that is the intended outcome.

**A major is:** wrong data shown or stored · data loss · a broken user-facing
path · a security or auth hole · a crash or hydration panic.

**A minor is everything else**, including: naming, comment accuracy, duplicated
logic, missing test coverage, weak or redundant assertions, "this could be
cleaner", style, and unreachable-in-practice edge cases. Do not argue for
promoting a minor. File it and move on.

Do not pad the majors list to look thorough. Zero majors is a legitimate,
useful result. Padding costs the maintainer real time.

## Scope

- Diff = what you were dispatched with; if given only a branch, use
  `git diff $(git merge-base origin/main HEAD)...HEAD`. **Read the full files
  the diff touches, not just hunks** — real findings live in the interaction
  between new code and the code around it.
- The dispatch's focus text (usually the task's acceptance criteria) defines
  what "correct" means. Treat each criterion as a *claim to test against the
  code*, not a description to trust.

## Hunt list (each entry has produced a real major here)

- **Destructive writes with a lying affordance**: a mutation that deletes or
  zeroes a row while the UI offers an Undo that cannot work. Trace the undo
  path to the endpoint, not just the toast.
- **Joins that drop rows**: an inner join where a `FULL OUTER JOIN` is meant —
  the wanted-but-not-held card that vanishes from a view while a counter on
  the same page still counts it. Twice now.
- **SSR before wasm**: does new markup exist in view-source, or only after
  hydration? `bind:value` emits no `value` attribute (field renders empty until
  wasm). Async data may need `SsrMode::Async` or it ships a skeleton with the
  content stuck in a `<template>`. Reading a resource in render is a hydration
  panic (`tachys` "expected a marker node").
- **Feature/backend cfg**: bench code must sit behind `component-bench`; code
  compiling under workspace feature-unification (`hosted` wins) can still break
  the `native`-only cfg.
- **Server fns**: GET vs POST matters (the Tauri dev proxy strips POST bodies
  and Cookie headers — a POST read is unverifiable on-device); opportunistic vs
  guarded auth; `ApiError` mapping.
- **Two numbers on one page computed two ways**: a header total and its rows,
  a badge and its list. Check they group by the same key — a board-blind total
  beside board-aware rows contradicts itself on screen.
- **Trust boundaries**: what a hostile or merely stale client can send the
  endpoint (quantities, ids, ordering), not just what the UI sends today.
- **Variant coverage**: anonymous *and* authed, desktop *and* mobile layouts.

## Rules

- **Review-only. You fix nothing.** "The fix is one line" is not an exception;
  neither is "while I'm in here". Before returning, run `git status --porcelain`
  and confirm the tree is exactly as you found it.
- **Verify before you claim** — by reading the code path end to end, or with a
  curl against the running server. A finding you could not verify goes under a
  separate "unverified" label, not mixed in with the majors.
- The `:3000` watch server and the emulator belong to the orchestrating
  session — never start, stop, or restart them.

## Output contract (your final message is the return value — raw findings, no preamble)

1. **Majors**, numbered, most severe first, each:
   `file:line — blocker|major — what breaks, concretely (input/state → wrong outcome)`.
   Empty list is fine — write `None`.
2. **Minors to file** — one line each, terse, no argument for why they matter.
3. Verdict line: `CLEAN` or `N majors (X blocker / Y major) + M minors to file`.
4. Tree-state line: output of `git status --porcelain` (must be empty).

No fixes, no praise, no restated diff, no "overall the code is well-written".
