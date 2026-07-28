---
name: phase-6-review
description: Verify one Phase 6 task at a time — dispatch a read-only subagent to check whether the entry is still true, brief the maintainer, then apply a disposition (keep/rescope/reclass/split/merge/drop/park/promote) to the queue. Use when the user says "review the next task", "verify P6-###", "run the phase 6 review", or asks to work through the Phase 6 triage. Also use to probe several tasks in parallel ahead of reviewing them.
---

# Phase 6 review — verify one task, decide, record, forget

107 entries accumulated in `specs/TODO-Phase-6.md` during Phase 5, written by
different review rounds over several weeks against code that has since changed.
Before any of it can be worked, each entry has to be checked for whether it is
still true. This skill runs that pass **one task at a time, with almost nothing
in context.**

## The context rule — this is the point of the skill

**You do not read the code.** A subagent does, writes its findings to a file,
and returns a ten-line summary. You hold the summary, the maintainer's answers,
and the disposition. That is all.

Everything needed to resume lives on disk:

| File | Role |
|---|---|
| `specs/TODO-Phase-6.md` | the queue — permanent ids `P6-001`..`P6-107` |
| `specs/TODO-Phase-6-triage.md` | severity classification per id |
| `specs/TODO-Phase-6-verification.md` | **the ledger — the resume point** |
| `specs/phase-6-probes/<id>.md` | one subagent report per task |

So: **clear or compact after every task.** Re-entering the skill with no
arguments picks up exactly where it stopped. Never carry a finished task's
detail into the next one, and never re-read a probe report you have already
distilled.

## Step 0 — pick the task

With an explicit id (`/phase-6-review P6-042`), use it. Otherwise read the
ledger and take the **first `pending` row in order** — but prefer a row already
marked `probed`, since its subagent has already run.

Read only that entry's line from `TODO-Phase-6.md` (`grep -n 'P6-042' specs/`)
and its triage rows. Do not read either file whole.

## Step 1 — probe (subagent, read-only)

Dispatch **one** subagent with `Agent`. Give it the task id, the entry's
**verbatim text**, and its triage class(es). The prompt below is the contract —
paste it, filling the three placeholders:

> You are verifying whether a single backlog entry in the three-rings repo is
> **still true**, against the code as it stands today. You are not reviewing the
> code and not proposing fixes.
>
> **Task id:** `<ID>`
> **Triage class(es):** `<CLASSES>`
> **The entry, verbatim:**
> ```
> <VERBATIM ENTRY TEXT>
> ```
>
> The entry was written by an earlier review round and may be stale, already
> fixed, partly fixed, or wrong as written. Treat every clause as a **claim to
> test**, not a description to trust. Where it cites `file:line`, check that the
> line still says what the claim says — line numbers drift and the claim may now
> point at unrelated code.
>
> **Your toolkit is read-only.** Read any file; `git log -S'<symbol>'`,
> `git log --oneline -- <path>` and `git blame` to find whether a fix already
> landed; `curl` the `:3000` watch server **only if it is already running**.
> Do not start or stop servers, run the test suite, touch the database, or
> modify any file except your own report. Confirm with `git status --porcelain`
> before you finish — the tree must be exactly as you found it, apart from your
> report.
>
> If a claim needs a runtime check you cannot do, say so and **name the exact
> check that would settle it**. Do not guess, and do not pad — "this claim is
> still true, here is the line" is a complete and valuable answer.
>
> **If the entry is a bundle** with `(a)`, `(b)`, `(c)` parts, give a verdict
> **per part**. That is the common case and the whole reason this pass exists.
>
> **Write your full report to `specs/phase-6-probes/<ID>.md`** using this shape:
>
> ```markdown
> # <ID> — <one-line restatement of the claim>
> Probed <DATE> against <git rev-parse --short HEAD>.
>
> ## Verdict: CONFIRMED | PARTLY | STALE | WRONG | UNVERIFIABLE
>
> ## Evidence
> - <claim> → <verdict> — `file:line`, what the code actually does now.
>
> ## What changed since the entry was written
> <commits or refactors that moved it, if any; "nothing" is a fine answer>
>
> ## If it is real, what fixing it touches
> <files//call sites, and anything that makes it bigger than it looks>
>
> ## Open — needs a runtime check
> <the named check, or "none">
> ```
>
> **Then return, as your entire final message, at most ten lines:** the verdict
> word, one line per sub-claim with its own verdict, the single most load-bearing
> piece of evidence, and a recommended disposition from `KEEP` / `RESCOPE` /
> `RECLASS` / `SPLIT` / `MERGE` / `DROP` / `PARK`. No preamble, no restatement.

Verdict vocabulary:

- **CONFIRMED** — the claim holds today, verified by reading the path end to end.
- **PARTLY** — some sub-claims hold, others are stale. Expected for bundles.
- **STALE** — real when written, since fixed or made moot.
- **WRONG** — never accurate as written, or the reasoning is inverted.
- **UNVERIFIABLE** — needs a runtime check; the report names it.

Mark the ledger row `probed` as soon as the report exists.

## Step 2 — brief, then grill

Give the maintainer **at most fifteen lines**:

1. What the entry claims, in one sentence — not a quote of the entry.
2. The verdict, and for a bundle the per-part split (`4 of 9 confirmed`).
3. The one piece of evidence that decides it.
4. What is at stake if it is real — the user-visible consequence, not the code.
5. What is still uncertain, and the named check that would settle it.

Then **stop and let them ask.** Answer from the probe report, reading only the
section asked about. If a question needs code the report does not cover, dispatch
a second narrow subagent rather than reading it yourself.

Grilling itself is not a skill in this repo — only the reply shape is, below.
Offer the grill in whichever direction is useful, and say which you are doing:

- **They grill you** — the default. Answer from evidence; when the honest answer
  is "the probe did not establish that", say so rather than reasoning to a
  plausible answer. A confident guess here poisons a disposition.
- **You grill them** — offer this when the disposition turns on a judgment only
  they can make (is this severity right, is this worth the cost, does this
  contradict a decision they already made). Ask **three questions at most**, each
  one that would change the disposition depending on the answer. Not a quiz for
  its own sake.

Do not propose the disposition until they are done asking.

### Shape every Q&A reply with `i-have-adhd`

Before your first reply in the Q&A, **invoke the `i-have-adhd` skill** and follow
it for the rest of this task's grill, in both directions. That skill does not
self-trigger — this instruction is what turns it on, so invoke it explicitly
rather than waiting to feel the need.

Skip it only when the maintainer has explicitly asked for something else — "stop
adhd mode", "normal mode", or a request for prose or the long version. If the
skill is not installed, say so in one line and follow the three-part shape below
on its own; it is the part that matters here.

Each reply, in this order:

1. **The answer in the first line** — the verdict word, the `file:line`, or "the
   probe did not establish that". Never a wind-up to it.
2. The evidence behind it, one or two lines.
3. If anything is still open, the one named check that would settle it.

**The shape changes; the evidence discipline does not.** A caveat is the answer,
not padding — "the probe did not establish that" survives every compression pass,
and a hedge carrying real uncertainty is never traded away for a cleaner line.
Where `i-have-adhd` asks for a closing next action, that action is the named check
or their next question — **never the disposition**, which stays gated until they
are done asking.

## Step 3 — dispose

Recommend exactly one, with a one-line reason, then apply it once they agree:

| Disposition | Means | Applies as |
|---|---|---|
| `KEEP` | Accurate, class right, scope right | Ledger only |
| `RESCOPE` | Real but the entry describes it wrongly or too broadly | Rewrite the entry text in place; id unchanged |
| `RECLASS` | Real, wrong severity class | Edit the triage row; ledger records old → new |
| `SPLIT` | Parts belong to different tasks — **the expected outcome for the 19 multi-class bundles** | New ids from the next free number for each part; original becomes a stub pointing at them, or is removed if fully split |
| `MERGE` | Duplicate of another id | Fold text into the survivor; this id is retired in the ledger, never reused |
| `DROP` | Fixed, wrong, or won't do | Delete from the queue; ledger keeps the id and the reason **permanently** — a dropped entry must not be rediscovered and refiled |
| `PARK` | Real, deliberately deferred | Move to the deferred section with an explicit **trigger condition** — what has to become true for it to matter |
| `PROMOTE` | Real and next — worth working now | Rewrite as an actionable task with acceptance criteria, per `specs/README.md` |

`SPLIT` allocates ids from the next free number in `TODO-Phase-6.md` — **never
renumber, never reuse**. Record in the ledger which sub-letter each new id came
from, so the probe report stays findable from the new entries.

## Step 4 — record and commit

Update the ledger row: `settled`, the disposition, a one-line reason. Then edit
`TODO-Phase-6.md` and `TODO-Phase-6-triage.md` as the disposition requires.

Commit **per batch, not per task** — one commit per working session over the
pass, message `docs(specs): verify P6-0xx..P6-0yy` with the dispositions in the
body. Committing per task buries the history under 107 commits.

Then tell the maintainer the task is settled and context can be cleared.

## Parallel mode

The probe step is independent per task; the grill step is not. So:

- **Probe ahead, review in sequence.** Dispatch probes for the next **3–5**
  pending ids in one message (they run concurrently), then work the ledger's
  `probed` rows one at a time. Each report is on disk, so nothing is lost to a
  clear between them.
- Do not probe more than ~5 ahead. Dispositions moot each other — a `SPLIT`
  upstream can invalidate a probe already run on a related entry — and a stale
  report read as fresh is worse than no report.
- Never run two grills at once. One task in front of the maintainer at a time is
  the entire design.

## Rules

- **You read no product code.** If you catch yourself opening a `.rs` file to
  check a claim, that is the subagent's job.
- **Nothing gets fixed during this pass.** It is a verification pass, not a fix
  pass. A confirmed blocker is recorded as confirmed and worked later.
- **The maintainer decides.** Recommend, argue once if you disagree, then apply
  what they choose. Never accept a spec as accepted or a task as done on their
  behalf.
- The `:3000` watch server and the emulator belong to the maintainer's session.
- Do not re-verify a `settled` row. If it needs revisiting, that is a new
  decision the maintainer makes explicitly, and the ledger records both.
