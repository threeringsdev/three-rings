//! Bench section for the custom `CountStepper` gap component: the happy path
//! (hover-reveal ± / click-to-type / commit-on-blur / undo toast) and a
//! failing-save row demonstrating the caller-revert contract.

use leptos::prelude::*;

use crate::components::ui::count_stepper::{CountStepper, StepperCommit};
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions, Toaster};

pub fn demo() -> AnyView {
    view! {
        // The Toaster is normally mounted once at the app root; the bench
        // mounts its own so the section is self-contained.
        <Toaster />
        <StepperRows />
    }
    .into_any()
}

#[component]
fn StepperRows() -> impl IntoView {
    let toast = expect_context::<ToastHandle>();

    // Happy path: commits always "succeed"; the caption mirrors the last
    // commit event so the probe can tell a pending step from a committed one.
    let basic = RwSignal::new(3);
    let last = RwSignal::new(String::from("—"));
    let on_basic = Callback::new(move |c: StepperCommit| {
        last.set(format!("{} → {}", c.from, c.to));
    });

    // Failing save: the pretend server rejects every commit a beat later —
    // the caller reverts the optimistic value and reports the error, which is
    // exactly the contract a real page implements.
    let failing = RwSignal::new(2);
    let on_failing = Callback::new(move |c: StepperCommit| {
        set_timeout(
            move || {
                failing.set(c.from);
                toast.show(
                    ToastOptions::message("Couldn't save count — reverted").kind(ToastKind::Error),
                );
            },
            std::time::Duration::from_millis(400),
        );
    });

    view! {
        <div class="flex flex-col gap-4">
            <div id="bench-stepper-basic" class="flex items-center gap-4">
                <span class="w-40 text-sm">"Lightning Bolt"</span>
                <CountStepper value=basic label="Lightning Bolt" on_commit=on_basic max=9 />
                <span class="text-xs text-muted-foreground">
                    "last commit: "
                    <span data-testid="bench-stepper-last">{move || last.get()}</span>
                </span>
            </div>
            <div id="bench-stepper-failing" class="flex items-center gap-4">
                <span class="w-40 text-sm">"Failing save"</span>
                <CountStepper value=failing label="Failing save" on_commit=on_failing />
            </div>
        </div>
    }
}
