use leptos::logging::log;
use leptos::prelude::*;

/// Render migration versions zero-padded to 4 digits (`10` -> `0010`), matching
/// the `NNNN_description.sql` filename convention, comma-separated.
fn fmt_versions(versions: &[i64]) -> String {
    versions
        .iter()
        .map(|v| format!("{v:04}"))
        .collect::<Vec<_>>()
        .join(", ")
}

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
            Ok(report) => {
                // MIGRATOR::run succeeded (the migration, if any, is
                // committed) but the independent read-back that verifies it
                // failed — not this run's failure. Exit 0: turning a
                // successful migration into a reported FAILED would abort a
                // future Render pre-deploy on a transient read blip.
                if let Some(read_err) = &report.verify_read_failed {
                    // `embedded` is a static, in-memory fact of this binary
                    // (MIGRATOR::iter(), no DB access) — known regardless of
                    // whether the post-run DB read above failed, so the
                    // embedded=N token below is still valid for
                    // scripts/migrate.sh's stale-embed guard even on this path.
                    log!(
                        "migrations: applied OK but the verification read failed: {read_err} — verify manually — embedded={n} host={host}",
                        n = report.embedded.len(),
                        host = report.host
                    );
                    return;
                }
                // Drift in either direction means Ok(()) from MIGRATOR::run
                // does not mean what it looks like it means — fail loudly
                // rather than print a success line (specs/phase-6-probes/
                // P6-059.md: this is the case that used to print "up to
                // date" unconditionally). Defense-in-depth in practice:
                // sqlx's own VersionMissing/VersionMismatch checks inside
                // MIGRATOR::run already refuse to run at all on most of this
                // drift, surfacing instead as the Err(failure) arm below —
                // this arm is what's left if that guard is ever bypassed.
                if !report.missing.is_empty() || !report.unknown.is_empty() {
                    if !report.applied.is_empty() {
                        log!(
                            "migrations: applied {} before the drift below was detected (versions {})",
                            report.applied.len(),
                            fmt_versions(&report.applied)
                        );
                    }
                    if !report.missing.is_empty() {
                        log!(
                            "migrations FAILED: embedded but not applied: {} (host {})",
                            fmt_versions(&report.missing),
                            report.host
                        );
                    }
                    if !report.unknown.is_empty() {
                        log!(
                            "migrations FAILED: applied in the database but not embedded in this binary: {} (host {})",
                            fmt_versions(&report.unknown),
                            report.host
                        );
                    }
                    std::process::exit(1);
                }
                // `embedded=N` is a stable, machine-parseable token (in
                // addition to being readable): scripts/migrate.sh greps it
                // out and compares it against migrations/*.sql on disk to
                // catch the one failure mode this DB-side check is
                // structurally blind to — a build that itself is stale (never
                // re-embedded a newly added .sql file at all), where every
                // field above is internally consistent because the DB and
                // this binary agree with each other while disagreeing with
                // what's actually on disk.
                if report.applied.is_empty() {
                    log!(
                        "migrations: all {n} embedded migrations already present (latest {latest}) — embedded={n} host={host}",
                        n = report.embedded.len(),
                        latest = report
                            .embedded
                            .last()
                            .map(|v| format!("{v:04}"))
                            .unwrap_or_else(|| "none".to_string()),
                        host = report.host
                    );
                } else {
                    log!(
                        "migrations: applied {k} (versions {versions}) — embedded={n} host={host}",
                        k = report.applied.len(),
                        versions = fmt_versions(&report.applied),
                        n = report.embedded.len(),
                        host = report.host
                    );
                }
                return;
            }
            Err(failure) => {
                if failure.applied_before_failure.is_empty() {
                    log!(
                        "migrations FAILED: {} (host {})",
                        failure.error,
                        failure.host
                    );
                } else {
                    log!(
                        "migrations FAILED: {} (host {}; applied before failure: {})",
                        failure.error,
                        failure.host,
                        fmt_versions(&failure.applied_before_failure)
                    );
                }
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

    // Dev seed data (specs/app-ui.md): `server --seed-dev <user-uuid>` builds
    // the test user's collection tree through the real CollectionStore methods
    // against DATABASE_URL (the dev branch) and exits. Invoked via
    // scripts/seed-dev-data.sh, which resolves the e2e user's uuid. Debug
    // builds only — release binaries (Render, artifacts) don't carry this arm.
    #[cfg(debug_assertions)]
    if let Some(i) = args.iter().position(|arg| arg == "--seed-dev") {
        let user_id = match args.get(i + 1).map(|s| s.parse::<uuid::Uuid>()) {
            Some(Ok(id)) => id,
            other => {
                log!("usage: server --seed-dev <user-uuid> (got {other:?})");
                std::process::exit(2);
            }
        };
        match app::seed::run(user_id).await {
            Ok(stats) => {
                log!("seed succeeded: {stats:?}");
                return;
            }
            Err(e) => {
                log!("seed FAILED: {e}");
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
