/// The launch splash the window paints while the first real page is still
/// being server-rendered. Only release builds run the embedded server that
/// serves it (debug rides `devUrl`), so it is compiled for release — plus
/// `cfg(test)`, which is a debug build, so its unit tests still run.
#[cfg(any(not(debug_assertions), test))]
mod splash;

/// Holds the in-process Axum server task handle so it can be
/// gracefully aborted when the window is closed.
///
/// To debug the release build:
/// `cargo tauri build -vv`
/// Then go to /Applications -> Show Package Contents -> Contents -> MacOS -> run the binary
struct ServerTask(tauri::async_runtime::JoinHandle<()>);

/// Port of the in-process Axum server. On Android the webview is created
/// asynchronously, so navigation happens from `on_page_load` (see below),
/// which reads the port from managed state.
#[cfg(all(not(debug_assertions), target_os = "android"))]
struct ServerPort(u16);

/// Open a URL in the system browser (Google OAuth hand-off — the webview
/// itself is refused by Google). Invoked from the wasm frontend via
/// `window.__TAURI__.core.invoke("open_url", …)`; the opener plugin's Rust
/// API routes to `open` on desktop and an Intent on Android.
#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("refusing to open a non-http(s) url".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            use tauri::Manager;
            let app_data_dir = app.path().app_local_data_dir().map_err(|e| {
                Box::<dyn std::error::Error>::from(format!(
                    "Failed to get app local data directory: {}",
                    e
                ))
            })?;
            if !app_data_dir.exists() {
                std::fs::create_dir_all(&app_data_dir).map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!(
                        "Failed to create app local data directory: {}",
                        e
                    ))
                })?;
            }
            #[cfg(not(debug_assertions))]
            {
                use leptos::prelude::get_configuration;
                use tauri::Manager;

                if std::env::var("LEPTOS_OUTPUT_NAME").is_err() {
                    std::env::set_var("LEPTOS_OUTPUT_NAME", "app");
                }

                // Packaged release apps (Finder-launched .app, Android APK)
                // receive no shell environment, so default the auth service to
                // the production branch (non-secret, specs/auth.md). Exported
                // env still wins — terminal launches can point at dev.
                if std::env::var("NEON_AUTH_BASE_URL").is_err() {
                    std::env::set_var(
                        "NEON_AUTH_BASE_URL",
                        "https://ep-curly-pond-atsb6fgp.neonauth.c-9.us-east-1.aws.neon.tech/neondb/auth",
                    );
                }

                // Android bundles resources inside the APK (resource_dir() is the
                // non-filesystem URI asset://localhost/), so Axum cannot serve them
                // via std::fs. Extract the embedded frontend assets into the app
                // data dir and configure Leptos from env vars instead of a
                // Cargo.toml on disk. Re-extracted on every launch; revisit with a
                // version check before real releases.
                #[cfg(target_os = "android")]
                let conf = {
                    let site_root = app_data_dir.join("site");
                    let resolver = app.asset_resolver();
                    for (path, _) in resolver.iter() {
                        let path = path.to_string();
                        let asset = resolver.get(path.clone()).ok_or_else(|| {
                            Box::<dyn std::error::Error>::from(format!(
                                "Missing embedded asset: {path}"
                            ))
                        })?;
                        let dest = site_root.join(path.trim_start_matches('/'));
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&dest, &asset.bytes).map_err(|e| {
                            Box::<dyn std::error::Error>::from(format!(
                                "Failed to extract asset to {}: {}",
                                dest.display(),
                                e
                            ))
                        })?;
                    }

                    std::env::set_var("LEPTOS_ENV", "PROD");
                    std::env::set_var("LEPTOS_SITE_ROOT", site_root.to_string_lossy().to_string());

                    let mut conf = get_configuration(None).map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!(
                            "Failed to load leptos configuration: {}",
                            e
                        ))
                    })?;
                    conf.leptos_options.site_root = site_root.to_string_lossy().to_string().into();
                    conf
                };

                #[cfg(not(target_os = "android"))]
                let conf = {
                    let resource_dir = app.path().resource_dir().map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!(
                            "Failed to get resource directory: {}",
                            e
                        ))
                    })?;
                    let site_root = resource_dir.join("site");
                    let cargo_toml_path = resource_dir.join("Cargo.toml");

                    std::env::set_var("LEPTOS_SITE_ROOT", site_root.to_string_lossy().to_string());

                    let cargo_toml_str = cargo_toml_path.to_str().ok_or_else(|| {
                        Box::<dyn std::error::Error>::from("Cargo.toml path is not valid UTF-8")
                    })?;
                    let mut conf = get_configuration(Some(cargo_toml_str)).map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!(
                            "Failed to load leptos configuration: {}",
                            e
                        ))
                    })?;
                    conf.leptos_options.site_root = site_root.to_string_lossy().to_string().into();
                    conf
                };

                // Must run inside the Tokio runtime context, hence the
                // `block_on` around what is otherwise synchronous code.
                //
                // `build_router` calls `generate_route_list(App)`, which *renders*
                // the app to enumerate its routes. That render runs `App`, which
                // provides the shared current-user `Resource`, and creating a
                // Resource spawns onto Leptos's global executor — `tokio::spawn`
                // (any_spawner 0.3, `Executor::init_tokio`). `tokio::spawn`
                // panics without a runtime in scope.
                //
                // The web server never notices: `server/src/main.rs` is
                // `#[tokio::main]`, so its whole body already has a runtime. Here
                // the setup hook runs on the main thread, and on macOS it is
                // called from inside the ObjC `applicationDidFinishLaunching`
                // callback — so the panic cannot unwind across the FFI boundary
                // and aborts the process instead, with the message going to
                // stderr where a Finder-launched app discards it. That is what
                // made this a silent "app won't open" on macOS and a
                // `panic_cannot_unwind` abort on Android, from one root cause.
                let router =
                    tauri::async_runtime::block_on(async move { app::build_router(conf.leptos_options) });

                // The one route the shell adds to the app's router: the launch
                // splash the window is pointed at first (see `splash`). Every
                // platform navigates there — desktop from the `setup` hook
                // below, Android from `on_page_load` (its webview does not exist
                // yet at this point).
                let router = splash::mount(router);

                let (port, listener) = tauri::async_runtime::block_on(async {
                    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                        Ok(l) => l,
                        Err(_) => tokio::net::TcpListener::bind("[::1]:0")
                            .await
                            .map_err(|e| {
                                Box::<dyn std::error::Error>::from(format!(
                                    "Failed to bind tcp listener: {}",
                                    e
                                ))
                            })?,
                    };
                    let port = listener
                        .local_addr()
                        .map_err(|e| {
                            Box::<dyn std::error::Error>::from(format!(
                                "Failed to get local addr: {}",
                                e
                            ))
                        })?
                        .port();
                    Ok::<_, Box<dyn std::error::Error>>((port, listener))
                })?;

                // Tell the app crate it runs as a single-user embedded server:
                // enables the system-browser Google flow (localhost callback +
                // in-memory challenge/session handoff — app/src/auth/native.rs)
                // and, since WB-01M036CA3M185WM4WGS5SDC161, is what
                // `cookies::request_origin` presents to Neon Auth for *every*
                // account:: call (sign-in/up, OTP, reset, …), not just Google.
                // `localhost`, not `127.0.0.1`: the auth service only trusts
                // the former, never the latter, in either an `Origin` header
                // or a callback URL (verified live against both Neon Auth
                // branches — specs/auth.md).
                let embedded_origin = format!("http://localhost:{}", port);
                std::env::set_var("TR_EMBEDDED_ORIGIN", &embedded_origin);

                let server_task = tauri::async_runtime::spawn(async move {
                    let _ = axum::serve(listener, router.into_make_service()).await;
                });
                app.manage(ServerTask(server_task));

                // Wait for the server to be ready before navigating
                tauri::async_runtime::block_on(async {
                    let addr = format!("127.0.0.1:{}", port);
                    for _ in 0..50 {
                        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                });

                // Android's webview is created asynchronously: a navigate() issued
                // here races the webview's initial asset-protocol load and loses.
                // Stash the port and navigate from on_page_load instead.
                #[cfg(target_os = "android")]
                app.manage(ServerPort(port));

                // Android's Google flow returns through the three-rings://
                // deep link (a frozen backgrounded app can't answer the
                // browser on its loopback port — specs/auth.md): complete
                // the verifier exchange here, in-process, now that the OS
                // has foregrounded us. The webview's current_user polling
                // then claims the parked session. Registered on desktop too
                // (the scheme is in tauri.conf.json), where it is simply
                // never exercised — desktop returns via the loopback server.
                {
                    use tauri_plugin_deep_link::DeepLinkExt;

                    // Shared by both delivery paths below — the plugin's
                    // Android side only pushes through `on_open_url` when a
                    // *running* app receives a new intent; a cold start
                    // (the OS killed the backgrounded app while the system
                    // browser ran Google's flow, then relaunched it via
                    // this very deep link — the exact "frozen app"
                    // scenario above) never fires it, verified live on the
                    // emulator (WB-01M0640EKXM1QCBMFG7K97E4M7: `adb shell am
                    // start -a VIEW -d three-rings://...` against a killed
                    // app produced no `on_open_url` callback at all).
                    fn handle_deep_link_urls(urls: Vec<url::Url>) {
                        for url in urls {
                            let verifier = url
                                .query_pairs()
                                .find(|(k, _)| k == app::auth::upstream::SESSION_VERIFIER_PARAM)
                                .map(|(_, v)| v.into_owned());
                            match verifier {
                                Some(verifier) => {
                                    tauri::async_runtime::spawn(async move {
                                        match app::auth::native::complete_google_return(&verifier)
                                            .await
                                        {
                                            Ok(()) => {
                                                log::info!("google deep-link return: session parked")
                                            }
                                            Err(e) => {
                                                log::error!("google deep-link return failed: {e}")
                                            }
                                        }
                                    });
                                }
                                None => log::warn!("deep link without a session verifier: {url}"),
                            }
                        }
                    }

                    // Path 1: the app is already running and receives the
                    // deep link as a new intent (`onNewIntent` / desktop's
                    // second-instance argv).
                    app.deep_link()
                        .on_open_url(|event| handle_deep_link_urls(event.urls()));

                    // Path 2: the deep link itself launched this process —
                    // the plugin's own docs: "Use get_current on app load to
                    // check whether your app was started via a deep link."
                    // Without this, a killed-then-relaunched app silently
                    // drops the Google return and the webview polls forever.
                    match app.deep_link().get_current() {
                        Ok(Some(urls)) => handle_deep_link_urls(urls),
                        Ok(None) => {}
                        Err(e) => log::warn!("deep_link get_current failed: {e}"),
                    }
                }

                #[cfg(not(target_os = "android"))]
                {
                    // Navigate the *window itself* to the same loopback origin
                    // just exported as `TR_EMBEDDED_ORIGIN`, not the raw
                    // `127.0.0.1` bind address: the webview's `Host` header is
                    // what `cookies::request_origin` reads for every
                    // account:: call, and Neon Auth trusts `localhost`, never
                    // `127.0.0.1` (WB-01M036CA3M185WM4WGS5SDC161 — the release
                    // window used to navigate to the raw bind address, so
                    // password/OTP/reset calls carried an untrusted Origin
                    // even though `TR_EMBEDDED_ORIGIN` already claimed
                    // `localhost`). `localhost` resolves straight back to the
                    // `127.0.0.1` listener — the same resolution `cargo tauri
                    // dev`'s `devUrl` already relies on — so this changes only
                    // which `Host`/`Origin` requests carry, not where they land.
                    let window = app.get_webview_window("main").ok_or_else(|| {
                        Box::<dyn std::error::Error>::from("Failed to get main window")
                    })?;
                    // …and to the *splash* on that origin, not straight to
                    // `/`. Every top-level route is `SsrMode::Async`, so `/`
                    // sends no HTML until `fetch_current_user` has been out to
                    // Neon Auth and `/my` out to the hosted API — 1–2s with
                    // everything warm, 15s measured against a cold hosted
                    // container — and a WKWebView paints nothing at all while a
                    // navigation is provisional. The splash answers off the
                    // loopback interface with no awaits, then hands the webview
                    // on to `/` once it has painted; the webview keeps showing
                    // it until the real response commits. See `splash`.
                    let splash_url = format!("{embedded_origin}{}", splash::PATH);
                    let url = tauri::Url::parse(&splash_url).map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!(
                            "Failed to parse URL: {}",
                            e
                        ))
                    })?;
                    window.navigate(url).map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!(
                            "Failed to navigate window: {}",
                            e
                        ))
                    })?;
                }
            }
            let _ = app;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                println!("Window close requested, cleaning up...");
                use tauri::Manager;
                if let Some(task) = window.try_state::<ServerTask>() {
                    task.0.abort();
                    println!("Axum server task aborted successfully.");
                }
            }
        })
        .on_page_load(|_webview, _payload| {
            // Android release: the initial page is whatever the *asset protocol*
            // serves at its root, which no Rust code gets to steer — the webview
            // is created before `setup` can navigate it. That used to be the
            // protocol's own "asset not found: index.html" error page (SSR emits
            // no index.html), which is the launch defect in WB-01M0DT7YTF; the
            // bundled `src-tauri/launch-placeholder.html` now occupies that slot
            // (tauri.conf.json's `beforeBuildCommand` copies it into
            // `frontendDist`). Either way, once that first load *finishes* the
            // webview provably exists, so this is where the shell takes over.
            //
            // Destination: the splash, not `/`. `/` is remote-blocked —
            // `SsrMode::Async` sends no HTML until Neon Auth and the hosted API
            // have answered (1 s warm, 15 s measured cold;
            // specs/architecture-spike.md) — and the webview keeps painting the
            // *current* document for that whole time. Sending it to `/__loading`
            // first means the wait happens behind the same branded loading state
            // desktop gets, and the splash's own `location.replace("/")` carries
            // it on once it has painted.
            //
            // The loop guard is the host, and it holds across all three loads:
            // the placeholder is served over the asset protocol
            // (`http://tauri.localhost/…`) so it navigates; `/__loading` and
            // everything the splash hands off to are on `127.0.0.1`, so they do
            // not. Keep the host as the raw bind address — desktop's `localhost`
            // rewrite (see the navigate in `setup`) is deliberately not applied
            // here, and this guard depends on the literal it produces.
            #[cfg(all(not(debug_assertions), target_os = "android"))]
            {
                use tauri::Manager;
                if matches!(_payload.event(), tauri::webview::PageLoadEvent::Finished)
                    && _payload.url().host_str() != Some("127.0.0.1")
                {
                    if let Some(port) = _webview.try_state::<ServerPort>() {
                        if let Ok(url) =
                            format!("http://127.0.0.1:{}{}", port.0, splash::PATH).parse()
                        {
                            let _ = _webview.navigate(url);
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![open_url])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
