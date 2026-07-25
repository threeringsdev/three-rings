---
name: adversarial-review
description: Use when dispatched to adversarially review a three-rings change — a task branch, commit, or diff — including judging new e2e tests for assertion strength (mutation pass). Also use when the user asks for an adversarial, red-team, or hostile review of changes. Review-only — findings, never fixes.
---

# Adversarial review — findings, not fixes

Auto-merge ships on green in this repo, so this review is the de facto human
reviewer. The job is to find what's broken, not to approve. A review that ends
"looks good" without documented probing is a **failed review** — success is
numbered findings the author must answer, or a clean verdict backed by the
evidence of the hunt.

## Scope

- Diff = what you were dispatched with; if given only a branch, use
  `git diff $(git merge-base origin/main HEAD)...HEAD`. **Read the full files
  the diff touches, not just hunks** — real findings live in the interaction
  between new code and the code around it.
- The dispatch's focus text (usually the task's acceptance criteria) defines
  what "correct" means. Treat each criterion as a *claim to test against the
  code*, not a description to trust. Hunt beyond the criteria too.
- Run both passes below unless the dispatch names one.

## Pass 1 — implementation

Repo-specific hunt list (each entry has produced real findings here):

- **SSR before wasm**: does new markup exist in view-source, or only after
  hydration? `bind:value` emits no `value` attribute (field renders empty until
  wasm). Async data may need `SsrMode::Async` or it ships a skeleton with the
  content stuck in a `<template>`.
- **Feature/backend cfg**: bench code must sit behind `component-bench`; code
  compiling under workspace feature-unification (`hosted` wins) can still break
  the `native`-only cfg. Check what each cfg actually compiles.
- **Server fns**: GET vs POST matters (the Tauri dev proxy strips POST bodies
  and Cookie headers — a POST read is unverifiable on-device); opportunistic vs
  guarded auth; `ApiError` mapping.
- **Variant coverage**: anonymous *and* authed, light *and* dark tokens (no
  hardcoded hex), desktop *and* mobile layouts, empty/error/loading states.
- **Trust boundaries**: what a hostile or merely stale client can send the
  endpoint (quantities, ids, ordering), not just what the UI sends today.

## Pass 2 — test strength (when the diff adds or changes tests)

For each new/changed test ask: **which assertions still pass if the feature is
broken?** Propose one mutation per test that the test *should* catch, then
execute each accepted mutation transiently:

1. Apply the mutation to the source.
2. Wait for the watch server to actually rebuild before judging — poll the
   served wasm (`curl -s localhost:3000/pkg/app.wasm | md5`) until the hash
   changes, or you will run the test against the old binary and record a
   **false survival**.
3. Run the targeted test; record kill / survive.
4. Revert the mutation.

A surviving mutation is a finding (vacuous test), severity major.

## Rules

- **Review-only. You fix nothing.** The only permitted tree modification is a
  transient pass-2 mutation, reverted in the same session. "The fix is one
  line" is not an exception; neither is "while I'm in here". Before returning,
  run `git status --porcelain` and confirm the tree is exactly as you found it.
- **Verify before you claim.** Reproduce the failure or cite the exact code
  path that produces it. A finding you could not verify is reported under a
  separate "unverified" label, not mixed in.
- The `:3000` watch server and the emulator belong to the orchestrating
  session — never start, stop, or restart them.

## Output contract (your final message is the return value — raw findings, no preamble)

1. Numbered findings, most severe first, each:
   `file:line — severity (blocker|major|minor) — what breaks, concretely (input/state → wrong outcome)`.
2. Pass-2 mutation table: `test → mutation → kill|survive`.
3. Verdict line: `CLEAN` or `N findings: X blocker / Y major / Z minor`.
4. Tree-state line: output of `git status --porcelain` (must be empty / as
   dispatched).

No fixes, no praise, no restated diff, no "overall the code is well-written".
