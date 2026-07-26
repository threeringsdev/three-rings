//! Bench section for the custom `CountStepper` gap component: the happy path
//! (hover-reveal ± / click-to-type / commit-on-blur / undo toast), a
//! failing-save row demonstrating the caller-revert contract, and a
//! caller-reported row where a committed 0 is the caller's to announce and undo.

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

    // Happy path: commits always "succeed". The caption mirrors the last
    // commit event so the probe can tell a pending step from a committed one;
    // the count exposes commit *cardinality* so a test can prove a session
    // commits exactly ONCE (not per click) — a lone `last` caption is
    // overwritten by duplicates and can't (Codex mutation pass, app-ui Findings).
    let basic = RwSignal::new(3);
    let last = RwSignal::new(String::from("—"));
    let commits = RwSignal::new(0u32);
    let on_basic = Callback::new(move |c: StepperCommit| {
        last.set(format!("{} → {}", c.from, c.to));
        commits.update(|n| *n += 1);
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

    // Caller-reported removal: a committed 0 deletes the thing being counted,
    // so its undo is a *different operation* and the stepper must not promise
    // its own. `caller_reports` hands those commits over whole — no toast from
    // the component — and this row raises the one message that is true, with an
    // Undo that restores the count the way the caller means to.
    let removable = RwSignal::new(2);
    let removed = RwSignal::new(false);
    let on_removable = Callback::new(move |c: StepperCommit| {
        if c.to > 0 {
            return;
        }
        removed.set(true);
        toast.show(
            ToastOptions::message(format!("Removed Counterspell ({} copies)", c.from))
                .kind(ToastKind::Success)
                .action(
                    "Undo",
                    Callback::new(move |()| {
                        removable.set(c.from);
                        removed.set(false);
                    }),
                ),
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
                    " · commits: "
                    <span data-testid="bench-stepper-count">{move || commits.get().to_string()}</span>
                </span>
            </div>
            <div id="bench-stepper-failing" class="flex items-center gap-4">
                <span class="w-40 text-sm">"Failing save"</span>
                <CountStepper value=failing label="Failing save" on_commit=on_failing />
            </div>
            <div id="bench-stepper-removable" class="flex items-center gap-4">
                <span class="w-40 text-sm">"Counterspell"</span>
                <Show
                    when=move || !removed.get()
                    fallback=move || {
                        view! {
                            <span
                                class="text-muted-foreground text-sm"
                                data-testid="bench-stepper-removed"
                            >
                                "removed"
                            </span>
                        }
                    }
                >
                    <CountStepper
                        value=removable
                        label="Counterspell"
                        on_commit=on_removable
                        caller_reports=Callback::new(|c: StepperCommit| c.to == 0)
                    />
                </Show>
            </div>
        </div>
    }
}
