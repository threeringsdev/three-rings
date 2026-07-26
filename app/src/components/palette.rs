//! The global **⌘K command palette** (design/command-palette.md; the
//! `⌘K — Command palette` region in design/wireframes.pen, frames P1/P2).
//!
//! The keyboard-jump layer over the logged-in app: `⌘K` (mac) / `Ctrl+K`
//! (elsewhere) opens a centered modal over a scrim; `⏎` navigates to a **place**
//! or runs one of exactly three **commands**; `esc` closes. It is an accelerator
//! over affordances that already exist — every row duplicates a designed path
//! and none replaces one.
//!
//! Six things are worth knowing before editing this file.
//!
//! **`/` is never bound here.** It belongs to the in-collection quick-add
//! (design/add-flow-prototype.md). The only chord this module listens for is
//! [`is_palette_chord`], and its unit tests assert `/` is not it.
//!
//! **Desktop-and-signed-in is one client-only gate, and that is also what keeps
//! hydration honest.** [`DESKTOP_MEDIA`] resolves in an `Effect`, so
//! [`desktop_signal`] is `false` during SSR *and* during the hydration render;
//! [`PaletteBody`] therefore never renders on the server, which is what lets it
//! read the tree resource in plain render without the tachys
//! "expected an HTML `<div>`" mismatch the destination picker hit. The media
//! query is **listened**, not sampled: one `MediaQueryList` per document with a
//! `change` handler, so resizing across the breakpoint (or docking a laptop)
//! takes effect. That is deliberately unlike `CardPreview`, which samples
//! `(pointer: coarse)` once per card and never listens — a filed discovery this
//! surface must not make worse.
//!
//! **The rows are rebuilt, never reordered in place — and that is load-bearing.**
//! `command`'s item registry is built in *mount* order and `visible_ids()`
//! returns that order (see [`super::ui::command`]). Unlike the destination
//! picker (which sorts once and only hides rows) this surface *ranks*, so its
//! order really does change per keystroke, and the registry would go stale
//! against the DOM unless every keystroke starts the list over. It does: the
//! list is one `<For>` item keyed on [`RowSet::key`], so any change to the
//! rendered rows disposes them and mounts the new order.
//!
//! **A plain dynamic closure is *not* enough, which is worth knowing before
//! "simplifying" this.** `{move || rows}` looks like a rebuild and is not —
//! tachys diffs an unkeyed collection of views positionally and reuses the
//! existing DOM nodes, so the rows survived each keystroke while the registry
//! kept growing. That was measured, not reasoned: the node-identity assertion in
//! `end2end/tests/command-palette.spec.ts` failed on the first version of this
//! file. That test is the guard — it asserts the first row's DOM node does *not*
//! survive a keystroke, so regressing to a positional diff fails it.
//!
//! **It filters itself.** `should_filter=false`, because the primitive's filter
//! is a lowercase `contains` that can neither match across word boundaries
//! (`trabin` → `Trade Binder`) nor *rank*, and the design asks for a fuzzy
//! filter with the best match pre-selected. [`score`] is the ranker; the query
//! is mirrored out of `CommandInput` through `on_search_change` because
//! `Command` keeps its own query private.
//!
//! **`Undo last move` is a session memory, not a server call.** There is no
//! `undo_last_move` endpoint and there deliberately never was: `crate::quick_add`
//! records that a server-side "undo the latest" races a second tab or a fast
//! second click. So every surface that writes an undoable move hands its move
//! ids to [`LastMoveState`], and this command replays exactly those — the same
//! reversal the undo toast offers. Nothing recorded (a fresh load) says so
//! rather than guessing.
//!
//! **The two create commands trigger the tree's flow, they don't reimplement
//! it.** Per the IA, tree management is in-place; `New binder…` navigates to My
//! cards and opens the same `TreeManage` create dialog the tree's context menu
//! opens. That is why `provide_tree_manage` moved up to the app shell.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_location, use_navigate};
use shared::{CollectionKind, Id};

use super::ui::command::{
    use_command_nav, CommandDialog, CommandEmpty, CommandFooter, CommandGroup, CommandGroupLabel,
    CommandInput, CommandItem, CommandList,
};
use super::ui::kbd::Kbd;
use super::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::my::tree::{assemble, AssembledTree, CollectionTreeResource, TreeNode};
use crate::my::tree_manage::TreeManage;

/// The palette's dialog id — one instance per document, so a constant is both
/// deterministic (SSR and hydration must agree) and unambiguous.
pub const PALETTE_ID: &str = "command-palette";

/// The search field's DOM id — the handle the open-Effect focuses.
pub const PALETTE_INPUT_ID: &str = "command-palette-input";

/// What "desktop" means here. Width *and* pointer, because the design's reason
/// for the gate is "hardware-keyboard accelerator": `min-width` is the same
/// 768 px line the shell's `md:` chrome switches on (so the palette exists
/// exactly where the sidebar tree it duplicates does), and `pointer: fine`
/// keeps a landscape tablet out. A narrow desktop window loses the palette,
/// which is the honest reading of "not on mobile in v1" — nothing else depends
/// on it, so there is nothing to strand.
pub const DESKTOP_MEDIA: &str = "(min-width: 768px) and (pointer: fine)";

/// How many recent places the RECENT group shows. The wireframe's P1 draws
/// three; five is the cap because the group sits above COMMANDS and a taller
/// list pushes them out of the 300 px `CommandList`.
pub const RECENT_CAP: usize = 5;

/// `localStorage` key for the recent-places ring.
///
/// **Not a cookie, unlike `tr_dest` and `tr_theme`** — and for their own stated
/// reason, applied honestly. Those are cookies because they must be readable
/// *during SSR*, so the server can render the real value instead of a
/// placeholder an effect corrects a frame later. This list is only ever read by
/// a surface that does not exist on the server at all, so a cookie would buy
/// nothing and cost a few hundred bytes on every request to the origin —
/// including every asset — for data the server never looks at.
pub const RECENT_STORAGE_KEY: &str = "tr_recent_places";

// --------------------------------------------------------------- last move --

/// The most recent undoable move(s) of this session — what `Undo last move`
/// reverses. `move_ids` because the batch surfaces (the selection tray's move,
/// the needs pull) write N ledger rows per action and reverse as one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastMove {
    pub move_ids: Vec<Id>,
}

/// Shell-level, for the same reason the selection is: the palette outlives every
/// page, so the page that *made* the move cannot own the memory of it.
#[derive(Clone, Copy)]
pub struct LastMoveState(pub RwSignal<Option<LastMove>>);

pub fn provide_last_move() {
    provide_context(LastMoveState(RwSignal::new(None)));
}

impl LastMoveState {
    /// Record an undoable move. Called by every surface that already raises an
    /// undo toast, right where it raises it, so the palette command and the
    /// toast button can never disagree about what "the last move" is.
    pub fn note(&self, move_ids: Vec<Id>) {
        if move_ids.is_empty() {
            return;
        }
        self.0.set(Some(LastMove { move_ids }));
    }
}

/// Record a move where the caller may not have the context (the bench renders
/// the palette surface outside the shell). A no-op without the state.
pub fn note_last_move(state: Option<LastMoveState>, move_ids: Vec<Id>) {
    if let Some(state) = state {
        state.note(move_ids);
    }
}

// -------------------------------------------------------- the trigger chord --

/// Is this keystroke the palette's chord? `⌘K` on mac, `Ctrl+K` elsewhere.
///
/// Split out and pure so the platform split is testable, and so the one thing
/// the design forbids — binding `/`, which belongs to quick-add — is asserted
/// rather than assumed. The *other* modifier is required to be absent so
/// `⌃⌘K` (a system or extension chord) isn't stolen.
pub fn is_palette_chord(key: &str, meta: bool, ctrl: bool, mac: bool) -> bool {
    if !key.eq_ignore_ascii_case("k") {
        return false;
    }
    if mac {
        meta && !ctrl
    } else {
        ctrl && !meta
    }
}

// ---------------------------------------------------------------- the index --

/// A place the palette can jump to. The key is what persists in the recent
/// ring, so it is data (a collection id or a system slug), never a list index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceKey {
    AllCards,
    Shopping,
    Catalog,
    MyCards,
    Collection(Id),
}

impl PlaceKey {
    /// The persisted token. Slugs cannot collide with a uuid.
    pub fn token(self) -> String {
        match self {
            PlaceKey::AllCards => "all".into(),
            PlaceKey::Shopping => "shopping".into(),
            PlaceKey::Catalog => "catalog".into(),
            PlaceKey::MyCards => "my".into(),
            PlaceKey::Collection(id) => id.to_string(),
        }
    }

    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "all" => Some(PlaceKey::AllCards),
            "shopping" => Some(PlaceKey::Shopping),
            "catalog" => Some(PlaceKey::Catalog),
            "my" => Some(PlaceKey::MyCards),
            other => other.parse().ok().map(PlaceKey::Collection),
        }
    }
}

/// One place row: what it is called, the parent path shown beside it
/// (`Trade Binder — Binders`), and where `⏎` goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub key: PlaceKey,
    pub name: String,
    /// Ancestor path, `/`-joined. Empty for a root or a system place.
    pub meta: String,
    pub href: String,
    pub icon: &'static str,
    /// Offered as a cold-start row when there is no history yet.
    pub default_row: bool,
}

/// The fixed v1 command registry. Three entries, deliberately — context-aware
/// actions were explicitly deferred, and `Sign out` was considered and dropped
/// (rare, destructive-ish, stays in the user menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteCommand {
    NewBinder,
    NewDeck,
    UndoLastMove,
}

impl PaletteCommand {
    pub const ALL: [PaletteCommand; 3] = [
        PaletteCommand::NewBinder,
        PaletteCommand::NewDeck,
        PaletteCommand::UndoLastMove,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PaletteCommand::NewBinder => "New binder…",
            PaletteCommand::NewDeck => "New deck…",
            PaletteCommand::UndoLastMove => "Undo last move",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            PaletteCommand::NewBinder => "new-binder",
            PaletteCommand::NewDeck => "new-deck",
            PaletteCommand::UndoLastMove => "undo-last-move",
        }
    }
}

/// What activating a row does. An enum rather than a closure so the bench can
/// render the surface with a logging handler instead of the real one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    Go(String),
    Run(PaletteCommand),
}

/// The whole place index, in the order the sidebar reads top to bottom: All
/// cards, the tree (Inbox pinned first by [`assemble`]), Shopping list, then the
/// two mode jumps.
///
/// **Inbox is not added as a system place** even though the design lists it as
/// one: it *is* a collection row, so the tree flatten already produced it, and
/// adding it again would put two `Inbox` rows in the palette.
///
/// `Go to My cards` and `All cards` both point at `/my` — the design lists both
/// (a mode jump and a place are different mental models) and they are kept, but
/// only `All cards` is a recent-ring key, so a visit to `/my` cannot show up
/// twice in RECENT.
pub fn place_index(tree: Option<&AssembledTree>) -> Vec<Place> {
    let mut out = vec![Place {
        key: PlaceKey::AllCards,
        name: "All cards".into(),
        meta: String::new(),
        href: "/my".into(),
        icon: "🗂",
        default_row: true,
    }];
    if let Some(tree) = tree {
        for node in &tree.roots {
            push_subtree(node, &[], &mut out);
        }
    }
    out.push(Place {
        key: PlaceKey::Shopping,
        name: "Shopping list".into(),
        meta: String::new(),
        href: "/my/shopping".into(),
        icon: "🛒",
        default_row: true,
    });
    out.push(Place {
        key: PlaceKey::Catalog,
        name: "Go to Catalog".into(),
        meta: "Mode".into(),
        href: "/catalog".into(),
        icon: "📖",
        default_row: false,
    });
    out.push(Place {
        key: PlaceKey::MyCards,
        name: "Go to My cards".into(),
        meta: "Mode".into(),
        href: "/my".into(),
        icon: "🗂",
        default_row: false,
    });
    out
}

/// Depth-first flatten of one subtree, carrying the ancestor names down so each
/// row can show the parent path the wireframe puts beside it.
fn push_subtree(node: &TreeNode, ancestors: &[String], out: &mut Vec<Place>) {
    let is_inbox = node.row.summary.is_inbox;
    out.push(Place {
        key: PlaceKey::Collection(node.row.summary.id),
        name: node.row.summary.name.clone(),
        meta: ancestors.join(" / "),
        href: format!("/my/collections/{}", node.row.summary.id),
        icon: match node.row.summary.kind {
            _ if is_inbox => "📥",
            CollectionKind::Deck => "🎴",
            CollectionKind::Binder => "🗂",
        },
        // The Inbox is the third cold-start row (design: All cards, Inbox,
        // Shopping list are the system places).
        default_row: is_inbox,
    });
    let mut deeper = ancestors.to_vec();
    deeper.push(node.row.summary.name.clone());
    for child in &node.children {
        push_subtree(child, &deeper, out);
    }
}

/// Which place a pathname *is*, for the recent ring. `None` for anything that
/// isn't a place (`/catalog`, `/cards/:id`, the auth pages) — the mode jumps are
/// navigation shortcuts, not somewhere you were, so they never enter RECENT.
///
/// A collection's subpages (`/my/collections/:id/needs`) count as the
/// collection: you are still in it, which is the same rule the sidebar's
/// `aria-current` uses.
pub fn place_key_for_path(path: &str) -> Option<PlaceKey> {
    match path.trim_end_matches('/') {
        "/my" => Some(PlaceKey::AllCards),
        "/my/shopping" => Some(PlaceKey::Shopping),
        other => other
            .strip_prefix("/my/collections/")
            .and_then(|rest| rest.split('/').next())
            .and_then(|id| id.parse().ok())
            .map(PlaceKey::Collection),
    }
}

/// Push `key` onto the front of the ring, deduplicated, capped.
pub fn push_recent(ring: &[PlaceKey], key: PlaceKey, cap: usize) -> Vec<PlaceKey> {
    let mut next = Vec::with_capacity(ring.len() + 1);
    next.push(key);
    next.extend(ring.iter().copied().filter(|k| *k != key));
    next.truncate(cap.max(1));
    next
}

pub fn serialize_recents(ring: &[PlaceKey]) -> String {
    ring.iter().map(|k| k.token()).collect::<Vec<_>>().join(",")
}

pub fn parse_recents(raw: &str) -> Vec<PlaceKey> {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .filter_map(PlaceKey::parse)
        .collect()
}

/// The at-rest group: what P1 draws above COMMANDS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtRest {
    /// `Recent` normally; `Places` on the cold start, because calling a place
    /// you have never visited "recent" would be a lie.
    pub label: &'static str,
    pub places: Vec<Place>,
}

/// Resolve the ring against the live index, most recent first.
///
/// Three rules, each earning its keep: a key the index no longer has is dropped
/// (a deleted collection must not offer a row every `⏎` would 404 on — the same
/// reasoning as the destination picker's `reconcile`); the place you are *on* is
/// dropped, which is what makes `⌘K ⏎` bounce to the last collection rather than
/// reload this one; and an empty result falls back to the system places, so the
/// first row of a first-ever palette is `All cards` and not `New binder…`.
pub fn at_rest(
    index: &[Place],
    ring: &[PlaceKey],
    current: Option<PlaceKey>,
    cap: usize,
) -> AtRest {
    let places: Vec<Place> = ring
        .iter()
        .filter(|k| Some(**k) != current)
        .filter_map(|k| index.iter().find(|p| p.key == *k))
        .take(cap)
        .cloned()
        .collect();
    if places.is_empty() {
        AtRest {
            label: "Places",
            places: index.iter().filter(|p| p.default_row).cloned().collect(),
        }
    } else {
        AtRest {
            label: "Recent",
            places,
        }
    }
}

// ----------------------------------------------------------------- matching --

/// Score bonuses. Tuned so a word-start match beats a mid-word one and a
/// contiguous run beats a scattered one — i.e. `Depth Box` outranks `Inbox` for
/// `bo`, and `New deck…` outranks `New binder…` for `de`.
const START_BONUS: i32 = 10;
const WORD_BONUS: i32 = 8;
const CONTIGUOUS_BONUS: i32 = 6;
const CHAR_BASE: i32 = 1;

fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | '/' | ':' | ',' | '(' | '·' | '—' | '…')
}

/// Fuzzy score for `needle` against `haystack`, or `None` when it doesn't match.
///
/// **Subsequence matching, but anchored:** after the first character every
/// further one must be either contiguous with the previous match or at a word
/// start. That is what keeps `trabin` → `Trade Binder` and `cd` →
/// `Commander Deck` working while refusing `de` → `Undo last move`, which a
/// plain subsequence match accepts and which would put noise in a list whose
/// whole value is that the top row is the right one. Whitespace in the needle is
/// ignored, so `tra bin` and `trabin` are the same query.
///
/// An empty needle scores 0 for everything — callers use the at-rest list
/// instead, but "matches nothing" would be the wrong answer for a filter.
pub fn score(haystack: &str, needle: &str) -> Option<i32> {
    let needle: Vec<char> = needle
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().flat_map(char::to_lowercase).collect();
    if needle.len() > hay.len() {
        return None;
    }

    // `row[p]` = best score for matching needle[..=j] with needle[j] landing on
    // hay[p]. O(n·m) with n,m both tiny (a collection name and a typed query).
    let mut row: Vec<Option<i32>> = (0..hay.len())
        .map(|p| {
            (hay[p] == needle[0]).then(|| {
                CHAR_BASE
                    + if p == 0 {
                        START_BONUS
                    } else if is_separator(hay[p - 1]) {
                        WORD_BONUS
                    } else {
                        0
                    }
            })
        })
        .collect();

    for &want in &needle[1..] {
        let mut next: Vec<Option<i32>> = vec![None; hay.len()];
        // Best score of any previous-character match *strictly before* p.
        let mut best_before: Option<i32> = None;
        for p in 0..hay.len() {
            if hay[p] == want {
                let contiguous = (p > 0)
                    .then(|| row[p - 1])
                    .flatten()
                    .map(|s| s + CHAR_BASE + CONTIGUOUS_BONUS);
                let at_word_start = (p > 0 && is_separator(hay[p - 1]))
                    .then_some(best_before)
                    .flatten()
                    .map(|s| s + CHAR_BASE + WORD_BONUS);
                next[p] = contiguous.max(at_word_start);
            }
            if let Some(s) = row[p] {
                best_before = best_before.max(Some(s));
            }
        }
        row = next;
    }
    row.into_iter().flatten().max()
}

/// A query's matches, grouped the way P2 groups them: COLLECTIONS and COMMANDS,
/// either group dropping out when it has no match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub places: Vec<Place>,
    pub commands: Vec<PaletteCommand>,
    /// Whether COMMANDS is drawn above COLLECTIONS — see [`ranked`].
    pub commands_first: bool,
}

/// Rank the index and the command registry against a query.
///
/// Ties break on the shorter name and then on registry order, so the result is
/// deterministic — which matters because the pre-selected row is simply the
/// first one.
///
/// **The groups are ordered too, and they have to be.** P2 draws COLLECTIONS
/// above COMMANDS and the design also says the best match is pre-selected; with
/// a fixed group order those two are only compatible while a place happens to
/// outscore every command. It doesn't always: typing `undo` scores
/// `Undo last move` at a prefix while a collection whose name merely *contains*
/// the word scores lower, and a fixed order pre-selected the collection. (Found
/// by `command-palette.spec.ts`, whose scratch binder was named
/// `zz-e2e-palette-undo-…`.) So whichever group holds the better top match leads.
/// For P2's own query (`tra`) that is still COLLECTIONS, so the wireframe is
/// unchanged.
pub fn ranked(index: &[Place], query: &str) -> Ranked {
    let mut places: Vec<(i32, usize, Place)> = index
        .iter()
        .enumerate()
        .filter_map(|(i, p)| score(&p.name, query).map(|s| (s, i, p.clone())))
        .collect();
    places.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.2.name.chars().count().cmp(&b.2.name.chars().count()))
            .then_with(|| a.1.cmp(&b.1))
    });

    let mut commands: Vec<(i32, usize, PaletteCommand)> = PaletteCommand::ALL
        .iter()
        .enumerate()
        .filter_map(|(i, c)| score(c.label(), query).map(|s| (s, i, *c)))
        .collect();
    commands.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    // Strictly greater: a tie keeps the wireframe's order.
    let commands_first = match (places.first(), commands.first()) {
        (Some(place), Some(command)) => command.0 > place.0,
        _ => false,
    };

    Ranked {
        places: places.into_iter().map(|(_, _, p)| p).collect(),
        commands: commands.into_iter().map(|(_, _, c)| c).collect(),
        commands_first,
    }
}

// ------------------------------------------------------------ the component --

/// Mount once, at the app shell. Renders nothing at all unless the viewport is
/// desktop *and* someone is signed in — see the module doc for why that gate is
/// also the hydration contract.
#[component]
pub fn CommandPalette() -> impl IntoView {
    let user = expect_context::<crate::shell::CurrentUserResource>().0;
    let desktop = desktop_signal();

    // The session, mirrored through an `Effect` rather than read where the gate
    // is evaluated. Two reasons, and the second is why a plain derived read is
    // wrong even though it compiles: a resource read outside a `Suspense` /
    // `Transition` / effect warns at runtime ("can cause hydration mismatch
    // errors"), and `hydration-check.mjs` counts that warning as a failure. An
    // `Effect` is the exempt path, and it also cannot run before hydration
    // finishes — so this signal is `false` in the SSR markup and `false` again
    // during the hydration render, which is exactly the contract the module doc
    // relies on. No `Transition` instead: that would put the palette's markup
    // inside an async boundary, and a global overlay has nothing to stream.
    let signed_in = RwSignal::new(false);
    Effect::new(move |_| {
        signed_in.set(matches!(user.get(), Some(Ok(Some(_)))));
    });

    let enabled = Signal::derive(move || desktop.get() && signed_in.get());

    view! {
        <Show when=move || enabled.get()>
            <PaletteBody />
        </Show>
    }
}

/// `true` while the viewport matches [`DESKTOP_MEDIA`]. Starts `false` and is
/// corrected in an `Effect` (client-only), then **kept** correct by a `change`
/// listener on the same `MediaQueryList`.
///
/// Public for the bench, which renders it as a readout: on the real pages the
/// palette's absence has two possible causes (viewport, session) and the Android
/// dev proxy can only reach the anonymous ones, so the bench is where the
/// viewport half is observable on its own.
pub fn desktop_signal() -> Signal<bool> {
    let desktop = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    {
        use leptos::prelude::LocalStorage;
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::JsCast;

        // Kept for the lifetime of this component so the listener can be
        // removed again; the palette lives as long as the shell, so this is one
        // registration per document load. `new_local` because neither a
        // `MediaQueryList` nor a `Closure` is `Send` (nor needs to be — this
        // whole block is wasm-only).
        type Watch =
            StoredValue<Option<(web_sys::MediaQueryList, Closure<dyn FnMut()>)>, LocalStorage>;
        let registration: Watch = StoredValue::new_local(None);

        // In an Effect, not the body: setting `desktop` synchronously during the
        // hydration render would mount `PaletteBody` against SSR markup that has
        // no palette in it.
        Effect::new(move |_| {
            let Some(mql) = window().match_media(DESKTOP_MEDIA).ok().flatten() else {
                return;
            };
            desktop.set(mql.matches());
            let watched = mql.clone();
            // A `FnMut()` re-reading `matches()`, rather than a handler taking a
            // `MediaQueryListEvent` — that type is not in the crate's web-sys
            // feature set, and the query is the source of truth anyway.
            let handler = Closure::wrap(Box::new(move || {
                desktop.set(watched.matches());
            }) as Box<dyn FnMut()>);
            if mql
                .add_event_listener_with_callback("change", handler.as_ref().unchecked_ref())
                .is_ok()
            {
                registration.set_value(Some((mql, handler)));
            }
        });

        on_cleanup(move || {
            if let Some((mql, handler)) = registration.try_update_value(Option::take).flatten() {
                let _ = mql.remove_event_listener_with_callback(
                    "change",
                    handler.as_ref().unchecked_ref(),
                );
            }
        });
    }

    desktop.into()
}

/// The palette proper: the chord listener, the recent ring, the place index, and
/// what each row does. Client-only by construction (see [`CommandPalette`]).
#[component]
fn PaletteBody() -> impl IntoView {
    let open = RwSignal::new(false);
    let tree = expect_context::<CollectionTreeResource>().0;
    let toast = expect_context::<ToastHandle>();
    let manage = use_context::<TreeManage>();
    let last_move = use_context::<LastMoveState>();
    let revision = use_context::<crate::my::move_selection::HoldingsRevision>();
    let pathname = use_location().pathname;
    let navigate = use_navigate();

    let recents = RwSignal::new(load_recents());
    let current = RwSignal::new(None::<PlaceKey>);

    // The place index, projected off the shared tree read **in an `Effect`**.
    //
    // Not a `Memo` over `tree.get()`, though that is what this was: a resource
    // read outside a `Suspense`/`Transition`/effect warns at runtime and
    // `hydration-check.mjs` fails the page for it. The `Effect` is also the
    // cheaper shape — it re-projects once per *fetch* instead of once per read,
    // and the rows closure reads this on every keystroke, so a `Memo` would have
    // re-`assemble`d (and re-cloned) the whole DTO per character.
    let index = RwSignal::new(place_index(None));
    Effect::new(move |_| {
        let next = match tree.get() {
            Some(Some(Ok(dto))) => place_index(Some(&assemble(dto))),
            // No tree yet (still resolving) or a failed read: the system places
            // and mode jumps still work, which beats an empty palette.
            _ => place_index(None),
        };
        // Only on a real change: `set` notifies unconditionally, and a refetch
        // that returned the same tree would otherwise rebuild every row.
        if next != index.get_untracked() {
            index.set(next);
        }
    });

    // Every navigation is a visit. Recording the *current* place too (and
    // excluding it in `at_rest`) is what makes the first RECENT row the place
    // you came from.
    Effect::new(move |_| {
        let key = place_key_for_path(&pathname.get());
        current.set(key);
        if let Some(key) = key {
            let next = push_recent(&recents.read_untracked(), key, RECENT_CAP);
            store_recents(&next);
            recents.set(next);
        }
    });

    // The chord. `prevent_default` matters on Windows/Linux, where Ctrl+K is
    // the browser's own address-bar search.
    #[cfg(feature = "hydrate")]
    {
        let mac = is_mac();
        let handle = window_event_listener(leptos::ev::keydown, move |ev| {
            if is_palette_chord(&ev.key(), ev.meta_key(), ev.ctrl_key(), mac) {
                ev.prevent_default();
                open.update(|o| *o = !*o);
            }
        });
        on_cleanup(move || handle.remove());
    }

    let run = Callback::new(move |action: PaletteAction| {
        open.set(false);
        match action {
            PaletteAction::Go(href) => navigate(&href, Default::default()),
            PaletteAction::Run(PaletteCommand::NewBinder) => {
                // Navigate first: the create dialog is mounted beside the tree,
                // which only exists in My-cards mode. The open flag is the
                // shell's, so setting it now survives the navigation and the
                // dialog comes up already open.
                navigate("/my", Default::default());
                if let Some(manage) = manage {
                    manage.open_create(None, CollectionKind::Binder);
                }
            }
            PaletteAction::Run(PaletteCommand::NewDeck) => {
                navigate("/my", Default::default());
                if let Some(manage) = manage {
                    manage.open_create(None, CollectionKind::Deck);
                }
            }
            PaletteAction::Run(PaletteCommand::UndoLastMove) => {
                let Some(state) = last_move else { return };
                let Some(LastMove { move_ids }) = state.0.get_untracked() else {
                    toast.show(ToastOptions::message(
                        "Nothing to undo yet — moves you make in this session land here.",
                    ));
                    return;
                };
                // Clear before the call: a second ⌘K ⏎ must not fire the same
                // reversal twice while the first is still in flight (it would be
                // harmless — undo is idempotent — but the second toast would
                // claim a second undo happened). Restored on failure below.
                state.0.set(None);
                let restore = move_ids.clone();
                spawn_local(async move {
                    let result = if move_ids.len() == 1 {
                        crate::undo_move(move_ids[0]).await
                    } else {
                        crate::undo_selection_move(move_ids).await
                    };
                    match result {
                        Ok(()) => {
                            tree.refetch();
                            if let Some(r) = revision {
                                r.bump();
                            }
                            toast.show(
                                ToastOptions::message("Undid the last move")
                                    .kind(ToastKind::Success),
                            );
                        }
                        Err(e) => {
                            // Put the handle back: the reversal did not land, so
                            // the move is still the last one and the command has
                            // to stay usable. Only if nothing newer arrived
                            // meanwhile — that one is genuinely the last move now.
                            if state.0.get_untracked().is_none() {
                                state.0.set(Some(LastMove { move_ids: restore }));
                            }
                            toast.show(
                                ToastOptions::message(format!(
                                    "Couldn't undo: {}",
                                    crate::catalog::describe_error(&e).1
                                ))
                                .kind(ToastKind::Error),
                            );
                        }
                    }
                });
            }
        }
    });

    let at_rest_rows =
        Signal::derive(move || at_rest(&index.read(), &recents.read(), current.get(), RECENT_CAP));

    view! { <PaletteSurface open index=index.into() at_rest=at_rest_rows on_run=run /> }
}

/// The dialog, the field, the grouped rows and the footer.
///
/// Split from [`PaletteBody`] so the bench can render it with a static index and
/// a logging handler — and because everything here has to live *inside* the
/// `Command` that `CommandDialog` creates, which is what makes
/// [`use_command_nav`] reachable.
#[component]
pub fn PaletteSurface(
    open: RwSignal<bool>,
    index: Signal<Vec<Place>>,
    at_rest: Signal<AtRest>,
    on_run: Callback<PaletteAction>,
) -> impl IntoView {
    view! {
        // `should_filter=false`: this surface ranks its own rows (module doc).
        <CommandDialog
            id=PALETTE_ID
            open=open
            should_filter=false
            class="sm:max-w-xl"
        >
            <PaletteContents open index at_rest on_run />
        </CommandDialog>
    }
}

#[component]
fn PaletteContents(
    open: RwSignal<bool>,
    index: Signal<Vec<Place>>,
    at_rest: Signal<AtRest>,
    on_run: Callback<PaletteAction>,
) -> impl IntoView {
    let nav = use_command_nav().expect("PaletteContents renders inside a Command");
    // The query, mirrored out of `CommandInput` — `Command` keeps its own copy
    // private, and we need it to build the rows.
    let query = RwSignal::new(String::new());

    // A reopen starts at rest. `Command` owns the field's value, so clearing
    // ours is not enough.
    Effect::new(move |_| {
        if !open.get() {
            query.set(String::new());
            nav.set_query("");
        }
    });

    // Hold the caret. The field only exists while the palette is open, so this
    // runs on the mount that just happened; the timeout is the fallback for a
    // browser that has not attached the node yet.
    Effect::new(move |_| {
        if open.get() {
            #[cfg(feature = "hydrate")]
            if !focus_field() {
                set_timeout(
                    || {
                        focus_field();
                    },
                    std::time::Duration::from_millis(0),
                );
            }
        }
    });

    // What is on screen right now. At rest the whole fixed command registry is
    // shown beside the places group (P1); typing ranks both (P2).
    let rows = Signal::derive(move || {
        let q = query.get();
        if q.trim().is_empty() {
            let AtRest { label, places } = at_rest.get();
            RowSet {
                places_label: label,
                places,
                commands: PaletteCommand::ALL.to_vec(),
                // P1 always draws the places group first: at rest there is no
                // query to be a better match for.
                commands_first: false,
            }
        } else {
            let Ranked {
                places,
                commands,
                commands_first,
            } = ranked(&index.read(), &q);
            RowSet {
                places_label: "Collections",
                places,
                commands,
                commands_first,
            }
        }
    });

    view! {
        // Mounted only while open: a closed dialog keeps its box in the DOM (the
        // `Sheet`/`popover` trap the e2e notes call out), so leaving the rows
        // mounted would leave real, clickable targets behind a scrim.
        <Show when=move || open.get()>
            <div class="flex items-center gap-2 border-b px-4">
                <span aria-hidden="true" class="text-muted-foreground shrink-0 text-sm">
                    "⌕"
                </span>
                <CommandInput
                    id=PALETTE_INPUT_ID
                    placeholder="Where to?"
                    on_search_change=Callback::new(move |v: String| query.set(v))
                />
            </div>
            <CommandList class="max-h-[21rem] p-1.5" {..} data-testid="palette-list">
                <CommandEmpty class="text-muted-foreground py-6 text-sm" {..} data-testid="palette-empty">
                    "No matches"
                </CommandEmpty>
                // **The remount, and why it is a `For` over one item.** A bare
                // dynamic closure looked like it rebuilt the list, and does
                // not: tachys diffs an unkeyed `Vec` of views positionally and
                // *reuses* the DOM nodes, so the rows survived every keystroke
                // (measured — the first version of
                // `command-palette.spec.ts`'s node-identity test caught it).
                // Keying one item on the row set's identity is what actually
                // disposes the old rows and mounts the new order, which is what
                // `command`'s mount-ordered registry needs (module doc, and
                // `command.rs`'s `visible_ids`). The key changes whenever the
                // rendered list changes at all — content or order — so an
                // unchanged list is left alone.
                <For each=move || [rows.get()] key=RowSet::key let:set>
                    {group_views(set, on_run)}
                </For>
            </CommandList>
            <PaletteFooter />
        </Show>
    }
}

/// The rows on screen: one places group (RECENT / PLACES / COLLECTIONS) and the
/// commands group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSet {
    pub places_label: &'static str,
    pub places: Vec<Place>,
    pub commands: Vec<PaletteCommand>,
    /// COMMANDS above the places group — set only when a command outranks every
    /// place (see [`ranked`]).
    pub commands_first: bool,
}

impl RowSet {
    /// An identity that changes whenever the rendered list changes — its
    /// contents, their names, or their order. `For` keyed on this is what makes
    /// a query change a real remount rather than an in-place diff; see the call
    /// site for the measurement that forced it.
    pub fn key(&self) -> String {
        let mut key = String::from(self.places_label);
        key.push(if self.commands_first { '^' } else { 'v' });
        for p in &self.places {
            key.push('\u{1f}');
            key.push_str(&p.key.token());
            key.push('\u{1f}');
            key.push_str(&p.name);
        }
        for c in &self.commands {
            key.push('\u{1f}');
            key.push_str(c.slug());
        }
        key
    }
}

/// The two groups, in document order. Empty groups drop out entirely — label
/// included, per P2.
fn group_views(set: RowSet, on_run: Callback<PaletteAction>) -> AnyView {
    let RowSet {
        places_label,
        places,
        commands,
        commands_first,
    } = set;
    let places_group = (!places.is_empty()).then(|| {
        view! {
            <CommandGroup>
                <CommandGroupLabel class="tracking-wide uppercase">{places_label}</CommandGroupLabel>
                {places.into_iter().map(|place| view! { <PlaceRow place on_run /> }).collect_view()}
            </CommandGroup>
        }
    });
    let commands_group = (!commands.is_empty()).then(|| {
        view! {
            <CommandGroup>
                <CommandGroupLabel class="tracking-wide uppercase">"Commands"</CommandGroupLabel>
                {commands
                    .into_iter()
                    .map(|command| view! { <CommandRow command on_run /> })
                    .collect_view()}
            </CommandGroup>
        }
    });
    // Two arms rather than one ordered pair: the groups are different view types,
    // and swapping them has to change the *DOM* order, not just a class.
    if commands_first {
        view! {
            {commands_group}
            {places_group}
        }
        .into_any()
    } else {
        view! {
            {places_group}
            {commands_group}
        }
        .into_any()
    }
}

#[component]
fn PlaceRow(place: Place, on_run: Callback<PaletteAction>) -> impl IntoView {
    let Place {
        key,
        name,
        meta,
        href,
        icon,
        ..
    } = place;
    let token = key.token();
    view! {
        <CommandItem
            value=name.clone()
            class="cursor-pointer gap-2"
            on_select=Callback::new(move |()| on_run.run(PaletteAction::Go(href.clone())))
        >
            // The test seam rides an inner element: `CommandItem` takes no
            // attribute spread, and its own `aria-selected` already means
            // "keyboard-highlighted" for a screen reader.
            <span aria-hidden="true" class="shrink-0">
                {icon}
            </span>
            <span class="truncate" data-testid="palette-row" data-palette-key=token>
                {name}
            </span>
            {(!meta.is_empty())
                .then(|| {
                    view! {
                        <span
                            class="text-muted-foreground ml-auto shrink-0 text-xs"
                            data-testid="palette-meta"
                        >
                            {meta}
                        </span>
                    }
                })}
        </CommandItem>
    }
}

#[component]
fn CommandRow(command: PaletteCommand, on_run: Callback<PaletteAction>) -> impl IntoView {
    view! {
        <CommandItem
            value=command.label()
            class="cursor-pointer gap-2"
            on_select=Callback::new(move |()| on_run.run(PaletteAction::Run(command)))
        >
            <span aria-hidden="true" class="shrink-0">
                "⚡"
            </span>
            <span
                class="truncate"
                data-testid="palette-row"
                data-palette-key=format!("cmd:{}", command.slug())
            >
                {command.label()}
            </span>
        </CommandItem>
    }
}

/// The keystroke ledger, verbatim from the wireframe's P1/P2 footer.
#[component]
fn PaletteFooter() -> impl IntoView {
    view! {
        <CommandFooter class="gap-3 px-3" {..} data-testid="palette-footer">
            <span class="inline-flex items-center gap-1">
                <Kbd>"↑↓"</Kbd>
                "navigate"
            </span>
            <span class="inline-flex items-center gap-1">
                <Kbd>"⏎"</Kbd>
                "open"
            </span>
            <span class="inline-flex items-center gap-1">
                <Kbd>"esc"</Kbd>
                "close"
            </span>
        </CommandFooter>
    }
}

// ------------------------------------------------------------ browser edges --

#[cfg(feature = "hydrate")]
fn is_mac() -> bool {
    window()
        .navigator()
        .platform()
        .map(|p| p.to_lowercase().contains("mac"))
        .unwrap_or(false)
}

/// Focus the search field; `false` when the node isn't in the document yet.
#[cfg(feature = "hydrate")]
fn focus_field() -> bool {
    use wasm_bindgen::JsCast;
    document()
        .get_element_by_id(PALETTE_INPUT_ID)
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|el| {
            let _ = el.focus();
            true
        })
        .unwrap_or(false)
}

#[cfg(feature = "hydrate")]
fn local_storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

fn load_recents() -> Vec<PlaceKey> {
    #[cfg(feature = "hydrate")]
    {
        if let Some(raw) =
            local_storage().and_then(|s| s.get_item(RECENT_STORAGE_KEY).ok().flatten())
        {
            return parse_recents(&raw);
        }
    }
    Vec::new()
}

fn store_recents(ring: &[PlaceKey]) {
    #[cfg(feature = "hydrate")]
    if let Some(storage) = local_storage() {
        // Ignored on purpose: a browser with storage denied (private mode,
        // quota) keeps a session-only ring rather than losing the palette.
        let _ = storage.set_item(RECENT_STORAGE_KEY, &serialize_recents(ring));
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = ring;
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{CollectionSummary, CollectionTree, CollectionTreeRow};

    // ------------------------------------------------------------- chord --

    #[test]
    fn the_chord_is_cmd_k_on_mac_and_ctrl_k_elsewhere() {
        assert!(is_palette_chord("k", true, false, true));
        assert!(is_palette_chord("k", false, true, false));
        // The wrong platform's modifier is not the chord.
        assert!(!is_palette_chord("k", false, true, true));
        assert!(!is_palette_chord("k", true, false, false));
        // Bare k is typing.
        assert!(!is_palette_chord("k", false, false, true));
        // Capital K (shift held) still opens it — the browser reports "K".
        assert!(is_palette_chord("K", true, false, true));
    }

    #[test]
    fn both_modifiers_together_are_someone_elses_chord() {
        assert!(!is_palette_chord("k", true, true, true));
        assert!(!is_palette_chord("k", true, true, false));
    }

    #[test]
    fn slash_is_never_the_palette() {
        // design/command-palette.md: `/` belongs to the in-collection quick-add.
        for (meta, ctrl, mac) in [
            (false, false, true),
            (false, false, false),
            (true, false, true),
            (false, true, false),
        ] {
            assert!(!is_palette_chord("/", meta, ctrl, mac));
        }
    }

    // ------------------------------------------------------------ scoring --

    #[test]
    fn a_word_start_outranks_a_mid_word_match() {
        // `bo` in `Bulk Box` starts a word; in `Inbox` it does not.
        let boxy = score("Bulk Box", "bo").unwrap();
        let inbox = score("Inbox", "bo").unwrap();
        assert!(boxy > inbox, "{boxy} should beat {inbox}");
    }

    #[test]
    fn a_prefix_outranks_everything_else() {
        let prefix = score("Depth Box", "de").unwrap();
        let inner = score("Trade Binder", "de").unwrap();
        assert!(prefix > inner, "{prefix} should beat {inner}");
    }

    #[test]
    fn matching_jumps_word_boundaries_but_not_mid_word_gaps() {
        // The useful fuzzy case: initials and prefixes across words.
        assert!(score("Commander Deck", "cd").is_some());
        assert!(score("Trade Binder", "trabin").is_some());
        assert!(score("Depth Drawer", "dd").is_some());
        // …and the noise it must refuse: `d` in Undo, `e` in mOvE.
        assert!(
            score("Undo last move", "de").is_none(),
            "a plain subsequence match would accept this"
        );
    }

    #[test]
    fn whitespace_in_the_query_is_ignored() {
        assert_eq!(
            score("Trade Binder", "tra bin"),
            score("Trade Binder", "trabin")
        );
    }

    #[test]
    fn an_empty_query_matches_everything_at_zero() {
        assert_eq!(score("Anything", ""), Some(0));
        assert_eq!(score("Anything", "   "), Some(0));
    }

    #[test]
    fn a_longer_needle_than_haystack_cannot_match() {
        assert!(score("Ab", "abc").is_none());
    }

    #[test]
    fn matching_is_case_insensitive_both_ways() {
        assert!(score("SHOEBOX", "shoe").is_some());
        assert!(score("shoebox", "SHOE").is_some());
    }

    // ------------------------------------------------------------ ranking --

    fn place(name: &str) -> Place {
        Place {
            key: PlaceKey::Collection(Id::new_v4()),
            name: name.into(),
            meta: String::new(),
            href: format!("/my/collections/{name}"),
            icon: "🗂",
            default_row: false,
        }
    }

    fn names(places: &[Place]) -> Vec<&str> {
        places.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn the_best_match_ranks_first() {
        // P2's exact case: typing `tra` puts Trade Binder above Trade duplicates
        // (same prefix, shorter name breaks the tie) and drops the rest.
        let index = [
            place("Shoebox"),
            place("Trade duplicates"),
            place("Trade Binder"),
        ];
        let ranked = ranked(&index, "tra");
        assert_eq!(names(&ranked.places), ["Trade Binder", "Trade duplicates"]);
    }

    #[test]
    fn ranking_reorders_as_the_query_grows() {
        // This is the property that makes the mount-order caveat live for this
        // surface: the *same* rows come back in a different order, so the row
        // views must be rebuilt rather than moved (see the module doc).
        let index = [place("Shoebox"), place("Depth Box")];
        assert_eq!(
            names(&ranked(&index, "e").places),
            ["Shoebox", "Depth Box"],
            "`e` is mid-word in both, so the shorter name leads"
        );
        assert_eq!(
            names(&ranked(&index, "eb").places),
            ["Depth Box", "Shoebox"],
            "`eb` jumps a word boundary in Depth Box, which outscores Shoebox's \
             contiguous mid-word run — the same rows, the other way round"
        );
    }

    #[test]
    fn commands_rank_too_and_keep_registry_order_on_a_tie() {
        let ranked = ranked(&[], "new");
        assert_eq!(
            ranked.commands,
            vec![PaletteCommand::NewBinder, PaletteCommand::NewDeck],
            "both are prefix matches; the registry order breaks the tie"
        );
        assert!(ranked.places.is_empty(), "the group must drop out entirely");
    }

    #[test]
    fn a_command_that_outranks_every_place_leads() {
        // P2's own query keeps the wireframe's order…
        let index = [place("Trade Binder"), place("Trade duplicates")];
        assert!(!ranked(&index, "tra").commands_first);
        // …but a collection that merely *contains* the word must not out-place a
        // command it loses to. `undo` is a prefix of `Undo last move` and only a
        // word-start hit inside this name.
        let index = [place("zz-e2e-palette-undo-1")];
        let r = ranked(&index, "undo");
        assert_eq!(r.commands, vec![PaletteCommand::UndoLastMove]);
        assert!(
            r.commands_first,
            "the pre-selected row is the first one drawn, so the better match's \
             group has to lead or `best match pre-selected` is false"
        );
    }

    #[test]
    fn a_group_with_nothing_to_compare_against_never_reorders() {
        // Only commands match: nothing to be "first" relative to.
        assert!(!ranked(&[], "new deck").commands_first);
        // Only places match.
        assert!(!ranked(&[place("Shoebox")], "shoebox").commands_first);
    }

    #[test]
    fn a_query_matching_nothing_ranks_nothing() {
        let ranked = ranked(&[place("Shoebox")], "zzz");
        assert!(ranked.places.is_empty() && ranked.commands.is_empty());
    }

    // ------------------------------------------------------------ row set --

    fn set(label: &'static str, places: Vec<Place>, commands: Vec<PaletteCommand>) -> RowSet {
        RowSet {
            places_label: label,
            places,
            commands,
            commands_first: false,
        }
    }

    #[test]
    fn the_row_set_key_changes_when_the_order_does() {
        // The whole point: reordering the *same* rows must remount them, because
        // `command`'s registry is ordered by mount (see the module doc).
        let a = place("Shoebox");
        let b = place("Depth Box");
        let forward = set("Collections", vec![a.clone(), b.clone()], vec![]);
        let reversed = set("Collections", vec![b, a], vec![]);
        assert_ne!(forward.key(), reversed.key());
    }

    #[test]
    fn the_row_set_key_is_stable_for_an_identical_list() {
        let places = vec![place("Shoebox")];
        let commands = vec![PaletteCommand::NewDeck];
        assert_eq!(
            set("Recent", places.clone(), commands.clone()).key(),
            set("Recent", places, commands).key(),
            "an unchanged list must not churn the DOM every read"
        );
    }

    #[test]
    fn the_row_set_key_notices_a_group_swap() {
        // Swapping the group order is a DOM reorder like any other, so it has to
        // remount too.
        let places = vec![place("Shoebox")];
        let commands = vec![PaletteCommand::UndoLastMove];
        let mut swapped = set("Collections", places.clone(), commands.clone());
        swapped.commands_first = true;
        assert_ne!(set("Collections", places, commands).key(), swapped.key());
    }

    #[test]
    fn the_row_set_key_notices_a_rename_a_label_change_and_a_command_change() {
        let base = set(
            "Recent",
            vec![place("Shoebox")],
            vec![PaletteCommand::NewDeck],
        );
        let mut renamed_place = base.places[0].clone();
        renamed_place.name = "Deck box".into();
        // Same id, new name — the row's text changed, so it has to be rebuilt.
        assert_ne!(
            base.key(),
            set("Recent", vec![renamed_place], vec![PaletteCommand::NewDeck]).key()
        );
        // RECENT → COLLECTIONS is a different group heading.
        assert_ne!(
            base.key(),
            set(
                "Collections",
                base.places.clone(),
                vec![PaletteCommand::NewDeck]
            )
            .key()
        );
        assert_ne!(
            base.key(),
            set(
                "Recent",
                base.places.clone(),
                vec![PaletteCommand::NewBinder]
            )
            .key()
        );
    }

    #[test]
    fn the_command_registry_is_exactly_three_and_excludes_sign_out() {
        assert_eq!(PaletteCommand::ALL.len(), 3);
        let labels: Vec<_> = PaletteCommand::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(labels, ["New binder…", "New deck…", "Undo last move"]);
        assert!(
            !labels.contains(&"Sign out"),
            "considered and dropped (design/command-palette.md)"
        );
    }

    // -------------------------------------------------------------- index --

    fn row(id: u128, parent: Option<u128>, name: &str, is_inbox: bool) -> CollectionTreeRow {
        CollectionTreeRow {
            summary: CollectionSummary {
                id: Id::from_u128(id),
                parent_id: parent.map(Id::from_u128),
                kind: CollectionKind::Binder,
                name: name.into(),
                is_inbox,
                position: 0.0,
                format: None,
            },
            present: 0,
        }
    }

    /// The seeded shape: Inbox, Trade Binder, Shoebox > Rares.
    fn fixture() -> AssembledTree {
        assemble(CollectionTree {
            collections: vec![
                row(1, None, "Trade Binder", false),
                row(2, None, "Shoebox", false),
                row(3, Some(2), "Rares", false),
                row(4, None, "Inbox", true),
            ],
            shopping_short: 0,
        })
    }

    #[test]
    fn the_index_flattens_the_tree_between_the_system_places() {
        let index = place_index(Some(&fixture()));
        assert_eq!(
            names(&index),
            [
                "All cards",
                "Inbox",
                "Trade Binder",
                "Shoebox",
                "Rares",
                "Shopping list",
                "Go to Catalog",
                "Go to My cards",
            ]
        );
    }

    #[test]
    fn a_nested_collection_carries_its_parent_path() {
        let index = place_index(Some(&fixture()));
        let rares = index.iter().find(|p| p.name == "Rares").unwrap();
        assert_eq!(rares.meta, "Shoebox", "the wireframe's `Rares — Shoebox`");
        let trade = index.iter().find(|p| p.name == "Trade Binder").unwrap();
        assert_eq!(trade.meta, "", "a root has no path to show");
    }

    #[test]
    fn deep_nesting_shows_the_whole_path() {
        let tree = assemble(CollectionTree {
            collections: vec![
                row(1, None, "Depth Box", false),
                row(2, Some(1), "Depth Shelf", false),
                row(3, Some(2), "Depth Drawer", false),
            ],
            shopping_short: 0,
        });
        let index = place_index(Some(&tree));
        let drawer = index.iter().find(|p| p.name == "Depth Drawer").unwrap();
        assert_eq!(drawer.meta, "Depth Box / Depth Shelf");
    }

    #[test]
    fn the_inbox_appears_once_even_though_it_is_a_system_place() {
        let index = place_index(Some(&fixture()));
        assert_eq!(
            index.iter().filter(|p| p.name == "Inbox").count(),
            1,
            "it is a tree row; adding it as a system place too would double it"
        );
    }

    #[test]
    fn without_a_tree_the_system_places_and_mode_jumps_still_exist() {
        let index = place_index(None);
        assert_eq!(
            names(&index),
            [
                "All cards",
                "Shopping list",
                "Go to Catalog",
                "Go to My cards"
            ]
        );
    }

    // ---------------------------------------------------------- the paths --

    #[test]
    fn pathnames_map_onto_place_keys() {
        assert_eq!(place_key_for_path("/my"), Some(PlaceKey::AllCards));
        assert_eq!(place_key_for_path("/my/"), Some(PlaceKey::AllCards));
        assert_eq!(place_key_for_path("/my/shopping"), Some(PlaceKey::Shopping));
        let id = Id::new_v4();
        assert_eq!(
            place_key_for_path(&format!("/my/collections/{id}")),
            Some(PlaceKey::Collection(id))
        );
        // A collection's subpage is still that collection.
        assert_eq!(
            place_key_for_path(&format!("/my/collections/{id}/needs")),
            Some(PlaceKey::Collection(id))
        );
    }

    #[test]
    fn non_places_are_not_recorded() {
        for path in [
            "/catalog",
            "/",
            "/login",
            "/cards/abc",
            "/my/collections/nope",
        ] {
            assert_eq!(place_key_for_path(path), None, "{path}");
        }
    }

    // -------------------------------------------------------- recent ring --

    #[test]
    fn the_ring_is_most_recent_first_deduplicated_and_capped() {
        let a = PlaceKey::Collection(Id::from_u128(1));
        let b = PlaceKey::Collection(Id::from_u128(2));
        let ring = push_recent(&[], a, 3);
        let ring = push_recent(&ring, b, 3);
        assert_eq!(ring, vec![b, a]);
        // Revisiting `a` moves it up rather than duplicating it.
        let ring = push_recent(&ring, a, 3);
        assert_eq!(ring, vec![a, b]);
        let ring = push_recent(&ring, PlaceKey::Shopping, 2);
        assert_eq!(ring, vec![PlaceKey::Shopping, a], "capped at 2");
    }

    #[test]
    fn the_ring_round_trips_through_storage() {
        let ring = vec![
            PlaceKey::Collection(Id::from_u128(7)),
            PlaceKey::AllCards,
            PlaceKey::Shopping,
            PlaceKey::Catalog,
            PlaceKey::MyCards,
        ];
        assert_eq!(parse_recents(&serialize_recents(&ring)), ring);
    }

    #[test]
    fn a_corrupt_stored_ring_degrades_instead_of_failing() {
        assert_eq!(parse_recents(""), vec![]);
        assert_eq!(
            parse_recents("all,,not-a-uuid, shopping "),
            vec![PlaceKey::AllCards, PlaceKey::Shopping],
            "unparseable tokens drop; the rest survive"
        );
    }

    // ------------------------------------------------------------- at rest --

    #[test]
    fn at_rest_shows_the_ring_most_recent_first_without_the_current_place() {
        let index = place_index(Some(&fixture()));
        let trade = index.iter().find(|p| p.name == "Trade Binder").unwrap().key;
        let rares = index.iter().find(|p| p.name == "Rares").unwrap().key;
        let rest = at_rest(&index, &[rares, trade, PlaceKey::AllCards], Some(rares), 5);
        assert_eq!(rest.label, "Recent");
        assert_eq!(
            names(&rest.places),
            ["Trade Binder", "All cards"],
            "⌘K ⏎ must bounce to the last collection, not reload this one"
        );
    }

    #[test]
    fn at_rest_drops_a_collection_that_no_longer_exists() {
        let index = place_index(Some(&fixture()));
        let gone = PlaceKey::Collection(Id::new_v4());
        let rest = at_rest(&index, &[gone, PlaceKey::Shopping], None, 5);
        assert_eq!(
            names(&rest.places),
            ["Shopping list"],
            "a deleted collection must not offer a row every ⏎ would 404 on"
        );
    }

    #[test]
    fn at_rest_falls_back_to_the_system_places_on_a_cold_start() {
        let index = place_index(Some(&fixture()));
        let rest = at_rest(&index, &[], None, 5);
        assert_eq!(rest.label, "Places", "nothing visited is not 'recent'");
        assert_eq!(
            names(&rest.places),
            ["All cards", "Inbox", "Shopping list"],
            "so the pre-selected row is a place, never `New binder…`"
        );
    }

    #[test]
    fn at_rest_honors_the_cap() {
        let index = place_index(Some(&fixture()));
        let keys: Vec<PlaceKey> = index.iter().take(4).map(|p| p.key).collect();
        assert_eq!(at_rest(&index, &keys, None, 2).places.len(), 2);
    }

    #[test]
    fn a_ring_holding_only_the_current_place_falls_back() {
        let index = place_index(Some(&fixture()));
        let rest = at_rest(&index, &[PlaceKey::AllCards], Some(PlaceKey::AllCards), 5);
        assert_eq!(rest.label, "Places");
    }
}
