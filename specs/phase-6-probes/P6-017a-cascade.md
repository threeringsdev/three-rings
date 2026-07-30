# P6-017a follow-up — what happens to a user's cards when a collection with descendants is deleted
Probed 2026-07-29 against f9639d6.

## Answer
**The cards are destroyed.** Deleting a collection destroys every holding and
every desire in that collection *and in every collection nested under it*, in the
same transaction, with no recovery path of any kind: no soft delete, no
`deleted_at`, no trash/archive, no audit table, no undo entry. Nothing re-parents
the rows — not to the parent, not to the Inbox, not anywhere. The single
`DELETE FROM collections WHERE id = $1 AND NOT is_inbox`
(`app/src/backend/hosted.rs:510`) is the *entire* delete path; every other effect
is the database's own referential action. `collections.parent_id` cascades to the
descendants, and each descendant's `holdings` / `desires` / `card_tags` /
deck-scoped `tags` cascade away with it. The `moves` ledger is the only survivor,
and it survives *falsified*: its two collection columns are `ON DELETE SET NULL`,
and NULL in that schema is not "unknown", it means "external intake" (`from`) and
"removal" (`to`) — so historical moves in and out of the deleted subtree silently
rewrite themselves into intakes and removals, and undoing one of them will not
put the copies back (`undo_one` skips a NULL end: `hosted.rs:2005-2010`). No move
row is written to record the deletion itself, so the ledger contains no trace of
the copies that were destroyed. The Inbox cannot be deleted (`AND NOT is_inbox`),
but that protects only the Inbox — holdings are per-collection rows, so a card
held **only** in the deleted subtree disappears from the user's ownership
entirely (`owned_by_card` is a pure aggregate over `holdings`,
`migrations/0003_collections.sql:89-94`). The confirm does say "This permanently
deletes … This cannot be undone", and its card count *does* cover the whole
subtree — but it counts holdings only, never the desires that die with them.

## Every FK into collections(id)

| table | column | on delete | effect on user data | file:line |
|---|---|---|---|---|
| `collections` | `parent_id` | `ON DELETE CASCADE` | **Destroyed.** Every descendant collection row is deleted, recursively — and each one re-fires every row below. | `migrations/0003_collections.sql:23` |
| `holdings` | `collection_id` (`NOT NULL`) | `ON DELETE CASCADE` | **Destroyed.** Every physical-copy row in the collection *and every descendant* is deleted. This is the user's actual card data. | `migrations/0003_collections.sql:47` |
| `desires` | `collection_id` (`NOT NULL`) | `ON DELETE CASCADE` | **Destroyed.** Every want/target row in the subtree is deleted. Not mentioned by the confirm at all. | `migrations/0003_collections.sql:61` |
| `card_tags` | `collection_id` (`NOT NULL`) | `ON DELETE CASCADE` | **Destroyed.** Every per-collection card annotation (including built-in `commander`/`companion` assignments) in the subtree is deleted. | `migrations/0006_card_tagging.sql:65` |
| `tags` | `collection_id` (nullable) | `ON DELETE CASCADE` | **Destroyed** — the whole tag row, not just the column. A deck-scoped tag dies with its deck (and cascades its own `card_tags` via `tags(id) ON DELETE CASCADE`, `0006:67`). Account-scoped tags (`collection_id IS NULL`) are untouched. | `migrations/0006_card_tagging.sql:46` |
| `moves` | `from_collection_id` (nullable) | `ON DELETE SET NULL` | **Orphaned and semantically falsified.** The row survives; the column is nulled. NULL there is documented as "external intake (+Have)", so a move *out of* the deleted collection now reads as a move out of nowhere. | `migrations/0003_collections.sql:76` |
| `moves` | `to_collection_id` (nullable) | `ON DELETE SET NULL` | **Orphaned and semantically falsified.** NULL there is documented as "removal", so a move *into* the deleted collection now reads as a card removed from the library. A move whose *both* ends were inside the subtree becomes a NULL→NULL row: an "intake that was also a removal". | `migrations/0003_collections.sql:77` |
| ~~`deck_commanders`~~ | `collection_id` | `ON DELETE CASCADE` | **Not live.** Declared at `0003:38`, but the table is dropped by `migrations/0006_card_tagging.sql:96` (commander became a built-in tag). Listed only so the FK sweep is complete. | `migrations/0003_collections.sql:38` |

Nothing else in the schema references `collections(id)`. `owned_by_card`
(`0003:89`) is a `VIEW` over `holdings`, not a table — it has no rows of its own
and simply shrinks as the holdings vanish. No `ALTER TABLE … ADD FOREIGN KEY`
exists anywhere in `migrations/`, so the inline declarations above are the
complete set. Every one of the destructive actions is an explicit
`ON DELETE CASCADE`; none of them relies on the SQL default, and nothing is
`RESTRICT`/`NO ACTION`, so **the delete is never blocked** by held cards.

## Is anything re-parented?
**No. Nothing re-parents anything.** This was tested specifically and the answer
is a flat no on all four possible mechanisms:

- **SQL triggers / functions:** none exist. `grep -riE "trigger|create (or replace )?function|plpgsql"` over `migrations/` returns nothing. The only `DO $$ … $$` blocks are the one-shot constraint-rename blocks in `migrations/0006_card_tagging.sql:22-38`, which run at migration time and have no runtime effect.
- **The server delete path:** `HostedBackend::delete_collection` (`app/src/backend/hosted.rs:508-521`) opens a transaction, runs **exactly one statement** — the `DELETE` — checks `rows_affected`, and commits. There is no `SELECT` of the subtree, no `UPDATE holdings SET collection_id = …`, no insert into `moves`. The only `UPDATE`s that touch `collections` anywhere in the file are `SET name` (rename, `:492`), `SET parent_id` (reparent, `:578`) and `SET position` (reorder, `:594`), and no `UPDATE` in the file ever writes `holdings.collection_id` or `desires.collection_id` — those columns are only ever read.
- **The API layer above it:** the Axum route (`app/src/backend/routes.rs:221-231`), the server fn (`app/src/lib.rs:758-769`) and the native backend (`app/src/backend/native.rs:294-301`, which just POSTs to the hosted API) are all pass-throughs with no pre- or post-processing.
- **The client:** `submit_delete` (`app/src/my/tree_manage.rs:553-595`) calls `crate::delete_collection(req.id)` and, on success, refetches the tree and navigates. It performs no card-moving call before or after.

So there is no "move the cards to the Inbox first" step, no "adopt the orphans
into the parent" step, and no equivalent anywhere between the button and the
database. The holdings and desires simply cease to exist.

## What the confirm discloses
`app/src/my/tree_manage.rs:729-758`, verbatim (the interpolated form):

- **Title** (`:731-735`): `format!("Delete {name}?")` — e.g. `Delete Shoebox?`. Falls back to the literal `"Delete?"` when the snapshot is missing.
- **Description** (`:737-757`): `format!("This permanently deletes {what} inside it. This cannot be undone.")`, where `what` is `format!("{descendants} nested collection{s} and ", …)` (emitted **only** when `descendants > 0`, `:742-747`) followed by `format!("{cards} card{s}", …)` (`:748-751`). So the full sentence for a subtree is e.g. `This permanently deletes 2 nested collections and 37 cards inside it. This cannot be undone.`, and for a leaf `This permanently deletes 37 cards inside it. This cannot be undone.`

**The card count covers the whole subtree** — this is correct on both entry
points:

- **Collection-header kebab:** `cards` is `i64::from(totals.present_total())` (`app/src/my/collection.rs:658`), and `present_total()` is `present + present_rollup` (`shared/src/collection.rs:331-333`), i.e. copies here *plus* copies in every strict descendant.
- **Sidebar tree row:** `cards=rolled_up` (`app/src/my/tree.rs:365`, `:382`), where `rolled_up = rows[i].present + Σ children.rolled_up` (`app/src/my/tree.rs:100`) — the same subtree sum, computed client-side.

Three caveats on what the number does *not* say:

1. **It counts holdings only, never desires.** The cascade destroys `desires` too, and no part of the copy mentions them. This is a knowingly-accepted trade-off, recorded in `specs/app-ui.md:3029-3034` ("Delete-confirm copy counts holdings, not desires … Left as-is").
2. **It counts copies, not distinct cards,** while the noun is "card" — a stack of 4 reads as "4 cards".
3. **The `descendants` half can under-report to zero** — the P6-017a defect proper. `descendants()` is `subtree.len() - 1` (`tree_manage.rs:134-136`), and `subtree` comes from `MenuTarget::for_collection`'s `forbidden`, which degrades to `{self}` when `find_node` misses the tree (`tree_manage.rs:88-93`). The *card* count is unaffected (it is passed in from the page's own totals, `tree_manage.rs:81-85`), so a stale/failed tree read yields a confirm that names the right number of cards while claiming no nested collections — and the server cascades them anyway.

**The Inbox is exempt** (`AND NOT is_inbox`, `hosted.rs:510`; the UI also withholds
the menu item), and a delete **can** therefore destroy holdings that exist nowhere
else. The exemption protects the Inbox row itself, not its contents' uniqueness:
holdings are per-collection rows keyed
`(collection_id, printing_id, finish, condition, language, board)`
(`migrations/0006_card_tagging.sql:29-30`), so a copy held only in the deleted
subtree has no other row anywhere. After the delete it is absent from
`owned_by_card` (`migrations/0003_collections.sql:89-94`), from the All-cards
total, and from the `/my` tree — the user's record that they own that card is
gone. Nothing sweeps the doomed holdings into the Inbox to save them.

## Test coverage
**None** — no test anywhere deletes a collection that holds cards and asserts what
survives.

- `end2end/tests/collection-tree-manage.spec.ts:255-284` ("Delete confirms with the cascade counts, then removes the subtree @fast") is the closest thing. It creates an **empty** parent and an **empty** child, asserts the dialog contains `"1 nested collection"` and `"cannot be undone"`, confirms, and asserts the row is gone from the tree and from `fetchTree`. No card is ever added, so the `{cards} card` clause is never asserted and nothing about holdings is checked.
- `end2end/tests/collection-tree-manage.spec.ts:286-329` asserts the confirm targets the snapshotted row rather than a later right-click. Empty collections again.
- `end2end/tests/collection-header-kebab.spec.ts:374-505` covers the three routing outcomes (walk up to parent, fall back to `/my`, stay put and drop the folder row). The last one asserts the header reads `"0 here"` afterwards — but that is an empty binder to begin with, so it proves the rollup clause disappeared, not that copies were destroyed.
- Rust unit tests (`app/src/my/tree_manage.rs:1483-1560`) cover `MenuTarget`/`DeleteReq` shape and `route_after_delete`. The `DeleteReq` fixture hard-codes `cards: 0` (`:1531`); nothing exercises the description copy.
- **Implicit reliance, not coverage:** most e2e specs create scratch `zz-e2e-…` collections, add cards to them, and delete the root in a `finally` precisely *because* the cascade takes the holdings with it (`end2end/tests/collection-view.spec.ts:26-27`, `end2end/tests/quick-add.spec.ts:26-30`, `specs/app-ui.md:3036-3043`). The whole suite depends on this behaviour and no test asserts it.
- No SQL-level tests exist (`migrations/` contains DDL only; there is no `tests/` for the database).

## Open — needs a live check
None of the above required a database. The schema and the delete path are
sufficient to establish the fate of every referencing table, because every
referential action is declared inline and no trigger or function exists to
override it.

Two claims *could* be strengthened against the live Neon `dev` branch — neither
changes the answer, and both are only worth running if the reader wants
belt-and-braces confirmation that the deployed schema matches `migrations/`
(a drift check, not a logic check):

```sql
-- 1. Confirm the deployed referential actions match the migrations verbatim.
SELECT c.conrelid::regclass AS tbl,
       a.attname            AS col,
       c.confdeltype        AS on_delete  -- 'c'=cascade 'n'=set null 'a'=no action 'r'=restrict
  FROM pg_constraint c
  JOIN unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) ON true
  JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum
 WHERE c.contype = 'f' AND c.confrelid = 'collections'::regclass
 ORDER BY tbl, col;
-- expected: collections.parent_id=c, holdings.collection_id=c,
--           desires.collection_id=c, card_tags.collection_id=c,
--           tags.collection_id=c, moves.from_collection_id=n,
--           moves.to_collection_id=n, and no deck_commanders row.

-- 2. Confirm no trigger has been added out-of-band on any of those tables.
SELECT tgrelid::regclass AS tbl, tgname
  FROM pg_trigger
 WHERE NOT tgisinternal
   AND tgrelid IN ('collections'::regclass, 'holdings'::regclass,
                   'desires'::regclass, 'card_tags'::regclass,
                   'tags'::regclass, 'moves'::regclass);
-- expected: zero rows.
```
