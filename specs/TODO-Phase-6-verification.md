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

**102 of 107 pending.** Two of the five `settled` were `[x]` before the pass began; `P6-010`, `P6-059` and `P6-068` settled 2026-07-28.
(Count the rows, not this page — a plain `grep` also matches the prose above.)

| # | Id | Class | Classes | Status | Disposition |
|---|---|---|---|---|---|
| 1 | `P6-010` | 3 correctness (was 1 blocker) | — | settled | `RESCOPE` + `RECLASS` — CONFIRMED, but hosted-only and self-heals on one reload, so not a blocker (maintainer call, 2026-07-28). Entry rewritten as a named S fix at `lib.rs:834-842` with acceptance criteria; triage row moved §1 → §3. |
| 2 | `P6-059` | 1 blocker | — | settled | `RESCOPE` — CONFIRMED, stays a blocker (maintainer call, 2026-07-28). Root cause found: no cargo dep edge on `migrations/`, not a flaky `touch`. Entry rewritten with **both** fixes required — `app/build.rs` `rerun-if-changed` *and* logging applied versions from the DB; the second is what removes the silent-success mode. |
| 3 | `P6-068` | 3 correctness (was 1 blocker) | — | settled | `RESCOPE` + `RECLASS` — CONFIRMED and diagnosed, so the "undiagnosed" blocker premise is gone (maintainer call, 2026-07-28). Cause: setup-body reads of `view_res` re-suspend `RequireAuth`'s `<Suspense>`; not the router. **Option B chosen** over the one-token `<Transition>` swap — remove the mis-wiring, not the symptom. M. Latent `/catalog` fragility deliberately **not** split: revisit only if a `Suspense` is ever added above `AppShell`'s `<Outlet/>`. |
| 4 | `P6-092` | 1 blocker | — | pending | — |
| 5 | `P6-102` | 1 blocker | — | pending | — |
| 6 | `P6-105` | 1 blocker | — | pending | — |
| 7 | `P6-017` | 2 data-integrity | 2, 3, 5, 7, 8 | pending | — |
| 8 | `P6-024` | 2 data-integrity | 2, 5, 8 | pending | — |
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
| 66 | `P6-091` | 5 ux-a11y | — | pending | — |
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
