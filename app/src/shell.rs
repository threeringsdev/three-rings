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
            // Mounted once, at the root: a toast outlives the row that raised
            // it (an undo toast must survive the search that scrolls its card
            // away), so it cannot live inside the page it was raised from.
            <Toaster />
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

/// Desktop sidebar rail — mode-filled (specs/app-ui.md): Catalog mode gets the
/// filter rail, My cards mode the collection tree.
///
/// The rail is rendered for the whole Catalog mode rather than only on
/// `/catalog`, which is what "mode-filled" means: it reads and writes the same
/// `?q=` the catalog page does, so touching a filter from `/cards/:id` lands
/// you back on the catalog carrying that filter.
#[component]
fn SidebarRail(my_mode: Memo<bool>) -> impl IntoView {
    view! {
        <aside aria-label="Sidebar" class="hidden w-60 shrink-0 border-r md:block">
            <div class="sticky top-14 space-y-4 p-4">
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

// `/cards/:id` graduated out of this file into `crate::cards` with the
// card-detail task, the same way `/catalog` did.

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
