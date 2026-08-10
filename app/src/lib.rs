// Leptos view trees are one deeply-nested generic type per component, so the
// type rustc has to resolve grows with the *page*, not with any one function.
// The filter rail (seven stacked sections inside the shell's sidebar) crossed
// the 128 default and failed to compile — but only for `aarch64-linux-android`,
// which is the trap: the host targets still built, so nothing caught it until
// the Android build ran. Raising the limit is the standard fix for this in
// Leptos; the alternative is splitting components purely to appease the
// compiler, which makes the UI code worse to read for no runtime benefit.
#![recursion_limit = "512"]

use leptos::children::ToChildren;
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{ParentRoute, Route, Router, Routes, RoutesProps},
    ParamSegment, SsrMode, StaticSegment,
};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    // Dark is the default theme (specs/app-ui.md, maintainer 2026-07-17); an
    // explicit toggle override is persisted in the `tr_theme` cookie and
    // re-applied here on every server render, so the class is right before
    // any wasm runs (no flash, no hydration mismatch). The <html> attributes
    // are outside the hydrated root, so the client toggle owns them after
    // hydration (components/ui/theme_toggle.rs).
    let dark = initial_theme_is_dark();
    // data-ssr-path records which URL this document was actually rendered
    // for. The Tauri Android webview reaches the server through an
    // in-process proxy that follows server-side redirects internally, so the
    // webview can receive the redirect *target's* HTML while its address bar
    // still shows the original URL — hydrating would panic (the router
    // renders the URL's route against the target's DOM). The hydrate entry
    // (shell::hydrate_entry) compares this stamp against location.pathname
    // and hard-replaces instead of hydrating on mismatch. Like the theme
    // class, <html> attributes live outside the hydrated root.
    let ssr_path = ssr_path_and_query();
    view! {
        <!DOCTYPE html>
        <html lang="en" class=if dark { "dark" } else { "" } data-ssr-path=ssr_path>
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

/// Stamp `data-hydrated` on `<html>` once the wasm client has taken over.
///
/// A test seam, and a deliberate one. Every page here is SSR-then-hydrate, so
/// there is a window where the markup is on screen but no event listener is
/// attached yet — input typed in it is dropped, and a test that types during
/// that window fails intermittently for reasons that have nothing to do with
/// what it is testing (observed while writing the filter-rail specs: the same
/// `page.fill` passed alone and failed under parallel load).
///
/// `Effect`s do not run during SSR, so the attribute's presence *is* the
/// definition of "hydrated" rather than an approximation of it. See
/// `end2end/tests/helpers.ts` for the matching wait.
fn mark_hydrated() {
    Effect::new(|_| {
        #[cfg(feature = "hydrate")]
        if let Some(el) = document().document_element() {
            let _ = el.set_attribute("data-hydrated", "true");
        }
    });
}

/// The `tr_theme` cookie override, else the dark default — shared with the
/// toggle so the shell and the component can never disagree.
fn initial_theme_is_dark() -> bool {
    components::ui::theme_toggle::cookie_theme_is_dark()
}

/// The request's path + query during SSR (from the axum `Parts` in context),
/// `""` outside a request. Feeds the `data-ssr-path` stamp on `<html>`.
fn ssr_path_and_query() -> String {
    #[cfg(feature = "ssr")]
    {
        if let Some(parts) = use_context::<http::request::Parts>() {
            return parts
                .uri
                .path_and_query()
                .map(|pq| pq.to_string())
                .unwrap_or_else(|| parts.uri.path().to_string());
        }
    }
    String::new()
}

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();
    shell::provide_current_user();
    mark_hydrated();

    // Route definitions are composed as a plain tuple (what the view! macro
    // builds from <Routes> children anyway) so the bench route can be
    // feature-gated: cfg on a node inside view! has no way to express
    // "no route here", and Routes is fed through its props builder because
    // the macro only accepts literal <Route> nodes as its children.
    //
    // Route map per specs/app-ui.md. `/` and the `/my/*` pages are
    // SsrMode::Async so their auth redirects can still set a real 302 —
    // out-of-order streaming would have sent headers before the user
    // resource resolves. Auth pages and the bench live outside the shell.
    let routes = view! {
        <Route path=StaticSegment("") view=shell::RootRedirect ssr=SsrMode::Async />
        <ParentRoute path=StaticSegment("") view=shell::AppShell>
            <Route path=StaticSegment("catalog") view=catalog::CatalogPage />
            // `Async`, not the default out-of-order streaming: this page is
            // public and shareable, so the detail has to be in the markup a
            // crawler or `curl` receives. Under OutOfOrder the whole
            // Transition ships as a <template> + hoisting script and the
            // in-place HTML is the skeleton (verified with curl during the
            // card-detail task).
            <Route
                path=(StaticSegment("cards"), ParamSegment("id"))
                view=cards::CardDetailPage
                ssr=SsrMode::Async
            />
            <ParentRoute path=StaticSegment("my") view=shell::RequireAuth>
                <Route path=StaticSegment("") view=my::all_cards::AllCardsPage ssr=SsrMode::Async />
                // The All-cards table on its own route. `/my` is a drill-down
                // list of collections below `md` (wireframes → "Mobile — My
                // cards root"), so the table needs a URL a phone can reach;
                // desktop `/my` still renders it, and every existing link and
                // `?q=`/`?cursor=` deep link there is untouched.
                <Route
                    path=StaticSegment("all")
                    view=my::all_cards::AllCardsTablePage
                    ssr=SsrMode::Async
                />
                <Route
                    path=(StaticSegment("collections"), ParamSegment("id"))
                    view=my::collection::CollectionPage
                    ssr=SsrMode::Async
                />
                <Route
                    path=(StaticSegment("collections"), ParamSegment("id"), StaticSegment("needs"))
                    view=my::needs::NeedsPage
                    ssr=SsrMode::Async
                />
                <Route
                    path=StaticSegment("shopping")
                    view=my::shopping::ShoppingPage
                    ssr=SsrMode::Async
                />
                <Route
                    path=StaticSegment("recently-deleted")
                    view=my::recently_deleted::RecentlyDeletedPage
                    ssr=SsrMode::Async
                />
            </ParentRoute>
        </ParentRoute>
        <Route path=StaticSegment("login") view=auth_pages::LoginPage />
        <Route path=StaticSegment("signup") view=auth_pages::SignupPage />
    }
    .into_inner();
    #[cfg(feature = "component-bench")]
    let routes = (
        routes,
        view! { <Route path=(StaticSegment("dev"), StaticSegment("components")) view=bench::BenchPage /> }
        .into_inner(),
    );

    view! {
        <Stylesheet id="leptos" href="/pkg/app.css" />

        <Title text="Three Rings" />

        <Router>
            {Routes(
                RoutesProps::builder()
                    .fallback(|| "Page not found.".into_view())
                    .children(ToChildren::to_children(move || routes))
                    .build(),
            )}
        </Router>
    }
}

pub mod account;
pub mod auth_pages;
#[cfg(feature = "component-bench")]
pub mod bench;
pub mod cards;
pub mod catalog;
pub mod components;
pub mod my;
pub mod shell;

#[cfg(feature = "ssr")]
pub mod auth;

/// The data-access trait seam (specs/data-access-backends.md). Present whenever
/// the embedded server is built; the concrete backend is picked by the
/// `hosted`/`native` feature inside.
#[cfg(feature = "ssr")]
pub mod backend;

/// Direct Neon access — the pool + the migration runner. Behind `hosted`: only
/// the web deployment (the authorization terminus) holds Postgres credentials;
/// the native shell reaches data over HTTPS instead.
#[cfg(feature = "hosted")]
pub mod db;

/// Catalog ingestion — the Scryfall bulk pipeline (`server --ingest`,
/// specs/catalog-ingestion.md). Behind `hosted` like `db`: it writes the
/// catalog tables directly (as the `catalog_ingest` role), which only the
/// hosted deployment ever does.
#[cfg(feature = "hosted")]
pub mod ingest;

/// Catalog search — the query grammar + its SQL emission
/// (specs/catalog-search.md). Behind `hosted`: only the backend that owns
/// the sqlx search query needs it.
#[cfg(feature = "hosted")]
pub mod search;

/// Dev seed data for the test user (specs/app-ui.md) — `server --seed-dev`.
/// Debug builds only: unlike `--ingest` (which requires the dedicated
/// `INGEST_DATABASE_URL` credential), the seed writes through the runtime
/// `DATABASE_URL`, so compiling it out of release binaries is what keeps the
/// production deployment from ever carrying a data-mutating CLI arm.
#[cfg(all(feature = "hosted", debug_assertions))]
pub mod seed;

/// Map a data-access [`shared::ApiError`] onto a server-fn error. The transport
/// channel carries the message; richer status semantics are collection-api's.
#[cfg(feature = "ssr")]
fn api_err(e: shared::ApiError) -> ServerFnError<String> {
    ServerFnError::ServerError(e.to_string())
}

/// Anonymous catalog size — the seam-proving catalog read
/// (specs/data-access-backends.md). Hosted: sqlx in-process. Native: HTTPS to
/// the hosted API. Both go through the `CatalogStore` trait, never the DB/HTTP
/// directly.
#[server(prefix = "/api", endpoint = "catalog_count")]
pub async fn catalog_count() -> Result<shared::CatalogCount, ServerFnError<String>> {
    #[cfg(feature = "hosted")]
    {
        use crate::backend::{CatalogStore, HostedBackend};
        HostedBackend::anonymous()
            .await
            .map_err(api_err)?
            .card_count()
            .await
            .map_err(api_err)
    }
    #[cfg(all(feature = "native", not(feature = "hosted")))]
    {
        use crate::backend::{CatalogStore, NativeBackend};
        NativeBackend::anonymous()
            .card_count()
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// One keyset page of catalog search results (specs/catalog-search.md) — the
/// exemplar thin server-fn adapter the later page tasks copy: extract headers,
/// pick the backend, project one trait method, map the error. No business logic
/// here; the grammar and its SQL live behind `CatalogStore::search`.
///
/// **GET, not the server-fn POST default.** This is a pure read whose arguments
/// belong in a cacheable URL, and the Tauri Android dev proxy strips POST bodies
/// (specs/ui-work-loop.md Findings) — a POST adapter is unverifiable on-device.
///
/// Auth is **opportunistic**: a valid session fills `CardSummary::owned`, an
/// absent or expired one degrades to the anonymous public projection rather than
/// 401ing. `/catalog` is a public page.
#[server(
    prefix = "/api",
    endpoint = "search_catalog",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn search_catalog(
    q: String,
    cursor: Option<String>,
) -> Result<shared::SearchResults, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    let headers = leptos_axum::extract::<axum::http::HeaderMap>()
        .await
        .map_err(|e| ServerFnError::ServerError(e.to_string()))?;
    #[cfg(feature = "ssr")]
    let (query, page) = (
        shared::SearchQuery { q: Some(q) },
        shared::Page {
            cursor,
            limit: None,
        },
    );

    #[cfg(feature = "hosted")]
    {
        use crate::backend::CatalogStore;
        crate::backend::routes::catalog_backend(&headers)
            .await
            .map_err(api_err)?
            .search(query, page)
            .await
            .map_err(api_err)
    }
    #[cfg(all(feature = "native", not(feature = "hosted")))]
    {
        use crate::auth::cookies;
        use crate::backend::{CatalogStore, NativeBackend};
        // Same opportunistic rule, expressed the native way: hand the backend
        // whatever session material the webview has (either may be absent) and
        // let the hosted terminus decide. It answers anonymously rather than
        // 401ing when the token is missing, so this needs no fallback arm.
        let token = cookies::cookie_value(&headers, cookies::JWT_COOKIE);
        let session = cookies::cookie_value(&headers, cookies::SESSION_COOKIE);
        let origin = cookies::request_origin(&headers);
        NativeBackend::authed(token, session, origin)
            .search(query, page)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (q, cursor);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// One window of the set list for the filter rail's Set facet
/// (specs/catalog-search.md → the `s:` term). A thin projection of
/// `CatalogStore::list_sets`, GET for the same two reasons as
/// [`search_catalog`]: a pure cacheable read, and the Tauri Android dev proxy
/// strips POST bodies.
///
/// **Anonymous on both backends** — sets carry no ownership, so unlike the card
/// reads there is no opportunistic-session arm here. `q` blank means "browse the
/// newest sets"; [`shared::SetQuery::term`] owns that rule so both backends
/// apply it identically.
#[server(
    prefix = "/api",
    endpoint = "list_sets",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn list_sets(q: String) -> Result<Vec<shared::SetSummary>, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    let query = shared::SetQuery {
        q: Some(q),
        limit: None,
    };

    #[cfg(feature = "hosted")]
    {
        use crate::backend::{CatalogStore, HostedBackend};
        HostedBackend::anonymous()
            .await
            .map_err(api_err)?
            .list_sets(query)
            .await
            .map_err(api_err)
    }
    #[cfg(all(feature = "native", not(feature = "hosted")))]
    {
        use crate::backend::{CatalogStore, NativeBackend};
        NativeBackend::anonymous()
            .list_sets(query)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = q;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// One card's full detail — printings, rulings, and (authed only) the caller's
/// copies and where they live. Same thin-adapter shape as [`search_catalog`],
/// and **GET** for the same two reasons: a pure cacheable read, and the Tauri
/// Android dev proxy strips POST bodies.
///
/// Auth is **opportunistic** — `catalog_backend` hands back a session-scoped
/// backend when the caller has one and an anonymous backend otherwise, which is
/// exactly what decides whether `CardDetail::ownership` is `Some`. `/cards/:id`
/// is a public page; a missing or expired session degrades to the public
/// projection rather than 401ing.
#[server(
    prefix = "/api",
    endpoint = "card_detail",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn card_detail(
    oracle_id: shared::Id,
) -> Result<shared::CardDetail, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    let headers = leptos_axum::extract::<axum::http::HeaderMap>()
        .await
        .map_err(|e| ServerFnError::ServerError(e.to_string()))?;

    #[cfg(feature = "hosted")]
    {
        use crate::backend::CatalogStore;
        crate::backend::routes::catalog_backend(&headers)
            .await
            .map_err(api_err)?
            .card_detail(oracle_id)
            .await
            .map_err(api_err)
    }
    #[cfg(all(feature = "native", not(feature = "hosted")))]
    {
        use crate::auth::cookies;
        use crate::backend::{CatalogStore, NativeBackend};
        let token = cookies::cookie_value(&headers, cookies::JWT_COOKIE);
        let session = cookies::cookie_value(&headers, cookies::SESSION_COOKIE);
        let origin = cookies::request_origin(&headers);
        NativeBackend::authed(token, session, origin)
            .card_detail(oracle_id)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = oracle_id;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The signed-in caller's collections — the seam-proving session-scoped read
/// (specs/data-access-backends.md). Hosted: verifies the JWT here, then runs the
/// read inside the `SET LOCAL app.user_id` transaction. Native: forwards the
/// `tr_jwt` cookie as `Authorization: Bearer` to the hosted API, which is the
/// authorization terminus. collection-api builds the UI that consumes this.
#[server(prefix = "/api", endpoint = "list_collections")]
pub async fn list_collections() -> Result<Vec<shared::CollectionSummary>, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .list_collections()
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The My-cards sidebar read (specs/app-ui.md → Collection tree): every
/// collection with its own present count plus the shopping-short badge, one
/// round-trip; `crate::my::tree` reassembles nesting and rolls up the badges.
/// GET per the read-adapter exemplar (`search_catalog`): cacheable URL, and the
/// Tauri Android dev proxy strips POST bodies.
#[server(
    prefix = "/api",
    endpoint = "collection_tree",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn collection_tree() -> Result<shared::CollectionTree, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .collection_tree()
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// One keyset page of the `/my` everything-view (specs/app-ui.md → `/my`):
/// every card across every collection, with its owned/wanted totals and the
/// collections holding it. GET per the read-adapter exemplar
/// ([`search_catalog`]): the page's `?q=`/`?cursor=` state *is* this call's
/// arguments, so a shared or reloaded URL SSRs the same page.
///
/// `q` is the quick search — a plain name substring, deliberately not the
/// catalog grammar (see `CollectionStore::all_cards`). It arrives as a plain
/// `String` rather than `Option<String>` because the URL's absent-vs-empty
/// distinction is meaningless here; the backend trims and treats empty as
/// browse-all.
#[server(
    prefix = "/api",
    endpoint = "all_cards",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn all_cards(
    q: String,
    cursor: Option<String>,
) -> Result<shared::AllCardsView, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .all_cards(
                Some(q),
                shared::Page {
                    cursor,
                    limit: None,
                },
            )
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (q, cursor);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// One keyset page of a collection's binder/deck view (specs/app-ui.md →
/// `/my/collections/:id`): its metadata, immediate children, card rows,
/// whole-collection totals and — on a deck — its commanders. GET per the
/// read-adapter exemplar ([`search_catalog`]), so `?q=`/`?cursor=` on the page
/// are literally this call's arguments and a shared URL SSRs the same page.
///
/// `q` is the in-collection quick search (a name substring, not the catalog
/// grammar), and a plain `String` for the same reason [`all_cards`]'s is.
#[server(
    prefix = "/api",
    endpoint = "collection_view",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn collection_view(
    id: shared::Id,
    q: String,
    cursor: Option<String>,
) -> Result<shared::CollectionView, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .collection_view(
                id,
                Some(q),
                shared::Page {
                    cursor,
                    limit: None,
                },
            )
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (id, q, cursor);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Set a holding's absolute quantity — the collection view's in-place count
/// stepper (specs/app-ui.md → "HERE is editable in place via the count
/// stepper"). `0` deletes the holding row, which is the component's documented
/// meaning for a committed zero.
///
/// Addressed by **holding id**, not by (collection, printing, board): a cell
/// that sums several finish/condition/language grains has no single row a lone
/// number could mean, so `CardRow::holding_id` is `None` there and the stepper
/// is not offered. POST, necessarily — it writes.
#[server(prefix = "/api", endpoint = "set_holding_quantity")]
pub async fn set_holding_quantity(
    holding_id: shared::Id,
    quantity: i32,
) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .set_holding_quantity(holding_id, shared::SetQuantity { quantity })
            .await
            .map(|_| ())
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (holding_id, quantity);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Remove a holding's copies from its collection — the collection view's
/// committed **0** (specs/app-ui.md → `/my/collections/:id`;
/// specs/collection-api.md → Move, `to = None`).
///
/// **Not `set_holding_quantity(id, 0)`, and that is the whole point.** Setting a
/// holding to zero runs `DELETE FROM holdings`, after which nothing can put the
/// copies back: the row's id is dead, and no other write knows what grain or
/// board it held (`CardRow` is `(printing, board)` with finish/condition/
/// language summed away). The count stepper offers Undo on every commit, so for
/// two tasks the floor here was `min = 1` — the destructive commit made
/// unreachable rather than reachable-and-lying, at the price of a binder card
/// that could not be removed at all.
///
/// A removal is a **move with no destination**. The server reads the grain, the
/// board and the owning collection off the named holding *inside the write
/// transaction* and appends a `moves` row, so undo is the ledger's `undone_at`
/// and puts those copies back on that board — the same undo every other move
/// gets. Returns the move id for the toast.
///
/// **The whole stack, not a client-supplied count**: "remove this row" is what
/// the user asked for, and a stale rendered count would otherwise leave copies
/// behind. Scalar in, scalar out (the `quick_add` convention).
#[server(prefix = "/api", endpoint = "remove_holding")]
pub async fn remove_holding(holding_id: shared::Id) -> Result<shared::Id, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .move_holding(
                holding_id,
                shared::HoldingMove {
                    to_collection_id: None,
                    quantity: None,
                },
            )
            .await
            .map(|r| r.move_id)
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = holding_id;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Undo one move by id — the toast action behind [`remove_holding`].
///
/// Idempotent at the trait level, so a double-clicked toast is harmless. It is
/// the same trait call [`undo_quick_add`] makes; the two are separate adapters
/// because each surface's endpoint names what it undoes, and collapsing them
/// into one generic endpoint is filed as follow-up rather than done here.
#[server(prefix = "/api", endpoint = "undo_move")]
pub async fn undo_move(move_id: shared::Id) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .undo_move(move_id)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = move_id;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Empty a deck (specs/app-ui.md → the deck variant's "Empty deck…" teardown;
/// specs/collection-api.md → Teardown). Returns how many move rows it wrote.
///
/// **Scalar, not the [`shared::Teardown`] enum** (the quick_add convention, and
/// the server-fn POST codec mangles nested/tagged DTOs anyway — app-ui
/// Findings): `to_collection_id = Some(dest)` is "empty to here",
/// `None` is "return each card to the collection it last came from, Inbox where
/// there is no history".
#[server(prefix = "/api", endpoint = "teardown_collection")]
pub async fn teardown_collection(
    collection_id: shared::Id,
    to_collection_id: Option<shared::Id>,
) -> Result<shared::TeardownReceipt, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        let mode = match to_collection_id {
            Some(to_collection_id) => shared::Teardown::EmptyTo { to_collection_id },
            None => shared::Teardown::ReturnToPrevious,
        };
        collection_backend()
            .await?
            .teardown(collection_id, mode)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (collection_id, to_collection_id);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Create a binder or deck from the tree's context menu (specs/app-ui.md →
/// Collection tree, management). **Scalars, not the whole [`shared::NewCollection`]**
/// — the quick_add convention: the tree's create dialog never sets a format,
/// so the adapter's wire contract cannot carry one either (`format: None` by
/// construction; the deck view's format editing is its own task's adapter).
#[server(prefix = "/api", endpoint = "create_collection")]
pub async fn create_collection(
    parent_id: Option<shared::Id>,
    kind: shared::CollectionKind,
    name: String,
) -> Result<shared::CollectionSummary, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .create_collection(shared::NewCollection {
                parent_id,
                kind,
                name,
                format: None,
            })
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (parent_id, kind, name);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Rename a collection (tree context menu). The API rejects the Inbox with a
/// 409 — surfaced, not pre-hidden, so a raw retry can't silently no-op.
#[server(prefix = "/api", endpoint = "rename_collection")]
pub async fn rename_collection(
    id: shared::Id,
    name: String,
) -> Result<shared::CollectionSummary, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .rename_collection(id, shared::Rename { name })
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (id, name);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Delete a collection (tree context menu) — which since
/// specs/collection-deletion.md **relocates rather than destroys**: the node is
/// hidden, its children re-parent to its parent, and its cards move out through
/// the real ledger. Returns the receipt (the handles a future undo toast
/// reverses), never a count.
///
/// **Scalar dispositions, not the tagged enums** (the quick_add convention,
/// and the reason `teardown_collection` above takes an `Option<Id>` instead of
/// `shared::Teardown`: the server-fn POST codec mangles nested/tagged DTOs —
/// app-ui Findings). The confirm dialog's two pickers (`P6-189`) offer three of
/// `HaveDisposition`'s four states and both of `WantDisposition`'s:
///
/// - `haves_to = Some(id)` → `HaveDisposition::To { collection_id: id }`.
/// - `haves_to = None, haves_discard = true` → `HaveDisposition::Discard`
///   ("Remove from Collection" on the have side); `haves_discard` wins over an
///   unset `haves_to`, since the dialog only ever sends one or the other.
/// - `haves_to = None, haves_discard = false` → `HaveDisposition::ToParent`,
///   the spec's default and the only reading of "neither was stated" — which
///   is also what a caller with no picker at all still gets (every call site
///   before this task, and every e2e cleanup helper today).
/// - `wants_to = Some(id)` → `WantDisposition::To { collection_id: id }`;
///   `None` → `WantDisposition::Discard`, the spec's default.
///
/// `HaveDisposition::ReturnToPrevious` stays reachable on the wire (the hosted
/// route is unchanged, per specs/collection-deletion.md step 4's "must not
/// change") but this adapter has no parameter for it — the confirm dialog's
/// wireframe offers exactly two controls, not a third for a mode
/// `teardown_collection` already covers elsewhere.
#[server(prefix = "/api", endpoint = "delete_collection")]
pub async fn delete_collection(
    id: shared::Id,
    haves_to: Option<shared::Id>,
    haves_discard: bool,
    wants_to: Option<shared::Id>,
) -> Result<shared::DeleteCollectionReceipt, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        let haves = match (haves_to, haves_discard) {
            (_, true) => shared::HaveDisposition::Discard,
            (Some(collection_id), false) => shared::HaveDisposition::To { collection_id },
            (None, false) => shared::HaveDisposition::ToParent,
        };
        let wants = match wants_to {
            Some(collection_id) => shared::WantDisposition::To { collection_id },
            None => shared::WantDisposition::Discard,
        };
        collection_backend()
            .await?
            .delete_collection(shared::DeleteCollectionReq {
                collection_id: id,
                haves,
                wants,
            })
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (id, haves_to, haves_discard, wants_to);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Undo a delete whole — the misclick path, fired from the delete toast's
/// Undo button (specs/collection-deletion.md → step 5).
///
/// **The whole receipt, not scalars.** Every other write adapter here takes
/// scalars because the server-fn POST codec mangles nested/tagged DTOs — but
/// `DeleteCollectionReceipt` carries no tagged enum, only ids and a list of
/// plain structs (`RelocatedDesire`), so `input = Json` (the same fix
/// `undo_selection_move`/`pull_needs` use for their own `Vec<…>` arguments)
/// handles it directly. **Client-held**: the receipt is already the return
/// value of [`delete_collection`] above, so the toast's own closure holds it
/// and posts it back whole rather than the server re-deriving or stashing it.
#[server(
    prefix = "/api",
    endpoint = "undo_delete_collection",
    input = leptos::server_fn::codec::Json
)]
pub async fn undo_delete_collection(
    receipt: shared::DeleteCollectionReceipt,
) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .undo_delete(receipt)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = receipt;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Restore a soft-deleted collection from the "Recently deleted" list — the
/// weaker, later recovery path (specs/collection-deletion.md → step 5).
/// Scalar id in, matching the rest of this adapter's convention; unlike
/// [`undo_delete_collection`] there is no receipt to carry.
#[server(prefix = "/api", endpoint = "restore_collection")]
pub async fn restore_collection(id: shared::Id) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .restore_collection(id)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = id;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The "Recently deleted" list (specs/collection-deletion.md → step 5): the
/// caller's own soft-deleted collections, newest first. GET, per the
/// read-adapter exemplar — a plain list on a shareable URL.
#[server(
    prefix = "/api",
    endpoint = "recently_deleted",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn recently_deleted() -> Result<Vec<shared::DeletedCollectionRow>, ServerFnError<String>>
{
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .recently_deleted()
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Reparent a collection (tree drag). `new_parent_id = None` = top level.
/// The API is the cycle-guard terminus (409 when the target parent is the
/// node or one of its descendants) — the client pre-checks only to paint
/// drop targets, never to decide legality.
#[server(prefix = "/api", endpoint = "reparent_collection")]
pub async fn reparent_collection(
    id: shared::Id,
    new_parent_id: Option<shared::Id>,
) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .reparent_collection(id, shared::Reparent { new_parent_id })
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (id, new_parent_id);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Set a collection's fractional sibling position (tree drag) — the client
/// computed the midpoint of the neighbors it dropped between.
#[server(prefix = "/api", endpoint = "reorder_collection")]
pub async fn reorder_collection(
    id: shared::Id,
    position: f64,
) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .reorder_collection(id, shared::Reorder { position })
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (id, position);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The session-scoped collection backend for a server fn, one per backend
/// feature. Collection work — unlike the catalog reads — has no anonymous
/// degradation, so this 401s rather than falling back.
///
/// Both arms extract the headers themselves and return the *same* shape, so an
/// adapter body is one line per trait call instead of a duplicated
/// header→session→backend chain per `cfg`. That duplication is what let the
/// original `list_collections` inline a rule `catalog_backend` had already
/// centralized for the catalog half; keeping one helper per backend is what
/// stops the two halves drifting again.
#[cfg(feature = "hosted")]
async fn collection_backend() -> Result<crate::backend::HostedBackend, ServerFnError<String>> {
    let headers = leptos_axum::extract::<axum::http::HeaderMap>()
        .await
        .map_err(|e| ServerFnError::ServerError(e.to_string()))?;
    crate::backend::routes::session_backend(&headers)
        .await
        .map_err(api_err)
}

#[cfg(all(feature = "native", not(feature = "hosted")))]
async fn collection_backend() -> Result<crate::backend::NativeBackend, ServerFnError<String>> {
    use crate::auth::cookies;
    let headers = leptos_axum::extract::<axum::http::HeaderMap>()
        .await
        .map_err(|e| ServerFnError::ServerError(e.to_string()))?;
    // The native embedded server never verifies the JWT — it forwards it to the
    // hosted terminus, which does. We hand the backend both the current `tr_jwt`
    // (may be absent once the 15-min token expires) and the long-lived
    // `tr_session` + our origin, so a hosted 401 triggers a silent re-mint + one
    // retry rather than surfacing as Unauthorized.
    let token = cookies::cookie_value(&headers, cookies::JWT_COOKIE);
    let session = cookies::cookie_value(&headers, cookies::SESSION_COOKIE);
    let origin = cookies::request_origin(&headers);
    Ok(crate::backend::NativeBackend::authed(
        token, session, origin,
    ))
}

/// The catalog quick-add: one card, one destination, from `+ Want` / `+ Have`
/// on a `/catalog` row (specs/app-ui.md → `/catalog`). Returns a
/// [`shared::QuickAddReceipt`] whose `undo_move_id` drives the toast's Undo.
///
/// **Have goes through `move_cards`, not `add_holding`** — a deliberate choice,
/// not an oversight. Both write the same thing (`add_holding` appends an intake
/// `moves` row of its own), but only `move_cards` *returns* that row's id, and
/// undo targets a specific move id (specs/collection-api.md → Undo). Routing a
/// Have through the intake form of a move (`from = None`) is how the toast gets
/// an undo handle without widening the trait. `undo_last_move` was the
/// alternative and was rejected: it races a second tab or a fast second click,
/// so the toast could undo a *different* add than the one it names.
///
/// **Want has no undo handle at all.** Desires are outside the move ledger and
/// the trait exposes no desire-quantity operation to compensate with, so the
/// receipt is `None` and the toast omits its action (queued as a follow-up).
///
/// POST, necessarily — this is a write. That means it cannot be exercised
/// through the Tauri Android *dev* proxy, which strips POST bodies
/// (specs/ui-work-loop.md Findings); the release webview is unaffected.
///
/// **The arguments are scalars, and the `AddLine` is built here.** An earlier
/// shape took the caller's whole `AddLine`, which let anything holding a
/// session POST a printing-pinned Want or a non-default board at an endpoint
/// whose entire contract is "one card, default grain". That is not a privilege
/// escalation — the same caller can already reach
/// `POST /api/collections/{id}/have` with any quantity on their *own*
/// collections — but an adapter whose wire contract is wider than its name is
/// a trap for the next caller. The grain is still true by construction; only
/// `quantity` is the caller's, because the quick-add panel's `⇧⏎ set count`
/// (specs/app-ui.md → Quick-add panel) is a keystroke of the shipped contract
/// and a playset cannot be four separate adds — undo targets one move row, so
/// four rows would need four undos.
#[server(prefix = "/api", endpoint = "quick_add")]
pub async fn quick_add(
    collection_id: shared::Id,
    kind: shared::QuickAddKind,
    oracle_id: shared::Id,
    printing_id: Option<shared::Id>,
    /// Copies to add, clamped server-side to `1..=`[`QUICK_ADD_MAX`] — a
    /// mistyped or hostile count can't write an absurd holding through a
    /// one-keystroke surface.
    quantity: u32,
) -> Result<shared::QuickAddReceipt, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        let backend = collection_backend().await?;
        let quantity = clamp_quick_add_quantity(quantity);
        match kind {
            shared::QuickAddKind::Have => {
                // Holdings are per-printing; a card whose oracle row resolved
                // no representative printing can be Wanted but not Had.
                let printing_id = printing_id.ok_or_else(|| {
                    ServerFnError::ServerError("this card has no printing to add".to_string())
                })?;
                let receipt = backend
                    .move_cards(shared::MoveRequest {
                        from_collection_id: None,
                        to_collection_id: Some(collection_id),
                        printing_id,
                        finish: shared::Finish::default(),
                        condition: shared::Condition::default(),
                        language: shared::default_language(),
                        // An intake has no source board; the copies land on the
                        // mainboard, which is where a catalog `+ Have` means.
                        from_board: shared::Board::default(),
                        to_board: shared::Board::default(),
                        quantity: quantity as i32,
                    })
                    .await
                    .map_err(api_err)?;
                Ok(shared::QuickAddReceipt {
                    undo_move_id: Some(receipt.move_id),
                })
            }
            shared::QuickAddKind::Want => {
                backend
                    .add_desire(
                        collection_id,
                        shared::AddWant {
                            oracle_id,
                            // No printing pin: "I want this card", not "I want
                            // this printing". Pinning is the card-detail
                            // surface's job, not a catalog row's.
                            printing_id: None,
                            board: shared::Board::default(),
                            quantity: quantity as i32,
                        },
                    )
                    .await
                    .map_err(api_err)?;
                Ok(shared::QuickAddReceipt { undo_move_id: None })
            }
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (collection_id, kind, oracle_id, printing_id, quantity);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The most copies one quick-add may write. A `⇧⏎` count is four digits away
/// from a typo, and the surface exists to add a playset (4), not a shipment.
pub const QUICK_ADD_MAX: u32 = 99;

/// Clamp a caller's count into `1..=`[`QUICK_ADD_MAX`]. Clamping rather than
/// rejecting: the count comes from a keystroke stream, and a 422 in the middle
/// of the metric path would cost more than capping the number.
pub fn clamp_quick_add_quantity(quantity: u32) -> u32 {
    quantity.clamp(1, QUICK_ADD_MAX)
}

/// Undo one quick-add, from its toast's action. Idempotent at the trait level,
/// so a double-click or a re-fired toast action is harmless.
#[server(prefix = "/api", endpoint = "undo_quick_add")]
pub async fn undo_quick_add(move_id: shared::Id) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .undo_move(move_id)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = move_id;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The selection tray's **batch move** (specs/app-ui.md → Selection tray;
/// specs/collection-api.md → "Move (batch)"): N selected rows → one
/// destination, one transaction, N ledger rows.
///
/// **This adapter is where a `/my` selection stops being un-moveable.** The
/// tray's key is an enum precisely so a `Card { oracle }` entry cannot be piped
/// into a [`shared::MoveItem`] — `from_collection_id: None` means *external
/// intake*, so guessing it would conjure copies out of nothing. Each key is
/// resolved here instead, against the caller's real holdings read **ungrouped**
/// (`holdings_of_oracle`). One movable candidate source ⇒ move; anything else ⇒
/// a [`SkipReason`] the caller reports by name.
///
/// [`SkipReason`]: crate::my::move_selection::SkipReason
///
/// **Ungrouped is the load-bearing word.** Every rendered read model collapses
/// the grain the write is addressed at — `collection_view` groups by
/// `(printing, board)`, `CardDetail::ownership` by `(collection, printing)` —
/// so a selectable row reading `present = 3` can be three foils, or copies on a
/// sideboard. Resolved against those, such an entry looks movable, and the
/// failure surfaces inside `holding_take` as `Conflict("no copies to move")`:
/// `move_batch` is one transaction, so one such row kills the whole batch with
/// an error naming no card. Every entry is now checked against the real
/// holdings first and refused individually, so the rest of the batch moves.
///
/// **Quantity is not the caller's** (the `quick_add` precedent): one copy per
/// selected entry, fixed here. The tray counts entries, not copies, so this is
/// the only quantity the pill does not lie about.
///
/// `input = Json` because the argument is a list of enums — the server-fn POST
/// default is URL-encoded and flattens nested DTOs to strings
/// (specs/app-ui.md Findings).
#[server(
    prefix = "/api",
    endpoint = "move_selection",
    input = leptos::server_fn::codec::Json
)]
pub async fn move_selection(
    to_collection_id: shared::Id,
    items: Vec<crate::my::move_selection::SelectionItem>,
) -> Result<crate::my::move_selection::MoveOutcome, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        use crate::components::ui::selection_tray::SelectionKey;
        use crate::my::move_selection::{
            resolve_card, resolve_held, CardSource, MoveOutcome, Skipped,
        };
        use std::collections::hash_map::Entry;
        use std::collections::HashMap;

        if items.len() > SELECTION_MOVE_MAX {
            return Err(ServerFnError::ServerError(format!(
                "a move can carry at most {SELECTION_MOVE_MAX} cards"
            )));
        }
        let backend = collection_backend().await?;

        let mut moved: Vec<String> = Vec::new();
        let mut skipped: Vec<Skipped> = Vec::new();
        let mut lines: Vec<shared::MoveItem> = Vec::new();
        // One holdings read per distinct card, not per entry: the tray can
        // hold the same card twice (a `/my` row and the collection row for the
        // same copies), and both resolve off the same rows.
        let mut owned: HashMap<shared::Id, Vec<shared::HoldingLine>> = HashMap::new();
        for item in items {
            let token = item.key.token();
            let holdings = match owned.entry(item.oracle_id) {
                Entry::Occupied(seen) => seen.into_mut(),
                Entry::Vacant(slot) => {
                    // Session-scoped: `collection_backend` carries the caller's
                    // identity, and the rows come back RLS-filtered to them.
                    let rows = backend
                        .holdings_of_oracle(item.oracle_id)
                        .await
                        .map_err(api_err)?;
                    slot.insert(rows)
                }
            };
            let source = match item.key {
                SelectionKey::Held {
                    collection_id,
                    printing_id,
                    board,
                } => resolve_held(
                    holdings,
                    collection_id,
                    printing_id,
                    board,
                    to_collection_id,
                ),
                SelectionKey::Card { oracle_id: _ } => resolve_card(holdings, to_collection_id),
            };
            match source {
                CardSource::Move(src) => {
                    moved.push(token);
                    lines.push(shared::MoveItem {
                        from_collection_id: Some(src.from),
                        printing_id: src.printing_id,
                        // The grain and board of the stack resolution actually
                        // found — never restated defaults. A `MoveItem` built at
                        // the default grain beside a foil-only stack is a write
                        // aimed at copies that do not exist, which reaches
                        // `holding_take` as a `Conflict` and rolls the whole
                        // batch back.
                        finish: src.finish,
                        condition: src.condition,
                        language: src.language,
                        from_board: src.board,
                        // Copies moved out of a sideboard into another
                        // collection are just copies there; re-labelling a board
                        // is card-tagging's separate op.
                        to_board: shared::Board::Main,
                        quantity: SELECTION_MOVE_QUANTITY,
                    });
                }
                CardSource::Refuse(reason) => skipped.push(Skipped { token, reason }),
            }
        }

        // No write at all when everything was refused: an empty batch would
        // still open a transaction and report a "successful" move of nothing.
        let move_ids = if lines.is_empty() {
            Vec::new()
        } else {
            match backend
                .move_batch(shared::BatchMove {
                    to_collection_id: Some(to_collection_id),
                    items: lines,
                })
                .await
            {
                Ok(receipts) => receipts.into_iter().map(|r| r.move_id).collect(),
                // The batch is one transaction, so this moved nothing. Trade the
                // item *index* the backend tagged the failure with for the
                // entry's token, so the client can put a card name on it — the
                // alternative is an error that names none of the cards the user
                // selected and is diagnosable only by bisecting the selection.
                Err(e) => {
                    return Err(match shared::batch_item_index(e.message()) {
                        Some((i, rest)) if i < moved.len() => {
                            ServerFnError::ServerError(format!("{}: {rest}", moved[i]))
                        }
                        _ => api_err(e),
                    })
                }
            }
        };

        Ok(MoveOutcome {
            move_ids,
            moved,
            skipped,
        })
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (to_collection_id, items);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Copies one selected row contributes to a batch move. Fixed, not the
/// caller's — see [`move_selection`].
#[cfg(feature = "ssr")]
const SELECTION_MOVE_QUANTITY: i32 = 1;

/// The most rows one batch move (or one destination-ranking read) may carry. A
/// tray selection is a hand of cards, not a shipment; the cap keeps a hostile
/// caller from making the server walk an unbounded list of per-card queries.
pub const SELECTION_MOVE_MAX: usize = 100;

/// Undo a whole batch move from its toast — one call, **one transaction**
/// (`CollectionStore::undo_moves`).
///
/// A batch writes one `moves` row per item and the ledger has no batch id, so
/// the single Undo the tray offers has N rows to reverse. Firing N single-move
/// undos would be N transactions, and a failure part-way would leave the batch
/// half-reverted behind a toast that already claimed it was undone — the shape
/// of the defect this repo has hit before. Idempotent per move, so a
/// double-clicked toast is harmless.
#[server(
    prefix = "/api",
    endpoint = "undo_selection_move",
    input = leptos::server_fn::codec::Json
)]
pub async fn undo_selection_move(move_ids: Vec<shared::Id>) -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .undo_moves(move_ids)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = move_ids;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The destination ranking for a whole selection: `suggested_destinations` per
/// card, folded into one shortfall-ordered list
/// (`my::move_selection::merge_suggestions`).
///
/// The loop lives here because the trait's read is per-oracle by contract
/// (collection-api's "suggested-destinations… for the card") while the tray
/// picks one destination for many cards. It is a fold of reads, not new policy.
#[server(
    prefix = "/api",
    endpoint = "selection_destinations",
    input = leptos::server_fn::codec::Json
)]
pub async fn selection_destinations(
    oracle_ids: Vec<shared::Id>,
) -> Result<Vec<shared::SuggestedDestination>, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        let backend = collection_backend().await?;
        let mut per_card = Vec::new();
        for oracle_id in oracle_ids.into_iter().take(SELECTION_MOVE_MAX) {
            per_card.push(
                backend
                    .suggested_destinations(oracle_id)
                    .await
                    .map_err(api_err)?,
            );
        }
        Ok(crate::my::move_selection::merge_suggestions(per_card))
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = oracle_ids;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// A collection's needs (specs/app-ui.md → `/my/collections/:id/needs`): the
/// cards it wants more copies of than it holds, each split into the part
/// fillable from the caller's other collections and the part still to buy.
///
/// GET per the read-adapter exemplar, so the page SSRs complete markup on a
/// shared URL. Deliberately unpaged — collection-api's Findings settle needs and
/// the shopping list as full lists ("derived and bounded in practice"), and a
/// bucket total that described only the visible page would be a lie the whole
/// feature is built on.
#[server(
    prefix = "/api",
    endpoint = "collection_needs",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn collection_needs(
    collection_id: shared::Id,
) -> Result<shared::NeedsView, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .needs(collection_id)
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = collection_id;
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// The global shopping list (specs/app-ui.md → `/my/shopping`). Named
/// `shopping_list_view` because `CollectionStore::shopping_list` is the trait
/// method this projects and the hosted JSON route `/api/shopping-list` is its
/// machine form; three things called the same thing in one crate is how a call
/// site ends up on the wrong one.
#[server(
    prefix = "/api",
    endpoint = "shopping_list_view",
    input = leptos::server_fn::codec::GetUrl
)]
pub async fn shopping_list_view() -> Result<shared::ShoppingList, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        collection_backend()
            .await?
            .shopping_list()
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// **Pull** — fill a collection's needs from the caller's other collections
/// (specs/app-ui.md → `/my/collections/:id/needs`: "a one-tap Pull (pre-filled
/// move)"; "Pull all generates a pick list… checking records the move").
///
/// **No new backend read was invented for this.** The pick list is composed
/// here, out of two reads that already exist: `needs` supplies each row's gap
/// and the `locations` holding fillable copies, and `holdings_of_oracle`
/// supplies the ungrouped stacks a write is addressed at. The write is
/// `move_batch` — one transaction, one ledger row per stack drawn from, so the
/// caller's single Undo (`undo_selection_move`) covers the whole pull.
///
/// (The spec line names `move_cards` + `suggested_destinations` as the
/// composition. `suggested_destinations` ranks *destinations* for a card, and on
/// this page the destination is the page itself — so the read that was actually
/// needed is the source-side one, `holdings_of_oracle`. Recorded rather than
/// bent to fit.)
///
/// **Quantity is not the caller's**, the same rule [`move_selection`] follows
/// and for a sharper reason: a pull's count is the *gap*, which is a fact about
/// the database at write time, not a number a page rendered some minutes ago.
/// So the server re-reads `needs`, re-runs the very allocation function the
/// checklist was rendered from (`my::needs::allocate`), and takes from the
/// client only *which* (card, source) lines to apply. A line whose need has
/// since closed is refused by name rather than moved.
///
/// `input = Json` because the argument is a list of structs — the POST default
/// is URL-encoded and flattens nested DTOs to strings.
#[server(
    prefix = "/api",
    endpoint = "pull_needs",
    input = leptos::server_fn::codec::Json
)]
pub async fn pull_needs(
    to_collection_id: shared::Id,
    items: Vec<crate::my::needs::PullItem>,
) -> Result<crate::my::needs::PullOutcome, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        use crate::my::move_selection::{SkipReason, Skipped};
        use crate::my::needs::{allocate, dedupe, gap_of, plan_pull, PullOutcome, Pulled};
        use std::collections::hash_map::Entry;
        use std::collections::HashMap;

        if items.len() > SELECTION_MOVE_MAX {
            return Err(ServerFnError::ServerError(format!(
                "a pull can carry at most {SELECTION_MOVE_MAX} lines"
            )));
        }
        let backend = collection_backend().await?;

        // The plan, re-derived: the same allocation the checklist rendered, but
        // over *this* transaction's reading of the gap.
        let needs = backend.needs(to_collection_id).await.map_err(api_err)?;
        let mut planned: HashMap<(shared::Id, shared::Id), i32> = HashMap::new();
        for row in &needs.rows {
            for (from, copies) in allocate(gap_of(row), &row.locations) {
                planned.insert((row.oracle_id, from), copies);
            }
        }

        let mut pulled: Vec<Pulled> = Vec::new();
        let mut skipped: Vec<Skipped> = Vec::new();
        let mut lines: Vec<shared::MoveItem> = Vec::new();
        // One holdings read per distinct card: a pull of the same card from two
        // collections is two lines off one read.
        let mut owned: HashMap<shared::Id, Vec<shared::HoldingLine>> = HashMap::new();
        // One line per (card, source). `planned` is a fixed per-pair number and
        // `owned` is not consumed between lines, so a repeated item would plan
        // the whole gap twice and move it twice — a quantity supplied by
        // repetition through an API that takes none. The cap above is checked
        // against what the caller *sent*, so duplicates cannot buy headroom.
        for item in dedupe(items) {
            let token = item.token();
            if item.from_collection_id == to_collection_id {
                skipped.push(Skipped {
                    token,
                    reason: SkipReason::AlreadyThere,
                });
                continue;
            }
            let Some(&want) = planned.get(&(item.oracle_id, item.from_collection_id)) else {
                skipped.push(Skipped {
                    token,
                    reason: SkipReason::NoLongerNeeded,
                });
                continue;
            };
            let holdings = match owned.entry(item.oracle_id) {
                Entry::Occupied(seen) => seen.into_mut(),
                Entry::Vacant(slot) => {
                    // Session-scoped and RLS-filtered — `holdings_of_oracle`
                    // can only return the caller's own rows, which is what
                    // makes trusting the client's oracle id safe here (a wrong
                    // one plans nothing and is refused).
                    let rows = backend
                        .holdings_of_oracle(item.oracle_id)
                        .await
                        .map_err(api_err)?;
                    slot.insert(rows)
                }
            };
            let plan = plan_pull(holdings, item.from_collection_id, want);
            if plan.is_empty() {
                skipped.push(Skipped {
                    token,
                    reason: SkipReason::NoCopies,
                });
                continue;
            }
            let copies = plan.iter().map(|l| l.quantity).sum();
            for line in plan {
                lines.push(shared::MoveItem {
                    from_collection_id: Some(line.source.from),
                    printing_id: line.source.printing_id,
                    // The grain and board of the stack actually found, never
                    // restated defaults: a `MoveItem` at the default grain
                    // beside a foil-only stack aims at copies that do not exist
                    // and rolls the whole batch back.
                    finish: line.source.finish,
                    condition: line.source.condition,
                    language: line.source.language,
                    from_board: line.source.board,
                    // Copies pulled *into* a collection are ordinary copies
                    // there; landing them on a board is card-tagging's relabel,
                    // not this move (specs/collection-api.md → moves).
                    to_board: shared::Board::Main,
                    quantity: line.quantity,
                });
            }
            pulled.push(Pulled { token, copies });
        }

        // Nothing to write is not a "successful" move of nothing.
        let move_ids = if lines.is_empty() {
            Vec::new()
        } else {
            match backend
                .move_batch(shared::BatchMove {
                    to_collection_id: Some(to_collection_id),
                    items: lines,
                })
                .await
            {
                Ok(receipts) => receipts.into_iter().map(|r| r.move_id).collect(),
                // One transaction, so this moved nothing at all. The batch
                // failure is tagged with an item index, but a pull's items are
                // stacks rather than cards — the index names a `holdings` row
                // the user never saw, so it is dropped rather than mis-attached
                // to a card name.
                Err(e) => return Err(api_err(e)),
            }
        };

        Ok(PullOutcome {
            move_ids,
            pulled,
            skipped,
        })
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (to_collection_id, items);
        Err(ServerFnError::ServerError("server-only".into()))
    }
}

/// Probe endpoint proving JWT auth end-to-end: verifies the bearer token and
/// echoes the caller's user id. 401 without a valid token. Superseded by real
/// `/my/*` routes once the data model lands; kept minimal until then.
#[cfg(feature = "ssr")]
async fn me(user: crate::auth::AuthUser) -> String {
    user.user_id.to_string()
}

/// Lands the Google sign-in redirect (specs/auth.md → Integration
/// architecture): exchanges the callback's session verifier plus the
/// challenge held in our httpOnly cookie for an upstream session, re-hosts
/// it in our cookies, and bounces to `/`. Any missing piece or upstream
/// refusal bounces to `/login?error=google` — the flow is restartable.
#[cfg(feature = "ssr")]
async fn auth_callback(
    headers: axum::http::HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use crate::auth::{cookies, upstream};
    use axum::http::header::{LOCATION, SET_COOKIE};
    use axum::http::StatusCode;

    let origin = cookies::request_origin(&headers);
    let secure = cookies::request_is_secure(&headers);
    let native = crate::auth::native::embedded_origin().is_some();

    // On the web the challenge rides our httpOnly cookie; under a Tauri shell
    // the flow ran in the system browser (which has no webview cookies), so
    // the embedded server holds it in memory instead.
    let challenge = cookies::cookie_value(&headers, cookies::CHALLENGE_COOKIE)
        .or_else(crate::auth::native::take_challenge);

    let session = match (params.get(upstream::SESSION_VERIFIER_PARAM), challenge) {
        (Some(verifier), Some(challenge)) => {
            upstream::social_complete(&origin, verifier, &challenge).await
        }
        _ => Err(upstream::UpstreamError::Http(
            "missing verifier or challenge".into(),
        )),
    };

    let clear_challenge = cookies::clear_cookie(cookies::CHALLENGE_COOKIE, secure);
    match session {
        Ok(session) => match upstream::mint_jwt(&origin, &session.cookie_value).await {
            Ok(jwt) => {
                if native {
                    // The system browser is a bystander here: park the session
                    // for the webview's `current_user` poll to claim, and tell
                    // the human to head back to the app.
                    crate::auth::native::stash_session(session);
                    return axum::http::Response::builder()
                        .status(StatusCode::OK)
                        .header(
                            axum::http::header::CONTENT_TYPE,
                            "text/html; charset=utf-8",
                        )
                        .header(SET_COOKIE, clear_challenge)
                        .body(axum::body::Body::from(
                            "<!DOCTYPE html><html><body style=\"background:#1a2332;color:#fff;\
                             font-family:sans-serif;display:grid;place-items:center;height:100vh\">\
                             <p>Signed in \u{2014} you can close this tab and return to Three Rings.</p>\
                             </body></html>",
                        ))
                        .expect("static page construction cannot fail");
                }
                axum::http::Response::builder()
                    .status(StatusCode::SEE_OTHER)
                    .header(LOCATION, "/")
                    .header(
                        SET_COOKIE,
                        cookies::set_cookie(
                            cookies::SESSION_COOKIE,
                            &session.cookie_value,
                            cookies::SESSION_MAX_AGE,
                            secure,
                        ),
                    )
                    .header(
                        SET_COOKIE,
                        cookies::set_cookie(
                            cookies::JWT_COOKIE,
                            &jwt,
                            cookies::JWT_MAX_AGE,
                            secure,
                        ),
                    )
                    .header(SET_COOKIE, clear_challenge)
                    .body(axum::body::Body::empty())
                    .expect("static redirect construction cannot fail")
            }
            Err(e) => {
                leptos::logging::log!("google callback: token mint failed: {e}");
                google_error_redirect(clear_challenge)
            }
        },
        Err(e) => {
            leptos::logging::log!("google callback: exchange failed: {e}");
            google_error_redirect(clear_challenge)
        }
    }
}

/// The Android return leg of the Google flow (specs/auth.md → Android
/// deep-link return). Android freezes the backgrounded app, so the system
/// browser cannot reach the embedded loopback server the way it does on
/// desktop — the OAuth callback lands here on the *public web origin*
/// instead, and this page hands the verifier back to the app through its
/// `three-rings://` deep link (the scheme is registered in
/// `src-tauri/tauri.conf.json`). The query is forwarded client-side from
/// `location.search`, so nothing user-controlled is interpolated into the
/// page. Auto-navigation to a custom scheme may need a user gesture in
/// Chrome, hence the visible link.
#[cfg(feature = "ssr")]
async fn auth_app_return() -> axum::response::Response {
    axum::http::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::from(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
             <title>Three Rings</title></head>\
             <body style=\"background:#1a2332;color:#fff;font-family:sans-serif;\
             display:grid;place-items:center;height:100vh;text-align:center\">\
             <div><p>Returning to Three Rings\u{2026}</p>\
             <p><a id=\"open\" style=\"color:#8ab4f8\" href=\"three-rings://auth/callback\">\
             Open the app</a></p>\
             <p style=\"opacity:.7\">You can close this tab once the app opens.</p></div>\
             <script>var t=\"three-rings://auth/callback\"+location.search;\
             document.getElementById(\"open\").href=t;location.replace(t);</script>\
             </body></html>",
        ))
        .expect("static page construction cannot fail")
}

/// Bounce a failed Google callback to the login page (flow is restartable).
#[cfg(feature = "ssr")]
fn google_error_redirect(clear_challenge: String) -> axum::response::Response {
    axum::http::Response::builder()
        .status(axum::http::StatusCode::SEE_OTHER)
        .header(axum::http::header::LOCATION, "/login?error=google")
        .header(axum::http::header::SET_COOKIE, clear_challenge)
        .body(axum::body::Body::empty())
        .expect("static redirect construction cannot fail")
}

#[cfg(feature = "ssr")]
pub fn build_router(leptos_options: LeptosOptions) -> axum::Router {
    use axum::routing::get;
    use axum::Router;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use tower_http::cors::{AllowOrigin, CorsLayer};

    let routes = generate_route_list(App);

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            let origin_bytes = origin.as_bytes();
            origin_bytes == b"tauri://localhost"
                || origin_bytes.starts_with(b"http://localhost:")
                || origin_bytes.starts_with(b"http://127.0.0.1:")
        }))
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    let router = Router::new()
        .route("/api/me", get(me))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/app-return", get(auth_app_return));

    // The hosted JSON API the native client calls (specs/data-access-backends.md).
    // Only the web deployment (the authorization terminus) mounts these; the
    // native embedded server has no `HostedBackend`, so it never serves them.
    #[cfg(feature = "hosted")]
    let router = crate::backend::routes::mount(router);

    router
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .layer(cors)
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options)
}
