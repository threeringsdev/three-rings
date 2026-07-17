# Specs

Feature specifications and project planning for Three Rings.

## Conventions

- One spec per file, named descriptively: `short-name.md`. The filename is the spec's stable identifier — never renamed once referenced.
- Start from `TEMPLATE.md`.
- Specs contain **no task lists**. All work tracking lives in [TODO.md](TODO.md); work needed to finish a draft goes in the spec's Open questions.
- A spec moves through: `draft` → `accepted` → `implemented` (status noted at the top of each file).
  * `draft` — under discussion; **no implementation work may be based on it**
  * `accepted` — design settled; tasks gated on it may proceed (accepting a spec is a human decision)
  * `implemented` — built; kept as reference
- An `accepted` spec may retain open questions **only** if each is annotated *(resolved during execution — <where>)*; unannotated open questions block acceptance.
- **Execution order lives in exactly one place: [TODO.md](TODO.md).** This index is a registry, not a schedule.

## Working the queue — process for agents (and humans)

Anyone told to "work on the next available task" follows this, with no other information required:

1. Read `TODO.md`. Phases are ordered top to bottom; tasks within a phase are ordered top to bottom.
2. **The next available task is the first task marked `[ ]`** in the topmost phase that contains one, skipping any task that is **blocked**. A task is blocked if:
   - a listed prerequisite task is not yet `[x]`, or
   - any spec in its `(specs: ...)` annotation does not have status `accepted` or `implemented`. Spec status is read from the spec file's header — it is never duplicated in TODO.md.

   Tasks with no `(specs: ...)` annotation are ungated. Tasks marked `[~]` are in progress — do not start them without being asked; do not skip past a `[~]` task's phase, work the next `[ ]` in it.

   **If every `[ ]` task is blocked by a `draft` spec**, the queue's real next action is spec review: report which specs are blocking, offer to resolve their open questions and finalize the draft, and wait for the human to flip the status to `accepted`. Never change a spec's status to `accepted` yourself.
3. Before starting: change the task's `[ ]` to `[~]` and commit that change with message `start: <task summary>`.
4. Read the spec the task links to (and its `Depends on:` specs) before writing any code.
5. Definition of done — ALL of:
   - The work is committed (conventional message describing the change).
   - The task's `[~]` is changed to `[x]` in the same commit as the final work.
   - Any findings, decisions, or surprises are recorded in the linked spec (Findings/Open questions sections).
   - New follow-up work discovered is added as new `[ ]` tasks in the appropriate phase — never silently absorbed.
6. If a task is ambiguous after reading its spec, **stop and ask** — do not guess. Record the question in the spec's Open questions first.

Task state legend: `[ ]` available · `[~]` in progress · `[x]` done.

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
