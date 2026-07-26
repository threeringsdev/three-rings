//! SelectionTray — custom gap component №3 (design/component-gap-analysis.md;
//! specs/app-ui.md "Custom gap components"). Not a registry copy: the registry's
//! `action_bar` was evaluated and ruled out in the gap analysis, so the docked
//! thumbnail stack + count + "Move to…" + clear is ours, composed from the
//! vendored [`Checkbox`] and plain elements.
//!
//! Three things are worth knowing before editing this file.
//!
//! **The selection is app state, not page state.** [`SelectionState`] is a
//! single `RwSignal<Vec<SelectedCard>>` installed by the shell *above* the
//! router outlet ([`provide_selection`]). That placement is the whole point:
//! the selection has to survive a Catalog ⇄ My-cards mode switch, a navigation
//! between `/my` and a collection, and `/my/collections/:id`'s habit of
//! detaching and re-attaching its entire DOM subtree after a `?q=` navigation.
//! Anything owned by a page would be disposed by all three. It is deliberately
//! **in-memory only** — a document load starts empty.
//!
//! **The key is the grain, and one of its two shapes is incomplete on
//! purpose.** [`SelectionKey`] is an enum, not a struct, because the two
//! surfaces that feed it know different amounts:
//!
//! - `/my/collections/:id` rows are `Held { collection, printing, board }` —
//!   the grain [`shared::MoveItem`] wants, minus the finish/condition/language
//!   it defaults;
//! - `/my` rows are `Card { oracle }` — that view aggregates every collection
//!   per *oracle* card and carries only a **representative** printing, so
//!   neither "from where" nor "which printing" is answerable from the row.
//!
//! An enum rather than `from_collection_id: Option<Id>` specifically so the
//! batch-move task cannot write `MoveItem { from_collection_id: None, .. }` by
//! accident: `None` there means *external intake* (copies appearing from
//! outside), which is the opposite of "we don't know yet". Resolving a `Card`
//! entry into move lines is [`crate::my::move_selection`]'s job, and it is done
//! server-side against the caller's real holdings.
//!
//! **The tray is the pill, not the dock, and it does not own its action.** This
//! component renders the wireframe's "Selection Tray" frame and nothing else;
//! the shell wraps it in the "Tray Wrap" frame that fixes it to the bottom of
//! the viewport, above the mobile tab bar. That split is what lets the bench
//! render it inline. The primary action ("Move to…") arrives as the [`action`]
//! slot rather than being built in, so this file stays free of server calls —
//! the shell passes [`crate::my::move_selection::MoveSelection`].
//!
//! [`action`]: SelectionTray

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use shared::{Board, Id};
use tw_merge::tw_merge;

use super::checkbox::Checkbox;

/// What one tray entry addresses — see the module docs on why this is an enum.
///
/// Serializable because it is also the **wire** shape the batch move takes
/// (`crate::move_selection`): the server resolves each key into a
/// [`shared::MoveItem`] or a stated refusal, so the "which copies did you mean"
/// question is answered once, against the database, rather than guessed by the
/// client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SelectionKey {
    /// A `/my/collections/:id` card row: copies of one printing, on one board,
    /// held in one collection. Board is carried because two rows of the same
    /// printing in the same deck (main vs sideboard) are two rows on screen and
    /// must be two checkboxes. It rode here for one task before the write could
    /// honor it; `move_cards`/`holding_take` are board-addressed now, so the key
    /// and the write finally agree.
    Held {
        collection_id: Id,
        printing_id: Id,
        board: Board,
    },
    /// A `/my` row: this oracle card, wherever its copies are.
    Card { oracle_id: Id },
}

impl SelectionKey {
    /// A stable, DOM-safe identifier — the `data-selection-key` attribute that
    /// lets a test tie a checked row to a tray entry.
    pub fn token(&self) -> String {
        match self {
            Self::Held {
                collection_id,
                printing_id,
                board,
            } => format!("held:{collection_id}:{printing_id}:{}", board.to_pg()),
            Self::Card { oracle_id } => format!("card:{oracle_id}"),
        }
    }
}

/// One selected row: what it addresses, plus the little the tray renders.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedCard {
    pub key: SelectionKey,
    /// The oracle card this row is a copy of. Not part of the key — two entries
    /// can share it (the same card in two collections) — but every selectable
    /// row knows it, and the move's destination ranking is per *oracle*
    /// (`suggested_destinations`), so carrying it here is what lets the picker
    /// rank without a second lookup.
    pub oracle_id: Id,
    pub name: String,
    pub image_uri: Option<String>,
}

/// The cross-view selection. `Copy`, so a row's checkbox closure can hold it.
#[derive(Debug, Clone, Copy)]
pub struct SelectionState {
    items: RwSignal<Vec<SelectedCard>>,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionState {
    pub fn new() -> Self {
        Self {
            items: RwSignal::new(Vec::new()),
        }
    }

    /// The selection in the order it was made — the order the thumbnail stack
    /// shows, so the newest pick is never the one silently dropped past three.
    pub fn items(self) -> RwSignal<Vec<SelectedCard>> {
        self.items
    }

    pub fn len(self) -> usize {
        self.items.with(Vec::len)
    }

    pub fn is_empty(self) -> bool {
        self.items.with(Vec::is_empty)
    }

    /// Reactive membership for one row's checkbox.
    pub fn selected(self, key: SelectionKey) -> Signal<bool> {
        Signal::derive(move || self.items.with(|v| v.iter().any(|c| c.key == key)))
    }

    /// Add the row if absent, drop it if present — the checkbox's whole job.
    pub fn toggle(self, card: SelectedCard) {
        self.items.update(|v| toggle_in(v, card));
    }

    pub fn clear(self) {
        self.items.update(Vec::clear);
    }

    /// Drop exactly the entries a batch move actually moved, named by
    /// [`SelectionKey::token`].
    ///
    /// Not `clear()`: a move can refuse part of the batch (a `/my` row held in
    /// two places, a sideboard row), and clearing everything would erase the
    /// refusals along with the successes — the user would see "some cards
    /// weren't moved" and have nothing left on screen to act on. What is left
    /// checked afterwards is exactly what still needs doing.
    pub fn remove_tokens(self, tokens: &[String]) {
        self.items.update(|v| retain_untokened(v, tokens));
    }
}

/// The removal itself, free of the signal so it is testable without a reactive
/// runtime (the [`toggle_in`] precedent).
fn retain_untokened(items: &mut Vec<SelectedCard>, tokens: &[String]) {
    items.retain(|c| !tokens.contains(&c.key.token()));
}

/// The toggle itself, free of the signal so it is testable without a reactive
/// runtime. Order is the selection order (the thumbnail stack's order), so a
/// removal closes the gap rather than shuffling.
fn toggle_in(items: &mut Vec<SelectedCard>, card: SelectedCard) {
    match items.iter().position(|c| c.key == card.key) {
        Some(i) => {
            items.remove(i);
        }
        None => items.push(card),
    }
}

/// Install the app-wide selection. Called once, by the shell, above the router
/// outlet — see the module docs.
pub fn provide_selection() -> SelectionState {
    let state = SelectionState::new();
    provide_context(state);
    state
}

/// The installed selection. Panics outside the shell, which is the honest
/// failure: a selectable row outside the shell has nowhere to accumulate into.
pub fn use_selection() -> SelectionState {
    expect_context::<SelectionState>()
}

/// How many thumbnails the stack shows before it stops (the wireframe draws
/// three).
const STACK: usize = 3;

/// One row's select control, wired to the shared selection.
#[component]
pub fn SelectionCheckbox(selection: SelectionState, card: SelectedCard) -> impl IntoView {
    let key = card.key;
    let checked = selection.selected(key);
    let aria_label = format!("Select {}", card.name);
    // The callback outlives this render (a row remounts on every route churn),
    // so the payload is stored rather than captured by move-clone per click.
    let card = StoredValue::new(card);

    view! {
        <Checkbox
            checked
            aria_label=aria_label
            on_checked_change=Callback::new(move |_| selection.toggle(card.get_value()))
            {..}
            data-testid="row-select"
            data-selection-key=key.token()
        />
    }
}

/// The wireframe's "Selection Tray" pill. Renders nothing at all when the
/// selection is empty — an empty tray is not a state this design has.
///
/// `action` is the pill's primary control — the wireframe's "Move to…". It is a
/// slot rather than a built-in button so this component stays free of server
/// calls and page context: the shell fills it with
/// [`crate::my::move_selection::MoveSelection`], and the bench fills it with the
/// same component under its own selection. Omitting it renders no action at all
/// (the honest rendering of "nothing is wired here"), never a dead button.
#[component]
pub fn SelectionTray(
    selection: SelectionState,
    #[prop(optional)] action: Option<ViewFn>,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let items = selection.items();
    // `Show`'s children is a `ChildrenFn` (re-invoked on every toggle), so
    // neither the merged class string nor the action slot can simply be moved
    // into it.
    let action = StoredValue::new(action);
    let merged = StoredValue::new(tw_merge!(
        "bg-foreground text-background flex items-center gap-3 rounded-[10px] px-3.5 py-2.5 shadow-lg",
        class
    ));

    view! {
        <Show when=move || !items.with(Vec::is_empty)>
            <div
                data-name="SelectionTray"
                data-testid="selection-tray"
                class=merged.get_value()
                role="region"
                aria-label="Selection"
            >
                <TrayStack items />
                <span
                    class="flex-1 text-sm font-medium"
                    aria-live="polite"
                    data-testid="tray-count"
                >
                    {move || count_label(items.with(Vec::len))}
                </span>
                {move || action.get_value().map(|a| a.run())}
                <button
                    type="button"
                    class="text-background/70 hover:text-background shrink-0 rounded-sm p-0.5 outline-none focus-visible:ring-2 focus-visible:ring-current"
                    aria-label="Clear selection"
                    data-testid="tray-clear"
                    on:click=move |_| selection.clear()
                >
                    // Lucide `x` (ISC), inlined — the icon convention the
                    // vendored checkbox established (no icon-crate dependency).
                    <svg
                        class="size-4"
                        xmlns="http://www.w3.org/2000/svg"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <path d="M18 6 6 18" />
                        <path d="m6 6 12 12" />
                    </svg>
                </button>
            </div>
        </Show>
    }
}

/// The overlapping thumbnail stack. Leftmost is topmost (the wireframe's
/// z-order), which plain document order gets backwards — hence the explicit
/// `z-index`.
#[component]
fn TrayStack(items: RwSignal<Vec<SelectedCard>>) -> impl IntoView {
    view! {
        <div class="flex shrink-0 items-center" aria-hidden="true" data-testid="tray-stack">
            {move || {
                items
                    .with(|v| {
                        v.iter()
                            .take(STACK)
                            .enumerate()
                            .map(|(i, card)| {
                                let class = format!(
                                    "border-background/40 bg-muted relative h-[30px] w-[22px] shrink-0 overflow-hidden rounded-[3px] border {}",
                                    if i == 0 { "" } else { "-ml-3" },
                                );
                                let style = format!("z-index:{}", STACK - i);
                                let src = card.image_uri.clone();
                                view! {
                                    <div class=class style=style data-testid="tray-thumb">
                                        {src
                                            .map(|src| {
                                                view! {
                                                    <img src=src alt="" class="h-full w-full object-cover" />
                                                }
                                            })}
                                    </div>
                                }
                            })
                            .collect_view()
                    })
            }}
        </div>
    }
}

/// `1 card` / `n cards` — the wireframe's count line.
fn count_label(n: usize) -> String {
    if n == 1 {
        "1 card".to_string()
    } else {
        format!("{n} cards")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Id {
        Id::from_u128(n)
    }

    fn card(key: SelectionKey, name: &str) -> SelectedCard {
        SelectedCard {
            key,
            oracle_id: match key {
                SelectionKey::Card { oracle_id } => oracle_id,
                SelectionKey::Held { printing_id, .. } => printing_id,
            },
            name: name.to_string(),
            image_uri: None,
        }
    }

    #[test]
    fn count_label_is_singular_at_one() {
        assert_eq!(count_label(0), "0 cards");
        assert_eq!(count_label(1), "1 card");
        assert_eq!(count_label(2), "2 cards");
    }

    #[test]
    fn tokens_separate_the_two_grains() {
        let held = SelectionKey::Held {
            collection_id: id(1),
            printing_id: id(2),
            board: Board::Side,
        };
        let by_card = SelectionKey::Card { oracle_id: id(1) };
        assert!(held.token().starts_with("held:"));
        assert!(held.token().ends_with(":side"));
        assert!(by_card.token().starts_with("card:"));
        assert_ne!(held.token(), by_card.token());
    }

    #[test]
    fn board_is_part_of_the_key() {
        // Two rows of the same printing in the same deck (main vs sideboard)
        // are two rows on screen, so they must be two selections — and the move
        // that consumes them now takes the board from the key it was given.
        let main = SelectionKey::Held {
            collection_id: id(1),
            printing_id: id(2),
            board: Board::Main,
        };
        let side = SelectionKey::Held {
            collection_id: id(1),
            printing_id: id(2),
            board: Board::Side,
        };
        assert_ne!(main, side);
        assert_ne!(main.token(), side.token());
    }

    #[test]
    fn a_my_row_and_a_collection_row_are_different_entries() {
        // `/my` selects a card wherever it is; a collection row selects the
        // copies in that collection. They address different things, so the
        // tray holds both rather than silently merging them.
        let anywhere = SelectionKey::Card { oracle_id: id(7) };
        let here = SelectionKey::Held {
            collection_id: id(1),
            printing_id: id(7),
            board: Board::Main,
        };
        assert_ne!(anywhere, here);
    }

    #[test]
    fn toggle_adds_removes_and_preserves_order() {
        let a = card(SelectionKey::Card { oracle_id: id(1) }, "Ancestral");
        let b = card(SelectionKey::Card { oracle_id: id(2) }, "Bolt");
        let c = card(SelectionKey::Card { oracle_id: id(3) }, "Counterspell");

        let mut items = Vec::new();
        toggle_in(&mut items, a.clone());
        toggle_in(&mut items, b.clone());
        toggle_in(&mut items, c.clone());
        assert_eq!(names(&items), vec!["Ancestral", "Bolt", "Counterspell"]);

        // Toggling an already-selected row removes it and leaves the rest in
        // order — the tray's thumbnail order is the selection order.
        toggle_in(&mut items, b.clone());
        assert_eq!(names(&items), vec!["Ancestral", "Counterspell"]);

        // …and re-selecting it puts it at the end, not back in its old slot.
        toggle_in(&mut items, b);
        assert_eq!(names(&items), vec!["Ancestral", "Counterspell", "Bolt"]);
    }

    #[test]
    fn the_same_card_from_two_surfaces_is_two_entries() {
        let mut items = Vec::new();
        toggle_in(
            &mut items,
            card(SelectionKey::Card { oracle_id: id(7) }, "Bolt"),
        );
        toggle_in(
            &mut items,
            card(
                SelectionKey::Held {
                    collection_id: id(1),
                    printing_id: id(7),
                    board: Board::Main,
                },
                "Bolt",
            ),
        );
        assert_eq!(items.len(), 2);
    }

    fn names(items: &[SelectedCard]) -> Vec<&str> {
        items.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn a_move_drops_only_what_it_moved() {
        // The refusals a batch move reports must stay checked: they are the
        // work still to do, and clearing the tray would leave the user with a
        // "2 weren't moved" toast and nothing on screen to act on.
        let moved = card(SelectionKey::Card { oracle_id: id(1) }, "Ancestral");
        let refused = card(SelectionKey::Card { oracle_id: id(2) }, "Bolt");
        let mut items = vec![moved.clone(), refused.clone()];

        retain_untokened(&mut items, &[moved.key.token()]);
        assert_eq!(names(&items), vec!["Bolt"]);

        // Idempotent: a re-fired callback (a double-clicked toast) is harmless.
        retain_untokened(&mut items, &[moved.key.token()]);
        assert_eq!(names(&items), vec!["Bolt"]);
    }

    #[test]
    fn removal_is_by_grain_not_by_card() {
        // The same printing on two boards is two entries; moving the mainboard
        // one must not silently drop the sideboard row with it.
        let main = card(
            SelectionKey::Held {
                collection_id: id(1),
                printing_id: id(2),
                board: Board::Main,
            },
            "Bolt",
        );
        let side = card(
            SelectionKey::Held {
                collection_id: id(1),
                printing_id: id(2),
                board: Board::Side,
            },
            "Bolt",
        );
        let mut items = vec![main.clone(), side.clone()];
        retain_untokened(&mut items, &[main.key.token()]);
        assert_eq!(items, vec![side]);
    }
}
