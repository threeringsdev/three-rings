//! The pure planner behind `pull_needs` (specs/collection-api.md → Pull /
//! Pull-all).
//!
//! Same split as [`super::delete_plan`], for the same reason: the write
//! transaction reads a snapshot, hands it to a pure function, and executes
//! whatever comes back with no decisions left. Before P6-120 this
//! composition lived in `lib.rs`'s server fn, spread across three
//! independently-committed reads/writes (`needs`, `holdings_of_oracle`,
//! `move_batch`) — a plan built from the first two could be stale by the
//! time the third one wrote, the same check-then-act window `move_holding`
//! was built to close for the single-row path.
//!
//! **What one transaction alone would *not* have fixed, and what closes it.**
//! [`super::hosted`] does not just move all three reads/writes inside one
//! transaction — order matters too. It locks every source stack a plan might
//! draw from (`FOR UPDATE`, bulk per oracle id) *before* reading the
//! destination's fresh gap, not after. That ordering is what guarantees the
//! gap read is never older than the locks: two overlapping pulls that share
//! an oracle id contend for the same locked rows, so the second one's own gap
//! read cannot run until it has acquired those locks, which cannot happen
//! until the first pull has committed. Reading the gap *before* locking (read
//! first, lock second — the natural-looking order, and this method's own
//! first draft) would still run inside the same one transaction and would
//! still be wrong: both overlapping pulls would plan off the identical stale
//! gap and, once both committed, overshoot the destination's `desired`. So
//! the guarantee [`plan_pull_needs`] gets to rely on is narrower than "the
//! plan and the write are looking at the same rows" — it is "every row a
//! write might touch was locked before the gap that sized the plan was read."
//!
//! **The allocation arithmetic itself is not re-implemented here.**
//! [`crate::my::needs::allocate`]/`gap_of`/`dedupe`/`plan_pull` are reused
//! directly — that module's own doc is explicit that the client's pick list
//! and the server's write must run "the *same* function over its *own* fresh
//! needs() read," and a second copy of that arithmetic in this file would be
//! exactly the drift that sentence forbids. What moved here is only the
//! *orchestration* around those functions: classifying each line
//! (`AlreadyThere` / `NoLongerNeeded` / `NoCopies` / pulled), which used to
//! live inline in the server fn.

use std::collections::HashMap;

use shared::{HoldingLine, Id, NeedRow};

use crate::my::move_selection::{MoveSource, SkipReason, Skipped};
use crate::my::needs::{allocate, dedupe, gap_of, plan_pull, PullItem, Pulled};

/// Everything the pull transaction has read before planning: the
/// destination's fresh needs (desired vs. present-here vs. elsewhere, the
/// same shape `needs()` returns) and, per distinct oracle any item asks
/// about, **every** holdings stack of that card the caller owns — the same
/// breadth `holdings_of_oracle` reads, gathered under `FOR UPDATE` so the
/// plan below is built from exactly the rows the write is about to touch.
///
/// Keyed by oracle id rather than by `(oracle, from_collection)`, matching
/// `holdings_of_oracle`'s own shape: [`plan_pull`] filters to the named
/// source itself, and two items pulling the same card from different
/// collections share one entry, same as the pre-P6-120 composition's
/// `owned: HashMap<Id, Vec<HoldingLine>>` cache.
#[derive(Debug, Clone, Default)]
pub struct PullSnapshot {
    pub needs: Vec<NeedRow>,
    pub holdings: HashMap<Id, Vec<HoldingLine>>,
}

/// One physical write the plan calls for: take `quantity` copies of
/// `source`'s exact stack (the grain and board actually found, never a
/// restated default) and land them on the destination's mainboard — a pull
/// never assigns a board, see [`crate::my::needs`]'s own doc on `to_board`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullWrite {
    pub source: MoveSource,
    pub quantity: i32,
}

/// The planned pull: the writes to make, and the two vectors
/// [`shared`]-wire `PullOutcome` carries, computed here rather than
/// assembled by the caller line by line.
#[derive(Debug, Clone, Default)]
pub struct PullPlan {
    pub writes: Vec<PullWrite>,
    pub pulled: Vec<Pulled>,
    pub skipped: Vec<Skipped>,
}

/// The distinct oracle ids `items` asks about, sorted — the set the hosted
/// impl reads (and locks) holdings for, in an order it controls rather than
/// the caller's. Locking in caller-supplied order is exactly the bug filed
/// (and deliberately left unfixed) against `move_batch` as P6-114: two
/// concurrent calls whose item lists overlap but arrive in opposite order can
/// each hold one row the other wants and deadlock. Sorting here costs
/// nothing extra — this transaction has to enumerate the ids anyway — and
/// closes that door for `pull_needs` specifically, without attempting the
/// general cross-operation ordering P6-114 itself declined to build.
pub fn oracle_ids_of(items: &[PullItem]) -> Vec<Id> {
    let mut ids: Vec<Id> = items.iter().map(|i| i.oracle_id).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Plan a pull: the same allocation [`crate::my::needs::pick_list`] shows,
/// applied to `items` over a snapshot the caller already holds locks for.
///
/// Ported unchanged from `pull_needs`'s pre-P6-120 body: `items` is deduped
/// first (a repeated line must not multiply a move — see
/// [`crate::my::needs::dedupe`]'s own doc); a line naming the destination as
/// its own source is [`SkipReason::AlreadyThere`]; a line whose `(oracle,
/// from)` pair no longer appears in the fresh allocation is
/// [`SkipReason::NoLongerNeeded`]; a line that survives both but finds no
/// copies at the locked source is [`SkipReason::NoCopies`] — reachable now
/// exactly when it could not be reached before: the snapshot's `holdings` for
/// that source held less than `want` (or none at all), because another
/// operation drained it between the pick list's own snapshot and this
/// transaction's lock.
pub fn plan_pull_needs(
    to_collection_id: Id,
    snapshot: &PullSnapshot,
    items: Vec<PullItem>,
) -> PullPlan {
    let mut planned: HashMap<(Id, Id), i32> = HashMap::new();
    for row in &snapshot.needs {
        for (from, copies) in allocate(gap_of(row), &row.locations) {
            planned.insert((row.oracle_id, from), copies);
        }
    }

    let empty: Vec<HoldingLine> = Vec::new();
    let mut plan = PullPlan::default();
    for item in dedupe(items) {
        let token = item.token();
        if item.from_collection_id == to_collection_id {
            plan.skipped.push(Skipped {
                token,
                reason: SkipReason::AlreadyThere,
            });
            continue;
        }
        let Some(&want) = planned.get(&(item.oracle_id, item.from_collection_id)) else {
            plan.skipped.push(Skipped {
                token,
                reason: SkipReason::NoLongerNeeded,
            });
            continue;
        };
        let holdings = snapshot.holdings.get(&item.oracle_id).unwrap_or(&empty);
        let lines = plan_pull(holdings, item.from_collection_id, want);
        if lines.is_empty() {
            plan.skipped.push(Skipped {
                token,
                reason: SkipReason::NoCopies,
            });
            continue;
        }
        let copies = lines.iter().map(|l| l.quantity).sum();
        for line in lines {
            plan.writes.push(PullWrite {
                source: line.source,
                quantity: line.quantity,
            });
        }
        plan.pulled.push(Pulled { token, copies });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{Board, CardLocation, Condition, Finish};
    use uuid::Uuid;

    fn id(n: u128) -> Id {
        Id::from_u128(n)
    }

    const DEST: u128 = 1;
    const SRC_A: u128 = 2;
    const SRC_B: u128 = 3;
    const ORACLE: u128 = 10;

    fn need(oracle: Id, desired: i32, present_here: i32, locations: Vec<CardLocation>) -> NeedRow {
        let gap = desired - present_here;
        let elsewhere: i32 = locations.iter().map(|l| l.quantity).sum();
        let owned_elsewhere = elsewhere.min(gap);
        NeedRow {
            oracle_id: oracle,
            name: "Lightning Bolt".to_string(),
            desired,
            present_here,
            owned_elsewhere,
            short: gap - owned_elsewhere,
            locations,
        }
    }

    fn loc(id: Id, name: &str, quantity: i32) -> CardLocation {
        CardLocation {
            collection_id: id,
            collection_name: name.to_string(),
            quantity,
        }
    }

    fn holding(collection: Id, quantity: i32) -> HoldingLine {
        HoldingLine {
            id: Uuid::new_v4(),
            collection_id: collection,
            printing_id: Uuid::from_u128(99),
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: "en".to_string(),
            board: Board::Main,
            quantity,
        }
    }

    fn item(from: Id) -> PullItem {
        PullItem {
            oracle_id: id(ORACLE),
            from_collection_id: from,
        }
    }

    #[test]
    fn a_line_naming_the_destination_as_its_own_source_is_already_there() {
        let snapshot = PullSnapshot {
            needs: vec![need(id(ORACLE), 2, 0, vec![loc(id(DEST), "Dest", 2)])],
            holdings: HashMap::new(),
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item(id(DEST))]);
        assert!(plan.writes.is_empty());
        assert!(plan.pulled.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, SkipReason::AlreadyThere);
    }

    #[test]
    fn a_pair_the_fresh_allocation_no_longer_names_is_no_longer_needed() {
        // The gap closed since the pick list's own snapshot (e.g. the
        // destination now holds enough already) — `SRC_A` no longer appears
        // in this fresh read's allocation at all.
        let snapshot = PullSnapshot {
            needs: vec![need(id(ORACLE), 1, 1, vec![loc(id(SRC_A), "A", 3)])],
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 3)])]),
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item(id(SRC_A))]);
        assert!(plan.writes.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, SkipReason::NoLongerNeeded);
    }

    #[test]
    fn a_source_fully_drained_since_the_snapshot_is_no_copies() {
        // The fresh needs() read still wants 2 from SRC_A (the destination's
        // gap has not changed), but the locked holdings read finds the source
        // empty — drained by something else between the pick list's snapshot
        // and this transaction's lock.
        let snapshot = PullSnapshot {
            needs: vec![need(id(ORACLE), 2, 0, vec![loc(id(SRC_A), "A", 2)])],
            holdings: HashMap::new(), // nothing locked at all for this oracle
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item(id(SRC_A))]);
        assert!(plan.writes.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, SkipReason::NoCopies);
    }

    /// The concurrency shape this task exists to make safe: the snapshot's
    /// locked holdings hold **fewer** copies than the fresh allocation wants
    /// (another operation moved some out between the pick list's own
    /// snapshot and this transaction's `FOR UPDATE` read, all of which is
    /// invisible to this pure function — it only ever sees what it is given).
    /// The plan must yield the honest partial rather than erroring or
    /// silently asking for more than is there: `pull_line_outcome` on the
    /// client already turns exactly this into "Pulled 1 of 2 — 1 still
    /// owed."
    #[test]
    fn a_source_with_fewer_copies_than_the_ask_plans_the_honest_partial() {
        let snapshot = PullSnapshot {
            needs: vec![need(id(ORACLE), 2, 0, vec![loc(id(SRC_A), "A", 2)])],
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 1)])]),
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item(id(SRC_A))]);
        assert_eq!(
            plan.writes,
            vec![PullWrite {
                source: MoveSource {
                    from: id(SRC_A),
                    printing_id: Uuid::from_u128(99),
                    finish: Finish::Nonfoil,
                    condition: Condition::Nm,
                    language: "en".to_string(),
                    board: Board::Main,
                },
                quantity: 1,
            }]
        );
        assert_eq!(plan.pulled.len(), 1);
        assert_eq!(
            plan.pulled[0].copies, 1,
            "honest — not the 2 that was asked"
        );
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn a_gap_spanning_two_sources_writes_one_line_per_source() {
        let snapshot = PullSnapshot {
            needs: vec![need(
                id(ORACLE),
                5,
                0,
                vec![loc(id(SRC_A), "A", 3), loc(id(SRC_B), "B", 4)],
            )],
            holdings: HashMap::from([(
                id(ORACLE),
                vec![holding(id(SRC_A), 3), holding(id(SRC_B), 4)],
            )]),
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item(id(SRC_A)), item(id(SRC_B))]);
        assert_eq!(plan.writes.len(), 2);
        assert_eq!(plan.pulled.len(), 2);
        assert!(plan.skipped.is_empty());
        let total: i32 = plan.pulled.iter().map(|p| p.copies).sum();
        assert_eq!(total, 5, "the whole gap, spread across both sources");
    }

    #[test]
    fn a_duplicated_item_does_not_multiply_the_write() {
        let snapshot = PullSnapshot {
            needs: vec![need(id(ORACLE), 2, 0, vec![loc(id(SRC_A), "A", 4)])],
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 4)])]),
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item(id(SRC_A)), item(id(SRC_A))]);
        assert_eq!(plan.writes.len(), 1);
        assert_eq!(plan.pulled.len(), 1);
        assert_eq!(plan.pulled[0].copies, 2, "the gap, once, not twice");
    }

    #[test]
    fn oracle_ids_of_is_sorted_and_deduplicated() {
        let a = id(1);
        let b = id(2);
        let items = vec![
            PullItem {
                oracle_id: b,
                from_collection_id: id(90),
            },
            PullItem {
                oracle_id: a,
                from_collection_id: id(91),
            },
            PullItem {
                oracle_id: b,
                from_collection_id: id(92),
            },
        ];
        let mut expected = [a, b];
        expected.sort();
        assert_eq!(oracle_ids_of(&items), expected.to_vec());
    }
}
