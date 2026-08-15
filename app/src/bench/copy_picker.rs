//! Bench section for the which-copies picker's rows — the quantity-and-version
//! control the batch move asks with (P6-150; `my::move_selection`).
//!
//! **Why this section exists at all.** Every page that can open the real picker
//! is authed, and the Android dev webview cannot hold a session (the dev proxy
//! strips the cookie — ui-work-loop Findings, the same reason the tray, needs
//! and header-kebab probes all run through here). The picker is a *mobile*
//! control: a `− n +` per row, sized for a thumb, inside a panel that has to fit
//! a phone. So the bench is the only surface on which that can be exercised on
//! the real engine, and this section is what the Android probe drives.
//!
//! It mounts [`PickerRows`] — the dialog's own rows, with the real
//! [`CardSection`](crate::my::move_selection) markup and testids — over
//! synthetic stacks, and shows the live total the dialog's confirm button would
//! name. Nothing here talks to a server: the rows are a `Vec<CardChoices>` and
//! the answer is a `Vec<i32>`, exactly the two halves `picks_of` joins.

use leptos::prelude::*;
use shared::{Board, Condition, Finish, Id};

use crate::components::ui::selection_tray::SelectionKey;
use crate::my::move_selection::{AskedCard, CardChoices, CopyStack, PickerRows, SkipReason};

pub fn demo() -> AnyView {
    view! { <CopyPickerDemo /> }.into_any()
}

fn id(n: u128) -> Id {
    Id::from_u128(n)
}

fn stack(collection: u128, name: &str, quantity: i32) -> CopyStack {
    CopyStack {
        collection_id: id(collection),
        collection_name: name.to_string(),
        printing_id: id(100),
        printing: None,
        board: Board::Main,
        finish: Finish::Nonfoil,
        condition: Condition::Nm,
        language: shared::default_language(),
        quantity,
    }
}

fn card(oracle: u128, name: &str, reason: SkipReason, rows: Vec<CopyStack>) -> CardChoices {
    CardChoices {
        card: AskedCard {
            key: SelectionKey::Card {
                oracle_id: id(oracle),
            },
            oracle_id: id(oracle),
            name: name.to_string(),
            reason,
        },
        rows,
    }
}

#[component]
fn CopyPickerDemo() -> impl IntoView {
    // Three shapes worth looking at side by side: the plain "how many" case
    // (one stack, several copies), the split-grain case the whole story exists
    // for (two finishes of one printing, plus a sideboard row), and a card
    // whose stacks all vanished between the ask and this read.
    let sections = RwSignal::new(vec![
        card(
            1,
            "Lightning Bolt",
            SkipReason::Several(4),
            vec![stack(1, "Trade Binder", 4)],
        ),
        card(
            2,
            "Brainstorm",
            SkipReason::ManyCollections(3),
            vec![
                stack(1, "Trade Binder", 2),
                CopyStack {
                    finish: Finish::Foil,
                    printing: Some("MH3 #123".to_string()),
                    ..stack(1, "Trade Binder", 3)
                },
                CopyStack {
                    board: Board::Side,
                    condition: Condition::Lp,
                    language: "ja".to_string(),
                    ..stack(2, "Mono-Red Burn", 1)
                },
            ],
        ),
        card(3, "Counterspell", SkipReason::Several(2), Vec::new()),
    ]);
    let counts = RwSignal::new(vec![1; 4]);
    let total = Memo::new(move |_| counts.with(|v| v.iter().sum::<i32>()));

    view! {
        <div class="max-w-md space-y-4" data-testid="bench-copy-picker">
            <PickerRows sections counts />
            <p class="text-muted-foreground text-sm" data-testid="bench-copy-picker-total">
                {move || {
                    match total.get() {
                        0 => "Move copies".to_string(),
                        1 => "Move 1 copy".to_string(),
                        n => format!("Move {n} copies"),
                    }
                }}
            </p>
        </div>
    }
}
