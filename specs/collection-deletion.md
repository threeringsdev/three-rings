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

- *(open — raised 2026-08-09, step 2)*
  **What should `undo_move` do when the move's other end is now hidden?**
  `undo_one` reverses a ledger row by writing straight into
  `from_collection_id` / `to_collection_id` — it is the one write path in
  `hosted.rs` that never goes through `require_owned_collection`, because the
  ids come from the ledger rather than the caller. Undoing an *old, unrelated*
  move whose source or destination has since been soft-deleted would therefore
  put copies back into a collection the user cannot see. Step 2 deliberately did
  not guess: the delete's **own** undo clears `deleted_at` first (so it is
  unaffected), and the three plausible answers for the unrelated case — refuse,
  redirect to the Inbox, or restore the collection — are a product decision, not
  an implementation detail. Needs a ruling before step 3 ships, since that is
  when `deleted_at` starts being set.

## Findings

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
