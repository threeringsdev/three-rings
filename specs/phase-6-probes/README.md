Read-only verification reports from the Phase 6 triage pass (2026-07-28 →
2026-07-30). Each is a subagent's evidence for the verdict recorded against a
Phase 6 task (now a `P6-NNN: …` task in the **Workbook queue**; the historical
record is [TODO-Phase-6.md](../TODO-Phase-6.md)).

Two shapes:

- **`P6-0xx.md`** — one file per task, from the first pass, which ran one task at
  a time with the maintainer in the loop.
- **`batch-<letter>-<topic>.md`** — one file per code area, from the second pass,
  which verified the remaining 96 entries in twelve parallel batches.

**The pass is complete**; these are kept as evidence, not as a work surface.
[TODO-Phase-6.md](../TODO-Phase-6.md)'s *Provenance* table maps every task id
back to the report that backs it — start there rather than grepping. Its
*Dropped during verification* table is the permanent never-refile record, and its
*What the pass did not settle* list names the claims that were **not** checked.

Reports were written against the code as it stood at the time and cite
`file:line`. **Line numbers drift** — grep for the cited symbol instead. Where a
report's own framing was later corrected by the maintainer or a follow-up probe,
the task's description (in Workbook, migrated verbatim from the queue entry) is
authoritative, not the report; the two
clearest cases are `P6-017a-cascade.md` (which found something far worse than the
entry it was probing) and `P6-017d-confirm-copy.md` (which found the entry
overstated).
