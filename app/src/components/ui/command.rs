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
//! - **`CommandItem` scrolls itself into view (`block: "nearest"`) when it
//!   becomes highlighted** — upstream leaves ↑↓ nav to whatever the browser's
//!   default focus-follows-key scrolling does, which does nothing here since
//!   nothing but the highlight state actually moves DOM focus. Not visible at
//!   a handful of rows; ↑↓ across a list taller than `CommandList`'s `max-h`
//!   was otherwise blind past the first screenful (P6-137 review, once the
//!   set picker made a 1,047-row list a real case).
//! - **`CommandInput` takes optional `stale`/`on_stale_enter` props** (P6-138).
//!   Both default to nothing, so every consumer but the set picker is
//!   unaffected: Enter still always activates whatever the registry currently
//!   highlights. A consumer whose rows are re-keyed by something *other* than
//!   `Command`'s own (synchronous, per-keystroke) query — the set picker's
//!   rows come from a 250ms-debounced server fetch — can pass `stale` to tell
//!   `CommandInput` its rows may be answering an older term than what's in the
//!   box, and `on_stale_enter` to run instead of the built-in activate when
//!   that's true. See `SetPicker` in `app/src/catalog/rail.rs` and
//!   specs/app-ui.md's set-picker section, 2026-08-12.
//! - **`CommandEmpty` takes optional `loading`/`failed` props, each paired
//!   with a `ViewFn` slot (`loading_children`/`failed_children`)** (P6-011).
//!   Upstream — and every consumer before this — has exactly one signal for
//!   "show the empty line": zero registered items. That conflates three
//!   different worlds a consumer whose rows come from a server read actually
//!   has to tell apart — not fetched yet, the fetch failed, genuinely no
//!   rows — because an empty registry is what *all three* look like. With
//!   neither prop set, `CommandEmpty` renders identically to before this
//!   existed: pure registry inference, `children` in a `<div>` — though the
//!   element's *lifecycle* changed for every consumer: the `<div>` is now
//!   fully unmounted while items are visible, where it used to sit in the
//!   DOM under `display:none`. Setting
//!   either takes over, with precedence `failed` > `loading` >
//!   registry-inferred empty. The `*_children` slots render **instead of**
//!   that `<div>` — a full swap, the `<div>` un-mounted rather than merely
//!   hidden — so a caller can put its own `role`/`data-testid`/class directly
//!   on the node that lands in the DOM, and "the registry-inferred line is
//!   gone" is actually true rather than "hidden but still findable"
//!   (`ViewFn` is `Show`'s own `fallback` convention; defaults to rendering
//!   nothing — an honest blank beats a false "nothing here" claim when a
//!   caller sets the signal without a slot). This is also why `children`
//!   itself became `ChildrenFn` (`Fn`, not the usual `FnOnce`): a branch that
//!   can be un/re-mounted has to be able to reconstruct all three branches,
//!   registry-inferred included, not just call it once at setup.
//!   [`crate::catalog::destination::DestinationList`]'s `failed` arm is the
//!   first migration: its `role="alert"` error line moved onto
//!   `failed_children` verbatim (same sentence, same `data-testid`, same
//!   `role`) — and the un-mount (not hide) semantics are exactly what its
//!   e2e coverage needed: `states.spec.ts`'s tree-move-dialog test asserts
//!   the registry-inferred "No collection to move into." is *absent*
//!   (`toHaveCount(0)`) during a failure, which a hidden-but-present `<div>`
//!   would still satisfy for a visibility check but fails for a presence
//!   one — caught by that very test against an earlier, `style:display`-based
//!   version of this prop. **`loading` followed the same road (P6-163):**
//!   the tree's `Move to…` dialog put its own "Loading collections…" line
//!   inside a `Transition` `fallback` *alongside* `DestinationList`, so while
//!   the tree read was pending both that line and the registry-inferred "No
//!   collection to move into." rendered at once — an empty registry looks
//!   identical to a fetch that just hasn't landed, the same collapse
//!   `failed` already existed to end for the failure case. `DestinationList`
//!   now forwards a `loading` signal here too, with its own
//!   `loading_children` slot carrying the same sentence, so exactly one line
//!   is ever mounted. Two consumers stay off this on purpose: the rail's
//!   set picker (`catalog/rail.rs`, `SetPicker`) keeps its own four-arm
//!   match — it needs a retry affordance and a distinct "not yet engaged"
//!   state this primitive doesn't model, and its rows are server-filtered
//!   (`should_filter=false`), so `CommandEmpty` was never in its render path
//!   to begin with, only a comment referencing it. The ⌘K palette
//!   (`palette.rs`) is untouched too — pure filter semantics are the correct
//!   contract there, not a workaround to retire.
//! - **`CommandEmpty`'s "any item visible?" check reads the shared
//!   [`CommandContext::visible`] memo** instead of rescanning `items` itself
//!   (`ctx.items.get().iter().any(...)`, upstream's shape). The rescan was
//!   the one per-consumer O(n) pass the P6-137 review flagged as left open
//!   when the rest of the component moved onto the shared memo; harmless
//!   until it isn't, same story as the rest of that finding (see
//!   [`CommandContext::visible`]'s doc). `visible.with(|v| !v.is_empty())`
//!   is the same verdict, read off the already-computed list instead of
//!   walking `items` again.

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
    /// True when the currently-registered rows may be answering an older
    /// term than what the box holds right now — P6-138: the set picker's rows
    /// come from a 250ms-debounced server fetch, so mid-window the registry
    /// still reflects the *previous* term. `None` (every other consumer) means
    /// "never stale": Enter always activates the highlighted row, unchanged
    /// from before this prop existed.
    #[prop(optional)]
    stale: Option<Signal<bool>>,
    /// Runs instead of the built-in "activate the highlighted row" when
    /// `stale` reads `true` at Enter time. Ignored (Enter is simply swallowed,
    /// same as always — `prevent_default` still fires) when `stale` is absent
    /// or false. See the module doc's P6-138 deviation entry.
    #[prop(optional)]
    on_stale_enter: Option<Callback<()>>,
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
                if stale.map(|s| s.get()).unwrap_or(false) {
                    if let Some(cb) = on_stale_enter {
                        cb.run(());
                    }
                } else {
                    ctx.activate_highlighted();
                }
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
pub fn CommandEmpty(
    /// `Fn`, not `FnOnce` — unlike every other `children` in this file.
    /// `failed`/`loading` mean this component's rendered branch can flip
    /// (registry-empty ⇄ failed ⇄ loading) more than once over its lifetime,
    /// and each flip has to reconstruct whichever branch it lands on,
    /// `children` included — see the body's `move ||`.
    children: ChildrenFn,
    #[prop(optional, into)] class: String,
    /// True while the caller's rows haven't resolved yet — no fetch has
    /// settled either way, so the registry-inferred verdict below would be
    /// answering a question nobody asked yet. `None` (every consumer so far)
    /// is byte-identical to before this prop existed. See the module doc's
    /// `CommandEmpty` deviation entry for the full three-state contract.
    #[prop(optional)]
    loading: Option<Signal<bool>>,
    /// Rendered **instead of** the registry-inferred `<div>` — not nested
    /// inside it, and the `<div>` is not mounted at all while this is —
    /// while `loading` reads `true` and `failed` does not. Defaults to
    /// nothing (`ViewFn`'s own default, `Show`'s `fallback` convention): a
    /// caller that sets `loading` without this still gets an honest blank
    /// instead of a false "nothing here" claim.
    #[prop(optional, into)]
    loading_children: ViewFn,
    /// True when the read behind the caller's rows failed outright — a third
    /// world registry inference cannot tell apart from "nothing fetched yet"
    /// or "genuinely nothing matched", because zero registered items is what
    /// all three look like. Takes precedence over `loading`. `None` (every
    /// consumer so far) is byte-identical to before this prop existed.
    #[prop(optional)]
    failed: Option<Signal<bool>>,
    /// Rendered **instead of** the registry-inferred `<div>` — not nested
    /// inside it, and the `<div>` is not mounted at all while this is —
    /// while `failed` reads `true`, so a caller such as
    /// [`crate::catalog::destination::DestinationList`] can put its own
    /// `role`/`data-testid`/class directly on the element that ends up in the
    /// DOM, and an assertion that the registry-inferred line is *absent*
    /// (not just hidden) during a failure stays true. Defaults to nothing,
    /// same reasoning as `loading_children`.
    #[prop(optional, into)]
    failed_children: ViewFn,
) -> impl IntoView {
    let ctx = expect_context::<CommandContext>();
    let merged_class = StoredValue::new(tw_merge!("py-6 text-sm text-center", class));
    // Reads the shared `visible` memo rather than rescanning `ctx.items` —
    // see the module doc's `CommandEmpty` deviation entry and
    // [`CommandContext::visible`].
    let any_visible = Memo::new(move |_| ctx.visible.with(|v| !v.is_empty()));

    // A single reactive branch, not an always-mounted `<div>` toggled by
    // `style:display` (upstream's shape, and this component's own shape
    // before `failed`/`loading` existed). That div-plus-display approach
    // leaves the registry-inferred line **in the DOM, merely hidden**
    // whenever a caller's `failed`/`loading` branch is active instead — which
    // makes a `getByText("…")` / `toHaveCount(0)` assertion for the line a
    // caller's `failed` arm is supposed to have replaced find it anyway
    // (`display:none` is still present to the DOM, just not the paint tree).
    // Measured on `DestinationList`'s failed arm (`states.spec.ts`, the tree
    // move dialog): the hidden line was there and `toHaveCount(0)` saw it.
    // Full mount/unmount per branch — which is why `children` has to be `Fn`
    // — is what makes "replaced", not just "hidden", true.
    move || {
        if failed.map(|f| f.get()).unwrap_or(false) {
            return failed_children.run();
        }
        if loading.map(|l| l.get()).unwrap_or(false) {
            return loading_children.run();
        }
        if any_visible.get() {
            return ().into_any();
        }
        view! {
            <div data-name="CommandEmpty" class=merged_class.get_value()>
                {children()}
            </div>
        }
        .into_any()
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

    // Scrolls the highlighted row into view on ↑↓ (P6-137 review). Without
    // this, keyboard nav across a list taller than `CommandList`'s `max-h`
    // was blind past whatever fit on screen — harmless at the handful of rows
    // every consumer had, a real gap once the set picker's cap lifted to
    // ~1,047 (the whole point of a keyboard-reachable "every match" list).
    // `block: "nearest"` — not `"center"` — so a row already fully visible
    // never causes a jump; the browser scrolls the minimum needed. Hydrate-
    // only: `ScrollIntoViewOptions` needs `dep:web-sys`, unavailable in a
    // non-hydrate build, and scrolling has nothing to do during SSR (no DOM)
    // or before hydration attaches (nothing has moved yet).
    let node_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    Effect::new(move |_| {
        let is_highlighted = highlighted.get();
        #[cfg(feature = "hydrate")]
        if is_highlighted {
            if let Some(el) = node_ref.get() {
                let opts = web_sys::ScrollIntoViewOptions::new();
                opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
                el.scroll_into_view_with_scroll_into_view_options(&opts);
            }
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = is_highlighted;
    });

    let merged_class = tw_merge!(
        "group relative flex gap-2 items-center px-2 py-1.5 text-sm rounded-sm cursor-default select-none outline-none aria-selected:bg-accent aria-selected:text-accent-foreground hover:bg-accent hover:text-accent-foreground",
        class
    );

    view! {
        <div
            node_ref=node_ref
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
