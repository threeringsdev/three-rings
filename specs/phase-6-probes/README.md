Per-task verification reports written by the `phase-6-review` skill, one file
per task id. Each is a subagent's evidence for the verdict recorded against that
id in [TODO-Phase-6-verification.md](../TODO-Phase-6-verification.md).

These exist so the evidence survives a context clear without occupying context:
the orchestrating session reads a report only when the maintainer asks about
that specific task.
