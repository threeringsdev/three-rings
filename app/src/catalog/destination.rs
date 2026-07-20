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
        if let Some(doc) =
            leptos::tachys::dom::document().dyn_ref::<web_sys::HtmlDocument>()
        {
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
fn initial_destination(collections: &[CollectionSummary], remembered: Option<Id>) -> Option<Destination> {
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
fn picker_order(mut collections: Vec<CollectionSummary>) -> Vec<CollectionSummary> {
    collections.sort_by(|a, b| {
        b.is_inbox
            .cmp(&a.is_inbox)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    collections
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

    // Seed the choice from the resolved list exactly once — `state.set` inside
    // the resource read would re-run on every dependency change and stomp a
    // user's pick every time the list refetched.
    Effect::new(move |_| {
        if state.get_untracked().is_some() {
            return;
        }
        if let Some(Ok(list)) = collections.get() {
            state.set(initial_destination(&list, stored_destination_id()));
        }
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
                <Command class="rounded-md">
                    <CommandInput placeholder="Search collections…" />
                    <CommandList class="max-h-64 overflow-y-auto p-1">
                        <CommandEmpty class="text-muted-foreground p-3 text-sm">
                            "No collection matches."
                        </CommandEmpty>
                        <Transition fallback=|| {
                            view! {
                                <p class="text-muted-foreground p-3 text-sm">"Loading collections…"</p>
                            }
                        }>
                            {move || Suspend::new(async move {
                                let list = collections.await.unwrap_or_default();
                                picker_order(list)
                                    .into_iter()
                                    .map(|c| view! { <DestinationOption collection=c /> })
                                    .collect_view()
                            })}
                        </Transition>
                    </CommandList>
                </Command>
            </PopoverContent>
        </Popover>
    }
}

#[component]
fn DestinationOption(collection: CollectionSummary) -> impl IntoView {
    let state = expect_context::<DestinationState>().0;
    let open = use_popover_open();
    let dest = Destination {
        id: collection.id,
        name: collection.name.clone(),
        is_inbox: collection.is_inbox,
    };
    let value = collection.name.clone();
    let label = dest.label();
    let selected = {
        let dest = dest.clone();
        Memo::new(move |_| state.get().is_some_and(|d| d.id == dest.id))
    };

    let choose = Callback::new(move |()| {
        state.set(Some(dest.clone()));
        remember_destination(dest.id);
        // Choosing is the popover's whole purpose — leaving it open would make
        // every pick need a second dismiss.
        if let Some(open) = open {
            open.set(false);
        }
    });

    // The test seam and the chosen-marker ride an inner element, not the
    // `CommandItem` itself: it takes no attribute spread, and its own
    // `aria-selected` already means "keyboard-highlighted" — a different thing
    // from "this is the current destination". Overloading it would make the
    // primitive lie to a screen reader.
    view! {
        <CommandItem value=value on_select=choose class="cursor-pointer justify-between">
            <span
                class="truncate"
                data-testid="destination-option"
                data-chosen=move || selected.get().then_some("true")
            >
                {label}
            </span>
            {move || selected.get().then(|| view! { <span aria-hidden="true">"✓"</span> })}
        </CommandItem>
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
        let names: Vec<_> = picker_order(list)
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec!["Inbox", "Alpha", "zebra"]);
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
