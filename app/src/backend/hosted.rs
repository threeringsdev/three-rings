//! Hosted (web) backend: in-process sqlx against Neon — the authorization
//! terminus (specs/data-access-backends.md). Holds the `DATABASE_URL` pool
//! (as the non-owner, RLS-subject `app_runtime` role) and runs every
//! session-scoped query inside a transaction that first sets `app.user_id`, so
//! data-model's RLS policies scope the rows even beneath this terminus.

use shared::{ApiError, ApiResult, CatalogCount, CollectionKind, CollectionSummary};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{CatalogStore, CollectionStore};

/// A per-request handle to the hosted database. Cheap to construct — it borrows
/// the process-wide pool. `session` is the authenticated user id for
/// session-scoped calls; `None` for anonymous catalog reads.
pub struct HostedBackend {
    pool: &'static PgPool,
    session: Option<Uuid>,
}

impl HostedBackend {
    /// Anonymous handle — catalog reads only. A [`CollectionStore`] call on this
    /// handle returns `Unauthorized`.
    pub async fn anonymous() -> ApiResult<Self> {
        Ok(Self {
            pool: pool().await?,
            session: None,
        })
    }

    /// Session-scoped handle for `user_id` (the verified `sub` from `AuthUser`).
    pub async fn for_user(user_id: Uuid) -> ApiResult<Self> {
        Ok(Self {
            pool: pool().await?,
            session: Some(user_id),
        })
    }

    /// Open a transaction and pin `app.user_id` to the session user for its
    /// duration, so RLS policies (`current_setting('app.user_id', true)::uuid`)
    /// scope every statement. `set_config(_, _, true)` is the transaction-local
    /// (`SET LOCAL`) form, bound as a parameter so the uuid is never
    /// string-interpolated. Errors `Unauthorized` if the handle has no session.
    async fn scoped_tx(&self) -> ApiResult<Transaction<'static, Postgres>> {
        let user_id = self
            .session
            .ok_or_else(|| ApiError::Unauthorized("no session".into()))?;
        let mut tx = self.pool.begin().await.map_err(upstream)?;
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(upstream)?;
        Ok(tx)
    }
}

impl CatalogStore for HostedBackend {
    async fn card_count(&self) -> ApiResult<CatalogCount> {
        // Public read; catalog RLS is off, so no scoped transaction needed.
        let (cards,): (i64,) = sqlx::query_as("SELECT count(*) FROM cards")
            .fetch_one(self.pool)
            .await
            .map_err(upstream)?;
        Ok(CatalogCount { cards })
    }
}

impl CollectionStore for HostedBackend {
    async fn list_collections(&self) -> ApiResult<Vec<CollectionSummary>> {
        let mut tx = self.scoped_tx().await?;
        let rows: Vec<CollectionRow> = sqlx::query_as(
            "SELECT id, parent_id, kind::text AS kind, name, is_inbox, \
                    position::float8 AS position, format \
             FROM collections ORDER BY position, name",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        rows.into_iter().map(CollectionRow::into_summary).collect()
    }
}

/// A `collections` row as decoded by sqlx. `kind` and `position` are read via
/// SQL casts (`kind::text`, `position::float8`): sqlx without the decimal
/// feature can't decode `numeric`, and the enum decodes cleanly as text.
#[derive(sqlx::FromRow)]
struct CollectionRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    kind: String,
    name: String,
    is_inbox: bool,
    position: f64,
    format: Option<String>,
}

impl CollectionRow {
    fn into_summary(self) -> ApiResult<CollectionSummary> {
        Ok(CollectionSummary {
            id: self.id,
            parent_id: self.parent_id,
            kind: CollectionKind::from_pg(&self.kind).ok_or_else(|| {
                ApiError::Upstream(format!("unknown collection_kind '{}'", self.kind))
            })?,
            name: self.name,
            is_inbox: self.is_inbox,
            position: self.position,
            format: self.format,
        })
    }
}

/// The process-wide Neon pool (as `app_runtime`). Connects on first use; needs
/// `DATABASE_URL`. Maps a connection failure onto `Upstream`.
async fn pool() -> ApiResult<&'static PgPool> {
    crate::db::pool().await.map_err(upstream)
}

/// Map a sqlx error onto the cross-backend error. The full cause is logged
/// server-side; the client sees a generic upstream message (no DB internals).
fn upstream(e: sqlx::Error) -> ApiError {
    leptos::logging::error!("hosted backend db error: {e}");
    ApiError::Upstream("database error".into())
}
