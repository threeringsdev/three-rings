use leptos::logging::log;
use leptos::prelude::*;

#[tokio::main]
async fn main() {
    if std::env::var("LEPTOS_OUTPUT_NAME").is_err() {
        std::env::set_var("LEPTOS_OUTPUT_NAME", "app");
    }

    // Deploy-time migration step: `server --migrate` runs pending migrations as
    // the owner/migration role and exits, so the serving process below can run
    // as a non-owner role with no DDL rights (specs/data-model.md → Migration
    // plan). Wire it as a Render pre-deploy command.
    if std::env::args().any(|arg| arg == "--migrate") {
        match app::db::migrate().await {
            Ok(()) => {
                log!("migrations: up to date");
                return;
            }
            Err(e) => {
                log!("migrations FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;

    let app = app::build_router(leptos_options);

    // Spike probe (architecture-spike task 5): prove Neon/sqlx connectivity
    // from the server path at startup. Non-fatal so the web demo still runs
    // without a DATABASE_URL.
    match app::db::card_count().await {
        Ok(n) => log!("neon: connected, cards table has {n} rows"),
        Err(e) => log!("neon: connectivity probe FAILED: {e}"),
    }

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
