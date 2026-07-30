# Batch L — CI / docs / hygiene

Triage verification pass over `P6-018`, `P6-025`, `P6-032`, `P6-066`, `P6-067`,
`P6-081`, `P6-082`, `P6-084`, `P6-085`, `P6-089`, `P6-106`, `P6-107`. Read-only;
no fixes proposed, no builds/tests run.

## P6-018 — `/my/all` missing from both route maps

- **Verdict:** CONFIRMED. Check: grep both route tables for `/my/all`.
- **Evidence:** `specs/app-ui.md:62-71` route table lists `/`, `/catalog`,
  `/cards/:id`, `/my`, `/my/collections/:id`, `/my/collections/:id/needs`,
  `/my/shopping`, `/login`/`/signup` — no `/my/all` row. Same at
  `design/information-architecture.md:99-108`. The route demonstrably exists in
  the app: `app-ui.md:327` (Findings) says "`/my` and `/my/all` share
  `AllCardsPayload`", so it's a live route absent from both authoritative maps.
- **Size:** S.
- **Disposition:** KEEP. Two one-line table additions.
- **blocked-by/duplicate-of:** none.

## P6-025 — three tree-move claims needing a runtime check

- **Verdict:** CONFIRMED (claim is "needs a runtime check", which is still
  true), but the three cited line numbers have drifted. Check: read the three
  cited call sites.
- **Evidence:** `app/src/my/tree_manage.rs` — `set_timeout(` now at line **626**
  (entry cites 465); `app/src/shell.rs` — the `invisible … md:visible` rail
  class now at line **447** (entry cites 381); `app/src/my/tree.rs` — the
  `md:opacity-0 md:group-hover/row:opacity-100 md:focus-visible:opacity-100`
  class now at line **569** (entry cites 508). All three constructs still exist
  substantively unchanged (same classes/pattern), only their line refs rotted
  from intervening edits. No runtime/DOM check has been done — `toBeVisible()`
  still can't distinguish `opacity-0` from visible, so (c) is still unpinned.
- **Size:** S.
- **Disposition:** KEEP, RESCOPE — refresh the three line refs when picked up;
  the actual work is still a runtime/computed-style check, not a code change.
- **blocked-by/duplicate-of:** none.

## P6-032 — `.claude/skills` ⇄ `.agents/skills` parity

- **Verdict:** CONFIRMED, and drift has **recurred** since the 2026-07-26
  resync. Check: `diff -rq .claude/skills .agents/skills`; `git log --since
  2026-07-26` on both trees; read `.github/workflows/validate.yml` for a parity
  step.
- **Evidence:** `diff -rq .claude/skills .agents/skills` (exit 1) reports:
  `Only in .claude/skills: i-have-adhd` and `Files .claude/skills/phase-6-review/SKILL.md
  and .agents/skills/phase-6-review/SKILL.md differ`. Root cause: commit
  `80c7395` (2026-07-28, "shape phase-6-review grill replies with
  i-have-adhd") added `.claude/skills/i-have-adhd/SKILL.md` and edited
  `.claude/skills/phase-6-review/SKILL.md` only — `.agents/skills/` untouched
  by either that commit or its follow-up `2cd1b6d`. So `.agents/skills/phase-6-review/SKILL.md`
  is now missing the i-have-adhd-shaping section entirely (confirmed via
  `diff` of the two files: 26 lines present only in `.claude`'s copy).
  `validate.yml` (read in full) has six clippy steps + test + `cargo leptos
  build --release` and no `diff -rq`/parity step of any kind — the gate still
  does not enforce this.
- **Size:** S.
- **Disposition:** KEEP — and the priority note in the triage doc undersells
  it: the "one-time repair, structural gap" framing already proved out (drift
  recurred within 2 days of the resync, from an otherwise unrelated skills
  commit). Worth flagging for a priority bump when re-filed.
- **blocked-by/duplicate-of:** none.

## P6-066 — superseded by P6-060, triage says delete

- **Verdict:** CONFIRMED delete is correct. Check: read `P6-060`'s entry text
  and search `ui-work-loop.md` for an existing Findings record.
- **Evidence:** `specs/TODO-Phase-6.md:78` (`P6-060`) already contains the full
  narrative: "**Supersedes** the batch-move task's earlier 'suspected flake,
  did not reproduce' note on `smoke.spec.ts:92` — it does reproduce; a single
  green run was not evidence of stability." So the "wrong diagnosis" record
  triage wants preserved already lives on `P6-060`'s own entry, not only on
  `P6-066`. `grep -i 'smoke.spec.ts:92\|wrong diagnosis'
  specs/ui-work-loop.md` returns nothing — no separate Findings record exists
  there yet, but none is needed since `P6-060` already carries it.
- **Size:** — (delete, not sized).
- **Disposition:** DROP. Delete the entry; no migration to `ui-work-loop.md`
  Findings needed since `P6-060`'s own text already preserves the record.
- **blocked-by/duplicate-of:** duplicate-of / superseded-by `P6-060`.

## P6-067 — component-gap-analysis entry for custom (non-vendored) components

- **Verdict:** CONFIRMED still an open decision (entry's own premise —
  "the analysis already records `action_bar` as ruled out" — checks out, and
  the underlying policy question is still unanswered). Check: read
  `design/component-gap-analysis.md` and `.claude/skills/vendor-component/SKILL.md`.
- **Evidence:** `design/component-gap-analysis.md:45` lists "Selection tray
  (docked…)" as **Gap**, and `:101` fully narrates the `action_bar`
  ruling-out — so the doc is *not* actually missing the tray, matching what
  the entry itself already says. `.claude/skills/vendor-component/SKILL.md:17,28,64`
  phrase the record-a-deviation instruction only in terms of an upstream
  component to deviate from ("upstream path… every deviation listed",
  "record discovered deviations in the component header *and* the gap
  analysis") — it never explicitly scopes itself to "vendored only" vs. "any
  gap component," so the decision the entry asks for is still unmade.
- **Size:** S, decision first.
- **Disposition:** KEEP — this is cheap to close since all the evidence is
  already gathered; PROMOTE to a quick maintainer decision rather than a
  research task.
- **blocked-by/duplicate-of:** none.

## P6-081 — cross-browser (firefox/webkit) pass deferred wholesale

- **Verdict:** CONFIRMED, debt still unmeasured. Check: read
  `end2end/playwright.config.ts` projects list and the `e2e-suite` skill
  description.
- **Evidence:** `end2end/playwright.config.ts` still defines all three
  projects — `chromium`, `firefox`, `webkit` (each with `dependencies:
  ["setup"]`) — so `npx playwright test` (no `--project` filter) would still
  exercise all three. The file's own comment is now stale/contradictory: it
  claims "The full three-browser tier runs at the end of EVERY task (revised
  2026-07-20)" while the `e2e-suite` skill's current description says
  "chromium-only tiers … firefox and webkit are never run" — confirming actual
  practice diverged from that comment, matching the entry's claim that the
  debt "accumulates unseen."
- **Size:** M, decision.
- **Disposition:** PARK — trigger: before any release milestone that claims
  desktop/webkit-class browser support, or on a cadence (e.g., once per phase).
  Low-cost side finding: `playwright.config.ts`'s comment block is itself
  stale and should be corrected whenever this is next touched, independent of
  when the pass runs.
- **blocked-by/duplicate-of:** none.

## P6-082 — assertion-strength / mutation-testing sweep

- **Verdict:** CONFIRMED. Check: `.claude/skills/e2e-suite/SKILL.md` for
  mutation-pass status.
- **Evidence:** `.claude/skills/e2e-suite/SKILL.md:133` states mutation passes
  are "switched off" in the loop, and `:142` records "three vacuous tests in
  one task" as the guidance-not-check precedent the entry describes.
- **Size:** L.
- **Disposition:** PARK — trigger already named in the entry: "once the Phase
  5 spec work lands." Confirm whether that trigger has since fired; if Phase 5
  spec work is done, this should move to KEEP.
- **blocked-by/duplicate-of:** none.

## P6-084 — Google sign-in doesn't honor `?next`

- **Verdict:** CONFIRMED. Check: read `app/src/account.rs`'s `google_sign_in`
  server fn.
- **Evidence:** `app/src/account.rs:372-428` — `google_sign_in()` builds the
  callback/error URLs (`{origin}/auth/callback`, `{origin}/login?error=google`,
  or the Android bounce `{web}/auth/app-return`) purely from `origin`/`native_origin`;
  no `next`/state parameter is read from the request or threaded into any of
  those URLs or into `upstream::social_start`'s challenge. `grep -i
  'next|state param'` over `app/src/auth.rs` turns up nothing OAuth-related
  (only an unrelated iterator `.next()`).
- **Size:** M.
- **Disposition:** KEEP.
- **blocked-by/duplicate-of:** none.

## P6-085 — `+ Want` cannot be undone

- **Verdict:** CONFIRMED. Check: grep `shared/src`, `app/src/backend/{hosted,native}.rs`
  for `set_desire_quantity` / `QuickAddReceipt`.
- **Evidence:** `shared/src/collection.rs:461-462` — `QuickAddReceipt` still
  has only `pub undo_move_id: Option<Id>`, no desire-id field. No
  `set_desire_quantity` exists anywhere (`hosted.rs`/`native.rs` only have
  `add_desire` at `hosted.rs:667`/`native.rs:330` and `set_holding_quantity` at
  `hosted.rs:696`/`native.rs:339`) — matches the entry's claim exactly, down to
  "the quick-add adapter has no desire id today."
- **Size:** M.
- **Disposition:** KEEP.
- **blocked-by/duplicate-of:** none.

## P6-089 — `#![recursion_limit]` invisible to clippy

- **Verdict:** CONFIRMED, gate still lacks the closing step. Check: full read
  of `.github/workflows/validate.yml` (no build run, per instructions).
- **Evidence:** `validate.yml`'s `validate` job steps are: fmt check, six
  clippy invocations (native workspace, frontend/wasm, native backend,
  component-bench ×2), `cargo test --workspace --exclude frontend`, then
  `cargo leptos build --release`. No standalone `cargo build -p three_rings`
  (or any codegen-level build of the Tauri crate outside the leptos
  build) exists — nothing in the gate would catch a regressed
  `#![recursion_limit]`.
- **Size:** S.
- **Disposition:** KEEP — already fully diagnosed with rationale in the entry;
  PROMOTE candidate (one-line gate addition, no new tooling needed since the
  runner already installs Tauri's Linux libs).
- **blocked-by/duplicate-of:** none.

## P6-106 — per-card `card_tags` orphan cleanup

- **Verdict:** CONFIRMED, still open/filed, matches spec almost verbatim.
  Check: read `specs/card-tagging.md` orphan-cleanup note; grep `hosted.rs`
  for any per-line cleanup on holdings/desires write paths.
- **Evidence:** `specs/card-tagging.md:333-336` states whole-collection
  teardown is covered by `ON DELETE CASCADE` on `card_tags.collection_id`, and
  "per-card orphan cleanup on the *last line* leaving rides the
  holdings/desires write paths and is a thin follow-up if a UI surfaces stale
  tags (filed)" — this is the entry, already filed and cross-referenced.
  `card_tags` hits in `hosted.rs` (lines 1556, 1637-1718, 2446) are all
  tag-assignment CRUD, not orphan cleanup triggered from holdings/desires
  writes — no code implements the per-line case.
- **Size:** S.
- **Disposition:** PARK — trigger is the spec's own condition: "if a UI
  surfaces stale tags."
- **blocked-by/duplicate-of:** none.

## P6-107 — bundled read-only catalog for offline browsing

- **Verdict:** CONFIRMED still unimplemented / deliberately deferred. Check:
  grep `app/src`, `shared/src` for any bundled/offline-catalog feature code.
- **Evidence:** No bundled-catalog code anywhere; all "offline" hits in
  `app/src` (`backend/native.rs:87-88`, `catalog/rail.rs:922`,
  `catalog/destination.rs:418`, `components/states.rs:50,343`,
  `bench/states.rs:107`) are about the native backend's "offline phone as
  ordinary failure" error-handling story, an unrelated concern. This entry
  remains a genuinely separate, unstarted feature.
- **Size:** L.
- **Disposition:** PARK — no trigger named beyond the entry's own
  "deliberately deferred"; treat as a backlog/roadmap item, not near-term.
- **blocked-by/duplicate-of:** none.
