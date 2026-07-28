# Phase 6 verification ledger

The state of the one-at-a-time verification pass over
[TODO-Phase-6.md](TODO-Phase-6.md). **This file is the resume point** — the
`phase-6-review` skill reads it to find the next task, and writes back the
outcome. Nothing about the pass lives in a conversation; clear or compact freely
between tasks.

## Columns

- **#** — review order: triage class first, then id. Blockers before hygiene, so
  a disposition that moots later entries happens early. Order is advisory; any
  id can be reviewed out of turn.
- **Classes** — set only when the entry's parts land in **more than one**
  severity class. That is the signature of a bundle that cannot be one task, and
  it is the primary `SPLIT` signal. 19 entries are marked.
- **Status** — `pending` (untouched) · `probed` (a subagent report exists at
  `phase-6-probes/<id>.md`, awaiting the maintainer) · `settled` (disposition
  applied to the queue).
- **Disposition** — one of `KEEP` · `RESCOPE` · `RECLASS` · `SPLIT` · `MERGE` ·
  `DROP` · `PARK` · `PROMOTE`, with a one-line reason. Defined in the skill.

## Progress

```sh
awk -F'|' 'NF>5 && $6 ~ /pending/' specs/TODO-Phase-6-verification.md | wc -l
```

**96 of 107 pending** · 2 `probed` (`P6-017`, `P6-024` — reports on disk, awaiting the maintainer) · 9 `settled`. Two of the nine `settled` were `[x]` before the pass began; `P6-010`, `P6-059`, `P6-068`, `P6-092`, `P6-102`, `P6-091` and `P6-105` settled 2026-07-28. **Ids allocated beyond the original 107 by `SPLIT`:** `P6-108`, `P6-109` (both from `P6-105`). Next free id: `P6-110`. Split products are not re-verified — they inherit the probe that produced them.
(Count the rows, not this page — a plain `grep` also matches the prose above.)

| # | Id | Class | Classes | Status | Disposition |
|---|---|---|---|---|---|
| 1 | `P6-010` | 3 correctness (was 1 blocker) | — | settled | `RESCOPE` + `RECLASS` — CONFIRMED, but hosted-only and self-heals on one reload, so not a blocker (maintainer call, 2026-07-28). Entry rewritten as a named S fix at `lib.rs:834-842` with acceptance criteria; triage row moved §1 → §3. |
| 2 | `P6-059` | 1 blocker | — | settled | `RESCOPE` — CONFIRMED, stays a blocker (maintainer call, 2026-07-28). Root cause found: no cargo dep edge on `migrations/`, not a flaky `touch`. Entry rewritten with **both** fixes required — `app/build.rs` `rerun-if-changed` *and* logging applied versions from the DB; the second is what removes the silent-success mode. |
| 3 | `P6-068` | 3 correctness (was 1 blocker) | — | settled | `RESCOPE` + `RECLASS` — CONFIRMED and diagnosed, so the "undiagnosed" blocker premise is gone (maintainer call, 2026-07-28). Cause: setup-body reads of `view_res` re-suspend `RequireAuth`'s `<Suspense>`; not the router. **Option B chosen** over the one-token `<Transition>` swap — remove the mis-wiring, not the symptom. M. Latent `/catalog` fragility deliberately **not** split: revisit only if a `Suspense` is ever added above `AppShell`'s `<Outlet/>`. |
| 4 | `P6-092` | 1 blocker | — | settled | `DROP` — CONFIRMED real, dropped anyway (maintainer call, 2026-07-28): **we are not gating CI on release builds.** No more crashes of that kind are expected, and a release-review step — likely manual — waits until the app is stable. Recreating it from scratch later was accepted as the tradeoff, so this id is retired permanently and must not be refiled. The probe also falsified the entry's "no GUI needed" clause (tauri 2.11.2 creates config windows *before* the setup hook, so a headless linux run needs xvfb) and the triage row's claim that one `cargo build -p three_rings` covers both this and P6-089 — no build at any profile catches a runtime panic. **P6-089 is unaffected and gets its own review.** |
| 5 | `P6-102` | ~~1 blocker~~ → done | — | settled | `PROMOTE` + `RECLASS` (§1 blocker M → config-only S) — **and fixed the same day at the maintainer's request** (2026-07-28), the one exception taken to this pass's no-fix rule. Probe verdict `PARTLY`: clause (a) (no second tab on web) is true but *deliberate*; clause (b) ("close this tab" doesn't return) is **wrong about web** — that page is native/Android-only. Maintainer then established the real repro: local `cargo leptos watch`, caused by `TR_EMBEDDED_ORIGIN` in the workspace `.env`. Deployed Render and release desktop were never affected, so the blocker premise was false. Fix: var moved to `beforeDevCommand` in `src-tauri/tauri.conf.json`; entry marked `[x]`; findings in `specs/auth.md` (2026-07-28). Second probe at `phase-6-probes/P6-102-discriminator.md` killed the `cfg(feature = "native")` approach (`cargo tauri dev` is served by the `hosted` `server` bin). Paired `P6-091` dropped separately (row 66). |
| 6 | `P6-105` | 1 blocker | — | settled | `SPLIT` (maintainer call, 2026-07-28) — probe verdict `PARTLY`: parts (b)(c)(d) confirmed unbuilt, but part (a) is **STALE as written** — `Mode::Bulk` and `server --ingest bulk` already ship gate-tested, so only the *run* remains. Two very different priorities in one entry, so: **→ `P6-108`** (part a + e-bulk: run the bulk load dev→prod and verify stage-2 acceptance; stays §1 in TODO-Phase-6.md, sized down L→M since it is a supervised run, not a build) and **→ `P6-109`** (parts b, c, d, f: the stage-3 daily incremental + Render cron + `/migrations` reconciliation + the spec status flip; **PARKED** in `TODO.md`'s Later/Parked section at the maintainer's direction — app stability and UI cleanup first — with unpark trigger *"a Scryfall set releases after the bulk load and the catalog visibly lacks it"*). Part (g) prereq was already satisfied. Triage open question resolved in favor of §1: 2,976 printings of ~116K, five set codes, so cards outside them are absent entirely rather than a clipped long tail. `P6-072f` reclassified from sibling to **prerequisite of `P6-108`**. Six references repointed (`P6-035`, `P6-104`, `P6-072f`, the Other-section gate, and two prose lines). This id is retired; probe report stays at `phase-6-probes/P6-105.md`, cited from both new entries. |
| 7 | `P6-017` | 2 data-integrity | 2, 3, 5, 7, 8 | probed | — |
| 8 | `P6-024` | 2 data-integrity | 2, 5, 8 | probed | — |
| 9 | `P6-031` | 2 data-integrity | — | pending | — |
| 10 | `P6-046` | 2 data-integrity | 2, 3, 5, 6 | pending | — |
| 11 | `P6-049` | 2 data-integrity | 2, 3, 5 | pending | — |
| 12 | `P6-054` | 2 data-integrity | — | pending | — |
| 13 | `P6-055` | 2 data-integrity | 2, 8 | pending | — |
| 14 | `P6-061` | 2 data-integrity | 2, 5, 6, 7, 8 | pending | — |
| 15 | `P6-002` | 3 correctness | — | pending | — |
| 16 | `P6-003` | 3 correctness | 3, 9 | pending | — |
| 17 | `P6-012` | 3 correctness | 3, 5, 8 | pending | — |
| 18 | `P6-013` | 3 correctness | — | pending | — |
| 19 | `P6-014` | 3 correctness | 3, 9 | pending | — |
| 20 | `P6-030` | 3 correctness | 3, 8 | pending | — |
| 21 | `P6-036` | 3 correctness | 3, 5, 6, 8 | pending | — |
| 22 | `P6-038` | 3 correctness | 3, 6, 8 | pending | — |
| 23 | `P6-039` | 3 correctness | — | pending | — |
| 24 | `P6-041` | 3 correctness | — | pending | — |
| 25 | `P6-042` | 3 correctness | 3, 5, 8 | pending | — |
| 26 | `P6-072` | 3 correctness | 3, 5, 7 | pending | — |
| 27 | `P6-074` | 3 correctness | — | pending | — |
| 28 | `P6-083` | 3 correctness | — | pending | — |
| 29 | `P6-086` | 3 correctness | — | pending | — |
| 30 | `P6-096` | 3 correctness | — | pending | — |
| 31 | `P6-005` | 4 capability | — | pending | — |
| 32 | `P6-016` | 4 capability | — | pending | — |
| 33 | `P6-019` | 4 capability | — | pending | — |
| 34 | `P6-023` | 4 capability | — | pending | — |
| 35 | `P6-044` | 4 capability | — | pending | — |
| 36 | `P6-047` | 4 capability | — | pending | — |
| 37 | `P6-048` | 4 capability | 4, 5 | pending | — |
| 38 | `P6-053` | 4 capability | — | pending | — |
| 39 | `P6-056` | 4 capability | — | pending | — |
| 40 | `P6-057` | 4 capability | — | pending | — |
| 41 | `P6-062` | 4 capability | — | pending | — |
| 42 | `P6-063` | 4 capability | — | pending | — |
| 43 | `P6-085` | 4 capability | — | pending | — |
| 44 | `P6-100` | 4 capability | — | pending | — |
| 45 | `P6-103` | 4 capability | — | pending | — |
| 46 | `P6-001` | 5 ux-a11y | — | pending | — |
| 47 | `P6-006` | 5 ux-a11y | — | pending | — |
| 48 | `P6-007` | 5 ux-a11y | — | pending | — |
| 49 | `P6-008` | 5 ux-a11y | — | pending | — |
| 50 | `P6-009` | 5 ux-a11y | 5, 6, 7, 8 | pending | — |
| 51 | `P6-011` | 5 ux-a11y | — | pending | — |
| 52 | `P6-020` | 5 ux-a11y | — | pending | — |
| 53 | `P6-021` | 5 ux-a11y | 5, 6 | pending | — |
| 54 | `P6-022` | 5 ux-a11y | — | pending | — |
| 55 | `P6-026` | 5 ux-a11y | — | pending | — |
| 56 | `P6-028` | 5 ux-a11y | — | pending | — |
| 57 | `P6-029` | 5 ux-a11y | 5, 6 | pending | — |
| 58 | `P6-033` | 5 ux-a11y | — | pending | — |
| 59 | `P6-034` | 5 ux-a11y | — | pending | — |
| 60 | `P6-035` | 5 ux-a11y | — | pending | — |
| 61 | `P6-045` | 5 ux-a11y | — | pending | — |
| 62 | `P6-069` | 5 ux-a11y | 5, 8 | pending | — |
| 63 | `P6-084` | 5 ux-a11y | — | pending | — |
| 64 | `P6-087` | 5 ux-a11y | — | pending | — |
| 65 | `P6-088` | 5 ux-a11y | — | pending | — |
| 66 | `P6-091` | 5 ux-a11y | — | settled | `DROP` — not probed; dropped on maintainer observation (2026-07-28) while working `P6-102`, to which it was tied ("do with P6-102"). Judged a stale artifact: the unstyled raw-HTML auth pages were not observed during live local + deployed auth testing that day. Maintainer will refile if seen again — **that is this id's one sanctioned exception to the never-refile rule**, and a refile should cite this row. Caveat recorded at drop time: only the native "close this tab" page was actually exercised; `/auth/app-return` (Android deep-link bounce) was not. |
| 67 | `P6-095` | 5 ux-a11y | — | pending | — |
| 68 | `P6-098` | 5 ux-a11y | — | pending | — |
| 69 | `P6-099` | 5 ux-a11y | — | pending | — |
| 70 | `P6-101` | 5 ux-a11y | — | pending | — |
| 71 | `P6-090` | 6 performance | — | pending | — |
| 72 | `P6-097` | 6 performance | — | pending | — |
| 73 | `P6-104` | 6 performance | — | pending | — |
| 74 | `P6-004` | 7 dev-loop | — | pending | — |
| 75 | `P6-025` | 7 dev-loop | — | pending | — |
| 76 | `P6-027` | 7 dev-loop | — | pending | — |
| 77 | `P6-032` | 7 dev-loop | — | pending | — |
| 78 | `P6-037` | 7 dev-loop | — | pending | — |
| 79 | `P6-040` | 7 dev-loop | — | pending | — |
| 80 | `P6-043` | 7 dev-loop | — | pending | — |
| 81 | `P6-050` | 7 dev-loop | — | pending | — |
| 82 | `P6-052` | 7 dev-loop | — | pending | — |
| 83 | `P6-058` | 7 dev-loop | — | pending | — |
| 84 | `P6-060` | 7 dev-loop | — | pending | — |
| 85 | `P6-065` | 7 dev-loop | — | pending | — |
| 86 | `P6-070` | 7 dev-loop | — | pending | — |
| 87 | `P6-075` | 7 dev-loop | — | pending | — |
| 88 | `P6-076` | 7 dev-loop | — | pending | — |
| 89 | `P6-077` | 7 dev-loop | — | pending | — |
| 90 | `P6-078` | 7 dev-loop | — | pending | — |
| 91 | `P6-079` | 7 dev-loop | — | pending | — |
| 92 | `P6-081` | 7 dev-loop | — | pending | — |
| 93 | `P6-082` | 7 dev-loop | — | pending | — |
| 94 | `P6-089` | 7 dev-loop | — | pending | — |
| 95 | `P6-094` | 7 dev-loop | — | pending | — |
| 96 | `P6-015` | 8 hygiene | — | pending | — |
| 97 | `P6-018` | 8 hygiene | — | pending | — |
| 98 | `P6-051` | 8 hygiene | — | pending | — |
| 99 | `P6-067` | 8 hygiene | — | pending | — |
| 100 | `P6-071` | 8 hygiene | — | pending | — |
| 101 | `P6-093` | 8 hygiene | — | pending | — |
| 102 | `P6-106` | 8 hygiene | — | pending | — |
| 103 | `P6-107` | 8 hygiene | — | pending | — |
| 104 | `P6-064` | 9 close-out | — | pending | — |
| 105 | `P6-066` | 9 close-out | — | pending | — |
| 106 | `P6-073` | 9 close-out | — | settled | DROP from queue — done 2026-07-25, belongs in a done log |
| 107 | `P6-080` | 9 close-out | — | settled | DROP from queue — done 2026-07-25, belongs in a done log |
