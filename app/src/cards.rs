//! `/cards/:id` — the public card detail surface, and the lightweight preview
//! affordance the catalog surfaces wrap their rows and tiles in
//! (specs/app-ui.md → "`/cards/:id`").
//!
//! The contract this screen implements:
//!
//! - **Public page, opportunistic auth.** Anyone can read a card. The "your
//!   copies & locations" block is the only authed part, and it is driven by
//!   `CardDetail::ownership` being `Some` — the adapter never 401s, it just
//!   returns the public projection when there is no session.
//! - **The full page SSRs.** The detail `Resource` is keyed on the route param
//!   alone, so a cold load (and a crawler, and a `curl`) gets rendered markup.
//! - **Previews never change the URL.** Hover on desktop, tap-to-sheet on
//!   touch; both are enhancements over a plain `<a>` that still navigates when
//!   JS is absent. The sheet is the only one that offers "Full details →",
//!   because on desktop the trigger itself is already the link.
//! - **Multi-face cards render an image.** The projection fallback lives in the
//!   hosted backend (`COALESCE(image_uris, faces->0->image_uris)`); this module
//!   assumes `image_uri` is populated whenever the printing has any art at all,
//!   and degrades to a skeleton rather than breaking when it isn't.
//! - **The printings table picks up the same preview + "current printing"
//!   contract.** Every row wraps the same `CardPreview` the catalog's list
//!   view uses (hover/tap for that printing's own art), is a real link back
//!   to this same page with `?printing=<id>` set, and the table itself caps
//!   at ~20 rows tall and scrolls inside that cap — see `Printings` and
//!   `resolve_current_printing` for the mechanics.

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use leptos_router::NavigateOptions;
use shared::{
    CardDetail, CardFaceSummary, CardSummary, OwnershipEntry, PrintingSummary, Ruling, WantEntry,
};

use crate::components::holding_stepper::{HaveStepper, WantStepper};
use crate::components::states::{self, RetryButton};
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardHeader, CardTitle};
use crate::components::ui::count_stepper::StepperCommit;
use crate::components::ui::hover_card::{HoverCard, HoverCardContent, HoverCardTrigger};
use crate::components::ui::separator::Separator;
use crate::components::ui::sheet::{Sheet, SheetContent, SheetDirection};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};
use crate::my::tree::CollectionTreeResource;

/// A card's art, with the skeleton behind rather than swapped out on load: no
/// JS, no layout shift, and it is what shows through for a printing whose
/// `image_uri` is genuinely absent.
#[component]
fn CardArt(
    #[prop(into)] name: String,
    image_uri: Option<String>,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let class = if class.is_empty() {
        "relative block w-full".to_string()
    } else {
        format!("relative block {class}")
    };
    view! {
        <div class=class>
            <Skeleton class="aspect-[5/7] w-full rounded-lg" />
            {image_uri
                .map(|src| {
                    view! {
                        <img
                            src=src
                            alt=name
                            loading="lazy"
                            decoding="async"
                            class="absolute inset-0 size-full rounded-lg object-cover"
                        />
                    }
                })}
        </div>
    }
}

/// The face-dependent slice of a card surface — what the flip control swaps.
/// One shape serves the detail page (all fields) and the previews (which have
/// no oracle text or stats to show).
#[derive(Clone, PartialEq)]
struct FacePanel {
    name: String,
    mana_cost: Option<String>,
    type_line: Option<String>,
    oracle_text: Option<String>,
    stats: Option<String>,
    image_uri: Option<String>,
}

/// "2/3", "Loyalty 4", "Defense 5", or nothing — the printed stat corner of a
/// card or face. Top-level card columns carry no `defense` today; faces can
/// (battle fronts), so the panel handles it once here.
fn stats_line(
    power: Option<&str>,
    toughness: Option<&str>,
    loyalty: Option<&str>,
    defense: Option<&str>,
) -> Option<String> {
    match (power, toughness, loyalty, defense) {
        (Some(p), Some(t), _, _) => Some(format!("{p}/{t}")),
        (_, _, Some(l), _) => Some(format!("Loyalty {l}")),
        (_, _, _, Some(d)) => Some(format!("Defense {d}")),
        _ => None,
    }
}

/// The DFC flip control (specs/TODO.md, back-face task): cycles the visible
/// face of a card whose layout has a real back face (`shared::has_back_face`).
/// Overlaid on the card art by an `absolute` class; callers render it only
/// when there are ≥ 2 flip faces, so `n_faces` is never zero here.
#[component]
fn FlipButton(
    face: RwSignal<usize>,
    n_faces: usize,
    /// Compact variant for the small preview art.
    #[prop(optional)]
    small: bool,
) -> impl IntoView {
    let size = if small {
        ButtonSize::IconXs
    } else {
        ButtonSize::IconSm
    };
    view! {
        <Button
            variant=ButtonVariant::Secondary
            size=size
            class="absolute right-1.5 top-1.5 z-10 rounded-full opacity-90 shadow-md"
            {..}
            aria-label="Flip card"
            data-testid="card-flip"
            on:click=move |ev: leptos::ev::MouseEvent| {
                // Never a navigation, and inside a preview the click must not
                // bubble to the trigger span that routes taps to the sheet.
                ev.prevent_default();
                ev.stop_propagation();
                face.update(|f| *f = (*f + 1) % n_faces);
            }
        >
            <svg
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                <path d="M21 3v5h-5" />
                <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                <path d="M8 16H3v5" />
            </svg>
        </Button>
    }
}

/// The shared body of both preview affordances: art, name, mana cost, type
/// line, and an owned badge when the caller has copies. Deliberately renders
/// from an already-loaded [`CardSummary`] — a preview that fetched would defeat
/// the point of being lightweight, and the catalog already holds this data.
///
/// A card with a real back face (`CardSummary::faces` non-empty — the layout
/// decision is the projection's) gets the flip control here too, swapping
/// name / mana / type / art per face. Face state is per-affordance: the hover
/// card and the sheet are separate `PreviewBody` instances, each starting at
/// the front — and each **stays** at the front across a close/reopen, even
/// though the instance itself is mounted once and never torn down (the
/// lazy-mount latch above `CardPreview`'s callers). `open` is what makes that
/// true: it is the affordance's real visible state (the hover card's own
/// open signal, or the sheet's), not the mount latch, and every time it
/// flips back to visible the face resets — matching this doc comment rather
/// than the flip silently surviving in the still-mounted body.
#[component]
fn PreviewBody(card: CardSummary, #[prop(into)] open: Signal<bool>) -> impl IntoView {
    let CardSummary {
        name,
        image_uri,
        mana_cost,
        type_line,
        owned,
        faces,
        ..
    } = card;
    let owned = owned.unwrap_or(0);
    let flippable = faces.len() >= 2;
    let n_faces = faces.len();
    let face = RwSignal::new(0usize);

    // Leading edge of a genuine reopen, not "every render": this only fires
    // when `open` itself changes value, so flipping the face while already
    // open doesn't touch it (nothing here reads `face`), and closing doesn't
    // either (the guard is `if open`). Harmless to run on first mount too —
    // `face` is already 0 then.
    Effect::new(move |_| {
        if open.get() {
            face.set(0);
        }
    });

    let panel: Signal<FacePanel> = if flippable {
        let front_img = image_uri;
        Signal::derive(move || {
            let i = face.get().min(faces.len() - 1);
            let f = &faces[i];
            FacePanel {
                name: f.name.clone(),
                mana_cost: f.mana_cost.clone(),
                type_line: f.type_line.clone(),
                oracle_text: None,
                stats: None,
                // Face 0 falls back to the flattened front image — same
                // printing, same value, but covers a projection that filled
                // only `image_uri`.
                image_uri: f
                    .image_uri
                    .clone()
                    .or_else(|| (i == 0).then(|| front_img.clone()).flatten()),
            }
        })
    } else {
        let base = FacePanel {
            name: name.clone(),
            mana_cost,
            type_line,
            oracle_text: None,
            stats: None,
            image_uri,
        };
        Signal::derive(move || base.clone())
    };

    view! {
        <div class="flex gap-3">
            <div class="relative w-24 shrink-0">
                {move || {
                    let p = panel.get();
                    view! { <CardArt name=p.name image_uri=p.image_uri /> }
                }}
                {flippable.then(|| view! { <FlipButton face n_faces small=true /> })}
            </div>
            <div class="min-w-0 space-y-1">
                <p class="text-sm font-medium">{move || panel.with(|p| p.name.clone())}</p>
                {move || {
                    panel
                        .with(|p| p.mana_cost.clone())
                        .filter(|m| !m.is_empty())
                        .map(|m| view! { <p class="text-muted-foreground text-xs">{m}</p> })
                }}
                {move || {
                    panel
                        .with(|p| p.type_line.clone())
                        .map(|t| view! { <p class="text-muted-foreground text-xs">{t}</p> })
                }}
                {(owned > 0)
                    .then(|| {
                        view! {
                            <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                                {format!("{owned} owned")}
                            </Badge>
                        }
                    })}
            </div>
        </div>
    }
}

/// Wraps a catalog row or tile in the preview affordances.
///
/// Desktop (fine pointer) gets a hover card after the component's 150 ms hover
/// intent. Touch (coarse pointer) gets a bottom sheet instead, and the tap that
/// opens it is prevented from navigating — the sheet's "Full details →" link is
/// how you get to the page from there.
///
/// Both are wired at once and the *pointer type* picks between them, because
/// touch browsers fire a synthetic `mouseenter` on tap: without disabling the
/// hover card on coarse pointers a tap would open both. The pointer type
/// resolves in an Effect (client-only), so SSR renders the desktop arrangement
/// and hydration corrects it — which is safe precisely because neither
/// affordance is load-bearing.
///
/// **Both bodies mount lazily.** Rendering them up-front put every card's name
/// and art into the DOM two extra times per row, which is not just weight: it
/// made `getByText(name).first()` on `/catalog` resolve to a *hidden* copy, so
/// the duplication was visible to assistive tech and tests alike. Each body now
/// mounts on the interaction that will reveal it, and stays mounted after.
#[component]
pub fn CardPreview(
    card: CardSummary,
    /// Whether to offer the desktop hover preview. Off for surfaces that
    /// already show the art — a hover card over a grid tile is a smaller copy
    /// of the image you are already looking at. The touch sheet stays on
    /// regardless, since a tap there still wants an alternative to navigating.
    #[prop(default = true)]
    hover: bool,
    /// Overrides the default `card-preview-{oracle_id}` / `card-sheet-{oracle_id}`
    /// DOM ids. Every call site but one mounts at most one `CardPreview` per
    /// `oracle_id`, so the default (derived from `card.oracle_id`) is unique
    /// on its own. The exception is the card-detail page's printings table
    /// (`Printings`, this module): every row's `CardSummary` shares the
    /// page's one `oracle_id` — only the printing differs — so without an
    /// override every row would render the *same* hover-card/sheet id,
    /// duplicate DOM ids that (among other things) break the hover card's
    /// CSS anchor positioning (`anchor-name` collides). Callers with more
    /// than one printing on screen pass the printing's own id here instead.
    #[prop(into, optional)]
    id: Option<String>,
    /// Overrides the sheet's "Full details →" destination (default
    /// `/cards/{oracle_id}`). `Printings` passes the row's own
    /// `?printing=<id>` link here, so a touch tap's sheet lands on the same
    /// printing the row itself would have — not this page's default hero.
    #[prop(into, optional)]
    detail_href: Option<String>,
    /// Whether a same-page navigation through this preview (a plain click
    /// that falls through past the sheet, or the sheet's own "Full details →"
    /// link) **replaces** the current history entry instead of pushing a new
    /// one. Default `false` — every ordinary call site (the catalog, `/my`'s
    /// collection views, the palette) uses this preview to open a card for
    /// the *first* time, a genuine new back-target that must push. `Printings`
    /// (this module) is the one exception: its rows all point at the page the
    /// reader is already on (`/cards/:id`, only `?printing=` differing), so a
    /// flip between them is not a new visit — "history granularity is per
    /// session", the same rule `components::query_bar` and `catalog::rail`
    /// already apply to `?q=` edits. See `back_nav`'s module doc for why a
    /// replace here is safe: the wrapped `history.replaceState` carries the
    /// *current* entry's `has_history` marker forward untouched, so however
    /// many printings a visit flips through, one `Back` still lands on
    /// whatever sent the reader to this card in the first place.
    #[prop(default = false)]
    replace_history: bool,
    /// Extra classes merged onto *both* wrapper levels the trigger sits
    /// inside — the inner `<span class="block">` this component owns, and
    /// (when `hover` is on) `HoverCardTrigger`'s own `<span>` outside that.
    /// Empty by default (every call site but `Printings` leaves this unset,
    /// so both spans keep their bare `block`/`block w-full` classes exactly
    /// as before). `Printings` passes `h-full` here — see that module's doc
    /// comment ("the hover anchor's geometry has to be the row's") for why a
    /// *chain* of explicit heights, not just one on the innermost `<a>`, is
    /// what a percentage height needs to resolve through two auto-height
    /// wrapper levels.
    #[prop(into, optional)]
    trigger_class: String,
    children: Children,
) -> impl IntoView {
    let oracle_id = card.oracle_id;
    let instance_id = id.unwrap_or_else(|| oracle_id.to_string());
    let href = detail_href.unwrap_or_else(|| format!("/cards/{oracle_id}"));
    let name = card.name.clone();
    let navigate = use_navigate();
    // Each affordance's body lands in its own per-node closure, so they each
    // need an owned copy rather than sharing one.
    let hover_card_body = card.clone();
    let sheet_open = RwSignal::new(false);
    // Mirrors the hover card's own open/closed state (via `on_open_change`),
    // so the hover `PreviewBody` — mounted once and kept mounted, unlike
    // `sheet_open` there is no other live signal for "is this actually
    // showing right now" to reset its flip state on.
    let hover_open = RwSignal::new(false);

    // `web-sys` is only a dependency of the wasm half, and Effects only ever
    // run on the client anyway, so the body is `hydrate`-gated rather than the
    // whole signal — SSR renders with `coarse == false` (the desktop
    // arrangement) and hydration corrects it.
    let coarse = RwSignal::new(false);
    Effect::new(move |_| {
        #[cfg(feature = "hydrate")]
        {
            let is_coarse = window()
                .match_media("(pointer: coarse)")
                .ok()
                .flatten()
                .is_some_and(|m: web_sys::MediaQueryList| m.matches());
            coarse.set(is_coarse);
        }
    });

    // Latched, not live: the point is to mount a body before the affordance
    // that reveals it appears, and unmounting again would empty the sheet
    // mid-slide (it animates out over 300 ms) or thrash the hover card on
    // every mouseleave.
    let hovered = RwSignal::new(false);
    let sheet_seen = RwSignal::new(false);

    // What the *last pointer event* actually was, as opposed to what the device
    // says its primary pointer is. A hybrid laptop reports `(pointer: fine)`
    // while still taking touch taps, so keying only off the media query made a
    // real finger-tap follow the link instead of opening the sheet (Codex
    // review, medium). `pointerdown` always precedes `click`, so this is
    // settled by the time the click handler reads it, and it flips back on the
    // next mouse click — the same device gets both behaviors, correctly.
    let touch_intent = RwSignal::new(false);
    let wants_sheet = Signal::derive(move || coarse.get() || touch_intent.get());

    let on_click = {
        // Own copies for the closure: `navigate` is `Clone` (a thin wrapper
        // over the router's context, see `leptos_router::hooks::use_navigate`),
        // and `href` is still needed below for the sheet's own "Full details"
        // link.
        let navigate = navigate.clone();
        let href = href.clone();
        move |ev: leptos::ev::MouseEvent| {
            // A modified click is a navigation instruction, not a preview request
            // — swallowing it would break "open in a new tab" for anyone with a
            // keyboard attached to a touch device.
            if crate::components::back_nav::is_modified_click(
                ev.meta_key(),
                ev.ctrl_key(),
                ev.shift_key(),
                ev.alt_key(),
            ) {
                return;
            }
            if wants_sheet.get() {
                ev.prevent_default();
                sheet_seen.set(true);
                sheet_open.set(true);
            } else if replace_history {
                // The desktop path: no sheet, and this click would otherwise
                // fall through to the child `<a>`'s own href, which
                // `leptos_router`'s global click delegate turns into a
                // **push** — see the `replace_history` prop doc for why that
                // is wrong for a same-page printing flip. Take over the
                // navigation ourselves instead, with `replace: true`.
                ev.prevent_default();
                navigate(
                    &href,
                    NavigateOptions {
                        replace: true,
                        ..Default::default()
                    },
                );
            }
        }
    };

    // Merged onto *both* wrapper spans below (not just the innermost one) —
    // see `trigger_class`'s own doc comment for why a percentage height
    // needs an explicit height at every level of the chain, not just one.
    let inner_trigger_class = if trigger_class.is_empty() {
        "block".to_string()
    } else {
        format!("block {trigger_class}")
    };
    let outer_trigger_class = if trigger_class.is_empty() {
        "block w-full".to_string()
    } else {
        format!("block w-full {trigger_class}")
    };

    // `span`, not `div`: this sits inside HoverCardTrigger's own `<span>`,
    // and flow content inside phrasing content is invalid HTML.
    let trigger = view! {
        <span
            class=inner_trigger_class
            on:click=on_click
            on:pointerdown=move |ev: leptos::ev::PointerEvent| {
                touch_intent.set(ev.pointer_type() == "touch")
            }
            on:mouseenter=move |_| hovered.set(true)
            on:focusin=move |_| hovered.set(true)
            data-testid="card-preview-trigger"
        >
            {children()}
        </span>
    };

    let trigger = if hover {
        view! {
            // Disabled on the same signal that routes clicks to the sheet, so a
            // hybrid device's touch tap suppresses the hover card too rather
            // than raising one behind the sheet.
            <HoverCard
                id=format!("card-preview-{instance_id}")
                disabled=wants_sheet
                on_open_change=Callback::new(move |v| hover_open.set(v))
            >
                <HoverCardTrigger class=outer_trigger_class>{trigger}</HoverCardTrigger>
                <HoverCardContent class="w-72" {..} data-testid="card-preview-hover">
                    <Show when=move || hovered.get()>
                        <PreviewBody card=hover_card_body.clone() open=hover_open />
                    </Show>
                </HoverCardContent>
            </HoverCard>
        }
        .into_any()
    } else {
        trigger.into_any()
    };

    view! {
        {trigger}
        // `contents`: the Sheet's wrapper div would otherwise be a second flex
        // item inside a grid tile's `<li>`, adding a phantom gap between the
        // art and the caption. Backdrop and panel are both position:fixed, so
        // the wrapper has no layout job to do.
        <Sheet id=format!("card-sheet-{instance_id}") open=sheet_open class="contents">
            <SheetContent
                direction=SheetDirection::Bottom
                aria_label=name
                // Trailing `class` deliberately: the prop immediately before
                // `{..}` must not end in a bare path or the view macro parses
                // it as struct-update syntax (same trap as catalog.rs).
                class="h-auto max-h-[80vh] overflow-y-auto"
                {..}
                data-testid="card-preview-sheet"
            >
                // Keyed on the latch, not on `sheet_open`: gating on the live
                // signal unmounts the body on the same tick the close
                // animation starts, so the sheet slides away as an empty box.
                // `sheet_open` still does useful work below — passed as
                // `PreviewBody`'s `open` prop, it is what resets the flip
                // state on a reopen without needing to unmount anything.
                <Show when=move || sheet_seen.get()>
                    <div class="space-y-4 p-4">
                        <PreviewBody card=card.clone() open=sheet_open />
                        <a
                            href=href.clone()
                            class="text-primary inline-block text-sm font-medium hover:underline"
                            data-testid="card-preview-full-details"
                            on:click={
                                // Fresh clones per invocation, not a move of
                                // the outer `href`/`navigate` — this closure
                                // sits inside `Show`'s `Fn` children, which
                                // must stay callable more than once.
                                let navigate = navigate.clone();
                                let href = href.clone();
                                move |ev: leptos::ev::MouseEvent| {
                                    // A no-op everywhere but `Printings` — see
                                    // the `replace_history` prop doc.
                                    if !replace_history {
                                        return;
                                    }
                                    if crate::components::back_nav::is_modified_click(
                                        ev.meta_key(),
                                        ev.ctrl_key(),
                                        ev.shift_key(),
                                        ev.alt_key(),
                                    ) {
                                        return;
                                    }
                                    ev.prevent_default();
                                    navigate(
                                        &href,
                                        NavigateOptions {
                                            replace: true,
                                            ..Default::default()
                                        },
                                    );
                                }
                            }
                        >
                            "Full details →"
                        </a>
                    </div>
                </Show>
            </SheetContent>
        </Sheet>
    }
}

/// What `CardDetailPage`'s resource carries.
///
/// **A named field, and not an `Option` at the top level — both halves matter.**
///
/// The named field is the [`crate::my::all_cards::AllCardsPayload`] /
/// [`crate::catalog::SearchPayload`] pattern, for the third time: `initial_value()`
/// reads `__RESOLVED_RESOURCES[<next monotonic id>]` for every `Resource::new`
/// without checking `during_hydration()`, so a resource created during a
/// client-side navigation reads a slot left behind by the page you came from, and
/// if it decodes the fetcher never runs.
///
/// This resource was the **worst-exposed of the three**, because its payload used
/// to be `Option<Result<…>>` and **a bare `null` deserializes into every
/// `Option`, whatever the inner type**. It did not need a structurally similar
/// struct to collide with; any `null` slot anywhere would do. `/catalog` leaves
/// four of them: measured at ids 1, 4, 7, 12 anonymous and 4, 7, 12 authed.
///
/// Measured, because the mechanism existing and the mechanism firing are
/// different claims (responsive audit, 2026-07-27). It does **not** fire today:
/// over 60 real click-throughs — anonymous and authed, six origin queries, six
/// tile positions each — every one fetched and rendered the right card. The
/// resource lands on id **64** (anonymous) / **66** (authed), while `/catalog`
/// serializes only **13** / **19** slots, so the slot it reads is `undefined` and
/// the fetcher runs. Pinned by *injection*, the inverse of the removal trick that
/// found the other two: flooding slots 0..199 with `"null"` reproduces the bug
/// exactly (`card_detail` never requested, "Card not found" rendered for a card
/// that exists), and a binary search on the flood ceiling puts the landing id at
/// those two numbers. So the margin is ~50 slots — real, but nothing enforces it,
/// and it is an accident of how many resources `/catalog` happens to build rather
/// than a designed gap.
///
/// The second half — `card` is a plain enum, not an `Option` — closes the hole
/// this wrapper would otherwise leave open. A struct whose only field is an
/// `Option` accepts `{}`, because serde defaults a missing `Option` field to
/// `None`; that would make the wrapper decorative against any future `{}`-shaped
/// payload. Naming the two outcomes also fixes a dishonest state on its own
/// terms: "the URL carried no parseable id" and "nothing arrived" are different
/// facts, and only the first of them justifies telling a visitor their card id
/// isn't valid.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardDetailPayload {
    card: CardIdOutcome,
}

/// The two things `CardDetailPage`'s fetcher can conclude — kept distinct so the
/// render site cannot confuse "your link is wrong" with "the read never
/// happened". See [`CardDetailPayload`].
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
enum CardIdOutcome {
    /// The route carried no parseable card id. The only case that honestly
    /// warrants "That card id isn't valid."
    NoId,
    /// The read ran and this is what it said. Boxed because `CardDetail` is far
    /// larger than `NoId` and clippy's `large_enum_variant` is right that every
    /// value would otherwise pay for the biggest one.
    Fetched(Box<Result<CardDetail, ServerFnError<shared::ApiError>>>),
}

/// `?printing=<id>` on `/cards/:id` — which printing the printings table's
/// row links carry (see [`Printings`] and the "current printing" doc on
/// [`CardDetailBody`]). Read here rather than folded into the route param
/// because every printing under one card shares the same `oracle_id`
/// (Scryfall's model: `oracle_id` is the card, `printings.id` is one
/// specific art/set/number) — there is no other id `/cards/:id` could carry
/// to distinguish them.
const PRINTING_PARAM: &str = "printing";

#[component]
pub fn CardDetailPage() -> impl IntoView {
    let params = use_params_map();
    let query_map = use_query_map();

    // The param is a Memo so that a navigation which doesn't change the id
    // (a query-string change, say) can't re-fire the fetch.
    let oracle_id = Memo::new(move |_| {
        params
            .read()
            .get("id")
            .and_then(|raw| raw.parse::<shared::Id>().ok())
    });

    // Deliberately *not* part of the resource's key: every printing this
    // card has already arrived in one `card_detail` fetch, so re-selecting
    // which one is "current" is a client-side reorder, not a re-fetch (same
    // reasoning `resolve_current_printing`'s doc comment gives).
    let selected_printing = Memo::new(move |_| {
        query_map
            .read()
            .get(PRINTING_PARAM)
            .and_then(|raw| raw.parse::<shared::Id>().ok())
    });

    // Parsing client-side means a malformed id renders "not found" without a
    // pointless round trip, and the server fn keeps a typed argument.
    //
    // The tray's batch move lives in the shell and has no handle on this
    // page's resource, so — same idiom as `all_cards.rs` / `collection.rs` —
    // the revision it bumps is one of the resource's sources: a move refetches
    // "Your copies" structurally instead of leaving it stale until reload.
    // `holdings_revision()` is a constant `0` signal outside the shell (the
    // bench), so this page stays mountable there without an `Option` check.
    let revision = crate::my::move_selection::holdings_revision();
    let detail_res = Resource::new(
        move || (oracle_id.get(), revision.get()),
        |(id, _revision)| async move {
            CardDetailPayload {
                card: match id {
                    Some(id) => CardIdOutcome::Fetched(Box::new(crate::card_detail(id).await)),
                    None => CardIdOutcome::NoId,
                },
            }
        },
    );

    view! {
        <div class="space-y-6 p-6" data-testid="card-detail">
            <BackControl />
            <Transition fallback=|| view! { <CardDetailSkeleton /> }>
                {move || {
                    Suspend::new(async move {
                        match detail_res.await.card {
                            CardIdOutcome::Fetched(res) => match *res {
                                Ok(card) => {
                                    view! { <CardDetailBody card=card selected_printing=selected_printing /> }
                                        .into_any()
                                }
                                Err(e) => match classify(&e) {
                                    Failure::Missing(detail) => {
                                        view! { <NotFound detail=detail /> }.into_any()
                                    }
                                    Failure::Broken(failure, detail) => {
                                        view! {
                                            <LoadFailed
                                                failure
                                                detail=detail
                                                detail_res=detail_res
                                            />
                                        }
                                            .into_any()
                                    }
                                },
                            },
                            // The route's own id, not a read outcome — see
                            // `CardIdOutcome`.
                            CardIdOutcome::NoId => {
                                view! { <NotFound detail="That card id isn't valid." /> }.into_any()
                            }
                        }
                    })
                }}
            </Transition>
        </div>
    }
}

/// The page's own way out (design/information-architecture.md's card-detail
/// section: `/cards/:id` is mode-neutral, reachable from the catalog, My
/// cards, a collection view and the mobile sheet's expand affordance — there
/// is no fixed "parent" screen the way a collection's drill-down breadcrumb
/// has one). Rendered above the `<Transition>`, not inside it, so it never
/// waits on the card fetch — the read-failure states need an escape at least
/// as much as the happy path, and `LoadFailed`'s own doc notes this is the
/// page people share, so the reader who lands here most often has no history
/// to go back through at all.
///
/// A real `<a>`, not a JS-only control: with the fallback destination as its
/// `href`, the Leptos router's own click-delegate turns a click into an SPA
/// navigation automatically, and with JS entirely absent the browser just
/// follows the link — the same "still navigates" contract `CardPreview`'s
/// module doc states for the sheet's "Full details →" link. `on:click` only
/// intercepts the case JS *can* improve on: with real in-app history behind
/// this page, `history.back()` returns the reader to the exact page (and
/// query string) they came from, which no fixed `href` could name. See
/// `components::back_nav` for the shared mechanics behind both this and the
/// desktop `⌘[` / `Alt+←` shortcut.
///
/// Shown at every width, unlike the mobile-only back row the collection view
/// and needs page use (`my/collection.rs`'s `CollectionPath`,
/// `my/needs.rs`'s `NeedsHeader`) — those hide on desktop because desktop
/// shows a breadcrumb instead, and this page has no breadcrumb equivalent to
/// fall back on.
#[component]
fn BackControl() -> impl IntoView {
    let nav = expect_context::<crate::components::back_nav::BackNavigation>();
    view! {
        <a
            href=nav.fallback_href
            class="text-muted-foreground hover:text-foreground flex w-fit items-center gap-1 text-sm"
            data-testid="card-detail-back"
            on:click=move |ev: leptos::ev::MouseEvent| {
                // A modified click is a navigation instruction (open in a new
                // tab/window), not a "take me back" request — same guard
                // `CardPreview::on_click` applies to the preview trigger, via
                // the shared `back_nav::is_modified_click`.
                if crate::components::back_nav::is_modified_click(
                    ev.meta_key(),
                    ev.ctrl_key(),
                    ev.shift_key(),
                    ev.alt_key(),
                ) {
                    return;
                }
                // Read fresh, not from a stored signal — see
                // `back_nav::has_history`'s own doc for why it's a plain
                // function now (round 2: a signal would have papered over
                // exactly the staleness bug that shipped in round 1).
                if crate::components::back_nav::has_history() {
                    ev.prevent_default();
                    crate::components::back_nav::browser_back();
                }
                // Else: let the default anchor click proceed. The router's
                // click-delegate turns it into an SPA navigation to the
                // fallback href; with JS absent the browser just follows it.
            }
        >
            <span aria-hidden="true">"‹"</span>
            "Back"
        </a>
    }
}

enum Failure {
    /// The catalog answered, and this card genuinely isn't in it.
    Missing(String),
    /// Something else went wrong — a different thing from "no such card", and
    /// telling a visitor their card doesn't exist because Neon is unreachable
    /// is a lie they'd act on. Carries the **shared** classification alongside
    /// the detail, so this page's affordances follow the same rule every
    /// `ErrorNote` surface follows instead of a second, looser one.
    Broken(states::Failure, String),
}

/// Split a read failure into the two things this page says differently.
///
/// The prefix table itself is shared ([`crate::components::states::describe`]) —
/// there is one wire format and it should be parsed in one place — but the copy
/// is not: a card that genuinely isn't in the catalog wants naming as such,
/// where every other surface's `Missing` means "your link is dead". Anything not
/// `not found:` is treated as breakage, which is the safe direction: a missing
/// card misreported as an outage is recoverable, the reverse is not.
fn classify(e: &ServerFnError<shared::ApiError>) -> Failure {
    match crate::components::states::describe(e) {
        (states::Failure::Missing, _) => {
            Failure::Missing("We don't have that card in the catalog.".into())
        }
        (failure, detail) => Failure::Broken(failure, detail),
    }
}

/// The read broke. Unlike [`NotFound`] — which has offered a way out since it
/// shipped — this arm had a page with no link and no control on it at all, which
/// on a **public** URL is the worst place for one: `/cards/:id` is the surface
/// people share, so the reader who lands here most often arrived from outside the
/// app and has no history to go back through.
///
/// The way out is unconditional and the **retry is not**, which is the same rule
/// [`ErrorNote`](crate::components::states::ErrorNote) applies — this page keeps
/// its own shape (a heading, not a banner) but must not keep its own policy. An
/// unconditional retry here re-sent a `validation:`/`conflict:`/`forbidden:`
/// request verbatim, forever, which is precisely what the shared classifier
/// exists to stop. `data-failure` is carried for the same reason the five banner
/// surfaces carry it: without the seam nothing can assert which arm this is.
#[component]
fn LoadFailed(
    failure: states::Failure,
    #[prop(into)] detail: String,
    detail_res: Resource<CardDetailPayload>,
) -> impl IntoView {
    // Only "on our side" when it actually is. A refused request is not an outage,
    // and inviting the reader to wait a moment for one they cannot fix by waiting
    // is the small lie this arm used to tell every non-`upstream:` failure.
    let sentence = if failure.retryable() {
        "Something went wrong on our side — try again in a moment."
    } else {
        "That request can't be answered as it stands."
    };
    view! {
        <div class="space-y-2" data-testid="card-detail-error" data-failure=failure.slug()>
            <h1 class="text-2xl font-bold">"We couldn't load this card"</h1>
            <p role="alert" class="text-muted-foreground text-sm">
                {sentence}
            </p>
            <p class="text-muted-foreground text-xs">{detail}</p>
            <div class="flex flex-wrap items-center gap-3 pt-1">
                {failure
                    .retryable()
                    .then(|| {
                        view! {
                            <RetryButton on_retry=Callback::new(move |()| detail_res.refetch()) />
                        }
                    })}
                <a href="/catalog" class="text-primary text-sm font-medium hover:underline">
                    "Back to the catalog"
                </a>
            </div>
        </div>
    }
}

#[component]
fn NotFound(#[prop(into)] detail: String) -> impl IntoView {
    view! {
        <div class="space-y-2" data-testid="card-detail-missing">
            <h1 class="text-2xl font-bold">"Card not found"</h1>
            <p class="text-muted-foreground text-sm">{detail}</p>
            <a href="/catalog" class="text-primary text-sm font-medium hover:underline">
                "Back to the catalog"
            </a>
        </div>
    }
}

#[component]
fn CardDetailSkeleton() -> impl IntoView {
    view! {
        <div class="grid gap-6 md:grid-cols-[18rem_1fr]" aria-busy="true" aria-label="Loading card">
            <Skeleton class="aspect-[5/7] w-full rounded-lg" />
            <div class="space-y-3">
                <Skeleton class="h-8 w-2/3" />
                <Skeleton class="h-4 w-1/3" />
                <Skeleton class="h-24 w-full" />
            </div>
        </div>
    }
}

/// Picks the printing that represents the card right now — the one the hero
/// panel's art comes from and the one [`reorder_current_first`] puts at the
/// top of the printings table.
///
/// `selected` (from `?printing=<id>`, [`PRINTING_PARAM`]) wins when it names
/// a printing this card actually has; a stale or foreign id (a bookmarked
/// link to a printing since removed, say) falls through rather than 404ing
/// the whole page. Absent a valid selection, this is the pre-existing
/// default: the oldest printing with art — searched, not `first()`, because
/// Scryfall carries artless rows (placeholders, some non-English printings),
/// and letting one of those sit first would blank the hero while a later
/// printing has art. The second `find` is the degenerate case of a printing
/// whose face 0 has no art but a later face does (`image_uri` coalesces face
/// 0 only).
fn resolve_current_printing(
    printings: &[PrintingSummary],
    selected: Option<shared::Id>,
) -> Option<PrintingSummary> {
    selected
        .and_then(|id| printings.iter().find(|p| p.id == id))
        .or_else(|| printings.iter().find(|p| p.image_uri.is_some()))
        .or_else(|| {
            printings
                .iter()
                .find(|p| p.face_image_uris.iter().any(|f| f.is_some()))
        })
        .cloned()
}

/// The printings table's row order: whichever printing [`resolve_current_printing`]
/// names moves to index 0, everything else keeps its original (release-date)
/// order behind it. Pure and separable from the hero-art derivation above so
/// both can share one "which printing is current" answer without agreeing on
/// it twice.
fn reorder_current_first(
    printings: &[PrintingSummary],
    selected: Option<shared::Id>,
) -> Vec<PrintingSummary> {
    let mut printings = printings.to_vec();
    if let Some(id) = resolve_current_printing(&printings, selected).map(|p| p.id) {
        if let Some(pos) = printings.iter().position(|p| p.id == id) {
            let p = printings.remove(pos);
            printings.insert(0, p);
        }
    }
    printings
}

#[component]
fn CardDetailBody(
    card: CardDetail,
    /// `?printing=<id>` — see [`PRINTING_PARAM`]. A `Signal`, not a plain
    /// `Option`, because clicking a printings-table row only changes the
    /// query string (same `oracle_id`, same page): the hero art and table
    /// order below must re-derive when it changes without this component
    /// remounting or the card refetching — every printing already arrived in
    /// the one `card_detail` response `card` carries.
    #[prop(into)]
    selected_printing: Signal<Option<shared::Id>>,
) -> impl IntoView {
    // Parsed before the destructure moves the fields; empty for every layout
    // without a real back face (shared::CardDetail::flip_faces).
    let flip_faces = card.flip_faces();
    let CardDetail {
        oracle_id,
        name,
        mana_cost,
        type_line,
        oracle_text,
        power,
        toughness,
        loyalty,
        keywords,
        layout,
        card_faces,
        printings,
        rulings,
        ownership,
        wants,
        ..
    } = card;

    // Reserved for the printings table below — the panel closures a few
    // lines down move `name`/`mana_cost`/`type_line` into themselves, and
    // every printing row's hover preview wants its own copy of the same
    // card-level fields (see `Printings`'s doc comment).
    let printings_name = name.clone();
    let printings_mana_cost = mana_cost.clone();
    let printings_type_line = type_line.clone();

    // Per-printing flip data for the table's hover previews, built once here
    // rather than typed as a `Printings` prop: `card_faces` is a raw
    // `serde_json::Value` (inferred, never named in this crate — `app`'s own
    // `serde_json` dependency is `ssr`-only, so naming it in a type position
    // that also compiles under `hydrate` would break the wasm build).
    // `CardFaceSummary::build` is the same pure zip the hosted backend runs
    // server-side for a catalog row's flip data (`shared::CardFaceSummary::
    // build`); run here since every printing already arrived in this page's
    // one `card_detail` fetch.
    let printing_faces: std::collections::HashMap<shared::Id, Vec<CardFaceSummary>> = printings
        .iter()
        .map(|p| {
            (
                p.id,
                CardFaceSummary::build(layout.as_deref(), card_faces.as_ref(), &p.face_image_uris),
            )
        })
        .collect();

    let printings_for_hero = printings.clone();
    let current_printing =
        Memo::new(move |_| resolve_current_printing(&printings_for_hero, selected_printing.get()));

    let flippable = flip_faces.len() >= 2;
    let n_faces = flip_faces.len();
    let face = RwSignal::new(0usize);
    // The combined "Front // Back" name stays on the page as a subtitle when
    // the heading swaps per face — it is the card's canonical identity (and
    // what search matched on).
    let combined_name = flippable.then(|| name.clone());

    let panel: Signal<FacePanel> = if flippable {
        Signal::derive(move || {
            let i = face.get().min(flip_faces.len() - 1);
            let f = &flip_faces[i];
            let current = current_printing.get();
            let front_img = current.as_ref().and_then(|p| p.image_uri.clone());
            let face_images = current.map(|p| p.face_image_uris).unwrap_or_default();
            FacePanel {
                name: f.name.clone(),
                mana_cost: f.mana_cost.clone(),
                type_line: f.type_line.clone(),
                oracle_text: f.oracle_text.clone(),
                stats: stats_line(
                    f.power.as_deref(),
                    f.toughness.as_deref(),
                    f.loyalty.as_deref(),
                    f.defense.as_deref(),
                ),
                image_uri: face_images
                    .get(i)
                    .cloned()
                    .flatten()
                    .or_else(|| (i == 0).then(|| front_img.clone()).flatten()),
            }
        })
    } else {
        // A dedicated clone, not the outer `name`: the closure below captures
        // by move, and `name` itself is still needed after this `if/else`
        // (YourCopies/YourWants, and `Printings`'s own copy above) — moving
        // it into only one arm of the branch still poisons every later use,
        // since the borrow checker can't assume which arm ran.
        let panel_name = name.clone();
        Signal::derive(move || FacePanel {
            name: panel_name.clone(),
            mana_cost: mana_cost.clone(),
            type_line: type_line.clone(),
            oracle_text: oracle_text.clone(),
            stats: stats_line(
                power.as_deref(),
                toughness.as_deref(),
                loyalty.as_deref(),
                None,
            ),
            image_uri: current_printing.get().and_then(|p| p.image_uri),
        })
    };

    view! {
        <div class="grid gap-6 md:grid-cols-[18rem_1fr]">
            <div class="relative md:w-72">
                {move || {
                    let p = panel.get();
                    view! { <CardArt name=p.name image_uri=p.image_uri /> }
                }}
                {flippable.then(|| view! { <FlipButton face n_faces /> })}
            </div>

            <div class="min-w-0 space-y-6">
                <div class="space-y-2">
                    <h1 class="text-2xl font-bold" data-testid="card-name">
                        {move || panel.with(|p| p.name.clone())}
                    </h1>
                    {combined_name
                        .map(|n| {
                            view! {
                                <p
                                    class="text-muted-foreground text-xs"
                                    data-testid="card-combined-name"
                                >
                                    {n}
                                </p>
                            }
                        })}
                    <p class="text-muted-foreground text-sm">
                        {move || {
                            panel
                                .with(|p| {
                                    format!(
                                        "{}{}",
                                        p.type_line.clone().unwrap_or_default(),
                                        p.mana_cost
                                            .clone()
                                            .filter(|m| !m.is_empty())
                                            .map(|m| format!(" · {m}"))
                                            .unwrap_or_default(),
                                    )
                                })
                        }}
                    </p>
                    {move || {
                        panel
                            .with(|p| p.stats.clone())
                            .map(|s| view! { <p class="text-sm font-medium">{s}</p> })
                    }}
                    {move || {
                        // `keywords` is card-level, not per-face: Scryfall's
                        // top-level `keywords` array is already the union of
                        // both faces' ability words, and there is no per-face
                        // equivalent to swap in instead — the raw `card_faces`
                        // jsonb this page's flip control reads never carried
                        // one (`ORACLE_FACE_KEYS`, app/src/ingest/extract.rs,
                        // deliberately excludes `keywords`; Scryfall's own
                        // Card Face schema has no such key to begin with). So
                        // the honest-minimal fix is to stop pairing the
                        // unioned row with a flipped-to back face — showing it
                        // made a back face display front-face keywords beside
                        // back-face oracle text — rather than fabricate
                        // per-face data the wire has never carried.
                        let is_front = !flippable || face.get() == 0;
                        (is_front && !keywords.is_empty())
                            .then(|| {
                                view! {
                                    <div class="flex flex-wrap gap-1" data-testid="card-keywords">
                                        {keywords
                                            .clone()
                                            .into_iter()
                                            .map(|k| {
                                                view! {
                                                    <Badge variant=BadgeVariant::Outline size=BadgeSize::Sm>
                                                        {k}
                                                    </Badge>
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                }
                            })
                    }}
                </div>

                {move || {
                    panel
                        .with(|p| p.oracle_text.clone())
                        .filter(|t| !t.is_empty())
                        .map(|t| {
                            view! {
                                <p
                                    class="text-sm leading-relaxed whitespace-pre-line"
                                    data-testid="card-oracle-text"
                                >
                                    {t}
                                </p>
                            }
                        })
                }}

                <Separator />

                {ownership.map(|o| view! { <YourCopies entries=o card_name=name.clone() /> })}
                {wants.map(|w| view! { <YourWants entries=w card_name=name.clone() /> })}
                <Printings
                    oracle_id
                    name=printings_name
                    mana_cost=printings_mana_cost
                    type_line=printings_type_line
                    printing_faces=printing_faces
                    printings=printings
                    selected_printing=selected_printing
                />
                <Rulings rulings=rulings />
            </div>
        </div>
    }
}

/// Rendered only when the caller is signed in — `ownership` is `None` for
/// anonymous readers, which is a different thing from "signed in with no
/// copies" (an empty list, which still shows the section and says so).
///
/// Each row carries [`HaveStepper`] — the same write semantics
/// `/my/collections/:id`'s HERE cell uses (maintainer ruling P6-054: an
/// optimistic set, a committed zero routed through `remove_holding` so it
/// stays undoable, and a refusal on a cell that sums more than one grain),
/// lifted to `crate::components::holding_stepper` for exactly this reuse. The
/// header total tracks a local delta rather than refetching the page's own
/// resource: doing that here would remount every row mid-toast, the same trap
/// `crate::my::collection`'s module doc warns `HereCount` away from.
#[component]
fn YourCopies(entries: Vec<OwnershipEntry>, card_name: String) -> impl IntoView {
    let base_total: i32 = entries.iter().map(|e| e.quantity).sum();
    let total_delta = RwSignal::new(0);
    // Shell-provided (AppShell wraps this public route too), same as
    // `HereCount`'s own `tree` — refetched after a settled write so the
    // sidebar's badges stay in step with the count this block just changed.
    let tree = expect_context::<CollectionTreeResource>().0;
    let on_settled = Callback::new(move |()| tree.refetch());

    view! {
        <Card {..} data-testid="your-copies">
            <CardHeader>
                <CardTitle class="text-base">
                    {move || format!("Your copies · {}", base_total + total_delta.get())}
                </CardTitle>
            </CardHeader>
            <CardContent>
                {if entries.is_empty() {
                    view! {
                        <p class="text-muted-foreground text-sm">
                            "You don't have this card yet."
                        </p>
                    }
                        .into_any()
                } else {
                    view! {
                        <ul class="space-y-1 text-sm">
                            {entries
                                .into_iter()
                                .map(|e| {
                                    let href = format!("/my/collections/{}", e.collection_id);
                                    let on_change = Callback::new(move |c: StepperCommit| {
                                        total_delta.update(|d| *d += c.to - c.from);
                                    });
                                    view! {
                                        <li
                                            class="flex items-center justify-between gap-4"
                                            data-testid="ownership-row"
                                            data-collection-id=e.collection_id.to_string()
                                        >
                                            <a href=href class="truncate hover:underline">
                                                {e.collection_name}
                                            </a>
                                            <HaveStepper
                                                name=card_name.clone()
                                                present=e.quantity
                                                holding_id=e.holding_id
                                                on_change
                                                on_settled
                                            />
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    }
                        .into_any()
                }}
            </CardContent>
        </Card>
    }
}

/// The wants counterpart of [`YourCopies`] — desired copies of this card, by
/// collection. Same authed-only gating (`wants` is `None` for anonymous
/// readers), same always-shown-once-authed shape (an empty list still shows
/// the section and says so), and each row carries [`WantStepper`] — the
/// wants write semantics (no ledger, so a committed zero is a direct,
/// non-undoable delete; see that component's doc).
#[component]
fn YourWants(entries: Vec<WantEntry>, card_name: String) -> impl IntoView {
    let base_total: i32 = entries.iter().map(|e| e.quantity).sum();
    let total_delta = RwSignal::new(0);
    let tree = expect_context::<CollectionTreeResource>().0;
    let on_settled = Callback::new(move |()| tree.refetch());

    view! {
        <Card {..} data-testid="your-wants">
            <CardHeader>
                <CardTitle class="text-base">
                    {move || format!("Your wants · {}", base_total + total_delta.get())}
                </CardTitle>
            </CardHeader>
            <CardContent>
                {if entries.is_empty() {
                    view! {
                        <p class="text-muted-foreground text-sm">
                            "You don't want this card anywhere yet."
                        </p>
                    }
                        .into_any()
                } else {
                    view! {
                        <ul class="space-y-1 text-sm">
                            {entries
                                .into_iter()
                                .map(|e| {
                                    let href = format!("/my/collections/{}", e.collection_id);
                                    let on_change = Callback::new(move |c: StepperCommit| {
                                        total_delta.update(|d| *d += c.to - c.from);
                                    });
                                    view! {
                                        <li
                                            class="flex items-center justify-between gap-4"
                                            data-testid="want-row"
                                            data-collection-id=e.collection_id.to_string()
                                        >
                                            <a href=href class="truncate hover:underline">
                                                {e.collection_name}
                                            </a>
                                            <WantStepper
                                                name=card_name.clone()
                                                desired=e.quantity
                                                desire_id=e.desire_id
                                                on_change
                                                on_settled
                                            />
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    }
                        .into_any()
                }}
            </CardContent>
        </Card>
    }
}

/// The printings table's scroll cap — derived from `Table`'s own row
/// geometry rather than a hand-picked pixel value. `TableHeader`'s `th` is
/// `h-10` (2.5rem); a body row is a `p-4`-padded (1rem top + bottom) cell of
/// `text-sm` content (1.25rem line-height), so one row stands 1rem + 1rem +
/// 1.25rem = 3.25rem tall. Twenty of those plus the header:
/// `2.5rem + 20 * 3.25rem` = `67.5rem`. `TableWrapper`'s own default
/// (`max-h-96`, ~7 rows) is too short for "approx 20 items", so this
/// overrides it — same mechanism the old `max-h-none` override used, just
/// capped instead of uncapped. The header stays pinned over the scrolling
/// body for free: `TableHeader` is already `sticky top-0` (table.rs).
const PRINTINGS_MAX_HEIGHT: &str = "max-h-[67.5rem]";

/// The printings table: one row per art/set/number this card was printed
/// as, capped to roughly twenty rows tall and scrollable inside that cap
/// ([`PRINTINGS_MAX_HEIGHT`]). Each row is a real link to this same card's
/// `/cards/:id` page carrying `?printing=<id>` ([`PRINTING_PARAM`]), which
/// [`CardDetailBody`] reads back to pick that printing's art and float it to
/// the top of this very list — see `resolve_current_printing`'s doc comment
/// for why a query param and not a distinct route.
///
/// Hovering a row reuses [`CardPreview`] — the same hover-card/touch-sheet
/// affordance the catalog's list view wraps its rows in — built from a
/// [`CardSummary`] assembled per row: `name`/`mana_cost`/`type_line` are
/// card-level (identical for every printing, passed down from
/// `CardDetailBody`), `image_uri` is this printing's own art, and `faces`
/// comes from the caller's `printing_faces` (built with
/// `CardFaceSummary::build`, the same pure zip the hosted backend uses for a
/// catalog row's flip data — see `CardDetailBody`'s doc comment on that field
/// for why it's precomputed there instead of built per row here). `owned` is
/// always `None` here, deliberately: it drives a per-oracle "N owned" badge
/// elsewhere, and this page already has its own per-collection ownership
/// breakdown (`YourCopies`) — a *per-printing* owned count isn't a thing
/// this projection carries, so the row preview shows none rather than
/// mislabeling the oracle-level figure as this printing's own.
///
/// **Not a stretched overlay link.** #158 originally made the row
/// whole-width clickable via the standard "stretched link" trick —
/// `TableRow` gets `position: relative` and one cell's anchor is
/// `absolute inset-0`, so the anchor's containing block is meant to be the
/// row. That trick relies on a UA actually honoring `position` on `<tr>`,
/// and CSS 2.1 leaves that UA-defined: Blink/Chromium do, WebKit does not.
/// In WebKit the anchor's containing block fell through past the
/// non-positioned `<tr>` to the nearest positioned ancestor up the tree —
/// the page container — so every row's "row-sized" overlay actually
/// rendered page-sized, and N stacked full-page invisible links ate every
/// mouse click on the whole card-detail screen (Back, steppers, hero —
/// whichever overlay painted topmost). Keyboard/a11y (`AXPress` bypasses
/// hit-testing entirely) and chromium e2e both worked, which is how it
/// shipped and how it slipped past review. Live-probed in the real WKWebView
/// `.app` (see `end2end/wkwebview/`), fixed here. Do not replace this with
/// another UA-defined table-row trick (`transform`/`clip-path`/`filter` on
/// `<tr>` are equally unreliable in WebKit) — the fix below avoids
/// positioning `<tr>` (or sizing anything against it) at all.
///
/// **The fix: one real link, plus a click affordance on every other cell.**
/// No `position` anywhere in this row. Cell 1 ("Set") wraps its visible
/// text directly in a normal, in-flow, block-level `<a>` — sized by its own
/// content the same way any link is, in every engine, so there is no
/// containing-block question to get wrong. That is the row's one real,
/// keyboard-focusable, screen-reader-announced link (`aria-label` carries
/// the full row description — see `link_label` below).
///
/// **a11y: plain readable cells (round 2).** Cells 2–4 do *not* get their
/// own `<a>`. An earlier version of this fix gave them duplicate anchors
/// (`aria-hidden` + `tabindex="-1"`, to avoid extra Tab stops) — caught in
/// adversarial review as trading one accessibility bug for another:
/// `aria-hidden` removes an element's entire subtree from the accessibility
/// tree, so a screen reader's cell-by-cell table navigation (e.g. VoiceOver
/// Ctrl+Option+arrows) heard *empty* cells where the collector number,
/// rarity, and finishes used to read as plain text — cell 1's `aria-label`
/// is a description of the whole row, not a substitute for reading each
/// cell's own data. Fixed by putting the click affordance on the `<td>`
/// itself instead: `on:click=replace_nav_click(...)` plus `cursor-pointer`
/// for the visual affordance. A `<td>` with a click handler is not
/// interactive markup — it hides nothing from the accessibility tree, adds
/// no Tab stop, and announces nothing beyond its own plain text content.
/// This is what keeps the row whole-width clickable without hiding data:
/// every cell covers its own box with *some* click handling — a real
/// `<a>` in cell 1, a `<td>` listener in cells 2–4 — so a click anywhere in
/// the row's rendered area does something, in every engine, with zero
/// positioning tricks. **Accepted consequence:** a modified click
/// (cmd/ctrl/shift/alt, "open in a new tab") only works via cell 1's real
/// `<a href>` — cells 2–4 have no `href` for the browser to open, so their
/// click handler is mouse/touch-only convenience, not a link (see
/// `replace_nav_click`'s own doc comment). (Also considered and rejected:
/// restructuring the row into a non-table `<a>`-wrapping grid — a bigger
/// blast radius for header alignment and table a11y semantics for no
/// behavioral gain over this.)
///
/// `select-none` (on cell 1's anchor and cells 2–4's `<td>`s alike)
/// preserves the tradeoff the old overlay had as a side effect of paint
/// order (an absolutely-positioned box always paints over its
/// non-positioned siblings, which meant a mouse drag across the row
/// selected nothing): with real in-flow text and click targets now doing
/// the work, a drag would otherwise select it, so non-selectability is now
/// explicit CSS instead of an accident of `z`-order.
///
/// **The hover anchor's geometry has to be the row's, not zero — and not
/// just at the innermost level.** `CardPreview`'s own trigger is a plain
/// `<span>` that auto-sizes to its *in-flow* children — the old overlay
/// anchor was `position: absolute` and contributed nothing to that, so a
/// `CardPreview` given only the overlay as children rendered a 0×0px
/// trigger box, and `HoverCardContent`'s `position-area: block-end` then
/// computed "below" from that zero-height box — i.e. from the row's *top*
/// edge, opening the panel over the hovered row and the next several
/// (round-2 adversarial review, caught by a bounding-box assertion no
/// earlier test had). Fixed the same way every other `CardPreview` call
/// site already works: real visible content *inside* the trigger — cell
/// 1's `<a>` *is* the trigger's content now, not a separate
/// overlay-plus-sibling-span.
///
/// That alone is not sufficient, though (round 2, second pass): the `<a>`
/// sits *two* auto-height `<span>` hops below the `<td>` — `CardPreview`'s
/// own trigger span, and (when hovering is on) `HoverCardTrigger`'s span
/// outside that — and a percentage height only resolves against a parent
/// whose `height` *property* is explicitly non-`auto`. **The `<td>`'s
/// rendered box is the row's real height (table layout stretches every
/// cell to the tallest one), but its `height` *property* is still `auto`**
/// — nothing here ever sets it — so that used, on-screen pixel size does
/// not count as "explicit" for a child's percentage-height resolution
/// (CSS 2.1 §10.5: a percentage height computes to `auto` unless the
/// containing block's height was itself specified, not merely
/// content-derived). Verified empirically, not assumed: a standalone
/// WKWebView probe reproducing this exact markup (`h-full` threaded
/// through both spans, nothing else) measured the trigger stuck at its own
/// short content height while the row rendered taller from a wrapped
/// sibling cell — the percentage chain never started. The classic table
/// fix is `h-[1px]` on the `<td>` itself: an explicit (if nonsensical)
/// `height: 1px` that the table layout algorithm overrides right back to
/// the row's real height for rendering — cells never shrink below their
/// content's needs — but which *registers* as "explicit" for the cascade,
/// unlocking every descendant's `h-full` below it. (Bracket syntax, not the
/// bare `h-px` keyword utility — this Tailwind version's spacing scale
/// carries no `px` token, so `h-px` compiles to a dead class with no rule
/// behind it; caught by checking the compiled `app.css`, not by assuming
/// the class name did what it says.) With `h-[1px]` added, the same probe
/// measured the trigger matching the row within a pixel (the row's own
/// `border-b` on the `<tr>`, not part of any cell's content box). Chain,
/// corrected: `<td>` (`h-[1px]`, explicit) → trigger span (`h-full`,
/// resolves against it) → `HoverCardTrigger` span (`h-full`, resolves) →
/// `<a>` (`h-full`, finally resolves) — and the hover panel positions off
/// the row's true bottom edge regardless of which cell's content is
/// tallest.
///
/// **Hover-preview scope, deliberately narrowed.** The old overlay spanned
/// the whole row, so hovering *anywhere* opened the preview. The new
/// structure only wires `CardPreview`'s hover/touch-sheet affordance to
/// cell 1 (cells 2–4 are plain click-to-navigate, no preview) — matching
/// the catalog list view's own `CardPreview` scope (name cell only,
/// `catalog.rs::ResultsList`), and not one of this fix's "must keep"
/// requirements (whole-row *clickability*, not whole-row *hover*). See
/// `end2end/tests/card-detail.spec.ts`'s "hovering a row" test, updated to
/// hover the row's link rather than the row's own (now not-necessarily-over-
/// cell-1) bounding-box center.
#[component]
fn Printings(
    oracle_id: shared::Id,
    #[prop(into)] name: String,
    mana_cost: Option<String>,
    type_line: Option<String>,
    /// Every printing's own flip-preview faces, keyed by `PrintingSummary::id`
    /// — precomputed by the caller (see `CardDetailBody`'s doc comment on
    /// this field for why: the raw `card_faces` jsonb can't be named as a
    /// prop type here without breaking the `hydrate`/wasm build).
    printing_faces: std::collections::HashMap<shared::Id, Vec<CardFaceSummary>>,
    printings: Vec<PrintingSummary>,
    #[prop(into)] selected_printing: Signal<Option<shared::Id>>,
) -> impl IntoView {
    let count = printings.len();
    let ordered = Memo::new(move |_| reorder_current_first(&printings, selected_printing.get()));
    // Shared by cells 2–4's duplicate links (`replace_nav_click`) — cell 1's
    // own click-interception lives inside `CardPreview` and already has its
    // own copy.
    let navigate = use_navigate();

    view! {
        <section class="space-y-2">
            <h2 class="text-lg font-semibold">{format!("Printings · {count}")}</h2>
            <TableWrapper class=PRINTINGS_MAX_HEIGHT>
                <Table {..} data-testid="card-printings">
                    <TableHeader>
                        <TableRow>
                            <TableHead {..} scope="col">
                                "Set"
                            </TableHead>
                            <TableHead class="hidden sm:table-cell" {..} scope="col">
                                "Number"
                            </TableHead>
                            <TableHead {..} scope="col">
                                "Rarity"
                            </TableHead>
                            <TableHead class="hidden sm:table-cell" {..} scope="col">
                                "Finishes"
                            </TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {move || {
                            ordered
                                .get()
                                .into_iter()
                                .map(|p| {
                                    let PrintingSummary {
                                        id,
                                        set_code,
                                        set_name,
                                        collector_number,
                                        rarity,
                                        image_uri,
                                        finishes,
                                        face_image_uris: _,
                                    } = p;
                                    let set = match (set_name, set_code) {
                                        (Some(n), Some(c)) => format!("{n} ({})", c.to_uppercase()),
                                        (Some(n), None) => n,
                                        (None, Some(c)) => c.to_uppercase(),
                                        (None, None) => "Unknown set".to_string(),
                                    };
                                    let href = format!("/cards/{oracle_id}?{PRINTING_PARAM}={id}");
                                    // Everything a sighted reader sees in the row, not just the
                                    // set — the label speaks for the row's whole clickable area,
                                    // and the other three columns are real distinguishing info
                                    // (adversarial review, round 2).
                                    let link_label = format!(
                                        "{name} — {set}, #{collector_number}, {rarity}, {}",
                                        finishes.join(", "),
                                    );
                                    // One closure per duplicate link (cells 2–4) — built here,
                                    // as plain owned locals, rather than inline in the `view!`
                                    // attribute position: the reactive `{move || …}` this row
                                    // lives inside must stay callable on every re-run
                                    // (`ordered` is a `Memo`), and building `replace_nav_click`'s
                                    // result inline at three attribute sites confused the
                                    // closure's captured-by-reference/moved analysis into
                                    // treating `navigate` as consumed after the first row.
                                    let number_click = replace_nav_click(navigate.clone(), href.clone());
                                    let rarity_click = replace_nav_click(navigate.clone(), href.clone());
                                    let finishes_click = replace_nav_click(navigate.clone(), href.clone());
                                    // A separately-named clone per `href=` attribute below, for
                                    // the same reason as the `_click` locals above: the `view!`
                                    // macro lowers each component/element's props into its own
                                    // closure, and each one fully *moves* whatever `href` it
                                    // captures — sharing one `href.clone()` expression across
                                    // multiple sibling elements' attributes (rather than one
                                    // uniquely-named local per site) hit the same "moved" error.
                                    // Only two now (cells 2–4 no longer have their own `<a>`,
                                    // hence no `href` of their own — see the module doc's "a11y:
                                    // plain readable cells" note).
                                    let preview_href = href.clone();
                                    let set_href = href;
                                    let preview = CardSummary {
                                        oracle_id,
                                        name: name.clone(),
                                        printing_id: Some(id),
                                        image_uri: image_uri.clone(),
                                        mana_cost: mana_cost.clone(),
                                        type_line: type_line.clone(),
                                        owned: None,
                                        faces: printing_faces.get(&id).cloned().unwrap_or_default(),
                                    };
                                    view! {
                                        <TableRow
                                            {..}
                                            data-testid="printing-row"
                                            data-printing-id=id.to_string()
                                        >
                                            // `h-[1px]`: an explicit (table-layout-overridden)
                                            // height, not a real 1px cell — what makes the
                                            // `trigger_class="h-full"` chain below able to resolve
                                            // at all. See the module doc's "hover anchor's
                                            // geometry" note for why a *rendered* full-row height
                                            // isn't enough on its own. Bracket syntax deliberately,
                                            // not the bare `h-px` keyword utility: this Tailwind
                                            // version's default spacing scale doesn't carry a `px`
                                            // token, so `h-px` renders as a dead class with no CSS
                                            // rule behind it — caught by checking the compiled
                                            // `app.css` for the rule, not by assuming the class
                                            // name compiled to something.
                                            <TableCell class="h-[1px] p-0 font-medium">
                                                <CardPreview
                                                    card=preview
                                                    id=id.to_string()
                                                    detail_href=preview_href
                                                    // Every row here is another printing of the
                                                    // page the reader is already on — a flip, not
                                                    // a new visit — so a same-page navigation
                                                    // through this preview replaces the current
                                                    // history entry instead of pushing a fresh
                                                    // one for every printing viewed. See the prop
                                                    // doc for the full "history granularity is
                                                    // per session" reasoning.
                                                    replace_history=true
                                                    // Threads an explicit height through both of
                                                    // `CardPreview`'s own wrapper spans, so the
                                                    // `h-full` on the `<a>` below actually
                                                    // resolves — see `trigger_class`'s doc comment
                                                    // and the module doc's "the hover anchor's
                                                    // geometry" note for why a percentage height
                                                    // needs an explicit height at *every* level of
                                                    // the chain (td → span → span → a), not just
                                                    // the innermost one.
                                                    trigger_class="h-full"
                                                >
                                                    // The row's one real, keyboard-focusable,
                                                    // screen-reader-announced link — see the
                                                    // module doc's "not a stretched overlay link"
                                                    // note. Real in-flow content (not an
                                                    // absolutely-positioned sibling), which is
                                                    // also what fixes the hover-trigger geometry:
                                                    // this *is* the trigger's content now.
                                                    <a
                                                        href=set_href
                                                        class="flex h-full select-none items-center p-4"
                                                        aria-label=link_label
                                                        data-testid="printing-row-link"
                                                    >
                                                        {set}
                                                    </a>
                                                </CardPreview>
                                            </TableCell>
                                            // Cells 2–4: no `<a>`, no `aria-hidden` — the click
                                            // affordance lives on the `<td>` itself
                                            // (`cursor-pointer` + `on:click`), so the cell's own
                                            // text stays plain, unhidden accessible content that
                                            // a screen reader's cell-by-cell table navigation
                                            // reads normally. See the module doc's "a11y: plain
                                            // readable cells" note.
                                            <TableCell
                                                class="text-muted-foreground hidden cursor-pointer select-none sm:table-cell"
                                                on:click=number_click
                                            >
                                                {collector_number}
                                            </TableCell>
                                            <TableCell
                                                class="cursor-pointer select-none capitalize"
                                                on:click=rarity_click
                                            >
                                                {rarity}
                                            </TableCell>
                                            <TableCell
                                                class="text-muted-foreground hidden cursor-pointer select-none capitalize sm:table-cell"
                                                on:click=finishes_click
                                            >
                                                {finishes.join(", ")}
                                            </TableCell>
                                        </TableRow>
                                    }
                                })
                                .collect_view()
                        }}
                    </TableBody>
                </Table>
            </TableWrapper>
        </section>
    }
}

/// The printings row's cells-2-through-4 click handler (`Printings`, above —
/// see its "a11y: plain readable cells" doc note for why these cells are
/// plain `<td>`s with `on:click`, not anchors). Unlike `CardPreview::
/// on_click` there is no real `href` underneath a `<td>` for a modified
/// click to fall through to. The guard stays anyway: a cmd/ctrl/shift/
/// alt-click asking to "open in a new tab" can't be honored on a `<td>`
/// with no link, so the honest response is to do *nothing* rather than
/// either fake a new-tab open or silently downgrade the reader's explicit
/// modifier into an in-place navigation they didn't ask for.
fn replace_nav_click(
    navigate: impl Fn(&str, NavigateOptions) + 'static,
    href: String,
) -> impl Fn(leptos::ev::MouseEvent) + 'static {
    move |ev: leptos::ev::MouseEvent| {
        if crate::components::back_nav::is_modified_click(
            ev.meta_key(),
            ev.ctrl_key(),
            ev.shift_key(),
            ev.alt_key(),
        ) {
            return;
        }
        ev.prevent_default();
        navigate(
            &href,
            NavigateOptions {
                replace: true,
                ..Default::default()
            },
        );
    }
}

#[component]
fn Rulings(rulings: Vec<Ruling>) -> impl IntoView {
    if rulings.is_empty() {
        return ().into_any();
    }
    view! {
        <section class="space-y-2">
            <h2 class="text-lg font-semibold">{format!("Rulings · {}", rulings.len())}</h2>
            <ul class="space-y-3" data-testid="card-rulings">
                {rulings
                    .into_iter()
                    .map(|r| {
                        view! {
                            <li class="text-sm">
                                <p class="leading-relaxed">{r.comment}</p>
                                <p class="text-muted-foreground mt-0.5 text-xs">
                                    {r.published_at.unwrap_or_default()}
                                </p>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        </section>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use shared::PrintingSummary;
    use uuid::Uuid;

    use super::{reorder_current_first, resolve_current_printing};

    fn printing(n: u128, image: bool) -> PrintingSummary {
        PrintingSummary {
            id: Uuid::from_u128(n),
            set_code: Some(format!("s{n}")),
            set_name: Some(format!("Set {n}")),
            collector_number: n.to_string(),
            rarity: "common".into(),
            image_uri: image.then(|| format!("https://example.test/{n}.jpg")),
            finishes: vec!["nonfoil".into()],
            face_image_uris: vec![],
        }
    }

    #[test]
    fn resolve_current_printing_prefers_a_valid_selection() {
        let printings = vec![printing(1, true), printing(2, true)];
        let current = resolve_current_printing(&printings, Some(Uuid::from_u128(2)));
        assert_eq!(current.map(|p| p.id), Some(Uuid::from_u128(2)));
    }

    #[test]
    fn resolve_current_printing_falls_back_on_a_stale_selection() {
        // A selection naming a printing this card doesn't have (a bookmarked
        // link to one since removed, say) must not blank the page — it falls
        // through to the ordinary default instead.
        let printings = vec![printing(1, false), printing(2, true)];
        let current = resolve_current_printing(&printings, Some(Uuid::from_u128(99)));
        assert_eq!(current.map(|p| p.id), Some(Uuid::from_u128(2)));
    }

    #[test]
    fn resolve_current_printing_defaults_to_the_first_printing_with_art() {
        // Oldest-first order is assumed (the query orders by release date);
        // the artless first printing must not blank the hero.
        let printings = vec![printing(1, false), printing(2, true), printing(3, true)];
        let current = resolve_current_printing(&printings, None);
        assert_eq!(current.map(|p| p.id), Some(Uuid::from_u128(2)));
    }

    #[test]
    fn resolve_current_printing_none_when_nothing_has_art() {
        let printings = vec![printing(1, false), printing(2, false)];
        assert!(resolve_current_printing(&printings, None).is_none());
    }

    #[test]
    fn reorder_current_first_moves_the_selection_to_the_front() {
        let printings = vec![printing(1, true), printing(2, true), printing(3, true)];
        let ordered = reorder_current_first(&printings, Some(Uuid::from_u128(3)));
        let ids: Vec<_> = ordered.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            vec![Uuid::from_u128(3), Uuid::from_u128(1), Uuid::from_u128(2)]
        );
    }

    #[test]
    fn reorder_current_first_leaves_the_default_hero_already_first() {
        // No explicit selection: the default hero (first with art) should
        // land at the top even when it isn't already the list's first entry.
        let printings = vec![printing(1, false), printing(2, true), printing(3, true)];
        let ordered = reorder_current_first(&printings, None);
        assert_eq!(ordered[0].id, Uuid::from_u128(2));
        // The rest keep their original relative order behind it.
        let rest: Vec<_> = ordered[1..].iter().map(|p| p.id).collect();
        assert_eq!(rest, vec![Uuid::from_u128(1), Uuid::from_u128(3)]);
    }

    #[test]
    fn reorder_current_first_is_a_no_op_on_empty_printings() {
        assert!(reorder_current_first(&[], Some(Uuid::from_u128(1))).is_empty());
    }
}
