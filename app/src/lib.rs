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
                <Route path=StaticSegment("") view=shell::MyCardsPage ssr=SsrMode::Async />
                <Route
                    path=(StaticSegment("collections"), ParamSegment("id"))
                    view=shell::CollectionPage
                    ssr=SsrMode::Async
                />
                <Route
                    path=(StaticSegment("collections"), ParamSegment("id"), StaticSegment("needs"))
                    view=shell::NeedsPage
                    ssr=SsrMode::Async
                />
                <Route
                    path=StaticSegment("shopping")
                    view=shell::ShoppingPage
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
        collection_backend().await?.list_collections().await.map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
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
    Ok(crate::backend::NativeBackend::authed(token, session, origin))
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
/// session POST `quantity: 20`, a printing-pinned Want, or a non-default board
/// at an endpoint whose entire contract is "one copy, default grain". That is
/// not a privilege escalation — the same caller can already reach
/// `POST /api/collections/{id}/have` with any quantity on their *own*
/// collections — but an adapter whose wire contract is wider than its name is
/// a trap for the next caller. Quantity 1 is now true by construction.
#[server(prefix = "/api", endpoint = "quick_add")]
pub async fn quick_add(
    collection_id: shared::Id,
    kind: shared::QuickAddKind,
    oracle_id: shared::Id,
    printing_id: Option<shared::Id>,
) -> Result<shared::QuickAddReceipt, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        use crate::backend::CollectionStore;
        let backend = collection_backend().await?;
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
                        quantity: 1,
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
                            quantity: 1,
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
        let _ = (collection_id, kind, oracle_id, printing_id);
        Err(ServerFnError::ServerError("server-only".into()))
    }
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
