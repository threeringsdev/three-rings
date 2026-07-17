use leptos::logging::log;
use leptos::prelude::*;

#[tokio::main]
async fn main() {
    // Load a workspace-root .env when present (host-side dev: DATABASE_URL,
    // NEON_AUTH_BASE_URL — see .devcontainer/.env.example). No-op when the
    // file doesn't exist (Render, containers pass real env). dotenvy never
    // overrides variables already set in the environment.
    dotenvy::dotenv().ok();

    if std::env::var("LEPTOS_OUTPUT_NAME").is_err() {
        std::env::set_var("LEPTOS_OUTPUT_NAME", "app");
    }

    // Owner-privileged migration step: `server --migrate` runs pending migrations
    // as the owner/migration role and exits, so the serving process below can run
    // as a non-owner role with no DDL rights (specs/data-model.md → Migration
    // plan). Invoked via scripts/migrate.sh (Option B, free tier); a Render
    // pre-deploy command is the future paid path.
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

    // Catalog ingestion step (specs/catalog-ingestion.md): `server --ingest
    // <poc|bulk>` runs the Scryfall bulk pipeline as the least-privilege
    // `catalog_ingest` role (INGEST_DATABASE_URL) and exits. Invoked via
    // scripts/ingest.sh today; the stage-3 Render cron job reuses this same
    // binary with the `update` mode when the incremental path lands.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|arg| arg == "--ingest") {
        let mode = match args.get(i + 1).map(String::as_str) {
            Some("poc") => app::ingest::Mode::Poc,
            Some("bulk") => app::ingest::Mode::Bulk,
            other => {
                log!("usage: server --ingest <poc|bulk> (got {other:?})");
                std::process::exit(2);
            }
        };
        match app::ingest::run(mode).await {
            Ok(stats) => {
                log!("ingest succeeded: {stats:?}");
                return;
            }
            Err(e) => {
                log!("ingest FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;

    let app = app::build_router(leptos_options);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
