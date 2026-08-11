//! Bench section for the vendored `dialog` (Leptos-rewired open state).
//! The "Confirm move…" dialog also doubles as the Tab focus trap's demo
//! (P6-125): three tabbable stops (the close button, Cancel, Move) — enough
//! to exercise a forward wrap (Move → close button), a backward wrap
//! (close button → Move via Shift+Tab), and an ordinary in-between Tab
//! (close button → Cancel) without adding dedicated markup. Driven by
//! `end2end/bench-check.mjs`.

use leptos::prelude::*;

use crate::components::ui::button::ButtonVariant;
use crate::components::ui::dialog::{
    Dialog, DialogAction, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter,
    DialogHeader, DialogTitle, DialogTrigger,
};

pub fn demo() -> AnyView {
    // A shared signal so the bench can also prove programmatic open.
    let open = RwSignal::new(false);

    view! {
        <div class="space-y-3">
            <Dialog id="bench-dialog" open=open>
                <DialogTrigger>"Confirm move…"</DialogTrigger>
                <DialogContent aria_label="Move cards">
                    <DialogBody>
                        <DialogHeader>
                            <DialogTitle>"Move 3 cards?"</DialogTitle>
                            <DialogDescription>
                                "They'll move from Trade Binder to Shoebox. You can undo this."
                            </DialogDescription>
                        </DialogHeader>
                        <DialogFooter>
                            <DialogClose>"Cancel"</DialogClose>
                            <DialogAction variant=ButtonVariant::Default>"Move"</DialogAction>
                        </DialogFooter>
                    </DialogBody>
                </DialogContent>
            </Dialog>
            <button
                class="text-muted-foreground text-xs underline"
                id="bench-dialog-programmatic"
                on:click=move |_| open.set(true)
            >
                "open programmatically (the m-key path)"
            </button>
            <p class="text-muted-foreground text-xs">
                "Open, then Tab: close button → Cancel → Move → wraps to close button. Shift+Tab from the close button wraps to Move."
            </p>
        </div>
    }
    .into_any()
}
