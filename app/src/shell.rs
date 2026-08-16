//! App shell + routing (specs/app-ui.md "App shell"): the top bar with the
//! `Catalog | My cards` mode switch, the sidebar rail frame, mobile bottom
//! tabs, the `/` auth redirect, and the `/my/*` auth guard. Every page body
//! that once lived here as a route skeleton has graduated to its own module.

use leptos::prelude::*;
use leptos_router::components::{Outlet, Redirect};
use leptos_router::hooks::use_location;

use crate::account::{fetch_current_user, CurrentUser, SignOut};
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::popover::{Popover, PopoverAlign, PopoverContent, PopoverTrigger};
use crate::components::ui::selection_tray::{provide_selection, SelectionState, SelectionTray};
use crate::components::ui::separator::Separator;
use crate::components::ui::sonner::Toaster;
use crate::components::ui::theme_toggle::ThemeToggle;

/// The signed-in user, fetched once and shared by the shell, the `/` redirect,
/// the `/my/*` guard, and the user menu — one `fetch_current_user` per load,
/// never one per consumer.
#[derive(Clone, Copy)]
pub struct CurrentUserResource(pub Resource<Result<Option<CurrentUser>, ServerFnError<String>>>);

pub fn provide_current_user() {
    provide_context(CurrentUserResource(Resource::new(
        || (),
        |_| fetch_current_user(),
    )));
}

/// Entry point for the wasm hydrate build (called from the `frontend` crate).
///
/// Before hydrating, recover from proxy-swallowed redirects: the Tauri
/// Android webview fetches documents through an in-process proxy that
/// follows server-side 302s internally, so this document can be the redirect
/// target's HTML while `location` still shows the original URL. Hydrating
/// then panics — the router renders the URL's route against the target's
/// DOM. `shell()` stamps the actually-rendered path on `<html
/// data-ssr-path>`; on a pathname mismatch, hard-replace to the stamped
/// path (one clean extra load) instead of hydrating. Real browsers follow
/// the 302 themselves, so the stamp always matches and this is a no-op.
#[cfg(feature = "hydrate")]
pub fn hydrate_entry() {
    if let Some(w) = web_sys::window() {
        let doc_el = w.document().and_then(|d| d.document_element());
        let loc_path = w.location().pathname().unwrap_or_default();
        if let Some(stamp) = doc_el.and_then(|el| el.get_attribute("data-ssr-path")) {
            let stamp_path = stamp.split('?').next().unwrap_or("");
            if stamp.starts_with('/') && stamp_path != loc_path {
                let _ = w.location().replace(&stamp);
                return;
            }
        }
    }
    leptos::mount::hydrate_body(crate::App);
}

/// Navigate the browser itself (full page load). Client-only: effects never
/// run during SSR, so the non-hydrate arm is just a cfg stub.
fn hard_navigate(path: &str) {
    #[cfg(feature = "hydrate")]
    {
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href(path);
        }
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = path;
    }
}

/// Percent-encode a same-origin path for use as a query value (`?next=…`).
/// `/` stays literal — it's legal in a query and keeps the URL readable.
fn encode_path_for_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `/` — mode dispatch, not a page: authed → `/my`, anonymous → `/catalog`.
/// Server renders answer with a real 302 (the route is `SsrMode::Async` so the
/// status can still be set); client-side navigations use `<Redirect/>`.
#[component]
pub fn RootRedirect() -> impl IntoView {
    let user = expect_context::<CurrentUserResource>().0;
    view! {
        // NB: never a unit fallback — `|| ()` desyncs hydration app-wide
        // (specs/auth.md Findings, 2026-07-13).
        <Suspense fallback=|| {
            view! { <p class="text-muted-foreground p-8 text-sm">"Redirecting…"</p> }
        }>
            {move || Suspend::new(async move {
                let target = match user.await {
                    Ok(Some(_)) => "/my",
                    _ => "/catalog",
                };
                #[cfg(feature = "ssr")]
                leptos_axum::redirect(target);
                view! { <Redirect path=target /> }
            })}
        </Suspense>
    }
}

/// `/my/*` auth guard (specs/app-ui.md Conventions): anonymous callers bounce
/// to `/login?next=<current>`; `/login` honors `next` after sign-in. This is
/// UX only — every server fn underneath re-checks auth itself.
///
/// **This `<Suspense>` is the nearest `SuspenseContext` for every page under
/// `/my/*`,** and a `Resource` read that is not inside a boundary of its own
/// registers on *it* (P6-068). When such a resource re-runs, this boundary goes
/// pending, and a `SuspenseBoundary<TRANSITION = false>` swaps to its fallback —
/// `EitherKeepAlive` unmounts the whole `<Outlet/>` subtree and re-inserts the
/// same nodes when the fetch lands, which blurs the focused field and silently
/// hides any showing native popover. So: a page under here must read its
/// resources inside its own `Suspense`/`Transition`, and anything a consumer
/// outside one needs must come through a plain signal written from inside it.
/// An `Effect` is **not** a boundary — the registration ignores effect scope.
/// (`/catalog` reads resources in render scope safely only because `AppShell`
/// provides no `SuspenseContext` above its `<Outlet/>`; adding one there would
/// hand that route the same defect.)
#[component]
pub fn RequireAuth() -> impl IntoView {
    let user = expect_context::<CurrentUserResource>().0;
    let location = use_location();
    view! {
        <Suspense fallback=|| {
            view! { <p class="text-muted-foreground p-8 text-sm">"Loading…"</p> }
        }>
            {move || {
                // Untracked: the guard re-evaluates on session changes (the
                // resource is tracked), never on URL changes — a tracked read
                // re-runs this closure mid-redirect and compounds
                // `next=/login?next=…` while the old route unmounts.
                let pathname = location.pathname.get_untracked();
                let search = location.search.get_untracked();
                Suspend::new(async move {
                    match user.await {
                        Ok(Some(_)) => view! { <Outlet /> }.into_any(),
                        _ => {
                            let mut current = pathname;
                            if !search.is_empty() {
                                current.push('?');
                                current.push_str(&search);
                            }
                            let target = format!(
                                "/login?next={}",
                                encode_path_for_query(&current),
                            );
                            #[cfg(feature = "ssr")]
                            leptos_axum::redirect(&target);
                            view! { <Redirect path=target /> }.into_any()
                        }
                    }
                })
            }}
        </Suspense>
    }
}

/// The app's brand line — the shell header's wordmark, factored out so
/// `auth_pages.rs` (which renders outside `AppShell`, per the design frame's
/// "Auth Logo" element) can show the same mark instead of inventing its own.
#[component]
pub fn Wordmark() -> impl IntoView {
    view! {
        <a href="/" class="text-sm font-semibold tracking-tight">
            "Three Rings"
        </a>
    }
}

/// The persistent chrome around every catalog/my-cards page: top bar (brand,
/// desktop mode switch, theme toggle, user menu), desktop sidebar rail frame,
/// mobile bottom tabs. Auth pages and the bench stay outside it.
#[component]
pub fn AppShell() -> impl IntoView {
    let location = use_location();
    let my_mode = Memo::new(move |_| {
        let p = location.pathname.get();
        p == "/my" || p.starts_with("/my/")
    });
    // Shell-level, not page-level: the wireframe's "persists across searches"
    // needs the choice to outlive the picker widget, and every add surface —
    // catalog today, my-cards later — reads the same one.
    crate::catalog::destination::provide_destination_state();
    // Also shell-level: the desktop rail's tree and the mobile tab badge read
    // one fetch, and quick-add refetches it after a successful add/undo.
    crate::my::tree::provide_collection_tree();
    // Shell-level so the tray's selection survives a Catalog ⇄ My-cards mode
    // switch and `/my` ⇄ collection navigation — a page-owned signal is disposed
    // by both. A third reason used to be listed and is now retired:
    // `/my/collections/:id` detaching its whole DOM subtree after a `?q=`
    // navigation. That was never the router re-rendering the route; it was that
    // page reading its own `Resource` in its **setup body**, where the nearest
    // `SuspenseContext` is `RequireAuth`'s `<Suspense>` above — so every
    // re-search re-suspended the auth guard and `EitherKeepAlive` unmounted the
    // `<Outlet/>` subtree for the length of the fetch. Fixed in P6-068 (the
    // reads go through plain `RwSignal`s now); the first two reasons are why
    // this still lives here.
    // Shell-level because the two surfaces that write the same `?q=` sit on
    // opposite sides of this component: the query bar is inside the `<Outlet/>`,
    // the filter rail is inside `SidebarRail` beside it. Context only flows
    // *down*, so the slot they share has to be provided above both (P6-086).
    crate::components::query_bar::provide_pending_query();
    let selection = provide_selection();
    // Bumped by the tray's batch move (and its undo); every page whose table
    // renders holdings takes it as a resource *source*, so a move refetches
    // what it invalidated instead of leaving stale counts on screen.
    crate::my::move_selection::provide_holdings_revision();
    // The tray's other proactive prune (P6-122, staleness policy on
    // `SelectionKey`): every time the sidebar's collection tree resolves —
    // the initial load and every refetch a create/rename/move/delete makes —
    // drop any `Held` entry whose collection is no longer among the live
    // ones. Free: the tree is fetched for the sidebar regardless, so this
    // reads data already in hand rather than issuing a read of its own. The
    // outer `Option` is "pending or anonymous", the inner is the fetch
    // result; either not-yet-resolved case is a no-op rather than a prune —
    // a still-pending tree must never read as "every collection is gone".
    let tree = expect_context::<crate::my::tree::CollectionTreeResource>().0;
    Effect::new(move |_| {
        if let Some(Some(Ok(fresh))) = tree.get() {
            let live: std::collections::HashSet<shared::Id> =
                fresh.collections.iter().map(|row| row.summary.id).collect();
            selection.prune_missing_collections(&live);
        }
    });
    // Shell-level so ⌘K's `New binder…` / `New deck…` can open the tree's own
    // create dialog from anywhere — including Catalog mode, where the sidebar
    // tree (which used to provide this) isn't mounted at all. `TreeDialogs`
    // still renders beside the tree, so the flag is set here and the dialog
    // comes up when My-cards mode mounts it.
    crate::my::tree_manage::provide_tree_manage();
    // Also shell-level, and for the palette specifically: the page that made a
    // move is long gone by the time `Undo last move` runs.
    crate::components::palette::provide_last_move();
    // `/cards/:id`'s Back control and the app-wide `⌘[`/`Alt+←` shortcut share
    // one mechanism (see `back_nav`'s module doc): a history-entry-stamping
    // scheme installed once here, and the shortcut has to work from anywhere.
    let back_nav = crate::components::back_nav::provide_back_navigation();
    crate::components::back_nav::install_back_shortcut(back_nav);

    // The rail's drawer state below `md` (see `SidebarRail`). Shell-level
    // because the toggle lives in the top bar and the panel is in the body.
    let rail_open = RwSignal::new(false);
    // A navigation is a dismissal: tapping a tree row in the drawer is a
    // "go there", and leaving the drawer over the page you just opened would
    // make every tap need a second one to see the result.
    Effect::new(move |_| {
        location.pathname.track();
        rail_open.set(false);
    });

    view! {
        <div class="bg-background text-foreground flex min-h-screen flex-col">
            <header class="bg-background sticky top-0 z-40 flex h-14 shrink-0 items-center gap-4 border-b px-4">
                // My-cards mode only, and deliberately: Catalog mode already
                // has its own designed mobile story for the rail — the
                // `FilterSheet` button in the results toolbar (wireframes →
                // "Mobile — Catalog filter sheet") — and a second way in would
                // be two controls opening two copies of the same filters. The
                // tree has no such sheet, which is the gap this fills.
                //
                // **Its justification is now narrower than when it was added.**
                // `/my` below `md` is the wireframe's drill-down root list
                // (`crate::my::root`), so *navigating* the tree no longer needs
                // this drawer. What still does is everything the list has no
                // affordance for: the tree's create / rename / **move** /
                // delete menu (which hangs off a row's `⋯`), and jumping
                // straight to a nested collection without walking down to it.
                // Removing the drawer would take those off touch entirely —
                // the exact defect the tree-move task fixed — so it stays until
                // a wireframe specifies a touch path to tree management. Filed
                // as a follow-up rather than invented here.
                <Show when=move || my_mode.get()>
                    // `size-11` is the 44 px touch target, not the look — the
                    // glyph stays 18 px, exactly as the collection header's `⋯`
                    // does. This button is the *only* way into tree management
                    // on a phone (a real long-press raises no `contextmenu` on
                    // the Android webview), so a 28×26 hit area was the smallest
                    // target on the most consequential control; measured
                    // 27.8×26 before, 44×44 after.
                    <button
                        type="button"
                        class="text-muted-foreground hover:text-foreground -ml-2 inline-flex size-11 shrink-0 items-center justify-center rounded-md text-lg leading-none md:hidden"
                        aria-label="Collections"
                        aria-controls="sidebar-rail"
                        aria-expanded=move || rail_open.get().to_string()
                        data-testid="rail-toggle"
                        on:click=move |_| rail_open.update(|o| *o = !*o)
                    >
                        <span aria-hidden="true">"☰"</span>
                    </button>
                </Show>
                <Wordmark />
                <ModeSwitch my_mode />
                <div class="ml-auto flex items-center gap-2">
                    <ThemeToggle />
                    <UserMenu />
                </div>
            </header>
            <div class="flex flex-1">
                <SidebarRail my_mode rail_open />
                // Mobile: pad past the fixed bottom tab bar — and past the
                // tray too when it is up, since a fixed element cannot push
                // the page it docks over (the pager is the bottom-most thing
                // on both selectable views).
                <main class=move || {
                    if selection.is_empty() {
                        "min-w-0 flex-1 pb-16 md:pb-0"
                    } else {
                        "min-w-0 flex-1 pb-32 md:pb-16"
                    }
                }>
                    <Outlet />
                </main>
            </div>
            <BottomTabs my_mode />
            // Mounted once, at the root: a toast outlives the row that raised
            // it (an undo toast must survive the search that scrolls its card
            // away), so it cannot live inside the page it was raised from.
            //
            // Before the tray, deliberately: the tray's move action reads this
            // `ToastHandle` out of context, and a context is only there for
            // things built after the `provide_context` that put it there.
            // Position is `fixed` on both, so the swap is invisible.
            //
            // The offset is [`toaster_offset`]'s decision — see its docs for why
            // it belongs to the shell rather than to the toaster.
            <Toaster class=Signal::derive(move || {
                toaster_offset(!selection.is_empty()).to_string()
            }) />
            <SelectionTrayDock selection />
            // The tree's four management dialogs, mounted at the shell rather
            // than beside the tree. Two reasons, and the second is a bug fix:
            // ⌘K's `New binder…` opens the create dialog from Catalog mode,
            // where the sidebar isn't rendered at all; and the rail is
            // off-screen below `md`, so a dialog mounted inside it could not be
            // shown on a phone — which made *every* tree action (create,
            // rename, move, delete) silently do nothing there.
            <crate::my::tree_manage::TreeDialogs />
            // The ⌘K palette (design/command-palette.md). Global by nature —
            // the chord works from every page in both modes — so it is mounted
            // here and gates itself on desktop-plus-signed-in. After the
            // `Toaster`, because its `Undo last move` reads that handle.
            <crate::components::palette::CommandPalette />
        </div>
    }
}

/// The wireframe's "Tray Wrap" frame: the fixed dock the selection tray sits
/// in. Separate from the tray itself so the component stays position-agnostic
/// (and the bench can render it inline).
///
/// How far off the viewport floor the toaster has to sit, given whether the
/// selection tray is up.
///
/// **This is the shell's decision, not the toaster's.** The tab bar and the tray
/// dock are both `fixed` chrome this shell owns, and a `fixed` element cannot be
/// pushed by a sibling — so the only code that knows what is already parked at
/// the bottom of the viewport is right here. The same reasoning already shapes
/// `<main>`'s padding.
///
/// The numbers are measured, not guessed (responsive audit, 2026-07-26, at the
/// two frame widths):
///
/// | | bottom chrome | toaster bottom |
/// |---|---|---|
/// | 390, no tray | tab bar occupies the bottom 59 px | 80 px |
/// | 390, tray up | tab bar + pill, pill top at 125.5 px | 136 px |
/// | 1440, no tray | nothing | 24 px (the base) |
/// | 1440, tray up | pill top at 61.5 px | 72 px |
///
/// Each clears the tallest thing under it by ~10 px. Before this, a visible
/// toast painted over the tray's clear `×` at 1440 (the `×` sat wholly inside
/// the toast's box) and over the bottom tab bar at 390. The e2e asserts the
/// *relationship* — toaster bottom above tray top — rather than these constants,
/// so a taller pill fails the test instead of silently re-colliding.
fn toaster_offset(tray_up: bool) -> &'static str {
    if tray_up {
        "bottom-[8.5rem] md:bottom-[4.5rem]"
    } else {
        // Only the tab bar to clear, and only below `md`.
        "bottom-[5rem] md:bottom-6"
    }
}

/// `bottom-16` on mobile is the bottom tab bar's height — the wireframe puts
/// the tray *above* the tabs, not over them; `md:bottom-0` docks it to the
/// viewport floor on desktop, where there are no tabs. `pointer-events-none`
/// on the wrapper matters: with an empty selection the tray renders nothing at
/// all, and a full-width invisible strip would still swallow clicks.
///
/// **`md:left-60` is the sidebar rail's width, and it is what makes the pill
/// centre on the table it describes rather than on the window.** The dock is
/// `fixed`, so `inset-x-0` measures the *viewport*; with a 240 px rail to the
/// left of the content column, `mx-auto` then centred the pill 120 px left of
/// the rows it is talking about (measured 720 against a content-column centre of
/// 840 at 1440). Below `md` the rail is an overlay drawer and the content column
/// *is* the viewport, so `inset-x-0` is already right there — which is why this
/// is a `md:` override and not a change to the base.
#[component]
fn SelectionTrayDock(selection: SelectionState) -> impl IntoView {
    view! {
        <div
            class="pointer-events-none fixed inset-x-0 bottom-16 z-50 px-2.5 pb-2.5 md:bottom-0 md:left-60"
            data-testid="selection-tray-dock"
        >
            <div class="pointer-events-auto mx-auto max-w-3xl">
                // The pill's primary action is a slot (see `SelectionTray`), so
                // the component itself stays free of server calls: this is
                // where the batch move is actually wired in.
                <SelectionTray
                    selection
                    action=ViewFn::from(move || {
                        view! { <crate::my::move_selection::MoveSelection selection /> }
                    })
                />
            </div>
        </div>
    }
}

/// Desktop segmented `Catalog | My cards` switch. Active mode derives from the
/// path (`/my*` = My cards; every other shell page is Catalog mode, including
/// `/cards/:id`), so plain prefix-matched links can't be used directly.
#[component]
fn ModeSwitch(my_mode: Memo<bool>) -> impl IntoView {
    const LINK: &str = "rounded-md px-3 py-1 text-sm transition-colors";
    const ACTIVE: &str = "bg-background text-foreground shadow-sm";
    const INACTIVE: &str = "text-muted-foreground hover:text-foreground";

    view! {
        <nav aria-label="Mode" class="bg-muted hidden items-center gap-1 rounded-lg p-1 md:flex">
            <a
                href="/catalog"
                class=move || {
                    format!("{LINK} {}", if my_mode.get() { INACTIVE } else { ACTIVE })
                }
                aria-current=move || (!my_mode.get()).then_some("page")
            >
                "Catalog"
            </a>
            <a
                href="/my"
                class=move || {
                    format!("{LINK} {}", if my_mode.get() { ACTIVE } else { INACTIVE })
                }
                aria-current=move || my_mode.get().then_some("page")
            >
                "My cards"
            </a>
        </nav>
    }
}

/// Sidebar rail — mode-filled (specs/app-ui.md): Catalog mode gets the filter
/// rail, My cards mode the collection tree.
///
/// The rail is rendered for the whole Catalog mode rather than only on
/// `/catalog`, which is what "mode-filled" means: it reads and writes the same
/// `?q=` the catalog page does, so touching a filter from `/cards/:id` lands
/// you back on the catalog carrying that filter.
///
/// **Below `md` it is a slide-over drawer, not `display: none`.** It used to be
/// `hidden md:block`, which meant a phone had no collection tree at all — and
/// therefore no way to reach the tree's own management menu, which is where the
/// IA puts create / rename / **move** / delete.
///
/// One instance at every width, unlike Catalog's mobile story: `FilterSheet`
/// mounts a *second* `RailBody` (which is why that body takes a `heading_id`),
/// and a second `CollectionTreeNav` would duplicate the `id`s its context menu
/// and its collapsibles key off. The switch is therefore pure CSS — no media
/// query resolved in JS, so the markup the server renders is the markup that
/// hydrates, at every width. The closed drawer is `invisible`, not merely
/// off-screen: off-screen alone would leave every tree link Tab-reachable
/// behind the page.
#[component]
fn SidebarRail(my_mode: Memo<bool>, rail_open: RwSignal<bool>) -> impl IntoView {
    view! {
        // Below `md`: a fixed panel under the top bar, slid in by `left`
        // rather than `translate-x` — a transformed ancestor is a containing
        // block, and the tree's context menu is a top-layer popover positioned
        // in viewport coordinates.
        <Show when=move || rail_open.get()>
            <div
                class="fixed inset-x-0 top-14 bottom-0 z-40 bg-black/50 md:hidden"
                data-testid="rail-scrim"
                on:click=move |_| rail_open.set(false)
            />
        </Show>
        <aside
            id="sidebar-rail"
            aria-label="Sidebar"
            data-open=move || rail_open.get().then_some("true")
            class="bg-background invisible fixed top-14 bottom-0 -left-60 z-50 w-60 shrink-0 overflow-y-auto border-r transition-[left] duration-200 data-[open=true]:visible data-[open=true]:left-0 md:visible md:static md:z-auto md:overflow-visible"
        >
            <div class="space-y-4 p-4 md:sticky md:top-14">
                <Show
                    when=move || my_mode.get()
                    fallback=|| view! { <crate::catalog::rail::FilterRail /> }
                >
                    <crate::my::tree::CollectionTreeNav />
                </Show>
            </div>
        </aside>
    }
}

/// Mobile bottom tabs (wireframe: `[📖 Catalog] [🗂 My cards •N]`). The badge
/// is the Inbox unsorted count, read off the shared tree resource (hidden at
/// zero and on an anonymous shell).
#[component]
fn BottomTabs(my_mode: Memo<bool>) -> impl IntoView {
    let tree = expect_context::<crate::my::tree::CollectionTreeResource>().0;
    const TAB: &str = "flex flex-1 flex-col items-center justify-center gap-0.5 py-2 text-xs";

    view! {
        <nav
            aria-label="Primary"
            class="bg-background fixed inset-x-0 bottom-0 z-40 flex border-t md:hidden"
        >
            <a
                href="/catalog"
                class=move || {
                    format!(
                        "{TAB} {}",
                        if my_mode.get() { "text-muted-foreground" } else { "text-foreground" },
                    )
                }
                aria-current=move || (!my_mode.get()).then_some("page")
            >
                <span aria-hidden="true" class="text-base">
                    "📖"
                </span>
                <span>"Catalog"</span>
            </a>
            <a
                href="/my"
                class=move || {
                    format!(
                        "{TAB} {}",
                        if my_mode.get() { "text-foreground" } else { "text-muted-foreground" },
                    )
                }
                aria-current=move || my_mode.get().then_some("page")
            >
                <span class="flex items-center gap-1">
                    <span aria-hidden="true" class="text-base">
                        "🗂"
                    </span>
                    // NB: string fallback, never `|| ()` (the unit-fallback
                    // hydration trap, specs/auth.md Findings 2026-07-13).
                    <Suspense fallback=|| "">
                        {move || Suspend::new(async move {
                            let n = match tree.await {
                                Some(Ok(dto)) => crate::my::tree::assemble(dto).inbox_count,
                                _ => 0,
                            };
                            (n > 0)
                                .then(|| {
                                    view! {
                                        <Badge variant=BadgeVariant::Default size=BadgeSize::Sm>
                                            {n}
                                        </Badge>
                                    }
                                })
                        })}
                    </Suspense>
                </span>
                <span>"My cards"</span>
            </a>
        </nav>
    }
}

/// Top-bar account entry: signed in → avatar opening a popover with the
/// account line + sign out; anonymous → a sign-in link.
#[component]
fn UserMenu() -> impl IntoView {
    let user = expect_context::<CurrentUserResource>().0;
    let sign_out = ServerAction::<SignOut>::new();

    Effect::new(move |_| {
        if matches!(sign_out.value().get(), Some(Ok(()))) {
            // The recent-places ring is per-user (P6-145), but the hard
            // navigation below is a full document load, not an SPA route
            // change — `localStorage` is untouched by that, so a browser left
            // on this machine keeps the ring after sign-out unless it is
            // cleared here explicitly. `get_untracked` reads whatever
            // `CurrentUserResource` last resolved to, which is still this
            // (now signing-out) user: the resource itself is never refetched
            // (see `lib.rs`), so nothing has invalidated it yet.
            let current_id = match user.get_untracked() {
                Some(Ok(Some(current))) => Some(current.id),
                _ => None,
            };
            crate::components::palette::clear_recents(current_id.as_deref());
            // Full-page load, not SPA navigation: the shared current-user
            // resource and every consumer (guard, redirect, this menu) must
            // see the now-anonymous session; a document load of /catalog
            // re-runs SSR with the cleared cookies, no stale-resource races.
            hard_navigate("/catalog");
        }
    });

    view! {
        <Suspense fallback=|| {
            view! { <span class="text-muted-foreground text-xs">"…"</span> }
        }>
            {move || Suspend::new(async move {
                match user.await {
                    Ok(Some(CurrentUser { email, name, .. })) => {
                        let who = email.or(name).unwrap_or_else(|| "you".into());
                        let initial = who
                            .chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_else(|| "?".into());
                        view! {
                            <Popover id="user-menu" align=PopoverAlign::End>
                                <PopoverTrigger attr:aria-label="Account menu">
                                    <span class="bg-muted flex size-8 items-center justify-center rounded-full text-sm font-medium">
                                        {initial}
                                    </span>
                                </PopoverTrigger>
                                <PopoverContent class="w-64 space-y-3 p-4 text-sm">
                                    <p class="text-muted-foreground">
                                        "Signed in as " <span class="text-foreground">{who}</span>
                                    </p>
                                    <Separator />
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        class="w-full"
                                        on:click=move |_| {
                                            sign_out.dispatch(SignOut {});
                                        }
                                    >
                                        "Sign out"
                                    </Button>
                                </PopoverContent>
                            </Popover>
                        }
                            .into_any()
                    }
                    _ => {
                        view! {
                            <a href="/login" class="text-sm underline">
                                "Sign in"
                            </a>
                        }
                            .into_any()
                    }
                }
            })}
        </Suspense>
    }
}

// ---- Route skeletons — all graduated. ----
//
// `/cards/:id` left this file for `crate::cards` with the card-detail task, the
// same way `/catalog` did; `/my` for `crate::my::all_cards`,
// `/my/collections/:id` for `crate::my::collection`, and the last two —
// `/my/collections/:id/needs` and `/my/shopping` — for `crate::my::needs` and
// `crate::my::shopping`. No placeholder route bodies remain.

#[cfg(test)]
mod tests {
    use super::*;

    /// The two arms must differ, and both must clear the mobile tab bar. A
    /// single shared offset was the pre-audit behaviour and it collided with
    /// something at every width.
    #[test]
    fn toaster_offset_clears_whatever_is_docked_below_it() {
        let resting = toaster_offset(false);
        let with_tray = toaster_offset(true);
        assert_ne!(resting, with_tray);

        // Below `md` the bottom tab bar is always there, so neither arm may fall
        // back to the toaster's own `bottom-6` base at phone width — that is the
        // regression that had a toast painting over the tabs.
        for offset in [resting, with_tray] {
            let base = offset
                .split_whitespace()
                .find(|c| !c.contains(':'))
                .expect("each arm states an unprefixed (phone-width) bottom");
            assert!(
                base.starts_with("bottom-["),
                "phone-width offset must be an explicit clearance, got {base:?}",
            );
            assert_ne!(base, "bottom-6", "bottom-6 is under the tab bar");
        }

        // And the tray arm must sit higher than the resting arm at *both*
        // widths, so it carries a `md:` override of its own rather than
        // inheriting the resting desktop value.
        assert!(
            with_tray.contains("md:"),
            "the tray arm needs its own desktop clearance: {with_tray}",
        );
    }

    /// `?next=` is a same-origin path, and the guard builds it by hand.
    #[test]
    fn encode_path_for_query_keeps_slashes_and_escapes_the_rest() {
        assert_eq!(
            encode_path_for_query("/my/collections/x"),
            "/my/collections/x"
        );
        assert_eq!(
            encode_path_for_query("/my/all?q=a b"),
            "/my/all%3Fq%3Da%20b"
        );
    }
}
