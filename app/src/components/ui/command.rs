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
//! - **[`use_command_nav`]** publishes those same registry operations so a
//!   *foreign* input — one the feature renders itself — can drive the list.
//!   Quick-add needs it: `CommandInput`'s handler swallows Enter and the arrows
//!   without ever seeing a modifier, so a surface whose contract is
//!   `↑↓ ⏎ ⇧⏎ ⌥⏎` plus digit entry has to own its own `on:keydown`.
//!   `CommandInput` now goes through the same three operations, so the two
//!   paths cannot drift.
//! - `use_random_id` / counter IDs are gone: `CommandDialog` takes a
//!   deterministic caller `id` (it already did upstream) and drives open
//!   state through the vendored [`super::dialog`] instead of an inline script.
//! - **`CommandInput` takes an optional `id`** and `CommandDialog` forwards
//!   `should_filter`. Both are additive and exist for ⌘K: the palette focuses
//!   its field programmatically when the dialog opens (so the field needs a
//!   deterministic handle), and it ranks its own rows, so the primitive's
//!   substring filter has to be off — `Command` already had the knob and
//!   `CommandDialog` was simply hiding it.
//! - `leptos_ui`'s `clx!` swapped for the vendored clx.rs; the `icons` `Check`
//!   inlined (Lucide, ISC).
//! - **The registry's visible-id list is memoized once, shared by every
//!   item**, rather than each `CommandItem` recomputing it from scratch in its
//!   own `highlighted` `Memo` (upstream's shape, kept until now). Harmless at
//!   the small lists this component saw until the catalog set picker uncapped
//!   to ~1,050 rows (specs/app-ui.md, P6-137): the per-item recompute made a
//!   single `highlight` reset — which fires on every keystroke — an O(N²)
//!   pass, measured at ~31s for one keystroke against the full list in a debug
//!   build. See [`CommandContext::visible`].

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
    /// Visible item ids in registration order — **memoized once**, not
    /// recomputed by every [`CommandItem`] (P6-137 perf finding). Every
    /// mounted item used to call [`CommandContext::visible_ids`] fresh from
    /// its own `highlighted` `Memo`, which re-derives the whole filtered list
    /// from `items` on every dependency change; with N items registered, a
    /// single `highlight` reset (the [`Command`] effect fires one on *every*
    /// keystroke) reran N of those Memos, each doing O(N) work — O(N²) per
    /// keystroke. Harmless at the small lists every consumer had until the
    /// set picker's uncap (specs/app-ui.md, P6-137): a 1,047-row list froze
    /// typing for ~30s in a dev build (measured). Hoisting the filter into one
    /// shared `Memo` here — computed once per `items`/visibility change,
    /// read back with [`Memo::with`] (no per-read clone) — turns that into a
    /// single O(N) pass plus N O(1) lookups: O(N) total, not O(N²).
    visible: Memo<Vec<usize>>,
}

impl CommandContext {
    /// Visible item ids in registration order — the shared [`Self::visible`]
    /// memo's current value.
    ///
    /// **Registration happens in [`CommandItem`]'s component body, i.e. when the
    /// view is *constructed* — not when it is inserted.** So "registration
    /// order" is the order a consumer *builds* its rows in, which is only the
    /// DOM order if it builds them in the order it mounts them. Building two
    /// sections and then choosing which to place first silently breaks this
    /// (the ⌘K palette did exactly that; see its `group_views`).
    ///
    /// It equals DOM order for every consumer so far, each for its own checked
    /// reason:
    ///
    /// * **destination picker** — sorts its *data* before any item mounts and
    ///   only ever hides rows while typing (never reorders them);
    /// * **quick-add panel** — its candidate list is rebuilt inside a `Suspend`
    ///   per query, so each server result set is a full remount in document
    ///   order and the registry is rebuilt with it;
    /// * **⌘K palette** — it *ranks* its rows, so its order genuinely changes
    ///   per query, and it needs two things to stay honest. It forces a real
    ///   remount (its whole list is one `<For>` item keyed on the row set's
    ///   identity), *and* it decides its group order before constructing either
    ///   group. Both were measured, not reasoned: a plain dynamic closure leaves
    ///   the DOM nodes in place (an unkeyed positional diff) while re-running the
    ///   registrations, and building the groups eagerly reversed the registry
    ///   against the DOM whenever a command outranked every place.
    ///   `command-palette.spec.ts` pins both.
    /// * **set picker** — browses newest-first and hides nothing while typing
    ///   (server-filtered, `should_filter=false`), so order is exactly
    ///   registration order (specs/app-ui.md, P6-137).
    ///
    /// In-place keyed *reorder* of persistent items would diverge from DOM
    /// order and want a `compareDocumentPosition` sort here. No consumer does
    /// that — the palette is the one that ranks, and it remounts instead — so
    /// the sort is still deferred (noted in app-ui). Anything that starts
    /// reordering rows without remounting them needs it.
    fn visible_ids(&self) -> Vec<usize> {
        self.visible.get()
    }

    /// Move the highlight one row down, clamped at the last visible item.
    fn next(&self) {
        let len = self.visible_ids().len();
        if len == 0 {
            return;
        }
        self.highlight.update(|h| *h = (*h + 1).min(len - 1));
    }

    /// Move the highlight one row up, clamped at the first visible item.
    fn prev(&self) {
        if self.visible_ids().is_empty() {
            return;
        }
        self.highlight.update(|h| *h = h.saturating_sub(1));
    }

    /// Run the highlighted item's `on_select`; `false` when nothing is visible.
    fn activate_highlighted(&self) -> bool {
        let visible = self.visible_ids();
        if visible.is_empty() {
            return false;
        }
        let target = visible[self.highlight.get().min(visible.len() - 1)];
        match self
            .items
            .get_untracked()
            .into_iter()
            .find(|i| i.id == target)
        {
            Some(item) => {
                item.activate.run(());
                true
            }
            None => false,
        }
    }
}

/// Keyboard navigation over a [`Command`]'s item registry, for an input the
/// *feature* renders instead of [`CommandInput`] — see the module doc.
///
/// Obtained with [`use_command_nav`] from anywhere inside a [`Command`], so a
/// composite whose field is a sibling of its list still has to nest both under
/// the `Command` that owns the registry.
#[derive(Clone, Copy)]
pub struct CommandNav(CommandContext);

/// The [`CommandNav`] of the enclosing [`Command`], or `None` outside one.
pub fn use_command_nav() -> Option<CommandNav> {
    use_context::<CommandContext>().map(CommandNav)
}

impl CommandNav {
    /// Publish the query the foreign input holds. Filters the items when the
    /// `Command` filters, and — either way — resets the highlight to the first
    /// row, which is what makes "best match pre-highlighted" true after every
    /// keystroke rather than only the first.
    pub fn set_query(&self, query: impl Into<String>) {
        self.0.query.set(query.into());
    }

    pub fn next(&self) {
        self.0.next();
    }

    pub fn prev(&self) {
        self.0.prev();
    }

    /// Run the highlighted item's `on_select`. `false` when there is nothing
    /// visible to activate, so the caller can fall through to its own Enter
    /// behavior instead of swallowing the key.
    pub fn activate(&self) -> bool {
        self.0.activate_highlighted()
    }

    /// The highlighted row's index *among the visible items*, clamped to the
    /// last one — the same index [`CommandItem`] highlights itself by, so a
    /// caller rendering per-row affordances off it cannot disagree with the
    /// `aria-selected` the primitive emits.
    pub fn highlighted(&self) -> Signal<usize> {
        let ctx = self.0;
        Signal::derive(move || {
            let len = ctx.visible_ids().len();
            if len == 0 {
                0
            } else {
                ctx.highlight.get().min(len - 1)
            }
        })
    }

    /// How many items are currently visible.
    pub fn visible_count(&self) -> Signal<usize> {
        let ctx = self.0;
        Signal::derive(move || ctx.visible_ids().len())
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
    let items = RwSignal::new(Vec::new());
    // Computed once per `items`/visibility change and shared by every
    // `CommandItem` — see `CommandContext::visible` for why this is not
    // inlined back into each item's own `Memo`.
    let visible = Memo::new(move |_| {
        items
            .get()
            .into_iter()
            .filter(|i: &ItemReg| i.visible.get())
            .map(|i| i.id)
            .collect::<Vec<_>>()
    });
    let ctx = CommandContext {
        query: RwSignal::new(String::new()),
        should_filter,
        next_id: RwSignal::new(0),
        items,
        highlight: RwSignal::new(0),
        visible,
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
    /// Deterministic DOM id, for a caller that focuses the field itself (⌘K
    /// focuses it when the dialog opens). Omitted = no `id` attribute at all,
    /// rather than an empty one.
    #[prop(into, optional)]
    id: Option<String>,
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
        if ctx.visible_ids().is_empty() {
            return;
        }
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                ctx.next();
            }
            "ArrowUp" => {
                ev.prevent_default();
                ctx.prev();
            }
            "Enter" => {
                ev.prevent_default();
                ctx.activate_highlighted();
            }
            _ => {}
        }
    };

    view! {
        <input
            data-name="CommandInput"
            id=id
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
    //
    // `ctx.visible.with(...)` — not `ctx.visible_ids()` — on purpose: `with`
    // borrows the shared memo's cached `Vec` in place instead of cloning it,
    // so this Memo (one per mounted item, up to N of them) does O(1) work off
    // an already-computed list rather than an O(N) clone each. See
    // `CommandContext::visible`'s doc for the O(N²)-per-keystroke bug this
    // closes (P6-137).
    let highlighted = Memo::new(move |_| {
        ctx.visible.with(|visible| {
            if visible.is_empty() {
                return false;
            }
            let h = ctx.highlight.get().min(visible.len() - 1);
            visible[h] == id
        })
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
    /// Forwarded to [`Command`] — off for a caller that ranks and filters its
    /// own rows (⌘K), on for a plain client-filtered list.
    #[prop(default = true)]
    should_filter: bool,
) -> impl IntoView {
    use super::dialog::{Dialog, DialogContent};

    let merged_class = tw_merge!("p-0 sm:max-w-lg overflow-hidden", class);

    view! {
        <Dialog id=id open=open>
            <DialogContent class=merged_class show_close_button=false aria_label="Command palette">
                <Command class="min-h-80" should_filter=should_filter>
                    {children()}
                </Command>
            </DialogContent>
        </Dialog>
    }
}
