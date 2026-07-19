use leptos::children::ToChildren;
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes, RoutesProps},
    StaticSegment,
};

#[cfg(feature = "hydrate")]
use web_sys::window;

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

    // Route definitions are composed as a plain tuple (what the view! macro
    // builds from <Routes> children anyway) so the bench route can be
    // feature-gated: cfg on a node inside view! has no way to express
    // "no route here", and Routes is fed through its props builder because
    // the macro only accepts literal <Route> nodes as its children.
    let routes = view! {
        <Route path=StaticSegment("") view=HomePage />
        <Route path=StaticSegment("cards") view=CardsPage />
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

        // sets the document title
        <Title text="Welcome to Three Rings" />

        // content for this welcome page
        <Router>
            <main>
                {Routes(
                    RoutesProps::builder()
                        .fallback(|| "Page not found.".into_view())
                        .children(ToChildren::to_children(move || routes))
                        .build(),
                )}
            </main>
        </Router>
    }
}

/// Renders the home page of your application.
#[component]
fn HomePage() -> impl IntoView {
    let increment_action = ServerAction::<IncrementCount>::new();

    // Local optimistic count state
    let (optimistic_count, set_optimistic_count) = signal(None::<u32>);

    // Server count resource
    let count = Resource::new(move || increment_action.version().get(), |_| get_count());

    // Initialize from localStorage or server
    Effect::new(move |_| {
        if optimistic_count.get().is_none() {
            // Try to get from localStorage first
            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = window() {
                    if let Ok(Some(storage)) = window.local_storage() {
                        if let Ok(Some(cached_count_str)) = storage.get_item("spin_counter_count") {
                            if let Ok(cached_count) = cached_count_str.parse::<u32>() {
                                set_optimistic_count.set(Some(cached_count));
                                return;
                            }
                        }
                    }
                }
            }

            // Fallback to server count
            if let Some(Ok(server_count)) = count.get() {
                set_optimistic_count.set(Some(server_count));

                // Cache in localStorage
                #[cfg(feature = "hydrate")]
                {
                    if let Some(window) = window() {
                        if let Ok(Some(storage)) = window.local_storage() {
                            let _ =
                                storage.set_item("spin_counter_count", &server_count.to_string());
                        }
                    }
                }
            }
        }
    });

    // Sync server updates to localStorage
    Effect::new(move |_| {
        if let Some(Ok(server_count)) = count.get() {
            // Only update if we have a successful server response and it's different
            if let Some(current_optimistic) = optimistic_count.get() {
                if server_count != current_optimistic {
                    set_optimistic_count.set(Some(server_count));
                }
            }

            // Always update localStorage with server value
            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = window() {
                    if let Ok(Some(storage)) = window.local_storage() {
                        let _ = storage.set_item("spin_counter_count", &server_count.to_string());
                    }
                }
            }
        }
    });

    // Optimistic increment
    let on_click = move |_| {
        // Immediately update UI
        let new_count = optimistic_count.get().unwrap_or(0) + 1;
        set_optimistic_count.set(Some(new_count));

        // Update localStorage immediately
        #[cfg(feature = "hydrate")]
        {
            if let Some(window) = window() {
                if let Ok(Some(storage)) = window.local_storage() {
                    let _ = storage.set_item("spin_counter_count", &new_count.to_string());
                }
            }
        }

        // Trigger server action
        increment_action.dispatch(IncrementCount {});
    };

    view! {
        <div class="min-h-screen bg-background flex items-center justify-center p-4">
            <div class="bg-card text-card-foreground rounded-xl shadow-2xl p-8 md:p-12 max-w-md w-full border">
                <div class="text-center space-y-8">
                    // Header
                    <div class="space-y-2">
                        <div class="flex items-center justify-center gap-3 mb-4">
                            // Fermyon-style logo placeholder
                            <div class="w-10 h-10 bg-primary rounded-lg flex items-center justify-center">
                                <span class="text-primary-foreground font-bold text-xl">C</span>
                            </div>
                            <h1 class="text-3xl md:text-4xl font-medium">
                                "Three Rings"
                            </h1>
                        </div>
                        <p class="text-muted-foreground text-sm">"Powered by Leptos + WASM"</p>
                    </div>

                    // Counter Display
                    <div class="relative">
                        <div class="bg-background rounded-lg p-8 border">
                            <div class="text-5xl md:text-6xl font-light tabular-nums">
                                {move || {
                                    optimistic_count
                                        .get()
                                        .map(|c| c.to_string())
                                        .unwrap_or_else(|| "...".to_string())
                                }}
                            </div>
                            <div class="text-muted-foreground text-sm mt-2 uppercase tracking-wider">
                                "Count Value"
                            </div>
                        </div>

                        // Loading indicator overlay
                        <Show when=move || increment_action.pending().get()>
                            <div class="absolute inset-0 flex items-center justify-center bg-background/50 rounded-lg">
                                <div class="animate-spin rounded-full h-8 w-8 border-2 border-transparent border-t-primary"></div>
                            </div>
                        </Show>
                    </div>

                    // Button
                    <button
                        on:click=on_click
                        disabled=move || increment_action.pending().get()
                        class="w-full rounded-lg bg-primary px-6 py-3 text-primary-foreground font-medium transition-all duration-200 hover:bg-primary/90 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-primary"
                    >
                        {move || {
                            if increment_action.pending().get() {
                                "Updating..."
                            } else {
                                "Increment Counter"
                            }
                        }}
                    </button>

                    // Status indicators
                    <div class="flex items-center justify-center gap-2 text-xs">
                        <div class=move || {
                            if optimistic_count.get().is_none() {
                                "w-2 h-2 rounded-full bg-yellow-500 animate-pulse"
                            } else if increment_action.pending().get() {
                                "w-2 h-2 rounded-full bg-primary animate-pulse"
                            } else {
                                "w-2 h-2 rounded-full bg-primary"
                            }
                        }></div>
                        <span class="text-muted-foreground uppercase tracking-wider">
                            {move || {
                                if optimistic_count.get().is_none() {
                                    "Loading"
                                } else if increment_action.pending().get() {
                                    "Syncing"
                                } else {
                                    "Ready"
                                }
                            }}
                        </span>
                    </div>

                    // Footer info
                    <div class="pt-4 border-t space-y-1">
                        // Interim toggle mount so the persisted override is
                        // reachable in production before the app shell lands
                        // (the shell task owns the permanent placement and
                        // deletes this page).
                        <div class="flex justify-center">
                            <components::ui::theme_toggle::ThemeToggle />
                        </div>
                        <p class="text-muted-foreground text-xs">"Running in Tauri WebView"</p>
                        <a class="text-muted-foreground text-xs underline" href="/cards">
                            "View the catalog →"
                        </a>
                        <auth_pages::AuthStatus />
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Catalog status page: proves the data-access seam end to end — the browser
/// calls the `catalog_count` server fn, which routes through the `CatalogStore`
/// trait to Neon (hosted) or the hosted API (native), SSR then hydrated. Shows
/// "0 cards" until catalog-ingestion populates the catalog.
#[component]
fn CardsPage() -> impl IntoView {
    let count = Resource::new(|| (), |_| catalog_count());

    view! {
        <div class="mx-auto max-w-md p-8">
            <h1 class="mb-4 text-2xl font-bold">"Catalog"</h1>
            <Suspense fallback=|| {
                view! { <p class="text-muted-foreground text-sm">"Loading catalog…"</p> }
            }>
                {move || Suspend::new(async move {
                    match count.await {
                        Ok(count) => {
                            view! {
                                <p class="text-sm">
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
            <a class="text-muted-foreground mt-4 inline-block text-sm underline" href="/">
                "← Home"
            </a>
        </div>
    }
}

pub mod account;
pub mod auth_pages;
#[cfg(feature = "component-bench")]
pub mod bench;
pub mod components;

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

#[cfg(feature = "ssr")]
mod storage {
    #[cfg(feature = "spin")]
    pub async fn get(key: &str) -> Result<Option<Vec<u8>>, String> {
        use spin_sdk::key_value::Store;
        let store = Store::open_default()
            .await
            .map_err(|e| format!("Failed to open Spin KV store: {}", e))?;
        store
            .get(key)
            .await
            .map_err(|e| format!("Failed to get from Spin KV: {}", e))
    }

    #[cfg(feature = "spin")]
    pub async fn set(key: &str, value: &[u8]) -> Result<(), String> {
        use spin_sdk::key_value::Store;
        let store = Store::open_default()
            .await
            .map_err(|e| format!("Failed to open Spin KV store: {}", e))?;
        store
            .set(key, value)
            .await
            .map_err(|e| format!("Failed to set in Spin KV: {}", e))
    }

    #[cfg(not(feature = "spin"))]
    pub async fn get(key: &str) -> Result<Option<Vec<u8>>, String> {
        use std::fs;
        use std::path::Path;

        let base_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data".to_string());
        let file_path = format!("{}/{}.txt", base_path, key);
        let path = Path::new(&file_path);

        if !path.exists() {
            return Ok(None);
        }

        fs::read(&file_path)
            .map(Some)
            .map_err(|e| format!("Failed to read file: {}", e))
    }

    #[cfg(not(feature = "spin"))]
    pub async fn set(key: &str, value: &[u8]) -> Result<(), String> {
        use std::fs;
        use std::path::Path;

        let base_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data".to_string());
        let dir_path = Path::new(&base_path);
        if !dir_path.exists() {
            fs::create_dir_all(dir_path)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        let file_path = format!("{}/{}.txt", base_path, key);
        fs::write(&file_path, value).map_err(|e| format!("Failed to write file: {}", e))
    }
}

#[server(prefix = "/api")]
pub async fn get_count() -> Result<u32, ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        match storage::get("counter").await {
            Ok(Some(value)) => {
                let count_str = String::from_utf8(value)
                    .map_err(|e| ServerFnError::ServerError(format!("Invalid UTF-8: {}", e)))?;
                let count = count_str.parse::<u32>().unwrap_or(0);
                println!("Retrieved count: {count}");
                Ok(count)
            }
            Ok(None) => {
                println!("No count found, returning 0");
                Ok(0)
            }
            Err(e) => {
                eprintln!("Error reading counter: {}", e);
                Ok(0)
            }
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError(
            "Server-only function".to_string(),
        ))
    }
}

#[server(prefix = "/api")]
pub async fn increment_count() -> Result<(), ServerFnError<String>> {
    #[cfg(feature = "ssr")]
    {
        let current_count = get_count().await?;
        let new_count = current_count + 1;
        println!("Incrementing count from {current_count} to {new_count}");

        storage::set("counter", new_count.to_string().as_bytes())
            .await
            .map_err(ServerFnError::ServerError)?;

        Ok(())
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError(
            "Server-only function".to_string(),
        ))
    }
}

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
