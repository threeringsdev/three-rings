//! Spike-only direct Neon access from the server path (architecture-spike
//! task 5): one shared runtime pool. The Phase 3 data-access-backends spec
//! replaces this direct access with a trait boundary before any real user data.
//!
//! Migrations do **not** run here. They run as a separate deploy step
//! (`server --migrate`, e.g. a Render pre-deploy command) under the
//! owner/migration role, so the long-running server can connect as a
//! non-owner, RLS-subject role that holds no DDL privileges
//! (see specs/data-model.md → Migration plan).

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::OnceCell;

static POOL: OnceCell<PgPool> = OnceCell::const_new();

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../migrations");

/// Shared runtime connection pool; connects on first call. Runs **no**
/// migrations — that is the `migrate()` deploy step's job. Requires
/// `DATABASE_URL` (see `.devcontainer/.env.example`).
pub async fn pool() -> Result<&'static PgPool, sqlx::Error> {
    POOL.get_or_try_init(|| async {
        let url = std::env::var("DATABASE_URL")
            .map_err(|_| sqlx::Error::Configuration("DATABASE_URL is not set".into()))?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;
        Ok(pool)
    })
    .await
}

/// Run pending migrations, then exit. Invoked by the `server --migrate` deploy
/// step (not the serving path), connecting as the owner/migration role via
/// `MIGRATION_DATABASE_URL` (falling back to `DATABASE_URL` for local dev where
/// one credential is used for both). Uses a short-lived one-connection pool.
pub async fn migrate() -> Result<(), sqlx::Error> {
    let url = std::env::var("MIGRATION_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| {
            sqlx::Error::Configuration(
                "neither MIGRATION_DATABASE_URL nor DATABASE_URL is set".into(),
            )
        })?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    MIGRATOR.run(&pool).await?;
    pool.close().await;
    Ok(())
}

/// Connectivity probe: row count of the spike `cards` table.
pub async fn card_count() -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM cards")
        .fetch_one(pool().await?)
        .await?;
    Ok(count)
}
