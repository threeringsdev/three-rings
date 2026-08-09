# Specs

Feature specifications and project planning for Three Rings.

## Conventions

- One spec per file, named descriptively: `short-name.md`. The filename is the spec's stable identifier — never renamed once referenced.
- Start from `TEMPLATE.md`.
- Specs contain **no task lists**. Work needed to finish a draft goes in the spec's Open questions. Work tracking:
   - **Workbook** — the active execution queue (`workbook next`; see "Working the queue" below and [.workbook/guidelines.md](../.workbook/guidelines.md))
   - [TODO.md](TODO.md) — phase history (0–4) and the parked "Later" items
   - [TODO-Phase-5.md](TODO-Phase-5.md) — the Phase 5 record
   - [TODO-Phase-6.md](TODO-Phase-6.md) — the Phase 6 historical record (queue migrated to Workbook 2026-08-06)
- A spec moves through: `draft` → `accepted` → `implemented` (status noted at the top of each file).
  * `draft` — under discussion; **no implementation work may be based on it**
  * `accepted` — design settled; tasks gated on it may proceed (accepting a spec is a human decision)
  * `implemented` — built; kept as reference
- An `accepted` spec may retain open questions **only** if each is annotated *(resolved during execution — <where>)*; unannotated open questions block acceptance.
- **Execution order lives in exactly one place: the Workbook queue** (rank order, walked by `workbook next`). This index is a registry, not a schedule.

## Working the queue — process for agents (and humans)

Anyone told to "work on the next available task" follows this, with no other information required. The queue lives in **Workbook**; canonical statuses, priorities and the CLI lifecycle are in [.workbook/guidelines.md](../.workbook/guidelines.md).

1. Run `workbook next --json`. It returns the next available task: the highest-priority `ready` task in rank order whose dependencies are all `done` (rank order preserves the migrated Phase 6 execution sequence). Read the full task with `workbook show <id> --json` — the description carries the original entry's evidence, file:line references and acceptance criteria.
2. **Spec gating still applies.** The description's header line names the task's specs (`Specs: specs/....md`); every one must have status `accepted` or `implemented`, read from the spec file's header. If a needed spec is `draft`, the queue's real next action is spec review: report which spec blocks, offer to resolve its open questions, and wait for the human to flip the status. Never change a spec's status to `accepted` yourself.
3. Two statuses are deliberately not offered by `next`: `blocked` tasks labeled `decision-first` are waiting on a maintainer ruling — surface them, do not work them; `backlog` tasks labeled `parked` are deliberately deferred, each description carrying the trigger that unparks it.
4. Before starting: claim the task with `workbook update <id> --status in-progress --json`.
5. Read the spec the task links to (and its `Depends on:` specs) before writing any code.
6. Definition of done — ALL of:
   - The work is committed (conventional message describing the change).
   - Any findings, decisions, or surprises are recorded in the linked spec (Findings/Open questions sections).
   - New follow-up work discovered is filed as a new Workbook task (`workbook create … --status backlog`) — never silently absorbed.
   - The task is moved to `in-review` when the PR is ready for human review, and to `done` only after the work is accepted and merged.
7. If a task is ambiguous after reading its spec, **stop and ask** — do not guess. Record the question in the spec's Open questions first.

The retired TODO files keep their `[ ]`/`[~]`/`[x]` checkbox legend as historical record. The only still-live checkbox entries are TODO.md's parked "Later" items; promote one by filing it in Workbook.

## Index

| Spec | Status |
|---|---|
| [app-ui](app-ui.md) | accepted |
| [architecture-spike](architecture-spike.md) | implemented |
| [auth](auth.md) | implemented |
| [card-tagging](card-tagging.md) | implemented |
| [catalog-ingestion](catalog-ingestion.md) | accepted |
| [catalog-search](catalog-search.md) | implemented |
| [collection-api](collection-api.md) | implemented |
| [data-access-backends](data-access-backends.md) | implemented |
| [data-model](data-model.md) | implemented |
| [delivery-pipeline](delivery-pipeline.md) | implemented |
| [dev-environment](dev-environment.md) | implemented |
| [ui-component-bench](ui-component-bench.md) | implemented |
| [ui-components](ui-components.md) | implemented |
| [ui-design](ui-design.md) | implemented |
| [ui-work-loop](ui-work-loop.md) | accepted |
