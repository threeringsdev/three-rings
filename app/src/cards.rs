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

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use shared::{CardDetail, CardSummary, OwnershipEntry, PrintingSummary, Ruling};

use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::card::{Card, CardContent, CardHeader, CardTitle};
use crate::components::ui::hover_card::{HoverCard, HoverCardContent, HoverCardTrigger};
use crate::components::ui::separator::Separator;
use crate::components::ui::sheet::{Sheet, SheetContent, SheetDirection};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

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

/// The shared body of both preview affordances: art, name, mana cost, type
/// line, and an owned badge when the caller has copies. Deliberately renders
/// from an already-loaded [`CardSummary`] — a preview that fetched would defeat
/// the point of being lightweight, and the catalog already holds this data.
#[component]
fn PreviewBody(card: CardSummary) -> impl IntoView {
    let CardSummary {
        name,
        image_uri,
        mana_cost,
        type_line,
        owned,
        ..
    } = card;
    let owned = owned.unwrap_or(0);

    view! {
        <div class="flex gap-3">
            <CardArt name=name.clone() image_uri=image_uri class="w-24 shrink-0" />
            <div class="min-w-0 space-y-1">
                <p class="text-sm font-medium">{name}</p>
                {mana_cost
                    .filter(|m| !m.is_empty())
                    .map(|m| view! { <p class="text-muted-foreground text-xs">{m}</p> })}
                {type_line
                    .map(|t| view! { <p class="text-muted-foreground text-xs">{t}</p> })}
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
    children: Children,
) -> impl IntoView {
    let oracle_id = card.oracle_id;
    let href = format!("/cards/{oracle_id}");
    let name = card.name.clone();
    // Each affordance's body lands in its own per-node closure, so they each
    // need an owned copy rather than sharing one.
    let hover_card_body = card.clone();
    let sheet_open = RwSignal::new(false);

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

    let on_click = move |ev: leptos::ev::MouseEvent| {
        if coarse.get() {
            ev.prevent_default();
            sheet_open.set(true);
        }
    };

    // Latched, not a live hover state: the point is to mount the body before
    // the hover card's 150 ms intent delay elapses, and unmounting on every
    // mouseleave would thrash it for no gain.
    let hovered = RwSignal::new(false);

    let trigger = view! {
        <div
            on:click=on_click
            on:mouseenter=move |_| hovered.set(true)
            on:focusin=move |_| hovered.set(true)
            data-testid="card-preview-trigger"
        >
            {children()}
        </div>
    };

    let trigger = if hover {
        view! {
            <HoverCard id=format!("card-preview-{oracle_id}") disabled=coarse>
                <HoverCardTrigger class="block w-full">{trigger}</HoverCardTrigger>
                <HoverCardContent class="w-72" {..} data-testid="card-preview-hover">
                    <Show when=move || hovered.get()>
                        <PreviewBody card=hover_card_body.clone() />
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
        <Sheet id=format!("card-sheet-{oracle_id}") open=sheet_open>
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
                <Show when=move || sheet_open.get()>
                    <div class="space-y-4 p-4">
                        <PreviewBody card=card.clone() />
                        <a
                            href=href.clone()
                            class="text-primary inline-block text-sm font-medium hover:underline"
                            data-testid="card-preview-full-details"
                        >
                            "Full details →"
                        </a>
                    </div>
                </Show>
            </SheetContent>
        </Sheet>
    }
}

#[component]
pub fn CardDetailPage() -> impl IntoView {
    let params = use_params_map();

    // The param is a Memo so that a navigation which doesn't change the id
    // (a query-string change, say) can't re-fire the fetch.
    let oracle_id = Memo::new(move |_| {
        params
            .read()
            .get("id")
            .and_then(|raw| raw.parse::<shared::Id>().ok())
    });

    // Parsing client-side means a malformed id renders "not found" without a
    // pointless round trip, and the server fn keeps a typed argument.
    let detail = Resource::new(
        move || oracle_id.get(),
        |id| async move {
            match id {
                Some(id) => Some(crate::card_detail(id).await),
                None => None,
            }
        },
    );

    view! {
        <div class="space-y-6 p-6" data-testid="card-detail">
            <Transition fallback=|| view! { <CardDetailSkeleton /> }>
                {move || {
                    Suspend::new(async move {
                        match detail.await {
                            Some(Ok(card)) => view! { <CardDetailBody card=card /> }.into_any(),
                            Some(Err(e)) => {
                                view! { <NotFound detail=message_of(&e) /> }.into_any()
                            }
                            None => {
                                view! { <NotFound detail="That card id isn't valid." /> }.into_any()
                            }
                        }
                    })
                }}
            </Transition>
        </div>
    }
}

/// The transport only carries `ApiError`'s `Display` string, so this is all the
/// structure there is to recover (the status-semantics gap is queued in TODO).
fn message_of(e: &ServerFnError<String>) -> String {
    match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
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

#[component]
fn CardDetailBody(card: CardDetail) -> impl IntoView {
    let CardDetail {
        name,
        mana_cost,
        type_line,
        oracle_text,
        power,
        toughness,
        loyalty,
        keywords,
        printings,
        rulings,
        ownership,
        ..
    } = card;

    // The first printing is the oldest (the query orders by release date), and
    // it is the one whose art represents the card.
    let hero = printings.first().and_then(|p| p.image_uri.clone());
    let stats = match (power, toughness, loyalty) {
        (Some(p), Some(t), _) => Some(format!("{p}/{t}")),
        (_, _, Some(l)) => Some(format!("Loyalty {l}")),
        _ => None,
    };

    view! {
        <div class="grid gap-6 md:grid-cols-[18rem_1fr]">
            <CardArt name=name.clone() image_uri=hero class="md:w-72" />

            <div class="min-w-0 space-y-6">
                <div class="space-y-2">
                    <h1 class="text-2xl font-bold" data-testid="card-name">
                        {name.clone()}
                    </h1>
                    <p class="text-muted-foreground text-sm">
                        {type_line.unwrap_or_default()}
                        {mana_cost
                            .filter(|m| !m.is_empty())
                            .map(|m| format!(" · {m}"))
                            .unwrap_or_default()}
                    </p>
                    {stats.map(|s| view! { <p class="text-sm font-medium">{s}</p> })}
                    {(!keywords.is_empty())
                        .then(|| {
                            view! {
                                <div class="flex flex-wrap gap-1">
                                    {keywords
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
                        })}
                </div>

                {oracle_text
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
                    })}

                <Separator />

                {ownership.map(|o| view! { <YourCopies entries=o /> })}
                <Printings printings=printings />
                <Rulings rulings=rulings />
            </div>
        </div>
    }
}

/// Rendered only when the caller is signed in — `ownership` is `None` for
/// anonymous readers, which is a different thing from "signed in with no
/// copies" (an empty list, which still shows the section and says so).
#[component]
fn YourCopies(entries: Vec<OwnershipEntry>) -> impl IntoView {
    let total: i32 = entries.iter().map(|e| e.quantity).sum();

    view! {
        <Card {..} data-testid="your-copies">
            <CardHeader>
                <CardTitle class="text-base">
                    {format!("Your copies · {total}")}
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
                                    view! {
                                        <li class="flex items-center justify-between gap-4">
                                            <a href=href class="truncate hover:underline">
                                                {e.collection_name}
                                            </a>
                                            <span class="text-muted-foreground tabular-nums">
                                                {e.quantity}
                                            </span>
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

#[component]
fn Printings(printings: Vec<PrintingSummary>) -> impl IntoView {
    view! {
        <section class="space-y-2">
            <h2 class="text-lg font-semibold">{format!("Printings · {}", printings.len())}</h2>
            <TableWrapper class="max-h-none">
                <Table {..} data-testid="card-printings">
                    <TableHeader>
                        <TableRow>
                            <TableHead>"Set"</TableHead>
                            <TableHead class="hidden sm:table-cell">"Number"</TableHead>
                            <TableHead>"Rarity"</TableHead>
                            <TableHead class="hidden sm:table-cell">"Finishes"</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {printings
                            .into_iter()
                            .map(|p| {
                                let PrintingSummary {
                                    set_code,
                                    set_name,
                                    collector_number,
                                    rarity,
                                    finishes,
                                    ..
                                } = p;
                                let set = match (set_name, set_code) {
                                    (Some(n), Some(c)) => format!("{n} ({})", c.to_uppercase()),
                                    (Some(n), None) => n,
                                    (None, Some(c)) => c.to_uppercase(),
                                    (None, None) => "Unknown set".to_string(),
                                };
                                view! {
                                    <TableRow>
                                        <TableCell class="font-medium">{set}</TableCell>
                                        <TableCell class="text-muted-foreground hidden sm:table-cell">
                                            {collector_number}
                                        </TableCell>
                                        <TableCell class="capitalize">{rarity}</TableCell>
                                        <TableCell class="text-muted-foreground hidden capitalize sm:table-cell">
                                            {finishes.join(", ")}
                                        </TableCell>
                                    </TableRow>
                                }
                            })
                            .collect_view()}
                    </TableBody>
                </Table>
            </TableWrapper>
        </section>
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
