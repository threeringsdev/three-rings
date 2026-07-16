//! Hosted (web) backend: in-process sqlx against Neon — the authorization
//! terminus (specs/data-access-backends.md). Holds the `DATABASE_URL` pool
//! (as the non-owner, RLS-subject `app_runtime` role) and runs every
//! session-scoped query inside a transaction that first sets `app.user_id`, so
//! data-model's RLS policies scope the rows even beneath this terminus.

use shared::{
    union_color_identity, AddHave, AddLine, AddWant, AllCardsRow, AllCardsView, ApiError,
    ApiResult, BatchMove, Board, CardDetail, CardRow, CardSummary, CatalogCount, CollectionKind,
    CollectionSummary, CollectionView, Condition, DeckCommanders, DesireLine, Finish, HoldingLine,
    Id, LineResult, MoveReceipt, MoveRequest, NeedLocation, NeedRow, NeedsView, NewCollection,
    NewTag, OwnershipEntry, Page, PrintingSummary, Rename, RenameTag, Reorder, Reparent, Ruling,
    SearchQuery, SearchResults, SetBoard, SetQuantity, ShoppingList, ShoppingRow,
    SuggestedDestination, Tag, TagAssignment, TagScope, TaggedCard, Teardown, TeardownReceipt,
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

    async fn card_detail(&self, oracle_id: Id) -> ApiResult<CardDetail> {
        // Catalog is public (no RLS) — read the card/printings/rulings off the
        // pool directly; only the ownership block needs the scoped transaction.
        let card: CardDetailSql = sqlx::query_as(
            "SELECT oracle_id, name, mana_cost, cmc::float8 AS cmc, type_line, oracle_text, \
                    colors, color_identity, keywords, power, toughness, loyalty, layout, \
                    legalities, card_faces, all_parts \
             FROM cards WHERE oracle_id = $1",
        )
        .bind(oracle_id)
        .fetch_optional(self.pool)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("card".into()))?;

        let printings: Vec<PrintingRowSql> = sqlx::query_as(
            "SELECT p.id, s.code AS set_code, s.name AS set_name, p.collector_number, p.rarity, \
                    p.image_uris->>'normal' AS image_uri, p.finishes::text[] AS finishes \
             FROM printings p LEFT JOIN sets s ON s.id = p.set_id \
             WHERE p.oracle_id = $1 ORDER BY s.released_at NULLS LAST, p.collector_number",
        )
        .bind(oracle_id)
        .fetch_all(self.pool)
        .await
        .map_err(upstream)?;

        let rulings: Vec<RulingSql> = sqlx::query_as(
            "SELECT published_at::text AS published_at, source, comment \
             FROM rulings WHERE oracle_id = $1 ORDER BY published_at NULLS LAST",
        )
        .bind(oracle_id)
        .fetch_all(self.pool)
        .await
        .map_err(upstream)?;

        let ownership = if self.session.is_some() {
            let mut tx = self.scoped_tx().await?;
            let rows: Vec<OwnershipSql> = sqlx::query_as(
                "SELECT h.collection_id, c.name AS collection_name, h.printing_id, \
                        sum(h.quantity)::int AS quantity \
                 FROM holdings h JOIN printings p ON p.id = h.printing_id \
                 JOIN collections c ON c.id = h.collection_id \
                 WHERE p.oracle_id = $1 \
                 GROUP BY h.collection_id, c.name, h.printing_id",
            )
            .bind(oracle_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;
            tx.commit().await.map_err(upstream)?;
            Some(
                rows.into_iter()
                    .map(|o| OwnershipEntry {
                        collection_id: o.collection_id,
                        collection_name: o.collection_name,
                        printing_id: o.printing_id,
                        quantity: o.quantity,
                    })
                    .collect(),
            )
        } else {
            None
        };

        Ok(CardDetail {
            oracle_id: card.oracle_id,
            name: card.name,
            mana_cost: card.mana_cost,
            cmc: card.cmc,
            type_line: card.type_line,
            oracle_text: card.oracle_text,
            colors: card.colors,
            color_identity: card.color_identity,
            keywords: card.keywords,
            power: card.power,
            toughness: card.toughness,
            loyalty: card.loyalty,
            layout: card.layout,
            legalities: card.legalities,
            card_faces: card.card_faces,
            all_parts: card.all_parts,
            printings: printings
                .into_iter()
                .map(|p| PrintingSummary {
                    id: p.id,
                    set_code: p.set_code,
                    set_name: p.set_name,
                    collector_number: p.collector_number,
                    rarity: p.rarity,
                    image_uri: p.image_uri,
                    finishes: p.finishes,
                })
                .collect(),
            rulings: rulings
                .into_iter()
                .map(|r| Ruling {
                    published_at: r.published_at,
                    source: r.source,
                    comment: r.comment,
                })
                .collect(),
            ownership,
        })
    }

    async fn card_summary(&self, oracle_id: Id) -> ApiResult<CardSummary> {
        let card: SearchRowSql = sqlx::query_as(
            "SELECT c.oracle_id, c.name, c.mana_cost, c.type_line, \
                    (SELECT image_uris->>'normal' FROM printings \
                     WHERE oracle_id = c.oracle_id LIMIT 1) AS image_uri \
             FROM cards c WHERE c.oracle_id = $1",
        )
        .bind(oracle_id)
        .fetch_optional(self.pool)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("card".into()))?;

        let owned = if self.session.is_some() {
            let mut tx = self.scoped_tx().await?;
            let owned: Option<(i32,)> =
                sqlx::query_as("SELECT owned FROM owned_by_card WHERE oracle_id = $1")
                    .bind(oracle_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(upstream)?;
            tx.commit().await.map_err(upstream)?;
            Some(owned.map(|(o,)| o).unwrap_or(0))
        } else {
            None
        };

        Ok(CardSummary {
            oracle_id: card.oracle_id,
            name: card.name,
            image_uri: card.image_uri,
            mana_cost: card.mana_cost,
            type_line: card.type_line,
            owned,
        })
    }

    async fn search(&self, query: SearchQuery, page: Page) -> ApiResult<SearchResults> {
        // Shell: fuzzy name match (trgm-indexed) until catalog-search owns the
        // query grammar. Keyset by (name, oracle).
        let needle = format!("%{}%", query.q.unwrap_or_default());
        let cursor: Option<OracleCursor> = page.cursor.as_deref().map(decode_cursor).transpose()?;
        let limit = page.limit();
        let base = "SELECT c.oracle_id, c.name, c.mana_cost, c.type_line, \
                    (SELECT image_uris->>'normal' FROM printings \
                     WHERE oracle_id = c.oracle_id LIMIT 1) AS image_uri \
             FROM cards c WHERE c.name ILIKE $1";
        let (keyset, order) = if cursor.is_some() {
            (
                " AND (c.name, c.oracle_id) > ($2, $3)",
                " ORDER BY c.name, c.oracle_id LIMIT $4",
            )
        } else {
            ("", " ORDER BY c.name, c.oracle_id LIMIT $2")
        };
        let sql = format!("{base}{keyset}{order}");
        let mut q = sqlx::query_as::<_, SearchRowSql>(&sql).bind(&needle);
        if let Some(c) = &cursor {
            q = q.bind(&c.name).bind(c.oracle_id);
        }
        let mut rows: Vec<SearchRowSql> = q
            .bind(limit + 1)
            .fetch_all(self.pool)
            .await
            .map_err(upstream)?;

        let has_more = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = has_more
            .then(|| {
                rows.last().map(|r| {
                    encode_cursor(&OracleCursor {
                        name: r.name.clone(),
                        oracle_id: r.oracle_id,
                    })
                })
            })
            .flatten();
        Ok(SearchResults {
            cards: rows
                .into_iter()
                .map(|r| CardSummary {
                    oracle_id: r.oracle_id,
                    name: r.name,
                    image_uri: r.image_uri,
                    mana_cost: r.mana_cost,
                    type_line: r.type_line,
                    owned: None,
                })
                .collect(),
            next_cursor,
        })
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
        let cursor: Option<CardCursor> = page.cursor.as_deref().map(decode_cursor).transpose()?;
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

    async fn move_cards(&self, req: MoveRequest) -> ApiResult<MoveReceipt> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        let move_id = apply_move(
            &mut tx,
            user_id,
            req.from_collection_id,
            req.to_collection_id,
            &Grain::from(&req),
            req.quantity,
        )
        .await?;
        tx.commit().await.map_err(upstream)?;
        Ok(MoveReceipt { move_id })
    }

    async fn move_batch(&self, req: BatchMove) -> ApiResult<Vec<MoveReceipt>> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        // All-or-nothing: a failing item aborts the whole transaction.
        let mut receipts = Vec::with_capacity(req.items.len());
        for item in &req.items {
            let grain = Grain {
                printing_id: item.printing_id,
                finish: item.finish.to_pg().to_string(),
                condition: item.condition.to_pg().to_string(),
                language: item.language.clone(),
            };
            let move_id = apply_move(
                &mut tx,
                user_id,
                item.from_collection_id,
                req.to_collection_id,
                &grain,
                item.quantity,
            )
            .await?;
            receipts.push(MoveReceipt { move_id });
        }
        tx.commit().await.map_err(upstream)?;
        Ok(receipts)
    }

    async fn undo_move(&self, move_id: Id) -> ApiResult<()> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        undo_one(&mut tx, user_id, move_id).await?;
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn undo_last_move(&self) -> ApiResult<Option<MoveReceipt>> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        let last: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM moves WHERE undone_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        let receipt = match last {
            Some((move_id,)) => {
                undo_one(&mut tx, user_id, move_id).await?;
                Some(MoveReceipt { move_id })
            }
            None => None,
        };
        tx.commit().await.map_err(upstream)?;
        Ok(receipt)
    }

    async fn suggested_destinations(&self, oracle_id: Id) -> ApiResult<Vec<SuggestedDestination>> {
        let mut tx = self.scoped_tx().await?;
        let rows: Vec<SuggestedRow> = sqlx::query_as(
            "WITH d AS ( \
               SELECT collection_id, sum(quantity)::int AS desired \
               FROM desires WHERE oracle_id = $1 GROUP BY collection_id \
             ), \
             p AS ( \
               SELECT h.collection_id, sum(h.quantity)::int AS present \
               FROM holdings h JOIN printings pr ON pr.id = h.printing_id \
               WHERE pr.oracle_id = $1 GROUP BY h.collection_id \
             ) \
             SELECT c.id AS collection_id, c.name AS collection_name, d.desired, \
                    COALESCE(p.present, 0) AS present \
             FROM d JOIN collections c ON c.id = d.collection_id \
             LEFT JOIN p ON p.collection_id = c.id \
             WHERE d.desired > COALESCE(p.present, 0) \
             ORDER BY (d.desired - COALESCE(p.present, 0)) DESC, c.name",
        )
        .bind(oracle_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        Ok(rows
            .into_iter()
            .map(|r| SuggestedDestination {
                collection_id: r.collection_id,
                collection_name: r.collection_name,
                desired: r.desired,
                present: r.present,
                shortfall: r.desired - r.present,
            })
            .collect())
    }

    async fn teardown(&self, collection_id: Id, mode: Teardown) -> ApiResult<TeardownReceipt> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        if let Teardown::EmptyTo { to_collection_id } = &mode {
            require_owned_collection(&mut tx, *to_collection_id).await?;
        }

        // Snapshot every holding in the collection (summed across board — moves
        // are board-agnostic), then relocate each and delete the source rows.
        let holdings: Vec<MoveGrainRow> = sqlx::query_as(
            "SELECT printing_id, finish::text AS finish, condition::text AS condition, \
                    language, sum(quantity)::int AS quantity \
             FROM holdings WHERE collection_id = $1 \
             GROUP BY printing_id, finish, condition, language",
        )
        .bind(collection_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        let inbox = match &mode {
            Teardown::ReturnToPrevious => Some(inbox_id(&mut tx, user_id).await?),
            Teardown::EmptyTo { .. } => None,
        };

        let mut moves = 0i64;
        for h in &holdings {
            let grain = Grain {
                printing_id: h.printing_id,
                finish: h.finish.clone(),
                condition: h.condition.clone(),
                language: h.language.clone(),
            };
            let dest = match &mode {
                Teardown::EmptyTo { to_collection_id } => *to_collection_id,
                Teardown::ReturnToPrevious => previous_location(&mut tx, collection_id, &grain)
                    .await?
                    .unwrap_or_else(|| inbox.expect("inbox resolved for ReturnToPrevious")),
            };
            // Teardown empties *all* boards for the grain (its snapshot summed
            // across board), so remove every board row rather than main-only.
            holding_delete_all_boards(&mut tx, collection_id, &grain).await?;
            holding_add(&mut tx, user_id, dest, &grain, h.quantity).await?;
            append_move(
                &mut tx,
                user_id,
                Some(collection_id),
                Some(dest),
                &grain,
                h.quantity,
            )
            .await?;
            moves += 1;
        }
        tx.commit().await.map_err(upstream)?;
        Ok(TeardownReceipt { moves })
    }

    async fn all_cards(&self, page: Page) -> ApiResult<AllCardsView> {
        let mut tx = self.scoped_tx().await?;
        let cursor: Option<OracleCursor> = page.cursor.as_deref().map(decode_cursor).transpose()?;
        let limit = page.limit();
        let base = "WITH agg AS ( \
               SELECT p.oracle_id, sum(h.quantity)::int AS owned, \
                      count(DISTINCT h.collection_id)::int AS in_collections \
               FROM holdings h JOIN printings p ON p.id = h.printing_id \
               GROUP BY p.oracle_id \
             ) \
             SELECT a.oracle_id, c.name, a.owned, a.in_collections \
             FROM agg a JOIN cards c ON c.oracle_id = a.oracle_id";
        let keyset = if cursor.is_some() {
            " WHERE (c.name, a.oracle_id) > ($1, $2) ORDER BY c.name, a.oracle_id LIMIT $3"
        } else {
            " ORDER BY c.name, a.oracle_id LIMIT $1"
        };
        let sql = format!("{base}{keyset}");
        let mut q = sqlx::query_as::<_, AllCardsRowSql>(&sql);
        if let Some(c) = &cursor {
            q = q.bind(&c.name).bind(c.oracle_id);
        }
        let mut rows: Vec<AllCardsRowSql> = q
            .bind(limit + 1)
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        let has_more = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = has_more
            .then(|| {
                rows.last().map(|r| {
                    encode_cursor(&OracleCursor {
                        name: r.name.clone(),
                        oracle_id: r.oracle_id,
                    })
                })
            })
            .flatten();
        Ok(AllCardsView {
            cards: rows
                .into_iter()
                .map(|r| AllCardsRow {
                    oracle_id: r.oracle_id,
                    name: r.name,
                    owned: r.owned,
                    in_collections: r.in_collections,
                })
                .collect(),
            next_cursor,
        })
    }

    async fn needs(&self, collection_id: Id) -> ApiResult<NeedsView> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        let rows: Vec<NeedSql> = sqlx::query_as(
            "WITH d AS ( \
               SELECT oracle_id, sum(quantity)::int AS desired \
               FROM desires WHERE collection_id = $1 GROUP BY oracle_id \
             ), \
             ph AS ( \
               SELECT p.oracle_id, sum(h.quantity)::int AS present_here \
               FROM holdings h JOIN printings p ON p.id = h.printing_id \
               WHERE h.collection_id = $1 GROUP BY p.oracle_id \
             ), \
             pe AS ( \
               SELECT p.oracle_id, sum(h.quantity)::int AS elsewhere \
               FROM holdings h JOIN printings p ON p.id = h.printing_id \
               WHERE h.collection_id <> $1 GROUP BY p.oracle_id \
             ) \
             SELECT d.oracle_id, c.name, d.desired, COALESCE(ph.present_here, 0) AS present_here, \
                    COALESCE(pe.elsewhere, 0) AS elsewhere \
             FROM d JOIN cards c ON c.oracle_id = d.oracle_id \
             LEFT JOIN ph ON ph.oracle_id = d.oracle_id \
             LEFT JOIN pe ON pe.oracle_id = d.oracle_id \
             WHERE d.desired > COALESCE(ph.present_here, 0) \
             ORDER BY c.name",
        )
        .bind(collection_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        // Per-location listing for the needed cards, in the user's OTHER
        // collections — one query, grouped in Rust.
        let oracles: Vec<Uuid> = rows.iter().map(|r| r.oracle_id).collect();
        let locs: Vec<LocationSql> = sqlx::query_as(
            "SELECT p.oracle_id, h.collection_id, c.name AS collection_name, \
                    sum(h.quantity)::int AS quantity \
             FROM holdings h JOIN printings p ON p.id = h.printing_id \
             JOIN collections c ON c.id = h.collection_id \
             WHERE h.collection_id <> $1 AND p.oracle_id = ANY($2) \
             GROUP BY p.oracle_id, h.collection_id, c.name \
             ORDER BY quantity DESC, c.name",
        )
        .bind(collection_id)
        .bind(&oracles)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        let need_rows = rows
            .into_iter()
            .map(|r| {
                let gap = r.desired - r.present_here;
                let owned_elsewhere = r.elsewhere.min(gap);
                NeedRow {
                    locations: locs
                        .iter()
                        .filter(|l| l.oracle_id == r.oracle_id)
                        .map(|l| NeedLocation {
                            collection_id: l.collection_id,
                            collection_name: l.collection_name.clone(),
                            quantity: l.quantity,
                        })
                        .collect(),
                    oracle_id: r.oracle_id,
                    name: r.name,
                    desired: r.desired,
                    present_here: r.present_here,
                    owned_elsewhere,
                    short: gap - owned_elsewhere,
                }
            })
            .collect();
        Ok(NeedsView {
            collection_id,
            rows: need_rows,
        })
    }

    async fn shopping_list(&self) -> ApiResult<ShoppingList> {
        let mut tx = self.scoped_tx().await?;
        let rows: Vec<ShoppingSql> = sqlx::query_as(
            "WITH d AS ( \
               SELECT oracle_id, sum(quantity)::int AS desired_total FROM desires GROUP BY oracle_id \
             ), \
             o AS ( \
               SELECT p.oracle_id, sum(h.quantity)::int AS owned \
               FROM holdings h JOIN printings p ON p.id = h.printing_id GROUP BY p.oracle_id \
             ) \
             SELECT d.oracle_id, c.name, d.desired_total, COALESCE(o.owned, 0) AS owned \
             FROM d JOIN cards c ON c.oracle_id = d.oracle_id \
             LEFT JOIN o ON o.oracle_id = d.oracle_id \
             WHERE d.desired_total > COALESCE(o.owned, 0) \
             ORDER BY c.name",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        let oracles: Vec<Uuid> = rows.iter().map(|r| r.oracle_id).collect();
        let wants: Vec<WantedBySql> = sqlx::query_as(
            "SELECT de.oracle_id, c.name AS collection_name \
             FROM desires de JOIN collections c ON c.id = de.collection_id \
             WHERE de.oracle_id = ANY($1) GROUP BY de.oracle_id, c.name ORDER BY c.name",
        )
        .bind(&oracles)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        let shop_rows = rows
            .into_iter()
            .map(|r| ShoppingRow {
                wanted_by: wants
                    .iter()
                    .filter(|w| w.oracle_id == r.oracle_id)
                    .map(|w| w.collection_name.clone())
                    .collect(),
                shortfall: r.desired_total - r.owned,
                oracle_id: r.oracle_id,
                name: r.name,
                desired_total: r.desired_total,
                owned: r.owned,
            })
            .collect();
        Ok(ShoppingList { rows: shop_rows })
    }

    // --- Tags & boards (specs/card-tagging.md) ------------------------------

    async fn create_tag(&self, req: NewTag) -> ApiResult<Tag> {
        if req.name.trim().is_empty() {
            return Err(ApiError::Validation("tag name must not be empty".into()));
        }
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        if let Some(cid) = req.collection_id {
            require_owned_collection(&mut tx, cid).await?;
        }
        // builtin stays NULL — the API never creates system tags. A duplicate
        // name within the scope trips a partial unique index (23505 → Conflict).
        let row: TagSql = sqlx::query_as(&format!(
            "INSERT INTO tags (user_id, collection_id, name, color) \
             VALUES ($1, $2, $3, $4) RETURNING {TAG_COLS}"
        ))
        .bind(user_id)
        .bind(req.collection_id)
        .bind(req.name.trim())
        .bind(&req.color)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(upstream)?;
        row.into_tag()
    }

    async fn rename_tag(&self, tag_id: Id, req: RenameTag) -> ApiResult<Tag> {
        if req.name.trim().is_empty() {
            return Err(ApiError::Validation("tag name must not be empty".into()));
        }
        let mut tx = self.scoped_tx().await?;
        // `builtin IS NULL` bars renaming a system tag (RLS also hides it from
        // writes — its `user_id` is NULL); either way a hit is `NotFound`.
        let row: Option<TagSql> = sqlx::query_as(&format!(
            "UPDATE tags SET name = $2 WHERE id = $1 AND builtin IS NULL RETURNING {TAG_COLS}"
        ))
        .bind(tag_id)
        .bind(req.name.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        match row {
            Some(r) => {
                tx.commit().await.map_err(upstream)?;
                r.into_tag()
            }
            None => Err(ApiError::NotFound("tag".into())),
        }
    }

    async fn delete_tag(&self, tag_id: Id) -> ApiResult<()> {
        let mut tx = self.scoped_tx().await?;
        // ON DELETE CASCADE on card_tags.tag_id drops this tag's assignments.
        let affected = sqlx::query("DELETE FROM tags WHERE id = $1 AND builtin IS NULL")
            .bind(tag_id)
            .execute(&mut *tx)
            .await
            .map_err(upstream)?
            .rows_affected();
        if affected == 0 {
            return Err(ApiError::NotFound("tag".into()));
        }
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn list_tags(&self, collection_id: Id) -> ApiResult<Vec<Tag>> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        // system (user_id NULL) + own account (collection_id NULL) + this deck's
        // (collection_id = $1). RLS (`tags_read`) already limits the non-system
        // rows to the caller's own, so a deck tag for another deck can't leak.
        let rows: Vec<TagSql> = sqlx::query_as(&format!(
            "SELECT {TAG_COLS} FROM tags \
             WHERE user_id IS NULL OR collection_id IS NULL OR collection_id = $1 \
             ORDER BY (builtin IS NULL), name"
        ))
        .bind(collection_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        rows.into_iter().map(TagSql::into_tag).collect()
    }

    async fn assign_tag(&self, req: TagAssignment) -> ApiResult<()> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, req.collection_id).await?;

        // The tag must be visible (system or own) — RLS hides others, so an
        // unknown/foreign tag reads as absent.
        let tag: TagScopeSql =
            sqlx::query_as("SELECT collection_id, builtin FROM tags WHERE id = $1")
                .bind(req.tag_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(upstream)?
                .ok_or_else(|| ApiError::NotFound("tag".into()))?;

        // Deck-tag containment: a deck-scoped tag applies only in its own deck.
        if let Some(tag_cid) = tag.collection_id {
            if tag_cid != req.collection_id {
                return Err(ApiError::Conflict(
                    "a deck tag applies only within its own collection".into(),
                ));
            }
        }

        // The card must actually be in the deck (held or desired).
        let in_deck: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 WHERE EXISTS ( \
               SELECT 1 FROM holdings h JOIN printings p ON p.id = h.printing_id \
                WHERE h.collection_id = $1 AND p.oracle_id = $2 \
               UNION ALL \
               SELECT 1 FROM desires d WHERE d.collection_id = $1 AND d.oracle_id = $2 )",
        )
        .bind(req.collection_id)
        .bind(req.oracle_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        if in_deck.is_none() {
            return Err(ApiError::Validation(
                "card is not in this collection".into(),
            ));
        }

        // Built-in caps: ≤ 2 commanders, ≤ 1 companion per deck. Count distinct
        // oracles already carrying the built-in, excluding the one being
        // assigned (so re-assigning an existing commander stays idempotent).
        if let Some(cap) = builtin_cap(tag.builtin.as_deref()) {
            let (count,): (i64,) = sqlx::query_as(
                "SELECT count(DISTINCT ct.oracle_id) FROM card_tags ct \
                 JOIN tags t ON t.id = ct.tag_id \
                 WHERE ct.collection_id = $1 AND t.builtin = $2 AND ct.oracle_id <> $3",
            )
            .bind(req.collection_id)
            .bind(tag.builtin.as_deref())
            .bind(req.oracle_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(upstream)?;
            if count >= cap {
                return Err(ApiError::Conflict(format!(
                    "a deck may have at most {cap} {}(s)",
                    tag.builtin.as_deref().unwrap_or("of this tag")
                )));
            }
        }

        sqlx::query(
            "INSERT INTO card_tags (collection_id, oracle_id, tag_id, user_id) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (collection_id, oracle_id, tag_id) DO NOTHING",
        )
        .bind(req.collection_id)
        .bind(req.oracle_id)
        .bind(req.tag_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn unassign_tag(&self, req: TagAssignment) -> ApiResult<()> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, req.collection_id).await?;
        // Idempotent: removing an absent assignment is a no-op. RLS restricts the
        // delete to the caller's own rows.
        sqlx::query(
            "DELETE FROM card_tags WHERE collection_id = $1 AND oracle_id = $2 AND tag_id = $3",
        )
        .bind(req.collection_id)
        .bind(req.oracle_id)
        .bind(req.tag_id)
        .execute(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn card_tags(&self, collection_id: Id, oracle_id: Id) -> ApiResult<Vec<Tag>> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        let rows: Vec<TagSql> = sqlx::query_as(
            "SELECT t.id, t.user_id, t.collection_id, t.name, t.builtin, t.color \
             FROM card_tags ct JOIN tags t ON t.id = ct.tag_id \
             WHERE ct.collection_id = $1 AND ct.oracle_id = $2 \
             ORDER BY (t.builtin IS NULL), t.name",
        )
        .bind(collection_id)
        .bind(oracle_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        rows.into_iter().map(TagSql::into_tag).collect()
    }

    async fn cards_with_tag(&self, collection_id: Id, tag_id: Id) -> ApiResult<Vec<TaggedCard>> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        let rows: Vec<TaggedCardSql> = sqlx::query_as(
            "SELECT c.oracle_id, c.name, c.mana_cost, c.type_line, c.color_identity, \
                    (SELECT image_uris->>'normal' FROM printings \
                     WHERE oracle_id = c.oracle_id LIMIT 1) AS image_uri \
             FROM card_tags ct JOIN cards c ON c.oracle_id = ct.oracle_id \
             WHERE ct.collection_id = $1 AND ct.tag_id = $2 ORDER BY c.name",
        )
        .bind(collection_id)
        .bind(tag_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        Ok(rows.into_iter().map(TaggedCardSql::into_card).collect())
    }

    async fn deck_commanders(&self, collection_id: Id) -> ApiResult<DeckCommanders> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        let rows: Vec<TaggedCardSql> = sqlx::query_as(
            "SELECT c.oracle_id, c.name, c.mana_cost, c.type_line, c.color_identity, \
                    (SELECT image_uris->>'normal' FROM printings \
                     WHERE oracle_id = c.oracle_id LIMIT 1) AS image_uri \
             FROM card_tags ct JOIN tags t ON t.id = ct.tag_id \
             JOIN cards c ON c.oracle_id = ct.oracle_id \
             WHERE ct.collection_id = $1 AND t.builtin = 'commander' ORDER BY c.name",
        )
        .bind(collection_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        let commanders: Vec<TaggedCard> = rows.into_iter().map(TaggedCardSql::into_card).collect();
        // Color identity is derived, never stored — the WUBRG union of the
        // commanders' identities, so it is always current after an assignment.
        let color_identity =
            union_color_identity(commanders.iter().map(|c| c.color_identity.as_slice()));
        Ok(DeckCommanders {
            commanders,
            color_identity,
        })
    }

    async fn set_holding_board(&self, holding_id: Id, req: SetBoard) -> ApiResult<()> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        let row: HoldingBoardSql = sqlx::query_as(
            "SELECT h.collection_id, h.printing_id, h.finish::text AS finish, \
                    h.condition::text AS condition, h.language, h.board::text AS board, \
                    h.quantity, col.kind::text AS kind \
             FROM holdings h JOIN collections col ON col.id = h.collection_id \
             WHERE h.id = $1",
        )
        .bind(holding_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("holding".into()))?;

        if row.kind != CollectionKind::Deck.to_pg() {
            return Err(ApiError::Validation("boards apply to decks only".into()));
        }
        let to = req.board.to_pg();
        if row.board == to {
            return Ok(()); // no-op: already on the target board
        }
        let move_qty = req.quantity.unwrap_or(row.quantity);
        if move_qty <= 0 || move_qty > row.quantity {
            return Err(ApiError::Validation(
                "board quantity must be > 0 and ≤ the row's quantity".into(),
            ));
        }

        // Upsert into the destination board's row (merging if it exists), then
        // decrement/delete the source — a quantity-preserving split.
        sqlx::query(
            "INSERT INTO holdings \
               (user_id, collection_id, printing_id, finish, condition, language, board, quantity) \
             VALUES ($1, $2, $3, $4::card_finish, $5::card_condition, $6, $7::card_board, $8) \
             ON CONFLICT ON CONSTRAINT holdings_uniq \
               DO UPDATE SET quantity = holdings.quantity + EXCLUDED.quantity",
        )
        .bind(user_id)
        .bind(row.collection_id)
        .bind(row.printing_id)
        .bind(&row.finish)
        .bind(&row.condition)
        .bind(&row.language)
        .bind(to)
        .bind(move_qty)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        take_or_delete_holding(&mut tx, holding_id, row.quantity, move_qty).await?;
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn set_desire_board(&self, desire_id: Id, req: SetBoard) -> ApiResult<()> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        let row: DesireBoardSql = sqlx::query_as(
            "SELECT d.collection_id, d.oracle_id, d.printing_id, d.board::text AS board, \
                    d.quantity, col.kind::text AS kind \
             FROM desires d JOIN collections col ON col.id = d.collection_id \
             WHERE d.id = $1",
        )
        .bind(desire_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("desire".into()))?;

        if row.kind != CollectionKind::Deck.to_pg() {
            return Err(ApiError::Validation("boards apply to decks only".into()));
        }
        let to = req.board.to_pg();
        if row.board == to {
            return Ok(());
        }
        let move_qty = req.quantity.unwrap_or(row.quantity);
        if move_qty <= 0 || move_qty > row.quantity {
            return Err(ApiError::Validation(
                "board quantity must be > 0 and ≤ the row's quantity".into(),
            ));
        }

        sqlx::query(
            "INSERT INTO desires (user_id, collection_id, oracle_id, printing_id, board, quantity) \
             VALUES ($1, $2, $3, $4, $5::card_board, $6) \
             ON CONFLICT ON CONSTRAINT desires_uniq \
               DO UPDATE SET quantity = desires.quantity + EXCLUDED.quantity",
        )
        .bind(user_id)
        .bind(row.collection_id)
        .bind(row.oracle_id)
        .bind(row.printing_id)
        .bind(to)
        .bind(move_qty)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        take_or_delete_desire(&mut tx, desire_id, row.quantity, move_qty).await?;
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }
}

/// The built-in-tag per-deck cap, if the tag is a capped built-in: `commander`
/// ≤ 2 (partners / Background / Doctor's-companion), `companion` ≤ 1. `None`
/// (uncapped) for user tags and any other built-in. Full legal-commander /
/// companion-restriction validation is a rules-engine concern (card-tagging OQ).
fn builtin_cap(builtin: Option<&str>) -> Option<i64> {
    match builtin {
        Some("commander") => Some(2),
        Some("companion") => Some(1),
        _ => None,
    }
}

/// Decrement a holding row by `take`, deleting it when that empties it (the
/// CHECK forbids quantity 0). Shared by the board split's source side.
async fn take_or_delete_holding(
    tx: &mut Transaction<'static, Postgres>,
    holding_id: Id,
    current: i32,
    take: i32,
) -> ApiResult<()> {
    if take >= current {
        sqlx::query("DELETE FROM holdings WHERE id = $1")
            .bind(holding_id)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    } else {
        sqlx::query("UPDATE holdings SET quantity = quantity - $2 WHERE id = $1")
            .bind(holding_id)
            .bind(take)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    }
    Ok(())
}

/// Desire counterpart of [`take_or_delete_holding`].
async fn take_or_delete_desire(
    tx: &mut Transaction<'static, Postgres>,
    desire_id: Id,
    current: i32,
    take: i32,
) -> ApiResult<()> {
    if take >= current {
        sqlx::query("DELETE FROM desires WHERE id = $1")
            .bind(desire_id)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    } else {
        sqlx::query("UPDATE desires SET quantity = quantity - $2 WHERE id = $1")
            .bind(desire_id)
            .bind(take)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    }
    Ok(())
}

/// A move's card grain: printing + finish/condition/language (Postgres enum
/// labels as text, cast in SQL). Board is deliberately absent — moves act on the
/// mainboard (the ledger has no board column).
struct Grain {
    printing_id: Uuid,
    finish: String,
    condition: String,
    language: String,
}

impl From<&MoveRequest> for Grain {
    fn from(r: &MoveRequest) -> Self {
        Grain {
            printing_id: r.printing_id,
            finish: r.finish.to_pg().to_string(),
            condition: r.condition.to_pg().to_string(),
            language: r.language.clone(),
        }
    }
}

/// Perform one move within an open transaction: validate, decrement the source
/// mainboard holding, upsert the destination, append the ledger row. Returns the
/// new move id.
async fn apply_move(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    from: Option<Id>,
    to: Option<Id>,
    grain: &Grain,
    quantity: i32,
) -> ApiResult<Uuid> {
    if quantity <= 0 {
        return Err(ApiError::Validation("quantity must be > 0".into()));
    }
    if from.is_none() && to.is_none() {
        return Err(ApiError::Validation(
            "a move needs a source or destination".into(),
        ));
    }
    if from.is_some() && from == to {
        return Err(ApiError::Validation(
            "source and destination are the same".into(),
        ));
    }
    if let Some(from) = from {
        require_owned_collection(tx, from).await?;
        holding_take(tx, from, grain, quantity).await?;
    }
    if let Some(to) = to {
        require_owned_collection(tx, to).await?;
        holding_add(tx, user_id, to, grain, quantity).await?;
    }
    append_move(tx, user_id, from, to, grain, quantity).await
}

/// Reverse a move and stamp `undone_at`. Idempotent: an already-undone or
/// missing-but-owned move is handled without double-reversing.
async fn undo_one(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    move_id: Id,
) -> ApiResult<()> {
    let m: Option<MoveRow> = sqlx::query_as(
        "SELECT printing_id, finish::text AS finish, condition::text AS condition, language, \
                from_collection_id, to_collection_id, quantity, (undone_at IS NOT NULL) AS undone \
         FROM moves WHERE id = $1",
    )
    .bind(move_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    let m = m.ok_or_else(|| ApiError::NotFound("move".into()))?;
    if m.undone {
        return Ok(()); // idempotent
    }
    let grain = Grain {
        printing_id: m.printing_id,
        finish: m.finish,
        condition: m.condition,
        language: m.language,
    };
    // Reverse: give the copies back to the source, take them from the dest.
    if let Some(from) = m.from_collection_id {
        holding_add(tx, user_id, from, &grain, m.quantity).await?;
    }
    if let Some(to) = m.to_collection_id {
        holding_take_clamp(tx, to, &grain, m.quantity).await?;
    }
    sqlx::query("UPDATE moves SET undone_at = now() WHERE id = $1")
        .bind(move_id)
        .execute(&mut **tx)
        .await
        .map_err(upstream)?;
    Ok(())
}

/// Upsert `+delta` into a collection's **mainboard** holding for the grain.
async fn holding_add(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    collection_id: Id,
    grain: &Grain,
    delta: i32,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO holdings \
           (user_id, collection_id, printing_id, finish, condition, language, board, quantity) \
         VALUES ($1, $2, $3, $4::card_finish, $5::card_condition, $6, 'main', $7) \
         ON CONFLICT ON CONSTRAINT holdings_uniq \
           DO UPDATE SET quantity = holdings.quantity + EXCLUDED.quantity",
    )
    .bind(user_id)
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .bind(delta)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Remove exactly `need` copies from a collection's mainboard holding; errors
/// `Conflict` if fewer are present. Deletes the row at zero (the CHECK forbids 0).
async fn holding_take(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
    need: i32,
) -> ApiResult<()> {
    let cur: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, quantity FROM holdings \
         WHERE collection_id = $1 AND printing_id = $2 AND finish = $3::card_finish \
           AND condition = $4::card_condition AND language = $5 AND board = 'main'",
    )
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    let (id, qty) = cur.ok_or_else(|| ApiError::Conflict("no copies to move".into()))?;
    if qty < need {
        return Err(ApiError::Conflict("insufficient copies to move".into()));
    }
    if qty == need {
        sqlx::query("DELETE FROM holdings WHERE id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    } else {
        sqlx::query("UPDATE holdings SET quantity = quantity - $2 WHERE id = $1")
            .bind(id)
            .bind(need)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    }
    Ok(())
}

/// Best-effort removal for undo: take up to `want` from the mainboard holding,
/// clamping to what's there (the dest may have changed since the move).
async fn holding_take_clamp(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
    want: i32,
) -> ApiResult<()> {
    let cur: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, quantity FROM holdings \
         WHERE collection_id = $1 AND printing_id = $2 AND finish = $3::card_finish \
           AND condition = $4::card_condition AND language = $5 AND board = 'main'",
    )
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    let Some((id, qty)) = cur else { return Ok(()) };
    if qty <= want {
        sqlx::query("DELETE FROM holdings WHERE id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    } else {
        sqlx::query("UPDATE holdings SET quantity = quantity - $2 WHERE id = $1")
            .bind(id)
            .bind(want)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    }
    Ok(())
}

/// Delete all board rows for a grain from a collection (teardown empties every
/// board; its snapshot already summed across board).
async fn holding_delete_all_boards(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
) -> ApiResult<()> {
    sqlx::query(
        "DELETE FROM holdings WHERE collection_id = $1 AND printing_id = $2 \
           AND finish = $3::card_finish AND condition = $4::card_condition AND language = $5",
    )
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .execute(&mut **tx)
    .await
    .map_err(upstream)?;
    Ok(())
}

/// Append a `moves` ledger row and return its id.
async fn append_move(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    from: Option<Id>,
    to: Option<Id>,
    grain: &Grain,
    quantity: i32,
) -> ApiResult<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO moves \
           (user_id, printing_id, finish, condition, language, \
            from_collection_id, to_collection_id, quantity) \
         VALUES ($1, $2, $3::card_finish, $4::card_condition, $5, $6, $7, $8) RETURNING id",
    )
    .bind(user_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .bind(from)
    .bind(to)
    .bind(quantity)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(id)
}

/// The most-recent collection this card was moved *into* the given collection
/// from (for teardown "return to previous"), or `None` if there's no history.
async fn previous_location(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
) -> ApiResult<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT from_collection_id FROM moves \
         WHERE to_collection_id = $1 AND printing_id = $2 AND finish = $3::card_finish \
           AND condition = $4::card_condition AND language = $5 AND from_collection_id IS NOT NULL \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    Ok(row.map(|(id,)| id))
}

/// The caller's Inbox id, provisioning it if missing (idempotent).
async fn inbox_id(tx: &mut Transaction<'static, Postgres>, user_id: Uuid) -> ApiResult<Uuid> {
    sqlx::query(
        "INSERT INTO collections (user_id, kind, name, is_inbox) \
         VALUES ($1, 'binder', 'Inbox', true) \
         ON CONFLICT (user_id) WHERE is_inbox DO NOTHING",
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(upstream)?;
    let (id,): (Uuid,) = sqlx::query_as("SELECT id FROM collections WHERE is_inbox")
        .fetch_one(&mut **tx)
        .await
        .map_err(upstream)?;
    Ok(id)
}

#[derive(sqlx::FromRow)]
struct MoveRow {
    printing_id: Uuid,
    finish: String,
    condition: String,
    language: String,
    from_collection_id: Option<Uuid>,
    to_collection_id: Option<Uuid>,
    quantity: i32,
    undone: bool,
}

#[derive(sqlx::FromRow)]
struct MoveGrainRow {
    printing_id: Uuid,
    finish: String,
    condition: String,
    language: String,
    quantity: i32,
}

#[derive(sqlx::FromRow)]
struct SuggestedRow {
    collection_id: Uuid,
    collection_name: String,
    desired: i32,
    present: i32,
}

#[derive(sqlx::FromRow)]
struct AllCardsRowSql {
    oracle_id: Uuid,
    name: String,
    owned: i32,
    in_collections: i32,
}

/// Keyset key for the everything-view: (name, oracle).
#[derive(serde::Serialize, serde::Deserialize)]
struct OracleCursor {
    name: String,
    oracle_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct NeedSql {
    oracle_id: Uuid,
    name: String,
    desired: i32,
    present_here: i32,
    elsewhere: i32,
}

#[derive(sqlx::FromRow)]
struct LocationSql {
    oracle_id: Uuid,
    collection_id: Uuid,
    collection_name: String,
    quantity: i32,
}

#[derive(sqlx::FromRow)]
struct ShoppingSql {
    oracle_id: Uuid,
    name: String,
    desired_total: i32,
    owned: i32,
}

#[derive(sqlx::FromRow)]
struct WantedBySql {
    oracle_id: Uuid,
    collection_name: String,
}

#[derive(sqlx::FromRow)]
struct CardDetailSql {
    oracle_id: Uuid,
    name: String,
    mana_cost: Option<String>,
    cmc: Option<f64>,
    type_line: Option<String>,
    oracle_text: Option<String>,
    colors: Vec<String>,
    color_identity: Vec<String>,
    keywords: Vec<String>,
    power: Option<String>,
    toughness: Option<String>,
    loyalty: Option<String>,
    layout: Option<String>,
    legalities: Option<serde_json::Value>,
    card_faces: Option<serde_json::Value>,
    all_parts: Option<serde_json::Value>,
}

#[derive(sqlx::FromRow)]
struct PrintingRowSql {
    id: Uuid,
    set_code: Option<String>,
    set_name: Option<String>,
    collector_number: String,
    rarity: String,
    image_uri: Option<String>,
    finishes: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct RulingSql {
    published_at: Option<String>,
    source: Option<String>,
    comment: String,
}

#[derive(sqlx::FromRow)]
struct OwnershipSql {
    collection_id: Uuid,
    collection_name: String,
    printing_id: Uuid,
    quantity: i32,
}

#[derive(sqlx::FromRow)]
struct SearchRowSql {
    oracle_id: Uuid,
    name: String,
    mana_cost: Option<String>,
    type_line: Option<String>,
    image_uri: Option<String>,
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

/// The `tags` projection matching [`TagSql`].
const TAG_COLS: &str = "id, user_id, collection_id, name, builtin, color";

#[derive(sqlx::FromRow)]
struct TagSql {
    id: Uuid,
    user_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    name: String,
    builtin: Option<String>,
    color: Option<String>,
}

impl TagSql {
    fn into_tag(self) -> ApiResult<Tag> {
        Ok(Tag {
            scope: TagScope::from_fks(self.user_id, self.collection_id),
            id: self.id,
            name: self.name,
            builtin: self.builtin,
            color: self.color,
        })
    }
}

/// The subset of a `tags` row the assignment path needs to enforce containment
/// and the built-in caps.
#[derive(sqlx::FromRow)]
struct TagScopeSql {
    collection_id: Option<Uuid>,
    builtin: Option<String>,
}

#[derive(sqlx::FromRow)]
struct TaggedCardSql {
    oracle_id: Uuid,
    name: String,
    mana_cost: Option<String>,
    type_line: Option<String>,
    color_identity: Vec<String>,
    image_uri: Option<String>,
}

impl TaggedCardSql {
    fn into_card(self) -> TaggedCard {
        TaggedCard {
            oracle_id: self.oracle_id,
            name: self.name,
            mana_cost: self.mana_cost,
            type_line: self.type_line,
            image_uri: self.image_uri,
            color_identity: self.color_identity,
        }
    }
}

/// A `holdings` row plus its collection kind — the board re-label's read.
#[derive(sqlx::FromRow)]
struct HoldingBoardSql {
    collection_id: Uuid,
    printing_id: Uuid,
    finish: String,
    condition: String,
    language: String,
    board: String,
    quantity: i32,
    kind: String,
}

/// A `desires` row plus its collection kind — the board re-label's read.
#[derive(sqlx::FromRow)]
struct DesireBoardSql {
    collection_id: Uuid,
    oracle_id: Uuid,
    printing_id: Option<Uuid>,
    board: String,
    quantity: i32,
    kind: String,
}

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

fn encode_cursor<T: serde::Serialize>(c: &T) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(c).expect("cursor serialization cannot fail"))
}

fn decode_cursor<T: serde::de::DeserializeOwned>(s: &str) -> ApiResult<T> {
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
