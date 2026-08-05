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

The highest-risk sites are the **four independent definitions of "owned"**
(`P6-039`):

| Site | What it is |
|---|---|
| `hosted.rs:100` | the `owned_by_card` view — `search` and `card_summary` |
| `hosted.rs:1274` | `all_cards`' inline `held` CTE |
| `hosted.rs:417` | `collection_tree`'s `shopping_short` `o` CTE |
| `hosted.rs:1458` | `shopping_list`'s `o` CTE |

They agree today with nothing enforcing that they continue to. Adding the same
filter to four places and missing one produces owned counts that silently
disagree between the catalog, `/my`, the tree and the shopping list — a bug with
no error and no failing test.

**Therefore `P6-039` is a prerequisite, not a sibling.** Collapse the four onto
one shared source first, then add the filter once.

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
