//! The sticky **destination picker** — `Adding to: 📥 Inbox ▾` — and the
//! shared state behind it (specs/app-ui.md → `/catalog`).
//!
//! Two pieces that deliberately live apart:
//!
//! * [`DestinationState`], provided once by the app shell, so the choice
//!   survives every search, view switch, and route change *within* the shell.
//!   The wireframe's "persists across searches" is exactly this: the picker
//!   widget unmounts and remounts freely, the choice does not live in it.
//! * [`DestinationPicker`], the widget — a `popover` + `command` combobox, the
//!   third consumer of the reactive `command` core (quick-add and ⌘K are the
//!   others).
//!
//! Persistence is the `tr_dest` **cookie**, not localStorage, following
//! `theme_toggle`'s reasoning: a cookie is readable during SSR *and* in the
//! wasm, so the server renders the destination the user actually chose instead
//! of a placeholder that a corrective effect rewrites a frame later. It stores
//! the collection **id** only — the display name is always resolved from the
//! live collection list, so a rename can't leave a stale label in the toolbar.

use leptos::prelude::*;
use shared::{CollectionSummary, Id};

use crate::components::ui::command::{
    Command, CommandEmpty, CommandInput, CommandItem, CommandList,
};
use crate::components::ui::popover::{use_popover_open, Popover, PopoverContent, PopoverTrigger};
use crate::shell::CurrentUserResource;

/// The cookie holding the chosen destination's id. Not `httpOnly` — the wasm
/// half has to read it too (same rationale as `tr_theme`).
const DEST_COOKIE: &str = "tr_dest";

/// The picker's `popover` id. One instance per document, so a constant is both
/// deterministic (SSR and hydration must agree) and unambiguous.
const PICKER_ID: &str = "destination-picker";

/// Where `+ Want` / `+ Have` currently add. `None` until the collection list
/// resolves — quick actions stay disabled until then rather than guessing a
/// destination and adding somewhere the user didn't choose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub id: Id,
    pub name: String,
    pub is_inbox: bool,
}

impl Destination {
    /// `📥 Inbox` / `🗂 Shoebox` — the icon the wireframe puts in the trigger.
    pub fn label(&self) -> String {
        let icon = if self.is_inbox { "📥" } else { "🗂" };
        format!("{icon} {}", self.name)
    }
}

/// The app-wide destination choice. A newtype rather than a bare signal so
/// `expect_context` can't collide with any other `RwSignal<Option<_>>`.
#[derive(Clone, Copy)]
pub struct DestinationState(pub RwSignal<Option<Destination>>);

/// Provide the destination state. Called once, by the shell — see the module
/// docs for why it isn't the picker's own state.
pub fn provide_destination_state() {
    provide_context(DestinationState(RwSignal::new(None)));
}

/// The persisted destination id, read from `tr_dest` on whichever side we're
/// running: request headers during SSR, `document.cookie` in the wasm.
fn stored_destination_id() -> Option<Id> {
    fn parse(cookies: &str) -> Option<Id> {
        cookies
            .split(';')
            .filter_map(|c| c.trim().split_once('='))
            .find(|(k, _)| *k == DEST_COOKIE)
            .and_then(|(_, v)| v.parse().ok())
    }

    #[cfg(feature = "ssr")]
    {
        if let Some(parts) = use_context::<http::request::Parts>() {
            for header in parts.headers.get_all(http::header::COOKIE) {
                if let Some(id) = header.to_str().ok().and_then(parse) {
                    return Some(id);
                }
            }
        }
    }
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(id) = leptos::tachys::dom::document()
            .dyn_ref::<web_sys::HtmlDocument>()
            .and_then(|d| d.cookie().ok())
            .as_deref()
            .and_then(parse)
        {
            return Some(id);
        }
    }
    None
}

/// Persist the choice. A no-op outside the wasm — the server has no business
/// writing this, and SSR only ever *reads* what the browser last stored.
fn remember_destination(id: Id) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(doc) = leptos::tachys::dom::document().dyn_ref::<web_sys::HtmlDocument>() {
            let _ = doc.set_cookie(&format!(
                "{DEST_COOKIE}={id}; Path=/; Max-Age=31536000; SameSite=Lax"
            ));
        }
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = id;
    }
}

/// Pick the destination to start on: the remembered one if it still exists,
/// else the Inbox, else the first collection. Resolving against the live list
/// is what makes a deleted or renamed collection degrade gracefully instead of
/// leaving the picker pointing at nothing.
fn initial_destination(
    collections: &[CollectionSummary],
    remembered: Option<Id>,
) -> Option<Destination> {
    let chosen = remembered
        .and_then(|id| collections.iter().find(|c| c.id == id))
        .or_else(|| collections.iter().find(|c| c.is_inbox))
        .or_else(|| collections.first())?;
    Some(Destination {
        id: chosen.id,
        name: chosen.name.clone(),
        is_inbox: chosen.is_inbox,
    })
}

/// Bring the current choice back in line with a freshly-fetched list.
///
/// Three cases, and the middle one is the one that bites: nothing chosen yet →
/// seed; the chosen collection still exists → keep the *id* but refresh its
/// name and inbox flag, so a rename elsewhere shows up instead of leaving a
/// stale label; the chosen collection is gone → fall back as if nothing were
/// chosen, rather than keeping an id every add would `NotFound` on.
fn reconcile(
    collections: &[CollectionSummary],
    current: Option<Destination>,
    remembered: Option<Id>,
) -> Option<Destination> {
    match current {
        Some(current) => match collections.iter().find(|c| c.id == current.id) {
            Some(live) => Some(Destination {
                id: live.id,
                name: live.name.clone(),
                is_inbox: live.is_inbox,
            }),
            None => initial_destination(collections, remembered),
        },
        None => initial_destination(collections, remembered),
    }
}

/// Order the picker shows collections in: Inbox pinned to the top, then the
/// rest by name.
///
/// This sorts the *data*, before any item mounts — not the mounted rows.
/// `command`'s item registry is built in mount order and `visible_ids()`
/// returns that order, so ↑↓ only tracks visual order while the list is
/// append-only in document order (the caveat recorded against this task in
/// specs/TODO.md). Sorting here, then rendering once per resource load, keeps
/// that invariant: typing in the picker *hides* rows, it never reorders them,
/// and a new collection list remounts the whole list. No `compareDocumentPosition`
/// sort is needed in `command` for this consumer.
pub(crate) fn picker_order(mut collections: Vec<CollectionSummary>) -> Vec<CollectionSummary> {
    collections.sort_by(|a, b| {
        b.is_inbox
            .cmp(&a.is_inbox)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    collections
}

/// One row the picker offers: where it points, plus an optional right-hand
/// hint. The catalog toolbar has nothing to hint; the selection tray's copy of
/// this control shows each suggested destination's shortfall (`wants 3`), which
/// is the whole point of ranking by `suggested_destinations`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestinationChoice {
    pub dest: Destination,
    pub hint: Option<String>,
}

impl DestinationChoice {
    /// A row with no hint — the catalog toolbar's shape.
    pub fn plain(dest: Destination) -> Self {
        Self { dest, hint: None }
    }
}

/// The picker's body: a `command` combobox whose rows are the caller's.
///
/// Extracted from [`PickerBody`] so the selection tray's "Move to…" is *this*
/// control rather than a second one that drifts from it — same search box, same
/// `destination-option` rows, same ✓ marker, same close-on-pick. What the two
/// callers do not share is where the rows come from (the whole collection list
/// vs `suggested_destinations` ranked for the selection), so the rows are
/// `children`.
///
/// **The rows are `children` rather than a `Signal<Vec<…>>` for a hydration
/// reason, not a stylistic one.** Each caller's rows come from a `Resource`, and
/// a resource read in plain render is unresolved during SSR but *already
/// resolved* at hydration — so a signal-driven list renders zero items on the
/// server and N on the client, which is the tachys "expected an HTML `<div>`"
/// panic (observed, on `/catalog`). The `Suspense`/`Transition` boundary that
/// keeps the two in step has to sit around the rows, and it belongs to whoever
/// owns the resource. (The sticky picker goes one further and puts *this whole
/// component* inside its boundary — see [`PickerBody`] for why it has to.)
///
/// **`empty` can only ever speak about *filtering*.** `CommandEmpty` infers
/// emptiness from the item registry, and zero registered items conflates three
/// different worlds — not fetched, fetch failed, genuinely no collections — which
/// is exactly the collapse the set picker's four arms exist to refuse. So a
/// caller whose rows came from a **failed** read must say so through `failed`
/// rather than let this line answer for it: "No collection matches." over an
/// unreachable backend is a false claim about the user's own collections.
#[component]
pub fn DestinationList(
    /// The option rows — `DestinationOption`s, inside the caller's own
    /// async boundary.
    children: ChildrenFn,
    #[prop(into, default = String::from("Search collections…"))] placeholder: String,
    #[prop(into, default = String::from("No collection matches."))] empty: String,
    /// True when the read behind `children` failed. Replaces the `empty` line —
    /// which would otherwise be inferring from an empty registry — with the one
    /// sentence that is true.
    ///
    /// **Effect-written by every caller, so this is for client-only pickers.**
    /// Effects don't run during SSR, so a server render always takes the
    /// not-failed branch; a picker that SSRs must decide inside its own `Suspend`
    /// instead (again, [`PickerBody`]). The tray and the tree's `Move to…` both
    /// live behind client-only state (an empty selection renders no tray, a
    /// closed dialog renders no list), so neither is ever server-rendered.
    #[prop(into, optional)]
    failed: Signal<bool>,
    /// Deterministic DOM id for the search field, for a caller that focuses it
    /// itself. The tree's `Move to…` does: it opens in a dialog, and a dialog
    /// that focuses nothing leaves the keyboard path dead-ended. Omitted = no
    /// `id` attribute at all, matching `CommandInput`'s own contract.
    #[prop(optional_no_strip)]
    input_id: Option<String>,
) -> impl IntoView {
    // `Show`'s fallback is a `Fn`, so the line has to be cloneable out on every
    // call rather than moved once.
    let empty = StoredValue::new(empty);
    view! {
        <Command class="rounded-md">
            {match input_id {
                Some(id) => view! { <CommandInput id=id placeholder=placeholder.clone() /> }
                    .into_any(),
                None => view! { <CommandInput placeholder=placeholder.clone() /> }.into_any(),
            }}
            <CommandList class="max-h-64 overflow-y-auto p-1">
                <Show
                    when=move || failed.get()
                    fallback=move || {
                        view! {
                            <CommandEmpty class="text-muted-foreground p-3 text-sm">
                                {empty.get_value()}
                            </CommandEmpty>
                        }
                    }
                >
                    // One sentence, here rather than at each call site, so two
                    // pickers cannot describe the same outage differently. No
                    // retry: closing the panel loses nothing (the selection and
                    // the standing destination both outlive it), so unlike the
                    // sticky picker this arm is not a dead end.
                    <p
                        role="alert"
                        class="text-destructive p-3 text-sm"
                        data-testid="destination-error"
                    >
                        "Couldn't load your collections."
                    </p>
                </Show>
                {children()}
            </CommandList>
        </Command>
    }
}

/// The sticky picker. Renders only for a signed-in caller — an anonymous
/// visitor has no collections to add to, and their quick actions are sign-in
/// prompts rather than adds.
#[component]
pub fn DestinationPicker() -> impl IntoView {
    let user = expect_context::<CurrentUserResource>().0;
    view! {
        <Transition fallback=|| ()>
            {move || Suspend::new(async move {
                matches!(user.await, Ok(Some(_))).then(|| view! { <PickerBody /> })
            })}
        </Transition>
    }
}

#[component]
fn PickerBody() -> impl IntoView {
    let state = expect_context::<DestinationState>().0;
    let collections = Resource::new(|| (), |_| crate::list_collections());

    // Re-resolve against every list the resource yields — seeding once was not
    // enough. The state outlives this widget (it is the shell's), so a
    // collection renamed or deleted between two mounts would otherwise leave
    // the trigger showing a stale name, or quick-add pointed at an id the
    // server will answer `NotFound` for.
    Effect::new(move |_| {
        if let Some(Ok(list)) = collections.get() {
            let next = reconcile(&list, state.get_untracked(), stored_destination_id());
            // Only write on a real change: `set` notifies unconditionally, and
            // an identical write each refetch is churn every subscriber pays.
            if next != state.get_untracked() {
                state.set(next);
            }
        }
    });

    let chosen = Signal::derive(move || state.get().map(|d| d.id));
    let choose = Callback::new(move |dest: Destination| {
        let id = dest.id;
        state.set(Some(dest));
        remember_destination(id);
    });

    view! {
        <Popover id=PICKER_ID>
            <PopoverTrigger class="h-9 gap-1.5 px-3 text-sm">
                <span class="text-muted-foreground">"Adding to:"</span>
                <span class="font-medium" data-testid="destination-label">
                    {move || {
                        state
                            .get()
                            .map(|d| d.label())
                            .unwrap_or_else(|| "…".to_string())
                    }}
                </span>
                <span aria-hidden="true">"▾"</span>
            </PopoverTrigger>
            <PopoverContent class="w-[280px] p-0">
                // **The whole list is inside the boundary, not just its rows** —
                // the one picker of the three that has to be, because it is the
                // one that SSRs (`/catalog` renders it for any session). A
                // caller-side `failed` flag has to be Effect-written to stay off
                // the read-in-render trap, Effects do not run during SSR, and so
                // a server render of a failed read would emit the wrong arm into
                // the HTML. Deciding inside the `Suspend` — where the resource is
                // resolved on *both* sides — cannot disagree with itself.
                //
                // What that costs: a refetch rebuilds `CommandInput`, losing any
                // typed filter. Acceptable here and nowhere else: this resource's
                // source is `()`, so nothing refetches it except the retry below,
                // where a rebuilt list is the point.
                <Transition fallback=|| {
                    view! {
                        <p class="text-muted-foreground p-3 text-sm">"Loading collections…"</p>
                    }
                }>
                    {move || Suspend::new(async move {
                        // **Not `unwrap_or_default()`.** Collapsing the error into
                        // an empty list left `DestinationList` nothing to
                        // register, and its `CommandEmpty` then asserted "No
                        // collection matches." — a failed fetch claiming the user
                        // has no collections, the exact dishonesty the set
                        // picker's four arms were built to refuse. On the native
                        // backend an offline phone is the *ordinary* case for this
                        // read, so this arm is reached in normal use rather than
                        // only under fault injection.
                        match collections.await {
                            Err(e) => {
                                view! {
                                    <div class="space-y-1.5 p-3" data-testid="destination-error">
                                        <p role="alert" class="text-destructive text-sm">
                                            {format!(
                                                "Couldn't load your collections: {}",
                                                crate::components::states::describe(&e).1,
                                            )}
                                        </p>
                                        // Adds still work while this is on screen:
                                        // the destination is the shell's state,
                                        // remembered in a cookie, and quick-add
                                        // reads that and not this list. So this is
                                        // a failure to *change* destination, and
                                        // saying only "no collection matches"
                                        // misdescribed it twice over.
                                        <button
                                            type="button"
                                            class="text-muted-foreground hover:text-foreground text-sm underline"
                                            data-testid="destination-retry"
                                            on:click=move |_| collections.refetch()
                                        >
                                            "Try again"
                                        </button>
                                    </div>
                                }
                                    .into_any()
                            }
                            Ok(list) => {
                                view! {
                                    <DestinationList>
                                        {picker_order(list.clone())
                                            .into_iter()
                                            .map(|c| {
                                                let choice = DestinationChoice::plain(Destination {
                                                    id: c.id,
                                                    name: c.name,
                                                    is_inbox: c.is_inbox,
                                                });
                                                view! { <DestinationOption choice chosen on_choose=choose /> }
                                            })
                                            .collect_view()}
                                    </DestinationList>
                                }
                                    .into_any()
                            }
                        }
                    })}
                </Transition>
            </PopoverContent>
        </Popover>
    }
}

/// One row of [`DestinationList`] — its label, an optional right-hand hint, and
/// the ✓ that marks the standing choice.
///
/// Split out from [`DestinationOption`] for the tree's `Move to…`, whose list
/// has one row that is **not a collection** (`Top level`, i.e. `parent_id =
/// None`) and so has no [`Destination`] to carry. Everything a picker row *is*
/// — the `CommandItem`, the `destination-option` test seam, the ✓, the hint —
/// lives here, so the third consumer composes this markup instead of growing a
/// second copy of it that drifts.
#[component]
pub fn DestinationRow(
    /// What the row reads: `🗂 Shoebox`.
    #[prop(into)]
    label: String,
    /// The text `command`'s filter matches the typed query against.
    #[prop(into)]
    value: String,
    /// Right-hand hint (`wants 3`), or nothing.
    #[prop(optional_no_strip)]
    hint: Option<String>,
    /// Whether this row is the *current* choice (the ✓) — a different thing
    /// from `command`'s keyboard highlight, which is its `aria-selected`.
    #[prop(into, default = Signal::derive(|| false))]
    chosen: Signal<bool>,
    on_select: Callback<()>,
) -> impl IntoView {
    // The test seam and the chosen-marker ride an inner element, not the
    // `CommandItem` itself: it takes no attribute spread, and its own
    // `aria-selected` already means "keyboard-highlighted" — a different thing
    // from "this is the current destination". Overloading it would make the
    // primitive lie to a screen reader.
    view! {
        <CommandItem value=value on_select=on_select class="cursor-pointer justify-between">
            <span
                class="truncate"
                data-testid="destination-option"
                data-chosen=move || chosen.get().then_some("true")
            >
                {label}
            </span>
            {hint
                .map(|h| {
                    view! {
                        <span
                            class="text-muted-foreground shrink-0 text-xs"
                            data-testid="destination-hint"
                        >
                            {h}
                        </span>
                    }
                })}
            {move || chosen.get().then(|| view! { <span aria-hidden="true">"✓"</span> })}
        </CommandItem>
    }
}

/// One row of [`DestinationList`] that points at a **collection** — the shape
/// the catalog toolbar and the selection tray both use.
#[component]
pub fn DestinationOption(
    choice: DestinationChoice,
    /// Which destination carries the ✓ (the *current* choice, a different thing
    /// from `command`'s keyboard highlight). `None` for a picker with no
    /// standing choice, such as the tray's.
    #[prop(into, optional)]
    chosen: Signal<Option<Id>>,
    on_choose: Callback<Destination>,
) -> impl IntoView {
    let open = use_popover_open();
    let DestinationChoice { dest, hint } = choice;
    let value = dest.name.clone();
    let label = dest.label();
    let id = dest.id;
    let selected = Memo::new(move |_| chosen.get() == Some(id));

    let choose = Callback::new(move |()| {
        on_choose.run(dest.clone());
        // Choosing is the popover's whole purpose — leaving it open would make
        // every pick need a second dismiss.
        if let Some(open) = open {
            open.set(false);
        }
    });

    view! {
        <DestinationRow
            label=label
            value=value
            hint=hint
            chosen=Signal::derive(move || selected.get())
            on_select=choose
        />
    }
}

/// The signed-in caller's chosen destination, or `None` while the collection
/// list is still resolving. Quick actions read this; nothing else should reach
/// into the context directly.
pub fn current_destination() -> Signal<Option<Destination>> {
    let state = expect_context::<DestinationState>().0;
    Signal::derive(move || state.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(name: &str, is_inbox: bool) -> CollectionSummary {
        CollectionSummary {
            id: Id::new_v4(),
            parent_id: None,
            kind: shared::CollectionKind::Binder,
            name: name.to_string(),
            is_inbox,
            position: 0.0,
            format: None,
        }
    }

    #[test]
    fn remembered_destination_wins_when_it_still_exists() {
        let list = vec![collection("Inbox", true), collection("Shoebox", false)];
        let remembered = list[1].id;
        let chosen = initial_destination(&list, Some(remembered)).unwrap();
        assert_eq!(chosen.id, remembered);
        assert_eq!(chosen.name, "Shoebox");
    }

    #[test]
    fn deleted_remembered_collection_falls_back_to_inbox() {
        let list = vec![collection("Inbox", true), collection("Shoebox", false)];
        let chosen = initial_destination(&list, Some(Id::new_v4())).unwrap();
        assert!(chosen.is_inbox, "a stale cookie must not strand the picker");
    }

    #[test]
    fn without_an_inbox_the_first_collection_is_used() {
        let list = vec![collection("Zebra", false), collection("Alpha", false)];
        let chosen = initial_destination(&list, None).unwrap();
        assert_eq!(chosen.name, "Zebra", "list order, not name order");
    }

    #[test]
    fn no_collections_means_no_destination() {
        assert!(initial_destination(&[], None).is_none());
    }

    #[test]
    fn inbox_pins_to_the_top_and_the_rest_sort_by_name() {
        let list = vec![
            collection("zebra", false),
            collection("Alpha", false),
            collection("Inbox", true),
        ];
        let names: Vec<_> = picker_order(list).into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["Inbox", "Alpha", "zebra"]);
    }

    fn dest(c: &CollectionSummary) -> Destination {
        Destination {
            id: c.id,
            name: c.name.clone(),
            is_inbox: c.is_inbox,
        }
    }

    #[test]
    fn reconcile_picks_up_a_rename_without_losing_the_choice() {
        let mut list = vec![collection("Inbox", true), collection("Shoebox", false)];
        let chosen = dest(&list[1]);
        list[1].name = "Deck box".into();
        let after = reconcile(&list, Some(chosen.clone()), None).unwrap();
        assert_eq!(after.id, chosen.id, "the choice itself must survive");
        assert_eq!(after.name, "Deck box", "but its label must be live");
    }

    #[test]
    fn reconcile_falls_back_when_the_chosen_collection_is_deleted() {
        let list = [collection("Inbox", true), collection("Shoebox", false)];
        let chosen = dest(&list[1]);
        let remaining = [list[0].clone()];
        let after = reconcile(&remaining, Some(chosen), None).unwrap();
        assert!(
            after.is_inbox,
            "a deleted destination must not stay selected — every add would 404"
        );
    }

    #[test]
    fn reconcile_leaves_a_live_choice_alone() {
        let list = vec![collection("Inbox", true), collection("Shoebox", false)];
        let chosen = dest(&list[1]);
        // Even with a cookie pointing elsewhere: the in-session pick wins.
        let after = reconcile(&list, Some(chosen.clone()), Some(list[0].id)).unwrap();
        assert_eq!(after, chosen);
    }

    #[test]
    fn label_marks_the_inbox_distinctly() {
        let inbox = Destination {
            id: Id::new_v4(),
            name: "Inbox".into(),
            is_inbox: true,
        };
        let binder = Destination {
            id: Id::new_v4(),
            name: "Shoebox".into(),
            is_inbox: false,
        };
        assert_eq!(inbox.label(), "📥 Inbox");
        assert_eq!(binder.label(), "🗂 Shoebox");
    }
}
