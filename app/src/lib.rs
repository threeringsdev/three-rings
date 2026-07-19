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
    view! {
        <!DOCTYPE html>
        <html lang="en" class=if dark { "dark" } else { "" }>
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

/// The `tr_theme` cookie override, else the dark default — shared with the
/// toggle so the shell and the component can never disagree.
fn initial_theme_is_dark() -> bool {
    components::ui::theme_toggle::cookie_theme_is_dark()
}

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();
    shell::provide_current_user();

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
            <Route path=StaticSegment("catalog") view=shell::CatalogPage />
            <Route path=(StaticSegment("cards"), ParamSegment("id")) view=shell::CardDetailPage />
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

/// The signed-in caller's collections — the seam-proving session-scoped read
/// (specs/data-access-backends.md). Hosted: verifies the JWT here, then runs the
/// read inside the `SET LOCAL app.user_id` transaction. Native: forwards the
/// `tr_jwt` cookie as `Authorization: Bearer` to the hosted API, which is the
/// authorization terminus. collection-api builds the UI that consumes this.
#[server(prefix = "/api", endpoint = "list_collections")]
pub async fn list_collections() -> Result<Vec<shared::CollectionSummary>, ServerFnError<String>> {
    #[cfg(feature = "hosted")]
    {
        use crate::backend::{CollectionStore, HostedBackend};
        let headers = leptos_axum::extract::<axum::http::HeaderMap>()
            .await
            .map_err(|e| ServerFnError::ServerError(e.to_string()))?;
        let user_id = crate::auth::user_id_from_headers(&headers)
            .await
            .map_err(|e| ServerFnError::ServerError(e.to_string()))?;
        HostedBackend::for_user(user_id)
            .await
            .map_err(api_err)?
            .list_collections()
            .await
            .map_err(api_err)
    }
    #[cfg(all(feature = "native", not(feature = "hosted")))]
    {
        use crate::auth::cookies;
        use crate::backend::{CollectionStore, NativeBackend};
        let headers = leptos_axum::extract::<axum::http::HeaderMap>()
            .await
            .map_err(|e| ServerFnError::ServerError(e.to_string()))?;
        // The native embedded server never verifies the JWT — it forwards it to
        // the hosted terminus, which does. We hand the backend both the current
        // `tr_jwt` (may be absent once the 15-min token expires) and the
        // long-lived `tr_session` + our origin, so a hosted 401 triggers a
        // silent re-mint + one retry rather than surfacing as Unauthorized.
        let token = cookies::cookie_value(&headers, cookies::JWT_COOKIE);
        let session = cookies::cookie_value(&headers, cookies::SESSION_COOKIE);
        let origin = cookies::request_origin(&headers);
        NativeBackend::authed(token, session, origin)
            .list_collections()
            .await
            .map_err(api_err)
    }
    #[cfg(not(feature = "ssr"))]
    {
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
