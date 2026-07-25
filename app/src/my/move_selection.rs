//! Batch move — the selection tray's "Move to…" (specs/app-ui.md → Selection
//! tray; specs/collection-api.md → "Move (batch)").
//!
//! Five things are worth knowing before editing this file.
//!
//! **A `/my` selection cannot be piped into a move, and this is where that is
//! resolved.** [`SelectionKey::Card`] names an oracle card and nothing else: the
//! `/my` row aggregates every collection, and its `printing_id` is the
//! *representative* (has-art-first) printing, which the caller may own zero of.
//! So the row answers neither "from which collection" nor "which printing".
//! Passing `from_collection_id: None` would not be "unknown" — in
//! [`shared::MoveItem`] it means **external intake**, i.e. conjuring copies from
//! outside the system. The resolution therefore happens **server-side against
//! the caller's actual holdings** ([`resolve_card`], fed by
//! `CardDetail::ownership`, which is `(collection, printing, quantity)` — the
//! `/my` row's `locations` *plus the printing* it lacks). Exactly one candidate
//! source ⇒ move it; anything else ⇒ refuse, by name, with a reason.
//!
//! **Refusals are reported, never dropped.** Whatever the server would not move
//! comes back as [`Skipped`] and stays checked in the tray, with a toast saying
//! why. A batch that silently shrank would be indistinguishable from one that
//! worked.
//!
//! **One copy per selected entry.** The tray counts *entries* ("2 cards"), not
//! copies, and `SelectedCard` carries no quantity, so a move of one copy per
//! entry is the only reading of the pill that cannot lie. It is also the small
//! blast radius: a mis-click moves one card, not a playset. Quantity is fixed
//! server-side rather than taken from the caller (the `quick_add` precedent —
//! an adapter whose wire contract is wider than its name is a trap).
//!
//! **The undo is one action over N ledger rows.** A batch move writes one
//! `moves` row per item — the ledger has no batch id — so "one undo covering
//! the whole batch" is `CollectionStore::undo_moves`, which reverses the list in
//! a *single transaction*. Looping the single-move undo would be N
//! transactions, and a failure part-way would leave the batch half-reverted
//! behind a toast that said it was undone.
//!
//! **Boards are refused, not silently mainboarded.** `moves` has no board
//! column and `holding_take` is hardcoded to `board = 'main'`, so a move issued
//! from a sideboard row would take mainboard copies — a different card's worth
//! of copies than the row the user checked. Those entries are refused with a
//! stated reason until the move-flows task gives the ledger a board.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use shared::{Board, CollectionSummary, Id, OwnershipEntry, SuggestedDestination};

use crate::catalog::destination::{
    picker_order, Destination, DestinationChoice, DestinationList, DestinationOption,
};
use crate::components::ui::popover::{Popover, PopoverAlign, PopoverContent, PopoverTrigger};
use crate::components::ui::selection_tray::{SelectionKey, SelectionState};
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};

// ------------------------------------------------ what a batch move reports ---

/// The result of one batch move: the ledger rows it wrote (the undo handle),
/// which selection entries actually moved, and which were refused and why.
///
/// Entries are named by [`SelectionKey::token`] rather than by index so the
/// client can match them back to tray rows without assuming the server kept the
/// order — and so the wire shape carries no card names the client already has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MoveOutcome {
    /// One per moved item, in batch order — all undone together.
    pub move_ids: Vec<Id>,
    /// Tokens of the entries that moved.
    pub moved: Vec<String>,
    /// Entries the server declined to move, each with its reason.
    pub skipped: Vec<Skipped>,
}

/// One entry the batch refused, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    pub token: String,
    pub reason: SkipReason,
}

/// Why a selected row was not moved. An enum, not a server-composed sentence:
/// the wording is the client's business and the reasons are a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    /// The copies are already in the destination.
    AlreadyThere,
    /// A deck sideboard/maybeboard row — the move ledger has no board.
    Board(Board),
    /// Nothing is held any more (a selection made stale by another tab).
    NoCopies,
    /// A `/my` row whose copies sit in several collections: which one to take
    /// from is the user's call, not a guess this code may make.
    ManyCollections(usize),
    /// A `/my` row held under several printings in one collection: same
    /// problem, one level down.
    ManyPrintings(usize),
}

impl SkipReason {
    /// The half-sentence the toast appends to a card name.
    pub fn phrase(self) -> String {
        match self {
            Self::AlreadyThere => "is already there".to_string(),
            Self::Board(board) => format!("is on the {} board", board.to_pg()),
            Self::NoCopies => "has no copies left to move".to_string(),
            Self::ManyCollections(n) => {
                format!("is in {n} collections — open one and select the row there")
            }
            Self::ManyPrintings(n) => {
                format!("is held under {n} printings — open its collection and select the row")
            }
        }
    }
}

// ------------------------------------------------------------- resolution ---

/// What a `/my` (oracle-grained) selection resolves to, given everything the
/// caller holds of that card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardSource {
    /// The one unambiguous candidate: this collection, this printing.
    Move {
        from: Id,
        printing_id: Id,
    },
    Refuse(SkipReason),
}

/// Resolve a `/my` row into a move source, or refuse.
///
/// `entries` is `CardDetail::ownership` — every `(collection, printing)` the
/// caller holds copies of this oracle card under. The destination is not a
/// *candidate source*, so it is filtered out first: a card sitting in both the
/// Trade Binder and the deck you are moving to has exactly one place it can
/// come from, and refusing that as "ambiguous" would be needlessly useless.
///
/// After that the rule is deliberately strict — **exactly one** candidate.
/// Anything else is a question only the user can answer, and the enum's whole
/// purpose is that this code cannot invent an answer.
pub fn resolve_card(entries: &[OwnershipEntry], to: Id) -> CardSource {
    let candidates: Vec<&OwnershipEntry> = entries
        .iter()
        .filter(|e| e.collection_id != to && e.quantity > 0)
        .collect();
    match candidates.len() {
        0 if entries.iter().any(|e| e.collection_id == to) => {
            CardSource::Refuse(SkipReason::AlreadyThere)
        }
        0 => CardSource::Refuse(SkipReason::NoCopies),
        1 => CardSource::Move {
            from: candidates[0].collection_id,
            printing_id: candidates[0].printing_id,
        },
        n => {
            let mut collections: Vec<Id> = candidates.iter().map(|e| e.collection_id).collect();
            collections.sort();
            collections.dedup();
            if collections.len() > 1 {
                CardSource::Refuse(SkipReason::ManyCollections(collections.len()))
            } else {
                CardSource::Refuse(SkipReason::ManyPrintings(n))
            }
        }
    }
}

/// Refuse a `/my/collections/:id` row the move cannot honor, or `None` to move
/// it. Grain-complete rows need no lookup — the only two refusals are the
/// board the ledger cannot express and a move to where the copies already are.
pub fn refuse_held(from: Id, board: Board, to: Id) -> Option<SkipReason> {
    if board != Board::Main {
        return Some(SkipReason::Board(board));
    }
    (from == to).then_some(SkipReason::AlreadyThere)
}

/// Fold per-card destination suggestions into one ranking for the whole
/// selection: a collection that wants three of the selected cards outranks one
/// that wants a single copy of one. Shortfall descending, then name, matching
/// `suggested_destinations`' own order for the single-card case.
pub fn merge_suggestions(per_card: Vec<Vec<SuggestedDestination>>) -> Vec<SuggestedDestination> {
    let mut merged: Vec<SuggestedDestination> = Vec::new();
    for row in per_card.into_iter().flatten() {
        match merged
            .iter_mut()
            .find(|m| m.collection_id == row.collection_id)
        {
            Some(m) => {
                m.desired += row.desired;
                m.present += row.present;
                m.shortfall += row.shortfall;
            }
            None => merged.push(row),
        }
    }
    merged.sort_by(|a, b| {
        b.shortfall.cmp(&a.shortfall).then_with(|| {
            a.collection_name
                .to_lowercase()
                .cmp(&b.collection_name.to_lowercase())
        })
    });
    merged
}

/// The picker's rows: the collections that *want* the selection first, each
/// hinting its shortfall, then everything else in the toolbar picker's order
/// (Inbox pinned, then by name).
///
/// Both halves are needed. Suggestions alone would leave a plain "put these in
/// that binder" move impossible whenever no collection happens to desire the
/// cards, which is the common case for a binder; the full list alone would
/// throw away the ranking this task exists to use.
pub fn picker_options(
    suggested: &[SuggestedDestination],
    all: &[CollectionSummary],
) -> Vec<DestinationChoice> {
    let mut rows: Vec<DestinationChoice> = Vec::new();
    for s in suggested {
        // Resolve names/flags from the live list so a suggestion cannot show a
        // stale name — and skip any suggestion for a collection that no longer
        // exists, rather than offering a destination every move would 404 on.
        if let Some(c) = all.iter().find(|c| c.id == s.collection_id) {
            rows.push(DestinationChoice {
                dest: Destination {
                    id: c.id,
                    name: c.name.clone(),
                    is_inbox: c.is_inbox,
                },
                hint: Some(format!("wants {}", s.shortfall)),
            });
        }
    }
    let rest = all
        .iter()
        .filter(|c| !rows.iter().any(|r| r.dest.id == c.id))
        .cloned()
        .collect::<Vec<_>>();
    rows.extend(picker_order(rest).into_iter().map(|c| {
        DestinationChoice::plain(Destination {
            id: c.id,
            name: c.name,
            is_inbox: c.is_inbox,
        })
    }));
    rows
}

// ----------------------------------------------------------------- wording ---

/// The confirmation toast's message. The count is stated in **copies**, because
/// the pill counts entries and one copy each is the thing a reader would
/// otherwise have to infer.
pub fn moved_message(moved: usize, destination: &str) -> String {
    let copies = if moved == 1 { "1 copy" } else { "1 copy each" };
    let cards = if moved == 1 {
        "1 card".to_string()
    } else {
        format!("{moved} cards")
    };
    format!("Moved {cards} ({copies}) → {destination}")
}

/// The refusal toast. Names up to [`NAMED_SKIPS`] cards with their reasons and
/// counts the rest, so a batch of thirty refusals is still one readable line.
pub fn skipped_message(skipped: &[(String, SkipReason)]) -> String {
    let head = skipped
        .iter()
        .take(NAMED_SKIPS)
        .map(|(name, reason)| format!("{name} {}", reason.phrase()))
        .collect::<Vec<_>>()
        .join("; ");
    let rest = skipped.len().saturating_sub(NAMED_SKIPS);
    let lead = if skipped.len() == 1 {
        "1 card wasn't moved".to_string()
    } else {
        format!("{} cards weren't moved", skipped.len())
    };
    if rest > 0 {
        format!("{lead}: {head}; and {rest} more")
    } else {
        format!("{lead}: {head}")
    }
}

/// How many refusals a toast names before it starts counting.
const NAMED_SKIPS: usize = 2;

// ---------------------------------------------------------------- staleness ---

/// Bumped whenever copies move, so the views that render holdings refetch.
///
/// The tray lives in the shell and the tables live in pages, so a move has no
/// handle on the resource it invalidated. This is that handle: pages include it
/// in their resource's *source*, which is a refetch by construction rather than
/// an effect that has to remember to fire. Provided once, by the shell.
#[derive(Clone, Copy)]
pub struct HoldingsRevision(pub RwSignal<u32>);

impl HoldingsRevision {
    pub fn bump(self) {
        self.0.update(|n| *n += 1);
    }
}

pub fn provide_holdings_revision() -> HoldingsRevision {
    let revision = HoldingsRevision(RwSignal::new(0));
    provide_context(revision);
    revision
}

/// The revision a page's resource should depend on. Zero and constant outside
/// the shell (the bench), so a page component stays mountable there.
pub fn holdings_revision() -> Signal<u32> {
    match use_context::<HoldingsRevision>() {
        Some(r) => Signal::derive(move || r.0.get()),
        None => Signal::derive(|| 0),
    }
}

// ------------------------------------------------------------- the control ---

/// The tray's primary action: "Move to…", the destination picker, the batch
/// write, and the one Undo that covers it.
///
/// Mounted inside the tray's `Show`, so its resources are fetched only while
/// there is a selection to move.
#[component]
pub fn MoveSelection(selection: SelectionState) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    // Optional, both of them: the bench hosts this control outside the shell.
    let tree = use_context::<crate::my::tree::CollectionTreeResource>();
    let revision = use_context::<HoldingsRevision>();

    let open = RwSignal::new(false);
    let pending = RwSignal::new(false);

    // The distinct oracle cards in the selection — `suggested_destinations` is
    // per-oracle, and two selected rows can be the same card in two places.
    let oracles = Memo::new(move |_| {
        let mut ids = selection
            .items()
            .with(|v| v.iter().map(|c| c.oracle_id).collect::<Vec<_>>());
        ids.sort();
        ids.dedup();
        ids
    });

    let suggested = Resource::new(
        move || oracles.get(),
        |ids| async move {
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            crate::selection_destinations(ids).await
        },
    );
    let collections = Resource::new(|| (), |_| crate::list_collections());

    let choose = Callback::new(move |dest: Destination| {
        if pending.get_untracked() {
            return;
        }
        let entries = selection.items().get_untracked();
        if entries.is_empty() {
            return;
        }
        // Names for the refusal toast: the server answers in tokens (it has no
        // business shipping back strings the client already holds).
        let names: Vec<(String, String)> = entries
            .iter()
            .map(|c| (c.key.token(), c.name.clone()))
            .collect();
        let keys: Vec<SelectionKey> = entries.iter().map(|c| c.key).collect();

        pending.set(true);
        open.set(false);
        spawn_local(async move {
            let result = crate::move_selection(dest.id, keys).await;
            pending.set(false);
            match result {
                Ok(outcome) => {
                    // Drop what moved; leave the refusals checked — they are
                    // what still needs doing, and the toast names them.
                    selection.remove_tokens(&outcome.moved);
                    if !outcome.move_ids.is_empty() {
                        if let Some(t) = tree {
                            t.0.refetch();
                        }
                        if let Some(r) = revision {
                            r.bump();
                        }
                        let move_ids = outcome.move_ids.clone();
                        toast.show(
                            ToastOptions::message(moved_message(
                                outcome.moved.len(),
                                &dest.label(),
                            ))
                            .kind(ToastKind::Success)
                            .action(
                                "Undo",
                                Callback::new(move |()| {
                                    undo(toast, tree, revision, move_ids.clone())
                                }),
                            ),
                        );
                    }
                    let refused = label_skips(&outcome.skipped, &names);
                    if !refused.is_empty() {
                        toast.show(
                            ToastOptions::message(skipped_message(&refused)).kind(ToastKind::Error),
                        );
                    }
                }
                Err(e) => {
                    toast.show(
                        ToastOptions::message(format!(
                            "Couldn't move: {}",
                            crate::my::collection::message_of(&e)
                        ))
                        .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    view! {
        // Aligned to its own end and opening upward (the popover's default
        // block-start placement): this control sits at the bottom of the
        // viewport, where a downward panel would be off screen.
        <Popover id="tray-destination" open=open align=PopoverAlign::End>
            <PopoverTrigger
                class="bg-background text-foreground h-auto shrink-0 rounded-md border-0 px-3 py-1.5 text-[13px] font-medium"
                attr:data-testid="tray-move"
            >
                {move || if pending.get() { "Moving…" } else { "Move to…" }}
            </PopoverTrigger>
            <PopoverContent class="w-[280px] p-0">
                <DestinationList empty="No collection to move to.">
                    // Same boundary the catalog's picker uses, and for the same
                    // reason: the rows come from resources, and only a
                    // suspense boundary keeps a render in step with them.
                    <Transition fallback=|| {
                        view! {
                            <p class="text-muted-foreground p-3 text-sm">"Loading collections…"</p>
                        }
                    }>
                        {move || Suspend::new(async move {
                            let all = collections.await.unwrap_or_default();
                            let ranked = suggested.await.unwrap_or_default();
                            picker_options(&ranked, &all)
                                .into_iter()
                                .map(|choice| {
                                    view! { <DestinationOption choice on_choose=choose /> }
                                })
                                .collect_view()
                        })}
                    </Transition>
                </DestinationList>
            </PopoverContent>
        </Popover>
    }
}

/// Reverse the whole batch — one call, one transaction (see the module docs).
fn undo(
    toast: ToastHandle,
    tree: Option<crate::my::tree::CollectionTreeResource>,
    revision: Option<HoldingsRevision>,
    move_ids: Vec<Id>,
) {
    let count = move_ids.len();
    spawn_local(async move {
        match crate::undo_selection_move(move_ids).await {
            Ok(()) => {
                if let Some(t) = tree {
                    t.0.refetch();
                }
                if let Some(r) = revision {
                    r.bump();
                }
                let cards = if count == 1 { "1 card" } else { "them" };
                toast.show(ToastOptions::message(format!("Put {cards} back")));
            }
            Err(e) => {
                toast.show(
                    ToastOptions::message(format!(
                        "Couldn't undo: {}",
                        crate::my::collection::message_of(&e)
                    ))
                    .kind(ToastKind::Error),
                );
            }
        }
    });
}

/// Pair each refusal with the card name the tray showed for it. A token with no
/// matching entry is impossible in practice (the batch is built from the tray)
/// but is reported rather than dropped — a silent disappearance is the exact
/// failure this whole reporting path exists to prevent.
pub fn label_skips(skipped: &[Skipped], names: &[(String, String)]) -> Vec<(String, SkipReason)> {
    skipped
        .iter()
        .map(|s| {
            let name = names
                .iter()
                .find(|(token, _)| *token == s.token)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| "A card".to_string());
            (name, s.reason)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Id {
        Id::from_u128(n)
    }

    fn own(collection: u128, printing: u128, quantity: i32) -> OwnershipEntry {
        OwnershipEntry {
            collection_id: id(collection),
            collection_name: format!("Collection {collection}"),
            printing_id: id(printing),
            quantity,
        }
    }

    #[test]
    fn one_source_resolves_to_that_printing() {
        let entries = [own(1, 100, 3)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Move {
                from: id(1),
                printing_id: id(100),
            }
        );
    }

    #[test]
    fn the_destination_is_not_a_candidate_source() {
        // Held in the deck we're moving to *and* in the binder: exactly one
        // place it can come from, so this resolves rather than refusing.
        let entries = [own(9, 100, 1), own(1, 101, 2)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Move {
                from: id(1),
                printing_id: id(101),
            }
        );
    }

    #[test]
    fn two_collections_are_a_question_for_the_user() {
        let entries = [own(1, 100, 1), own(2, 100, 1)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::ManyCollections(2))
        );
    }

    #[test]
    fn two_printings_in_one_collection_are_ambiguous_too() {
        // Same collection, two printings — the row on `/my` names neither, and
        // its representative printing may be one of them or neither.
        let entries = [own(1, 100, 1), own(1, 101, 1)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::ManyPrintings(2))
        );
    }

    #[test]
    fn nothing_held_never_becomes_an_intake() {
        // The bug this whole enum exists to prevent: `from = None` in a
        // `MoveItem` means copies arriving from outside the system, so an
        // unresolvable selection must refuse, not fall back to it.
        assert_eq!(
            resolve_card(&[], id(9)),
            CardSource::Refuse(SkipReason::NoCopies)
        );
    }

    #[test]
    fn everything_already_in_the_destination_says_so() {
        let entries = [own(9, 100, 2)];
        assert_eq!(
            resolve_card(&entries, id(9)),
            CardSource::Refuse(SkipReason::AlreadyThere)
        );
    }

    #[test]
    fn a_sideboard_row_is_refused_rather_than_mainboarded() {
        assert_eq!(
            refuse_held(id(1), Board::Side, id(9)),
            Some(SkipReason::Board(Board::Side))
        );
        assert_eq!(refuse_held(id(1), Board::Main, id(9)), None);
        assert_eq!(
            refuse_held(id(1), Board::Main, id(1)),
            Some(SkipReason::AlreadyThere)
        );
    }

    fn suggestion(collection: u128, name: &str, shortfall: i32) -> SuggestedDestination {
        SuggestedDestination {
            collection_id: id(collection),
            collection_name: name.to_string(),
            desired: shortfall,
            present: 0,
            shortfall,
        }
    }

    #[test]
    fn suggestions_add_up_across_the_selection() {
        // Two cards, both wanted by the same deck: that deck outranks one that
        // wants a single copy of one of them.
        let merged = merge_suggestions(vec![
            vec![suggestion(1, "Deck", 1), suggestion(2, "Shoebox", 2)],
            vec![suggestion(1, "Deck", 2)],
        ]);
        let names: Vec<_> = merged.iter().map(|s| s.collection_name.as_str()).collect();
        assert_eq!(names, vec!["Deck", "Shoebox"]);
        assert_eq!(merged[0].shortfall, 3);
    }

    fn collection(n: u128, name: &str, is_inbox: bool) -> CollectionSummary {
        CollectionSummary {
            id: id(n),
            parent_id: None,
            kind: shared::CollectionKind::Binder,
            name: name.to_string(),
            is_inbox,
            position: 0.0,
            format: None,
        }
    }

    #[test]
    fn wanted_destinations_lead_and_everything_else_still_appears() {
        let all = [
            collection(1, "Deck", false),
            collection(2, "Inbox", true),
            collection(3, "Shoebox", false),
        ];
        let rows = picker_options(&[suggestion(3, "Shoebox", 2)], &all);
        let labels: Vec<_> = rows.iter().map(|r| r.dest.name.as_str()).collect();
        // Suggested first (with its hint), then Inbox, then the rest by name —
        // and the suggested one is not repeated in the tail.
        assert_eq!(labels, vec!["Shoebox", "Inbox", "Deck"]);
        assert_eq!(rows[0].hint.as_deref(), Some("wants 2"));
        assert!(rows[1].hint.is_none());
    }

    #[test]
    fn a_suggestion_for_a_deleted_collection_is_not_offered() {
        let rows = picker_options(&[suggestion(7, "Gone", 3)], &[collection(1, "Inbox", true)]);
        let labels: Vec<_> = rows.iter().map(|r| r.dest.name.as_str()).collect();
        assert_eq!(labels, vec!["Inbox"]);
    }

    #[test]
    fn the_toast_counts_copies_not_just_cards() {
        assert_eq!(moved_message(1, "🗂 Deck"), "Moved 1 card (1 copy) → 🗂 Deck");
        assert_eq!(
            moved_message(3, "🗂 Deck"),
            "Moved 3 cards (1 copy each) → 🗂 Deck"
        );
    }

    #[test]
    fn refusals_are_named_then_counted() {
        let one = [("Bolt".to_string(), SkipReason::ManyCollections(2))];
        assert_eq!(
            skipped_message(&one),
            "1 card wasn't moved: Bolt is in 2 collections — open one and select the row there"
        );

        let many = [
            ("Bolt".to_string(), SkipReason::AlreadyThere),
            ("Counterspell".to_string(), SkipReason::Board(Board::Side)),
            ("Ancestral".to_string(), SkipReason::NoCopies),
            ("Brainstorm".to_string(), SkipReason::NoCopies),
        ];
        assert_eq!(
            skipped_message(&many),
            "4 cards weren't moved: Bolt is already there; \
             Counterspell is on the side board; and 2 more"
        );
    }

    #[test]
    fn a_refusal_keeps_its_name() {
        let skipped = [Skipped {
            token: "card:x".to_string(),
            reason: SkipReason::NoCopies,
        }];
        let names = [("card:x".to_string(), "Bolt".to_string())];
        assert_eq!(
            label_skips(&skipped, &names),
            vec![("Bolt".to_string(), SkipReason::NoCopies)]
        );
        // An unmatched token is still reported — never dropped.
        assert_eq!(label_skips(&skipped, &[]).len(), 1);
    }
}
