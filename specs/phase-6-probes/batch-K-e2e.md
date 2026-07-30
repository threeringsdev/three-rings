# Batch K — e2e / test strength / dev loop

Triage verification pass. Read-only; no fixes proposed, no tests run, no DB
connections made. Neon dev-branch data claims are marked UNVERIFIABLE with
the check that would confirm them.

## P6-004 — stale watch-server read indistinguishable from a pass

- **verdict**: CONFIRMED
- **evidence**: `.claude/skills/e2e-suite/SKILL.md` has no "poll a marker, not
  elapsed time" authoring rule anywhere (full file read). The "already-recorded
  trap" the entry points at (`cargo leptos watch` silently drops a save
  mid-rebuild) is *not* actually in a skill either — it only exists as prose in
  a Findings entry, `specs/app-ui.md:1157-1160` ("Liveness was proved... rather
  than trusting the watch — which matters, since `cargo leptos watch` has been
  observed silently dropping a save mid-rebuild"). Neither trap has been
  promoted.
- **size**: S
- **disposition**: MERGE → one commit with P6-040b + P6-077 (three e2e
  authoring traps into `e2e-suite` skill). Grouping still correct.

## P6-027 — `@fast` tier has a real data race, not just hydration flake

- **verdict**: CONFIRMED
- **evidence**: No record of this finding (tree-move writes racing
  command-palette/needs reads on the shared seeded user) anywhere in
  `specs/ui-work-loop.md` or `specs/app-ui.md` (grepped for "parallel worker",
  "shared fixture", "8 worker" — only unrelated hits). `end2end/playwright.config.ts:19`
  still sets `workers: process.env.CI ? 1 : undefined` (parallel by default
  locally); the e2e-suite skill's `--workers=1` full-tier rule (lines 100-110)
  mitigates the *symptom* for the official run but the fixture-sharing root
  cause this entry names is undocumented elsewhere and unfixed.
- **size**: S to record as a Finding; the actual fix (isolate tree-move's
  fixture data) is a separate, unfiled, larger (M/L) task.
- **disposition**: PARK — trigger: "before parallel workers are reconsidered
  for the loop or CI." No value in acting now since `--workers=1` already
  covers the practical mitigation; record the finding in `ui-work-loop.md`
  Findings when convenient.

## P6-037 — recurring vacuous-test shape, filed as a pattern

- **verdict**: PARTLY
- **evidence**: Of the three generalizable guards, only one has been folded
  into the skill: `.claude/skills/e2e-suite/SKILL.md:138-142` ("A test can
  only distinguish two behaviors the **fixture** distinguishes... Check the
  fixture actually contains the shape you are asserting on"). The "positive
  control for a negative assertion" and "assertion sharing provenance with the
  code under test" guards are not generalized (the skill's popover note at
  line 129-131 is a specific instance, not the general rule). The concrete 4th
  instance is still live and unfixed: `end2end/tests/catalog.spec.ts:601`
  — `expect(ownedBadgeFor(page, none!.oracle_id)).toHaveCount(0)` — has no
  companion assertion that `none`'s tile itself rendered (grepped the file;
  no `tileFor(page, none...)` or equivalent exists).
- **size**: S
- **disposition**: KEEP — finish promoting the remaining two guards into
  `e2e-suite` and add the missing positive-control assertion at
  `catalog.spec.ts:601`.

## P6-040 — promote two traps out of code comments

### (a) `{..}` struct-update-syntax trap → vendor-component

- **verdict**: CONFIRMED
- **evidence**: `.claude/skills/vendor-component/SKILL.md` (full file read) has
  no mention of the bare-path-before-`{..}` parse trap (E0797) anywhere in its
  "Trap checks" section — the closest content is the unrelated "Extra
  attributes... pass anything else via spread" note, which is about a
  different concern. The trap has now been rediscovered/re-commented **four**
  times, not two: `app/src/cards.rs:379-380`, `app/src/catalog.rs:751`,
  `app/src/components/query_bar.rs:196`, and
  `app/src/components/ui/selection_tray.rs:236`, plus write-ups in
  `specs/app-ui.md:1060,1851,2088`.
- **size**: S
- **disposition**: PROMOTE into `vendor-component`'s Trap checks list.

### (b) `cargo leptos watch` drops a save mid-rebuild / `touch` doesn't retrigger

- **verdict**: CONFIRMED
- **evidence**: Same gap as P6-004 — the trap lives only in
  `specs/app-ui.md:1157-1160` prose, not in `e2e-suite` (or any skill). The
  `touch`-doesn't-retrigger nuance (content-hash based, not mtime) isn't
  recorded anywhere either.
- **size**: S
- **disposition**: MERGE → same commit as P6-004 + P6-077.

## P6-052 — dev fixture Inbox has accumulated Lightning Bolt desires

- **verdict**: PARTLY — code-side mechanism CONFIRMED; count/dominance claim
  UNVERIFIABLE (Neon dev-branch data, not queried per instructions)
- **evidence**:
  - Code: `end2end/tests/destination-picker.spec.ts:187-209`, test
    `"+ Want confirms but offers no undo"`, quick-adds Lightning Bolt into the
    real Inbox (`destination-label` asserted `/Inbox/` at line 191) every run,
    and explicitly documents there is no undo path for a want ("desires are
    outside the move ledger and there is no compensating operation" — lines
    204-205). This is exactly the unbounded-growth mechanism the entry
    describes; `quick-add.spec.ts` itself, by contrast, already scopes its
    writes into `zz-e2e-quickadd-*` scratch collections with `finally` cleanup
    (lines 59-63, 150-402) and never touches "Lightning Bolt" at all — so the
    growth comes from `destination-picker.spec.ts`, not `quick-add.spec.ts` as
    the entry's phrasing might suggest; same underlying claim ("quick-add
    tests... don't scope their own wants"), just a different spec file.
  - `end2end/cleanup-mutation-leftovers.mjs` exists (confirmed), is not in
    `end2end/package.json` `scripts` (confirmed absent), and only cleans
    **holdings** (`view.cards` filtered then removed via `/api/moves`) — it
    does not touch desires/wants at all, so even wiring it in as-is would not
    address the Inbox-desire accumulation this entry is about.
  - UNVERIFIABLE (named check): the literal "88 desires, shortfall 86" and
    "`/my/shopping` dominated by it" claims require either querying the Neon
    **dev** branch's desires table for the e2e user's Inbox filtered to
    Lightning Bolt printings, or loading `/my/shopping` in a browser against
    the dev server — neither was done per the no-DB-connection instruction.
- **size**: S
- **disposition**: KEEP, grouped with P6-065 + P6-075 (+ P6-009g) as the
  triage doc proposes.

## P6-060 — `hydrated(page)` doesn't imply a streamed island is interactive

- **verdict**: CONFIRMED (accurately describes current state — mitigation
  shipped, real fix still open)
- **evidence**: `.claude/skills/e2e-suite/SKILL.md:100-110` already records
  the `--workers=1` mitigation with the exact numbers the entry cites
  (2 failed/141 passed in 1.4 min parallel vs 143/143 in 3.8 min serial,
  measured 2026-07-25) — so the entry's own claim that this is "recorded in
  the e2e-suite skill" is true. `end2end/tests/helpers.ts:15-17` shows
  `hydrated()` is still the single global `html[data-hydrated=true]` stamp
  wait — no per-island/hydration-aware click helper has been added since.
- **size**: M
- **disposition**: KEEP — real fix (hydration-aware click helper) still
  unbuilt.

## P6-065 + P6-075 (+ P6-009g) — `zz-e2e-*` leaks / name collisions

- **verdict**: CONFIRMED
- **evidence**: `end2end/playwright.config.ts` (full file read) has no
  `globalTeardown` key at all — nothing sweeps `zz-e2e-*`.
  `end2end/tests/batch-move.spec.ts:74-76` confirmed: `scratchName` appends a
  `Math.random().toString(36).slice(2,7)` suffix on top of
  `zz-e2e-move-${what}-w${workerIndex}-${seq}`. `quick-add.spec.ts`,
  `command-palette.spec.ts`, `needs.spec.ts`, `removal.spec.ts`, and
  `collection-view.spec.ts` (grepped all `scratchName` definitions) use only
  `zz-e2e-<prefix>-w<worker>-<seq>`, no randomness — so the name-collision risk
  on a leaked collection is real and specific to those five specs.
  `collection-tree-manage.spec.ts` documents the `request`-fixture-torn-down
  leak explicitly at line 19 ("Deleting a parent cascades its subtree...")
  with `finally`-based best-effort cleanup throughout.
- **size**: S
- **disposition**: KEEP, one commit: `globalTeardown` sweeping `zz-e2e-*`.
  P6-009g (the orphaned `zz-e2e-inb-src-w1-9` collection) is a one-off Neon
  data claim, UNVERIFIABLE here (would require querying/deleting on the dev
  branch) — do it in the same pass once the teardown exists, per the triage
  doc.

## P6-070 — `end2end/probe-add-tmp.mjs` stale temp probe

- **verdict**: CONFIRMED
- **evidence**: File still present (`end2end/probe-add-tmp.mjs`, last modified
  Jul 20), still not in `end2end/package.json` `scripts` (11 `probe:*` entries
  present, this isn't one). It hardcodes an absolute local path
  (`/Users/dylan.goings/source/three-rings/end2end/playwright/.auth/user.json`)
  and is a one-off `+ Have` click debug script, not written as a reusable
  probe.
- **size**: S
- **disposition**: DROP (delete). The hardcoded absolute path makes
  "promote to a registered probe" the wrong call — it isn't portable as
  written and would need a rewrite to be worth keeping.

## P6-076 — nine Android probes are unregistered folklore

- **verdict**: PARTLY (progress made since filing; enumeration is stale)
- **evidence**: `end2end/package.json` now registers **11** `probe:android-*`
  scripts (collection, quick-add, selection-tray, rail, needs, palette,
  tree-move, my-root, header-kebab, states, tap-targets) — `android-rail-check`,
  one of the entry's original nine, is now registered. `end2end/` contains 21
  `android-*-check.mjs` files total (directory listing), so **10** remain
  unregistered: `android-cdp-check`, `android-stepper-check`,
  `android-tree-check`, `android-all-cards-check`, `android-card-detail-check`,
  `android-dfc-check`, `android-quick-actions-check`,
  `android-tree-manage-check` (8 from the original list, still true), plus
  **two new ones the entry doesn't mention**: `android-catalog-paging-check.mjs`
  and `android-owned-badge-check.mjs` (both created after filing).
- **size**: S
- **disposition**: RESCOPE — update the entry's probe enumeration to the
  current 10 before executing the "register the lot" commit.

## P6-077 — `tr_jwt` cookie expiry reads like a page bug

- **verdict**: CONFIRMED
- **evidence**: `.claude/skills/e2e-suite/SKILL.md:77` mentions the
  `tr_session`/`tr_jwt` cookie *names* (storageState capture) but nothing
  about the ~20-minute expiry, the
  `Couldn't load this collection: unauthorized: invalid token` symptom, or the
  `npx playwright test --project=setup` refresh fix. Not recorded anywhere
  else searched (`ui-work-loop.md`).
- **size**: S
- **disposition**: MERGE → same commit as P6-004 + P6-040b.

## P6-078 — collection-view test-strength leftovers (review round 2)

### (a) inert `here_delta` `is_some()` guard, comment factually wrong

- **verdict**: CONFIRMED
- **evidence**: `app/src/my/collection.rs:205-221`, unchanged since its
  introduction in commit `7ece00f`. The comment's premise ("`is_some()` is the
  load-bearing half: a resource in flight reads `None`, and that is exactly
  the window where the old totals... are still what's on screen") is the exact
  claim the entry disputes as inconsistent with Leptos 0.8 resource/Transition
  semantics — neither the comment nor a test guarding the guard has been
  added/changed.
- **size**: S
- **disposition**: KEEP (correct the comment; either add a kill test for the
  guard or drop it, per the entry).

### (b) color-identity badge asserted against its own producing helper

- **verdict**: CONFIRMED
- **evidence**: `app/src/backend/hosted.rs:2434-2458`, `commanders_in` (the
  function that builds the deck-view color-identity badge) calls
  `union_color_identity` — the same function in `shared/src/tags.rs:130-146`
  the entry says the e2e test has no independent authority over. Unchanged;
  `cards_with_tag` (`hosted.rs:1707-1733`) does return per-card
  `color_identity`, confirmed present, and the entry's fix (assert card-level
  union in the e2e rather than trusting the helper) isn't applied.
- **size**: S
- **disposition**: KEEP.

### (c) "three columns agree" 1-in-3 firefox flake from shared Trade Binder printing

- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/collection-view.spec.ts:920-921`, `somePrinting()`
  reads from the `"Trade Binder"` collection. All three write tests use it:
  the stepper test (`:927`), the floor test (`:1005`, via `somePrinting` at
  line ~1019), and the teardown test ("emptying a deck moves its cards to the
  chosen destination", `:1050`, via `somePrinting` at line ~1054). The read
  test "the three columns agree with the collection read" (`:239`) also reads
  `"Trade Binder"` (among `["Trade Binder", "Depth Box"]`, line 251). The
  shared-printing mechanism the entry names for the flake is structurally
  present and unfixed; the flake itself wasn't re-run to confirm (out of
  scope — no test runs).
- **size**: S
- **disposition**: KEEP (point the three write tests at a Depth-Box printing
  instead of `somePrinting()`/Trade Binder).

## P6-079 — stepper zero-floor e2e covers only `−` click and typed-0-⏎

- **verdict**: CONFIRMED
- **evidence**: `end2end/tests/collection-view.spec.ts:1005-1046` ("the
  stepper's last copy is removable, not floored") only exercises
  `dec.click()`. `end2end/tests/removal.spec.ts:173-174` has the
  typed-0-then-Enter path (`STEPPER_INPUT.fill("0")` +
  `.press("Enter")`) as a shared helper used across its tests. Grepped both
  files (and `count-stepper.spec.ts`, the generic bench-component spec) for
  `ArrowDown`, `paste`, and negative/non-numeric input handling in the
  collection-scoped stepper context — none found. (`count-stepper.spec.ts` is
  a different, generic bench-page spec with its own keyboard-clamp coverage at
  `:120-130`, not the collection-view zero-floor path this entry is about.)
- **size**: S
- **disposition**: KEEP.
