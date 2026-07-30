# Phase 6 verification ledger

The record of the verification pass over [TODO-Phase-6.md](TODO-Phase-6.md).
**The pass is complete.** Every entry that stood on 2026-07-28 has been checked
against the code as it stands today, disposed of, and folded into the rewritten
queue. This file is now the permanent record of *what was decided and why* —
including, importantly, the ids that were **dropped**, so a dropped entry is not
rediscovered and refiled.

## How it ran

Two passes, both read-only:

1. **2026-07-28, one at a time** — `P6-010`, `P6-059`, `P6-068`, `P6-091`,
   `P6-092`, `P6-102`, `P6-105`, plus probes for `P6-017` and `P6-024`. One
   subagent per id, maintainer in the loop for each disposition. Reports at
   `phase-6-probes/P6-0xx.md`.
2. **2026-07-30, twelve parallel batches** — the remaining 96 entries, batched by
   code area (A–L), one subagent per batch, dispositions applied by the
   orchestrator under a standing instruction to use its own judgement. Reports at
   `phase-6-probes/batch-<letter>-<topic>.md`. Two follow-up probes from the first
   pass also landed in this window: `P6-017a-cascade.md` and
   `P6-017d-confirm-copy.md`.

**Nothing was fixed during either pass.** Every disposition is a queue edit.

## Outcome

- **98 entries verified**, ~190 individual sub-claims.
- **19 sub-claims came back STALE or WRONG** and were dropped — see the drop table
  in [TODO-Phase-6.md](TODO-Phase-6.md), which is the authoritative never-refile
  record.
- **The bundles were split.** The 19 multi-class "minors from its review round"
  entries became standalone tasks; the map is below.
- **The queue was re-ordered** from severity classes into execution stages.
- **137 active tasks** across ten stages, plus **11 parked** (each with a trigger),
  3 done, and 22 rows in the never-refile drop table.

**Ids.** Original ids `P6-001`…`P6-109` are all accounted for below. Split
products took `P6-110`…`P6-187`. **Next free id: `P6-188`.** `P6-153` was
allocated during the rewrite and released before publication (the task it named
kept its original id `P6-047`) — treat it as retired and do not reuse it.

Split products are **not** re-verified: each inherits the probe that produced it,
named in the map below.

## Bundle → task map

Where a bundled entry became several standalone tasks. Cite the source report
when working any of these.

| Was | Now | Probe report |
|---|---|---|
| `P6-009` (a–g) | `P6-158` (d), `P6-169` (e), `P6-176` (c) · **dropped:** a, b, f · **merged:** g → `P6-065` | `batch-I-responsive.md` |
| `P6-012` (a–g) | `P6-142` (c), `P6-157` (a), `P6-177` (d, e), `P6-178` (b), `P6-179` (g) · **dropped:** f | `batch-I-responsive.md` |
| `P6-017` (a–h) | `P6-110` + `P6-111` (a), `P6-126` (b), `P6-127` (c), `P6-129` (d), `P6-155` (e, with `P6-024h`), `P6-128` (f), `P6-174` (g, h) | `P6-017.md`, `P6-017a-cascade.md`, `P6-017d-confirm-copy.md` |
| `P6-021` (a–f) | `P6-154` (c), `P6-160` (d, e), `P6-165` (a), `P6-166` (b) · **dropped:** f | `batch-I-responsive.md` |
| `P6-024` (a–j) | `P6-121` (a, b), `P6-155` (h, with `P6-017e`), `P6-156` (c, d), `P6-163` (g), `P6-174` (e), `P6-176` (i), `P6-184` (j), `P6-185` (f) | `P6-024.md` |
| `P6-030` (a–h) | `P6-144` (c), `P6-145` (e), `P6-146` (a, b), `P6-176` (h), `P6-180` (d, f) · **dropped:** g | `batch-G-palette.md` |
| `P6-036` (a–i) | `P6-136` (b, c), `P6-137` (a), `P6-138` (e), `P6-139` (f), `P6-168` (d), `P6-175` (g, h, i) | `batch-F-command.md` |
| `P6-038` (a–g) | `P6-135` (a), `P6-175` (b, c, d), `P6-181` (f, g) · **merged:** e → `TODO.md` large-collection item | `batch-E-catalog.md` |
| `P6-042` (a–f) | `P6-130` (b), `P6-131` (e), `P6-132` (a), `P6-133` (c, d), `P6-175` (f) | `batch-E-catalog.md` |
| `P6-046` (a–h) | `P6-122` (g), `P6-150` (a, b, c), `P6-161` (f) · **dropped:** d, e · h folded into `P6-065` | `batch-B-tray.md` |
| `P6-049` (a–j) | `P6-119` (b), `P6-120` (f), `P6-140` (a), `P6-141` (c, d), `P6-143` (i), `P6-182` (e, j), `P6-183` (g, h) | `batch-C-needs.md` |
| `P6-055` (a–l) | `P6-112` (f), `P6-113` (h), `P6-114` (i), `P6-115` (j), `P6-116` (c), `P6-117` (a, b), `P6-118` (d, e), `P6-176` (l), `P6-177` (k) · **dropped:** g (→ `P6-031`) | `batch-D-moves.md` |
| `P6-061` (a–l) | `P6-123` (a, b, h), `P6-152` (e, f), `P6-167` (c, k), `P6-186` (d, l) · **dropped:** g, i, j | `batch-B-tray.md` |
| `P6-069` (a–k) | `P6-147` (a), `P6-148` (b, c, f, g), `P6-159` (h), `P6-180` (e, i), `P6-187` (j) · **dropped:** k | `batch-G-palette.md` |
| `P6-072` (a–f) | `P6-124` (f), `P6-162` (e), `P6-164` (c, d), `P6-173` (a, b) | `batch-J-cards.md` |
| `P6-078` (a–c) | kept whole as `P6-078` | `batch-K-e2e.md` |
| `P6-028` (a–b) | `P6-125` (a), `P6-149` (b) | `batch-G-palette.md` |
| `P6-029` | `P6-029` (scroll-into-view), `P6-172` (the O(n²) half) | `batch-F-command.md` |
| `P6-105` | `P6-108` (a, e-bulk), `P6-109` (b, c, d, f — parked in `TODO.md`) | `P6-105.md` |

Entries merged wholesale rather than split: `P6-014` → `P6-002`; `P6-041` +
`P6-096` → `P6-134`; `P6-103` → `P6-005`; `P6-052` + `P6-075` + `P6-009g` →
`P6-065`; `P6-004` + `P6-040b` + `P6-077` → `P6-170`; `P6-040a` → `P6-171`;
`P6-062` → `P6-150`; `P6-063` → `P6-151`.

## Per-id dispositions

Every id that stood on 2026-07-28. `KEEP` means the entry survived intact
(possibly with refreshed line references); `RESCOPE` means the text was rewritten
because verification found the claim mis-stated.

| Id | Verdict | Disposition |
|---|---|---|
| `P6-001` | UNVERIFIABLE | `KEEP` — mechanism consistent (`hidden md:table-cell`), pixel figures need re-measuring at 320/375/768 before the fix. Stage 7. |
| `P6-002` | CONFIRMED | `KEEP` + absorbs `P6-014`. Two live same-type pairs found. The "not firing at today's id layout" claim is unverified — run `npm run diag:resource-ids` first. |
| `P6-003` | CONFIRMED as a constraint | **`DROP`** — its own action (move the facts to `app-ui.md` Findings) is already done; all four facts are in `app-ui.md` and in a 14-line source comment. |
| `P6-004` | CONFIRMED | `MERGE` → `P6-170` (three e2e traps, one commit). |
| `P6-005` | CONFIRMED | `KEEP` + absorbs `P6-103`. Top of the capability class. |
| `P6-006` | CONFIRMED | `KEEP` — Stage 6, needs a design ruling. |
| `P6-007` | CONFIRMED | `KEEP` — Stage 6, needs a design ruling. |
| `P6-008` | CONFIRMED | `KEEP` — no ruling needed, straightforward copy fix. |
| `P6-009` | PARTLY (3 of 7 stale) | `SPLIT` — see map. a, b, f dropped. |
| `P6-010` | CONFIRMED | `RESCOPE` + `RECLASS` (2026-07-28) — hosted-only, self-heals on one reload. Named S fix at `lib.rs:834-842`. |
| `P6-011` | CONFIRMED | `KEEP` — Stage 3 (primitive-level). `P6-163` and `P6-152` are its visible instances. |
| `P6-012` | PARTLY (1 of 7 stale) | `SPLIT` — see map. f dropped. |
| `P6-013` | CONFIRMED, **entry's own claims corrected** | `KEEP` + `PROMOTE` — never actually filed upstream; version cites wrong (leptos 0.8.14 / hydration_context 0.3.0 per `Cargo.lock`); `leptos_server-0.8.7` is byte-identical so a bump is not a fix. |
| `P6-014` | CONFIRMED | **`MERGE`** → `P6-002`. Id retired. |
| `P6-015` | CONFIRMED | `KEEP` — Stage 6, needs a ruling (doc vs. server guard). |
| `P6-016` | CONFIRMED | `KEEP` — per-row kebab still unbuilt. |
| `P6-017` | CONFIRMED (8 of 8; (a) wording wrong) | `SPLIT` — and the (a) follow-up probe surfaced `P6-110`, the most severe item in the file. |
| `P6-018` | CONFIRMED | `KEEP` — both route maps still omit `/my/all`. |
| `P6-019` | CONFIRMED | `KEEP` — no icon crate in `Cargo.toml`. |
| `P6-020` | CONFIRMED | `KEEP` — no `whitespace-nowrap` anywhere. |
| `P6-021` | PARTLY (1 of 6 stale) | `SPLIT` — see map. f dropped. |
| `P6-022` | CONFIRMED, **understated** | `RESCOPE` — five primitives, not one; root cause found upstream in `tw_merge`'s `get_collision_id.rs:617-627`. S → M. |
| `P6-023` | CONFIRMED | **`PARK`** — trigger: a design decision defines the non-drag reorder affordance. |
| `P6-024` | CONFIRMED (8 of 10; d, g PARTLY) | `SPLIT` — see map. |
| `P6-025` | CONFIRMED (line refs rotted) | `KEEP` + `RESCOPE` — refreshed all three line references; the runtime check still has not been done. |
| `P6-026` | CONFIRMED | `KEEP` — three independent a11y gaps, still all present. |
| `P6-027` | CONFIRMED | **`PARK`** — trigger: before parallel workers are re-enabled. Recorded nowhere in `ui-work-loop.md` yet. |
| `P6-028` | CONFIRMED | `SPLIT` → `P6-125` (a, the `dialog.rs` focus trap — reaches all 4 Dialog consumers, not Sheet/Popover) + `P6-149` (b). |
| `P6-029` | CONFIRMED (both halves) | `SPLIT` → `P6-029` (scroll-into-view) + `P6-172` (the O(n²)). |
| `P6-030` | PARTLY (1 of 8 stale) | `SPLIT` — see map. g dropped; f is stronger than filed. |
| `P6-031` | CONFIRMED | `KEEP` + absorbs `P6-055g`. |
| `P6-032` | CONFIRMED, **drift recurred** | `KEEP` + priority bump — the trees are out of sync again two days after the manual resync, which retires the "just resync it" argument. |
| `P6-033` | CONFIRMED | `KEEP`. |
| `P6-034` | CONFIRMED | `KEEP` — primitive-level. |
| `P6-035` | CONFIRMED | **`PARK`** — trigger: `P6-108` lands, then re-triage. |
| `P6-036` | PARTLY (a's mechanism wrong) | `SPLIT` — see map. (a) rescoped: the SQL *does* bind the limit; `lib.rs` always passes `None`. |
| `P6-037` | PARTLY | `KEEP` + `RESCOPE` — the fixture-distinguishability guard already landed; the positive-control and shared-provenance guards did not, and the live instance still stands. |
| `P6-038` | CONFIRMED (e unverifiable) | `SPLIT` — see map. e merged into `TODO.md`. |
| `P6-039` | CONFIRMED, **worse** | `RESCOPE` — four definitions of "owned", not three. |
| `P6-040` | CONFIRMED | `SPLIT` → `P6-171` (a, `vendor-component`) + `P6-170` (b, `e2e-suite`). |
| `P6-041` | CONFIRMED | **`MERGE`** → `P6-134`, with `P6-096`. |
| `P6-042` | CONFIRMED | `SPLIT` — see map. |
| `P6-043` | CONFIRMED | `KEEP` — cheaper after `P6-083`. |
| `P6-044` | CONFIRMED | `KEEP`. |
| `P6-045` | **STALE** | **`DROP`** — fixed in `7649d80a` (2026-07-27); the span is `size-11 md:size-4`. |
| `P6-046` | PARTLY (2 of 8 stale) | `SPLIT` — see map. d, e dropped. |
| `P6-047` | CONFIRMED | `KEEP` — pairs with `P6-151` as the place to answer the grain question. |
| `P6-048` | CONFIRMED | `KEEP`. |
| `P6-049` | CONFIRMED (10 of 10) | `SPLIT` — see map. Nothing stale. |
| `P6-050` | CONFIRMED | `KEEP`. |
| `P6-051` | CONFIRMED | `KEEP`. |
| `P6-052` | PARTLY | `MERGE` → `P6-065`. The mechanism is confirmed (`destination-picker.spec.ts` grows real desires with no cleanup); the "88 desires" count is unverified. |
| `P6-053` | CONFIRMED | `KEEP`. |
| `P6-054` | CONFIRMED | `KEEP` — still unreachable from the UI; spec-owner call. |
| `P6-055` | CONFIRMED (11 of 12; g stale) | `SPLIT` — see map. Largest single source of Stage 1. |
| `P6-056` | CONFIRMED | `RESCOPE` — struck the now-false "nothing undoes a teardown" tail; land after `P6-113`. |
| `P6-057` | CONFIRMED, narrowed | `RESCOPE` — the move case is already covered by the tray; only remove and per-grain edit are missing. |
| `P6-058` | PARTLY | `RESCOPE` — the undo-adapter half is **wrong** (`undo_selection_move` calls `undo_moves`, and `lib.rs:643-648` documents why each exists) and is dropped; the e2e-helper half is confirmed and worse (`createCollection` redefined in 9 files). |
| `P6-059` | CONFIRMED | `RESCOPE` (2026-07-28) — stays a ship gate. Both fixes required. |
| `P6-060` | CONFIRMED | `KEEP` — no per-island helper has been built. |
| `P6-061` | PARTLY (3 of 12 stale) | `SPLIT` — see map. g, i, j dropped. |
| `P6-062` | CONFIRMED | **`MERGE`** → `P6-150` (cannot be decided apart from the tray's grain semantics). |
| `P6-063` | PARTLY | **`MERGE`** → `P6-151`, rescoped — the "until removal/teardown widens the write path" framing is obsolete. |
| `P6-064` | **STALE** | **`DROP`** — `search` now fills `owned`; the cited `into_summary(None)` is gone. Live residue is `P6-135`. |
| `P6-065` | CONFIRMED | `KEEP` + absorbs `P6-075`, `P6-052`, `P6-009g`. |
| `P6-066` | CONFIRMED (delete is correct) | **`DROP`** — superseded by `P6-060`, which carries the full record. |
| `P6-067` | CONFIRMED (decision unmade) | `KEEP` + `PROMOTE` — Stage 6; evidence complete, just needs the call. |
| `P6-068` | CONFIRMED | `RESCOPE` + `RECLASS` (2026-07-28) — diagnosed; Option B chosen. |
| `P6-069` | CONFIRMED (k accepted as-is) | `SPLIT` — see map. |
| `P6-070` | CONFIRMED | `KEEP` — the task is "delete the file"; it is not portable enough to promote. |
| `P6-071` | PARTLY | `RESCOPE` — the growth trigger **has already fired**: `AddToast` is 9 fields, not 8. Do it now. |
| `P6-072` | CONFIRMED (6 of 6) | `SPLIT` — see map. (f) promoted to a `P6-108` prerequisite. |
| `P6-073` | — | Already `[x]`; moved to the Done section. |
| `P6-074` | CONFIRMED | `KEEP` — `hosted.rs:919` already comments on the discrepancy. |
| `P6-075` | CONFIRMED | `MERGE` → `P6-065`. |
| `P6-076` | PARTLY | `RESCOPE` — 11 of 21 probes are now registered and two unregistered ones postdate the entry; re-enumerate before registering. |
| `P6-077` | CONFIRMED | `MERGE` → `P6-170`. |
| `P6-078` | CONFIRMED (3 of 3) | `KEEP` whole. |
| `P6-079` | CONFIRMED | `KEEP`. |
| `P6-080` | — | Already `[x]`; moved to the Done section. |
| `P6-081` | CONFIRMED | **`PARK`** — trigger: a pre-release desktop/WKWebView claim, or a decided cadence. Note the stale comment in `playwright.config.ts`. |
| `P6-082` | CONFIRMED | **`PARK`** — trigger is the entry's own ("once Phase 5 spec work lands"); verify whether it has fired. |
| `P6-083` | CONFIRMED exactly as written | `KEEP`. |
| `P6-084` | CONFIRMED | `KEEP`. |
| `P6-085` | CONFIRMED | `KEEP`. |
| `P6-086` | CONFIRMED | `KEEP` — `rail.rs:952-956` names it as the still-open race. |
| `P6-087` | CONFIRMED | `KEEP` — Stage 6, needs a ruling. |
| `P6-088` | CONFIRMED | `KEEP` — Stage 6, needs a ruling; the latch precedent is already live in `cards.rs`. |
| `P6-089` | CONFIRMED | `KEEP` + `PROMOTE` — `validate.yml` still has no `cargo build -p three_rings` step. |
| `P6-090` | CONFIRMED | `KEEP`. |
| `P6-091` | — | **`DROP`** (2026-07-28, maintainer observation). One sanctioned refile exception; see the drop table. |
| `P6-092` | CONFIRMED, dropped anyway | **`DROP`** (2026-07-28, maintainer call) — not gating CI on release builds. Retired permanently. |
| `P6-093` | CONFIRMED | **`PARK`** — trigger: an observed collision, or a path seeding fractional positions. |
| `P6-094` | CONFIRMED | `KEEP` — `seed.rs:128-131` names the gap itself. |
| `P6-095` | CONFIRMED | `KEEP` — documenting the constraint is likely sufficient. |
| `P6-096` | CONFIRMED | **`MERGE`** → `P6-134`, with `P6-041`. |
| `P6-097` | PARTLY / UNVERIFIABLE | `RESCOPE` → measure first. Three candidate causes named; profile before and after `P6-108`. |
| `P6-098` | CONFIRMED | `KEEP`. |
| `P6-099` | CONFIRMED (token pairing) | `KEEP` — route through a deliberate dark-theme contrast pass. |
| `P6-100` | CONFIRMED | `KEEP`. |
| `P6-101` | CONFIRMED | `KEEP`. |
| `P6-102` | — | Done 2026-07-28 (config only); in the Done section. |
| `P6-103` | CONFIRMED | **`MERGE`** → `P6-005`. |
| `P6-104` | CONFIRMED | **`PARK`** — trigger: `P6-108` lands *and* profiling shows either read hot. |
| `P6-105` | PARTLY | `SPLIT` (2026-07-28) → `P6-108` + `P6-109`. Id retired. |
| `P6-106` | CONFIRMED | **`PARK`** — trigger is the spec's own: a UI surfaces stale tags. |
| `P6-107` | CONFIRMED | **`PARK`** — no near-term trigger; roadmap. |
| `P6-108` | — | Split product, inherits `P6-105.md`. Now gated on `P6-124`. |
| `P6-109` | — | Split product, parked in `TODO.md` with an unpark trigger. |

## What the pass did not settle

Named here so nobody assumes it was checked:

- **Every pixel measurement** in `P6-001` — the figures come from the original
  audit and the code has moved. Re-measure before fixing.
- **The `P6-002` latency claim** — "not firing at today's id layout" and "~50
  slots of headroom" are layout-dependent and were not re-measured.
  `npm run diag:resource-ids` is the tool.
- **All database state** — the pass never connected to Neon. So the "88 desires"
  figure in `P6-065`, the orphaned `zz-e2e-inb-src-w1-9` collection, and
  `P6-038e`'s Seq Scan are all unconfirmed.
- **Browser behavior** — `P6-185` (which engine fires `contextmenu` from keyup)
  and `P6-025`'s three claims are code shapes confirmed, runtime unconfirmed.
- **`P6-097`** — no profile was taken; the three candidate causes are read from
  the code, not measured.
- **`P6-187`** — whether the co-registration window that would make
  `Candidate.index` and `nav.highlighted()` disagree actually exists.
