//! Bench section for the **My-cards root list** (`app/src/my/root.rs`) — the
//! phone's `/my` (wireframes → *Mobile — My cards root*).
//!
//! It is an app composite over `Item` + `Separator`, not a vendored registry
//! component, so it is here for one reason the other app composites did not
//! have: **`/my` is unreachable on the Android emulator.** The Tauri dev proxy
//! strips Cookie headers, so every authed surface redirects to
//! `/login?next=…` — recorded three times over in specs/app-ui.md Findings, and
//! why the quick-add panel, the selection tray and the tree's touch menu are all
//! driven on this page instead. The rows are the drill-down: a tap has to reach
//! the `<a>` and navigate. That is what `probe:android-my-root` asserts, and it
//! needs the list to exist somewhere anonymous.
//!
//! The tree is a fixture, not a fetch — the bench page lives outside `AppShell`
//! and has no `CollectionTreeResource`. `root_rows` is the projection under
//! test either way, and its own unit tests cover the shapes a fixture can't.

use leptos::prelude::*;
use shared::{CollectionKind, CollectionSummary, CollectionTree, CollectionTreeRow, Id};

use crate::my::root::{root_rows, MyRootList, ALL_CARDS_PATH};
use crate::my::tree::assemble;

fn row(
    id: u128,
    parent: Option<u128>,
    name: &str,
    is_inbox: bool,
    present: i64,
) -> CollectionTreeRow {
    CollectionTreeRow {
        summary: CollectionSummary {
            id: Id::from_u128(id),
            parent_id: parent.map(Id::from_u128),
            kind: CollectionKind::Binder,
            name: name.into(),
            is_inbox,
            position: 0.0,
            format: None,
        },
        present,
        desired: 0,
    }
}

pub fn demo() -> AnyView {
    // The IA sketch's cast (information-architecture.md lines 21–34), with
    // `Inbox` returned last so the pin is visible on the page and not just in
    // the unit test.
    // `Trade` carries a nonzero `desired` — every other fixture row is
    // `desired: 0` (Adversarial review, this task), which is realistic for
    // none of them: a demo tree where nothing is ever wanted is a fixture
    // gap for any future bench section that reads it (the delete confirm's
    // wants count among them).
    let trade = CollectionTreeRow {
        desired: 6,
        ..row(2, Some(1), "Trade", false, 120)
    };
    let tree = assemble(CollectionTree {
        collections: vec![
            row(1, None, "Binders", false, 5),
            trade,
            row(3, Some(1), "Bulk", false, 520),
            row(4, None, "Decks", false, 72),
            row(5, Some(4), "Grixis", false, 100),
            row(6, None, "Inbox", true, 7),
        ],
        shopping_short: 2,
    });

    view! {
        <div class="space-y-3">
            <p class="text-muted-foreground text-sm">
                "The phone's "<code>"/my"</code>
                ": the sidebar's top level as a chevroned drill-down. Nested collections ("
                <code>"Trade"</code>", "<code>"Bulk"</code>", "<code>"Grixis"</code>
                ") are deliberately absent — you reach them by drilling in. Rows are real links, so a tap here navigates."
            </p>
            // Framed at the wireframe's own width so the row metrics (44 px tap
            // target, truncation, chevron alignment) are readable on a desktop
            // bench without resizing the window.
            <div class="bg-background w-[390px] max-w-full overflow-hidden rounded-xl border">
                <h3 class="px-4 pt-[18px] pb-2.5 text-xl font-semibold">"My cards"</h3>
                <MyRootList rows=root_rows(&tree, ALL_CARDS_PATH) />
            </div>
        </div>
    }
    .into_any()
}
