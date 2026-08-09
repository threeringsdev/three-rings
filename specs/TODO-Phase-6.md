# Project TODO - Phase 6 — historical record

**The Phase 6 execution queue moved to Workbook on 2026-08-06.** Select work
with `workbook next`; the lifecycle and canonical machine values are in
[.workbook/guidelines.md](../.workbook/guidelines.md), and the selection process
is in [README.md](README.md) ("Working the queue"). **This file is no longer a
queue.** It remains the permanent record of the 2026-07-30 verification pass —
the Done and Dropped ledgers, the provenance map, and the
unsettled-measurements list below — because those tables are what stops a
dropped entry being rediscovered and refiled. The probe evidence stays in
[phase-6-probes/](phase-6-probes/).

**How the queue was migrated.** Every active task became one Workbook task
titled `P6-NNN: …`, with the full original entry as its description (evidence,
file:line references, acceptance criteria, spec links). The mapping:

- **Order.** The doc's execution order (stages top→bottom, tasks within a
  stage top→bottom) is preserved in Workbook's rank order, so `workbook next`
  walks the same sequence this file did.
- **Stages → labels** `stage-1` … `stage-10`, plus `phase-6` on every task.
- **Priorities.** Stages 1–3 `high`, stages 4–6 `medium`, stages 7–10 `low` —
  the same rationale as the old stage ordering (destructive actions and ship
  gates first, hygiene last).
- **Statuses.** Active tasks are `ready`. The eight *decision first* tasks
  (`P6-054`, `P6-150`, and the six Stage 6 decisions) are `blocked` with label
  `decision-first` — blocked on a maintainer ruling, not on code, exactly as
  this file said. The eleven parked tasks are `backlog` with label `parked`,
  each carrying its unpark trigger in its description.
- **Hard prerequisites → Workbook dependencies:** the deletion chain
  `P6-039 → P6-110 → P6-188 → P6-189`/`P6-190`; `P6-124 → P6-108`, which also
  gates parked `P6-035` and `P6-104`; `P6-113 → P6-056`; `P6-083 → P6-043`.
- **Sizes** (**S** ≤ half a day · **M** a day or two · **L** a week, or a
  design decision first) are recorded in each description's header line.

**Task ids.** `P6-` ids are permanent and live on in the Workbook task titles.
The series is frozen with the migration — new work gets a Workbook id directly,
so `P6-191` and beyond will never be allocated, and a dropped id stays retired
(ledger below).

---

## Superseded original problem statements, kept for the record

<details>
<summary>Original <code>P6-110</code> problem statement</summary>

**Deleting a collection destroys every card in it and in every collection nested under it, permanently, with no recovery path of any kind** — and the ledger does not merely fail to record it, it records something false. Verified 2026-07-29 (`specs/phase-6-probes/P6-017a-cascade.md`). `DELETE FROM collections WHERE id = $1 AND NOT is_inbox` (`app/src/backend/hosted.rs:510`) is the **entire** delete path; every other effect is the database's referential action. `collections.parent_id` cascades to descendants, and each descendant's `holdings` / `desires` / `card_tags` / deck-scoped `tags` cascade away with it. There is no soft delete, no `deleted_at`, no trash, no archive, no audit table, no undo entry, and nothing re-parents the rows — not to the parent, not to the Inbox. A card held **only** inside the deleted subtree disappears from the user's ownership entirely (`owned_by_card` is a pure aggregate over `holdings`). **The `moves` ledger survives falsified**: its two collection columns are `ON DELETE SET NULL`, and NULL in that schema does not mean "unknown", it means "external intake" (`from`) and "removal" (`to`) — so historical moves in and out of the deleted subtree silently rewrite themselves into intakes and removals, and undoing one will not put the copies back (`undo_one` skips a NULL end, `hosted.rs:2005-2010`). No move row records the deletion itself. **Decide the policy before writing code**: soft-delete + a trash view, re-parent the contents to the parent or Inbox, an explicit "and destroy N cards" confirm, or accept it and fix only the confirm (`P6-111`). Whatever is chosen must also decide what the ledger should say about a deleted collection. **Resolved 2026-08-04** — see [collection-deletion.md](collection-deletion.md).

</details>

<details>
<summary>Original <code>P6-111</code>, absorbed by <code>P6-189</code></summary>

**The delete confirm undercounts what it is about to destroy, in two independent ways** (was `P6-017a`; verified CONFIRMED, with the entry's wording corrected). (1) When `find_node` misses — a failed, anonymous, or still-in-flight tree read, all of which produce a plain `Vec::new()` via `assembled_roots` (`app/src/my/collection.rs:378-385`) — `for_collection` degrades `forbidden` to `{self}` (`app/src/my/tree_manage.rs:86-93`), `DeleteReq::descendants()` reads 0, and the `descendants > 0` guard at `tree_manage.rs:742` means the confirm **silently omits the nested-collections clause entirely** rather than printing "0". The original entry said it "says 0 nested collections"; it does not, which makes the omission harder to spot, not easier. The card count stays honest in that state (it comes from `present_total()`, already rolled up over the subtree), so the dialog is confidently right about one number and silent about the other. (2) In **every** state the confirm counts holdings only and **never counts the desires** that die with them. Both are fixed by `P6-189`, which also corrects the count in the *opposite* direction from how this was filed: children now survive, so the number must shrink to this collection's own `present`.

</details>

---

## Done

- [x] `P6-073` **A card cannot be removed from a binder at all** — **fixed** by the removal/teardown task (2026-07-25): grain-addressed `move_holding` resolves grain and board off the holding row `FOR UPDATE` inside the write transaction, the `min=1` floor is gone, and the removal is undoable at the original grain and board. Mutation-verified. The one remaining case is a multi-grain cell, now `P6-057`.
- [x] `P6-080` **The UI work loop costs too much per task and has no stopping rule** — **done** (2026-07-25, maintainer decision): chromium only; one e2e run per task after the review fixes; one review round, code-only, blockers/majors only; no mutation passes; minors filed as Phase 5 discoveries, never fixed. Landed in `specs/ui-work-loop.md` and the `ui-task-loop`, `adversarial-review` and `e2e-suite` skills. Recorded in `DECISION-LOG.md`.
- [x] `P6-102` **Done 2026-07-28 (config only, no code).** Google sign-in on the local web dev server dead-ended on the native "Signed in — you can close this tab" page instead of the 303 to `/`. Cause: `TR_EMBEDDED_ORIGIN` lived in the workspace `.env`, which `server` loads via `dotenvy`, so the web dev server also took the embedded-server branch in `/auth/callback`. Both clauses of the original premise were wrong — same-tab navigation on web is the intended OAuth pattern, and the "close this tab" page is native/Android-only; deployed Render and release desktop were never affected. Fix: moved the var into `beforeDevCommand` in `src-tauri/tauri.conf.json`. Findings in specs/auth.md.

---

## Dropped during verification — do not refile

Recorded permanently so a dropped entry is not rediscovered and filed again. Full reasoning in the probe reports.

| Id | Reason |
|---|---|
| `P6-003` | Not a task. The `selection_destinations` raw-array constraint and all four of its supporting facts are **already** written down in `app-ui.md` (`:270-280`, `:304`, `:310-311`, `:490`) and in a 14-line comment at `move_selection.rs:546-559`. Nothing left to move. The reopen trigger (if the tray ever SSRs) sits beside the evidence at `app-ui.md:490`. |
| `P6-014` | Duplicate of `P6-002` — the same defect filed by two review rounds. `all_cards.rs:110-119` states both in one doc comment. Folded in, including the `diag:resource-ids` pointer. |
| `P6-045` | **STALE** — fixed in `7649d80a` (2026-07-27). `SelectionCheckbox`'s span is `size-11 md:size-4`; the mobile tap target is already 44 px. |
| `P6-046d` / `P6-061j` | **STALE** — fixed in `7649d80a`. `toaster_offset()` repositions the `Toaster` when the tray is up (`shell.rs:330-337`). Same fix, filed twice. |
| `P6-046e` | **STALE** — fixed in `7649d80a`. The dock carries `md:left-60`, with a comment naming this exact bug. |
| `P6-055g` | **STALE** — the teardown dialog's "every move is in the history" is now **true** (`hosted.rs:1245-1253` appends one move per board, `:1256` returns the ids). The residue is the missing toast button, which is `P6-031`. |
| `P6-061g` | **STALE** — the cited vacuous assertion was rewritten by the state-arms task (`selection-tray.spec.ts:116-128`); the other `style:display` case correctly uses `toBeVisible()`. |
| `P6-061i` | **STALE** — `SkipReason` has no `Board` variant any more (`move_selection.rs:130-153`); `app-ui.md:1358-1360` records it as "gone entirely". |
| `P6-064` | **STALE** — `search` now calls `owned_by_oracle` over the page ids and maps `owned_of` into `into_summary(n)` (`hosted.rs:313-321`); the cited `into_summary(None)` is gone. `owned` is `None` only for anonymous callers, which is correct. The live residue is `P6-135`. |
| `P6-066` | Self-declared superseded by `P6-060`, which already carries the full record of how the wrong diagnosis was reached from one green run. Nothing to preserve elsewhere. |
| `P6-009a` | **STALE** — `responsive.spec.ts:409-443` no longer slices; `smallHolders` iterates all candidates. |
| `P6-009b` | **STALE / WRONG** — both the `px-1` savings and the 44 px select column now switch at `md` (`collection.rs:1191-1199`, `all_cards.rs:348-356`), so the 640–767 px band the entry describes does not exist. |
| `P6-009f` | **STALE** — the comment at `responsive.spec.ts:403-408` is already correct; the inverted reasoning was fixed. |
| `P6-009g` | Merged into `P6-065` (the orphaned `zz-e2e-inb-src-w1-9` collection is swept by the same `globalTeardown`). |
| `P6-012f` | **STALE** — the `err.locator("..")` parent-hop pattern is not present anywhere in `states.spec.ts`. |
| `P6-021f` | Historical note only — the shape it warns about was fixed during its own task, and `my-root.spec.ts:328` already carries the guard. |
| `P6-030g` | **STALE** — `TreeDialogs` is shell-mounted now; the sidebar is no longer `hidden md:block` around it, so tree dialogs are not invisible below `md`. |
| `P6-038e` | Merged into `TODO.md`'s existing large-collection aggregate-performance item — the `holdings` Seq Scan is irrelevant at 101 rows and belongs with real-scale profiling. Needs `EXPLAIN ANALYZE` at scale, which that item already schedules. |
| `P6-069k` | Accepted as-is. `Resource::get()` keeps the previous payload while refetching, so for one `collection_view` round trip after switching collections the panel's destination/kind/present describe the previous collection — cosmetic, and the header and body are equally stale, so nothing contradicts anything. |
| `P6-091` | Dropped 2026-07-28 on maintainer observation while working `P6-102`. Unstyled raw-HTML auth pages were not reproduced during live local + deployed testing. **This id's one sanctioned exception to the never-refile rule**: if seen again, refile citing the ledger row. Caveat: `/auth/app-return` (Android deep-link bounce) was never exercised. |
| `P6-092` | CONFIRMED real, dropped anyway (maintainer call, 2026-07-28): we are not gating CI on release builds. Probe also falsified the "no GUI needed" clause and the claim that one `cargo build -p three_rings` covers both this and `P6-089`. Retired permanently. |
| `P6-105` | Split 2026-07-28 into `P6-108` (the bulk run) and `P6-109` (stage 3, parked in `TODO.md`). Id retired; probe report stays at `phase-6-probes/P6-105.md`. |
| `P6-111` | **Absorbed by `P6-189`** (2026-08-04). Both halves of the undercount — the silently-omitted nested-collections clause and the never-counted desires — are fixed by the rewritten confirm dialog, which also corrects the card count in the *opposite* direction from how this was filed: children now survive a delete, so the number shrinks to this collection's own `present`. Original text kept collapsed above. |
| `P6-153` | Allocated during the 2026-07-30 rewrite and released before publication — the task it named kept its original id `P6-047`. Never published; retired, do not reuse. |

**The id series is frozen at `P6-190`** — `P6-191` and up will never be allocated; new work is filed directly in Workbook.

---

## Provenance — which probe report backs which task

Split products are **not** re-verified; each inherits the probe that produced it. Use this to find the evidence behind a task whose id did not exist before 2026-07-30. Sub-item letters in parentheses are the parts of the original bundle.

| Original | Became | Report |
|---|---|---|
| `P6-009` (a–g) | `P6-158` (d) · `P6-169` (e) · `P6-176` (c) — a, b, f dropped; g → `P6-065` | `batch-I-responsive.md` |
| `P6-012` (a–g) | `P6-142` (c) · `P6-157` (a) · `P6-177` (d, e) · `P6-178` (b) · `P6-179` (g) — f dropped | `batch-I-responsive.md` |
| `P6-017` (a–h) | `P6-110` + `P6-111` (a) · `P6-126` (b) · `P6-127` (c) · `P6-129` (d) · `P6-155` (e) · `P6-128` (f) · `P6-174` (g, h) | `P6-017.md`, `P6-017a-cascade.md`, `P6-017d-confirm-copy.md` |
| `P6-021` (a–f) | `P6-154` (c) · `P6-160` (d, e) · `P6-165` (a) · `P6-166` (b) — f dropped | `batch-I-responsive.md` |
| `P6-024` (a–j) | `P6-121` (a, b) · `P6-155` (h) · `P6-156` (c, d) · `P6-163` (g) · `P6-174` (e) · `P6-176` (i) · `P6-184` (j) · `P6-185` (f) | `P6-024.md` |
| `P6-028` (a–b) | `P6-125` (a) · `P6-149` (b) | `batch-G-palette.md` |
| `P6-029` | `P6-029` (scroll-into-view) · `P6-172` (the O(n²) half) | `batch-F-command.md` |
| `P6-030` (a–h) | `P6-144` (c) · `P6-145` (e) · `P6-146` (a, b) · `P6-176` (h) · `P6-180` (d, f) — g dropped | `batch-G-palette.md` |
| `P6-036` (a–i) | `P6-136` (b, c) · `P6-137` (a) · `P6-138` (e) · `P6-139` (f) · `P6-168` (d) · `P6-175` (g, h, i) | `batch-F-command.md` |
| `P6-038` (a–g) | `P6-135` (a) · `P6-175` (b, c, d) · `P6-181` (f, g) — e → `TODO.md` | `batch-E-catalog.md` |
| `P6-042` (a–f) | `P6-130` (b) · `P6-131` (e) · `P6-132` (a) · `P6-133` (c, d) · `P6-175` (f) | `batch-E-catalog.md` |
| `P6-046` (a–h) | `P6-122` (g) · `P6-150` (a, b, c) · `P6-161` (f) — d, e dropped; h → `P6-065` | `batch-B-tray.md` |
| `P6-049` (a–j) | `P6-119` (b) · `P6-120` (f) · `P6-140` (a) · `P6-141` (c, d) · `P6-143` (i) · `P6-182` (e, j) · `P6-183` (g, h) | `batch-C-needs.md` |
| `P6-055` (a–l) | `P6-112` (f) · `P6-113` (h) · `P6-114` (i) · `P6-115` (j) · `P6-116` (c) · `P6-117` (a, b) · `P6-118` (d, e) · `P6-176` (l) · `P6-177` (k) — g → `P6-031` | `batch-D-moves.md` |
| `P6-061` (a–l) | `P6-123` (a, b, h) · `P6-152` (e, f) · `P6-167` (c, k) · `P6-186` (d, l) — g, i, j dropped | `batch-B-tray.md` |
| `P6-069` (a–k) | `P6-147` (a) · `P6-148` (b, c, f, g) · `P6-159` (h) · `P6-180` (e, i) · `P6-187` (j) — k dropped | `batch-G-palette.md` |
| `P6-072` (a–f) | `P6-124` (f) · `P6-162` (e) · `P6-164` (c, d) · `P6-173` (a, b) | `batch-J-cards.md` |
| `P6-105` | `P6-108` · `P6-109` | `P6-105.md` |

Merged whole rather than split: `P6-014`→`P6-002` · `P6-041`+`P6-096`→`P6-134` · `P6-103`→`P6-005` · `P6-052`+`P6-075`+`P6-009g`→`P6-065` · `P6-004`+`P6-040b`+`P6-077`→`P6-170` · `P6-040a`→`P6-171` · `P6-062`→`P6-150` · `P6-063`→`P6-151`. Kept whole with their original ids and reports: everything else — `batch-A-tree.md` (`P6-015`, `P6-023`, `P6-026`, `P6-093`), `batch-H-leptos.md` (`P6-002`, `P6-013`, `P6-022`, `P6-083`), `batch-K-e2e.md`, `batch-L-ci-docs.md`, and the per-id reports `P6-010.md`, `P6-059.md`, `P6-068.md`.

---

## What the pass did not settle

Named so nobody assumes it was checked. **Each of these needs a real measurement before the task it belongs to is sized or worked.** The same cautions are carried inside the corresponding Workbook task descriptions.

- **Every pixel measurement in `P6-001`** — the figures come from the original responsive audit and the code has moved since. The check: load the tables at 320/375/768 and compare `scrollWidth` to `clientWidth`.
- **`P6-002`'s latency claim** — "not firing at today's id layout" and "~50 slots of headroom" are layout-dependent and were not re-measured. `npm run diag:resource-ids` is the tool.
- **All database state** — the pass never connected to Neon. So the "88 desires" figure in `P6-065`, the orphaned `zz-e2e-inb-src-w1-9` collection, and the Seq Scan folded into `TODO.md`'s large-collection item are all unconfirmed.
- **Browser behavior** — `P6-185` (which engines fire `contextmenu` from keyup) and all three of `P6-025`'s claims are confirmed as code shapes and unconfirmed at runtime.
- **`P6-097`** — no profile was taken. The three candidate causes are read from the code, not measured, and the profile will change materially after `P6-108`.
- **`P6-187`** — whether the co-registration window that would make `Candidate.index` and `nav.highlighted()` disagree actually exists.
