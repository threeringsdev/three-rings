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
//! [`crate::my::needs::offers_of`]/`dedupe`/`plan_pull` are reused
//! directly — that module's own doc is explicit that the client's pick list
//! and the server's write must run "the *same* function over its *own* fresh
//! needs() read," and a second copy of that arithmetic in this file would be
//! exactly the drift that sentence forbids. What moved here is only the
//! *orchestration* around those functions: classifying each line
//! (`AlreadyThere` / `NoLongerNeeded` / `NoCopies` / pulled), which used to
//! live inline in the server fn.

use std::collections::HashMap;

use shared::{Board, HoldingLine, Id, NeedRow};

use crate::my::move_selection::{MoveSource, SkipReason, Skipped};
use crate::my::needs::{dedupe, offers_of, plan_pull, PullItem, Pulled};

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
/// restated default) and land them on the destination's `to_board`.
///
/// **The two boards are independent and both are real** (P6-074). `source.board`
/// is where the copies came off — read off the locked holding, so undo puts
/// them back on the stack they left. `to_board` is the board of the
/// [`NeedRow`] this write is closing, carried in on [`PullItem::board`]: a
/// sideboard need is filled by landing copies on the sideboard. It used to be
/// hardcoded `main` at the call site, which meant a sideboard need could never
/// be closed by pulling — the copies landed on `main` and the need survived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullWrite {
    pub source: MoveSource,
    pub to_board: Board,
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
/// Ported from `pull_needs`'s pre-P6-120 body: `items` is deduped
/// first (a repeated line must not multiply a move — see
/// [`crate::my::needs::dedupe`]'s own doc); a line naming the destination as
/// its own source is [`SkipReason::AlreadyThere`]; a line whose `(oracle,
/// board, from)` key no longer appears in the fresh allocation is
/// [`SkipReason::NoLongerNeeded`]; a line that survives both but finds no
/// copies at the locked source is [`SkipReason::NoCopies`] — reachable now
/// exactly when it could not be reached before: the snapshot's `holdings` for
/// that source held less than `want` (or none at all), because another
/// operation drained it between the pick list's own snapshot and this
/// transaction's lock.
///
/// **Board grain (P6-074).** `planned` is keyed by `(oracle, board, from)`
/// because [`NeedRow`] is now per `(oracle, board)`: the same card wanted on
/// two boards is two rows with two gaps, and each has its own line to pull.
/// Every write carries the need row's board as its `to_board`, so the copies
/// land where they were wanted.
///
/// **Two boards can still name the same physical stack, so the stacks are
/// consumed as they are planned.** `NeedRow::locations` is board-blind on
/// purpose (a copy elsewhere can fill any board's need — see [`NeedRow`]'s own
/// doc), and while `apportion_elsewhere` now stops two boards claiming the same
/// copy in *total*, it does not decide *which collection* each board's share
/// comes out of: [`offers_of`] walks `locations` from the front for every row,
/// so a card with 2 in binder A and 1 in binder B, wanted 2 on `main` and 1 on
/// `side`, offers A twice — 2 copies then 1 more — against A's 2. `remaining`
/// tracks per **holding row** what earlier lines already committed, so the
/// second board gets the honest partial (or `NoCopies`) instead of a write the
/// loop's `holding_take` would reject, aborting the whole pull. That is the same
/// shape a stale pick list already produces, and the toast already says it.
///
/// This is also what makes the `dedupe`-guaranteed uniqueness insufficient on
/// its own: it was one line per `(oracle, from)` before boards existed here.
pub fn plan_pull_needs(
    to_collection_id: Id,
    snapshot: &PullSnapshot,
    items: Vec<PullItem>,
) -> PullPlan {
    let mut planned: HashMap<(Id, Board, Id), i32> = HashMap::new();
    for row in &snapshot.needs {
        for (from, copies) in offers_of(row) {
            planned.insert((row.oracle_id, row.board, from), copies);
        }
    }

    // Per `holdings.id`, what is still unspoken for after the lines planned so
    // far. Seeded from the locked snapshot; only ever decremented here.
    let mut remaining: HashMap<Id, i32> = snapshot
        .holdings
        .values()
        .flatten()
        .map(|h| (h.id, h.quantity))
        .collect();

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
        let Some(&want) = planned.get(&(item.oracle_id, item.board, item.from_collection_id))
        else {
            plan.skipped.push(Skipped {
                token,
                reason: SkipReason::NoLongerNeeded,
            });
            continue;
        };
        // The stacks as they stand *after* earlier lines of this same plan —
        // a drained stack is dropped entirely, exactly as `plan_pull` treats a
        // zero-quantity holding.
        let holdings: Vec<HoldingLine> = snapshot
            .holdings
            .get(&item.oracle_id)
            .unwrap_or(&empty)
            .iter()
            .filter_map(|h| {
                let left = remaining.get(&h.id).copied().unwrap_or(h.quantity);
                (left > 0).then(|| HoldingLine {
                    quantity: left,
                    ..h.clone()
                })
            })
            .collect();
        let lines = plan_pull(&holdings, item.from_collection_id, want);
        if lines.is_empty() {
            plan.skipped.push(Skipped {
                token,
                reason: SkipReason::NoCopies,
            });
            continue;
        }
        let copies = lines.iter().map(|l| l.quantity).sum();
        for line in lines {
            // `plan_pull` returns a `MoveSource`, which does not carry the
            // holding's id — match the stack back by its full grain, the same
            // tuple `holdings_uniq` is keyed on, so the decrement lands on the
            // row the write will actually touch.
            if let Some(h) = snapshot
                .holdings
                .get(&item.oracle_id)
                .unwrap_or(&empty)
                .iter()
                .find(|h| {
                    h.collection_id == line.source.from
                        && h.printing_id == line.source.printing_id
                        && h.finish == line.source.finish
                        && h.condition == line.source.condition
                        && h.language == line.source.language
                        && h.board == line.source.board
                })
            {
                let left = remaining.entry(h.id).or_insert(h.quantity);
                *left -= line.quantity;
            }
            plan.writes.push(PullWrite {
                source: line.source,
                to_board: item.board,
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
        need_on(Board::Main, oracle, desired, present_here, locations)
    }

    /// The same row on a named board — the shape `read_needs_rows` emits since
    /// P6-074, where one oracle can yield a row per board.
    fn need_on(
        board: Board,
        oracle: Id,
        desired: i32,
        present_here: i32,
        locations: Vec<CardLocation>,
    ) -> NeedRow {
        let gap = desired - present_here;
        let elsewhere: i32 = locations.iter().map(|l| l.quantity).sum();
        let owned_elsewhere = elsewhere.min(gap);
        NeedRow {
            oracle_id: oracle,
            name: "Lightning Bolt".to_string(),
            board,
            desired,
            present_here,
            owned_elsewhere,
            short: gap - owned_elsewhere,
            locations,
        }
    }

    /// Rewrite a set of rows the way `read_needs_rows` builds them: the
    /// per-oracle elsewhere pool split across the rows in order, not applied
    /// whole to each. Every multi-board case below goes through this, so the
    /// planner is tested against snapshots the read can actually produce.
    fn apportioned(rows: Vec<NeedRow>) -> Vec<NeedRow> {
        let gaps: Vec<crate::my::needs::NeedGap> = rows
            .iter()
            .map(|r| crate::my::needs::NeedGap {
                oracle_id: r.oracle_id,
                gap: r.desired - r.present_here,
                elsewhere: r.locations.iter().map(|l| l.quantity).sum(),
            })
            .collect();
        let shares = crate::my::needs::apportion_elsewhere(&gaps);
        rows.into_iter()
            .zip(shares)
            .map(|(mut r, share)| {
                r.owned_elsewhere = share;
                r.short = (r.desired - r.present_here) - share;
                r
            })
            .collect()
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
        item_on(Board::Main, from)
    }

    /// A line asking for the copies to land on a named board.
    fn item_on(board: Board, from: Id) -> PullItem {
        PullItem {
            oracle_id: id(ORACLE),
            from_collection_id: from,
            board,
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
                to_board: Board::Main,
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

    // ------------------------------------------- P6-074: board-aware needs ---

    /// The defect this task fixes, at the planner's own layer: the deck holds
    /// the card on `main` and wants one on `side`. `read_needs_rows` now emits
    /// a `side` row (present_here 0 on that board), and the plan has to pull
    /// **for the sideboard** — a write landing on `main` would leave the need
    /// standing and the same pull would be offered forever.
    #[test]
    fn a_want_on_the_sideboard_pulls_onto_the_sideboard() {
        let snapshot = PullSnapshot {
            needs: vec![need_on(
                Board::Side,
                id(ORACLE),
                1,
                0,
                vec![loc(id(SRC_A), "A", 2)],
            )],
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 2)])]),
        };
        let plan = plan_pull_needs(id(DEST), &snapshot, vec![item_on(Board::Side, id(SRC_A))]);
        assert_eq!(plan.writes.len(), 1);
        assert_eq!(plan.writes[0].quantity, 1);
        assert_eq!(
            plan.writes[0].to_board,
            Board::Side,
            "the copies land on the board that wanted them"
        );
        // The *source* board is still whatever the locked stack was found at,
        // so undo puts the copy back on the binder's mainboard.
        assert_eq!(plan.writes[0].source.board, Board::Main);
        assert!(plan.skipped.is_empty());
    }

    /// One card wanted on two boards is two rows with two gaps and two lines —
    /// the shape the board-blind read could not produce at all. The tokens must
    /// differ too, or the client cannot tell the two outcomes apart.
    #[test]
    fn two_boards_wanting_the_same_card_are_two_lines() {
        let snapshot = PullSnapshot {
            needs: apportioned(vec![
                need(id(ORACLE), 1, 0, vec![loc(id(SRC_A), "A", 4)]),
                need_on(Board::Side, id(ORACLE), 2, 0, vec![loc(id(SRC_A), "A", 4)]),
            ]),
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 4)])]),
        };
        let plan = plan_pull_needs(
            id(DEST),
            &snapshot,
            vec![item(id(SRC_A)), item_on(Board::Side, id(SRC_A))],
        );
        assert_eq!(plan.pulled.len(), 2);
        assert_ne!(
            plan.pulled[0].token, plan.pulled[1].token,
            "the board is part of a line's identity"
        );
        assert_eq!(plan.pulled[0].copies, 1);
        assert_eq!(plan.pulled[1].copies, 2);
        assert_eq!(plan.writes.len(), 2);
        assert_eq!(plan.writes[0].to_board, Board::Main);
        assert_eq!(plan.writes[1].to_board, Board::Side);
        assert!(plan.skipped.is_empty());
    }

    /// One elsewhere copy, two boards wanting it. Since the review the *read*
    /// already refuses to promise it twice (`apportion_elsewhere`), so the
    /// sideboard line is not in the fresh allocation at all and a client that
    /// sends it anyway is told so — rather than the plan quietly writing two
    /// moves against one copy.
    #[test]
    fn two_boards_sharing_one_copy_do_not_plan_it_twice() {
        let snapshot = PullSnapshot {
            needs: apportioned(vec![
                need(id(ORACLE), 1, 0, vec![loc(id(SRC_A), "A", 1)]),
                need_on(Board::Side, id(ORACLE), 1, 0, vec![loc(id(SRC_A), "A", 1)]),
            ]),
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 1)])]),
        };
        // The apportioning itself: the sideboard row is Short, not pullable.
        assert_eq!(snapshot.needs[0].owned_elsewhere, 1);
        assert_eq!(snapshot.needs[1].owned_elsewhere, 0);
        assert_eq!(snapshot.needs[1].short, 1);

        let plan = plan_pull_needs(
            id(DEST),
            &snapshot,
            vec![item(id(SRC_A)), item_on(Board::Side, id(SRC_A))],
        );
        let moved: i32 = plan.writes.iter().map(|w| w.quantity).sum();
        assert_eq!(moved, 1, "one copy exists, so one copy moves");
        assert_eq!(plan.pulled.len(), 1);
        assert_eq!(plan.writes[0].to_board, Board::Main);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, SkipReason::NoLongerNeeded);
    }

    /// The partial version of the same shape: three copies at the source, a
    /// mainboard gap of two and a sideboard gap of two. Apportioning gives the
    /// mainboard its two and the sideboard the one that is left.
    #[test]
    fn a_second_board_takes_what_the_first_left() {
        let snapshot = PullSnapshot {
            needs: apportioned(vec![
                need(id(ORACLE), 2, 0, vec![loc(id(SRC_A), "A", 3)]),
                need_on(Board::Side, id(ORACLE), 2, 0, vec![loc(id(SRC_A), "A", 3)]),
            ]),
            holdings: HashMap::from([(id(ORACLE), vec![holding(id(SRC_A), 3)])]),
        };
        let plan = plan_pull_needs(
            id(DEST),
            &snapshot,
            vec![item(id(SRC_A)), item_on(Board::Side, id(SRC_A))],
        );
        assert_eq!(plan.pulled.len(), 2);
        assert_eq!(plan.pulled[0].copies, 2);
        assert_eq!(
            plan.pulled[1].copies, 1,
            "the honest residual, not the 2 its own gap asked for"
        );
        let moved: i32 = plan.writes.iter().map(|w| w.quantity).sum();
        assert_eq!(moved, 3, "never more than the source holds");
    }

    /// **Apportioning fixes the totals, not the per-location split**, and the
    /// stack-consumption guard is what covers the difference. Two binders — A
    /// holds 2, B holds 1 — with a mainboard gap of 2 and a sideboard gap of 1.
    /// The pool (3) covers both gaps, so both rows are fully pullable; but
    /// `offers_of` walks `locations` from the front for each row independently,
    /// so both name **A**, asking 3 copies of a stack that holds 2. The plan
    /// must draw A twice for a total of 2, not 3.
    #[test]
    fn two_boards_naming_the_same_binder_cannot_overdraw_it() {
        let snapshot = PullSnapshot {
            needs: apportioned(vec![
                need(
                    id(ORACLE),
                    2,
                    0,
                    vec![loc(id(SRC_A), "A", 2), loc(id(SRC_B), "B", 1)],
                ),
                need_on(
                    Board::Side,
                    id(ORACLE),
                    1,
                    0,
                    vec![loc(id(SRC_A), "A", 2), loc(id(SRC_B), "B", 1)],
                ),
            ]),
            holdings: HashMap::from([(
                id(ORACLE),
                vec![holding(id(SRC_A), 2), holding(id(SRC_B), 1)],
            )]),
        };
        // Both rows are fully covered by the pool, so both offer against A.
        assert_eq!(snapshot.needs[0].owned_elsewhere, 2);
        assert_eq!(snapshot.needs[1].owned_elsewhere, 1);

        let plan = plan_pull_needs(
            id(DEST),
            &snapshot,
            vec![item(id(SRC_A)), item_on(Board::Side, id(SRC_A))],
        );
        let from_a: i32 = plan
            .writes
            .iter()
            .filter(|w| w.source.from == id(SRC_A))
            .map(|w| w.quantity)
            .sum();
        assert_eq!(from_a, 2, "A holds 2 and cannot be drawn for 3");
        assert_eq!(plan.pulled.len(), 1);
        assert_eq!(plan.pulled[0].copies, 2, "the mainboard line, in full");
        // The sideboard line named A, and A is spent. It is refused rather than
        // claiming a copy it did not get — the same honest shape a stale pick
        // list produces. (Its copy is genuinely available in B; nothing here
        // re-aims a line at another source, and nothing pretends it moved.)
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].reason, SkipReason::NoCopies);
    }

    #[test]
    fn oracle_ids_of_is_sorted_and_deduplicated() {
        let a = id(1);
        let b = id(2);
        let items = vec![
            PullItem {
                oracle_id: b,
                from_collection_id: id(90),
                board: Board::Main,
            },
            PullItem {
                oracle_id: a,
                from_collection_id: id(91),
                board: Board::Main,
            },
            PullItem {
                oracle_id: b,
                from_collection_id: id(92),
                board: Board::Side,
            },
        ];
        let mut expected = [a, b];
        expected.sort();
        assert_eq!(oracle_ids_of(&items), expected.to_vec());
    }
}
