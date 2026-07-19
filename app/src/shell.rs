//! App shell + routing (specs/app-ui.md "App shell"): the top bar with the
//! `Catalog | My cards` mode switch, the sidebar rail frame, mobile bottom
//! tabs, the `/` auth redirect, and the `/my/*` auth guard. Page bodies here
//! are route skeletons — each later Stage task replaces its own.

use leptos::prelude::*;
use leptos_router::components::{Outlet, Redirect};
use leptos_router::hooks::{use_location, use_params_map};

use crate::account::{fetch_current_user, CurrentUser, SignOut};
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::popover::{Popover, PopoverAlign, PopoverContent, PopoverTrigger};
use crate::components::ui::separator::Separator;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::theme_toggle::ThemeToggle;

/// The signed-in user, fetched once and shared by the shell, the `/` redirect,
/// the `/my/*` guard, and the user menu — one `fetch_current_user` per load,
/// never one per consumer.
#[derive(Clone, Copy)]
pub struct CurrentUserResource(
    pub Resource<Result<Option<CurrentUser>, ServerFnError<String>>>,
);

pub fn provide_current_user() {
    provide_context(CurrentUserResource(Resource::new(
        || (),
        |_| fetch_current_user(),
    )));
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
#[component]
pub fn RequireAuth() -> impl IntoView {
    let user = expect_context::<CurrentUserResource>().0;
    let location = use_location();
    view! {
        <Suspense fallback=|| {
            view! { <p class="text-muted-foreground p-8 text-sm">"Loading…"</p> }
        }>
            {move || {
                let pathname = location.pathname.get();
                let search = location.search.get();
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

    view! {
        <div class="bg-background text-foreground flex min-h-screen flex-col">
            <header class="bg-background sticky top-0 z-40 flex h-14 shrink-0 items-center gap-4 border-b px-4">
                <a href="/" class="text-sm font-semibold tracking-tight">
                    "Three Rings"
                </a>
                <ModeSwitch my_mode />
                <div class="ml-auto flex items-center gap-2">
                    <ThemeToggle />
                    <UserMenu />
                </div>
            </header>
            <div class="flex flex-1">
                <SidebarRail my_mode />
                // Mobile: pad past the fixed bottom tab bar.
                <main class="min-w-0 flex-1 pb-16 md:pb-0">
                    <Outlet />
                </main>
            </div>
            <BottomTabs my_mode />
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

/// Desktop sidebar rail — frame only. Catalog mode gets the filter rail with
/// the `/catalog` task; My cards mode gets the collection tree with its task.
#[component]
fn SidebarRail(my_mode: Memo<bool>) -> impl IntoView {
    view! {
        <aside aria-label="Sidebar" class="hidden w-60 shrink-0 border-r md:block">
            <div class="sticky top-14 space-y-4 p-4">
                <p class="text-muted-foreground text-xs font-medium uppercase tracking-wide">
                    {move || if my_mode.get() { "Collections" } else { "Filters" }}
                </p>
                <div class="space-y-2">
                    <div class="bg-muted/50 h-4 w-3/4 rounded-md"></div>
                    <div class="bg-muted/50 h-4 w-1/2 rounded-md"></div>
                    <div class="bg-muted/50 h-4 w-2/3 rounded-md"></div>
                </div>
            </div>
        </aside>
    }
}

/// Mobile bottom tabs (wireframe: `[📖 Catalog] [🗂 My cards •N]`). The badge
/// is the Inbox unsorted count — wired up by the collection-tree task, `None`
/// (hidden) until then.
#[component]
fn BottomTabs(my_mode: Memo<bool>) -> impl IntoView {
    let inbox_count: Option<u32> = None;
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
                    {inbox_count
                        .filter(|n| *n > 0)
                        .map(|n| {
                            view! {
                                <Badge variant=BadgeVariant::Default size=BadgeSize::Sm>
                                    {n}
                                </Badge>
                            }
                        })}
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

// ---- Route skeletons — each replaced by its own Stage 2/3 task. ----

/// `/catalog` — carries the seam-proving anonymous `catalog_count` read
/// (folded in from the old `/cards` placeholder); the search bar, grid, and
/// rail content land with the `/catalog` task.
#[component]
pub fn CatalogPage() -> impl IntoView {
    let count = Resource::new(|| (), |_| crate::catalog_count());
    view! {
        <div class="space-y-6 p-6">
            <h1 class="text-2xl font-bold">"Catalog"</h1>
            <Suspense fallback=|| {
                view! { <p class="text-muted-foreground text-sm">"Loading catalog…"</p> }
            }>
                {move || Suspend::new(async move {
                    match count.await {
                        Ok(count) => {
                            view! {
                                <p class="text-muted-foreground text-sm">
                                    {format!("{} cards in the catalog.", count.cards)}
                                </p>
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! {
                                <p class="text-muted-foreground text-sm">
                                    {format!("Failed to load catalog: {e}")}
                                </p>
                            }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
            <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
                {(0..8).map(|_| view! { <Skeleton class="aspect-[5/7] w-full" /> }).collect_view()}
            </div>
        </div>
    }
}

/// `/cards/:id` — full detail (printings, rulings, your-copies) lands with
/// the card-detail task.
#[component]
pub fn CardDetailPage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();
    view! {
        <div class="space-y-6 p-6">
            <h1 class="text-2xl font-bold">"Card detail"</h1>
            <p class="text-muted-foreground text-sm">{id}</p>
            <div class="flex gap-6">
                <Skeleton class="aspect-[5/7] w-48 shrink-0" />
                <div class="flex-1 space-y-3">
                    <Skeleton class="h-6 w-1/2" />
                    <Skeleton class="h-4 w-1/3" />
                    <Skeleton class="h-4 w-2/3" />
                </div>
            </div>
        </div>
    }
}

/// `/my` — the All-cards aggregate table lands with its task.
#[component]
pub fn MyCardsPage() -> impl IntoView {
    view! {
        <div class="space-y-6 p-6">
            <h1 class="text-2xl font-bold">"All cards"</h1>
            <div class="space-y-2">
                {(0..6).map(|_| view! { <Skeleton class="h-8 w-full" /> }).collect_view()}
            </div>
        </div>
    }
}

/// `/my/collections/:id` — binder/deck view lands with its task.
#[component]
pub fn CollectionPage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();
    view! {
        <div class="space-y-6 p-6">
            <h1 class="text-2xl font-bold">"Collection"</h1>
            <p class="text-muted-foreground text-sm">{id}</p>
            <div class="space-y-2">
                {(0..6).map(|_| view! { <Skeleton class="h-8 w-full" /> }).collect_view()}
            </div>
        </div>
    }
}

/// `/my/collections/:id/needs` — needs buckets + pick list land with their task.
#[component]
pub fn NeedsPage() -> impl IntoView {
    view! {
        <div class="space-y-6 p-6">
            <h1 class="text-2xl font-bold">"Needs"</h1>
            <div class="space-y-2">
                {(0..4).map(|_| view! { <Skeleton class="h-8 w-full" /> }).collect_view()}
            </div>
        </div>
    }
}

/// `/my/shopping` — the shopping list page lands with its task.
#[component]
pub fn ShoppingPage() -> impl IntoView {
    view! {
        <div class="space-y-6 p-6">
            <h1 class="text-2xl font-bold">"Shopping list"</h1>
            <div class="space-y-2">
                {(0..4).map(|_| view! { <Skeleton class="h-8 w-full" /> }).collect_view()}
            </div>
        </div>
    }
}
