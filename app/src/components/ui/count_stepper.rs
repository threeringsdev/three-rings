//! CountStepper — custom gap component №2 (design/component-gap-analysis.md;
//! specs/app-ui.md "Custom gap components"). Not a registry copy: no
//! number/stepper input exists upstream, so this composes the vendored
//! [`Button`] + [`Input`] and owns the interaction logic.
//!
//! Behavior (the collection-view in-place count editor):
//! - **Hover/focus-revealed `− n +`** — the ± buttons are invisible until the
//!   stepper (or an ancestor carrying `group/row`, e.g. a table row) is
//!   hovered or holds focus.
//! - **Click-to-type** — clicking the count swaps it for the vendored `Input`
//!   (focused, selected). Built on `Input`, not a raw `<input>`: `bind:value`
//!   emits no `value` attribute, and `Input` seeds it from the bound signal.
//! - **Keyboard ±** — `+`/`-`/arrows step while the stepper holds focus; in
//!   edit mode the native number input's arrow stepping takes over.
//! - **Commit-on-blur** — steps and typing accumulate in one editing session,
//!   shown immediately; the session commits once, when focus leaves the
//!   stepper (or on ⏎). ⎋ cancels the session. A parse failure on the typed
//!   text cancels rather than guessing.
//!
//! Commit contract: on commit the stepper writes the new count into `value`
//! (the optimistic update), raises the undo toast (`Toaster` must be mounted
//! above; undo re-commits the old count through the same channel), then fires
//! `on_commit(StepperCommit { from, to })`. The caller owns persistence: on
//! failure it sets `value` back and reports the error — the stepper knows
//! nothing about transport.
//!
//! Engine note: the ± buttons cancel `pointerdown` so a click steals no focus
//! — WebKit never focuses clicked buttons, so an uncancelled click would blur
//! the session's input (committing mid-session) on that engine while Chrome
//! kept it alive. When no focus is inside yet, the count element is focused
//! programmatically so blur-commit has an anchor in every engine.

use leptos::html;
use leptos::prelude::*;
use tw_merge::tw_merge;

use super::button::{Button, ButtonSize, ButtonVariant};
use super::input::{Input, InputType};
use super::sonner::{ToastHandle, ToastKind, ToastOptions};

/// One committed editing session. `from` was the committed count when the
/// session closed, `to` what it committed; `from != to` always.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepperCommit {
    pub from: i32,
    pub to: i32,
}

/// Reveal classes for the ± buttons: invisible until the stepper itself — or
/// a `group/row` ancestor (the future collection-view table row) — is hovered
/// or holds focus.
const REVEAL: &str = "opacity-0 transition-opacity group-hover/stepper:opacity-100 group-focus-within/stepper:opacity-100 group-hover/row:opacity-100 group-focus-within/row:opacity-100";

#[component]
pub fn CountStepper(
    /// The committed count, caller-owned. The stepper writes each commit into
    /// it optimistically; a caller whose persistence fails must set it back.
    value: RwSignal<i32>,
    /// What the count counts (the card name) — names the undo toast.
    #[prop(into)]
    label: String,
    /// Fired once per committed session, after the optimistic write.
    on_commit: Callback<StepperCommit>,
    /// Lower clamp (default 0 — quantity 0 means "delete the holding row").
    #[prop(optional)]
    min: i32,
    /// Optional upper clamp.
    #[prop(optional)]
    max: Option<i32>,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let toast = expect_context::<ToastHandle>();
    let label = StoredValue::new(label);

    // None = at rest. Some(n) = an open ±-session showing n uncommitted.
    let pending = RwSignal::new(None::<i32>);
    // Edit mode (click-to-type): the input's text is the session state.
    let editing = RwSignal::new(false);
    let text = RwSignal::new(String::new());

    let container_ref = NodeRef::<html::Div>::new();
    let value_ref = NodeRef::<html::Button>::new();
    let input_ref = NodeRef::<html::Input>::new();

    let clamp = move |v: i32| {
        let v = v.max(min);
        match max {
            Some(m) => v.min(m),
            None => v,
        }
    };
    let display = move || pending.get().unwrap_or_else(|| value.get());

    let step = move |delta: i32| {
        if editing.get_untracked() {
            let base = text
                .get_untracked()
                .trim()
                .parse::<i32>()
                .unwrap_or_else(|_| value.get_untracked());
            let next = clamp(base + delta);
            if next != base {
                text.set(next.to_string());
            }
        } else {
            let base = pending
                .get_untracked()
                .unwrap_or_else(|| value.get_untracked());
            let next = clamp(base + delta);
            // No dead session at a bound: opening a pending session at the
            // same value would reveal the ± controls and anchor focus only to
            // commit nothing. The ± buttons announce `aria-disabled` there, and
            // this makes the click actually inert to match.
            if next != base {
                pending.set(Some(next));
            }
        }
    };

    let enter_edit = move || {
        if !editing.get_untracked() {
            text.set(
                pending
                    .get_untracked()
                    .unwrap_or_else(|| value.get_untracked())
                    .to_string(),
            );
            editing.set(true);
        }
    };

    // Focus (+ select) the input once edit mode has mounted it.
    Effect::new(move |_| {
        if editing.get() {
            focus_and_select(input_ref);
        }
    });

    // Keyboard continuity after ⏎/⎋: the count `<button>` these paths should
    // land focus on is *unmounted* while editing (its NodeRef points at a
    // detached node), so the refocus has to wait for the remount — this
    // effect fires once the NodeRef refills. The input's own unmount-while-
    // focused fires a stray focusout, but session state is cleared before the
    // unmount, so the resulting commit_session call finds nothing to commit.
    let refocus = RwSignal::new(false);
    Effect::new(move |_| {
        if refocus.get() && !editing.get() && node_mounted(value_ref) {
            focus_button(value_ref);
            refocus.set(false);
        }
    });

    let cancel = move || {
        pending.set(None);
        editing.set(false);
        refocus.set(true);
    };

    let commit_session = move || {
        // The deferred blur path (below) fires this from a raw timer, which
        // outlives the reactive owner — so if a parent unmounted the row in the
        // meantime (a list refetch), the component's signals are disposed. Bail
        // rather than read them. Everything after the guard is synchronous, so
        // the owner can't vanish mid-body.
        let (Some(is_editing), Some(from)) =
            (editing.try_get_untracked(), value.try_get_untracked())
        else {
            return;
        };
        let target = if is_editing {
            text.get_untracked().trim().parse::<i32>().ok().map(clamp)
        } else {
            pending.get_untracked()
        };
        pending.set(None);
        editing.set(false);
        if let Some(to) = target {
            if to != from {
                // Read the component-owned `label` and build the toast message
                // *before* on_commit: a committed 0 is a deletion, and a caller
                // that removes the row synchronously would dispose `label`, so
                // reading it afterward would touch disposed state. on_commit
                // therefore fires last, after the optimistic write and toast.
                let message = format!("{}: {from} → {to}", label.get_value());
                value.set(to);
                let undo = Callback::new(move |()| {
                    // The row may have unmounted since (a refetch re-rendered
                    // the list); a disposed signal makes undo a no-op.
                    let Some(cur) = value.try_get_untracked() else {
                        return;
                    };
                    if cur != from {
                        value.set(from);
                        on_commit.run(StepperCommit {
                            from: cur,
                            to: from,
                        });
                    }
                });
                toast.show(
                    ToastOptions::message(message)
                        .kind(ToastKind::Success)
                        .action("Undo", undo),
                );
                on_commit.run(StepperCommit { from, to });
            }
        }
    };

    // Blur-commit, decided a beat late: the display⇄edit swap unmounts a
    // *focused* element, which fires a focusout with no relatedTarget — read
    // synchronously that is indistinguishable from focus leaving the stepper,
    // and committing there closes edit mode the instant it opens. So a
    // focusout that looks like an exit only schedules the commit; a 0ms
    // macrotask later (after the focus effects, which are microtasks) it
    // commits only if focus genuinely ended up outside.
    let on_focusout = move |ev: leptos::ev::FocusEvent| {
        if focus_left(container_ref, &ev) {
            set_timeout(
                move || {
                    if !holds_focus(container_ref) {
                        commit_session();
                    }
                },
                std::time::Duration::ZERO,
            );
        }
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();
        if editing.get_untracked() {
            match key.as_str() {
                "Enter" => {
                    ev.prevent_default();
                    commit_session();
                    refocus.set(true);
                }
                "Escape" => {
                    ev.prevent_default();
                    cancel();
                }
                // Arrows: the native number input steps (and clamps) itself.
                _ => {}
            }
        } else {
            match key.as_str() {
                "ArrowUp" | "+" | "=" => {
                    ev.prevent_default();
                    step(1);
                }
                "ArrowDown" | "-" => {
                    ev.prevent_default();
                    step(-1);
                }
                "Escape" => {
                    pending.set(None);
                }
                _ => {}
            }
        }
    };

    // Cancel pointerdown on the ± buttons: no focus steal (see module docs),
    // but make sure *something* inside anchors the session for blur-commit.
    let anchor_focus = move |ev: leptos::ev::PointerEvent| {
        ev.prevent_default();
        if !holds_focus(container_ref) {
            focus_button(value_ref);
        }
    };

    let dec_disabled = Signal::derive(move || (display() <= min).to_string());
    let inc_disabled = Signal::derive(move || max.is_some_and(|m| display() >= m).to_string());

    let merged = tw_merge!("group/stepper inline-flex items-center gap-0.5", class);
    let aria_label = format!("{} count", label.get_value());
    let input_aria = aria_label.clone();

    view! {
        <div
            data-name="CountStepper"
            data-testid="count-stepper"
            class=merged
            node_ref=container_ref
            on:focusout=on_focusout
            on:keydown=on_keydown
        >
            <Button
                variant=ButtonVariant::Ghost
                size=ButtonSize::IconXs
                class={REVEAL}
                {..}
                tabindex="-1"
                aria-label="Decrease"
                aria-disabled=dec_disabled
                data-testid="count-stepper-dec"
                on:pointerdown=anchor_focus
                on:click=move |_| step(-1)
            >
                "−"
            </Button>
            {move || {
                if editing.get() {
                    view! {
                        <Input
                            r#type=InputType::Number
                            bind_value=text
                            min=min.to_string()
                            class="h-6 w-12 min-w-0 px-1 py-0 text-right md:text-sm [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                            node_ref={input_ref}
                            {..}
                            // Native ArrowUp/Down step *and clamp* on the
                            // number input; without `max` it overshoots the
                            // upper bound until the commit clamps (min rides
                            // the typed prop above; the spread carries max
                            // because Input's `max` prop can't take an Option).
                            max=max.map(|m| m.to_string())
                            aria-label=input_aria.clone()
                            data-testid="count-stepper-input"
                        />
                    }
                        .into_any()
                } else {
                    view! {
                        <button
                            type="button"
                            role="spinbutton"
                            class="min-w-6 rounded px-1 text-right text-sm tabular-nums cursor-text outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                            aria-valuenow=move || display().to_string()
                            aria-valuemin=min.to_string()
                            aria-valuemax=max.map(|m| m.to_string())
                            aria-label=aria_label.clone()
                            data-testid="count-stepper-value"
                            node_ref=value_ref
                            on:click=move |_| enter_edit()
                        >
                            {move || display()}
                        </button>
                    }
                        .into_any()
                }
            }}
            <Button
                variant=ButtonVariant::Ghost
                size=ButtonSize::IconXs
                class={REVEAL}
                {..}
                tabindex="-1"
                aria-label="Increase"
                aria-disabled=inc_disabled
                data-testid="count-stepper-inc"
                on:pointerdown=anchor_focus
                on:click=move |_| step(1)
            >
                "+"
            </Button>
        </div>
    }
}

/// Whether a focusout means focus left the stepper entirely (→ commit).
/// Client-only; event handlers never run during SSR (same shape as
/// context_menu's `pointer_outside`).
#[allow(unused_variables, clippy::needless_return)]
fn focus_left(container: NodeRef<html::Div>, ev: &leptos::ev::FocusEvent) -> bool {
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        let Some(el) = container.get_untracked() else {
            return false;
        };
        return match ev.related_target() {
            Some(t) => !el.contains(Some(t.unchecked_ref::<web_sys::Node>())),
            None => true,
        };
    }
    #[cfg(not(feature = "hydrate"))]
    false
}

/// Whether the document's focus currently sits inside the stepper.
#[allow(unused_variables, clippy::needless_return)]
fn holds_focus(container: NodeRef<html::Div>) -> bool {
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        let Some(el) = container.get_untracked() else {
            return false;
        };
        let active = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element());
        return match active {
            Some(a) => el.contains(Some(a.unchecked_ref::<web_sys::Node>())),
            None => false,
        };
    }
    #[cfg(not(feature = "hydrate"))]
    false
}

/// Whether a NodeRef currently holds a mounted node (reactive — refills on
/// remount, which is what the refocus effect waits for).
#[allow(unused_variables, clippy::needless_return)]
fn node_mounted(node: NodeRef<html::Button>) -> bool {
    #[cfg(feature = "hydrate")]
    {
        return node.get().is_some();
    }
    #[cfg(not(feature = "hydrate"))]
    false
}

/// Focus the count element (the blur-commit anchor).
#[allow(unused_variables)]
fn focus_button(node: NodeRef<html::Button>) {
    #[cfg(feature = "hydrate")]
    if let Some(el) = node.get_untracked() {
        let _ = el.focus();
    }
}

/// Focus + select the edit input once mounted (reactive: the NodeRef fills
/// after the conditional view renders it).
#[allow(unused_variables)]
fn focus_and_select(node: NodeRef<html::Input>) {
    #[cfg(feature = "hydrate")]
    if let Some(el) = node.get() {
        let _ = el.focus();
        el.select();
    }
}
