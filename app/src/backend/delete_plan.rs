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

use shared::{ApiError, ApiResult, HaveDisposition, Id, WantDisposition};

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
    /// stack's history" — the only case that falls back to the Inbox.
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
}
