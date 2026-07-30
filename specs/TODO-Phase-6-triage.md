# Phase 6 triage — why the queue is ordered the way it is

The original triage (2026-07-28) classified 107 entries into eight severity
classes. Verification (2026-07-28 → 2026-07-30) then checked every one against
the code, split the bundles, and dropped what was stale. **That work is done**,
and per this document's own plan the classification has now been reduced to what
it exists for: the **rationale for the order** in
[TODO-Phase-6.md](TODO-Phase-6.md).

- The queue itself, in execution order → [TODO-Phase-6.md](TODO-Phase-6.md)
- Per-id verdicts, dispositions and the bundle→id map →
  [TODO-Phase-6-verification.md](TODO-Phase-6-verification.md)
- Evidence → [phase-6-probes/](phase-6-probes/)

Size shirts: **S** ≤ half a day · **M** a day or two · **L** a week, or a design
decision first.

---

## The ordering, and why

**Stage 1 — destructive actions and data integrity.** The user's collection is
the asset, and these are the only entries that cost the user something they
cannot get back. They are also cheap: 14 of the 17 are S, and they cluster in
`hosted.rs` and `my/collection.rs`, so they sweep together in one or two
sessions. `P6-110` leads because it is a policy decision the rest of the stage
should be consistent with.

**Stage 2 — ship gates.** Small, and each closes a hole that lets a bad change
reach `main` or prod unnoticed. Doing them before the bulk work means the bulk
work is protected by them. `P6-124` sits here rather than in a correctness stage
purely because it is a hard prerequisite of `P6-108`.

**Stage 3 — leverage.** Each of these removes a *category* of future defect
rather than one instance: the upstream resource bug, the payload-collision class,
the flaky-hydration class, the error-status class, the focus-trap gap across
every dialog, the `CommandEmpty` conflation, the `tw_merge` ring collision across
five primitives. Doing them before Stage 4 makes Stage 4 cheaper and the tests
trustworthy.

**Stage 4 — correctness.** Wrong or misleading, not destructive. Grouped by file
rather than by severity, because the binding constraint here is context-switching
cost, not risk.

**Stage 5 — missing capability.** Specified or drawn, not built. `P6-005` leads:
the add flow is the product, and it is the one gap where a frame draws a control
the app does not have.

**Stage 6 — decisions the maintainer owns.** Six entries blocked on a ruling
rather than on code, collected so they can be answered in one sitting. Each is
cheap once answered, and three of them (`P6-088`, `P6-087`, `P6-067`) set
precedent that later work will follow whether or not it is decided
deliberately.

**Stages 7–10 — UX/a11y, performance, dev loop, hygiene.** Ordered last not
because they are unimportant but because none of them block anything above them.
Within Stage 9, the "fold this trap into a skill" items are worth doing early and
cheaply: they are the ones that stop the *next* agent repeating a mistake.

**Parked** entries each carry an explicit trigger. Nothing there is worked until
its trigger fires — that is the whole point of parking rather than deleting.

---

## What verification changed about the original classification

The triage was written taking every entry at its own word. Six of those words
turned out to be wrong in ways that moved an entry:

1. **A blocker nobody had filed.** The `P6-017a` follow-up probe found that
   deleting a collection **destroys every holding and desire in it and every
   collection under it**, irreversibly, and that the `moves` ledger survives
   *falsified* rather than merely incomplete. That is `P6-110`, and it is more
   severe than anything the original triage classified. It was reached by asking
   a question the entry did not ask — "what actually happens to the cards?" —
   which is the argument for the follow-up probes over accepting a bundle's own
   framing.
2. **The reverse, on the same bundle.** `P6-017d` was elevated in triage as
   "delete/rename pointed at the wrong collection". Verification found the dialog
   reads its own snapshot and names *and deletes* the same collection throughout;
   the residual is only that the page behind it can change identity. Elevating it
   was wrong, and it is now an S.
3. **`P6-013` was never reported upstream.** "Already filed" meant filed in this
   backlog. It is still the highest-leverage item in the file, and its version
   cites were wrong besides — a bump to `leptos_server-0.8.7` would not have
   fixed it, because that function is byte-identical.
4. **Two entries were understated.** `P6-022` hits five vendored primitives, not
   one, and the cause is upstream in `tw_merge`. `P6-039` has four definitions of
   "owned", not three.
5. **`P6-032`'s drift recurred** during the verification window itself, two days
   after the manual resync. That retires the "just resync it again" option and is
   why it sits in Stage 2 rather than in hygiene.
6. **19 sub-claims were already fixed.** Mostly by `7649d80a` (the tray/toaster
   round) and the state-arms and teardown tasks. The original triage's own
   warning — that a quarter of the entries predated the 2026-07-25 loop
   recalibration — was correct, and the drop table in
   [TODO-Phase-6.md](TODO-Phase-6.md) is the permanent record so they are not
   refiled.

The one structural observation from the original triage held up and is worth
keeping: **the bundled "minors from its review round" entries were not minor as
a class.** Each was scoped by a reviewer under an explicit instruction to hold
everything below the major bar, so they were sorted by *review-round confidence*,
not by user impact. Ten of the seventeen Stage 1 entries are sub-items of a
bundle labeled "none reaching the major bar" — including `P6-112`, which lets an
API caller collapse every board in a deck onto `main`.

---

## Severity classes, for reference

The classification is no longer maintained per-id — the queue's stage ordering
supersedes it — but the classes are the vocabulary the probe reports use:

1. **Blockers** — the app is not functional until fixed. Now Stage 2 (`P6-059`,
   `P6-108`); `P6-010`, `P6-068` and `P6-092` left this class on verification.
2. **Data integrity & destructive actions** — Stage 1.
3. **Correctness** — wrong or misleading, not destructive. Stage 4.
4. **Missing capability** — specified or drawn, not built. Stage 5.
5. **UX, a11y and visual** — Stage 7, with the rulings pulled forward to Stage 6.
6. **Performance** — Stage 8.
7. **Dev loop, tests and process** — Stage 9. Doesn't touch users; decides
   whether the next hundred tasks are correct.
8. **Hygiene and docs** — Stage 10.
9. **Close out** — folded into the Done and Dropped sections of the queue.
