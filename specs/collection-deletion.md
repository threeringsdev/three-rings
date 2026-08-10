# Collection deletion

**Status:** accepted
**Depends on:** [data-model](data-model.md) (owns `collections`, `holdings`,
`desires`, `moves` and the FKs this changes), [collection-api](collection-api.md)
(owns the endpoint surface and the existing `teardown` this reuses),
[app-ui](app-ui.md) (owns the confirm dialog and the tree), [auth](auth.md) (the
user id everything scopes to)

## Problem

**Deleting a collection today destroys every card in it, and in every collection
nested under it, permanently and silently.**

`delete_collection` is a single statement — `DELETE FROM collections WHERE id = $1
AND NOT is_inbox` (`app/src/backend/hosted.rs:510`). Everything else is the
database's own referential action:

- `collections.parent_id` is `ON DELETE CASCADE`, so the whole subtree goes.
- `holdings.collection_id`, `desires.collection_id` and `card_tags.collection_id`
  are `ON DELETE CASCADE`, so every card and every want in that subtree goes with
  it.
- There is no soft delete, no `deleted_at`, no trash, no archive, no audit table
  and no undo entry. Nothing re-parents the rows — not to the parent, not to the
  Inbox.

A card held **only** inside the deleted subtree disappears from the user's
ownership entirely, because `owned_by_card` is a pure aggregate over `holdings`.

**The `moves` ledger does not merely fail to record this — it survives
falsified.** `moves.from_collection_id` and `moves.to_collection_id` are
`ON DELETE SET NULL`, and NULL in that schema is not "unknown": it means
*external intake* (`from`) and *removal* (`to`). So historical moves in and out
of the deleted subtree silently rewrite themselves into intakes and removals, and
undoing one of them will not put the copies back — `undo_one` skips a NULL end
(`hosted.rs:2005-2010`). No move row records the deletion itself, so the ledger
contains no trace of the copies that were destroyed.

The confirm dialog does say "This permanently deletes … This cannot be undone",
and its card count covers the subtree — but it counts holdings only, never the
desires that die with them, and when the tree read is stale or failed it silently
omits the nested-collections clause altogether. (That was filed as `P6-111` and
is now absorbed by `P6-189` in [TODO-Phase-6.md](TODO-Phase-6.md), which
implements the corrected dialog below.)

**The user's framing, which this spec follows:** a collection is not a box that
cards are trapped in. Collections can represent physical locations, but they are
also ephemeral — deleting a *deck* does not mean you threw the cards in the
trash. Deletion should relocate, not destroy.

## Scope

**In:**

- Soft deletion of a collection row, and the read-path filtering it forces.
- Where holdings and desires go, with a default and user-chosen overrides.
- What happens to child collections.
- What the `moves` ledger records, and what it stops falsifying.
- Undo (immediate) and Restore (later), which are deliberately different
  operations.
- The confirm dialog's counts and copy.

**Out:**

- **Automatic purge of soft-deleted rows.** Deliberately deferred: the rows are
  small, and a purge policy is a decision that can be made later when it matters.
  No scheduled-job home exists in this app today.
- **Permanent-delete-from-trash.** Follows purge; not needed for the MVP.
- **Bulk delete** of several collections at once. The tree offers one at a time.
- **Undoing a `Discard`d desire individually.** Desires have no ledger and no
  quantity operation (`P6-085`); this spec makes them recoverable *as part of the
  collection*, not row by row.
- **Deleting the Inbox.** Remains refused, unchanged.

## Design

### The core idea

Delete **hides** the collection; it does not destroy it. Anything the user
chooses to *move out* moves through the real `moves` ledger with existing
machinery. Anything they do *not* move out simply goes hidden with the
collection. That is one mechanism, fully reversible, introducing no new
destruction path.

The consequence worth stating plainly: **"discard" writes nothing.** Discarded
rows stay attached to the now-hidden collection, disappear from every count and
every view because the collection is filtered out, and return intact on undo.

### Data model

One migration (`0010_collection_soft_delete.sql`):

```sql
ALTER TABLE collections ADD COLUMN deleted_at timestamptz;

-- The hot path is "live collections for this user", so index that specifically.
CREATE INDEX collections_user_parent_live_idx
    ON collections (user_id, parent_id) WHERE deleted_at IS NULL;
```

Notes:

- The existing `collections_one_inbox` unique index (`WHERE is_inbox`) is left
  alone: the Inbox cannot be deleted, so a soft-deleted Inbox cannot exist.
- The `ON DELETE CASCADE` on `parent_id`, `holdings`, `desires` and `card_tags`
  **stays**. It is now only reachable by a hard delete — which this spec does not
  perform — and by user deletion, where cascading is correct.
- The `ON DELETE SET NULL` on the two `moves` collection columns **stays** for
  the same reason. Soft delete never fires it, which is precisely what stops the
  ledger falsifying.
- This migration is expand-only and safe to apply before the code that reads the
  column.

### What deleting does

Deleting `Mono-Red` — a deck under `Shoebox`, holding 12 cards, wanting 3, with
2 child collections:

| Thing | What happens |
|---|---|
| The collection row | `deleted_at = now()`. Hidden everywhere. Not destroyed. |
| Its child collections | `parent_id` re-pointed to `Shoebox`. They survive untouched. |
| Its holdings | Move out as real ledger moves. Default destination: `Shoebox`. |
| Its desires | Default: stay attached, hidden with the collection. |
| The Inbox | Still refused (`AND NOT is_inbox`). |

**Children survive.** Delete removes exactly one node. A child re-parents to the
deleted collection's parent, or becomes top-level if the deleted collection was
top-level. Deleting a folder means "un-group these", not "destroy these".

### The two dispositions

```rust
pub struct DeleteCollectionReq {
    pub collection_id: Id,
    pub haves: HaveDisposition,
    pub wants: WantDisposition,
}

/// Where the collection's holdings go. `ToParent` is the default.
pub enum HaveDisposition {
    ToParent,                     // nearest surviving parent, or Inbox if top-level
    To { collection_id: Id },
    ReturnToPrevious,             // reuses the existing teardown mode
    Discard,                      // stays attached to the hidden collection
}

/// Where the collection's desires go. `Discard` is the default.
pub enum WantDisposition {
    Discard,                      // stays attached to the hidden collection
    To { collection_id: Id },
}
```

**`Discard` is the internal name; the user never sees that word.** Both controls
label this option **"Remove from Collection"** (resolved 2026-08-05). The variant
keeps the shorter name because it is what the code and the ledger reasoning call
it throughout this spec, but the label is not a free choice at implementation
time — it is specified here, and it is the same string on the have side and the
want side.

The two are chosen **separately**, because a have and a want are not the same
kind of thing: a have is a physical object that must be somewhere, a want is an
intention that was very likely scoped to the deck being deleted.

`ReturnToPrevious` costs almost nothing to offer — it is the existing
`Teardown::ReturnToPrevious` machinery (`hosted.rs`), which sends each card back
to the most recent collection it was moved into this one *from*, falling back to
the Inbox where there is no history.

`ToParent` resolves to the **nearest surviving parent**. Because children
re-parent rather than cascade, and because the deleted collection's own parent is
by definition not being deleted in the same operation, this is always exactly the
collection's `parent_id` — or the Inbox when `parent_id IS NULL`.

### The receipt

Mirrors `TeardownReceipt`, and for the same reason: the caller needs handles, not
a count.

```rust
pub struct DeleteCollectionReceipt {
    pub collection_id: Id,
    pub move_ids: Vec<Id>,     // holdings relocations, in write order
    pub reparented: Vec<Id>,   // children whose parent_id changed
}
```

`Discard`ed rows produce no ids because they produce no writes. The receipt is
what the undo toast holds.

### Undo and Restore are different operations

This is deliberate and the difference must survive into the UI copy.

**Undo** — from the delete toast, reversing the operation as a whole:

1. Clear `deleted_at`.
2. Reverse every `move_id` in the receipt (existing `undo_moves`).
3. Re-parent every id in `reparented` back to the restored collection.

Correct for the misclick, which is what a toast is for.

**Restore** — from the "Recently deleted" list, potentially days later:

1. Clear `deleted_at`.
2. Re-attach to the original parent if that parent is still live; otherwise
   top-level.
3. **Leave cards and children where they now are.**

Restore is not a time machine. By the time it is used, the re-parented children
may have been moved, renamed, or deleted themselves, and the relocated cards may
have been moved on, played with, or sold. Reversing into that state is
destructive, not helpful. The collection comes back with whatever `Discard`ed
rows went hidden with it, and nothing else.

### The read path — the risk in this design

Soft deletion is cheap to write and easy to get wrong to read. **Every query
touching `collections`, `holdings` or `desires` must exclude soft-deleted
collections**, or a deleted collection's cards keep counting toward totals the
user can no longer see or reach.

**`P6-039` closed this out.** Owned-per-oracle now comes from exactly **one**
source: the `owned_by_card` SQL view (`migrations/0003_collections.sql`,
`security_invoker`). `search`/`card_summary` (via `owned_by_oracle`) and
`collection_view` already read it directly; `collection_tree`'s
`shopping_short` badge, `all_cards`, and `shopping_list` were each re-deriving
`sum(holdings.quantity)` joined through `printings`, grouped by oracle id —
`P6-039` collapsed all three onto `SELECT oracle_id, owned FROM owned_by_card`.
A structural test (`owned_definition_guard` in `hosted.rs`) fails `cargo test`
if an inline copy of that aggregation reappears.

So the future `deleted_at IS NULL` filter for owned-per-oracle lands **exactly
once, in the view definition** (a migration), not scattered across `hosted.rs`.

**Caution — "one place" covers owned-per-oracle only.** `needs`'
`present_here`/`elsewhere` CTEs and `suggested_destinations`' `desired`/
`present` CTEs are collection-scoped aggregations over `holdings` in their own
right (present-in-this-collection, present-elsewhere, per-collection demand) —
they are not re-derivations of `owned_by_card` and `P6-039` deliberately left
them alone (see specs/collection-api.md Findings, 2026-08-09). Each of these
still needs its own `deleted_at IS NULL` handling when this task lands; do not
assume the view fix covers them.

Other sites needing the filter: `collection_tree`, `list_collections`,
`require_owned_collection`, `inbox_id`, `needs`, `suggested_destinations`, and
`search`'s ownership read.

`require_owned_collection` deserves specific attention: a soft-deleted collection
must **not** be a valid move destination or a valid write target, so it should
fail the ownership check exactly as a non-existent one does.

### The confirm dialog

```
Delete "Mono-Red"?

  12 cards       → [ Shoebox (parent)        ▾ ]
   3 wants       → [ Remove from Collection  ▾ ]
   2 collections → move up to Shoebox

  [ Cancel ]   [ Delete ]
```

- The card count is **`present` for this collection only**, no longer
  `present + present_rollup`. Since children survive, rolling their cards into
  the number would overstate what is being relocated. This supersedes `P6-111`'s
  undercount, in the opposite direction from how that entry was filed.
- The wants count is stated explicitly. Today it is never mentioned at all.
- The child-collections line states where they go, and appears only when there
  are children. When the tree read is stale or failed the dialog must not
  silently omit it — that is `P6-111`'s degraded-state bug, and the fix is that
  the count comes from the same snapshot the write acts on.
- The dialog already reads its own `DeleteReq` snapshot and names the collection
  it will actually delete, verified in
  `phase-6-probes/P6-017d-confirm-copy.md`. Keep that property.
- "This cannot be undone" must go. It will no longer be true.

### The "Recently deleted" list

A small list under My cards: name, kind, when, and a Restore button. No purge, no
permanent delete, no counts. It exists so the soft delete is reachable after the
toast is gone; anything more is out of scope.

## Testing

- **Unit, pure:** disposition planning — given a collection, its children and its
  parent, which children re-parent where, and which grain goes to which
  destination. Follows the `plan_move` / `plan_drop` precedent of a pure planner
  tested apart from the write.
- **Unit:** `ToParent` resolves to the Inbox for a top-level collection.
- **Integration (dev branch):** delete → assert the collection is invisible to
  every read path, the cards are at the destination, the children are re-parented,
  and the ledger rows are real moves rather than intakes/removals; then undo →
  assert full restoration.
- **The regression that matters most:** a card held **only** in the deleted
  collection must still be `owned` afterwards, at the destination — and must
  **not** be owned when `Discard`ed. That single assertion covers the four "owned"
  definitions at once, and is the reason `P6-039` comes first.
- **e2e:** delete a collection with children and cards from the tree, assert tree
  shape and card locations, press Undo, assert the original state returns.

## Suggested sequencing

1. `P6-039` — collapse the four "owned" definitions onto one source.
2. Migration `0010` + the read-path filter everywhere, with no behavior change
   yet (nothing sets `deleted_at`, so every filter is a no-op and can land
   safely).
3. The delete operation: dispositions, receipt, ledger moves, re-parenting.
4. The confirm dialog with the two pickers and corrected counts.
5. Undo toast + the "Recently deleted" list.

Steps 2–5 are each independently shippable; step 2 is deliberately inert so the
risky part lands under no time pressure.

## Open questions

- *(resolved — 2026-08-05)*
  **Should `Discard` be the word on the have-side control?** Use `Remove from Collection`
  for both haves and wants for this action.

- *(resolved — 2026-08-05)*
  **Does a soft-deleted collection's name still block re-use?** There is no
  unique constraint on `(user_id, parent_id, name)` today, so creating a new
  "Mono-Red" while a deleted one exists is already legal.
  This is intentional, _names_ of collections do not need to be unique (collection_ids
  should already be unique).
- *(resolved — 2026-08-05)*
  **Retention.** No purge is specified. Policy will be decided once a fully working
  POC of the app is implemented (currently there are no users).

- *(resolved — 2026-08-09, maintainer ruling by Dylan; raised 2026-08-09, step 2)*
  **What should `undo_move` do when the move's other end is now hidden?**
  **Redirect the copies to the user's Inbox.** Cards always come back, and they
  never silently land in a collection the user cannot see. The other two
  candidates were considered and rejected: *refuse* leaves the user holding a
  toast that does nothing, and *restore the collection* un-deletes something
  they deliberately deleted as a side effect of an unrelated undo.

  The question, for the record: `undo_one` reverses a ledger row by writing
  straight into `from_collection_id` / `to_collection_id` — it is the one write
  path in `hosted.rs` that never goes through `require_owned_collection`,
  because the ids come from the ledger rather than the caller. Undoing an *old,
  unrelated* move whose source has since been soft-deleted would otherwise put
  copies back into a hidden collection. The delete's **own** undo is unaffected
  either way: it clears `deleted_at` first, so the source is live again by the
  time the moves are reversed. Implemented in step 3 (`live_or_inbox`, applied
  to `undo_one`'s write-back, so `undo_move` / `undo_moves` / `undo_last_move`
  all inherit it).

## Findings

- 2026-08-09 — **Step 4 review fixes** (adversarial review of the `P6-189`
  commit below; zero majors, six smaller findings, all fixed here).
  1. **The pinned "(parent)" row's search value was the literal string
     `"Parent"`, not the parent's name.** `command`'s typed-text filter
     matches against `value`, not `label` — so typing the parent's *actual*
     name (the single most obvious thing to type for the picker's own
     default) matched nothing, since the real row for that collection is
     also excluded from the plain list. `value` is now the parent's plain
     name; the "(parent)" affordance stays label-only.
  2. **`have_destinations` excluded the parent by routing `Some(pid)` through
     `resolve_parent` too**, when `pid` was already the known id with nothing
     to look up. Now excludes by the known id directly for that case;
     `resolve_parent` is only still needed for the top-level (Inbox) case,
     where there is no id to start from.
  3. **The children-line's stale-tree-read fallback, `"its former parent"`,
     read as the children staying put** rather than moving up a level —
     reachable via the header kebab when `collection_view` succeeds but the
     sidebar's separate tree read hasn't caught the parent's row yet.
     `children_destination_label` now returns `Option<String>`, and the
     caller drops "to …" entirely when it's `None`, saying only what's
     certain: "N collection(s) move up a level."
  4. **The dialog lost its `DialogDescription` entirely** in the rewrite —
     restored one sentence: nothing is deleted, and "Remove from Collection"
     leaves items attached and recoverable. Deliberately silent on *how* to
     get them back (no "Restore" promise) — the "Recently deleted" list is
     step 5 (`P6-190`), not built yet.
  5. **`bench/my_root.rs`'s fixture rows all hardcoded `desired: 0`** — no
     bench row has ever shown a nonzero wants count. One row (`Trade`) now
     carries `desired: 6`, cheap insurance for any future bench section that
     reads it (today none does — see the runtime-verification note below).
  6. **Verified live, not by reading: Escape with a picker popover open
     closed the whole delete dialog too.** Confirmed with a throwaway
     Playwright probe against the running dev server before touching any
     code (`create → open delete → open haves picker → Escape → dialog
     data-state`), which read `closed` — the bug was real. Root cause: our
     `Popover` (`app/src/components/ui/popover.rs`) never registered with
     `overlay_stack` the way `Dialog` does, so `Dialog`'s own window-level
     Escape listener always believed itself topmost regardless of an open
     popover nested inside it, and closed on the same keypress that the
     browser's native `popover="auto"` light-dismiss was *also* closing the
     popover for. This exact nesting (a `Popover` mounted inside a `Dialog`)
     didn't exist anywhere in the app before this task's two pickers — every
     earlier `Popover` (the sticky destination picker, the tray's "Move
     to…") is a top-level overlay, never a dialog's child, which is why nine
     days of `Popover` in production never surfaced it. Fix: `PopoverContent`
     now pushes/removes its own id on `overlay_stack` exactly like
     `DialogContent`, and owns its own Escape listener gated on
     `overlay_stack::is_top`, consuming the keypress
     (`stop_immediate_propagation`) so a wrapping `Dialog` never sees it —
     `dialog.rs` itself needed **zero** changes. Verified live again after
     the fix (first Escape closes only the popover, dialog stays `open`;
     second Escape then closes the dialog normally) and pinned as a
     permanent regression test (`collection-tree-manage.spec.ts`, "Escape
     closes only the open picker, not the delete dialog behind it").
  - **Verification.** `cargo fmt --check`, `clippy` (hosted, native, hydrate,
    plus both `component-bench` lines) and `cargo test -p app --features
    hosted` (256, up from 255) all green. Full chromium `@fast` + non-`@fast`
    pass, `--workers=1`, 252 tests: **234 passed, 18 failed.** 17 of the 18
    are `` `the fixture has fewer than N catalog cards the dev user owns
    nowhere` `` in `batch-move.spec.ts` (4), `command-palette.spec.ts` (3),
    `needs.spec.ts` (5) and `removal.spec.ts` (5) — the finite
    `catalog/search?q=z` fixture pool this task's own Findings entry above
    already flagged as drained by this task's repeated local iteration; not
    a regression from these fixes (none of those files or the code paths
    they exercise changed here). The 18th is this round's own new test,
    **"Escape closes only the open picker, not the delete dialog behind it"**
    — failed once, mid-run, on a `popover-open` poll timeout; reproduces
    green on its own (`-g` isolated) and green running its whole file
    (14/14) immediately after, so not chased further as `@flaky` — named
    here per the request rather than silently rerun until it disappeared.
  - **On the catalog-pool exhaustion**, since it now blocks four files
    instead of two: still not fixed at the source (out of scope — it is
    `needs.spec.ts`/`removal.spec.ts`/`batch-move.spec.ts`/
    `command-palette.spec.ts`'s own `unownedCards`-style helpers, each with
    its own `limit=60`, not this task's file). The dev branch's card
    ownership is now more heavily loaded than when this task started
    (repeated full-suite runs during both the implementation and this review
    round each permanently relocate a few more catalog cards into the
    Inbox); a proper fix wants either a much larger shared pool or a way to
    return a test's cards to "unowned" in cleanup, and is worth a dedicated
    follow-up rather than another file-local `limit` bump.

- 2026-08-09 — **Step 4 landed: the confirm dialog gets its two pickers and
  honest counts** (`P6-189`, absorbs `P6-111`, on top of `P6-188`'s relocating
  delete). `app/src/my/tree_manage.rs`'s delete dialog now shows this
  collection's own present/desired counts, a haves picker and a wants picker,
  and a child-collections line sourced from the same read as the counts.
  - **The two pickers reuse the move picker's machinery exactly as asked**:
    `DestinationList`/`DestinationRow`, `move_destinations`'s self+descendant
    exclusion (`req.subtree`, the same `forbidden` set `Move to…` uses), and
    `picker_order`. Each is a small `Popover` combobox (the sticky "Adding
    to:" picker's shape, not the move picker's full dialog — two of these
    live *inside* one dialog already). New pure functions, unit-tested apart
    from the reactive graph: `resolve_parent` (haves' default target),
    `have_destinations` (plain list minus the pinned parent row),
    `have_trigger_label`/`want_trigger_label`, `HaveChoice`/`WantChoice` and
    their `to_wire()`.
  - **Surprise: a haves picker needs *two* different "where does this land"
    answers, not one.** `resolve_parent` (Inbox when top-level, for the haves
    default) is deliberately a *different* function from
    `children_destination_label` (top level, when top-level) — the spec's own
    "Children survive… or becomes top-level" line, easy to miss when both look
    like "the deleted node's parent" at a glance. Getting this wrong would
    have had the confirm claim reparented children land in the Inbox, which
    they never do (only haves need a real collection to sit in; a child
    collection can legitimately be top-level itself). Pinned with its own
    test (`children_destination_label_differs_from_resolve_parent_at_top_level`).
  - **Surprise: a `move ||` closure that only `.clone()`s a captured
    `HashSet` still isn't `Fn`.** `DestinationList`'s `children: ChildrenFn`
    needs a closure invocable more than once; capturing `subtree: HashSet<Id>`
    by `move` and calling `subtree.clone()` inside compiled, but rustc still
    rejected it as `FnOnce` (E0525) — not chased further, worked around the
    established way: `StoredValue::new(subtree)` + `.get_value()`, the exact
    trick `RowShell` already uses for its own `forbidden` set. Two instances
    (haves, wants).
  - **The picker-open-closer sidesteps the Suspense/context trap on
    purpose, not by luck.** `move_rows`' own doc names the hazard: a
    `Provider` above a `Suspense`/async boundary does not reach a
    `use_context()` call made *inside* it (bit `TreeManage`'s `menu_target`
    once). `use_popover_open()` is therefore called in each picker's
    synchronous body — a child of `<Popover>`, never inside a `Suspend` — and
    the resulting `Option<RwSignal<bool>>` is just a captured value from there
    on, so nothing downstream needs a context lookup at all. This task's
    pickers don't even have a `Suspend` any more (next point), so the trap
    turned out to be avoidable rather than merely worked around.
  - **No `Suspend`/`Transition` needed for the pickers at all — a second
    Effect-written snapshot instead.** The first design awaited
    `CollectionTreeResource` per picker (`move_rows`'s pattern) but a
    `Popover`'s trigger has to show a label *before* the user opens anything,
    and a trigger sits outside `PopoverContent`'s own boundary — so the label
    would need the same async data the content does, from a sibling that
    can't itself await. Fix: `tree_rows`, an `Effect`-written
    `RwSignal<Vec<CollectionTreeRow>>` sitting next to the existing
    `load_failed` (same justifying comment: safe because both live behind
    `delete_open`, a client-only signal false on every server render). Both
    the trigger labels and the row lists read it as a plain reactive value —
    no nested async boundary, no `Suspend::new`, at the cost of a `Vec::new()`
    fallback while the tree hasn't resolved yet (matching `load_failed`'s own
    already-covered failure arm).
  - **The honest child-collections count needed a new, more reliable source
    per open path — not the same `forbidden` the pickers still use.**
    `DeleteReq` grew `wants: i64` and `children: i64` (immediate children
    only, not the whole subtree `subtree.len() - 1` used to report). Sourced
    per the two ways the dialog opens: the sidebar row already has
    `TreeNode.children.len()` for free (`tree.rs`'s `TreeRow`, always
    accurate — the tree loaded successfully or the row wouldn't exist to
    right-click); the header kebab now reads `collection_view.children.len()`
    (`collection.rs`'s `CollectionHeader`) instead of walking the sidebar's
    *separate* tree resource, which is exactly the read `MenuTarget::for_collection`'s
    own doc already called out as best-effort ("degrades to 'not itself' … on
    a failed tree read"). Both sources are the **same request** that already
    produces the honest `cards`/`wants` counts, so the three numbers cannot
    disagree with each other even when the sidebar's tree is stale or failed
    — closing `P6-111`'s degraded-state bug structurally rather than by
    hardening the old code path.
  - **`cards` changed meaning under the same field name.** It used to be
    "rolled-up present copies in the whole subtree" (`tree.rs` passed
    `RowShell` its `rolled_up`; `collection.rs` passed `totals.present_total()`).
    Both call sites now pass the **own**, non-rolled-up count
    (`row.present`/`totals.present`) — the field name didn't change, only
    what feeds it, so the diff is easy to misread as a no-op if you don't
    check the call sites. `MenuTarget::Row` gained `wants`/`children` fields
    alongside it, threaded through the same two call sites.
  - **A new `desired` column on the sidebar tree read** — the sidebar-opened
    delete has no `collection_view` to read a wants count from, unlike the
    header kebab, so `CollectionTreeRow` grew `desired: i64` (`#[serde(default)]`
    for wire safety) and `collection_tree()`'s query grew a second `LEFT JOIN`
    over `desires`, mirroring `present`'s existing one exactly (same "no live
    filter needed, it's LEFT JOINed from the already-filtered `collections`
    scan" reasoning). One round trip, not two.
  - **Server-fn wire shape, following the `teardown_collection` precedent
    cited in the task**: `delete_collection(id, haves_to: Option<Id>,
    haves_discard: bool, wants_to: Option<Id>)`. `wants_to` alone fully
    covers `WantDisposition` (`Some` → `To`, `None` → `Discard`, exactly
    `teardown_collection`'s `Option<Id>` shape). `HaveDisposition` needed one
    more bit since it has four variants: `haves_discard` wins over an unset
    `haves_to`, `(None, false)` is `ToParent`. **`ReturnToPrevious` is
    deliberately not exposed by this adapter or the dialog** — the spec's
    wireframe shows exactly two controls, not a third for a mode
    `teardown_collection` already covers elsewhere; the hosted route stays
    capable of it (untouched, per the task's "must not change"), so nothing
    forecloses wiring it in later.
  - **The hosted `/api/collections/{id}/delete` route is untouched** — every
    change is in the Leptos server fn (`app/src/lib.rs`) and the client. The
    "give `collection_id` a default derived from the path" / body-optional
    behavior `P6-188` built stays exactly as it was.
  - **Tests.** Unit: 255 in `app` (up from ~230), the new ones covering
    `HaveChoice`/`WantChoice` defaults and `to_wire()`, `resolve_parent`
    (found + degrades-to-`None`), `children_destination_label`'s divergence
    from `resolve_parent` at the top level, `have_destinations`' exclusion of
    both the subtree *and* the resolved parent (two cases: a real parent, and
    the Inbox-as-parent at the top level), and both trigger-label functions
    (including the no-tree-yet fallback). `shared`'s own delete-defaults and
    wire-shape tests are unaffected (the DTO didn't change). e2e: rewrote
    `collection-tree-manage.spec.ts`'s interim-copy assertions
    (`"Nested collections move up a level."`) into the new
    `data-testid`-scoped count assertions, and added two tests — honest
    per-node counts with a rolled-up-would-lie fixture (parent's own 1 card +
    1 want, child's own different card, asserting the dialog reads "1 card"/
    "1 want" not "2") and an explicit haves-picker pick relocating to a
    chosen collection instead of the default parent.
  - **e2e verification, reported honestly.** `collection-tree-manage.spec.ts`
    (12/12) and `collection-header-kebab.spec.ts` (12-13/13, see below) both
    green in clean, single-worker (`--workers=1`) runs — the task's own new
    assertions passed on every run that had uncontended fixture data. One
    pre-existing test, `collection-header-kebab.spec.ts`'s "Move to… resolves
    the subtree off the route" (unrelated to this task — it exercises the
    move picker's subtree exclusion, a code path this task did not touch),
    intermittently failed mid-suite while passing every time it ran in tight
    isolation; not chased to ground, filed below as a quarantine candidate
    rather than silently ignored. A full-suite, 9-worker `@fast` pass showed
    12 failures that a subsequent clean single-worker rerun of the same files
    did **not** reproduce, consistent with the skill's own documented
    worker-pressure flake class rather than a regression.
  - **Self-inflicted, worth recording rather than hiding: this task's own
    repeated local iteration measurably drained a shared fixture.** Delete's
    default `ToParent` **relocates** a card rather than removing it from
    ownership (correct, per this spec's whole point) — so an e2e test that
    adds a have and then deletes its collection leaves that card permanently
    "owned" in the Inbox afterward. `needs.spec.ts`'s `unownedCards` pattern
    (copied into this file for the two new tests) draws from a **finite**
    `catalog/search?q=z&limit=60` slice that only ever shrinks under repeated
    *local* runs (a single CI run per PR never hits this). Several dozen
    consecutive runs during this task's own debugging drained the front of
    that slice enough to make `removal.spec.ts` and this file's own new tests
    fail on `unownedCards`' sanity check — confirmed by widening the net
    (`limit=200`) in this file's copy, which fixed it immediately, and by a
    direct query showing 73 still-free cards past position 60. Widened this
    file's own helper; did **not** touch `needs.spec.ts`/`removal.spec.ts`
    (out of scope) — filed as a follow-up below. Not a product defect: it is
    an e2e-fixture-hygiene gap that predates this task (inherent since
    `P6-188` shipped relocating delete) and that only unusually repetitive
    local iteration exposes.

- 2026-08-09 — **Step 3 landed: delete relocates instead of destroying**
  (`P6-188`, on top of `P6-110`'s inert machinery). The hard
  `DELETE FROM collections` is gone; `delete_collection` now reads a snapshot,
  plans against it, and executes the plan in one transaction, stamping
  `deleted_at` **last**.
  - **Shape: read → plan → write.** The rules ("which children re-parent
    where, and which grain goes to which destination") live in a pure
    `app/src/backend/delete_plan.rs` — `plan_delete(&DeleteSnapshot, haves,
    wants) -> DeletePlan` — with the hosted transaction reduced to a loop with
    no decisions in it. Same split as `plan_move`/`plan_drop`, for the same
    reason: every rule here is an edge case (top-level, no history, discard, a
    surviving child as the destination) and none of them need a database to be
    right. The module imports no sqlx at all, so the boundary is structural
    rather than a convention.
  - **The Inbox refusal moved into the planner**, deliberately, so the rule has
    exactly one statement and that statement is a unit test rather than a
    dev-branch transcript. The message is unchanged
    (`Conflict("the Inbox cannot be deleted")`), and the executor's earlier
    read still answers absent / not-owned / already-hidden with the one
    `NotFound` every other path uses.
  - **Every destination is re-validated, not just the user's pick.** The
    executor runs `require_owned_collection` over `DeletePlan::destinations()`
    — the `To` target, the parent, each previous location, the Inbox — so "a
    soft-deleted collection is never a write target" holds for all four
    dispositions at once instead of only the explicit one. The case should be
    unreachable once this ships (delete re-parents children, so no live row
    keeps a hidden ancestor), which is exactly why it is a cheap assertion
    rather than a comment.
  - **Holdings leave through the same take/add/append triple `teardown` uses**,
    per board, so a delete's relocations are indistinguishable from any other
    move to undo, to history and to every count. `Discard` runs zero
    statements — the loop skips a `None` destination — which is what makes it
    reversible rather than a delete in disguise.
  - **Desires move as a merge-and-drop** (`INSERT … SELECT … ON CONFLICT ON
    CONSTRAINT desires_uniq DO UPDATE`, then delete the source rows). They have
    no ledger to ride, so `WantDisposition::To` is **not** reversible by the
    step-5 undo the way holdings are — the receipt cannot carry a handle that
    does not exist. Noted as an open item for step 5 below.
  - **Endpoint shape.** `POST /api/collections/{id}/delete` keeps its path (the
    per-collection op convention in specs/collection-api.md); the body is the
    spec's `DeleteCollectionReq`, whose two dispositions are `#[serde(default)]`
    so the pre-step-4 caller posts nothing and gets `ToParent`/`Discard`. The
    DTO also names the collection, as this spec writes it, and the route
    **refuses** a body that disagrees with the path rather than picking a winner
    — silently deleting the other collection is the worst available answer.
    The Leptos server fn still takes the id alone and now returns the receipt;
    when step 4 adds the pickers it should grow **scalar** parameters, not the
    tagged enums (the server-fn POST codec mangles those — app-ui Findings, and
    why `teardown_collection` takes an `Option<Id>`).
  - **`undo_one` redirects to the Inbox** when its write-back target is hidden
    (the ruling above), via a `live_or_inbox` helper; a missing row takes the
    same branch as a hidden one, since both mean "nowhere of its own to go
    back to". The redirect is deliberately *not* recorded as a new ledger row:
    appending one would make it "the last move" and change what ⌘K's
    `Undo last move` reverses next.
  - **Tests (no DB, so they run in CI):** thirteen planner unit tests covering
    all four `HaveDisposition`s, both `WantDisposition`s, the children-reparent
    rule at both levels, the Inbox refusal, a disposition pointing at the
    collection being deleted, a surviving child as a legal destination — and
    **the regression that matters most**, modelled against a miniature
    `owned_by_card` (sum over live collections): a card held only in the
    deleted collection is still owned at the destination under *every*
    non-`Discard` disposition, and not owned — but still present, attached to
    the hidden row — under `Discard`. Two serde tests in `shared` pin the
    defaults and the `{"mode": …}` wire shape. `owned_definition_guard` and
    `soft_delete_guard` stay green (the new `live_or_inbox` lookup carries the
    filter the latter counts).
  - **Dev-branch evidence** (Neon `dev`, the e2e user, as `app_runtime` with
    `app.user_id` set — `scoped_tx` exactly). A committed `#[ignore]`d
    integration test (`delete_live` in `hosted.rs`, the `search_live`
    precedent) builds `root → subject(deck) → child → grandchild` plus
    `source`/`elsewhere`, using two printings of cards the user owns **none**
    of; card A reaches `subject` by a real move out of `source` (so it is held
    nowhere else *and* has a live previous location), card B by an intake (so
    it has none). Inspected independently in `psql` at each phase:
    | Disposition | one node hidden | children | ledger rows | `owned_by_card` A / B |
    |---|---|---|---|---|
    | `ToParent` | subject only (5 live) | child → root, grandchild → child | 2, both ends non-NULL, subject → root | **2** / 5 |
    | `ReturnToPrevious` | subject only | same | 2 → `source` and Inbox | **2** / 5 |
    | `To { elsewhere }` | subject only | same | 2 → elsewhere (+ the wants) | **2** / 5 |
    | `Discard` | subject only | same | **0** | **0** / 4 |
    Under `Discard` the copies are still *there* (2 + 1 rows attached to the
    hidden subject) — hidden, not destroyed. The undo redirect was checked with
    a hand-hidden `source` and an unrelated older move `source → elsewhere`:
    after `undo_move`, the hidden source received **0** and the Inbox gained
    exactly the 3 copies, with the ledger row keeping both real ends and its
    `undone_at` stamp. Cleanup verified: collections/holdings/desires/moves
    back to `10 / 87 / 6 / 2698`, 0 soft-deleted, 0 scratch rows, and no `moves`
    row created in the run's window survives.
  - **Surprise worth writing down:** the dev branch already carries **2416
    `moves` rows with *both* ends NULL**, all dated 2026-07-24…27 — the
    falsified ledger this spec's Problem section describes, left by the old
    hard delete cascading `ON DELETE SET NULL` over historical rows. It is
    pre-existing damage, unrecoverable (the ids are gone), and it is the
    concrete evidence that the hole was real rather than theoretical. Nothing
    in this step adds to it: soft delete never fires the referential action.
  - **`SELECT … FOR UPDATE` on `collections` works under FORCE'd RLS** as
    `app_runtime` — checked live rather than assumed, the same caution P6-110
    used for the `FOR UPDATE` + correlated-`EXISTS` shape. The delete holds its
    subject row for the whole operation, so a concurrent second delete
    serializes behind it instead of racing the holdings snapshot.
  - **Follow-ups, filed rather than absorbed:**
    - `seed.rs`'s failure rollback is now **best-effort**: it deleted its root
      collections and relied on the cascade, and delete no longer cascades — a
      failed seed leaves the created children behind as top-level rows. It
      passes `Discard`/`Discard` so at least nothing spills into the Inbox. The
      real fix is a bottom-up sweep, which needs the seed to track more than
      roots.
    - **`card_tags` stay attached** to the hidden collection when its holdings
      move out. card-tagging's rule ("remove a card's tags when its last
      holding *and* desire leave the deck") is not applied here on purpose:
      the collection is going hidden anyway, `card_tags.collection_id` is
      `ON DELETE CASCADE` (never fired by a soft delete), and leaving them is
      what lets a restore bring the deck back intact.
    - **Step 5's undo cannot reverse `WantDisposition::To`** — desires have no
      ledger, so the receipt has no handle for them. Either the receipt grows a
      desire-relocation record, or the undo's contract says so out loud.
    - **The other e2e specs' cleanup helpers still assume the cascade.**
      `collection-tree-manage.spec.ts`'s is now subtree-aware (deepest-first);
      `collection-header-kebab`, `collection-tree-move`, `command-palette`,
      `needs`, `quick-add`, `batch-move`, `collection-view` and `removal` each
      keep their own one-liner. Any of those that deletes a collection with
      children now strands them as top-level scratch rows on the dev branch.
      A shared helper is the fix, and it is a test-hygiene task rather than
      part of this one.

- 2026-08-09 — **Review fixes on top of the above** (adversarial review of the
  P6-188 commit; the server-side write path came back clean).
  - **The confirm dialog was still lying.** It read "This permanently deletes
    {N nested collection(s) and }{M cards} inside it. This cannot be undone." —
    false three times over after this change: nothing is destroyed, the nested
    collections survive by moving up a level, and `M` was the *rolled-up*
    subtree total, whose cards a delete does not touch at all. Replaced with
    interim, number-free copy that is true for the default dispositions ("Its
    cards move up to the parent collection — your Inbox if it is top-level.
    Nested collections move up a level. No cards are deleted."). **No number
    beats a wrong number**: the honest per-node counts and the two pickers are
    `P6-189`'s, and the count *fields* are left on `DeleteReq` for it.
  - **`route_after_delete` fled too far.** It navigated away whenever the
    current route was anywhere in the deleted node's `subtree`, which was right
    when the delete cascaded and is wrong now — a descendant's page still shows
    a real collection with real cards, and ejecting the user from it is a
    worse bug than the stale page it was written to prevent. It now leaves only
    when the route *is* the deleted node. Its test asserted the old behaviour
    and now asserts the inverse.
  - **`ReturnToPrevious` could corrupt a stack, silently.** It was the one
    disposition that skipped the self-destination check, and a previous location
    *can* resolve to the collection being deleted: `teardown` has no
    `from != to` guard, so an "empty this deck into itself" writes a ledger row
    with both ends equal, and `previous_location` hands it straight back.
    Nothing downstream catches that — the same-collection guard lives in
    `apply_move`, and `delete_collection` does not call it, driving
    `holding_take` / `holding_add` / `append_move` directly instead. The write
    would therefore **succeed**: the stack taken off its own board and re-added
    on `main` in the same collection (a sideboard collapsed into the mainboard),
    plus a ledger row pointing at itself, all inside a collection hidden a
    moment later and so invisible to inspect. The planner now treats
    `prev == the subject` as no source and falls back to the Inbox, with a test.
    *(First written up here as "would 500 the delete" — wrong, and worth the
    correction: the real failure mode is quiet corruption, which is the stronger
    argument for refusing it in the planner rather than trusting the write.)*
  - **The delete endpoint had quietly become body-mandatory**, which would have
    broken every existing caller: the operation took no body at all before the
    dispositions existed, and the e2e cleanup helpers post either nothing (three
    files) or `{}` (six). Both are now valid again — `Option<Json<…>>` for the
    no-body case, `#[serde(default)]` on every field for `{}` — and
    `DeleteCollectionReq::resolve_path_id` fills an unstated `collection_id`
    from the path while still **refusing** a stated one that disagrees. This is
    the "give `collection_id` a default derived from the path" option the review
    floated and set aside as unnecessary; the nine call sites are why it was
    necessary after all.

- 2026-08-09 — **Step 2 landed: migration `0010` + the read-path filter, inert**
  (`P6-110`, after `P6-039`'s owned collapse). Nothing sets `deleted_at`, so
  every filter is currently a no-op — verified as such on the dev branch by
  setting the column by hand and putting it back (transcript below).
  - **Migration `0010_collection_soft_delete.sql`** is exactly the spec's DDL —
    `deleted_at timestamptz`, the partial `collections_user_parent_live_idx` —
    plus the **`owned_by_card` redefinition** the spec's own "the filter lands
    once, in the view" rule requires. `CREATE OR REPLACE VIEW` (not DROP +
    CREATE) so 0003's `GRANT SELECT … TO app_runtime` survives, and
    `WITH (security_invoker = true)` restated explicitly rather than assumed:
    reloptions come from the statement, and a view that silently lost the
    invoker flag would run as its RLS-exempt owner and show every user every
    user's counts. Confirmed post-apply on dev: `pg_class.reloptions` is still
    `{security_invoker=true}` and `pg_get_viewdef` carries
    `WHERE c.deleted_at IS NULL`.
  - **Applied to dev the project way** — `scripts/migrate.sh`'s path
    (`server --migrate`, sqlx embedded migrations, owner role) — so
    `_sqlx_migrations` now records version 10 and the migration stays applied.
    Applying it by hand in `psql` would have left sqlx's ledger out of step and
    broken the next `--migrate`.
  - **Filtered read sites**, beyond the ones the task named
    (`collection_tree`, `list_collections`, `require_owned_collection`,
    `inbox_id`, `needs`, `suggested_destinations`, `search` via the view):
    `card_detail`'s ownership block, `collection_view` (metadata, children, the
    `descendants` CTE in both its page query and its totals, and the totals'
    `elsewhere` half — the same board-blind arithmetic `needs` uses, so it needed
    the same treatment), `all_cards` (`wanted` + the per-location read),
    `shopping_list` (`d` + `wanted_by`), `holdings_of_oracle`,
    `set_holding_quantity`, `move_holding`, `set_holding_board`,
    `set_desire_board`, `absent_or_inbox`, the tree writes
    (`rename`/`delete`/`reparent`/`reorder`, plus `create`'s parent check and
    its `max(position)` sibling scan), and — the one that matters most —
    **`previous_location`**, which is the only read in the file whose result
    becomes a *write destination*. Unfiltered it would have sent
    `ReturnToPrevious` copies into a hidden collection, and that mode is reused
    verbatim as a delete disposition in step 3.
  - **Deliberately left unfiltered**, with the reasons in comments:
    `reparent_collection`'s ancestor walk (a cycle guard is strictly safer
    walking hidden ancestors too — filtering could cut the chain and let a cycle
    through), and every aggregate already scoped to a single `collection_id`
    that a `require_owned_collection` / `collection_view` metadata read has just
    proved live (`teardown`'s snapshot, `holding_take`/`holding_add`, the
    `present`/`want` CTEs, `assign_tag`'s in-deck check). The tree's `present`
    sub-select needs nothing either: it is LEFT JOINed *from* the filtered
    `collections` scan.
  - **Shape of the filter.** Where a query already joins `collections`, it is a
    plain `c.deleted_at IS NULL`. Where it reads `holdings`/`desires` alone, it
    is a correlated `EXISTS` built by one helper (`in_live_collection`) so the
    predicate has a single definition and a single doc comment.
  - **Tests (no DB, so they run in CI):** `soft_delete_guard` in `hosted.rs` —
    (a) every `SELECT 1 FROM collections WHERE id = $1` ownership lookup carries
    `AND deleted_at IS NULL` (the guard the spec singles out; needle assembled at
    runtime so `include_str!` can't self-match, the same trap `P6-039` hit), and
    (b) migration `0010` still contains the column, the partial index, and the
    filtered + invoker-scoped view. `owned_definition_guard` stays green.
  - **Dev-branch evidence** (as `app_runtime`, RLS FORCED, each pass inside one
    transaction opening with `set_config('app.user_id', …, true)` — i.e.
    `scoped_tx` exactly). Subject: "Depth Box", holding *Amped Raptor* ×2 (held
    **nowhere else**) and *Altar of the Goyf* ×2 (4 elsewhere), wanting *Amped
    Raptor* ×3, with a child chain Depth Shelf → Depth Drawer.
    | Read | live | `deleted_at = now()` | restored |
    |---|---|---|---|
    | `list_collections` / `collection_tree` rows | 10 (Depth Box present 4) | 9, gone | 10 |
    | tree `shopping_short` badge | 4 | 3 | 4 |
    | `require_owned_collection` | 1 row (OK) | **0 rows → NotFound** | 1 row |
    | `collection_view` metadata | 1 row | 0 rows → 404 | 1 row |
    | `owned_by_card` | Raptor 2, Altar 6 | **Raptor: no row**, Altar 4 | 2 / 6 |
    | `card_detail` ownership block | 2 rows | 0 rows | 2 rows |
    | `all_cards` (owned/wanted) | 2 / 3 | row gone entirely | 2 / 3 |
    | `shopping_list` row | desired 3, owned 2 | row gone | desired 3, owned 2 |
    | `needs`/totals `elsewhere` | Raptor 2, Altar 6 | Raptor gone, Altar 4 | 2 / 6 |
    | `suggested_destinations` | Depth Box offered | **no destinations** | offered |
    | `holdings_of_oracle` | 2 rows / 2 copies | 0 / 0 | 2 / 2 |
    | `inbox_id` | 1 | 1 | 1 |
    A second pass hid the *child* (Depth Shelf) instead: `collection_view`'s
    children 1 → 0, `descendants` 2 → 0 and `present_rollup` 4 → 0, restored on
    clearing the column. Data left exactly as found (14 collections, 0
    soft-deleted, holdings/desires/moves untouched).
  - **Surprise worth writing down:** `SELECT … FOR UPDATE` with the correlated
    `EXISTS` appended (`move_holding`) is accepted by Postgres — the lock applies
    to `holdings`, the subquery is not locked — and returns zero rows for a
    holding in a hidden collection, so the call maps to `NotFound("holding")`.
    Checked live rather than assumed, because `FOR UPDATE` rejects several other
    query shapes outright.
  - **Also fixed in passing:** `require_owned_collection`'s doc comment had
    drifted onto `ensure_inbox` (two doc blocks had merged above the wrong
    function); it is back on the function it describes, now stating the liveness
    rule too.
  - **Known interim state, by design:** because nothing re-parents children yet,
    a hand-set `deleted_at` on a parent leaves its children visible in the tree
    with a hidden parent. Step 3's re-parenting is what closes that, and it is
    unreachable in the shipped app until then.
  - **Interim-state notes for steps 3–5** (from the review, all reachable only
    once something sets the column): the `descendants` recursive CTE **cuts at a
    hidden node**, so a live grandchild under a hidden parent drops out of
    `present_rollup` while still rendering in the tree — the same
    hidden-parent gap as above, seen from the counts side, and re-parenting
    closes both at once. A hypothetically hidden Inbox would surface as an opaque
    500, since `inbox_id`'s `fetch_one` has no row to return rather than a typed
    error, so step 3 must keep the Inbox undeletable rather than rely on a nice
    message. And `create_collection`'s `max(position)` scan now ignores hidden
    siblings, so a step-5 restore can hand two live siblings the same `position`
    — harmless, because `ORDER BY position, name` breaks the tie.
