//! The pure planner behind `delete_collection`
//! (specs/collection-deletion.md → step 3).
//!
//! Deletion relocates rather than destroys, and *deciding what moves where* is
//! the part worth testing without a database. So it lives here, apart from the
//! write: [`plan_delete`] turns a snapshot the transaction has already read into
//! the list of writes to make, and the hosted impl merely executes that list.
//! Same split as `plan_move` / `plan_drop` in `crate::my::tree_manage`, for the
//! same reason — the rules are all edge cases and none of them need sqlx.
//!
//! This module deliberately imports nothing from sqlx: if a rule cannot be
//! expressed against [`DeleteSnapshot`], the fix is to read one more fact into
//! the snapshot, not to reach for the database from here.

use shared::{
    ApiError, ApiResult, DeleteCollectionReceipt, HaveDisposition, Id, RelocatedDesire,
    WantDisposition,
};

/// Everything the delete transaction has read about the collection, gathered
/// before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteSnapshot {
    /// The collection being deleted. Already proved **live and owned** by the
    /// read that produced this snapshot (RLS + `deleted_at IS NULL`).
    pub collection_id: Id,
    /// Its parent, `None` when it is top-level.
    pub parent_id: Option<Id>,
    /// Whether it is the user's Inbox — refused here rather than at the call
    /// site so the rule has exactly one statement and that statement is
    /// testable without a database (specs/collection-deletion.md keeps the
    /// Inbox undeletable; P6-110 recorded that a hidden Inbox would otherwise
    /// surface as an opaque 500 from `inbox_id`).
    pub is_inbox: bool,
    /// The caller's Inbox id — the destination of last resort for both
    /// `ToParent` (at top level) and `ReturnToPrevious` (no live history).
    pub inbox_id: Id,
    /// Its **live** children, in tree order. They survive: delete removes
    /// exactly one node, so each re-points at [`Self::parent_id`].
    pub children: Vec<Id>,
    /// One entry per holding stack in the collection, positionally aligned with
    /// the caller's snapshot of those stacks. Each entry is that stack's
    /// *previous location*: the most recent **live** collection the copies were
    /// moved into this one from, or `None` where the history has none.
    ///
    /// Only `ReturnToPrevious` reads it, and the P6-110-corrected reading
    /// applies: a hidden previous source is skipped over in favour of the
    /// next-most-recent live one (that is `previous_location`'s `WHERE`, ahead
    /// of its `ORDER BY`), so `None` here means "no live source anywhere in this
    /// stack's history" — the only case that falls back to the Inbox. P6-113
    /// extended that same skip to an **undone** move: a move into this
    /// collection can itself later be reversed — not this collection's own
    /// delete (its relocations move copies *out*, so they are never a
    /// candidate here), but **another** collection's delete having landed
    /// copies here and since been undone (P6-190). That reversed move is
    /// skipped exactly like a hidden source rather than treated as history.
    /// So `None` here really means "no live, un-undone source anywhere" —
    /// already-live-and-not-undone-filtered by the time it reaches the planner.
    pub holdings: Vec<Option<Id>>,
    /// Whether any desire rows are attached (they move or stay as one group —
    /// desires have no ledger and no per-row operation).
    pub has_desires: bool,
}

/// The writes a delete makes. Positional and id-shaped, so executing it is a
/// loop with no decisions left in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlan {
    /// Destination per holding stack, positionally aligned with
    /// [`DeleteSnapshot::holdings`]. `None` = leave the copies attached to the
    /// now-hidden collection — `Discard` **writes nothing**, which is the whole
    /// reason it is reversible.
    pub holding_dests: Vec<Option<Id>>,
    /// Where the desires go, `None` to leave them attached.
    pub desire_dest: Option<Id>,
    /// Children whose `parent_id` becomes [`Self::reparent_to`].
    pub reparent: Vec<Id>,
    /// The deleted node's parent; `None` makes those children top-level.
    pub reparent_to: Option<Id>,
}

impl DeletePlan {
    /// Every distinct collection this plan writes copies into, in first-write
    /// order.
    ///
    /// The caller re-validates each one with `require_owned_collection` before
    /// touching anything, which is how "a soft-deleted collection is never a
    /// write target" holds for *all four* dispositions at once rather than only
    /// for the one the user picked explicitly.
    pub fn destinations(&self) -> Vec<Id> {
        let mut out: Vec<Id> = Vec::new();
        for dest in self.holding_dests.iter().flatten().chain(&self.desire_dest) {
            if !out.contains(dest) {
                out.push(*dest);
            }
        }
        out
    }

    /// How many holding stacks actually move (the number of ledger rows the
    /// execution will write).
    pub fn moves(&self) -> usize {
        self.holding_dests.iter().flatten().count()
    }
}

/// Plan a delete: which children re-parent where, and which grain goes to which
/// destination.
///
/// Errors before any write happens:
/// - `Conflict` — the Inbox is undeletable.
/// - `Validation` — a disposition points at the collection being deleted, which
///   would "move" copies into the very row about to be hidden.
pub fn plan_delete(
    snapshot: &DeleteSnapshot,
    haves: HaveDisposition,
    wants: WantDisposition,
) -> ApiResult<DeletePlan> {
    if snapshot.is_inbox {
        return Err(ApiError::Conflict("the Inbox cannot be deleted".into()));
    }

    // `ToParent` resolves to the **nearest surviving parent**, which is always
    // exactly `parent_id`: children re-parent rather than cascade, and the
    // deleted collection's own parent is by definition not being deleted in the
    // same operation. Top-level means the Inbox.
    let to_parent = snapshot.parent_id.unwrap_or(snapshot.inbox_id);

    let holding_dests = match haves {
        HaveDisposition::ToParent => snapshot.holdings.iter().map(|_| Some(to_parent)).collect(),
        HaveDisposition::To { collection_id } => {
            check_destination(snapshot, collection_id, "cards")?;
            snapshot
                .holdings
                .iter()
                .map(|_| Some(collection_id))
                .collect()
        }
        // Per stack, not per collection: this is the one disposition whose
        // answer differs card by card.
        //
        // A previous location that *is* the collection being deleted counts as
        // no source at all. It is reachable: the ledger can hold a row whose two
        // ends are the same collection — `teardown` has no `from != to` guard,
        // so an "empty this deck into itself" writes one — and
        // `previous_location` would hand it straight back.
        //
        // **And nothing downstream would catch it.** The same-collection guard
        // lives in `apply_move`, which `delete_collection` does not call: it
        // drives `holding_take` / `holding_add` / `append_move` directly, and
        // none of those three compares the ends. So the plan would not fail — it
        // would *succeed quietly*, taking the stack off its own board and adding
        // it back on `main` in the same collection (a sideboard silently
        // collapsed into the mainboard) and appending a ledger row whose two
        // ends are the same id. Then the collection is hidden, so none of it is
        // visible to look at. Silent corruption inside a row on its way out of
        // sight is a far better reason to refuse this than a loud failure would
        // have been.
        HaveDisposition::ReturnToPrevious => snapshot
            .holdings
            .iter()
            .map(|previous| {
                Some(
                    previous
                        .filter(|p| *p != snapshot.collection_id)
                        .unwrap_or(snapshot.inbox_id),
                )
            })
            .collect(),
        // Writes nothing. The rows stay attached to the hidden collection,
        // disappear from every count because every read filters it out, and
        // come back intact when the delete is undone.
        HaveDisposition::Discard => snapshot.holdings.iter().map(|_| None).collect(),
    };

    let desire_dest = match wants {
        WantDisposition::Discard => None,
        WantDisposition::To { collection_id } => {
            check_destination(snapshot, collection_id, "wants")?;
            // No rows, no write — keeps `destinations()` honest about what this
            // plan actually touches.
            snapshot.has_desires.then_some(collection_id)
        }
    };

    Ok(DeletePlan {
        holding_dests,
        desire_dest,
        reparent: snapshot.children.clone(),
        reparent_to: snapshot.parent_id,
    })
}

/// A disposition may not point at the collection being deleted: the copies would
/// "move" into the row this operation is about to hide, which is `Discard`
/// wearing a destination's clothes and would write a ledger move whose two ends
/// are the same collection.
fn check_destination(snapshot: &DeleteSnapshot, dest: Id, what: &str) -> ApiResult<()> {
    if dest == snapshot.collection_id {
        return Err(ApiError::Validation(format!(
            "cannot send {what} to the collection being deleted"
        )));
    }
    Ok(())
}

/// One write **undo** makes, in the order [`plan_undo`] emits it — a value the
/// hosted executor walks with no decisions left in it, the same discipline
/// [`DeletePlan`] gives the delete itself
/// (specs/collection-deletion.md → step 5, "Undo").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoStep {
    /// Clear `deleted_at` — **always first**. Reversing the moves below has to
    /// target a live collection, or `undo_one`'s Inbox redirect (the
    /// maintainer ruling `undo_move` follows for an *unrelated* old move whose
    /// source has since been hidden) fires on the delete's own undo and sends
    /// the copies to the Inbox instead of back where they came from.
    Unhide,
    /// Reverse one holdings relocation (the hosted `undo_one`, exactly as
    /// `undo_moves` calls it).
    UndoMove(Id),
    /// Re-parent one child back to the restored collection.
    Reparent(Id),
    /// Reverse one relocated desire: decrement it off the merge destination,
    /// re-insert it at the source.
    RestoreDesire(RelocatedDesire),
}

/// Plan an undo from a delete's own receipt — pure, no I/O, so the ordering
/// invariant above (`Unhide` first) is a direct unit test rather than
/// something only a dev-branch transcript could show. The hosted `undo_delete`
/// executes this list step by step, so the plan *is* the execution order —
/// there is no second copy of the ordering to drift from it.
///
/// **Deliberately does not validate `receipt.reparented`.** The receipt is
/// client-held (specs/collection-deletion.md → step 5), so a caller can name
/// anything there; this function's only job is "what would each named id
/// mean, mechanically" — whether a given id is *safe* to act on is
/// [`reparent_is_safe`]'s job, checked by the executor against the database
/// facts this pure function has no access to.
pub fn plan_undo(receipt: &DeleteCollectionReceipt) -> Vec<UndoStep> {
    let mut steps = vec![UndoStep::Unhide];
    steps.extend(receipt.move_ids.iter().copied().map(UndoStep::UndoMove));
    steps.extend(receipt.reparented.iter().copied().map(UndoStep::Reparent));
    steps.extend(receipt.desires.iter().copied().map(UndoStep::RestoreDesire));
    steps
}

/// Whether reparenting `child_id` back onto `restored` (the collection undo
/// is restoring — [`UndoStep::Reparent`]) is safe to apply.
///
/// **The guard that closes a real cycle** (adversarial review, `P6-190`): a
/// client-held receipt's `reparented` list is untrusted, and it can name
/// anything, including ids that would commit a cycle if the write ran
/// unguarded. Two shapes were found, and the rule below is sized to both —
/// not just the first one found, which is exactly how the first draft of
/// this function under-covered the second.
///
/// - **Naming `restored`'s own current parent.** `{collection_id: S (hidden),
///   reparented: [R]}` where `R` is `S`'s live parent. `Unhide` leaves
///   `S.parent_id == R` untouched, and applying `Reparent(R)` unguarded would
///   set `R.parent_id = S`: a two-node cycle.
/// - **Naming `restored` itself.** `{collection_id: S, reparented: [S]}`.
///   `S`'s own current parent trivially equals `reparent_to` (it *is*
///   `reparent_to`'s child), so a guard that only compared parents would
///   wave this through and bind `UPDATE collections SET parent_id = S WHERE
///   id = S` — a one-node self-parent cycle, the exact case
///   `reparent_collection` already refuses by id equality
///   (`new_parent == Some(id)`) elsewhere in this file. The first version of
///   this guard missed it: its proof ("`child_current_parent == reparent_to`
///   proves `child` is a sibling of `restored`, never an ancestor") is true
///   for every `child_id != restored`, but a node is not its own sibling —
///   the proof silently assumed the case away instead of covering it.
///
/// Either shape, applied to any page that then walks the cycle
/// (`collection_view`/`collection_totals`'s `WITH RECURSIVE` CTEs, no
/// `CYCLE` clause, no depth cap) hangs; `assemble` (the client's tree
/// builder) silently drops the cycle's members from the sidebar and the
/// phone root list first, so the hang is the *second* symptom, reachable
/// only by hitting the page directly.
///
/// **The rule, and why each half is sufficient.** `child_id != restored`
/// rules out the self-parent shape outright — nothing past this point can
/// resurrect it. `child_current_parent == reparent_to` **for `child_id !=
/// restored`** then does prove `child_id` is *right now* a sibling of
/// `restored` — both children of the same `reparent_to` — never one of its
/// ancestors, and turning a sibling into a child cannot create a cycle (a
/// cycle needs `child_id` to already be an ancestor of the thing it's about
/// to become a child of, which a sibling structurally is not). The two
/// conditions compose: neither alone is the guard, both together are.
///
/// **The accepted widening, left alone on purpose.** At the top level
/// (`reparent_to = None`), "sibling of `restored`" reduces to "also
/// top-level", so this admits *every* top-level, non-Inbox collection, not
/// only ids that were actually `restored`'s own former children — the delete
/// that hid `restored` is the one thing that could have recorded "these
/// exact ids were its children," and that fact no longer exists to check
/// against by the time undo runs. Still acyclic (verified above) and
/// drag-repairable if it ever mis-nests something, so round 2 of this
/// review kept it rather than narrowing it further.
///
/// `NOT is_inbox` mirrors `reparent_collection`'s own protection: nesting the
/// Inbox under anything is refused there by name, and a receipt naming the
/// Inbox (reachable when it currently happens to be a live sibling of
/// `restored`) must not smuggle that past undo instead.
pub fn reparent_is_safe(
    child_id: Id,
    child_current_parent: Option<Id>,
    child_is_inbox: bool,
    restored: Id,
    reparent_to: Option<Id>,
) -> bool {
    child_id != restored && !child_is_inbox && child_current_parent == reparent_to
}

/// Reject a receipt whose relocated-desire quantities fall outside what the
/// `desires` table itself allows (`CHECK (quantity > 0)`,
/// migrations/0003_collections.sql) — checked before any write, not
/// discovered as a constraint violation partway through undo.
///
/// A client-held receipt is untrusted input (adversarial review, `P6-190`):
/// a zero-or-negative quantity would make `desire_take_clamp` *increment* the
/// merge destination instead of decrementing it (its `qty <= want` branch
/// inverts sign-wise) and would still write that same non-positive quantity
/// into the re-inserted source row, so validating here — pure, before the
/// transaction even opens — is what keeps a crafted receipt from writing
/// anything at all rather than writing something wrong.
pub fn validate_receipt(receipt: &DeleteCollectionReceipt) -> ApiResult<()> {
    for d in &receipt.desires {
        if d.quantity < 1 {
            return Err(ApiError::Validation(format!(
                "a relocated desire's quantity must be positive (the `desires` \
                 table requires it), got {}",
                d.quantity
            )));
        }
    }
    Ok(())
}

/// **Restore**'s parent-fallback rule (specs/collection-deletion.md → step 5):
/// re-attach to the original parent if it is still live, otherwise top level.
/// Pure — the "still live" fact is the one thing the caller has to read from
/// the database (`require_owned_collection`); everything past that is this
/// one match, which is what makes the rule itself a unit test rather than
/// only a dev-branch transcript.
///
/// Deliberately **not** an error path, unlike [`UndoStep::Unhide`]'s own
/// dead-parent refusal above: restore is the later, weaker recovery — a dead
/// parent by the time it's used is the expected shape of things, not a
/// surprise to refuse.
pub fn restore_parent(parent_id: Option<Id>, parent_is_live: bool) -> Option<Id> {
    match parent_id {
        Some(pid) if parent_is_live => Some(pid),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn id(n: u128) -> Id {
        Id::from_u128(n)
    }

    const INBOX: u128 = 1;
    const PARENT: u128 = 2;
    const SUBJECT: u128 = 3;
    const CHILD_A: u128 = 4;
    const CHILD_B: u128 = 5;
    const ELSEWHERE: u128 = 6;
    const OLD_HOME: u128 = 7;
    const GRANDPARENT: u128 = 8;

    /// A deck under a parent, two children, two holding stacks (the first has a
    /// live previous location, the second has none) and desires attached.
    fn nested() -> DeleteSnapshot {
        DeleteSnapshot {
            collection_id: id(SUBJECT),
            parent_id: Some(id(PARENT)),
            is_inbox: false,
            inbox_id: id(INBOX),
            children: vec![id(CHILD_A), id(CHILD_B)],
            holdings: vec![Some(id(OLD_HOME)), None],
            has_desires: true,
        }
    }

    fn top_level() -> DeleteSnapshot {
        DeleteSnapshot {
            parent_id: None,
            ..nested()
        }
    }

    #[test]
    fn to_parent_is_the_default_and_sends_every_stack_to_the_parent() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::default(),
            WantDisposition::default(),
        )
        .expect("plans");
        assert_eq!(plan.holding_dests, vec![Some(id(PARENT)); 2]);
        assert_eq!(plan.desire_dest, None, "wants default to Discard");
        assert_eq!(plan.destinations(), vec![id(PARENT)]);
    }

    /// The spec's own unit case: `ToParent` on a top-level collection has no
    /// parent to resolve to, and the Inbox is where physical copies land.
    #[test]
    fn to_parent_falls_back_to_the_inbox_at_top_level() {
        let plan = plan_delete(
            &top_level(),
            HaveDisposition::ToParent,
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(plan.holding_dests, vec![Some(id(INBOX)); 2]);
        assert_eq!(plan.reparent_to, None, "its children become top-level");
    }

    #[test]
    fn to_a_named_collection_sends_every_stack_there() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::To {
                collection_id: id(ELSEWHERE),
            },
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(plan.holding_dests, vec![Some(id(ELSEWHERE)); 2]);
    }

    /// Per stack, and the Inbox only where a stack has **no live** previous
    /// source at all — P6-110's correction: a hidden source is skipped by
    /// `previous_location` in favour of the next live one, so a `None` here
    /// really does mean "nowhere to go back to".
    #[test]
    fn return_to_previous_answers_per_stack_with_an_inbox_fallback() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::ReturnToPrevious,
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(
            plan.holding_dests,
            vec![Some(id(OLD_HOME)), Some(id(INBOX))]
        );
        assert_eq!(plan.destinations(), vec![id(OLD_HOME), id(INBOX)]);
    }

    /// A previous location pointing at the collection being deleted is treated
    /// as no source at all — the Inbox, not a move into the row that is about
    /// to be hidden.
    ///
    /// Not hypothetical: `moves` can hold a row whose two ends are the same
    /// collection (`teardown` writes one for an "empty into itself", having no
    /// `from != to` guard of its own), and `previous_location` would return it
    /// happily.
    ///
    /// **The failure it prevents is silent, not loud.** `apply_move` does reject
    /// a same-collection move — but `delete_collection` never calls it, driving
    /// `holding_take` / `holding_add` / `append_move` directly instead, and none
    /// of those checks the ends. The write would go through: the stack taken off
    /// its own board and re-added on `main` in the same collection (a sideboard
    /// quietly collapsed into the mainboard), plus a ledger row pointing at
    /// itself — all inside a collection that is hidden a moment later. Which is
    /// why the planner refuses it rather than trusting the write path.
    #[test]
    fn return_to_previous_ignores_a_source_that_is_the_collection_itself() {
        let snapshot = DeleteSnapshot {
            holdings: vec![Some(id(SUBJECT)), Some(id(OLD_HOME))],
            ..nested()
        };
        let plan = plan_delete(
            &snapshot,
            HaveDisposition::ReturnToPrevious,
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(
            plan.holding_dests,
            vec![Some(id(INBOX)), Some(id(OLD_HOME))],
            "the self-referencing stack falls back to the Inbox; the other is unaffected"
        );
        assert!(
            !plan.destinations().contains(&id(SUBJECT)),
            "no plan may ever name the collection being deleted"
        );
    }

    /// The load-bearing property of `Discard`: **no writes**. Not "writes that
    /// delete rows" — none at all, which is what makes the operation reversible
    /// and keeps the ledger free of destruction it cannot undo.
    #[test]
    fn discard_writes_nothing_but_still_re_parents_the_children() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::Discard,
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(plan.holding_dests, vec![None, None]);
        assert_eq!(plan.moves(), 0);
        assert_eq!(plan.desire_dest, None);
        assert!(plan.destinations().is_empty());
        // Children are not part of the disposition: they survive either way.
        assert_eq!(plan.reparent, vec![id(CHILD_A), id(CHILD_B)]);
        assert_eq!(plan.reparent_to, Some(id(PARENT)));
    }

    #[test]
    fn wants_move_only_when_asked_and_only_when_there_are_any() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::Discard,
            WantDisposition::To {
                collection_id: id(ELSEWHERE),
            },
        )
        .expect("plans");
        assert_eq!(plan.desire_dest, Some(id(ELSEWHERE)));
        assert_eq!(plan.destinations(), vec![id(ELSEWHERE)]);

        let empty = DeleteSnapshot {
            has_desires: false,
            ..nested()
        };
        let plan = plan_delete(
            &empty,
            HaveDisposition::Discard,
            WantDisposition::To {
                collection_id: id(ELSEWHERE),
            },
        )
        .expect("plans");
        assert_eq!(plan.desire_dest, None, "nothing to move, nothing to write");
    }

    /// Delete removes **exactly one node**; a child re-parents to the deleted
    /// collection's parent, or becomes top-level. Deleting a folder means
    /// "un-group these", not "destroy these".
    #[test]
    fn children_re_parent_to_the_deleted_nodes_parent() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::ToParent,
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(plan.reparent, vec![id(CHILD_A), id(CHILD_B)]);
        assert_eq!(plan.reparent_to, Some(id(PARENT)));

        let plan = plan_delete(
            &top_level(),
            HaveDisposition::ToParent,
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(plan.reparent, vec![id(CHILD_A), id(CHILD_B)]);
        assert_eq!(plan.reparent_to, None);
    }

    #[test]
    fn the_inbox_is_refused_before_anything_is_planned() {
        let inbox = DeleteSnapshot {
            collection_id: id(INBOX),
            is_inbox: true,
            ..nested()
        };
        assert_eq!(
            plan_delete(&inbox, HaveDisposition::ToParent, WantDisposition::Discard),
            Err(ApiError::Conflict("the Inbox cannot be deleted".into()))
        );
        // …whatever the dispositions say.
        assert!(matches!(
            plan_delete(
                &inbox,
                HaveDisposition::Discard,
                WantDisposition::To {
                    collection_id: id(ELSEWHERE)
                }
            ),
            Err(ApiError::Conflict(_))
        ));
    }

    #[test]
    fn a_disposition_cannot_point_at_the_collection_being_deleted() {
        let s = nested();
        assert!(matches!(
            plan_delete(
                &s,
                HaveDisposition::To {
                    collection_id: id(SUBJECT)
                },
                WantDisposition::Discard
            ),
            Err(ApiError::Validation(_))
        ));
        assert!(matches!(
            plan_delete(
                &s,
                HaveDisposition::Discard,
                WantDisposition::To {
                    collection_id: id(SUBJECT)
                }
            ),
            Err(ApiError::Validation(_))
        ));
    }

    /// A surviving **child** is a legal destination: it re-parents to the
    /// deleted node's parent, so it is still live and still reachable
    /// afterwards. Worth pinning, because "descendant" reads like "goes away
    /// too" and here it does not.
    #[test]
    fn a_surviving_child_is_a_legal_destination() {
        let plan = plan_delete(
            &nested(),
            HaveDisposition::To {
                collection_id: id(CHILD_A),
            },
            WantDisposition::Discard,
        )
        .expect("plans");
        assert_eq!(plan.holding_dests, vec![Some(id(CHILD_A)); 2]);
    }

    // --- the regression that matters most ---------------------------------
    //
    // specs/collection-deletion.md: "a card held **only** in the deleted
    // collection must still be `owned` afterwards, at the destination — and must
    // **not** be owned when `Discard`ed. That single assertion covers the four
    // 'owned' definitions at once."
    //
    // Modelled here at the planner's own level: apply the plan to a tiny
    // holdings map, then compute `owned` the way the `owned_by_card` view does
    // (sum over holdings in **live** collections — the deleted one is hidden, so
    // it contributes nothing).

    /// `(collection, card) -> copies`, the world the plan acts on.
    type Holdings = HashMap<(Id, &'static str), i32>;

    /// The `owned_by_card` view in miniature: sum a card's copies across the
    /// collections that are still visible.
    fn owned(holdings: &Holdings, hidden: Id, card: &str) -> i32 {
        holdings
            .iter()
            .filter(|((collection, name), _)| *collection != hidden && *name == card)
            .map(|(_, qty)| qty)
            .sum()
    }

    /// Execute a plan against `holdings`, one stack at a time — exactly what the
    /// hosted transaction does (`holding_take` then `holding_add`), minus SQL.
    fn apply(plan: &DeletePlan, holdings: &mut Holdings, from: Id, stacks: &[(&'static str, i32)]) {
        for (stack, dest) in stacks.iter().zip(&plan.holding_dests) {
            let Some(dest) = *dest else { continue };
            let (card, qty) = *stack;
            *holdings.entry((from, card)).or_default() -= qty;
            *holdings.entry((dest, card)).or_default() += qty;
        }
    }

    #[test]
    fn a_card_held_only_here_stays_owned_under_every_non_discard_disposition() {
        // Two copies of "Amped Raptor", held nowhere else in the world.
        let stacks: &[(&str, i32)] = &[("raptor", 2)];
        let snapshot = DeleteSnapshot {
            holdings: vec![Some(id(OLD_HOME))],
            ..nested()
        };

        for haves in [
            HaveDisposition::ToParent,
            HaveDisposition::To {
                collection_id: id(ELSEWHERE),
            },
            HaveDisposition::ReturnToPrevious,
        ] {
            let mut world: Holdings = HashMap::new();
            world.insert((id(SUBJECT), "raptor"), 2);
            assert_eq!(
                owned(&world, id(SUBJECT), "raptor"),
                0,
                "precondition: while it is held only in the collection about to \
                 be hidden, hiding it alone would lose the card"
            );

            let plan = plan_delete(&snapshot, haves, WantDisposition::Discard).expect("plans");
            apply(&plan, &mut world, id(SUBJECT), stacks);

            assert_eq!(
                owned(&world, id(SUBJECT), "raptor"),
                2,
                "{haves:?} must leave the copies owned, at the destination"
            );
            assert_eq!(plan.moves(), 1, "{haves:?} writes one real ledger move");
        }
    }

    #[test]
    fn a_discarded_card_held_only_here_is_no_longer_owned() {
        let stacks: &[(&str, i32)] = &[("raptor", 2)];
        let snapshot = DeleteSnapshot {
            holdings: vec![Some(id(OLD_HOME))],
            ..nested()
        };
        let mut world: Holdings = HashMap::new();
        world.insert((id(SUBJECT), "raptor"), 2);

        let plan = plan_delete(
            &snapshot,
            HaveDisposition::Discard,
            WantDisposition::Discard,
        )
        .expect("plans");
        apply(&plan, &mut world, id(SUBJECT), stacks);

        assert_eq!(owned(&world, id(SUBJECT), "raptor"), 0);
        // …and the copies are still *there*, attached to the hidden collection,
        // which is what makes them come back on undo rather than being gone.
        assert_eq!(world.get(&(id(SUBJECT), "raptor")), Some(&2));
    }

    /// The other half of the same rule: a card also held elsewhere keeps only
    /// its other copies when discarded, rather than dropping to zero.
    #[test]
    fn discarding_does_not_disturb_copies_held_elsewhere() {
        let stacks: &[(&str, i32)] = &[("altar", 2)];
        let snapshot = DeleteSnapshot {
            holdings: vec![None],
            ..nested()
        };
        let mut world: Holdings = HashMap::new();
        world.insert((id(SUBJECT), "altar"), 2);
        world.insert((id(ELSEWHERE), "altar"), 4);

        let plan = plan_delete(
            &snapshot,
            HaveDisposition::Discard,
            WantDisposition::Discard,
        )
        .expect("plans");
        apply(&plan, &mut world, id(SUBJECT), stacks);
        assert_eq!(owned(&world, id(SUBJECT), "altar"), 4);

        let mut world: Holdings = HashMap::new();
        world.insert((id(SUBJECT), "altar"), 2);
        world.insert((id(ELSEWHERE), "altar"), 4);
        let plan = plan_delete(
            &snapshot,
            HaveDisposition::ToParent,
            WantDisposition::Discard,
        )
        .expect("plans");
        apply(&plan, &mut world, id(SUBJECT), stacks);
        assert_eq!(
            owned(&world, id(SUBJECT), "altar"),
            6,
            "relocated, not lost"
        );
    }

    // --- undo (step 5) -----------------------------------------------------

    fn receipt(moves: &[u128], reparented: &[u128], desires: &[u128]) -> DeleteCollectionReceipt {
        DeleteCollectionReceipt {
            collection_id: id(SUBJECT),
            move_ids: moves.iter().map(|n| id(*n)).collect(),
            reparented: reparented.iter().map(|n| id(*n)).collect(),
            desires: desires
                .iter()
                .map(|n| RelocatedDesire {
                    to_collection_id: id(ELSEWHERE),
                    oracle_id: id(*n),
                    printing_id: None,
                    board: shared::Board::Main,
                    quantity: 1,
                })
                .collect(),
        }
    }

    /// The one ordering property the spec pins by name: **un-hide first,
    /// always** — including when there is nothing else to undo at all (an
    /// all-`Discard` delete's receipt is empty apart from the collection id).
    #[test]
    fn undo_un_hides_before_anything_else_even_with_an_empty_receipt() {
        let steps = plan_undo(&receipt(&[], &[], &[]));
        assert_eq!(steps, vec![UndoStep::Unhide]);
    }

    /// With every kind of handle present, `Unhide` still leads, and every
    /// handle in the receipt turns into exactly one step — none dropped, none
    /// duplicated.
    #[test]
    fn undo_un_hides_before_reversing_moves_reparents_or_desires() {
        let steps = plan_undo(&receipt(&[10, 11], &[12], &[13]));
        assert_eq!(steps[0], UndoStep::Unhide);
        assert_eq!(
            steps[1..],
            vec![
                UndoStep::UndoMove(id(10)),
                UndoStep::UndoMove(id(11)),
                UndoStep::Reparent(id(12)),
                UndoStep::RestoreDesire(RelocatedDesire {
                    to_collection_id: id(ELSEWHERE),
                    oracle_id: id(13),
                    printing_id: None,
                    board: shared::Board::Main,
                    quantity: 1,
                }),
            ]
        );
    }

    /// Every `move_id`/child id/desire in the receipt survives into the plan —
    /// nothing here silently thins the list the receipt promised the toast it
    /// would reverse.
    #[test]
    fn undo_carries_every_handle_the_receipt_names() {
        let r = receipt(&[1, 2, 3], &[4, 5], &[6, 7]);
        let steps = plan_undo(&r);
        let moves = steps
            .iter()
            .filter(|s| matches!(s, UndoStep::UndoMove(_)))
            .count();
        let reparents = steps
            .iter()
            .filter(|s| matches!(s, UndoStep::Reparent(_)))
            .count();
        let desires = steps
            .iter()
            .filter(|s| matches!(s, UndoStep::RestoreDesire(_)))
            .count();
        assert_eq!((moves, reparents, desires), (3, 2, 2));
        assert_eq!(
            steps.len(),
            1 + 3 + 2 + 2,
            "one Unhide plus one step per handle"
        );
    }

    // --- the reparent cycle guard (adversarial review, P6-190, rounds 1&2) --

    /// **Round 1's crafted receipt.** `S` (`SUBJECT`) is hidden, its live
    /// parent is `R` (`PARENT`), and the receipt claims `reparented: [R]` —
    /// asking undo to make `S`'s own parent a child of `S`. `plan_undo`
    /// mechanically turns that into `UndoStep::Reparent(R)` (it does not
    /// validate — see its own doc), but `reparent_is_safe` — fed `R`'s
    /// *real* current parent (its grandparent `G`, not `S`) — refuses it,
    /// because `R`'s current parent is `G`, not `reparent_to` (`R` itself
    /// would have to be its own parent for this to pass, which nothing in
    /// this codebase ever allows). This is the end-to-end proof the
    /// two-node cycle is impossible: the step the plan would hand the
    /// executor is exactly the dangerous one, and the guard rejects it
    /// using only facts a real database read would supply.
    #[test]
    fn a_crafted_receipt_naming_its_own_parent_is_refused_not_applied() {
        let crafted = receipt(&[], &[PARENT], &[]);
        let steps = plan_undo(&crafted);
        assert_eq!(
            steps,
            vec![UndoStep::Unhide, UndoStep::Reparent(id(PARENT))],
            "the plan itself does not filter anything — that is the guard's job"
        );

        // R's (PARENT's) real current parent is G (GRANDPARENT), not S — so it
        // fails the "currently a sibling of S" test that makes the reparent
        // safe. `restored` is S, `reparent_to` is `Some(PARENT)` — S's own
        // current parent (what `Unhide` reads and the legitimate case checks
        // against).
        assert!(!reparent_is_safe(
            id(PARENT),
            Some(id(GRANDPARENT)),
            false,
            id(SUBJECT),
            Some(id(PARENT)),
        ));
    }

    /// **Round 2's crafted receipt — the one round 1's guard missed.** `S`
    /// (`SUBJECT`, nested under live parent `R`) is hidden, and the receipt
    /// claims `reparented: [S]` — naming itself. `S`'s own current parent
    /// trivially equals `reparent_to` (it *is* `reparent_to`'s child), so a
    /// guard that only compared parents would wave this through and bind
    /// `UPDATE collections SET parent_id = S WHERE id = S`: a one-node
    /// self-parent cycle. `reparent_is_safe` now takes `restored` explicitly
    /// and refuses by id equality before the parent comparison ever runs.
    #[test]
    fn a_crafted_receipt_naming_itself_is_refused_not_applied_nested() {
        let crafted = receipt(&[], &[SUBJECT], &[]);
        let steps = plan_undo(&crafted);
        assert_eq!(
            steps,
            vec![UndoStep::Unhide, UndoStep::Reparent(id(SUBJECT))],
            "the plan itself does not filter anything — that is the guard's job"
        );

        // S's own current parent (PARENT) genuinely equals `reparent_to`
        // (also PARENT) — the parent-match half would pass on its own,
        // which is exactly why the id-equality half has to run too.
        assert!(!reparent_is_safe(
            id(SUBJECT),
            Some(id(PARENT)),
            false,
            id(SUBJECT),
            Some(id(PARENT)),
        ));
    }

    /// The same self-reference attack, top-level: `S` has no parent
    /// (`reparent_to = None`), and its own current parent (`None`) still
    /// trivially equals `reparent_to`. Pinned separately from the nested
    /// case because `reparent_to = None` is also the shape of the accepted
    /// widening (see the function's own doc) — this proves the id-equality
    /// check refuses the self-reference regardless, rather than the
    /// widening accidentally re-admitting it.
    #[test]
    fn a_crafted_receipt_naming_itself_is_refused_not_applied_top_level() {
        let crafted = DeleteCollectionReceipt {
            collection_id: id(SUBJECT),
            move_ids: vec![],
            reparented: vec![id(SUBJECT)],
            desires: vec![],
        };
        let steps = plan_undo(&crafted);
        assert_eq!(
            steps,
            vec![UndoStep::Unhide, UndoStep::Reparent(id(SUBJECT))],
        );

        assert!(!reparent_is_safe(
            id(SUBJECT),
            None,
            false,
            id(SUBJECT),
            None,
        ));
    }

    /// The legitimate case the guard must still allow: a real child of the
    /// deleted collection, sitting exactly where the delete's own reparent
    /// step left it (a sibling of the restored collection, both under
    /// `reparent_to`) — and critically, a *different* id from `restored`.
    #[test]
    fn a_real_former_child_still_sitting_where_the_delete_left_it_is_safe() {
        assert!(reparent_is_safe(
            id(CHILD_A),
            Some(id(PARENT)),
            false,
            id(SUBJECT),
            Some(id(PARENT)),
        ));
        // Top-level case: the child, and the restored collection, both have
        // no parent (`reparent_to = None`) — the accepted widening's own
        // shape, still safe because `child_id != restored`.
        assert!(reparent_is_safe(
            id(CHILD_A),
            None,
            false,
            id(SUBJECT),
            None
        ));
    }

    /// The Inbox is refused by name, mirroring `reparent_collection`'s own
    /// protection — even when it currently happens to be a live sibling of
    /// the collection being restored (so the parent-match half of the check
    /// would otherwise pass).
    #[test]
    fn the_inbox_is_never_a_safe_reparent_target_even_as_a_sibling() {
        assert!(!reparent_is_safe(
            id(INBOX),
            Some(id(PARENT)),
            true,
            id(SUBJECT),
            Some(id(PARENT)),
        ));
    }

    /// An id that is neither a sibling of the restored collection, the
    /// restored collection itself, nor the Inbox — some unrelated node the
    /// receipt names — is refused too.
    #[test]
    fn an_unrelated_id_is_not_a_safe_reparent_target() {
        assert!(!reparent_is_safe(
            id(PARENT),
            Some(id(GRANDPARENT)),
            false,
            id(SUBJECT),
            Some(id(PARENT)),
        ));
        assert!(!reparent_is_safe(
            id(CHILD_A),
            None,
            false,
            id(SUBJECT),
            Some(id(PARENT)),
        ));
        assert!(!reparent_is_safe(
            id(CHILD_A),
            Some(id(PARENT)),
            false,
            id(SUBJECT),
            None,
        ));
    }

    // --- receipt validation (adversarial review, P6-190) --------------------

    fn desire(qty: i32) -> RelocatedDesire {
        RelocatedDesire {
            to_collection_id: id(ELSEWHERE),
            oracle_id: id(OLD_HOME),
            printing_id: None,
            board: shared::Board::Main,
            quantity: qty,
        }
    }

    #[test]
    fn a_positive_desire_quantity_validates() {
        let r = DeleteCollectionReceipt {
            collection_id: id(SUBJECT),
            move_ids: vec![],
            reparented: vec![],
            desires: vec![desire(1), desire(9999)],
        };
        assert!(validate_receipt(&r).is_ok());
    }

    /// Zero and negative quantities are refused before any write — the
    /// `desires` table's own `CHECK (quantity > 0)` mirrored here, and the
    /// reason it matters: `desire_take_clamp`'s `qty <= want` branch inverts
    /// sign-wise on a non-positive `want`, so a negative quantity would
    /// *increment* the merge destination instead of decrementing it.
    #[test]
    fn a_non_positive_desire_quantity_is_refused() {
        for bad in [0, -1, -100] {
            let r = DeleteCollectionReceipt {
                collection_id: id(SUBJECT),
                move_ids: vec![],
                reparented: vec![],
                desires: vec![desire(bad)],
            };
            assert!(
                matches!(validate_receipt(&r), Err(ApiError::Validation(_))),
                "quantity {bad} should be refused"
            );
        }
    }

    // --- restore (step 5) ---------------------------------------------------

    /// The spec's own two cases: a live parent wins; a dead one falls back to
    /// top level rather than refusing (restore is the weaker path — unlike
    /// undo, a gone parent is not treated as an error here).
    #[test]
    fn restore_reattaches_to_a_live_parent_but_falls_back_to_top_level_off_a_dead_one() {
        assert_eq!(restore_parent(Some(id(PARENT)), true), Some(id(PARENT)));
        assert_eq!(restore_parent(Some(id(PARENT)), false), None);
    }

    /// A collection that was already top-level when it was deleted has no
    /// parent to check liveness for at all — it stays top-level regardless of
    /// the (irrelevant) liveness flag.
    #[test]
    fn restore_of_a_top_level_collection_has_no_parent_to_lose() {
        assert_eq!(restore_parent(None, true), None);
        assert_eq!(restore_parent(None, false), None);
    }
}
