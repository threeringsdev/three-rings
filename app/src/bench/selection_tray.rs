//! Bench section for the custom `SelectionTray` gap component: rows carrying
//! the real `SelectionCheckbox`, the tray they accumulate into, and the two
//! key grains side by side (a `/my`-shaped `Card` entry and two
//! `/my/collections/:id`-shaped `Held` entries that differ only by board).
//!
//! The bench mounts its **own** `SelectionState` rather than the shell's — the
//! bench lives outside the app shell, and a section that shared the real
//! selection would let a poke here follow you onto `/my`.
//!
//! Docking is not benched: the fixed "Tray Wrap" wrapper lives in the shell
//! (`shell::SelectionTrayDock`), so this section renders the pill inline. The
//! above-the-tab-bar docking is asserted in `end2end/tests/selection-tray.spec.ts`
//! against the real pages.

use leptos::prelude::*;
use shared::{Board, Id};

use crate::components::ui::selection_tray::{
    SelectedCard, SelectionCheckbox, SelectionKey, SelectionState, SelectionTray,
};

pub fn demo() -> AnyView {
    view! { <TrayDemo /> }.into_any()
}

/// A 22×30 placeholder "card back", as a self-contained `data:` URI — the
/// bench must not hotlink Scryfall to prove the thumbnail path renders an
/// `<img>` at all.
const ART: &str = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='22' height='30'><rect width='22' height='30' fill='%237c3aed'/><circle cx='11' cy='15' r='6' fill='%23fbbf24'/></svg>";

fn id(n: u128) -> Id {
    Id::from_u128(n)
}

#[component]
fn TrayDemo() -> impl IntoView {
    let selection = SelectionState::new();

    let rows = vec![
        (
            "Lightning Bolt — /my row (oracle grain)",
            SelectedCard {
                key: SelectionKey::Card { oracle_id: id(1) },
                name: "Lightning Bolt".into(),
                image_uri: Some(ART.into()),
            },
        ),
        (
            "Lightning Bolt — Trade Binder, main",
            SelectedCard {
                key: SelectionKey::Held {
                    collection_id: id(10),
                    printing_id: id(100),
                    board: Board::Main,
                },
                name: "Lightning Bolt".into(),
                image_uri: Some(ART.into()),
            },
        ),
        (
            "Lightning Bolt — Trade Binder, sideboard",
            SelectedCard {
                key: SelectionKey::Held {
                    collection_id: id(10),
                    printing_id: id(100),
                    board: Board::Side,
                },
                name: "Lightning Bolt".into(),
                image_uri: None,
            },
        ),
        (
            "Counterspell — Blue Deck, main (no art)",
            SelectedCard {
                key: SelectionKey::Held {
                    collection_id: id(11),
                    printing_id: id(101),
                    board: Board::Main,
                },
                name: "Counterspell".into(),
                image_uri: None,
            },
        ),
    ];

    view! {
        <div class="flex flex-col gap-4">
            <p class="text-muted-foreground text-sm">
                "Four rows, three grains. The last two boxes differ only by board — they are two
                entries, because they are two rows on screen. The stack shows at most three."
            </p>
            <div id="bench-tray-rows" class="flex flex-col gap-2">
                {rows
                    .into_iter()
                    .map(|(label, card)| {
                        view! {
                            <div class="flex items-center gap-3 text-sm">
                                <SelectionCheckbox selection card />
                                <span>{label}</span>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
            <p class="text-muted-foreground text-xs">
                "Selected: "
                <span data-testid="bench-tray-selected">
                    {move || selection.len().to_string()}
                </span>
            </p>
            // Inline, not docked — see the module docs.
            <div id="bench-tray">
                <SelectionTray selection />
            </div>
        </div>
    }
}
