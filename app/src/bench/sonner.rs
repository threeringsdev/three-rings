//! Bench section for the native `sonner` toaster (programmatic + undo action).

use leptos::prelude::*;

use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions, Toaster};

pub fn demo() -> AnyView {
    view! {
        // The Toaster is normally mounted once at the app root; the bench
        // mounts its own so the section is self-contained.
        <Toaster />
        <SonnerButtons />
    }
    .into_any()
}

#[component]
fn SonnerButtons() -> impl IntoView {
    let toast = expect_context::<ToastHandle>();

    view! {
        <div class="flex flex-wrap gap-2">
            <Button on:click=move |_| {
                toast.show(ToastOptions::message("Saved."));
            }>"Default"</Button>
            <Button
                variant=ButtonVariant::Secondary
                on:click=move |_| {
                    toast
                        .show(
                            ToastOptions::message("Moved 3 cards to Shoebox")
                                .action("Undo", Callback::new(|_| leptos::logging::log!("undo!"))),
                        );
                }
            >
                "With undo action"
            </Button>
            <Button
                variant=ButtonVariant::Destructive
                on:click=move |_| {
                    toast.show(ToastOptions::message("Move failed").kind(ToastKind::Error));
                }
            >
                "Error"
            </Button>
        </div>
    }
}
