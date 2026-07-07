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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
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
            std::env::set_var("STORAGE_PATH", app_data_dir.to_string_lossy().to_string());

            #[cfg(not(debug_assertions))]
            {
                use leptos::prelude::get_configuration;
                use tauri::Manager;

                if std::env::var("LEPTOS_OUTPUT_NAME").is_err() {
                    std::env::set_var("LEPTOS_OUTPUT_NAME", "app");
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

                let router = app::build_router(conf.leptos_options);

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

                #[cfg(not(target_os = "android"))]
                {
                    let window = app.get_webview_window("main").ok_or_else(|| {
                        Box::<dyn std::error::Error>::from("Failed to get main window")
                    })?;
                    let url =
                        tauri::Url::parse(&format!("http://127.0.0.1:{}", port)).map_err(|e| {
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
            // Android release: the initial page is the asset protocol's
            // "asset not found: index.html" (SSR ships no index.html). Once that
            // load finishes the webview provably exists, so redirect it to the
            // in-process server. Guarded so it fires only for non-server URLs.
            #[cfg(all(not(debug_assertions), target_os = "android"))]
            {
                use tauri::Manager;
                if matches!(_payload.event(), tauri::webview::PageLoadEvent::Finished)
                    && _payload.url().host_str() != Some("127.0.0.1")
                {
                    if let Some(port) = _webview.try_state::<ServerPort>() {
                        if let Ok(url) = format!("http://127.0.0.1:{}", port.0).parse() {
                            let _ = _webview.navigate(url);
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
