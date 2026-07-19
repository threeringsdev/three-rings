//! Command — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/command.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. This is the shared core of quick-add, the
//! destination picker, and ⌘K, so its behavior is **fully reactive** — the
//! rewiring happens once here (specs/app-ui.md → Vendoring). Deviations from
//! upstream:
//! - **The parallel vanilla-`<script>`** (keyboard nav + filter, which fought
//!   the reactive path by also writing item visibility) is **gone**. Filter
//!   is a per-item `Memo`; ↑↓/⏎ navigation is a Leptos item registry driven
//!   from `CommandInput`. This is what lets features layer ⇧⏎/⌥⏎/count entry
//!   on top by reading key modifiers in their own handlers.
//! - `use_random_id` / counter IDs are gone: `CommandDialog` takes a
//!   deterministic caller `id` (it already did upstream) and drives open
//!   state through the vendored [`super::dialog`] instead of an inline script.
//! - `leptos_ui`'s `clx!` swapped for the vendored clx.rs; the `icons` `Check`
//!   inlined (Lucide, ISC).

use leptos::prelude::*;
use tw_merge::tw_merge;

use super::clx::clx;

mod components {
    use super::*;
    clx! {CommandHeader, div, "flex flex-col gap-2 text-center hidden sm:text-left"}
    clx! {CommandTitle, h2, "text-lg font-semibold leading-none"}
    clx! {CommandDescription, p, "text-sm text-muted-foreground"}
    clx! {CommandList, div, "overflow-y-auto overflow-x-hidden max-h-[300px] scroll-py-1 scroll-pt-2 scroll-pb-1.5"}
    clx! {CommandGroup, div, "overflow-hidden p-1 text-foreground"}
    clx! {CommandGroupLabel, div, "text-muted-foreground px-2 py-1.5 text-xs font-medium"}
    clx! {CommandFooter, footer, "flex gap-4 items-center px-4 h-10 text-xs font-medium rounded-b-xl border-t text-muted-foreground border-t-border bg-muted"}
}

pub use components::*;

/// One registered, currently-mounted item — the keyboard-navigation registry.
#[derive(Clone)]
struct ItemReg {
    id: usize,
    visible: Signal<bool>,
    activate: Callback<()>,
}

#[derive(Clone, Copy)]
struct CommandContext {
    query: RwSignal<String>,
    should_filter: bool,
    /// Monotonic id source for item registration.
    next_id: RwSignal<usize>,
    /// Live registry of mounted items (rebuilt as items mount/unmount).
    items: RwSignal<Vec<ItemReg>>,
    /// Index into the *visible* items of the currently-highlighted row.
    highlight: RwSignal<usize>,
}

impl CommandContext {
    /// Visible item ids in registration order — which equals DOM order for
    /// this component's consumers (all append-only: static client-filtered
    /// lists for quick-add / destination-picker / ⌘K, and full remounts on
    /// new server result sets). In-place keyed *reorder* of persistent items
    /// would diverge from DOM order and want a `compareDocumentPosition` sort
    /// here; no consumer does that, so it's deferred (noted in app-ui).
    fn visible_ids(&self) -> Vec<usize> {
        self.items
            .get()
            .into_iter()
            .filter(|i| i.visible.get())
            .map(|i| i.id)
            .collect()
    }
}

#[component]
pub fn Command(
    children: Children,
    #[prop(into, optional)] class: String,
    /// When false, disables client-side filtering (server-backed search:
    /// items are always "visible" and the server returns the filtered set).
    #[prop(default = true)]
    should_filter: bool,
) -> impl IntoView {
    let ctx = CommandContext {
        query: RwSignal::new(String::new()),
        should_filter,
        next_id: RwSignal::new(0),
        items: RwSignal::new(Vec::new()),
        highlight: RwSignal::new(0),
    };
    provide_context(ctx);

    // Reset the highlight to the first row whenever the query changes.
    Effect::new(move |_| {
        ctx.query.track();
        ctx.highlight.set(0);
    });

    let merged_class = tw_merge!(
        "flex overflow-hidden flex-col w-full h-full bg-transparent rounded-none text-popover-foreground",
        class
    );

    view! {
        <div data-name="Command" class=merged_class>
            {children()}
        </div>
    }
}

#[component]
pub fn CommandInput(
    #[prop(into, optional)] class: String,
    #[prop(into, optional)] placeholder: String,
    /// Fired on every keystroke — use for server-side search.
    #[prop(optional)]
    on_search_change: Option<Callback<String>>,
) -> impl IntoView {
    let ctx = expect_context::<CommandContext>();
    let merged_class = tw_merge!(
        "flex py-3 w-full h-10 text-sm bg-transparent rounded-md disabled:opacity-50 disabled:cursor-not-allowed placeholder:text-muted-foreground outline-hidden",
        class
    );

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let visible = ctx.visible_ids();
        if visible.is_empty() {
            return;
        }
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                ctx.highlight
                    .update(|h| *h = (*h + 1).min(visible.len() - 1));
            }
            "ArrowUp" => {
                ev.prevent_default();
                ctx.highlight.update(|h| *h = h.saturating_sub(1));
            }
            "Enter" => {
                ev.prevent_default();
                let h = ctx.highlight.get().min(visible.len() - 1);
                let target = visible[h];
                if let Some(item) = ctx
                    .items
                    .get_untracked()
                    .into_iter()
                    .find(|i| i.id == target)
                {
                    item.activate.run(());
                }
            }
            _ => {}
        }
    };

    view! {
        <input
            data-name="CommandInput"
            class=merged_class
            autocomplete="off"
            spellcheck="false"
            aria-autocomplete="list"
            role="combobox"
            aria-expanded="true"
            placeholder=placeholder
            type="text"
            prop:value=move || ctx.query.get()
            on:input=move |ev| {
                let value = event_target_value(&ev);
                ctx.query.set(value.clone());
                if let Some(callback) = on_search_change {
                    callback.run(value);
                }
            }
            on:keydown=on_keydown
            data-1p-ignore="true"
            data-lpignore="true"
        />
    }
}

#[component]
pub fn CommandEmpty(children: Children, #[prop(optional, into)] class: String) -> impl IntoView {
    let ctx = expect_context::<CommandContext>();
    let merged_class = tw_merge!("py-6 text-sm text-center", class);
    // Shown only when no item is visible (reactive — upstream did this with a
    // `:has()` CSS rule against inline display styles).
    let any_visible = Memo::new(move |_| ctx.items.get().iter().any(|i| i.visible.get()));

    view! {
        <div
            data-name="CommandEmpty"
            class=merged_class
            style:display=move || if any_visible.get() { "none" } else { "block" }
        >
            {children()}
        </div>
    }
}

#[component]
pub fn CommandItem(
    children: Children,
    #[prop(optional, into)] class: String,
    /// The text matched against the query for client-side filtering.
    #[prop(optional, into)]
    value: String,
    #[prop(optional)] on_select: Option<Callback<()>>,
) -> impl IntoView {
    let ctx = expect_context::<CommandContext>();
    let value_for_filter = value;

    let is_visible = Memo::new({
        let value = value_for_filter.clone();
        move |_| {
            if !ctx.should_filter {
                return true;
            }
            let search = ctx.query.get().to_lowercase();
            search.is_empty() || value.to_lowercase().contains(&search)
        }
    });

    // Register in the keyboard-nav registry on mount; deregister on cleanup so
    // server-driven remounts and conditional items stay consistent.
    let id = ctx.next_id.get_untracked();
    ctx.next_id.set(id + 1);
    let activate = Callback::new(move |_| {
        if let Some(cb) = on_select {
            cb.run(());
        }
    });
    ctx.items.update(|v| {
        v.push(ItemReg {
            id,
            visible: is_visible.into(),
            activate,
        });
    });
    on_cleanup(move || {
        ctx.items.update(|v| v.retain(|i| i.id != id));
    });

    // Highlighted when this id is the highlight-th visible item, with the
    // index clamped to the last visible row so a set that shrank (conditional
    // items / server results) beneath a stale highlight still shows one
    // selection instead of none.
    let highlighted = Memo::new(move |_| {
        let visible = ctx.visible_ids();
        if visible.is_empty() {
            return false;
        }
        let h = ctx.highlight.get().min(visible.len() - 1);
        visible[h] == id
    });

    let merged_class = tw_merge!(
        "group relative flex gap-2 items-center px-2 py-1.5 text-sm rounded-sm cursor-default select-none outline-none aria-selected:bg-accent aria-selected:text-accent-foreground hover:bg-accent hover:text-accent-foreground",
        class
    );

    view! {
        <div
            data-name="CommandItem"
            class=merged_class
            role="option"
            tabindex="-1"
            aria-selected=move || highlighted.get().to_string()
            style:display=move || if is_visible.get() { "flex" } else { "none" }
            on:click=move |_| activate.run(())
            on:mousemove=move |_| {
                // Point-to-highlight: sync the keyboard highlight to hover.
                let visible = ctx.visible_ids();
                if let Some(pos) = visible.iter().position(|&vid| vid == id) {
                    ctx.highlight.set(pos);
                }
            }
        >
            {children()}
        </div>
    }
}

/// Dialog-hosted command palette (⌘K). Wraps [`Command`] in the vendored
/// [`super::dialog`] so open state is Leptos-owned; the caller passes the
/// shared `open` signal (⌘K is bound at the app shell).
#[component]
pub fn CommandDialog(
    children: Children,
    #[prop(into)] id: String,
    open: RwSignal<bool>,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    use super::dialog::{Dialog, DialogContent};

    let merged_class = tw_merge!("p-0 sm:max-w-lg overflow-hidden", class);

    view! {
        <Dialog id=id open=open>
            <DialogContent class=merged_class show_close_button=false aria_label="Command palette">
                <Command class="min-h-80">{children()}</Command>
            </DialogContent>
        </Dialog>
    }
}
