//! Spike-only direct Neon access from the server path (architecture-spike
//! task 5): one shared pool, embedded migrations run on first use. The
//! Phase 2 data-access-backends spec replaces this direct access with a
//! trait boundary before any real user data.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::OnceCell;

static POOL: OnceCell<PgPool> = OnceCell::const_new();

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../migrations");

/// Shared connection pool; connects and runs migrations on first call.
/// Requires `DATABASE_URL` (see `.devcontainer/.env.example`).
pub async fn pool() -> Result<&'static PgPool, sqlx::Error> {
    POOL.get_or_try_init(|| async {
        let url = std::env::var("DATABASE_URL")
            .map_err(|_| sqlx::Error::Configuration("DATABASE_URL is not set".into()))?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(pool)
    })
    .await
}

/// Connectivity probe: row count of the spike `cards` table.
pub async fn card_count() -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM cards")
        .fetch_one(pool().await?)
        .await?;
    Ok(count)
}
