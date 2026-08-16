//! The in-place quantity steppers for a single `holdings` row (haves) or a
//! single `desires` row (wants) — lifted out of `app/src/my/collection.rs`'s
//! `HereCount` (the collection view's HERE cell) so a second surface can reuse
//! the exact write semantics instead of re-deriving them. `/cards/:id`'s "Your
//! copies" / "Your wants" blocks (the card-quantities-on-detail-page task) are
//! that second surface; `HereCount` now delegates its own render + write logic
//! to [`HaveStepper`] rather than duplicating it.
//!
//! **[`HaveStepper`] mirrors a maintainer ruling (P6-054, 2026-08-15):**
//! quantity edits are off-ledger (`set_holding_quantity`), and a committed
//! **zero** always routes through `remove_holding` instead — a move with no
//! destination, which is what makes it undoable. The stepper never renders at
//! all when the caller has no single `holdings` row to address (a cell
//! summing several finish/condition/language/board grains); see
//! `holding_id`'s own doc on [`shared::CardRow`] / [`shared::OwnershipEntry`].
//!
//! **[`WantStepper`] is the wants counterpart, and it is deliberately
//! simpler.** Desires carry no ledger at all (`shared::QuickAddReceipt`'s own
//! doc: a `+ Want` is confirmed but never undoable), so a committed zero there
//! is a direct, non-undoable delete — no move, no Undo action, no
//! `LastMoveState` bookkeeping. It still needs the same *dead-id* guard
//! `HaveStepper` does (a stale toast/undo must not write to a row the zero
//! commit already deleted), so it keeps the same `removed`-gated shape.
//!
//! Both components own their write, their optimistic update, and their own
//! toast; what they do **not** own is any page-level aggregate (a header
//! total, a deck section's slot count) or which secondary reads to refresh
//! afterward — those are the caller's business, reached through `on_change`
//! and `on_settled` rather than a hardcoded dependency on any one page's
//! resources. That is what makes the same component usable from a table row
//! (`CardTableRow`, which also drives a selection checkbox off `removed`) and
//! from a plain list row (`/cards/:id`, which has neither).

use leptos::prelude::*;
use leptos::task::spawn_local;
use shared::Id;

use super::palette::{forget_last_move, note_last_move, LastMoveState};
use super::ui::count_stepper::{CountStepper, StepperCommit};
use super::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::my::collection::message_of;

/// A zero reads as absence, not as a number worth aligning against.
pub fn count_or_dash(n: i32) -> String {
    if n > 0 {
        n.to_string()
    } else {
        "—".to_string()
    }
}

/// Whether a stepper's `on_commit` should drop a commit rather than act on it.
///
/// `CountStepper`'s own built-in commit-toast carries an Undo that re-fires
/// `on_commit` later, and its only guard is "is the row's `value` signal still
/// live" — true here, because a removal deliberately does **not** dispose the
/// row (that is what keeps the *removal's own* Undo toast reachable). So a
/// "3 → 1" toast raised before a removal can still fire after it, and without
/// this check `on_commit` would post the reversed count to a row the removal
/// just deleted — a write to a dead id, surfaced as a bogus "Couldn't save:
/// not found" error toast (app-ui.md → Findings).
///
/// Read at the moment a commit arrives, not once: while the row is live this
/// must return `false` for the *real* first commit too, or nothing could ever
/// be typed into the stepper.
fn stale_commit_should_be_dropped(row_removed: bool) -> bool {
    row_removed
}

/// A cell whose count cannot be edited in place — either it sums more than one
/// underlying row, or (defensively) there is nothing here at all.
fn refusal_span(n: i32, title: &'static str) -> impl IntoView {
    view! {
        <span class="text-muted-foreground" data-testid="here-count" title=title>
            {count_or_dash(n)}
        </span>
    }
}

/// The "row is gone" placeholder — shown once a committed zero has removed the
/// backing row, until an ancestor refetch drops the row from view entirely (or,
/// for a have, until the removal's own Undo restores it).
fn removed_span(title: &'static str) -> impl IntoView {
    view! {
        <span class="text-muted-foreground" data-testid="here-count" title=title>
            "—"
        </span>
    }
}

/// The HERE-style stepper for one `holdings` row: optimistic set,
/// commit-to-zero routed through `remove_holding` (undoable), multi-grain
/// refusal when there is no single row to address. See the module doc for the
/// P6-054 ruling this implements.
#[component]
pub fn HaveStepper(
    /// Names the write's toasts (the card/printing name).
    #[prop(into)]
    name: String,
    /// This cell's current present count.
    present: i32,
    /// The one `holdings` row backing this cell, or `None` when it sums more
    /// than one finish/condition/language/board grain — the stepper then
    /// refuses instead of rendering (see the callers' own `holding_id` docs).
    holding_id: Option<Id>,
    /// Fired with the *authoritative* (from, to) whenever this cell's real
    /// count changes — a commit, a removal (`to` is always 0), or that
    /// removal's undo (`from` is always 0). Callers that keep their own
    /// aggregate (a header total, a deck section's slot count) update it from
    /// here rather than re-deriving it, since a removal and its undo have no
    /// `StepperCommit` of their own to report the change through.
    #[prop(into)]
    on_change: Callback<StepperCommit>,
    /// Run after every write that settles successfully — a commit, a removal,
    /// or an undo. The usual job is refetching a *secondary* read this cell
    /// doesn't own (the sidebar tree's badges); this cell's own count is
    /// already right from the optimistic write, so refetching *this* page's
    /// resource here would dispose the stepper mid-toast (collection.rs's own
    /// module doc explains why `view_res` stays off this path).
    #[prop(into)]
    on_settled: Callback<()>,
    /// Run *in addition to* `on_settled`, only after a removal's own undo
    /// succeeds. Undo is a terminal action — nothing is still reaching into
    /// this row's identity afterward — so it is the one safe point for a
    /// caller to run a heavier, row-rebuilding refetch that would break a
    /// still-open Undo toast anywhere else in this component's lifecycle
    /// (`CollectionPage`'s `HoldingsRevision` bump is exactly that refetch).
    #[prop(optional)]
    on_undo_settled: Option<Callback<()>>,
    /// Externally observed "this row is gone" flag. `CardTableRow` also drives
    /// its selection checkbox off this exact signal, so it owns it and passes
    /// it in; a caller with nothing else watching it can leave this unset —
    /// one is created internally.
    #[prop(optional)]
    removed: Option<RwSignal<bool>>,
) -> impl IntoView {
    let Some(holding_id) = holding_id else {
        let title = if present > 0 {
            "several finishes or conditions here — edit them individually"
        } else {
            "wanted here, not held"
        };
        return refusal_span(present, title).into_any();
    };

    // A signal, not a plain `Id`: undoing a removal re-inserts the holding
    // under a *new* id, and `remove`/`on_commit` below read this at call time
    // rather than capturing it once, so `undo_removal` can rewire it in place
    // — closing the window where a +/- during an in-flight undo would post to
    // the dead pre-removal id (app-ui.md → Findings).
    let holding_id = RwSignal::new(holding_id);
    let value = RwSignal::new(present);
    let removed = removed.unwrap_or_else(|| RwSignal::new(false));
    let toast = expect_context::<ToastHandle>();
    // A removal is a move with no destination, so ⌘K's `Undo last move`
    // reverses it too — absent (`None`) wherever the palette isn't mounted
    // (the bench), same graceful-degrade every other reader of this context
    // already uses.
    let last_move = use_context::<LastMoveState>();
    let label = StoredValue::new(name.clone());

    // Reverse the removal through the move ledger — the copies come back at
    // the grain and on the board they left, which is the whole reason the
    // removal is a move rather than a delete.
    let undo_removal = move |move_id: Id, copies: i32| {
        forget_last_move(last_move, &[move_id]);
        spawn_local(async move {
            match crate::undo_move(move_id).await {
                Ok(receipt) => {
                    // `try_*` throughout: a toast outlives its row, so this can
                    // run after a navigation disposed these signals.
                    on_change.run(StepperCommit {
                        from: 0,
                        to: copies,
                    });
                    let _ = removed.try_set(false);
                    let _ = value.try_set(copies);
                    // Rewire to the *live* id immediately — undoing a removal
                    // re-inserts the holding under a new one, and the server
                    // just told us which.
                    if let Some(new_id) = receipt.restored_holding_id {
                        let _ = holding_id.try_set(new_id);
                    }
                    on_settled.run(());
                    if let Some(cb) = on_undo_settled {
                        cb.run(());
                    }
                }
                Err(e) => {
                    toast.show(
                        ToastOptions::message(format!("Couldn't undo: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    // A committed 0. The optimistic write has already happened (the stepper
    // wrote `value`), and the stepper raised nothing — `caller_reports`
    // claimed this commit — so the message and the undo are both this
    // callback's.
    let remove = move |copies: i32| {
        on_change.run(StepperCommit {
            from: copies,
            to: 0,
        });
        removed.set(true);
        spawn_local(async move {
            let Some(id) = holding_id.try_get_untracked() else {
                return;
            };
            match crate::remove_holding(id).await {
                Ok(receipt) => {
                    on_settled.run(());
                    note_last_move(last_move, vec![receipt.move_id]);
                    // From the receipt, not the rendered `copies` this
                    // callback was passed: the removal takes the *whole
                    // stack*, and if it grew in another tab since this row
                    // last rendered, `copies` undercounts what the server
                    // actually removed.
                    let removed_qty = receipt.quantity;
                    let copies_label = if removed_qty == 1 {
                        "1 copy".to_string()
                    } else {
                        format!("{removed_qty} copies")
                    };
                    toast.show(
                        ToastOptions::message(format!(
                            "Removed {} ({copies_label})",
                            label.get_value()
                        ))
                        .kind(ToastKind::Success)
                        .action(
                            "Undo",
                            Callback::new(move |()| undo_removal(receipt.move_id, removed_qty)),
                        ),
                    );
                }
                Err(e) => {
                    removed.set(false);
                    value.set(copies);
                    on_change.run(StepperCommit {
                        from: 0,
                        to: copies,
                    });
                    toast.show(
                        ToastOptions::message(format!("Couldn't remove: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    let on_commit = Callback::new(move |c: StepperCommit| {
        // See `stale_commit_should_be_dropped`: a stale count-change toast's
        // own Undo can still fire after this row is removed, and the only
        // legitimate write left at that point is the removal's own reversal
        // — which runs through `undo_removal` above, not through here.
        if stale_commit_should_be_dropped(removed.get_untracked()) {
            return;
        }
        if c.to == 0 {
            remove(c.from);
            return;
        }
        // Optimistic: the stepper already wrote `value`, so the caller's own
        // aggregate must move with it or the two disagree on screen.
        on_change.run(c);
        spawn_local(async move {
            let Some(id) = holding_id.try_get_untracked() else {
                return;
            };
            match crate::set_holding_quantity(id, c.to).await {
                Ok(()) => on_settled.run(()),
                Err(e) => {
                    value.set(c.from);
                    on_change.run(StepperCommit {
                        from: c.to,
                        to: c.from,
                    });
                    toast.show(
                        ToastOptions::message(format!("Couldn't save: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        <Show
            when=move || !removed.get()
            fallback=move || {
                removed_span("removed — Undo from the toast, or reload to see the card gone")
            }
        >
            <CountStepper
                value
                label=label.get_value()
                on_commit
                caller_reports=Callback::new(|c: StepperCommit| c.to == 0)
                class="justify-end"
            />
        </Show>
    }
    .into_any()
}

/// The wants counterpart of [`HaveStepper`], for one `desires` row. Desires
/// carry no ledger (`shared::QuickAddReceipt`'s own doc), so a committed zero
/// here is a direct, non-undoable delete rather than a reversible move — no
/// `LastMoveState`, no Undo action, just a confirmation. See the module doc.
#[component]
pub fn WantStepper(
    /// Names the write's toasts (the card name).
    #[prop(into)]
    name: String,
    /// This cell's current desired count.
    desired: i32,
    /// The one `desires` row backing this cell, or `None` when it sums more
    /// than one board/printing-pin grain.
    desire_id: Option<Id>,
    /// As [`HaveStepper::on_change`]: fired with the authoritative (from, to)
    /// on every settled change (a commit or a removal — wants have no undo to
    /// report a third time through).
    #[prop(into)]
    on_change: Callback<StepperCommit>,
    /// As [`HaveStepper::on_settled`]: run after every successful write.
    #[prop(into)]
    on_settled: Callback<()>,
) -> impl IntoView {
    let Some(desire_id) = desire_id else {
        let title = if desired > 0 {
            "wanted across more than one board or pinned printing here — edit them individually"
        } else {
            "not wanted here"
        };
        return refusal_span(desired, title).into_any();
    };

    let desire_id = RwSignal::new(desire_id);
    let value = RwSignal::new(desired);
    let removed = RwSignal::new(false);
    let toast = expect_context::<ToastHandle>();
    let label = StoredValue::new(name.clone());

    let on_commit = Callback::new(move |c: StepperCommit| {
        // Same dead-id guard as `HaveStepper`: `CountStepper`'s own undo path
        // re-fires `on_commit` against a row a prior zero commit may already
        // have deleted server-side.
        if stale_commit_should_be_dropped(removed.get_untracked()) {
            return;
        }
        on_change.run(c);
        if c.to == 0 {
            removed.set(true);
        }
        spawn_local(async move {
            let Some(id) = desire_id.try_get_untracked() else {
                return;
            };
            match crate::set_desire_quantity(id, c.to).await {
                Ok(()) => {
                    on_settled.run(());
                    if c.to == 0 {
                        toast.show(
                            ToastOptions::message(format!(
                                "Removed {} from wants",
                                label.get_value()
                            ))
                            .kind(ToastKind::Success),
                        );
                    }
                }
                Err(e) => {
                    removed.set(false);
                    value.set(c.from);
                    on_change.run(StepperCommit {
                        from: c.to,
                        to: c.from,
                    });
                    toast.show(
                        ToastOptions::message(format!("Couldn't save: {}", message_of(&e)))
                            .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        <Show
            when=move || !removed.get()
            fallback=move || removed_span("removed from your wants — reload to see it gone")
        >
            <CountStepper
                value
                label=label.get_value()
                on_commit
                caller_reports=Callback::new(|c: StepperCommit| c.to == 0)
                class="justify-end"
            />
        </Show>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_or_dash_reads_zero_as_absence() {
        assert_eq!(count_or_dash(0), "—");
        assert_eq!(count_or_dash(-1), "—");
        assert_eq!(count_or_dash(3), "3");
    }

    #[test]
    fn stale_commits_are_dropped_only_once_the_row_is_removed() {
        // The defect: a "3 → 1" count-change toast's own Undo firing after the
        // row it targets was removed must not turn into a write.
        assert!(stale_commit_should_be_dropped(true));
        // A live row's commits — including its very first one — must never be
        // dropped, or the stepper could never save anything.
        assert!(!stale_commit_should_be_dropped(false));
    }
}
