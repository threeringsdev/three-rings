//! Bench section for the vendored **`CommandDialog`** — which had none until
//! now — through its only consumer, the **⌘K command palette**
//! (`app/src/components/palette.rs`). The `command` section above benches
//! `Command` inline; the dialog wrapper, its scrim, its ESC gate and the
//! palette's grouped ranking were untested on this page.
//!
//! Two things the real surface cannot show here, each with a reason:
//!
//! * **The desktop gate is rendered as a readout.** The palette exists only on
//!   `(min-width: 768px) and (pointer: fine)` and only for a session, so on the
//!   real pages "is it absent because of the viewport, or because nobody is
//!   signed in?" is unanswerable — and the authed pages are unreachable through
//!   the Tauri Android *dev* proxy at all. The readout isolates the viewport
//!   half, so `probe:android-palette` can assert `false` on a real phone-width
//!   webview and a chromium test can assert it flips at the breakpoint.
//! * **The trigger is a button, not the chord.** The bench page lives outside
//!   `AppShell`, so the global ⌘K listener is not mounted here; wiring a second
//!   one would test the bench rather than the app.
//!
//! Rows are a static index — the bench has no collection tree — and `on_run`
//! reports the action instead of navigating.

use leptos::prelude::*;
use shared::Id;

use crate::components::palette::{
    desktop_signal, AtRest, PaletteAction, PaletteSurface, Place, PlaceKey,
};

pub fn demo() -> AnyView {
    let open = RwSignal::new(false);
    let ran = RwSignal::new(String::new());
    // The same signal the real gate uses — listened, so it follows a resize.
    let desktop = desktop_signal();

    // The wireframe's P1/P2 cast: two nested collections that share a prefix
    // (so `tra` has something to rank) plus the system places.
    let index: Vec<Place> = vec![
        Place {
            key: PlaceKey::AllCards,
            name: "All cards".into(),
            meta: String::new(),
            href: "/my".into(),
            icon: "🗂",
            default_row: true,
        },
        Place {
            key: PlaceKey::Collection(Id::from_u128(1)),
            name: "Trade Binder".into(),
            meta: "Binders".into(),
            href: "/my/collections/1".into(),
            icon: "🗂",
            default_row: false,
        },
        Place {
            key: PlaceKey::Collection(Id::from_u128(2)),
            name: "Trade duplicates".into(),
            meta: "Bulk Box".into(),
            href: "/my/collections/2".into(),
            icon: "🗂",
            default_row: false,
        },
        Place {
            key: PlaceKey::Collection(Id::from_u128(3)),
            name: "Grixis Control".into(),
            meta: "Decks".into(),
            href: "/my/collections/3".into(),
            icon: "🎴",
            default_row: false,
        },
        Place {
            key: PlaceKey::Shopping,
            name: "Shopping list".into(),
            meta: String::new(),
            href: "/my/shopping".into(),
            icon: "🛒",
            default_row: true,
        },
    ];
    let at_rest = AtRest {
        label: "Recent",
        // P1's three rows, in P1's order.
        places: vec![index[1].clone(), index[3].clone(), index[4].clone()],
    };
    let index_signal = Signal::derive({
        let index = index.clone();
        move || index.clone()
    });
    let at_rest_signal = Signal::derive(move || at_rest.clone());

    let on_run = Callback::new(move |action: PaletteAction| {
        ran.set(match action {
            PaletteAction::Go(href) => format!("go {href}"),
            PaletteAction::Run(cmd) => format!("run {}", cmd.label()),
        });
    });

    view! {
        <div class="space-y-3">
            <p class="text-muted-foreground max-w-2xl text-sm">
                "Frames P1 (at rest: RECENT + COMMANDS) and P2 (typed: COLLECTIONS + COMMANDS). "
                "Type to rank, ↑↓ to move, ⏎ to commit, esc to close. The real surface is opened by "
                "⌘K / Ctrl+K from the app shell — this page is outside it."
            </p>
            <div class="flex flex-wrap items-center gap-3">
                <button
                    id="bench-palette-open"
                    class="hover:bg-muted rounded-md border px-3 py-1.5 text-sm"
                    on:click=move |_| open.set(true)
                >
                    "Open palette"
                </button>
                <p class="text-muted-foreground text-xs">
                    "desktop gate: "
                    <span data-testid="bench-palette-desktop" class="font-medium">
                        {move || desktop.get().to_string()}
                    </span>
                </p>
                <p class="text-muted-foreground text-xs" data-testid="bench-palette-ran">
                    "ran: "
                    {move || ran.get()}
                </p>
            </div>
            <PaletteSurface open index=index_signal at_rest=at_rest_signal on_run />
        </div>
    }
    .into_any()
}
