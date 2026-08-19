//! The **quick-add panel** — the keyboard-first intake surface
//! (specs/app-ui.md → "Quick-add panel"; design/add-flow-prototype.md and the
//! `Proto — Add flow` storyboards in design/wireframes.pen).
//!
//! One field, one floating panel, and a fixed keystroke contract:
//!
//! ```text
//! ↑↓ navigate · ⏎ add 1 here · ⇧⏎ set count · ⌥⏎ want instead
//! ```
//!
//! Five things are worth knowing before editing this file.
//!
//! **The field is the page's existing quick search, not a second box.** The
//! storyboard's `Add or find cards…` field *is* the collection header's search
//! (M1), so this composite wraps [`QueryBar`] rather than growing a rival input:
//! typing still filters the collection through `?q=`, and the panel hangs off
//! the same text. That is also why there is no debounce here — the URL already
//! has one, and keying the candidate search on the *committed* query means the
//! rows in `IN THIS COLLECTION` and the ones under `ADD FROM CATALOG` are always
//! answers to the same question.
//!
//! **Only the catalog candidates are keyboard targets.** The storyboard
//! pre-highlights the best *catalog* match with a row above it in
//! `IN THIS COLLECTION` left unhighlighted (S2), so the present rows are
//! context — "you already have three here" — rendered as plain links, not
//! [`CommandItem`]s. Highlight index 0 is therefore the best candidate by
//! construction, and `command`'s mount-order registry needs no
//! `compareDocumentPosition` sort: the candidate list is rebuilt inside a
//! `Suspend` per query, so every result set is a full remount in document order.
//!
//! **⏎ adds what you can see, not what the resource holds.** The commit sets
//! the kind/quantity for the *next* activation and then asks `command` to
//! activate its highlighted item, whose `on_select` closes over its own card.
//! Indexing a snapshot of the resource instead would let a result set that
//! landed mid-keystroke add a different card than the one under the highlight.
//!
//! **The panel is client-only.** Its contents mount under a `Show` gated on
//! `open`, which no server render can set — an overlay driven by focus and
//! keystrokes has nothing to SSR, and the gate is what keeps a resource read
//! from disagreeing between the `SsrMode::Async` render and hydration.
//!
//! **This panel is deliberately *not* the vendored `popover`,** which the spec's
//! composite names and which the destination picker uses happily. Two measured
//! failures made the native Popover API the wrong substrate *here*, both because
//! this surface is anchored to a field on a page that navigates as you type:
//!
//! 1. **The click that focuses the field dismisses the panel.** A
//!    `popover="auto"` light-dismisses on `pointerup` when the pointer went down
//!    outside it. Opening on `focusin` therefore showed the panel and the same
//!    click closed it again (observed: `showPopover` returns `Ok`, then a
//!    `toggle` back to `closed`).
//! 2. **A same-page navigation closed it silently.** On every `?q=` change —
//!    which is every debounced keystroke here — this page's whole subtree was
//!    removed and re-inserted, and removing a showing popover from the document
//!    hides it *without* firing `toggle` (HTML's popover removing steps pass
//!    `fireEvents = false`). The Rust `open` signal stayed `true` while the panel
//!    was gone, so nothing could even notice and re-show it. **This was not the
//!    router**, as this note used to claim: `CollectionPage` read its own
//!    `Resource` in its setup body, which registers on the nearest
//!    `SuspenseContext` — the `RequireAuth` `<Suspense>` above the whole `/my/*`
//!    tree — so a re-search re-suspended the auth guard and unmounted its
//!    `<Outlet/>`. Fixed in P6-068; the subtree now stays mounted. Reason 1 is
//!    on its own enough to keep this surface off `popover`, so the choice below
//!    stands unchanged.
//!
//! An absolutely-positioned panel in a `relative` wrapper has neither problem: a
//! plain element survives being reparented, and nothing dismisses it but us. The
//! costs paid instead are Escape (`Action::Cancel`) and the outside-pointerdown
//! listener below, plus no top layer — accepted because no ancestor of the field
//! clips overflow, and `z-50` outranks the sticky header's `z-40`.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use shared::{CardRow, CardSummary, Id, QuickAddKind};

use super::query_bar::QueryBar;
use super::ui::command::{
    use_command_nav, Command, CommandFooter, CommandGroupLabel, CommandItem, CommandList,
};
use super::ui::kbd::Kbd;
use super::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::catalog::destination::Destination;
use crate::catalog::{describe_error, raise_add_toast, AddToast};

/// Marks the field-plus-panel wrapper, so the outside-pointerdown listener can
/// ask "did this click land in *our* surface?" with one `closest()`. The `view!`
/// below spells the same attribute out literally — macros need a name literal —
/// and this is the selector half; only the wasm listener uses it.
#[cfg(feature = "hydrate")]
const ROOT_ATTR: &str = "data-quick-add-root";

/// Digits a `⇧⏎` count may carry. Two, because [`crate::QUICK_ADD_MAX`] is 99 —
/// the cap is then unreachable by typing rather than enforced by a surprise.
const MAX_COUNT_DIGITS: usize = 2;

// ------------------------------------------------------- keystroke contract --

/// What one keydown means to the panel. `Pass` hands the key back to the search
/// box (typing, its own Enter-commits-now shortcut); everything else the panel
/// owns and swallows. Escape is never `Pass` — see [`decode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Pass,
    Next,
    Prev,
    BeginCount,
    Digit(char),
    Backspace,
    /// Escape: abandon a pending count if there is one, else close the panel —
    /// the `esc` hint the storyboard puts in the field.
    Cancel,
    /// Add the highlighted candidate. `flip` = ⌥ was held, i.e. the *other*
    /// kind than this collection leads with.
    Commit {
        flip: bool,
    },
}

/// Decode one keystroke.
///
/// `rows` is how many candidates are mounted, which doubles as "is the panel
/// open" for every key *except* Escape — its items only exist while it is, so
/// a closed panel passes the rest of the contract through and the field
/// behaves exactly as it does on `/catalog`. Escape is checked first and
/// unconditionally, because the panel can be open with zero rows (an empty
/// query, or a query nothing in the catalog matches) and Escape has to close
/// it there too — outside-click already does, and there is no reason `esc`
/// should need candidates on screen when a pointer doesn't (P6-148).
///
/// `⇧⏎` starts count entry; a second `⏎` (with or without shift) commits it, so
/// the detour is `⇧⏎ + digits + ⏎` as the storyboard's accounting assumes.
pub(crate) fn decode(key: &str, shift: bool, alt: bool, rows: usize, counting: bool) -> Action {
    if key == "Escape" {
        return Action::Cancel;
    }
    if rows == 0 {
        return Action::Pass;
    }
    match key {
        "ArrowDown" => Action::Next,
        "ArrowUp" => Action::Prev,
        "Enter" if shift && !counting => Action::BeginCount,
        "Enter" => Action::Commit { flip: alt },
        "Backspace" if counting => Action::Backspace,
        d if counting => match d.chars().next() {
            Some(c) if d.len() == 1 && c.is_ascii_digit() => Action::Digit(c),
            _ => Action::Pass,
        },
        _ => Action::Pass,
    }
}

/// Blur the field so Escape's close is symmetric with a click outside: opening
/// is driven by `focusin` (see `QuickAddSurface`'s wrapper), and a field that
/// keeps focus after Escape can never fire that event again — reopening then
/// needed a click away and back. Client-only, same shape as
/// `view_switch::focus_switch_item` (P6-148; moved there from `catalog` when
/// the grid-toggle task lifted it out for reuse by the My-cards views).
#[allow(unused_variables)]
fn blur_field(ev: &KeyboardEvent) {
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        if let Some(el) = ev
            .target()
            .and_then(|t| t.dyn_into::<leptos::web_sys::HtmlElement>().ok())
        {
            let _ = el.blur();
        }
    }
}

/// The kind `⌥⏎` commits: whichever one `⏎` does not.
fn flipped(kind: QuickAddKind) -> QuickAddKind {
    match kind {
        QuickAddKind::Have => QuickAddKind::Want,
        QuickAddKind::Want => QuickAddKind::Have,
    }
}

/// The footer's `⏎` hint — `add 1 here` in a Have-led collection, `want 1` in a
/// Want-led one (storyboard S1 vs D1).
fn enter_hint(kind: QuickAddKind) -> &'static str {
    match kind {
        QuickAddKind::Have => "add 1 here",
        QuickAddKind::Want => "want 1",
    }
}

/// The footer's `⌥⏎` hint — always the flipped kind, spelled out.
fn alt_hint(kind: QuickAddKind) -> &'static str {
    match kind {
        QuickAddKind::Have => "want instead",
        QuickAddKind::Want => "have instead",
    }
}

/// The highlighted row's action chip: `⏎ Have` / `⏎ Want`.
fn chip_label(kind: QuickAddKind) -> &'static str {
    match kind {
        QuickAddKind::Have => "Have",
        QuickAddKind::Want => "Want",
    }
}

// ------------------------------------------------------- IN THIS COLLECTION --

/// One `IN THIS COLLECTION` row: an oracle card the destination already holds,
/// with the count the table's HERE column shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentMatch {
    pub oracle_id: Id,
    pub name: String,
    pub here: i32,
}

/// Fold the collection read's rows into one row per oracle card, keeping the
/// order the read returned.
///
/// `here` is `present + present_rollup` — the same number the page's HERE column
/// renders, so the panel and the table behind it cannot disagree about what you
/// already have. Rows totalling zero are dropped: a card this collection only
/// *wants* is not "in this collection" in the copies sense, and the page's
/// WANTED column already says so.
pub fn present_matches(rows: &[CardRow]) -> Vec<PresentMatch> {
    let mut out: Vec<PresentMatch> = Vec::new();
    for row in rows {
        let here = row.present + row.present_rollup;
        match out.iter_mut().find(|m| m.oracle_id == row.oracle_id) {
            Some(existing) => existing.here += here,
            None => out.push(PresentMatch {
                oracle_id: row.oracle_id,
                name: row.name.clone(),
                here,
            }),
        }
    }
    out.retain(|m| m.here > 0);
    out
}

/// Should `PresentSection` render at all? An empty (post-trim) query has no
/// candidate search running either (see the `candidates` `Resource` above,
/// gated the same way), so without this the section would fill with whatever
/// `present` currently holds — the destination's unfiltered first page both
/// when the panel first opens and in the moment after every add clears the
/// field, since `QuickAddFacts` (`app/src/my/collection.rs`) retains the last
/// payload across a refetch (P6-068) rather than going empty. This is a
/// **render** gate on the query text, not a change to what `present` carries —
/// the retained facts keep flowing to `present` exactly as before; the section
/// just declines to draw them while there is nothing to have matched.
fn present_visible(query: &str, present: &[PresentMatch]) -> bool {
    !query.trim().is_empty() && !present.is_empty()
}

/// A candidate's disambiguation line. The storyboard shows `DMU · 1R` — set code
/// plus mana cost — but [`CardSummary`] carries no set (a catalog row is
/// per-oracle, and its representative printing's set is not projected), so this
/// is the mana cost, falling back to the type line for a card that has none.
fn meta_line(card: &CardSummary) -> String {
    card.mana_cost
        .clone()
        .filter(|m| !m.is_empty())
        .or_else(|| card.type_line.clone())
        .unwrap_or_default()
}

// -------------------------------------------------------------- the surface --

/// What the next activation should write. Set immediately before asking
/// `command` to activate; a pointer click leaves it `None` and gets the
/// collection's default, one copy.
#[derive(Clone, Copy)]
struct Commit {
    kind: QuickAddKind,
    quantity: u32,
}

/// The composite: the page's quick-search field plus the type-ahead panel.
///
/// Everything below the `Command` wrapper lives inside it on purpose — the field
/// drives the item registry through [`use_command_nav`], and context only
/// reaches descendants.
#[component]
pub fn QuickAddPanel(
    /// The field's DOM id — deterministic, and what the page's `/` hint focuses.
    #[prop(into)]
    field_id: String,
    /// The text in the field, owned by the page (its empty-state messages read
    /// it too).
    text: RwSignal<String>,
    /// The URL's canonical query: what the candidates are for.
    url_q: Memo<String>,
    /// Build the URL a committed query navigates to.
    to_url: Callback<String, String>,
    #[prop(into)] placeholder: String,
    #[prop(into)] aria_label: String,
    /// Where `⏎` adds, and the name the toast reports. `None` while the page is
    /// still resolving it — the panel then finds candidates but adds nothing
    /// rather than guessing a destination.
    #[prop(into)]
    destination: Signal<Option<Destination>>,
    /// Which kind `⏎` commits (`crate::my::collection::add_default`): Want in a
    /// deck, Have everywhere else. `⌥⏎` commits the other.
    #[prop(into)]
    default_kind: Signal<QuickAddKind>,
    /// What the destination already holds matching the query.
    #[prop(into)]
    present: Signal<Vec<PresentMatch>>,
    /// Run after an add's Undo lands, for a caller whose own read has to move
    /// with it. The add itself already clears the field, which re-navigates.
    #[prop(optional)]
    on_undo: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        // `should_filter=false`: the server returns the filtered set, so every
        // mounted candidate is a live target and the highlight index is the
        // candidate index.
        <Command
            should_filter=false
            class="h-auto min-w-0 overflow-visible rounded-none bg-transparent"
        >
            <QuickAddSurface
                field_id=field_id
                text=text
                url_q=url_q
                to_url=to_url
                placeholder=placeholder
                aria_label=aria_label
                destination=destination
                default_kind=default_kind
                present=present
                on_undo=on_undo
            />
        </Command>
    }
}

/// What the quick-add panel's candidate resource carries.
///
/// A named field, the same pattern as [`crate::catalog::SearchPayload`]. This
/// payload is the one that caused the **first** collision: its closed-panel value
/// serializes as `{"Ok":{"cards":[],"next_cursor":null}}`, which is byte-identical
/// to an empty `shared::AllCardsView`, and `/my` decoded it and rendered "You
/// haven't added any cards yet." on an account with 100 cards (#75). That instance
/// was closed from the *consumer* side by `AllCardsPayload`; naming the field here
/// closes it from the producer side too, so the next type that happens to look
/// like `SearchResults` does not have to be found the hard way.
///
/// Two collection pages both mount a quick-add panel, so this resource is also
/// exposed to the **same-type** case a named field cannot fix — that one needs the
/// payload to echo the request it answered. Unchanged, and measured not to fire.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickAddPayload {
    quick_add: shared::SearchResults,
}

#[component]
fn QuickAddSurface(
    #[prop(into)] field_id: String,
    text: RwSignal<String>,
    url_q: Memo<String>,
    to_url: Callback<String, String>,
    #[prop(into)] placeholder: String,
    #[prop(into)] aria_label: String,
    destination: Signal<Option<Destination>>,
    default_kind: Signal<QuickAddKind>,
    present: Signal<Vec<PresentMatch>>,
    /// `optional_no_strip` so the outer component can forward its own `Option`
    /// straight through instead of unwrapping and rewrapping it.
    #[prop(optional_no_strip)]
    on_undo: Option<Callback<()>>,
) -> impl IntoView {
    let nav = use_command_nav().expect("QuickAddSurface renders inside a Command");
    let toast = expect_context::<ToastHandle>();
    // The sidebar badges count what an add changes (the shell-level resource,
    // per specs/app-ui.md Findings).
    let tree = use_context::<crate::my::tree::CollectionTreeResource>();
    // ⌘K's `Undo last move` remembers what this panel adds (see `palette`).
    let last_move = use_context::<super::palette::LastMoveState>();

    let open = RwSignal::new(false);
    // `Some("")` = count entry started, no digits yet; `Some("4")` = ×4 pending.
    let count = RwSignal::new(None::<String>);
    let pending = RwSignal::new(None::<Commit>);
    // Bumped to clear the field after an add — through `QueryBar` so the armed
    // debounce dies with it (see its module doc).
    let reset = RwSignal::new(0u32);

    // Candidates for the *committed* query, fetched only while the panel is
    // open — an anonymous or unfocused page load must not search the catalog.
    let candidates = Resource::new(
        move || (open.get(), url_q.get()),
        |(open, q)| async move {
            if !open || q.trim().is_empty() {
                return Ok(QuickAddPayload {
                    quick_add: shared::SearchResults {
                        cards: Vec::new(),
                        next_cursor: None,
                    },
                });
            }
            Ok(QuickAddPayload {
                quick_add: crate::search_catalog(q, None, None).await?,
            })
        },
    );

    // Editing the query invalidates a pending count (the count belongs to the
    // row you were looking at) and re-seeds the highlight on the best match.
    Effect::new(move |_| {
        let typed = text.get();
        count.set(None);
        nav.set_query(typed);
    });

    // Closing — Escape, or a click outside — abandons count entry too, so
    // reopening never inherits a half-typed number.
    Effect::new(move |_| {
        if !open.get() {
            count.set(None);
        }
    });

    // Dismiss on a pointerdown outside the field-and-panel wrapper. `pointerdown`
    // rather than `click`, so a press that lands on the page behind closes before
    // it can also activate something under a panel that is about to disappear.
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        let handle = window_event_listener(leptos::ev::pointerdown, move |ev| {
            if !open.get_untracked() {
                return;
            }
            let inside = ev
                .target()
                .and_then(|t| t.dyn_into::<leptos::web_sys::Element>().ok())
                .and_then(|el| el.closest(&format!("[{ROOT_ATTR}]")).ok().flatten())
                .is_some();
            if !inside {
                open.set(false);
            }
        });
        on_cleanup(move || handle.remove());
    }

    // **There was a 120 ms focus keeper here, and it is gone (P6-068).** It
    // re-focused the field whenever `document.activeElement` fell back to
    // `<body>` while the panel was open, because a `?q=` navigation detached and
    // re-attached this page's whole subtree and the caret did not survive it.
    // The detach was never the router's: `CollectionPage` read its own
    // `Resource` in its setup body, which registers on the *nearest*
    // `SuspenseContext` — `RequireAuth`'s `<Suspense>`, an ancestor of the page
    // — so every re-search re-suspended the auth guard and `EitherKeepAlive`
    // unmounted the `<Outlet/>` subtree for the duration of the fetch. That read
    // now goes through a plain `RwSignal` (`app/src/my/collection.rs`), nothing
    // unmounts, and an interval that steals focus back from `<body>` is dead
    // weight with a real cost of its own — it fights any deliberate blur that is
    // not a click on a focusable element.

    let add = Callback::new(move |card: CardSummary| {
        let Commit { kind, quantity } = pending.get_untracked().unwrap_or(Commit {
            kind: default_kind.get_untracked(),
            quantity: 1,
        });
        pending.set(None);
        let Some(dest) = destination.get_untracked() else {
            toast.show(
                ToastOptions::message("Still loading this collection — try that again.")
                    .kind(ToastKind::Error),
            );
            // Mirror the success path: a stale count must not survive a failed
            // add, or a later bare ⏎ silently reuses it (P6-148).
            count.set(None);
            return;
        };
        // Holdings are per printing, so a card whose oracle row resolved no
        // representative printing can be Wanted but not Had — the same rule the
        // catalog's `+ Have` disables itself under, said out loud here because
        // there is no button to grey out.
        if kind == QuickAddKind::Have && card.printing_id.is_none() {
            toast.show(
                ToastOptions::message(format!(
                    "{} has no printing to add — ⌥⏎ wants it instead.",
                    card.name
                ))
                .kind(ToastKind::Error),
            );
            count.set(None);
            return;
        }
        let name = card.name.clone();
        let oracle_id = card.oracle_id;
        let printing_id = card.printing_id;
        spawn_local(async move {
            match crate::quick_add(dest.id, kind, oracle_id, printing_id, quantity).await {
                Ok(receipt) => {
                    if let Some(t) = tree {
                        t.0.refetch();
                    }
                    // The loop point (storyboard M2): field cleared, panel still
                    // open and focused, so the next card starts immediately.
                    // Clearing re-navigates, which is what refetches the page.
                    count.set(None);
                    reset.update(|n| *n += 1);
                    raise_add_toast(AddToast {
                        toast,
                        tree,
                        name,
                        dest,
                        kind,
                        quantity,
                        undo_move_id: receipt.undo_move_id,
                        after_undo: on_undo,
                        last_move,
                    });
                }
                Err(e) => {
                    toast.show(
                        ToastOptions::message(format!(
                            "Couldn't add {name}: {}",
                            describe_error(&e).1
                        ))
                        .kind(ToastKind::Error),
                    );
                }
            }
        });
    });

    let on_key = Callback::new(move |ev: KeyboardEvent| {
        let counting = count.read_untracked().is_some();
        let action = decode(
            &ev.key(),
            ev.shift_key(),
            ev.alt_key(),
            nav.visible_count().get_untracked(),
            counting,
        );
        match action {
            Action::Pass => return false,
            Action::Next => nav.next(),
            Action::Prev => nav.prev(),
            Action::BeginCount => count.set(Some(String::new())),
            Action::Digit(d) => count.update(|c| {
                if let Some(buf) = c {
                    if buf.len() < MAX_COUNT_DIGITS {
                        buf.push(d);
                    }
                }
            }),
            Action::Backspace => count.update(|c| {
                if let Some(buf) = c {
                    buf.pop();
                    if buf.is_empty() {
                        *c = None;
                    }
                }
            }),
            // One Escape per thing to abandon: the count first, then the panel.
            Action::Cancel => {
                if counting {
                    count.set(None);
                } else {
                    open.set(false);
                    // Symmetric with how it opened (`focusin`) — see
                    // `blur_field`.
                    blur_field(&ev);
                }
            }
            Action::Commit { flip } => {
                let kind = if flip {
                    flipped(default_kind.get_untracked())
                } else {
                    default_kind.get_untracked()
                };
                let quantity = count
                    .get_untracked()
                    .and_then(|buf| buf.parse::<u32>().ok())
                    .map(crate::clamp_quick_add_quantity)
                    .unwrap_or(1);
                pending.set(Some(Commit { kind, quantity }));
                // Nothing highlighted means nothing to add — hand the key back
                // rather than swallowing it.
                if !nav.activate() {
                    pending.set(None);
                    return false;
                }
            }
        }
        ev.prevent_default();
        true
    });

    view! {
        // One wrapper for field + panel: it is the positioning context, and the
        // thing the outside-pointerdown listener tests membership of.
        <div
            class="relative min-w-0"
            data-quick-add-root="true"
            on:focusin=move |_| open.set(true)
            on:input=move |_| open.set(true)
        >
            <QueryBar
                text=text
                url_q=url_q
                to_url=to_url
                id=field_id
                placeholder=placeholder
                aria_label=aria_label
                on_key=on_key
                reset=reset
            />
            // Mounted only while open, so nothing here renders server-side and
            // the candidate items exist exactly when they are targets.
            <Show when=move || open.get()>
                <div
                    class="bg-popover text-popover-foreground absolute top-full right-0 left-0 z-50 mt-1 flex flex-col overflow-hidden rounded-md border shadow-lg"
                    data-testid="quick-add-panel"
                    role="listbox"
                    aria-label="Card suggestions"
                >
                    <CommandList class="max-h-[19rem] p-1.5">
                        <PresentSection present=present url_q=url_q />
                        <CandidateSection
                            candidates=candidates
                            present=present
                            url_q=url_q
                            default_kind=default_kind
                            count=count
                            add=add
                        />
                    </CommandList>
                    <PanelFooter default_kind=default_kind />
                </div>
            </Show>
        </div>
    }
}

#[component]
fn PresentSection(present: Signal<Vec<PresentMatch>>, url_q: Memo<String>) -> impl IntoView {
    view! {
        <Show when=move || present.with(|p| present_visible(&url_q.read(), p))>
            <CommandGroupLabel class="tracking-wide uppercase">
                "In this collection"
            </CommandGroupLabel>
            <For each=move || present.get() key=|m| m.oracle_id let:m>
                // Context, not a target: the storyboard leaves these
                // unhighlighted with the best *catalog* match selected above
                // them, so they are links rather than `CommandItem`s.
                <a
                    href=format!("/cards/{}", m.oracle_id)
                    class="hover:bg-accent hover:text-accent-foreground flex items-center gap-2 rounded-sm px-2 py-1.5 text-sm"
                    data-testid="quick-add-present"
                    data-oracle=m.oracle_id.to_string()
                >
                    <span class="truncate">{m.name.clone()}</span>
                    <span
                        class="text-muted-foreground ml-auto shrink-0 text-xs"
                        data-testid="quick-add-present-count"
                    >
                        {m.here}
                    </span>
                </a>
            </For>
        </Show>
    }
}

#[component]
fn CandidateSection(
    candidates: Resource<Result<QuickAddPayload, ServerFnError<shared::ApiError>>>,
    present: Signal<Vec<PresentMatch>>,
    url_q: Memo<String>,
    default_kind: Signal<QuickAddKind>,
    count: RwSignal<Option<String>>,
    add: Callback<CardSummary>,
) -> impl IntoView {
    view! {
        // The fallback view is built once, not re-run per read — so the "is a
        // query even running" condition has to be a `Show` *inside* it, or the
        // `url_q` read happens outside any tracking context.
        <Transition fallback=move || {
            view! {
                <Show when=move || !url_q.read().is_empty()>
                    <p class="text-muted-foreground px-2 py-3 text-sm">"Searching the catalog…"</p>
                </Show>
            }
        }>
            {move || {
                let q = url_q.get();
                Suspend::new(async move {
                    let cards = match candidates.await.map(|p| p.quick_add) {
                        Ok(results) => results.cards,
                        Err(e) => {
                            let (_, message) = describe_error(&e);
                            return view! {
                                <p
                                    class="text-muted-foreground px-2 py-3 text-sm"
                                    data-testid="quick-add-error"
                                >
                                    {message}
                                </p>
                            }
                                .into_any();
                        }
                    };
                    if cards.is_empty() {
                        // Nothing anywhere is worth saying; nothing in the
                        // catalog while the collection matched is not — the
                        // present rows above already answered.
                        return (!q.is_empty() && present.read().is_empty())
                            .then(|| {
                                view! {
                                    <p
                                        class="text-muted-foreground px-2 py-3 text-sm"
                                        data-testid="quick-add-empty"
                                    >
                                        {format!("Nothing matches “{q}”.")}
                                    </p>
                                }
                            })
                            .into_any();
                    }
                    view! {
                        <CommandGroupLabel class="tracking-wide uppercase">
                            "Add from catalog"
                        </CommandGroupLabel>
                        {cards
                            .into_iter()
                            .enumerate()
                            .map(|(index, card)| {
                                view! {
                                    <Candidate
                                        card=card
                                        index=index
                                        default_kind=default_kind
                                        count=count
                                        add=add
                                    />
                                }
                            })
                            .collect_view()}
                    }
                        .into_any()
                })
            }}
        </Transition>
    }
}

#[component]
fn Candidate(
    card: CardSummary,
    /// This row's position among the mounted candidates, which is its position
    /// among the *visible* items (nothing is filtered) — so it is the index
    /// `command`'s highlight is expressed in.
    index: usize,
    default_kind: Signal<QuickAddKind>,
    count: RwSignal<Option<String>>,
    add: Callback<CardSummary>,
) -> impl IntoView {
    let nav = use_command_nav().expect("Candidate renders inside a Command");
    let highlighted = nav.highlighted();
    let mine = Memo::new(move |_| highlighted.get() == index);

    let name = card.name.clone();
    let meta = meta_line(&card);
    let oracle = card.oracle_id.to_string();
    let selected = card.clone();

    view! {
        <CommandItem
            value=card.name.clone()
            class="cursor-pointer gap-2"
            on_select=Callback::new(move |()| add.run(selected.clone()))
        >
            // The test seam rides an inner element: `CommandItem` takes no
            // attribute spread, and its own `aria-selected` already means
            // "keyboard-highlighted" for a screen reader.
            <span
                class="truncate"
                data-testid="quick-add-candidate"
                data-oracle=oracle
                data-highlighted=move || mine.get().then_some("true")
            >
                {name}
            </span>
            <span class="text-muted-foreground shrink-0 text-xs">{meta}</span>
            <span class="ml-auto shrink-0">
                {move || mine.get().then(|| view! { <Chip default_kind=default_kind count=count /> })}
            </span>
        </CommandItem>
    }
}

/// The highlighted row's chip — `⏎ Have` at rest, `× 4 ⏎` during count entry
/// (storyboard S2 / S4).
#[component]
fn Chip(default_kind: Signal<QuickAddKind>, count: RwSignal<Option<String>>) -> impl IntoView {
    view! {
        <span
            class="bg-accent text-accent-foreground inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium"
            data-testid="quick-add-chip"
        >
            {move || match count.get() {
                Some(buf) => {
                    let shown = if buf.is_empty() { "__".to_string() } else { buf };
                    view! {
                        <span data-testid="quick-add-count">{format!("× {shown}")}</span>
                        <Kbd>"⏎"</Kbd>
                    }
                        .into_any()
                }
                None => {
                    view! {
                        <Kbd>"⏎"</Kbd>
                        {move || chip_label(default_kind.get())}
                    }
                        .into_any()
                }
            }}
        </span>
    }
}

/// The keystroke ledger, verbatim from the storyboard's footer.
#[component]
fn PanelFooter(default_kind: Signal<QuickAddKind>) -> impl IntoView {
    view! {
        <CommandFooter class="flex-wrap gap-3 px-3" {..} data-testid="quick-add-footer">
            <span class="inline-flex items-center gap-1">
                <Kbd>"↑↓"</Kbd>
                "navigate"
            </span>
            <span class="inline-flex items-center gap-1">
                <Kbd>"⏎"</Kbd>
                {move || enter_hint(default_kind.get())}
            </span>
            <span class="inline-flex items-center gap-1">
                <Kbd>"⇧⏎"</Kbd>
                "set count"
            </span>
            <span class="inline-flex items-center gap-1">
                <Kbd>"⌥⏎"</Kbd>
                {move || alt_hint(default_kind.get())}
            </span>
        </CommandFooter>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, oracle: Id, present: i32, rollup: i32) -> CardRow {
        CardRow {
            oracle_id: oracle,
            printing_id: Id::new_v4(),
            name: name.to_string(),
            set_code: Some("tst".into()),
            collector_number: "1".into(),
            image_uri: None,
            mana_cost: None,
            type_line: None,
            colors: vec![],
            present,
            desired: 0,
            owned: present,
            present_rollup: rollup,
            board: shared::Board::default(),
            holding_id: None,
            desire_id: None,
            faces: vec![],
        }
    }

    // ------------------------------------------------------------ contract --

    #[test]
    fn arrows_navigate_and_enter_commits_the_default() {
        assert_eq!(decode("ArrowDown", false, false, 3, false), Action::Next);
        assert_eq!(decode("ArrowUp", false, false, 3, false), Action::Prev);
        assert_eq!(
            decode("Enter", false, false, 3, false),
            Action::Commit { flip: false }
        );
    }

    #[test]
    fn alt_enter_flips_the_kind_and_shift_enter_starts_a_count() {
        assert_eq!(
            decode("Enter", false, true, 3, false),
            Action::Commit { flip: true }
        );
        assert_eq!(decode("Enter", true, false, 3, false), Action::BeginCount);
    }

    #[test]
    fn a_closed_panel_passes_every_key_to_the_search_box() {
        // No mounted candidates *is* "closed" — the items only exist while the
        // panel is open, which is what keeps the field behaving like /catalog's.
        // Escape is not in this list: it closes regardless of row count, so it
        // is never `Pass` — see the dedicated tests below (P6-148).
        for key in ["ArrowDown", "ArrowUp", "Enter", "a"] {
            assert_eq!(
                decode(key, false, false, 0, false),
                Action::Pass,
                "{key} must fall through"
            );
        }
    }

    #[test]
    fn escape_cancels_whether_or_not_a_count_is_pending() {
        // One decode either way; the caller is what abandons the count first and
        // the panel second, so `esc esc` gets you out of a half-typed playset.
        assert_eq!(decode("Escape", false, false, 3, false), Action::Cancel);
        assert_eq!(decode("Escape", false, false, 3, true), Action::Cancel);
    }

    #[test]
    fn escape_cancels_even_with_zero_candidate_rows() {
        // The panel can be open with nothing mounted — an empty query, or a
        // query the catalog has no match for — and Escape has to close it
        // there too: outside-click already does, and rows == 0 must not make
        // `esc` the odd one out (P6-148, formerly a known minor).
        assert_eq!(decode("Escape", false, false, 0, false), Action::Cancel);
        assert_eq!(decode("Escape", false, false, 0, true), Action::Cancel);
    }

    #[test]
    fn count_entry_captures_digits_and_commits_on_enter() {
        assert_eq!(decode("4", false, false, 3, true), Action::Digit('4'));
        assert_eq!(
            decode("Backspace", false, false, 3, true),
            Action::Backspace
        );
        assert_eq!(
            decode("Enter", false, false, 3, true),
            Action::Commit { flip: false }
        );
        // ⇧⏎ while already counting commits rather than restarting: the detour
        // the storyboard prices is ⇧⏎ + digits + ⏎.
        assert_eq!(
            decode("Enter", true, false, 3, true),
            Action::Commit { flip: false }
        );
        // …and ⌥ still flips the kind on a counted commit.
        assert_eq!(
            decode("Enter", false, true, 3, true),
            Action::Commit { flip: true }
        );
    }

    #[test]
    fn digits_only_belong_to_the_panel_while_counting() {
        assert_eq!(decode("4", false, false, 3, false), Action::Pass);
        assert_eq!(decode("Backspace", false, false, 3, false), Action::Pass);
        // A letter typed mid-count keeps searching (and the caller drops the
        // count, since the row it belonged to is about to change).
        assert_eq!(decode("g", false, false, 3, true), Action::Pass);
    }

    #[test]
    fn the_flip_and_the_hints_agree_on_which_kind_leads() {
        assert_eq!(flipped(QuickAddKind::Have), QuickAddKind::Want);
        assert_eq!(flipped(QuickAddKind::Want), QuickAddKind::Have);
        assert_eq!(enter_hint(QuickAddKind::Have), "add 1 here");
        assert_eq!(alt_hint(QuickAddKind::Have), "want instead");
        assert_eq!(enter_hint(QuickAddKind::Want), "want 1");
        assert_eq!(alt_hint(QuickAddKind::Want), "have instead");
        assert_eq!(chip_label(QuickAddKind::Want), "Want");
        assert_eq!(chip_label(QuickAddKind::Have), "Have");
    }

    // ------------------------------------------------------------- present --

    #[test]
    fn present_rows_fold_to_one_per_oracle_card() {
        let bolt = Id::new_v4();
        let rows = [
            row("Lightning Bolt", bolt, 2, 0),
            row("Lightning Bolt", bolt, 1, 0),
        ];
        let folded = present_matches(&rows);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].here, 3, "both printings count toward HERE");
    }

    #[test]
    fn present_rows_include_rolled_up_copies() {
        let helix = Id::new_v4();
        let folded = present_matches(&[row("Lightning Helix", helix, 1, 4)]);
        assert_eq!(
            folded[0].here, 5,
            "HERE is present + rollup, as on the page"
        );
    }

    #[test]
    fn a_card_only_wanted_here_is_not_a_present_row() {
        let strike = Id::new_v4();
        assert!(present_matches(&[row("Lightning Strike", strike, 0, 0)]).is_empty());
    }

    #[test]
    fn present_rows_keep_the_reads_order() {
        let a = Id::new_v4();
        let b = Id::new_v4();
        let names: Vec<_> = present_matches(&[row("Zap", b, 1, 0), row("Arc", a, 1, 0)])
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, vec!["Zap", "Arc"]);
    }

    #[test]
    fn present_hides_on_an_empty_or_blank_query_even_with_matches() {
        let matches = [PresentMatch {
            oracle_id: Id::new_v4(),
            name: "Lightning Bolt".into(),
            here: 3,
        }];
        assert!(
            !present_visible("", &matches),
            "no query means the panel is at rest, not filtered by anything"
        );
        assert!(
            !present_visible("   ", &matches),
            "whitespace-only is the same as no query, post-trim"
        );
    }

    #[test]
    fn present_shows_only_once_a_query_actually_matched_something() {
        let matches = [PresentMatch {
            oracle_id: Id::new_v4(),
            name: "Lightning Bolt".into(),
            here: 3,
        }];
        assert!(present_visible("bolt", &matches));
        assert!(
            !present_visible("bolt", &[]),
            "a query with nothing here is still nothing to show"
        );
    }

    // ---------------------------------------------------------------- meta --

    #[test]
    fn meta_prefers_the_mana_cost_and_falls_back_to_the_type_line() {
        let mut card = CardSummary {
            oracle_id: Id::new_v4(),
            name: "Lightning Bolt".into(),
            printing_id: None,
            image_uri: None,
            mana_cost: Some("{R}".into()),
            type_line: Some("Instant".into()),
            owned: None,
            faces: vec![],
        };
        assert_eq!(meta_line(&card), "{R}");
        card.mana_cost = Some(String::new());
        assert_eq!(meta_line(&card), "Instant", "a land has no mana cost");
        card.type_line = None;
        assert_eq!(meta_line(&card), "");
    }
}
