//! Hosted (web) backend: in-process sqlx against Neon — the authorization
//! terminus (specs/data-access-backends.md). Holds the `DATABASE_URL` pool
//! (as the non-owner, RLS-subject `app_runtime` role) and runs every
//! session-scoped query inside a transaction that first sets `app.user_id`, so
//! data-model's RLS policies scope the rows even beneath this terminus.

use shared::{
    AddHave, AddLine, AddWant, ApiError, ApiResult, Board, CardRow, CatalogCount, CollectionKind,
    CollectionSummary, CollectionView, Condition, DesireLine, Finish, HoldingLine, Id, LineResult,
    NewCollection, Page, Rename, Reorder, Reparent, SetQuantity,
};
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

    /// The session user id, or `Unauthorized`. Used where a value (not just the
    /// GUC) is needed — e.g. the `user_id` column on an INSERT.
    fn session_id(&self) -> ApiResult<Uuid> {
        self.session
            .ok_or_else(|| ApiError::Unauthorized("no session".into()))
    }
}

/// The `collections` projection matching [`CollectionRow`] — `kind`/`position`
/// cast so sqlx (no decimal feature) decodes them.
const COLLECTION_COLS: &str =
    "id, parent_id, kind::text AS kind, name, is_inbox, position::float8 AS position, format";

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
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;

        // Lazily provision the one Inbox on first authed load (idempotent via the
        // `collections_one_inbox` partial unique index).
        sqlx::query(
            "INSERT INTO collections (user_id, kind, name, is_inbox) \
             VALUES ($1, 'binder', 'Inbox', true) \
             ON CONFLICT (user_id) WHERE is_inbox DO NOTHING",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(upstream)?;

        let rows: Vec<CollectionRow> = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS} FROM collections ORDER BY position, name"
        ))
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        rows.into_iter().map(CollectionRow::into_summary).collect()
    }

    async fn create_collection(&self, req: NewCollection) -> ApiResult<CollectionSummary> {
        if req.format.is_some() && req.kind != CollectionKind::Deck {
            return Err(ApiError::Validation("format is deck-only".into()));
        }
        if req.name.trim().is_empty() {
            return Err(ApiError::Validation("name is required".into()));
        }
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;

        // Parent must exist and be owned — RLS makes a non-owned parent invisible,
        // so this EXISTS both validates ownership and rejects a bad id.
        if let Some(parent_id) = req.parent_id {
            let exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM collections WHERE id = $1")
                .bind(parent_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(upstream)?;
            if exists.is_none() {
                return Err(ApiError::NotFound("parent collection".into()));
            }
        }

        // Append after the current siblings (max position + 1).
        let row: CollectionRow = sqlx::query_as(&format!(
            "INSERT INTO collections (user_id, parent_id, kind, name, format, position) \
             VALUES ($1, $2, $3::collection_kind, $4, $5, \
                     COALESCE((SELECT max(position) FROM collections \
                               WHERE parent_id IS NOT DISTINCT FROM $2), 0) + 1) \
             RETURNING {COLLECTION_COLS}"
        ))
        .bind(user_id)
        .bind(req.parent_id)
        .bind(req.kind.to_pg())
        .bind(req.name.trim())
        .bind(req.format)
        .fetch_one(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        row.into_summary()
    }

    async fn rename_collection(&self, id: Id, req: Rename) -> ApiResult<CollectionSummary> {
        if req.name.trim().is_empty() {
            return Err(ApiError::Validation("name is required".into()));
        }
        let mut tx = self.scoped_tx().await?;
        let updated: Option<CollectionRow> = sqlx::query_as(&format!(
            "UPDATE collections SET name = $2 WHERE id = $1 AND NOT is_inbox \
             RETURNING {COLLECTION_COLS}"
        ))
        .bind(id)
        .bind(req.name.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        let summary = match updated {
            Some(row) => row.into_summary()?,
            None => return Err(self.absent_or_inbox(&mut tx, id, "rename").await),
        };
        tx.commit().await.map_err(upstream)?;
        Ok(summary)
    }

    async fn delete_collection(&self, id: Id) -> ApiResult<()> {
        let mut tx = self.scoped_tx().await?;
        let affected = sqlx::query("DELETE FROM collections WHERE id = $1 AND NOT is_inbox")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(upstream)?
            .rows_affected();
        if affected == 0 {
            return Err(self.absent_or_inbox(&mut tx, id, "delete").await);
        }
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn reparent_collection(&self, id: Id, req: Reparent) -> ApiResult<()> {
        let new_parent = req.new_parent_id;
        if new_parent == Some(id) {
            return Err(ApiError::Conflict(
                "a collection cannot be its own parent".into(),
            ));
        }
        let mut tx = self.scoped_tx().await?;

        // Node must exist / be owned.
        let node: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM collections WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(upstream)?;
        if node.is_none() {
            return Err(ApiError::NotFound("collection".into()));
        }

        if let Some(parent_id) = new_parent {
            // Parent must exist / be owned.
            let parent: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM collections WHERE id = $1")
                .bind(parent_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(upstream)?;
            if parent.is_none() {
                return Err(ApiError::NotFound("parent collection".into()));
            }
            // Cycle check: walk the target parent's ancestors; if `id` is among
            // them, moving `id` under it would create a cycle.
            let cycle: Option<(i32,)> = sqlx::query_as(
                "WITH RECURSIVE anc AS ( \
                   SELECT id, parent_id FROM collections WHERE id = $1 \
                   UNION ALL \
                   SELECT c.id, c.parent_id FROM collections c JOIN anc ON c.id = anc.parent_id \
                 ) SELECT 1 FROM anc WHERE id = $2 LIMIT 1",
            )
            .bind(parent_id)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(upstream)?;
            if cycle.is_some() {
                return Err(ApiError::Conflict(
                    "reparent would create a cycle (target is a descendant)".into(),
                ));
            }
        }

        sqlx::query("UPDATE collections SET parent_id = $2 WHERE id = $1")
            .bind(id)
            .bind(new_parent)
            .execute(&mut *tx)
            .await
            .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn reorder_collection(&self, id: Id, req: Reorder) -> ApiResult<()> {
        let mut tx = self.scoped_tx().await?;
        let affected = sqlx::query("UPDATE collections SET position = $2 WHERE id = $1")
            .bind(id)
            .bind(req.position)
            .execute(&mut *tx)
            .await
            .map_err(upstream)?
            .rows_affected();
        if affected == 0 {
            return Err(ApiError::NotFound("collection".into()));
        }
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn add_holding(&self, collection_id: Id, req: AddHave) -> ApiResult<HoldingLine> {
        if req.quantity <= 0 {
            return Err(ApiError::Validation("quantity must be > 0".into()));
        }
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;

        // Upsert the holding (increment on the unique grain), then append the
        // intake move (`from = NULL`). RLS only checks holdings.user_id, so the
        // owned-collection guard above is what stops writing into someone else's
        // collection.
        let row: HoldingRow = sqlx::query_as(&format!(
            "INSERT INTO holdings \
               (user_id, collection_id, printing_id, finish, condition, language, board, quantity) \
             VALUES ($1, $2, $3, $4::card_finish, $5::card_condition, $6, $7::card_board, $8) \
             ON CONFLICT ON CONSTRAINT holdings_uniq \
               DO UPDATE SET quantity = holdings.quantity + EXCLUDED.quantity \
             RETURNING {HOLDING_COLS}"
        ))
        .bind(user_id)
        .bind(collection_id)
        .bind(req.printing_id)
        .bind(req.finish.to_pg())
        .bind(req.condition.to_pg())
        .bind(&req.language)
        .bind(req.board.to_pg())
        .bind(req.quantity)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        sqlx::query(
            "INSERT INTO moves \
               (user_id, printing_id, finish, condition, language, \
                from_collection_id, to_collection_id, quantity) \
             VALUES ($1, $2, $3::card_finish, $4::card_condition, $5, NULL, $6, $7)",
        )
        .bind(user_id)
        .bind(req.printing_id)
        .bind(req.finish.to_pg())
        .bind(req.condition.to_pg())
        .bind(&req.language)
        .bind(collection_id)
        .bind(req.quantity)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        tx.commit().await.map_err(upstream)?;
        row.into_line()
    }

    async fn add_desire(&self, collection_id: Id, req: AddWant) -> ApiResult<DesireLine> {
        if req.quantity <= 0 {
            return Err(ApiError::Validation("quantity must be > 0".into()));
        }
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;

        let row: DesireRow = sqlx::query_as(&format!(
            "INSERT INTO desires (user_id, collection_id, oracle_id, printing_id, board, quantity) \
             VALUES ($1, $2, $3, $4, $5::card_board, $6) \
             ON CONFLICT ON CONSTRAINT desires_uniq \
               DO UPDATE SET quantity = desires.quantity + EXCLUDED.quantity \
             RETURNING {DESIRE_COLS}"
        ))
        .bind(user_id)
        .bind(collection_id)
        .bind(req.oracle_id)
        .bind(req.printing_id)
        .bind(req.board.to_pg())
        .bind(req.quantity)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        tx.commit().await.map_err(upstream)?;
        row.into_line()
    }

    async fn set_holding_quantity(
        &self,
        holding_id: Id,
        req: SetQuantity,
    ) -> ApiResult<Option<HoldingLine>> {
        let mut tx = self.scoped_tx().await?;
        if req.quantity <= 0 {
            let affected = sqlx::query("DELETE FROM holdings WHERE id = $1")
                .bind(holding_id)
                .execute(&mut *tx)
                .await
                .map_err(upstream)?
                .rows_affected();
            if affected == 0 {
                return Err(ApiError::NotFound("holding".into()));
            }
            tx.commit().await.map_err(upstream)?;
            return Ok(None);
        }
        let row: Option<HoldingRow> = sqlx::query_as(&format!(
            "UPDATE holdings SET quantity = $2 WHERE id = $1 RETURNING {HOLDING_COLS}"
        ))
        .bind(holding_id)
        .bind(req.quantity)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        match row {
            Some(r) => {
                tx.commit().await.map_err(upstream)?;
                Ok(Some(r.into_line()?))
            }
            None => Err(ApiError::NotFound("holding".into())),
        }
    }

    async fn batch_add(
        &self,
        collection_id: Id,
        lines: Vec<AddLine>,
    ) -> ApiResult<Vec<LineResult>> {
        // Each line runs in its own transaction (via add_holding/add_desire), so
        // a failure isolates to that line — per-line results, not all-or-nothing.
        let mut results = Vec::with_capacity(lines.len());
        for line in lines {
            let outcome = match line {
                AddLine::Have(h) => self.add_holding(collection_id, h).await.map(|_| ()),
                AddLine::Want(w) => self.add_desire(collection_id, w).await.map(|_| ()),
            };
            results.push(match outcome {
                Ok(()) => LineResult::Ok,
                Err(error) => LineResult::Error { error },
            });
        }
        Ok(results)
    }

    async fn collection_view(&self, id: Id, page: Page) -> ApiResult<CollectionView> {
        let mut tx = self.scoped_tx().await?;

        // Metadata (owned check via RLS) + immediate children.
        let collection: CollectionRow = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS} FROM collections WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("collection".into()))?;
        let children: Vec<CollectionRow> = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS} FROM collections WHERE parent_id = $1 \
             ORDER BY position, name"
        ))
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        // One keyset page of card rows. Aggregates are per (printing, board) in
        // THIS collection; `owned` is the global per-oracle aggregate (the
        // security-invoker `owned_by_card` view, RLS-scoped to the user);
        // `present_rollup` sums holdings in the strict descendant collections.
        let cursor = page.cursor.as_deref().map(decode_cursor).transpose()?;
        let limit = page.limit();
        let base = "WITH RECURSIVE descendants AS ( \
               SELECT id FROM collections WHERE parent_id = $1 \
               UNION ALL \
               SELECT c.id FROM collections c JOIN descendants d ON c.parent_id = d.id \
             ), \
             present AS ( \
               SELECT printing_id, board, sum(quantity)::int AS present \
               FROM holdings WHERE collection_id = $1 GROUP BY printing_id, board \
             ), \
             want AS ( \
               SELECT oracle_id, board, sum(quantity)::int AS desired \
               FROM desires WHERE collection_id = $1 GROUP BY oracle_id, board \
             ), \
             rollup AS ( \
               SELECT printing_id, sum(quantity)::int AS present_rollup \
               FROM holdings WHERE collection_id IN (SELECT id FROM descendants) \
               GROUP BY printing_id \
             ) \
             SELECT p.oracle_id, pr.printing_id, ca.name, s.code AS set_code, \
                    p.collector_number, p.image_uris->>'normal' AS image_uri, \
                    ca.mana_cost, ca.type_line, ca.colors, pr.present, \
                    COALESCE(w.desired, 0) AS desired, COALESCE(o.owned, 0) AS owned, \
                    COALESCE(ro.present_rollup, 0) AS present_rollup, \
                    pr.board::text AS board \
             FROM present pr \
             JOIN printings p ON p.id = pr.printing_id \
             JOIN cards ca ON ca.oracle_id = p.oracle_id \
             LEFT JOIN sets s ON s.id = p.set_id \
             LEFT JOIN owned_by_card o ON o.oracle_id = p.oracle_id \
             LEFT JOIN want w ON w.oracle_id = p.oracle_id AND w.board = pr.board \
             LEFT JOIN rollup ro ON ro.printing_id = pr.printing_id";
        let keyset = if cursor.is_some() {
            " WHERE (ca.name, pr.printing_id, pr.board) > ($2, $3, $4::card_board)"
        } else {
            ""
        };
        let order = if cursor.is_some() {
            " ORDER BY ca.name, pr.printing_id, pr.board LIMIT $5"
        } else {
            " ORDER BY ca.name, pr.printing_id, pr.board LIMIT $2"
        };
        let sql = format!("{base}{keyset}{order}");

        let mut q = sqlx::query_as::<_, CardRowSql>(&sql).bind(id);
        if let Some(c) = &cursor {
            q = q.bind(&c.name).bind(c.printing_id).bind(&c.board);
        }
        // Fetch one extra row to know whether a next page exists without a
        // phantom empty final fetch.
        let mut rows: Vec<CardRowSql> = q
            .bind(limit + 1)
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        let has_more = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = has_more
            .then(|| rows.last().map(CardRowSql::cursor))
            .flatten();
        let cards = rows
            .into_iter()
            .map(CardRowSql::into_row)
            .collect::<ApiResult<Vec<_>>>()?;

        Ok(CollectionView {
            collection: collection.into_summary()?,
            children: children
                .into_iter()
                .map(CollectionRow::into_summary)
                .collect::<ApiResult<Vec<_>>>()?,
            cards,
            next_cursor,
        })
    }
}

impl HostedBackend {
    /// Disambiguate a write that affected no rows: an existing-but-Inbox row is a
    /// `Conflict` (Inbox is protected), an absent/not-owned row is `NotFound`.
    async fn absent_or_inbox(
        &self,
        tx: &mut Transaction<'static, Postgres>,
        id: Id,
        op: &str,
    ) -> ApiError {
        match sqlx::query_as::<_, (bool,)>("SELECT is_inbox FROM collections WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
        {
            Ok(Some((true,))) => ApiError::Conflict(format!("the Inbox cannot be {op}d")),
            Ok(Some((false,))) => ApiError::NotFound("collection".into()),
            Ok(None) => ApiError::NotFound("collection".into()),
            Err(e) => upstream(e),
        }
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

/// The `holdings` projection matching [`HoldingRow`] (enum columns cast to text).
const HOLDING_COLS: &str = "id, collection_id, printing_id, finish::text AS finish, \
     condition::text AS condition, language, board::text AS board, quantity";

/// The `desires` projection matching [`DesireRow`].
const DESIRE_COLS: &str =
    "id, collection_id, oracle_id, printing_id, board::text AS board, quantity";

#[derive(sqlx::FromRow)]
struct HoldingRow {
    id: Uuid,
    collection_id: Uuid,
    printing_id: Uuid,
    finish: String,
    condition: String,
    language: String,
    board: String,
    quantity: i32,
}

impl HoldingRow {
    fn into_line(self) -> ApiResult<HoldingLine> {
        Ok(HoldingLine {
            id: self.id,
            collection_id: self.collection_id,
            printing_id: self.printing_id,
            finish: Finish::from_pg(&self.finish)
                .ok_or_else(|| ApiError::Upstream(format!("bad finish '{}'", self.finish)))?,
            condition: Condition::from_pg(&self.condition)
                .ok_or_else(|| ApiError::Upstream(format!("bad condition '{}'", self.condition)))?,
            language: self.language,
            board: Board::from_pg(&self.board)
                .ok_or_else(|| ApiError::Upstream(format!("bad board '{}'", self.board)))?,
            quantity: self.quantity,
        })
    }
}

#[derive(sqlx::FromRow)]
struct DesireRow {
    id: Uuid,
    collection_id: Uuid,
    oracle_id: Uuid,
    printing_id: Option<Uuid>,
    board: String,
    quantity: i32,
}

impl DesireRow {
    fn into_line(self) -> ApiResult<DesireLine> {
        Ok(DesireLine {
            id: self.id,
            collection_id: self.collection_id,
            oracle_id: self.oracle_id,
            printing_id: self.printing_id,
            board: Board::from_pg(&self.board)
                .ok_or_else(|| ApiError::Upstream(format!("bad board '{}'", self.board)))?,
            quantity: self.quantity,
        })
    }
}

#[derive(sqlx::FromRow)]
struct CardRowSql {
    oracle_id: Uuid,
    printing_id: Uuid,
    name: String,
    set_code: Option<String>,
    collector_number: String,
    image_uri: Option<String>,
    mana_cost: Option<String>,
    type_line: Option<String>,
    colors: Vec<String>,
    present: i32,
    desired: i32,
    owned: i32,
    present_rollup: i32,
    board: String,
}

impl CardRowSql {
    fn cursor(&self) -> String {
        encode_cursor(&CardCursor {
            name: self.name.clone(),
            printing_id: self.printing_id,
            board: self.board.clone(),
        })
    }

    fn into_row(self) -> ApiResult<CardRow> {
        Ok(CardRow {
            oracle_id: self.oracle_id,
            printing_id: self.printing_id,
            name: self.name,
            set_code: self.set_code,
            collector_number: self.collector_number,
            image_uri: self.image_uri,
            mana_cost: self.mana_cost,
            type_line: self.type_line,
            colors: self.colors,
            present: self.present,
            desired: self.desired,
            owned: self.owned,
            present_rollup: self.present_rollup,
            board: Board::from_pg(&self.board)
                .ok_or_else(|| ApiError::Upstream(format!("bad board '{}'", self.board)))?,
        })
    }
}

/// The keyset sort key encoded in an opaque page cursor: the last row's
/// (name, printing, board). Base64url of its JSON — opaque to clients, so the
/// cursor stays shareable/restorable without exposing the sort internals.
#[derive(serde::Serialize, serde::Deserialize)]
struct CardCursor {
    name: String,
    printing_id: Uuid,
    board: String,
}

fn encode_cursor(c: &CardCursor) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(c).expect("cursor serialization cannot fail"))
}

fn decode_cursor(s: &str) -> ApiResult<CardCursor> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| ApiError::Validation("invalid cursor".into()))?;
    serde_json::from_slice(&bytes).map_err(|_| ApiError::Validation("invalid cursor".into()))
}

/// Reject an operation targeting a collection the caller doesn't own — RLS makes
/// a non-owned collection invisible, so this EXISTS both checks ownership and
/// rejects a bad id. (The `holdings`/`desires` RLS policies only gate on their
/// own `user_id`, not the collection's, so this guard is load-bearing.)
async fn require_owned_collection(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
) -> ApiResult<()> {
    let found: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM collections WHERE id = $1")
        .bind(collection_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(upstream)?;
    if found.is_none() {
        return Err(ApiError::NotFound("collection".into()));
    }
    Ok(())
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

/// Like [`upstream`] but classifies common Postgres constraint violations into
/// client-facing errors: a foreign-key miss (e.g. an unknown printing/oracle) is
/// `NotFound`, a unique clash is `Conflict`, a CHECK failure is `Validation`.
fn db_err(e: sqlx::Error) -> ApiError {
    if let Some(dbe) = e.as_database_error() {
        match dbe.code().as_deref() {
            Some("23503") => return ApiError::NotFound("referenced card/printing".into()),
            Some("23505") => return ApiError::Conflict("already exists".into()),
            Some("23514") => return ApiError::Validation("violates a check constraint".into()),
            _ => {}
        }
    }
    upstream(e)
}
