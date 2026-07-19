//! Sonner (toaster) — a small **native Leptos** toaster, NOT a vendored copy
//! of the registry's `sonner.rs` (maintainer decision, app-ui.md Open
//! questions: undo-on-toast wants first-class Leptos state; upstream's Rust
//! side is markup that declaratively triggers a separate `sonner.js` engine
//! we don't ship, and its own unfinished `_sonner_leptos_only_later/` points
//! the same way). The API shape (Toaster container + programmatic toasts with
//! an optional action) follows the registry so callers read familiarly.
//!
//! Usage: mount one [`Toaster`] near the app root (it `provide_context`s a
//! [`ToastHandle`]); anywhere beneath, `expect_context::<ToastHandle>()` and
//! call `.show(...)`. The undo toast fired after a move is the motivating
//! case — a toast carries an optional action button.

use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastKind {
    #[default]
    Default,
    Success,
    Error,
}

impl ToastKind {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "bg-popover text-popover-foreground border",
            Self::Success => "bg-primary text-primary-foreground",
            Self::Error => "bg-destructive text-white",
        }
    }
}

/// One toast's optional action button (e.g. "Undo").
#[derive(Clone)]
pub struct ToastAction {
    pub label: String,
    pub on_action: Callback<()>,
}

#[derive(Clone)]
struct Toast {
    id: usize,
    message: String,
    kind: ToastKind,
    action: Option<ToastAction>,
}

/// Programmatic toast handle, provided by [`Toaster`] via context.
#[derive(Clone, Copy)]
pub struct ToastHandle {
    toasts: RwSignal<Vec<Toast>>,
    next_id: RwSignal<usize>,
}

/// How a toast is fired — message, kind, optional action, auto-dismiss ms.
pub struct ToastOptions {
    pub message: String,
    pub kind: ToastKind,
    pub action: Option<ToastAction>,
    /// Auto-dismiss delay; `0` = sticky until dismissed.
    pub duration_ms: u64,
}

impl ToastOptions {
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            kind: ToastKind::Default,
            action: None,
            duration_ms: 5000,
        }
    }

    pub fn kind(mut self, kind: ToastKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn action(mut self, label: impl Into<String>, on_action: Callback<()>) -> Self {
        self.action = Some(ToastAction {
            label: label.into(),
            on_action,
        });
        self
    }
}

impl ToastHandle {
    /// Fire a toast; auto-dismisses after `duration_ms` (unless 0).
    pub fn show(&self, opts: ToastOptions) -> usize {
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        self.toasts.update(|t| {
            t.push(Toast {
                id,
                message: opts.message,
                kind: opts.kind,
                action: opts.action,
            });
        });
        if opts.duration_ms > 0 {
            let toasts = self.toasts;
            let _ = leptos::prelude::set_timeout_with_handle(
                move || toasts.update(|t| t.retain(|x| x.id != id)),
                std::time::Duration::from_millis(opts.duration_ms),
            );
        }
        id
    }

    /// Dismiss a toast by id (used by the close button and after an action).
    pub fn dismiss(&self, id: usize) {
        self.toasts.update(|t| t.retain(|x| x.id != id));
    }
}

/// The toaster container. Mount once near the app root. Bottom-right,
/// stacking upward.
#[component]
pub fn Toaster() -> impl IntoView {
    let handle = ToastHandle {
        toasts: RwSignal::new(Vec::new()),
        next_id: RwSignal::new(0),
    };
    provide_context(handle);

    view! {
        <ol
            data-name="Toaster"
            aria-live="polite"
            class="fixed bottom-6 right-6 z-[200] flex w-[360px] max-w-[calc(100vw-2rem)] flex-col gap-2 pointer-events-none [&>*]:pointer-events-auto"
        >
            <For each=move || handle.toasts.get() key=|t| t.id let:toast>
                {
                    let id = toast.id;
                    let action = toast.action.clone();
                    view! {
                        <li
                            data-name="Toast"
                            class=format!(
                                "flex items-center gap-3 rounded-md px-4 py-3 text-sm shadow-lg {}",
                                toast.kind.classes(),
                            )
                        >
                            <span class="flex-1">{toast.message.clone()}</span>
                            {action
                                .map(|a| {
                                    let on_action = a.on_action;
                                    view! {
                                        <button
                                            type="button"
                                            class="shrink-0 rounded px-2 py-1 text-xs font-semibold underline underline-offset-2 hover:opacity-80"
                                            on:click=move |_| {
                                                on_action.run(());
                                                handle.dismiss(id);
                                            }
                                        >
                                            {a.label}
                                        </button>
                                    }
                                })}
                            <button
                                type="button"
                                aria-label="Dismiss"
                                class="shrink-0 rounded p-1 opacity-70 hover:opacity-100"
                                on:click=move |_| handle.dismiss(id)
                            >
                                <svg
                                    class="size-3.5"
                                    xmlns="http://www.w3.org/2000/svg"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path d="M18 6 6 18" />
                                    <path d="m6 6 12 12" />
                                </svg>
                            </button>
                        </li>
                    }
                }
            </For>
        </ol>
    }
}
