//! Hosted (web) backend: in-process sqlx against Neon — the authorization
//! terminus (specs/data-access-backends.md). Holds the `DATABASE_URL` pool
//! (as the non-owner, RLS-subject `app_runtime` role) and runs every
//! session-scoped query inside a transaction that first sets `app.user_id`, so
//! data-model's RLS policies scope the rows even beneath this terminus.

use shared::{
    union_color_identity, AddHave, AddLine, AddWant, AllCardsRow, AllCardsView, ApiError,
    ApiResult, BatchMove, Board, CardDetail, CardLocation, CardRow, CardSummary, CatalogCount,
    CollectionKind, CollectionSummary, CollectionTree, CollectionTreeRow, CollectionView,
    Condition, DeckCommanders, DeleteCollectionReceipt, DeleteCollectionReq, DeletedCollectionRow,
    DesireLine, Finish, HaveDisposition, HoldingLine, HoldingMove, Id, LineResult, MoveReceipt,
    MoveRequest, NeedRow, NeedsView, NewCollection, NewTag, OwnershipEntry, Page, PrintingSummary,
    RelocatedDesire, Rename, RenameTag, Reorder, Reparent, Ruling, SearchQuery, SearchResults,
    SetBoard, SetQuantity, SetQuery, SetSummary, ShoppingList, ShoppingRow, SuggestedDestination,
    Tag, TagAssignment, TagScope, TaggedCard, Teardown, TeardownReceipt, UndoReceipt,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

use super::delete_plan::{
    plan_delete, plan_undo, reparent_is_safe, restore_parent, validate_receipt, DeleteSnapshot,
    UndoStep,
};
use super::pull_plan;
use super::{CatalogStore, CollectionStore};
use crate::my::move_selection::MoveSource;
use crate::my::needs::{PullItem, PullOutcome};

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

    /// The caller's **owned** count (data-model's global per-user, per-oracle
    /// aggregate) for each of `oracle_ids` — the one source both `CardSummary`
    /// projections read, so a tile and the detail page can never disagree about
    /// the number.
    ///
    /// `None` means *anonymous*: `owned` is unknown, which is a different claim
    /// from `Some(0)` ("signed in, holds none") and is why the catalog can't
    /// just default it. A card with no holdings has **no row** in the view, so
    /// an authed miss is `Some(0)` — see [`owned_of`].
    ///
    /// `owned_by_card` is a `security_invoker` view over the RLS-forced
    /// `holdings`/`collections`, so it is only readable inside [`scoped_tx`]
    /// (`self.pool` would see zero rows). Callers whose main query is the public
    /// unscoped catalog read therefore pay **one** extra round trip for a whole
    /// page: every id goes in a single `= ANY($1)`, never one query per row.
    ///
    /// [`scoped_tx`]: HostedBackend::scoped_tx
    async fn owned_by_oracle(&self, oracle_ids: &[Uuid]) -> ApiResult<Option<HashMap<Uuid, i32>>> {
        if self.session.is_none() {
            return Ok(None);
        }
        if oracle_ids.is_empty() {
            return Ok(Some(HashMap::new()));
        }
        let mut tx = self.scoped_tx().await?;
        let rows: Vec<(Uuid, i32)> =
            sqlx::query_as("SELECT oracle_id, owned FROM owned_by_card WHERE oracle_id = ANY($1)")
                .bind(oracle_ids)
                .fetch_all(&mut *tx)
                .await
                .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        Ok(Some(rows.into_iter().collect()))
    }
}

/// One card's `owned` out of [`HostedBackend::owned_by_oracle`]'s answer:
/// anonymous stays `None`, an authed card the view has no row for is `Some(0)`.
fn owned_of(owned: &Option<HashMap<Uuid, i32>>, oracle_id: Uuid) -> Option<i32> {
    owned
        .as_ref()
        .map(|m| m.get(&oracle_id).copied().unwrap_or(0))
}

/// The `collections` projection matching [`CollectionRow`] — `kind`/`position`
/// cast so sqlx (no decimal feature) decodes them.
const COLLECTION_COLS: &str =
    "id, parent_id, kind::text AS kind, name, is_inbox, position::float8 AS position, format";

/// SQL predicate: the row's collection is **live** — not soft-deleted
/// (specs/collection-deletion.md → "The read path"). Deletion hides a
/// collection *and everything hanging off it*: its holdings stop counting, its
/// desires stop generating needs/shopping rows, and it is never a legal move
/// destination or write target.
///
/// Written as a correlated `EXISTS` for the queries that read `holdings` or
/// `desires` without already joining `collections`; where the join is already
/// there, the filter is a plain `deleted_at IS NULL` on it instead.
///
/// **Owned-per-oracle is deliberately not filtered through here.** That filter
/// lands exactly once, in the `owned_by_card` view
/// (migrations/0010_collection_soft_delete.sql), which is what P6-039's collapse
/// and its `owned_definition_guard` test bought. This helper is for the
/// collection-scoped aggregations that are *not* re-derivations of that view —
/// present-here / present-elsewhere, per-collection demand, per-location
/// breakdowns — which the spec calls out as needing their own handling.
///
/// `collection_id_col` is always one of our own column expressions, never user
/// input; the `live` alias is picked so it cannot collide with a caller's.
fn in_live_collection(collection_id_col: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM collections live \
         WHERE live.id = {collection_id_col} AND live.deleted_at IS NULL)"
    )
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
            // Multi-face fallback: migration 0002 leaves top-level `image_uris`
            // NULL on double-faced layouts and puts the per-face images in
            // `faces`, so a bare `image_uris->>'normal'` renders every DFC
            // imageless. Every image projection in this file carries the same
            // COALESCE (specs/app-ui.md, card-detail task).
            "SELECT p.id, s.code AS set_code, s.name AS set_name, p.collector_number, p.rarity, \
                    COALESCE(p.image_uris->>'normal', p.faces->0->'image_uris'->>'normal') \
                        AS image_uri, p.finishes::text[] AS finishes, \
                    CASE WHEN p.faces IS NOT NULL THEN \
                        (SELECT array_agg(f->'image_uris'->>'normal' ORDER BY ord) \
                         FROM jsonb_array_elements(p.faces) WITH ORDINALITY AS t(f, ord)) \
                    END AS face_image_uris \
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
            // `c.deleted_at IS NULL`: the "you hold N copies, here" block on the
            // card page is a per-collection breakdown, so a soft-deleted
            // collection's copies must drop out of it exactly as they drop out
            // of `owned` (specs/collection-deletion.md).
            let rows: Vec<OwnershipSql> = sqlx::query_as(
                "SELECT h.collection_id, c.name AS collection_name, h.printing_id, \
                        sum(h.quantity)::int AS quantity \
                 FROM holdings h JOIN printings p ON p.id = h.printing_id \
                 JOIN collections c ON c.id = h.collection_id \
                 WHERE p.oracle_id = $1 AND c.deleted_at IS NULL \
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
                    face_image_uris: p.face_image_uris.unwrap_or_default(),
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
            &(summary_select() + REPRESENTATIVE_PRINTING_JOIN + "WHERE c.oracle_id = $1"),
        )
        .bind(oracle_id)
        .fetch_optional(self.pool)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("card".into()))?;

        let owned = self.owned_by_oracle(&[oracle_id]).await?;
        Ok(card.into_summary(owned_of(&owned, oracle_id)))
    }

    async fn search(&self, query: SearchQuery, page: Page) -> ApiResult<SearchResults> {
        // The catalog-search query engine: parse the v1 grammar (a parse error
        // is a 422 naming the offending term — never silently-wrong results),
        // emit the WHERE clause, keyset by (name, oracle) — Scryfall's own
        // default sort. Empty query = browse-all.
        let terms = shared::search::parse(query.q.as_deref().unwrap_or(""))
            .map_err(|e| ApiError::Validation(e.to_string()))?;
        let cursor: Option<OracleCursor> = page.cursor.as_deref().map(decode_cursor).transpose()?;
        let limit = page.limit();
        let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> =
            sqlx::QueryBuilder::new(summary_select() + REPRESENTATIVE_PRINTING_JOIN + "WHERE true");
        crate::search::sql::apply(&mut qb, &terms);
        if let Some(c) = &cursor {
            qb.push(" AND (c.name, c.oracle_id) > (");
            qb.push_bind(c.name.clone());
            qb.push(", ");
            qb.push_bind(c.oracle_id);
            qb.push(")");
        }
        qb.push(" ORDER BY c.name, c.oracle_id LIMIT ");
        qb.push_bind(limit + 1);
        let mut rows: Vec<SearchRowSql> = qb
            .build_query_as()
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

        // The `owned` badge, authed-only: the catalog read above is deliberately
        // unscoped (public, anonymous-safe), while `owned_by_card` is RLS-scoped
        // and readable only inside `scoped_tx` — so ownership is a second,
        // session-only round trip over *this page's* ids rather than a join into
        // the paging query. Two reasons it is the second query and not the join:
        // the anonymous path then stays byte-for-byte the query it always was
        // (no transaction, no `owned` column, `None` not `0`), and the number
        // comes out of the same helper `card_summary` uses, so tile and detail
        // page cannot drift. Runs after `truncate`, so the discarded has-more
        // probe row is not looked up.
        let ids: Vec<Uuid> = rows.iter().map(|r| r.oracle_id).collect();
        let owned = self.owned_by_oracle(&ids).await?;
        Ok(SearchResults {
            cards: rows
                .into_iter()
                .map(|r| {
                    let n = owned_of(&owned, r.oracle_id);
                    r.into_summary(n)
                })
                .collect(),
            next_cursor,
        })
    }

    async fn list_sets(&self, query: SetQuery) -> ApiResult<Vec<SetSummary>> {
        // Public read; catalog RLS is off, so no scoped transaction (same as
        // `card_count`). `term()` — not the raw `q` — so a blank box browses
        // instead of substring-matching every set with `''`.
        let term = query.term();
        let rows: Vec<SetRowSql> = sqlx::query_as(
            "SELECT code, name, set_type, released_at::text AS released_at \
             FROM sets \
             WHERE $1::text IS NULL \
                OR code ILIKE '%' || $1 || '%' \
                OR name ILIKE '%' || $1 || '%' \
             ORDER BY CASE \
                        WHEN lower(code) = lower(coalesce($1, '')) THEN 0 \
                        WHEN code ILIKE coalesce($1, '') || '%' \
                          OR name ILIKE coalesce($1, '') || '%' THEN 1 \
                        ELSE 2 \
                      END, \
                      released_at DESC NULLS LAST, code \
             LIMIT $2",
        )
        // The three ORDER BY tiers exist because the window is bounded: typing
        // `mh3` matches `amh3`/`tmh3`/`pmh3` as well, and without exact-code-first
        // the set the user named can fall off the end of the page. Newest-first
        // within a tier — a set filter is nearly always about a recent release.
        .bind(term)
        .bind(query.limit())
        .fetch_all(self.pool)
        .await
        .map_err(upstream)?;

        Ok(rows
            .into_iter()
            .map(|r| SetSummary {
                code: r.code,
                name: r.name,
                set_type: r.set_type,
                released_at: r.released_at,
            })
            .collect())
    }
}

#[derive(sqlx::FromRow)]
struct SetRowSql {
    code: String,
    name: String,
    set_type: String,
    released_at: Option<String>,
}

impl CollectionStore for HostedBackend {
    async fn list_collections(&self) -> ApiResult<Vec<CollectionSummary>> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        ensure_inbox(&mut tx, user_id).await?;

        let rows: Vec<CollectionRow> = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS} FROM collections WHERE deleted_at IS NULL \
             ORDER BY position, name"
        ))
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        rows.into_iter().map(CollectionRow::into_summary).collect()
    }

    async fn collection_tree(&self) -> ApiResult<CollectionTree> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        ensure_inbox(&mut tx, user_id).await?;

        // The `present`/`desired` sub-selects need no live filter of their
        // own: both are LEFT JOINed *from* the filtered `collections` scan,
        // so a hidden collection's holdings/desires have nothing to attach
        // to. `desired` rides the same read as `present` (rather than a
        // second round-trip) so the delete confirm's honest wants count
        // (specs/collection-deletion.md → step 4) is available wherever the
        // sidebar tree already is — the row opened from has no
        // `collection_view` to read it from instead.
        let rows: Vec<CollectionTreeSql> = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS}, COALESCE(h.present, 0)::bigint AS present, \
                    COALESCE(d.desired, 0)::bigint AS desired \
             FROM collections \
             LEFT JOIN (SELECT collection_id, sum(quantity) AS present \
                        FROM holdings GROUP BY collection_id) h \
               ON h.collection_id = collections.id \
             LEFT JOIN (SELECT collection_id, sum(quantity) AS desired \
                        FROM desires GROUP BY collection_id) d \
               ON d.collection_id = collections.id \
             WHERE collections.deleted_at IS NULL \
             ORDER BY position, name"
        ))
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        // The badge is a COUNT over the same short-card rule `shopping_list`
        // renders (total desired − owned > 0, per oracle) — keep them in step.
        // `o` reads the `owned_by_card` view (security_invoker, RLS-scoped to
        // the caller inside this `scoped_tx`) rather than re-deriving the sum
        // from `holdings`/`printings` — the one shared source every "owned per
        // card" query in this file reads (specs/collection-api.md Findings), and
        // where the soft-delete filter for `owned` therefore already lives.
        // `d` is its own aggregation over `desires` and needs the filter here:
        // a hidden collection's wants must stop shortening the badge.
        let (shopping_short,): (i64,) = sqlx::query_as(&format!(
            "WITH d AS ( \
               SELECT oracle_id, sum(quantity) AS desired_total FROM desires \
               WHERE {live} GROUP BY oracle_id \
             ), \
             o AS ( \
               SELECT oracle_id, owned FROM owned_by_card \
             ) \
             SELECT count(*) FROM d LEFT JOIN o ON o.oracle_id = d.oracle_id \
             WHERE d.desired_total > COALESCE(o.owned, 0)",
            live = in_live_collection("desires.collection_id"),
        ))
        .fetch_one(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        Ok(CollectionTree {
            collections: rows
                .into_iter()
                .map(|r| {
                    Ok(CollectionTreeRow {
                        summary: r.row.into_summary()?,
                        present: r.present,
                        desired: r.desired,
                    })
                })
                .collect::<ApiResult<Vec<_>>>()?,
            shopping_short,
        })
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

        // Parent must exist, be owned and be **live** — RLS makes a non-owned
        // parent invisible and `deleted_at IS NULL` makes a soft-deleted one
        // invisible too, so this EXISTS validates ownership, rejects a bad id
        // and refuses to nest a new collection under a hidden one.
        if let Some(parent_id) = req.parent_id {
            let exists: Option<(i32,)> =
                sqlx::query_as("SELECT 1 FROM collections WHERE id = $1 AND deleted_at IS NULL")
                    .bind(parent_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(upstream)?;
            if exists.is_none() {
                return Err(ApiError::NotFound("parent collection".into()));
            }
        }

        // Append after the current **live** siblings (max position + 1); a
        // hidden sibling is not on screen, so it should not push the new row
        // down past the end of the list the user can see.
        let row: CollectionRow = sqlx::query_as(&format!(
            "INSERT INTO collections (user_id, parent_id, kind, name, format, position) \
             VALUES ($1, $2, $3::collection_kind, $4, $5, \
                     COALESCE((SELECT max(position) FROM collections \
                               WHERE parent_id IS NOT DISTINCT FROM $2 \
                                 AND deleted_at IS NULL), 0) + 1) \
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
        // `deleted_at IS NULL` joins `NOT is_inbox` on every tree write below: a
        // hidden collection is not a write target, and a miss reads as absent
        // (NotFound) exactly as a non-existent id does.
        let updated: Option<CollectionRow> = sqlx::query_as(&format!(
            "UPDATE collections SET name = $2 \
             WHERE id = $1 AND NOT is_inbox AND deleted_at IS NULL \
             RETURNING {COLLECTION_COLS}"
        ))
        .bind(id)
        .bind(req.name.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        let summary = match updated {
            Some(row) => row.into_summary()?,
            None => return Err(self.absent_or_inbox(&mut tx, id, "renamed").await),
        };
        tx.commit().await.map_err(upstream)?;
        Ok(summary)
    }

    /// **Delete relocates; it does not destroy** (specs/collection-deletion.md,
    /// step 3 — the task that closes the data-loss hole).
    ///
    /// What used to be `DELETE FROM collections` — one statement whose blast
    /// radius was four `ON DELETE CASCADE`s and a ledger silently rewritten into
    /// intakes and removals — is now: read a snapshot, plan against it
    /// ([`plan_delete`]), then execute the plan in one transaction. The row is
    /// hidden last, so every write before it still sees a live collection.
    ///
    /// Read → plan → write, rather than deciding as it goes, because *what goes
    /// where* is all edge cases (top-level, no history, discard, a child as the
    /// destination) and none of them need a database to be right — that is the
    /// `plan_move`/`plan_drop` precedent, and the reason those rules are unit
    /// tests rather than dev-branch transcripts.
    async fn delete_collection(
        &self,
        req: DeleteCollectionReq,
    ) -> ApiResult<DeleteCollectionReceipt> {
        let user_id = self.session_id()?;
        let id = req.collection_id;
        let mut tx = self.scoped_tx().await?;

        // One read settles ownership (RLS), liveness (`deleted_at IS NULL`) and
        // the two facts the plan needs about the node itself. `FOR UPDATE` holds
        // it for the whole operation: everything below is keyed on this row
        // still being live and still having this parent.
        let row: Option<(Option<Uuid>, bool)> = sqlx::query_as(
            "SELECT parent_id, is_inbox FROM collections \
             WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        // Absent, not owned or already hidden — one answer, as everywhere else.
        let Some((parent_id, is_inbox)) = row else {
            return Err(ApiError::NotFound("collection".into()));
        };

        // Children survive, so only the **live** immediate children are read:
        // a grandchild keeps its parent, which is itself about to be re-pointed.
        let children: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM collections WHERE parent_id = $1 AND deleted_at IS NULL \
             ORDER BY position, name",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        // Per board, exactly as `teardown` snapshots: one ledger row per board
        // is what makes the undo exact (a 2-main/1-side stack must not come back
        // as 3 mainboard copies).
        let holdings: Vec<MoveGrainRow> = sqlx::query_as(
            "SELECT printing_id, finish::text AS finish, condition::text AS condition, \
                    language, board::text AS board, sum(quantity)::int AS quantity \
             FROM holdings WHERE collection_id = $1 \
             GROUP BY printing_id, finish, condition, language, board",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        let (has_desires,): (bool,) =
            sqlx::query_as("SELECT EXISTS (SELECT 1 FROM desires WHERE collection_id = $1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(upstream)?;

        // Resolved unconditionally (and provisioned if missing): it is the
        // fallback for both `ToParent` at top level and `ReturnToPrevious`
        // without history, and the planner is pure, so it cannot go looking.
        let inbox = inbox_id(&mut tx, user_id).await?;

        // `ReturnToPrevious` is the one disposition whose answer differs per
        // stack, so it is the only one that pays for these lookups.
        let mut previous = Vec::with_capacity(holdings.len());
        for h in &holdings {
            previous.push(match req.haves {
                HaveDisposition::ReturnToPrevious => {
                    previous_location(&mut tx, id, &grain_of(h)).await?
                }
                _ => None,
            });
        }

        let snapshot = DeleteSnapshot {
            collection_id: id,
            parent_id,
            is_inbox,
            inbox_id: inbox,
            children: children.into_iter().map(|(id,)| id).collect(),
            holdings: previous,
            has_desires,
        };
        let plan = plan_delete(&snapshot, req.haves, req.wants)?;

        // Every destination the plan produced — the user's `To` pick, the
        // parent, a previous location, the Inbox — re-validated as live and
        // owned before anything moves. Checking the *plan* rather than the
        // request is what makes "a hidden collection is never a write target"
        // hold for all four dispositions at once.
        for dest in plan.destinations() {
            require_owned_collection(&mut tx, dest).await?;
        }

        // 1. Children survive. Delete removes exactly one node.
        if !plan.reparent.is_empty() {
            sqlx::query(
                "UPDATE collections SET parent_id = $2 \
                 WHERE id = ANY($1) AND deleted_at IS NULL",
            )
            .bind(plan.reparent.as_slice())
            .bind(plan.reparent_to)
            .execute(&mut *tx)
            .await
            .map_err(upstream)?;
        }

        // 2. Holdings leave as real ledger moves — the same take/add/append
        //    triple `teardown` uses, so undo, history and every count treat a
        //    delete's relocations exactly like any other move. A `None`
        //    destination is `Discard`: the loop writes nothing at all.
        let mut move_ids: Vec<Id> = Vec::with_capacity(plan.moves());
        for (h, dest) in holdings.iter().zip(&plan.holding_dests) {
            let Some(dest) = *dest else { continue };
            let grain = grain_of(h);
            let board = board_of(&h.board)?;
            holding_take(&mut tx, id, &grain, board, h.quantity).await?;
            holding_add(&mut tx, user_id, dest, &grain, Board::Main, h.quantity).await?;
            move_ids.push(
                append_move(
                    &mut tx,
                    user_id,
                    Some(id),
                    Some(dest),
                    &grain,
                    (board, Board::Main),
                    h.quantity,
                )
                .await?,
            );
        }

        // 3. Desires, if they were asked to move. No ledger exists for them
        //    (desires have no move rows and no quantity operation), so this is a
        //    merge-and-drop: the destination may already want the same card, and
        //    `desires_uniq` is `NULLS NOT DISTINCT`, so an unpinned want merges
        //    with an unpinned want. Two source rows can never collide on the
        //    destination key, because they are distinct on it in the source too.
        //
        //    The source rows are read **before** the merge — the receipt's
        //    `desires` handles (maintainer ruling 2026-08-10: undo must fully
        //    reverse `WantDisposition::To`) are exactly this snapshot, since
        //    the merge-and-drop below is what makes them otherwise
        //    unrecoverable (no ledger, and the source rows are gone after the
        //    `DELETE`).
        let mut relocated_desires: Vec<RelocatedDesire> = Vec::new();
        if let Some(dest) = plan.desire_dest {
            let source: Vec<DesireGrainRow> = sqlx::query_as(
                "SELECT oracle_id, printing_id, board::text AS board, quantity \
                 FROM desires WHERE collection_id = $1",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;
            for row in &source {
                relocated_desires.push(RelocatedDesire {
                    to_collection_id: dest,
                    oracle_id: row.oracle_id,
                    printing_id: row.printing_id,
                    board: board_of(&row.board)?,
                    quantity: row.quantity,
                });
            }
            sqlx::query(
                "INSERT INTO desires (user_id, collection_id, oracle_id, printing_id, board, quantity) \
                 SELECT user_id, $2, oracle_id, printing_id, board, quantity \
                 FROM desires WHERE collection_id = $1 \
                 ON CONFLICT ON CONSTRAINT desires_uniq \
                   DO UPDATE SET quantity = desires.quantity + EXCLUDED.quantity",
            )
            .bind(id)
            .bind(dest)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            sqlx::query("DELETE FROM desires WHERE collection_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(upstream)?;
        }

        // 4. Hide the node — last, so every write above acted on a live
        //    collection and `require_owned_collection` could not have refused
        //    the operation's own subject.
        sqlx::query(
            "UPDATE collections SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(upstream)?;

        tx.commit().await.map_err(upstream)?;
        Ok(DeleteCollectionReceipt {
            collection_id: id,
            move_ids,
            reparented: plan.reparent,
            desires: relocated_desires,
        })
    }

    /// **Undo** — the misclick path, off the delete toast: reverse a delete
    /// *whole*, from its own receipt (specs/collection-deletion.md → step 5).
    /// Deliberately the stricter of the two recovery paths (see
    /// [`Self::restore_collection`] for the weaker one): a stale receipt is
    /// refused with an honest error rather than guessed at.
    ///
    /// **The receipt is client-held**, not server-derived or stashed — it is
    /// already handed to the caller by [`Self::delete_collection`]'s return
    /// value, so the toast just posts it back whole. Every id it names is
    /// re-validated here: RLS scopes every read/write in `tx` to the caller
    /// regardless, but a foreign or already-reused id still has to fail with a
    /// real error instead of a silent partial no-op — and a *crafted* receipt
    /// (adversarial review, `P6-190`) must not be able to write anything
    /// dangerous either; see the `Reparent` arm and [`validate_receipt`]
    /// below.
    ///
    /// Executes [`plan_undo`]'s steps **in order** — the plan *is* the
    /// execution, so "un-hide first" cannot drift out of sync between a pure
    /// description and what actually runs. `Unhide` additionally refuses a
    /// **stale receipt**: the collection must still be the caller's and still
    /// hidden (`require_deleted_collection`, `FOR UPDATE` — see its own doc
    /// for why), and if it had a parent, that parent must still be live —
    /// undo puts the collection back *exactly* where it was, and a parent
    /// soft-deleted in the interim is not a case undo can paper over the way
    /// [`Self::restore_collection`]'s top-level fallback does; it is the
    /// toast trying to reverse a world that no longer exists, so it refuses
    /// loudly instead.
    async fn undo_delete(&self, receipt: DeleteCollectionReceipt) -> ApiResult<()> {
        let user_id = self.session_id()?;
        // Pure, no I/O, checked before the transaction even opens: a receipt
        // with an invalid desire quantity must not write anything at all,
        // not fail partway through (adversarial review — see the function's
        // own doc for the sign-inversion this closes).
        validate_receipt(&receipt)?;
        let mut tx = self.scoped_tx().await?;
        // `reparent_to` — the restored collection's own current parent,
        // read once by `Unhide` (the first step `plan_undo` ever emits) and
        // reused by every `Reparent` step's cycle guard below. `None` until
        // `Unhide` runs, which it always does first.
        let mut reparent_to: Option<Uuid> = None;

        for step in plan_undo(&receipt) {
            match step {
                UndoStep::Unhide => {
                    require_deleted_collection(&mut tx, receipt.collection_id).await?;
                    let (parent_id,): (Option<Uuid>,) =
                        sqlx::query_as("SELECT parent_id FROM collections WHERE id = $1")
                            .bind(receipt.collection_id)
                            .fetch_one(&mut *tx)
                            .await
                            .map_err(upstream)?;
                    reparent_to = parent_id;
                    if let Some(pid) = parent_id {
                        // Only a genuinely-gone parent becomes the honest
                        // "try Restore instead" Conflict — a real Upstream/DB
                        // failure must propagate as itself, not be relabeled
                        // into a false "it was deleted" claim (adversarial
                        // review).
                        match require_owned_collection(&mut tx, pid).await {
                            Ok(()) => {}
                            Err(ApiError::NotFound(_)) => {
                                return Err(ApiError::Conflict(
                                    "its parent collection was deleted in the meantime; \
                                     undo can't put it back where it was — try Restore \
                                     instead"
                                        .into(),
                                ));
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    sqlx::query("UPDATE collections SET deleted_at = NULL WHERE id = $1")
                        .bind(receipt.collection_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(upstream)?;
                }
                UndoStep::UndoMove(move_id) => {
                    undo_one(&mut tx, user_id, move_id).await?;
                }
                UndoStep::Reparent(child_id) => {
                    // **The cycle guard** (adversarial review, `P6-190`,
                    // rounds 1 and 2): a crafted receipt could name
                    // `receipt.collection_id`'s own current parent here, or
                    // — the case round 1's guard missed — name
                    // `receipt.collection_id` itself, either of which would
                    // commit a cycle if applied blindly. `reparent_is_safe`
                    // takes both `child_id` and `receipt.collection_id`
                    // explicitly so it can refuse the self-parent shape by id
                    // equality before it ever asks whether the parents match
                    // — see its own doc for the full rule and why each half
                    // is sufficient. A guard failure refuses the **whole**
                    // undo rather than silently dropping this one step: the
                    // receipt no longer describes reality, and undo's
                    // contract is "put it back exactly, or say why not," the
                    // same rule the stale-parent check above follows.
                    let row: Option<(Option<Uuid>, bool)> =
                        sqlx::query_as("SELECT parent_id, is_inbox FROM collections WHERE id = $1")
                            .bind(child_id)
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(upstream)?;
                    let safe = match row {
                        Some((cur_parent, is_inbox)) => reparent_is_safe(
                            child_id,
                            cur_parent,
                            is_inbox,
                            receipt.collection_id,
                            reparent_to,
                        ),
                        None => false,
                    };
                    if !safe {
                        return Err(ApiError::Conflict(
                            "this receipt's re-parented collections no longer match what \
                             the delete actually moved; undo refuses rather than risking \
                             a cycle"
                                .into(),
                        ));
                    }
                    sqlx::query("UPDATE collections SET parent_id = $2 WHERE id = $1")
                        .bind(child_id)
                        .bind(receipt.collection_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(upstream)?;
                }
                UndoStep::RestoreDesire(d) => {
                    // Decrement the merge destination first (clamped — it may
                    // have changed since), then re-insert at the source. Mirrors
                    // `delete_collection`'s own merge-and-drop in reverse.
                    desire_take_clamp(
                        &mut tx,
                        d.to_collection_id,
                        d.oracle_id,
                        d.printing_id,
                        d.board,
                        d.quantity,
                    )
                    .await?;
                    sqlx::query(
                        "INSERT INTO desires \
                           (user_id, collection_id, oracle_id, printing_id, board, quantity) \
                         VALUES ($1, $2, $3, $4, $5::card_board, $6) \
                         ON CONFLICT ON CONSTRAINT desires_uniq \
                           DO UPDATE SET quantity = desires.quantity + EXCLUDED.quantity",
                    )
                    .bind(user_id)
                    .bind(receipt.collection_id)
                    .bind(d.oracle_id)
                    .bind(d.printing_id)
                    .bind(d.board.to_pg())
                    .bind(d.quantity)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
                }
            }
        }

        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    /// **Restore** — from the "Recently deleted" list, potentially days later:
    /// the weaker path, deliberately (specs/collection-deletion.md → step 5).
    /// Clears `deleted_at`, re-attaches to the original parent **if it is
    /// still live**, otherwise top-level — and leaves cards and children
    /// exactly where they now are. No move to reverse, no receipt to consume:
    /// restore does not know or care what the delete did to its contents,
    /// which is the whole point (a restore days later is not a time machine).
    async fn restore_collection(&self, id: Id) -> ApiResult<()> {
        let mut tx = self.scoped_tx().await?;
        require_deleted_collection(&mut tx, id).await?;

        // The parent it had when it was hidden — soft delete never touches a
        // collection's own `parent_id`, only its children's, so this is still
        // exactly where it was.
        let (parent_id,): (Option<Uuid>,) =
            sqlx::query_as("SELECT parent_id FROM collections WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(upstream)?;

        // Unlike undo, a dead parent here is the *expected* shape of a restore
        // used days later, not a surprise to refuse — `restore_parent` falls
        // back to the top level rather than erroring.
        let parent_is_live = match parent_id {
            Some(pid) => require_owned_collection(&mut tx, pid).await.is_ok(),
            None => false,
        };
        let new_parent = restore_parent(parent_id, parent_is_live);

        sqlx::query("UPDATE collections SET deleted_at = NULL, parent_id = $2 WHERE id = $1")
            .bind(id)
            .bind(new_parent)
            .execute(&mut *tx)
            .await
            .map_err(upstream)?;

        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    /// The "Recently deleted" list (specs/collection-deletion.md → step 5): the
    /// caller's own soft-deleted collections, newest first. Deliberately thin —
    /// no counts, no rows for what's inside them; it exists so a soft delete is
    /// reachable after its toast is gone.
    ///
    /// **The one documented, deliberate read of hidden collections in this
    /// file** (`soft_delete_guard`'s exemption below): every other read filters
    /// `deleted_at IS NULL`, but finding what's hidden is this list's entire
    /// job. RLS still scopes it to the caller — nothing here bypasses
    /// ownership, only liveness.
    async fn recently_deleted(&self) -> ApiResult<Vec<DeletedCollectionRow>> {
        let mut tx = self.scoped_tx().await?;
        // Bounded: the spec calls this "a small list", not a full trash
        // archive (no purge exists to keep it small on its own over months
        // of use) — 50 is generous for "reachable after the toast is gone"
        // and cheap insurance against an unbounded scan/payload either way.
        let rows: Vec<DeletedCollectionSql> = sqlx::query_as(
            "SELECT id, name, kind::text AS kind, \
                    to_char(deleted_at AT TIME ZONE 'UTC', \
                            'Mon DD, YYYY \"at\" HH12:MI AM \"UTC\"') AS deleted_at \
             FROM collections WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC LIMIT 50",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        rows.into_iter()
            .map(|r| {
                Ok(DeletedCollectionRow {
                    id: r.id,
                    name: r.name,
                    kind: CollectionKind::from_pg(&r.kind).ok_or_else(|| {
                        ApiError::Upstream(format!("unknown collection kind: {}", r.kind))
                    })?,
                    deleted_at: r.deleted_at,
                })
            })
            .collect()
    }

    async fn reparent_collection(&self, id: Id, req: Reparent) -> ApiResult<()> {
        let new_parent = req.new_parent_id;
        if new_parent == Some(id) {
            return Err(ApiError::Conflict(
                "a collection cannot be its own parent".into(),
            ));
        }
        let mut tx = self.scoped_tx().await?;

        // Node must exist / be owned / be live.
        let node: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM collections WHERE id = $1 AND deleted_at IS NULL")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(upstream)?;
        if node.is_none() {
            return Err(ApiError::NotFound("collection".into()));
        }

        if let Some(parent_id) = new_parent {
            // Parent must exist / be owned / be live.
            let parent: Option<(i32,)> =
                sqlx::query_as("SELECT 1 FROM collections WHERE id = $1 AND deleted_at IS NULL")
                    .bind(parent_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(upstream)?;
            if parent.is_none() {
                return Err(ApiError::NotFound("parent collection".into()));
            }
            // Cycle check: walk the target parent's ancestors; if `id` is among
            // them, moving `id` under it would create a cycle.
            //
            // Deliberately **not** filtered on `deleted_at`: this is a safety
            // guard, and walking hidden ancestors too can only reject more, never
            // fewer, cycles. Filtering would cut the chain at a hidden node and
            // let a cycle through it past the check — and by design the case
            // cannot arise anyway (deleting a collection re-parents its children,
            // so no live row keeps a hidden ancestor).
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

        // `NOT is_inbox`: the Inbox is pinned first at the top level (IA
        // invariant) — nesting it would defeat the pin, so it joins the
        // rename/delete protections. Found by the tree task's review: the
        // sidebar pins Inbox only among the roots.
        let affected = sqlx::query(
            "UPDATE collections SET parent_id = $2 \
             WHERE id = $1 AND NOT is_inbox AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(new_parent)
        .execute(&mut *tx)
        .await
        .map_err(upstream)?
        .rows_affected();
        if affected == 0 {
            return Err(self.absent_or_inbox(&mut tx, id, "reparented").await);
        }
        tx.commit().await.map_err(upstream)?;
        Ok(())
    }

    async fn reorder_collection(&self, id: Id, req: Reorder) -> ApiResult<()> {
        let mut tx = self.scoped_tx().await?;
        let affected = sqlx::query(
            "UPDATE collections SET position = $2 WHERE id = $1 AND deleted_at IS NULL",
        )
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

        // `to_board` is the board the copies actually landed on, not `main`:
        // a `+ Have` straight onto a sideboard is a real intake of a
        // sideboard stack, and an undo that took the copies off the mainboard
        // instead would remove copies the user never added.
        sqlx::query(
            "INSERT INTO moves \
               (user_id, printing_id, finish, condition, language, to_board, \
                from_collection_id, to_collection_id, quantity) \
             VALUES ($1, $2, $3::card_finish, $4::card_condition, $5, $6::card_board, \
                     NULL, $7, $8)",
        )
        .bind(user_id)
        .bind(req.printing_id)
        .bind(req.finish.to_pg())
        .bind(req.condition.to_pg())
        .bind(&req.language)
        .bind(req.board.to_pg())
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
        // The stepper addresses a holding by id, so the live-collection guard
        // has to ride on the row itself: a holding left attached to a hidden
        // collection is unreachable, and a stale id pointing at one reads as
        // absent rather than silently mutating invisible data.
        let live = in_live_collection("holdings.collection_id");
        if req.quantity <= 0 {
            let affected = sqlx::query(&format!("DELETE FROM holdings WHERE id = $1 AND {live}"))
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
            "UPDATE holdings SET quantity = $2 WHERE id = $1 AND {live} \
             RETURNING {HOLDING_COLS}"
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

    async fn collection_view(
        &self,
        id: Id,
        q: Option<String>,
        page: Page,
    ) -> ApiResult<CollectionView> {
        let mut tx = self.scoped_tx().await?;

        // Metadata (owned check via RLS, live check via `deleted_at`) +
        // immediate children. A soft-deleted collection 404s here exactly as a
        // non-existent one does, which is also what makes the per-collection
        // aggregates below safe to leave scoped to `$1` alone.
        let collection: CollectionRow = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS} FROM collections WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?
        .ok_or_else(|| ApiError::NotFound("collection".into()))?;
        let children: Vec<CollectionRow> = sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS} FROM collections \
             WHERE parent_id = $1 AND deleted_at IS NULL \
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
        //
        // `lines` is `held` **plus the desire-only rows** — the same correction
        // `all_cards` needed (specs/app-ui.md Findings, `/my`): a deck's whole
        // point is the cards it wants and does not have, and an inner join on
        // `holdings` made those invisible in the very view whose needs chip
        // counts them. A desire-only row has no held printing, so it borrows the
        // catalog's representative printing (has-art-first, lowest id) for its
        // art and identity; a card with no printings at all cannot be pictured
        // and drops out (the lateral is an inner join).
        //
        // `holding_id` is deliberately NULL unless exactly one `holdings` row
        // backs the cell: the stepper writes an absolute quantity, and a cell
        // summing a foil playset and a nonfoil single cannot say which grain a
        // typed `3` meant.
        let cursor: Option<CardCursor> = page.cursor.as_deref().map(decode_cursor).transpose()?;
        let limit = page.limit();
        let layouts = back_face_layout_list();
        let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> =
            sqlx::QueryBuilder::new("WITH RECURSIVE me AS (SELECT ");
        qb.push_bind(id);
        qb.push(
            "::uuid AS cid), \
             descendants AS ( \
               SELECT id FROM collections \
               WHERE parent_id = (SELECT cid FROM me) AND deleted_at IS NULL \
               UNION ALL \
               SELECT c.id FROM collections c JOIN descendants d ON c.parent_id = d.id \
               WHERE c.deleted_at IS NULL \
             ), \
             present AS ( \
               SELECT printing_id, board, sum(quantity)::int AS present, \
                      CASE WHEN count(*) = 1 THEN (array_agg(id))[1] END AS holding_id \
               FROM holdings WHERE collection_id = (SELECT cid FROM me) \
               GROUP BY printing_id, board \
             ), \
             want AS ( \
               SELECT oracle_id, board, sum(quantity)::int AS desired \
               FROM desires WHERE collection_id = (SELECT cid FROM me) \
               GROUP BY oracle_id, board \
             ), \
             rollup AS ( \
               SELECT printing_id, sum(quantity)::int AS present_rollup \
               FROM holdings WHERE collection_id IN (SELECT id FROM descendants) \
               GROUP BY printing_id \
             ), \
             held AS ( \
               SELECT p.oracle_id, pr.printing_id, pr.board, pr.present, pr.holding_id \
               FROM present pr JOIN printings p ON p.id = pr.printing_id \
             ), \
             lines AS ( \
               SELECT oracle_id, printing_id, board, present, holding_id FROM held \
               UNION ALL \
               SELECT w.oracle_id, rep.id, w.board, 0, NULL::uuid \
               FROM want w \
               JOIN LATERAL ( \
                 SELECT p.id FROM printings p WHERE p.oracle_id = w.oracle_id \
                 ORDER BY (COALESCE(p.image_uris->>'normal', \
                                    p.faces->0->'image_uris'->>'normal') IS NULL), p.id \
                 LIMIT 1 ) rep ON true \
               WHERE NOT EXISTS ( \
                 SELECT 1 FROM held h \
                 WHERE h.oracle_id = w.oracle_id AND h.board = w.board ) \
             ) \
             SELECT l.oracle_id, l.printing_id, ca.name, s.code AS set_code, \
                    p.collector_number, \
                    COALESCE(p.image_uris->>'normal', p.faces->0->'image_uris'->>'normal') \
                        AS image_uri, \
                    ca.mana_cost, ca.type_line, ca.colors, l.present, \
                    COALESCE(w.desired, 0) AS desired, COALESCE(o.owned, 0) AS owned, \
                    COALESCE(ro.present_rollup, 0) AS present_rollup, \
                    l.board::text AS board, l.holding_id, ca.layout, ",
        );
        qb.push(format!(
            "CASE WHEN ca.layout IN ({layouts}) THEN ca.card_faces END AS card_faces, "
        ));
        qb.push(
            "CASE WHEN p.faces IS NOT NULL THEN \
                 (SELECT array_agg(f->'image_uris'->>'normal' ORDER BY ord) \
                  FROM jsonb_array_elements(p.faces) WITH ORDINALITY AS t(f, ord)) \
             END AS face_image_uris \
             FROM lines l \
             JOIN printings p ON p.id = l.printing_id \
             JOIN cards ca ON ca.oracle_id = l.oracle_id \
             LEFT JOIN sets s ON s.id = p.set_id \
             LEFT JOIN owned_by_card o ON o.oracle_id = l.oracle_id \
             LEFT JOIN want w ON w.oracle_id = l.oracle_id AND w.board = l.board \
             LEFT JOIN rollup ro ON ro.printing_id = l.printing_id \
             WHERE true",
        );
        // The in-collection quick search: a plain name substring, not the
        // catalog grammar (design/information-architecture.md → "Two search
        // surfaces"), through the same escaping helper `/catalog` and `/my` use
        // so a typed `%` is literal in all three.
        if let Some(needle) = q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND ca.name ILIKE ");
            qb.push_bind(crate::search::sql::pattern(needle));
        }
        if let Some(c) = &cursor {
            qb.push(" AND (ca.name, l.printing_id, l.board) > (");
            qb.push_bind(c.name.clone());
            qb.push(", ");
            qb.push_bind(c.printing_id);
            qb.push(", ");
            qb.push_bind(c.board.clone());
            qb.push("::card_board)");
        }
        // Fetch one extra row to know whether a next page exists without a
        // phantom empty final fetch.
        qb.push(" ORDER BY ca.name, l.printing_id, l.board LIMIT ");
        qb.push_bind(limit + 1);
        let mut rows: Vec<CardRowSql> = qb
            .build_query_as()
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;

        let has_more = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = has_more
            .then(|| rows.last().map(CardRowSql::cursor))
            .flatten();
        let cards = rows
            .into_iter()
            .map(CardRowSql::into_row)
            .collect::<ApiResult<Vec<_>>>()?;

        // Whole-collection counts for the header + needs chip. Deliberately not
        // page-scoped (the card rows are): a header that changed as you paged
        // would be lying about the collection. Same tx as the page, so the two
        // cannot straddle a concurrent move. The needs halves reuse `needs`'
        // arithmetic exactly — gap = desired − present here, of which
        // `min(gap, held elsewhere)` is pullable — so the chip and the page it
        // links to cannot disagree.
        //
        // **Known limitation, inherited from `needs()` and deliberately kept
        // identical to it: this arithmetic is board-blind.** `d`/`ph` group by
        // oracle alone, while the card rows above group by `(oracle, board)`.
        // A deck holding a card on `main` and wanting it on `side` therefore
        // renders a Sideboard row reading WANTED 1 while the chip counts
        // nothing missing. Fixing it in this query alone would make the chip
        // disagree with the `/needs` page it links to, which is worse; the
        // honest fix is a board-aware `needs()` + a `board` on `NeedRow`, which
        // is collection-api's read model rather than this page's. Filed rather
        // than half-done here.
        //
        // Soft delete (specs/collection-deletion.md): `descendants` skips hidden
        // children, and `pe` — "held somewhere else" — skips holdings in hidden
        // collections, since a card the user cannot reach is not pullable. `d`
        // and `ph` are scoped to `$1`, which the metadata read above already
        // proved live, so they need no filter of their own.
        let totals: TotalsSql = sqlx::query_as(&format!(
            "WITH RECURSIVE descendants AS ( \
               SELECT id FROM collections WHERE parent_id = $1 AND deleted_at IS NULL \
               UNION ALL \
               SELECT c.id FROM collections c JOIN descendants d ON c.parent_id = d.id \
               WHERE c.deleted_at IS NULL \
             ), \
             d AS ( \
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
               WHERE h.collection_id <> $1 AND {live} GROUP BY p.oracle_id \
             ), \
             gap AS ( \
               SELECT (d.desired - COALESCE(ph.present_here, 0)) AS gap, \
                      COALESCE(pe.elsewhere, 0) AS elsewhere \
               FROM d LEFT JOIN ph ON ph.oracle_id = d.oracle_id \
               LEFT JOIN pe ON pe.oracle_id = d.oracle_id \
               WHERE d.desired > COALESCE(ph.present_here, 0) \
             ) \
             SELECT \
               (SELECT COALESCE(sum(quantity), 0)::int FROM holdings \
                WHERE collection_id = $1) AS present, \
               (SELECT COALESCE(sum(quantity), 0)::int FROM holdings \
                WHERE collection_id IN (SELECT id FROM descendants)) AS present_rollup, \
               (SELECT COALESCE(sum(quantity), 0)::int FROM desires \
                WHERE collection_id = $1) AS desired, \
               (SELECT COALESCE(sum(gap), 0)::int FROM gap) AS missing, \
               (SELECT COALESCE(sum(LEAST(gap, elsewhere)), 0)::int FROM gap) AS owned_elsewhere",
            live = in_live_collection("h.collection_id"),
        ))
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(upstream)?;

        let collection = collection.into_summary()?;
        // Decks carry their commanders in the same round trip (the header
        // renders them as a card); a binder never asks.
        let commanders = match collection.kind {
            CollectionKind::Deck => Some(commanders_in(&mut tx, id).await?),
            CollectionKind::Binder => None,
        };
        tx.commit().await.map_err(upstream)?;

        Ok(CollectionView {
            collection,
            children: children
                .into_iter()
                .map(CollectionRow::into_summary)
                .collect::<ApiResult<Vec<_>>>()?,
            cards,
            next_cursor,
            totals: totals.into_totals(),
            commanders,
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
            (req.from_board, req.to_board),
            req.quantity,
        )
        .await?;
        tx.commit().await.map_err(upstream)?;
        Ok(MoveReceipt { move_id })
    }

    async fn move_batch(&self, req: BatchMove) -> ApiResult<Vec<MoveReceipt>> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        // All-or-nothing: a failing item aborts the whole transaction. The
        // item's **position** rides on the error, because that is the only
        // thing that makes a whole-batch rollback diagnosable: the caller
        // built the list and can name the card at index `i`, while
        // `Conflict("no copies to move")` on its own names none of them (the
        // batch-move task's recorded defect).
        let mut receipts = Vec::with_capacity(req.items.len());
        for (i, item) in req.items.iter().enumerate() {
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
                (item.from_board, item.to_board),
                item.quantity,
            )
            .await
            .map_err(|e| at_item(i, e))?;
            receipts.push(MoveReceipt { move_id });
        }
        tx.commit().await.map_err(upstream)?;
        Ok(receipts)
    }

    async fn move_holding(&self, holding_id: Id, req: HoldingMove) -> ApiResult<MoveReceipt> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;

        // The whole point of this method: the grain, the board, the owning
        // collection and the quantity are read **inside the write
        // transaction**, and `FOR UPDATE` holds the row until it commits. A
        // caller that resolved them with a separate read would be doing
        // check-then-act across two transactions, which is the window the
        // batch-move task could not close from outside.
        let row: Option<HoldingRow> = sqlx::query_as(&format!(
            "SELECT {HOLDING_COLS} FROM holdings WHERE id = $1 AND {live} FOR UPDATE",
            live = in_live_collection("holdings.collection_id"),
        ))
        .bind(holding_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(upstream)?;
        // RLS scopes `holdings` to the caller, so another user's row is
        // indistinguishable from a deleted one — both are `NotFound`.
        let line = row
            .ok_or_else(|| ApiError::NotFound("holding".into()))?
            .into_line()?;

        let quantity = req.quantity.unwrap_or(line.quantity);
        if quantity <= 0 {
            return Err(ApiError::Validation("quantity must be > 0".into()));
        }
        if quantity > line.quantity {
            return Err(ApiError::Conflict("insufficient copies to move".into()));
        }
        let grain = Grain {
            printing_id: line.printing_id,
            finish: line.finish.to_pg().to_string(),
            condition: line.condition.to_pg().to_string(),
            language: line.language.clone(),
        };
        // The copies leave the board they are on and land on the mainboard:
        // moved out of a sideboard into a binder they are simply copies, and
        // re-labelling a board is card-tagging's op. Undo reverses exactly
        // this, which is what puts a removed sideboard stack back where it was.
        let move_id = apply_move(
            &mut tx,
            user_id,
            Some(line.collection_id),
            req.to_collection_id,
            &grain,
            (line.board, Board::Main),
            quantity,
        )
        .await?;
        tx.commit().await.map_err(upstream)?;
        Ok(MoveReceipt { move_id })
    }

    async fn undo_move(&self, move_id: Id) -> ApiResult<UndoReceipt> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        let restored_holding_id = undo_one(&mut tx, user_id, move_id).await?;
        tx.commit().await.map_err(upstream)?;
        Ok(UndoReceipt {
            restored_holding_id,
        })
    }

    async fn undo_moves(&self, move_ids: Vec<Id>) -> ApiResult<()> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        // One transaction for the whole list: a batch move is applied
        // all-or-nothing, so its undo has to revert the same way. A per-id loop
        // of `undo_move` would commit each reversal separately and could stop
        // half way, leaving the user with a half-undone batch behind a toast
        // that claimed the batch was undone.
        for move_id in move_ids {
            undo_one(&mut tx, user_id, move_id).await?;
        }
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

    async fn holdings_of_oracle(&self, oracle_id: Id) -> ApiResult<Vec<HoldingLine>> {
        // Ungrouped on purpose — see the trait doc. RLS scopes the rows to the
        // caller, the same way every other read in this impl is scoped.
        let mut tx = self.scoped_tx().await?;
        let rows: Vec<HoldingRow> = sqlx::query_as(&format!(
            "SELECT h.id, h.collection_id, h.printing_id, h.finish::text AS finish, \
                    h.condition::text AS condition, h.language, h.board::text AS board, \
                    h.quantity \
             FROM holdings h JOIN printings p ON p.id = h.printing_id \
             WHERE p.oracle_id = $1 AND {live} \
             ORDER BY h.collection_id, h.printing_id, h.board, h.finish",
            live = in_live_collection("h.collection_id"),
        ))
        .bind(oracle_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;
        rows.into_iter().map(HoldingRow::into_line).collect()
    }

    async fn suggested_destinations(&self, oracle_id: Id) -> ApiResult<Vec<SuggestedDestination>> {
        let mut tx = self.scoped_tx().await?;
        // Both CTEs carry their own live filter (specs/collection-deletion.md is
        // explicit that these per-collection aggregations are not covered by the
        // `owned_by_card` view's one filter): a hidden collection must never be
        // offered as a destination, and its copies must not count as already
        // present there.
        let rows: Vec<SuggestedRow> = sqlx::query_as(&format!(
            "WITH d AS ( \
               SELECT collection_id, sum(quantity)::int AS desired \
               FROM desires WHERE oracle_id = $1 AND {live_d} GROUP BY collection_id \
             ), \
             p AS ( \
               SELECT h.collection_id, sum(h.quantity)::int AS present \
               FROM holdings h JOIN printings pr ON pr.id = h.printing_id \
               WHERE pr.oracle_id = $1 AND {live_p} GROUP BY h.collection_id \
             ) \
             SELECT c.id AS collection_id, c.name AS collection_name, d.desired, \
                    COALESCE(p.present, 0) AS present \
             FROM d JOIN collections c ON c.id = d.collection_id \
             LEFT JOIN p ON p.collection_id = c.id \
             WHERE d.desired > COALESCE(p.present, 0) \
             ORDER BY (d.desired - COALESCE(p.present, 0)) DESC, c.name",
            live_d = in_live_collection("desires.collection_id"),
            live_p = in_live_collection("h.collection_id"),
        ))
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

        // Snapshot every holding in the collection **per board**. Teardown
        // empties all of them (a deck's sideboard leaves with the rest), and
        // one ledger row per board is what makes the undo exact: summing the
        // boards into one row and relocating that would put a 2-main/1-side
        // stack back as 3 mainboard copies.
        let holdings: Vec<MoveGrainRow> = sqlx::query_as(
            "SELECT printing_id, finish::text AS finish, condition::text AS condition, \
                    language, board::text AS board, sum(quantity)::int AS quantity \
             FROM holdings WHERE collection_id = $1 \
             GROUP BY printing_id, finish, condition, language, board",
        )
        .bind(collection_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        let inbox = match &mode {
            Teardown::ReturnToPrevious => Some(inbox_id(&mut tx, user_id).await?),
            Teardown::EmptyTo { .. } => None,
        };

        let mut move_ids: Vec<Id> = Vec::with_capacity(holdings.len());
        for h in &holdings {
            let grain = grain_of(h);
            let board = board_of(&h.board)?;
            let dest = match &mode {
                Teardown::EmptyTo { to_collection_id } => *to_collection_id,
                // Board-agnostic on purpose: collection-api specifies the
                // lookup "per printing/finish", and where a card *came from* is
                // a collection, not a board. Matching the board too would send
                // a sideboarded copy to the Inbox merely because it was
                // main-boarded on the way in.
                Teardown::ReturnToPrevious => previous_location(&mut tx, collection_id, &grain)
                    .await?
                    .unwrap_or_else(|| inbox.expect("inbox resolved for ReturnToPrevious")),
            };
            holding_take(&mut tx, collection_id, &grain, board, h.quantity).await?;
            holding_add(&mut tx, user_id, dest, &grain, Board::Main, h.quantity).await?;
            let move_id = append_move(
                &mut tx,
                user_id,
                Some(collection_id),
                Some(dest),
                &grain,
                (board, Board::Main),
                h.quantity,
            )
            .await?;
            move_ids.push(move_id);
        }
        tx.commit().await.map_err(upstream)?;
        Ok(TeardownReceipt { move_ids })
    }

    async fn all_cards(&self, q: Option<String>, page: Page) -> ApiResult<AllCardsView> {
        let mut tx = self.scoped_tx().await?;
        let cursor: Option<OracleCursor> = page.cursor.as_deref().map(decode_cursor).transpose()?;
        let limit = page.limit();

        // `mine` is a FULL OUTER JOIN, not an inner one: a card you *want* but
        // hold nowhere still belongs in the everything-view (it is what the
        // shopping list is made of, and the collection view lists desired rows
        // the same way). Such a row is owned 0 / no locations / wanted N.
        //
        // Neither CTE filters by user — `wanted` reads the RLS-scoped `desires`
        // table directly, `held` reads the security-invoker `owned_by_card`
        // view (itself over RLS-scoped `holdings`/`collections` — the one
        // shared "owned per card" source, specs/collection-api.md Findings),
        // and this all runs inside `scoped_tx`, so the caller's own rows are
        // all that exist here. The catalog joins (`cards`, `printings`) are
        // unscoped by design.
        let base = format!(
            "WITH held AS ( \
               SELECT oracle_id, owned FROM owned_by_card \
             ), \
             wanted AS ( \
               SELECT oracle_id, sum(quantity)::int AS wanted \
               FROM desires WHERE {live_wanted} GROUP BY oracle_id \
             ), \
             mine AS ( \
               SELECT COALESCE(held.oracle_id, wanted.oracle_id) AS oracle_id, \
                      COALESCE(held.owned, 0) AS owned, \
                      COALESCE(wanted.wanted, 0) AS wanted \
               FROM held FULL OUTER JOIN wanted ON wanted.oracle_id = held.oracle_id \
             ) {select} JOIN mine ON mine.oracle_id = c.oracle_id{rep}WHERE true",
            select = summary_select_with(", mine.owned, mine.wanted"),
            rep = REPRESENTATIVE_PRINTING_JOIN,
            // `held` needs no filter — `owned_by_card` carries it (migration
            // 0010) — but `wanted` is this query's own aggregation over
            // `desires`, so a hidden collection's wants would otherwise keep a
            // card in the everything-view with `owned 0 / wanted N`.
            live_wanted = in_live_collection("desires.collection_id"),
        );
        let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> = sqlx::QueryBuilder::new(base);
        // Quick search: a plain name substring, not the catalog grammar (see the
        // trait doc). Same escaping helper as `/catalog`'s bare terms, so a
        // typed `%` is literal in both places.
        if let Some(needle) = q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            qb.push(" AND c.name ILIKE ");
            qb.push_bind(crate::search::sql::pattern(needle));
        }
        if let Some(c) = &cursor {
            qb.push(" AND (c.name, c.oracle_id) > (");
            qb.push_bind(c.name.clone());
            qb.push(", ");
            qb.push_bind(c.oracle_id);
            qb.push(")");
        }
        qb.push(" ORDER BY c.name, c.oracle_id LIMIT ");
        qb.push_bind(limit + 1);
        let mut rows: Vec<AllCardsRowSql> = qb
            .build_query_as()
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;

        let has_more = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = has_more
            .then(|| {
                rows.last().map(|r| {
                    encode_cursor(&OracleCursor {
                        name: r.card.name.clone(),
                        oracle_id: r.card.oracle_id,
                    })
                })
            })
            .flatten();

        // Where the copies actually are — one query for the whole page, grouped
        // in Rust (the `needs` view's shape, minus its exclude-here filter).
        // Same transaction as the totals above, so `owned` and the sum of a
        // row's locations cannot disagree across a concurrent move.
        let oracles: Vec<Uuid> = rows.iter().map(|r| r.card.oracle_id).collect();
        let locs: Vec<LocationSql> = sqlx::query_as(
            "SELECT p.oracle_id, h.collection_id, c.name AS collection_name, \
                    sum(h.quantity)::int AS quantity \
             FROM holdings h JOIN printings p ON p.id = h.printing_id \
             JOIN collections c ON c.id = h.collection_id \
             WHERE p.oracle_id = ANY($1) AND c.deleted_at IS NULL \
             GROUP BY p.oracle_id, h.collection_id, c.name \
             ORDER BY quantity DESC, c.name",
        )
        .bind(&oracles)
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;
        tx.commit().await.map_err(upstream)?;

        Ok(AllCardsView {
            cards: rows
                .into_iter()
                .map(|r| {
                    let locations = locs
                        .iter()
                        .filter(|l| l.oracle_id == r.card.oracle_id)
                        .map(|l| CardLocation {
                            collection_id: l.collection_id,
                            collection_name: l.collection_name.clone(),
                            quantity: l.quantity,
                        })
                        .collect();
                    AllCardsRow {
                        wanted: r.wanted,
                        card: r.card.into_summary(Some(r.owned)),
                        locations,
                    }
                })
                .collect(),
            next_cursor,
        })
    }

    async fn needs(&self, collection_id: Id) -> ApiResult<NeedsView> {
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, collection_id).await?;
        let rows = read_needs_rows(&mut tx, collection_id).await?;
        tx.commit().await.map_err(upstream)?;
        Ok(NeedsView {
            collection_id,
            rows,
        })
    }

    /// **Pull** — see the trait doc for what closed. Read → plan → write in
    /// one transaction, following [`Self::delete_collection`]'s own precedent:
    /// [`pull_plan::plan_pull_needs`] is pure and does all the deciding, this
    /// method only gathers the snapshot it needs (locked) and executes what
    /// comes back.
    async fn pull_needs(
        &self,
        to_collection_id: Id,
        items: Vec<PullItem>,
    ) -> ApiResult<PullOutcome> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        require_owned_collection(&mut tx, to_collection_id).await?;

        // 1. Lock every source stack the plan might draw from **first**, one
        //    oracle at a time in **canonical (sorted) order** — never the
        //    caller's item order. Caller-order locking is exactly the bug
        //    P6-114 found (and deliberately left unfixed) in `move_batch`:
        //    two concurrent calls whose item lists overlap but arrive in
        //    opposite order can each hold one row the other wants. Sorting
        //    costs nothing extra here — this transaction enumerates the ids
        //    anyway — so it is worth doing for this one operation without
        //    building the general cross-operation ordering P6-114 itself
        //    declined to build.
        let mut holdings: HashMap<Uuid, Vec<HoldingLine>> = HashMap::new();
        for oracle_id in pull_plan::oracle_ids_of(&items) {
            // Mirrors `holdings_of_oracle`'s own query (full breadth across
            // every live collection — `plan_pull` filters to the named
            // source), with `FOR UPDATE OF h` added so only the `holdings`
            // row itself is locked, never the joined `printings` catalog row.
            // Columns are qualified (`h.…`) rather than reusing `HOLDING_COLS`
            // — that constant's bare `id` is ambiguous once `printings` (which
            // also has an `id`) is joined in.
            //
            // **This bulk-locks every live holdings row of the oracle**, not
            // just the specific `from_collection_id` an item names — wider
            // than the row this transaction will actually decrement (see the
            // Findings entry this task added to specs/collection-api.md for
            // the tradeoff: it is what makes two overlapping pulls sharing an
            // oracle id serialize against each other below, at the cost of a
            // wider blocking surface against unrelated `move_batch` calls
            // touching the same card).
            let rows: Vec<HoldingRow> = sqlx::query_as(&format!(
                "SELECT h.id, h.collection_id, h.printing_id, h.finish::text AS finish, \
                        h.condition::text AS condition, h.language, h.board::text AS board, \
                        h.quantity \
                 FROM holdings h JOIN printings p ON p.id = h.printing_id \
                 WHERE p.oracle_id = $1 AND {live} \
                 ORDER BY h.collection_id, h.printing_id, h.board, h.finish, h.condition, h.language \
                 FOR UPDATE OF h",
                live = in_live_collection("h.collection_id"),
            ))
            .bind(oracle_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(upstream)?;
            let lines: ApiResult<Vec<HoldingLine>> =
                rows.into_iter().map(HoldingRow::into_line).collect();
            holdings.insert(oracle_id, lines?);
        }

        // 2. **Only now** read the destination's fresh needs — the exact
        //    read `needs()` makes, in this same transaction instead of a
        //    separate one, and deliberately run *after* every lock above
        //    rather than before it. Two overlapping pulls that share an
        //    oracle id contend for the same rows locked in step 1 (that
        //    query is bulk over the whole oracle, not filtered to one
        //    source — see the comment above), so whichever pull is second
        //    blocks there until the first commits; only then does *this*
        //    read run, and it sees the first pull's already-updated
        //    `present_here`/gap. Reading needs before the lock loop (the
        //    first draft of this method) let two overlapping pulls each plan
        //    off the *same* stale gap and overshoot `desired` once both had
        //    committed — this ordering is what actually closes that, not
        //    merely "one transaction."
        let needs = read_needs_rows(&mut tx, to_collection_id).await?;

        // 3. Plan — pure, over the needs read above and exactly what step 1
        //    locked.
        let snapshot = pull_plan::PullSnapshot { needs, holdings };
        let plan = pull_plan::plan_pull_needs(to_collection_id, &snapshot, items);

        // 4. Write every planned line — the same take/add/append triple
        //    `delete_collection`/`teardown` use, so a pull's moves are
        //    ordinary ledger rows. Each source row is already locked by step
        //    1, so `holding_take`'s own `FOR UPDATE` re-acquires instantly.
        //    **Not routed through `apply_move`**, unlike every other move
        //    site in this file — so it does not inherit that function's
        //    `quantity > 0`, `from != to` and ownership guards. All three are
        //    provably unreachable via this path today (`plan_pull` never
        //    emits a non-positive quantity; the `AlreadyThere` skip in
        //    `plan_pull_needs` already refuses `from == to_collection_id`
        //    before a write is ever planned; `from` and `to_collection_id`
        //    are both RLS-scoped/`require_owned_collection`-checked reads),
        //    but a guard added to `apply_move` in the future will not
        //    automatically cover this loop too.
        let mut move_ids: Vec<Id> = Vec::with_capacity(plan.writes.len());
        for (i, write) in plan.writes.iter().enumerate() {
            let grain = Grain::from(&write.source);
            holding_take(
                &mut tx,
                write.source.from,
                &grain,
                write.source.board,
                write.quantity,
            )
            .await
            .map_err(|e| at_item(i, e))?;
            holding_add(
                &mut tx,
                user_id,
                to_collection_id,
                &grain,
                Board::Main,
                write.quantity,
            )
            .await
            .map_err(|e| at_item(i, e))?;
            move_ids.push(
                append_move(
                    &mut tx,
                    user_id,
                    Some(write.source.from),
                    Some(to_collection_id),
                    &grain,
                    (write.source.board, Board::Main),
                    write.quantity,
                )
                .await
                .map_err(|e| at_item(i, e))?,
            );
        }

        tx.commit().await.map_err(upstream)?;
        Ok(PullOutcome {
            move_ids,
            pulled: plan.pulled,
            skipped: plan.skipped,
        })
    }

    async fn shopping_list(&self) -> ApiResult<ShoppingList> {
        let mut tx = self.scoped_tx().await?;
        // `o` reads the `owned_by_card` view — see the same note in
        // `all_cards`/`collection_tree` (specs/collection-api.md Findings): one
        // shared "owned per card" source, not a re-derived sum here, and the
        // place the soft-delete filter for `owned` lives. `d` is this query's
        // own aggregation over `desires` and carries its own.
        let rows: Vec<ShoppingSql> = sqlx::query_as(&format!(
            "WITH d AS ( \
               SELECT oracle_id, sum(quantity)::int AS desired_total FROM desires \
               WHERE {live} GROUP BY oracle_id \
             ), \
             o AS ( \
               SELECT oracle_id, owned FROM owned_by_card \
             ) \
             SELECT d.oracle_id, c.name, d.desired_total, COALESCE(o.owned, 0) AS owned \
             FROM d JOIN cards c ON c.oracle_id = d.oracle_id \
             LEFT JOIN o ON o.oracle_id = d.oracle_id \
             WHERE d.desired_total > COALESCE(o.owned, 0) \
             ORDER BY c.name",
            live = in_live_collection("desires.collection_id"),
        ))
        .fetch_all(&mut *tx)
        .await
        .map_err(upstream)?;

        let oracles: Vec<Uuid> = rows.iter().map(|r| r.oracle_id).collect();
        let wants: Vec<WantedBySql> = sqlx::query_as(
            "SELECT de.oracle_id, c.name AS collection_name \
             FROM desires de JOIN collections c ON c.id = de.collection_id \
             WHERE de.oracle_id = ANY($1) AND c.deleted_at IS NULL \
             GROUP BY de.oracle_id, c.name ORDER BY c.name",
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
                    (SELECT COALESCE(image_uris->>'normal', \
                                     faces->0->'image_uris'->>'normal') FROM printings \
                     WHERE oracle_id = c.oracle_id \
                       AND COALESCE(image_uris->>'normal', \
                                    faces->0->'image_uris'->>'normal') IS NOT NULL \
                     ORDER BY id LIMIT 1) AS image_uri \
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
        let out = commanders_in(&mut tx, collection_id).await?;
        tx.commit().await.map_err(upstream)?;
        Ok(out)
    }

    async fn set_holding_board(&self, holding_id: Id, req: SetBoard) -> ApiResult<()> {
        let user_id = self.session_id()?;
        let mut tx = self.scoped_tx().await?;
        let row: HoldingBoardSql = sqlx::query_as(
            "SELECT h.collection_id, h.printing_id, h.finish::text AS finish, \
                    h.condition::text AS condition, h.language, h.board::text AS board, \
                    h.quantity, col.kind::text AS kind \
             FROM holdings h JOIN collections col ON col.id = h.collection_id \
             WHERE h.id = $1 AND col.deleted_at IS NULL",
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
             WHERE d.id = $1 AND col.deleted_at IS NULL",
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
/// labels as text, cast in SQL).
///
/// **Board is not part of it, deliberately.** A move's two ends can sit on
/// different boards — copies leaving a deck's sideboard land in a binder as
/// ordinary copies — so a board travels beside a grain as its own argument,
/// once per end, and the `moves` ledger records both (migration 0009). Folding
/// it into `Grain` would have made "which end is this?" unanswerable at every
/// call site.
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

/// The grain a [`pull_plan::PullWrite`]'s source stack was found at — the
/// exact stack [`pull_plan::plan_pull_needs`] chose, never a restated default.
impl From<&MoveSource> for Grain {
    fn from(s: &MoveSource) -> Self {
        Grain {
            printing_id: s.printing_id,
            finish: s.finish.to_pg().to_string(),
            condition: s.condition.to_pg().to_string(),
            language: s.language.clone(),
        }
    }
}

/// Perform one move within an open transaction: validate, decrement the source
/// holding on `from_board`, upsert the destination on `to_board`, append the
/// ledger row. Returns the new move id.
async fn apply_move(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    from: Option<Id>,
    to: Option<Id>,
    grain: &Grain,
    boards: (Board, Board),
    quantity: i32,
) -> ApiResult<Uuid> {
    let (from_board, to_board) = boards;
    if quantity <= 0 {
        return Err(ApiError::Validation("quantity must be > 0".into()));
    }
    if from.is_none() && to.is_none() {
        return Err(ApiError::Validation(
            "a move needs a source or destination".into(),
        ));
    }
    if from.is_some() && from == to {
        // Same collection, whatever the boards: relabelling a board in place is
        // card-tagging's quantity-preserving op (`set_holding_board`), not a
        // move, and the ledger would otherwise carry a row that undoes to a
        // no-op.
        return Err(ApiError::Validation(
            "source and destination are the same".into(),
        ));
    }
    if let Some(from) = from {
        require_owned_collection(tx, from).await?;
        holding_take(tx, from, grain, from_board, quantity).await?;
    }
    if let Some(to) = to {
        require_owned_collection(tx, to).await?;
        holding_add(tx, user_id, to, grain, to_board, quantity).await?;
    }
    append_move(tx, user_id, from, to, grain, boards, quantity).await
}

/// Reverse one move and stamp `undone_at`, returning the id of the holding
/// row its copies landed back on — `None` when there is no such holding to
/// name: the move had no origin collection, the move was already undone (the
/// idempotent no-op case — nothing is written, so nothing restored), or the
/// origin has since been soft-deleted and the copies redirected to the Inbox
/// instead (see `shared::UndoReceipt`).
async fn undo_one(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    move_id: Id,
) -> ApiResult<Option<Id>> {
    let m: Option<MoveRow> = sqlx::query_as(
        "SELECT printing_id, finish::text AS finish, condition::text AS condition, language, \
                from_board::text AS from_board, to_board::text AS to_board, \
                from_collection_id, to_collection_id, quantity, (undone_at IS NOT NULL) AS undone \
         FROM moves WHERE id = $1",
    )
    .bind(move_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    let m = m.ok_or_else(|| ApiError::NotFound("move".into()))?;
    if m.undone {
        return Ok(None); // idempotent — nothing written, so nothing restored
    }
    let grain = Grain {
        printing_id: m.printing_id,
        finish: m.finish,
        condition: m.condition,
        language: m.language,
    };
    // The boards are read back from the ledger rather than assumed: a removal
    // taken off a sideboard must return to the *sideboard*, and returning it to
    // the mainboard would be a silent data change dressed as an undo.
    let from_board = board_of(&m.from_board)?;
    let to_board = board_of(&m.to_board)?;
    // Reverse: give the copies back to the source, take them from the dest.
    //
    // **The source may since have been soft-deleted**, and this is the one write
    // path in the file that never goes through `require_owned_collection`,
    // because its ids come from the ledger rather than from the caller. Undoing
    // an old, unrelated move whose source is now hidden would otherwise put real
    // copies somewhere the user cannot see.
    //
    // Maintainer ruling, 2026-08-09 (specs/collection-deletion.md → Open
    // questions, now resolved): **redirect them to the Inbox.** Cards always
    // come back, and they never silently land in a hidden collection. Refusing
    // the undo and auto-restoring the collection were both considered and
    // rejected. The delete's *own* undo is unaffected — it clears `deleted_at`
    // before reversing its moves, so the source is live again by the time this
    // runs.
    // Named only when the copies actually landed back at `from` — a redirect
    // (below) puts them somewhere real but not where the caller's own
    // `from_collection_id` says, and naming that holding would let a caller
    // still rendering the *original* collection's row (the collection-view
    // stepper) rewire itself onto an Inbox holding through a row that no
    // longer describes it.
    let mut restored_holding_id = None;
    if let Some(from) = m.from_collection_id {
        let resolved = live_or_inbox(tx, user_id, from).await?;
        let holding_id = holding_add(tx, user_id, resolved, &grain, from_board, m.quantity).await?;
        if resolved == from {
            restored_holding_id = Some(holding_id);
        }
    }
    if let Some(to) = m.to_collection_id {
        holding_take_clamp(tx, to, &grain, to_board, m.quantity).await?;
    }
    sqlx::query("UPDATE moves SET undone_at = now() WHERE id = $1")
        .bind(move_id)
        .execute(&mut **tx)
        .await
        .map_err(upstream)?;
    Ok(restored_holding_id)
}

/// Upsert `+delta` into a collection's holding for the grain **on `board`**,
/// returning the row's id — the row that already existed, or the one this
/// insert just created. `undo_one` is the caller that needs it: a removal's
/// undo re-inserts a fresh holding, and the caller addressing it by id (the
/// collection-view stepper) has to be told which id that is.
async fn holding_add(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    collection_id: Id,
    grain: &Grain,
    board: Board,
    delta: i32,
) -> ApiResult<Id> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO holdings \
           (user_id, collection_id, printing_id, finish, condition, language, board, quantity) \
         VALUES ($1, $2, $3, $4::card_finish, $5::card_condition, $6, $7::card_board, $8) \
         ON CONFLICT ON CONSTRAINT holdings_uniq \
           DO UPDATE SET quantity = holdings.quantity + EXCLUDED.quantity \
         RETURNING id",
    )
    .bind(user_id)
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .bind(board.to_pg())
    .bind(delta)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(id)
}

/// Remove exactly `need` copies from a collection's holding on `board`; errors
/// `Conflict` if fewer are present. Deletes the row at zero (the CHECK forbids 0).
async fn holding_take(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
    board: Board,
    need: i32,
) -> ApiResult<()> {
    let cur: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, quantity FROM holdings \
         WHERE collection_id = $1 AND printing_id = $2 AND finish = $3::card_finish \
           AND condition = $4::card_condition AND language = $5 AND board = $6::card_board \
         FOR UPDATE",
    )
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .bind(board.to_pg())
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

/// Best-effort removal for undo: take up to `want` from the holding on `board`,
/// clamping to what's there (the dest may have changed since the move).
async fn holding_take_clamp(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
    board: Board,
    want: i32,
) -> ApiResult<()> {
    let cur: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, quantity FROM holdings \
         WHERE collection_id = $1 AND printing_id = $2 AND finish = $3::card_finish \
           AND condition = $4::card_condition AND language = $5 AND board = $6::card_board \
         FOR UPDATE",
    )
    .bind(collection_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .bind(board.to_pg())
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

/// Best-effort desire decrement for undo: take up to `want` off a collection's
/// desire row — identified the way `desires_uniq` identifies it
/// (`collection_id`, `oracle_id`, `printing_id`, `board`) — clamping to what's
/// there. Mirrors [`holding_take_clamp`]'s reasoning exactly, on the desires
/// side: the merge destination's want may have changed since the delete that
/// merged into it, so undo takes back at most what remains rather than
/// erroring or going negative.
async fn desire_take_clamp(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    oracle_id: Id,
    printing_id: Option<Id>,
    board: Board,
    want: i32,
) -> ApiResult<()> {
    let cur: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, quantity FROM desires \
         WHERE collection_id = $1 AND oracle_id = $2 AND printing_id IS NOT DISTINCT FROM $3 \
           AND board = $4::card_board \
         FOR UPDATE",
    )
    .bind(collection_id)
    .bind(oracle_id)
    .bind(printing_id)
    .bind(board.to_pg())
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    let Some((id, qty)) = cur else { return Ok(()) };
    if qty <= want {
        sqlx::query("DELETE FROM desires WHERE id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    } else {
        sqlx::query("UPDATE desires SET quantity = quantity - $2 WHERE id = $1")
            .bind(id)
            .bind(want)
            .execute(&mut **tx)
            .await
            .map_err(upstream)?;
    }
    Ok(())
}

/// Append a `moves` ledger row and return its id. `boards` is
/// `(from_board, to_board)` — the two ends, recorded separately so undo can put
/// the copies back on the board they left.
async fn append_move(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    from: Option<Id>,
    to: Option<Id>,
    grain: &Grain,
    boards: (Board, Board),
    quantity: i32,
) -> ApiResult<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO moves \
           (user_id, printing_id, finish, condition, language, from_board, to_board, \
            from_collection_id, to_collection_id, quantity) \
         VALUES ($1, $2, $3::card_finish, $4::card_condition, $5, $6::card_board, \
                 $7::card_board, $8, $9, $10) RETURNING id",
    )
    .bind(user_id)
    .bind(grain.printing_id)
    .bind(&grain.finish)
    .bind(&grain.condition)
    .bind(&grain.language)
    .bind(boards.0.to_pg())
    .bind(boards.1.to_pg())
    .bind(from)
    .bind(to)
    .bind(quantity)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(id)
}

/// Parse a `card_board` label read back from Postgres. An unknown label means
/// the enum and this code have drifted, which is an upstream fault, not a
/// silent `main`.
fn board_of(label: &str) -> ApiResult<Board> {
    Board::from_pg(label).ok_or_else(|| ApiError::Upstream(format!("unknown board: {label}")))
}

/// Attribute a batch-move failure to the item at `index`, preserving the
/// variant so the status code is still the right one — only the message grows
/// the position the caller needs to name a card (`shared::batch_item_error`).
/// Used by [`HostedBackend::move_batch`] and, since P6-120,
/// [`HostedBackend::pull_needs`]'s write loop (`index` there is the position
/// in [`pull_plan::PullPlan::writes`], the same numbering the pre-P6-120
/// composition's `move_batch` call already tagged errors by).
fn at_item(index: usize, e: ApiError) -> ApiError {
    use ApiError::*;
    let tag = |m: String| shared::batch_item_error(index, &m);
    match e {
        Conflict(m) => Conflict(tag(m)),
        Validation(m) => Validation(tag(m)),
        NotFound(m) => NotFound(tag(m)),
        Forbidden(m) => Forbidden(tag(m)),
        // Unauthorized / Upstream are not one item's fault — leave them alone
        // rather than blaming whichever item happened to be in flight.
        other => other,
    }
}

/// The most-recent **live** collection this card was moved *into* the given
/// collection from (for teardown "return to previous"), or `None` if there's no
/// such history — in which case the caller falls back to the Inbox.
///
/// The live filter matters more here than anywhere else in this file: this is
/// the one read whose result becomes a *write destination*. Returning a
/// soft-deleted collection would relocate real copies into a place the user
/// cannot see (specs/collection-deletion.md), and `ReturnToPrevious` is reused
/// verbatim as a delete disposition in the next step of that spec.
///
/// **Precisely what a hidden source does here:** the filter sits in the `WHERE`,
/// ahead of `ORDER BY created_at DESC LIMIT 1`, so a hidden previous location is
/// *skipped over* and the next-most-recent **live** source wins — it does not
/// abort the lookup. The Inbox fallback fires only when the card has no live
/// source in its whole history. That is the intended reading of "return to
/// previous" (the most recent place the copies could actually go back to), but
/// it is not the same as "treat a hidden source as no history", so step 3 should
/// plan against this behaviour rather than the simpler one.
///
/// **An undone move gets the same treatment, for the same reason.**
/// `undone_at IS NULL` sits beside the live filter in that `WHERE`, so a move
/// that was later reversed is *skipped over* exactly like a hidden source —
/// the next-most-recent **live, un-undone** move wins, and the Inbox fallback
/// only fires when none is left. This matters most after P6-190: a delete's
/// relocations have `from_collection_id` set to the collection being deleted,
/// so they are never candidates for *that same* collection's own previous
/// location — the undone move that can pollute this lookup always belongs to
/// **another** collection's delete landing copies into `collection_id`
/// (a `to_collection_id = collection_id` row) that was later undone. Concretely:
/// `X` already holds some copies of a card; a delete of `Y` relocates two more
/// copies of the same card into `X`; undoing that delete pulls those two back
/// out and stamps `undone_at` on the move. Without this filter, `X`'s own,
/// unrelated copies would then "return" to `Y` — a collection that never
/// really held them as live history.
async fn previous_location(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
    grain: &Grain,
) -> ApiResult<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(&format!(
        "SELECT from_collection_id FROM moves \
         WHERE to_collection_id = $1 AND printing_id = $2 AND finish = $3::card_finish \
           AND condition = $4::card_condition AND language = $5 AND from_collection_id IS NOT NULL \
           AND undone_at IS NULL AND {live} \
         ORDER BY created_at DESC LIMIT 1",
        live = in_live_collection("moves.from_collection_id"),
    ))
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

/// `collection_id` if it is still live and the caller's, otherwise the caller's
/// Inbox — the redirect behind [`undo_one`]'s write-back
/// (specs/collection-deletion.md, maintainer ruling 2026-08-09).
///
/// A row that is missing entirely takes the same branch as a hidden one, and
/// deliberately: both mean "these copies have nowhere of their own to go back
/// to", and the Inbox is the answer to that question everywhere else in this
/// file too.
async fn live_or_inbox(
    tx: &mut Transaction<'static, Postgres>,
    user_id: Uuid,
    collection_id: Id,
) -> ApiResult<Uuid> {
    let live: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM collections WHERE id = $1 AND deleted_at IS NULL")
            .bind(collection_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(upstream)?;
    match live {
        Some(_) => Ok(collection_id),
        None => inbox_id(tx, user_id).await,
    }
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
    // `deleted_at IS NULL` is belt-and-braces: the Inbox is undeletable, so a
    // soft-deleted one cannot exist (the `collections_one_inbox` index is left
    // unfiltered for exactly that reason). If one ever did, this read failing is
    // the right outcome — silently handing out a hidden Inbox as the fallback
    // destination for teardown and `ToParent` would not be.
    let (id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM collections WHERE is_inbox AND deleted_at IS NULL")
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
    from_board: String,
    to_board: String,
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
    board: String,
    quantity: i32,
}

/// A snapshotted `desires` row — `delete_collection`'s pre-merge read, the
/// source of the receipt's [`RelocatedDesire`] handles. Desires have no ledger,
/// so this snapshot (not a `move_id`) is what undo has to reverse the
/// merge-and-drop from.
#[derive(sqlx::FromRow)]
struct DesireGrainRow {
    oracle_id: Uuid,
    printing_id: Option<Uuid>,
    board: String,
    quantity: i32,
}

/// The [`Grain`] of a snapshotted stack — the board rides beside it, never in
/// it (see [`Grain`]). Shared by the two operations that empty a collection:
/// `teardown` and `delete_collection`.
fn grain_of(h: &MoveGrainRow) -> Grain {
    Grain {
        printing_id: h.printing_id,
        finish: h.finish.clone(),
        condition: h.condition.clone(),
        language: h.language.clone(),
    }
}

#[derive(sqlx::FromRow)]
struct SuggestedRow {
    collection_id: Uuid,
    collection_name: String,
    desired: i32,
    present: i32,
}

/// One everything-view row: the shared summary projection plus this caller's
/// two aggregate totals. Flattened rather than re-listing the summary columns,
/// so `/my` cannot drift from `/catalog` on what a card row *is*.
#[derive(sqlx::FromRow)]
struct AllCardsRowSql {
    #[sqlx(flatten)]
    card: SearchRowSql,
    owned: i32,
    wanted: i32,
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

/// A row of `recently_deleted` — `kind`/`deleted_at` read back as text (the
/// enum cast and the formatted timestamp respectively), turned into
/// [`DeletedCollectionRow`] by the caller.
#[derive(sqlx::FromRow)]
struct DeletedCollectionSql {
    id: Uuid,
    name: String,
    kind: String,
    deleted_at: String,
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
    /// `NULL` for single-face printings; elements can be NULL individually (a
    /// face without a `normal` image), hence the nested `Option`.
    face_image_uris: Option<Vec<Option<String>>>,
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
    printing_id: Option<Uuid>,
    image_uri: Option<String>,
    layout: Option<String>,
    /// NULL unless the layout has a real back face (the select gates it), so
    /// single-face rows don't drag their absent jsonb over the wire.
    card_faces: Option<serde_json::Value>,
    face_image_uris: Option<Vec<Option<String>>>,
}

impl SearchRowSql {
    /// One summary shape for both per-oracle projections — including the flip
    /// faces, which are only built for [`shared::catalog::BACK_FACE_LAYOUTS`].
    /// `owned` is not in this projection (it is RLS-scoped and the catalog read
    /// is not): both callers fill it from [`HostedBackend::owned_by_oracle`],
    /// which answers `None` for an anonymous caller.
    fn into_summary(self, owned: Option<i32>) -> CardSummary {
        let faces = shared::CardFaceSummary::build(
            self.layout.as_deref(),
            self.card_faces.as_ref(),
            &self.face_image_uris.unwrap_or_default(),
        );
        CardSummary {
            oracle_id: self.oracle_id,
            name: self.name,
            printing_id: self.printing_id,
            image_uri: self.image_uri,
            mana_cost: self.mana_cost,
            type_line: self.type_line,
            owned,
            faces,
        }
    }
}

/// The shared select list of the per-oracle summary projections (search, card
/// summary). `card_faces` ships only for layouts with a real back face — the
/// `IN` list is generated from [`shared::catalog::BACK_FACE_LAYOUTS`] (our own
/// compile-time constants, not user input) so SQL and the Rust-side gate in
/// [`shared::CardFaceSummary::build`] cannot drift.
fn summary_select() -> String {
    summary_select_with("")
}

/// [`summary_select`] with extra columns appended to the select list — for
/// projections that render the same card row *plus* their own numbers (`/my`'s
/// owned/wanted totals). `extra_cols` is our own SQL fragment, never user input,
/// and must start with its own comma.
fn summary_select_with(extra_cols: &str) -> String {
    let layouts = back_face_layout_list();
    format!(
        "SELECT c.oracle_id, c.name, c.mana_cost, c.type_line, c.layout, \
         CASE WHEN c.layout IN ({layouts}) THEN c.card_faces END AS card_faces, \
         rep.id AS printing_id, rep.image_uri, rep.face_image_uris{extra_cols} \
         FROM cards c"
    )
}

/// A deck's `commander`-tagged cards + their derived color identity, inside a
/// caller-owned transaction. Extracted so the deck header (`collection_view`,
/// which carries commanders per specs/collection-api.md) and the standalone
/// `deck_commanders` read cannot drift on what a commander *is*.
async fn commanders_in(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
) -> ApiResult<DeckCommanders> {
    let rows: Vec<TaggedCardSql> = sqlx::query_as(
        "SELECT c.oracle_id, c.name, c.mana_cost, c.type_line, c.color_identity, \
                (SELECT COALESCE(image_uris->>'normal', \
                                 faces->0->'image_uris'->>'normal') FROM printings \
                 WHERE oracle_id = c.oracle_id \
                   AND COALESCE(image_uris->>'normal', \
                                faces->0->'image_uris'->>'normal') IS NOT NULL \
                 ORDER BY id LIMIT 1) AS image_uri \
         FROM card_tags ct JOIN tags t ON t.id = ct.tag_id \
         JOIN cards c ON c.oracle_id = ct.oracle_id \
         WHERE ct.collection_id = $1 AND t.builtin = 'commander' ORDER BY c.name",
    )
    .bind(collection_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(upstream)?;
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

/// The SQL `IN` list of layouts that carry a real back face — generated from
/// [`shared::catalog::BACK_FACE_LAYOUTS`] (our own compile-time constants, never
/// user input) so every projection that gates `card_faces` uses one list.
fn back_face_layout_list() -> String {
    shared::catalog::BACK_FACE_LAYOUTS
        .iter()
        .map(|l| format!("'{l}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The **representative printing** of `cards c` — one lateral shared by the
/// per-oracle projections (search, card summary) so they can't drift on which
/// printing a row stands for. Ordering puts printings that have art first
/// (`false` < `true`), then lowest id, so `image_uri` matches what the previous
/// image-only subquery returned while `id` is populated even for a card whose
/// printings all lack art — `+ Have` needs a printing id regardless.
/// `face_image_uris` carries the same printing's per-face art for the flip
/// control, so front and back always come from one printing.
const REPRESENTATIVE_PRINTING_JOIN: &str = " LEFT JOIN LATERAL ( \
     SELECT p.id, \
            COALESCE(p.image_uris->>'normal', p.faces->0->'image_uris'->>'normal') AS image_uri, \
            CASE WHEN p.faces IS NOT NULL THEN \
                (SELECT array_agg(f->'image_uris'->>'normal' ORDER BY ord) \
                 FROM jsonb_array_elements(p.faces) WITH ORDINALITY AS t(f, ord)) \
            END AS face_image_uris \
     FROM printings p WHERE p.oracle_id = c.oracle_id \
     ORDER BY (COALESCE(p.image_uris->>'normal', \
                        p.faces->0->'image_uris'->>'normal') IS NULL), p.id \
     LIMIT 1 ) rep ON true ";

impl HostedBackend {
    /// Disambiguate a write that affected no rows: an existing-but-Inbox row is a
    /// `Conflict` (Inbox is protected), an absent/not-owned/**soft-deleted** row
    /// is `NotFound`. `op` is the past-tense verb for the message ("renamed",
    /// "deleted", …).
    async fn absent_or_inbox(
        &self,
        tx: &mut Transaction<'static, Postgres>,
        id: Id,
        op: &str,
    ) -> ApiError {
        match sqlx::query_as::<_, (bool,)>(
            "SELECT is_inbox FROM collections WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        {
            Ok(Some((true,))) => ApiError::Conflict(format!("the Inbox cannot be {op}")),
            Ok(Some((false,))) => ApiError::NotFound("collection".into()),
            Ok(None) => ApiError::NotFound("collection".into()),
            Err(e) => upstream(e),
        }
    }
}

/// A `collections` row as decoded by sqlx. `kind` and `position` are read via
/// SQL casts (`kind::text`, `position::float8`): sqlx without the decimal
/// feature can't decode `numeric`, and the enum decodes cleanly as text.
/// [`CollectionRow`] plus the own-present and own-desired counts of the
/// sidebar-tree read.
#[derive(sqlx::FromRow)]
struct CollectionTreeSql {
    #[sqlx(flatten)]
    row: CollectionRow,
    present: i64,
    desired: i64,
}

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
    /// NULL when the cell aggregates several grains, or is desire-only.
    holding_id: Option<Uuid>,
    layout: Option<String>,
    /// NULL unless the layout has a real back face (the select gates it).
    card_faces: Option<serde_json::Value>,
    face_image_uris: Option<Vec<Option<String>>>,
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
        // The flip faces come from *this row's* printing, not a representative
        // one: in a collection you are looking at the copy you hold.
        let faces = shared::CardFaceSummary::build(
            self.layout.as_deref(),
            self.card_faces.as_ref(),
            &self.face_image_uris.unwrap_or_default(),
        );
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
            holding_id: self.holding_id,
            faces,
        })
    }
}

/// The collection-wide header counts. `to_buy` is derived rather than selected —
/// `missing = owned_elsewhere + to_buy` is the definition, and a second SQL
/// expression for it could drift from the two it is the remainder of.
#[derive(sqlx::FromRow)]
struct TotalsSql {
    present: i32,
    present_rollup: i32,
    desired: i32,
    missing: i32,
    owned_elsewhere: i32,
}

impl TotalsSql {
    fn into_totals(self) -> shared::CollectionTotals {
        shared::CollectionTotals {
            present: self.present,
            present_rollup: self.present_rollup,
            desired: self.desired,
            missing: self.missing,
            owned_elsewhere: self.owned_elsewhere,
            to_buy: self.missing - self.owned_elsewhere,
        }
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

/// Lazily provision the caller's one Inbox (idempotent via the
/// `collections_one_inbox` partial unique index). Every "first `/my` request"
/// read runs this — `list_collections` and `collection_tree`
/// (specs/collection-api.md → Inbox provisioning).
async fn ensure_inbox(tx: &mut Transaction<'static, Postgres>, user_id: Uuid) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO collections (user_id, kind, name, is_inbox) \
         VALUES ($1, 'binder', 'Inbox', true) \
         ON CONFLICT (user_id) WHERE is_inbox DO NOTHING",
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(upstream)?;
    Ok(())
}

/// A collection's needs (specs/collection-api.md → `NeedsView`): cards it
/// desires beyond what it holds, split into owned-elsewhere (with per-location
/// listings) and short-to-buy. Factored out of [`HostedBackend::needs`] so
/// [`HostedBackend::pull_needs`] can re-derive the identical read inside its
/// own write transaction (P6-120) instead of a separate one — the caller owns
/// `require_owned_collection` and the commit, this only reads.
///
/// `present_here` / `elsewhere` are collection-scoped aggregations in their
/// own right, not re-derivations of `owned_by_card`, so the soft delete filter
/// has to land here too (specs/collection-deletion.md is explicit about this
/// pair). `d`/`ph` are scoped to `$1`, which the caller's own
/// `require_owned_collection` has already proved live; `pe` spans every
/// *other* collection and so carries the filter.
async fn read_needs_rows(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
) -> ApiResult<Vec<NeedRow>> {
    let rows: Vec<NeedSql> = sqlx::query_as(&format!(
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
           WHERE h.collection_id <> $1 AND {live} GROUP BY p.oracle_id \
         ) \
         SELECT d.oracle_id, c.name, d.desired, COALESCE(ph.present_here, 0) AS present_here, \
                COALESCE(pe.elsewhere, 0) AS elsewhere \
         FROM d JOIN cards c ON c.oracle_id = d.oracle_id \
         LEFT JOIN ph ON ph.oracle_id = d.oracle_id \
         LEFT JOIN pe ON pe.oracle_id = d.oracle_id \
         WHERE d.desired > COALESCE(ph.present_here, 0) \
         ORDER BY c.name",
        live = in_live_collection("h.collection_id"),
    ))
    .bind(collection_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(upstream)?;

    // Per-location listing for the needed cards, in the user's OTHER **live**
    // collections — one query, grouped in Rust. The rows here are "pull it
    // from here" offers, so a hidden collection must not appear.
    let oracles: Vec<Uuid> = rows.iter().map(|r| r.oracle_id).collect();
    let locs: Vec<LocationSql> = sqlx::query_as(
        "SELECT p.oracle_id, h.collection_id, c.name AS collection_name, \
                sum(h.quantity)::int AS quantity \
         FROM holdings h JOIN printings p ON p.id = h.printing_id \
         JOIN collections c ON c.id = h.collection_id \
         WHERE h.collection_id <> $1 AND p.oracle_id = ANY($2) \
           AND c.deleted_at IS NULL \
         GROUP BY p.oracle_id, h.collection_id, c.name \
         ORDER BY quantity DESC, c.name",
    )
    .bind(collection_id)
    .bind(&oracles)
    .fetch_all(&mut **tx)
    .await
    .map_err(upstream)?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let gap = r.desired - r.present_here;
            let owned_elsewhere = r.elsewhere.min(gap);
            NeedRow {
                locations: locs
                    .iter()
                    .filter(|l| l.oracle_id == r.oracle_id)
                    .map(|l| CardLocation {
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
        .collect())
}

/// Reject an operation targeting a collection the caller doesn't own **or that
/// has been soft-deleted** — RLS makes a non-owned collection invisible and
/// `deleted_at IS NULL` makes a hidden one invisible, so this EXISTS settles
/// ownership, liveness and a bad id in one read, with one answer: `NotFound`.
/// (The `holdings`/`desires` RLS policies only gate on their own `user_id`, not
/// the collection's, so this guard is load-bearing.)
///
/// A soft-deleted collection failing here *exactly* as a non-existent one does
/// is what stops it ever being a move destination or any other write target
/// (specs/collection-deletion.md → "The read path"), and the doc comment this
/// function had lost — it had drifted onto `ensure_inbox` above — is restored
/// here with it.
async fn require_owned_collection(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
) -> ApiResult<()> {
    let found: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM collections WHERE id = $1 AND deleted_at IS NULL")
            .bind(collection_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(upstream)?;
    if found.is_none() {
        return Err(ApiError::NotFound("collection".into()));
    }
    Ok(())
}

/// Reject an undo/restore targeting a collection that is not the caller's, or
/// that is not currently soft-deleted — the mirror image of
/// [`require_owned_collection`], and used by both [`HostedBackend::undo_delete`]
/// and [`HostedBackend::restore_collection`].
///
/// **The one documented exemption from `soft_delete_guard`'s "every ownership
/// lookup filters `deleted_at IS NULL`" rule** (specs/collection-deletion.md →
/// step 5): undo and restore both exist to find a **hidden** row, so this is
/// the one place in the file that legitimately looks for the opposite. The
/// guard test below allowlists this exact query rather than loosening its
/// needle, so a *new*, undocumented unfiltered lookup still fails it.
///
/// **`FOR UPDATE`** (adversarial review, `P6-190`): `undo_one` is idempotent,
/// but `RestoreDesire` is not — two overlapping `undo_delete` calls carrying
/// the same receipt would otherwise both pass this check before either
/// commits, and both would re-insert the relocated desires, doubling them.
/// Locking the row here makes the second caller block on the first, then
/// re-evaluate the same `WHERE` against the *committed* row: once the first
/// call's `Unhide` clears `deleted_at`, the second sees a live row, fails
/// `deleted_at IS NOT NULL` honestly, and its whole transaction refuses
/// rather than double-applying anything.
async fn require_deleted_collection(
    tx: &mut Transaction<'static, Postgres>,
    collection_id: Id,
) -> ApiResult<()> {
    let found: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM collections WHERE id = $1 AND deleted_at IS NOT NULL FOR UPDATE",
    )
    .bind(collection_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(upstream)?;
    if found.is_none() {
        return Err(ApiError::NotFound("deleted collection".into()));
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

#[cfg(test)]
mod search_live {
    use super::*;

    /// End-to-end checks of the catalog-search query engine against the live
    /// dev-branch POC catalog (specs/catalog-search.md). Deliberately
    /// #[ignore]d — needs `DATABASE_URL` (the app_runtime read credential):
    ///
    ///   DATABASE_URL=… cargo test -p app --features hosted -- --ignored
    #[tokio::test]
    #[ignore = "hits the live dev catalog (DATABASE_URL required)"]
    async fn query_engine_against_dev_poc_data() {
        let b = HostedBackend::anonymous().await.expect("pool");
        let q = |s: &str| SearchQuery { q: Some(s.into()) };
        let page = Page {
            cursor: None,
            limit: Some(10),
        };
        let search =
            |query: SearchQuery, page: Page| async { CatalogStore::search(&b, query, page).await };

        // browse-all pages by (name, oracle) with a live cursor
        let p1 = search(SearchQuery::default(), page.clone()).await.unwrap();
        assert_eq!(p1.cards.len(), 10);
        let cur = p1.next_cursor.clone().expect("more than one page");
        let p2 = search(
            SearchQuery::default(),
            Page {
                cursor: Some(cur),
                limit: Some(10),
            },
        )
        .await
        .unwrap();
        assert!(
            p2.cards[0].name >= p1.cards[9].name,
            "page 2 continues past page 1"
        );
        assert_ne!(p1.cards[0].oracle_id, p2.cards[0].oracle_id);

        // name substring
        let r = search(q("lightning bolt"), page.clone()).await.unwrap();
        assert!(
            r.cards.iter().any(|c| c.name == "Lightning Bolt"),
            "{:?}",
            r.cards
        );

        // o: reaches BACK-face text through oracle_search_text (this phrase
        // exists only on Ral, Leyline Prodigy — a transform back face)
        let r = search(q("o:\"an additional loyalty counter\""), page.clone())
            .await
            .unwrap();
        assert!(
            r.cards
                .iter()
                .any(|c| c.name.starts_with("Ral, Monsoon Mage")),
            "{:?}",
            r.cards
        );

        // combined card-scoped terms
        let r = search(q("t:instant c:r mv<=1"), page.clone())
            .await
            .unwrap();
        assert!(
            r.cards.iter().any(|c| c.name == "Lightning Bolt"),
            "{:?}",
            r.cards
        );

        // printing-scoped comma-OR + one-EXISTS semantics
        let r = search(q("s:lea r:rare,mythic"), page.clone())
            .await
            .unwrap();
        assert!(!r.cards.is_empty(), "Alpha rares exist in the subset");

        // colorless + identity on artifacts
        let r = search(q("c:colorless t:artifact s:lea"), page.clone())
            .await
            .unwrap();
        assert!(!r.cards.is_empty(), "Alpha artifacts are colorless");

        // negation excludes (a slice small enough to dodge the 200-row cap)
        let big = Page {
            cursor: None,
            limit: Some(200),
        };
        let creatures = search(q("t:creature s:lea"), big.clone()).await.unwrap();
        assert!(
            !creatures.cards.is_empty() && creatures.cards.len() < 200,
            "sanity: fits one page ({})",
            creatures.cards.len()
        );
        let non_white = search(q("t:creature s:lea -c:w"), big).await.unwrap();
        assert!(!non_white.cards.is_empty());
        assert!(non_white.cards.len() < creatures.cards.len());

        // unknown syntax is a 422 naming the term
        let err = search(q("pow>3"), page.clone()).await.unwrap_err();
        match err {
            ApiError::Validation(msg) => assert!(msg.contains("pow>3"), "{msg}"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod delete_live {
    //! The dev-branch integration check specs/collection-deletion.md → Testing
    //! asks for: delete → the collection is hidden, the cards are at the
    //! destination, the children are re-parented, and the ledger rows are real
    //! moves rather than intakes/removals — plus the regression that matters
    //! most, a card held **only** in the deleted collection.
    //!
    //! Deliberately `#[ignore]`d, like [`super::search_live`]: it writes to a
    //! real database. Needs the runtime credential and a user to act as —
    //!
    //! ```text
    //! DATABASE_URL=… TR_DEV_USER_ID=… \
    //!   cargo test -p app --features hosted -- --ignored delete_live --nocapture
    //! ```
    //!
    //! (`scripts/seed-dev-data.sh` shows how to resolve the e2e user's uuid from
    //! `neon_auth."user"` with the owner credential.)
    //!
    //! **Self-cleaning, and it proves it.** Every collection it makes is named
    //! with [`SCRATCH`]; the two printings it uses are picked from cards the
    //! user owns *none* of, so their holdings can be swept from anywhere —
    //! including the Inbox copies a `ReturnToPrevious` or an undo-redirect
    //! lands there. The test's last assertion is that the user's row counts are
    //! exactly what they were before it ran.

    use super::*;
    use shared::{HaveDisposition, WantDisposition};

    /// Name prefix for everything this test creates, so a crashed run is
    /// cleanable (and is cleaned, on the next run's setup).
    const SCRATCH: &str = "P6-188 scratch";

    struct Fixture {
        be: HostedBackend,
        user_id: Uuid,
        root: Id,
        subject: Id,
        child: Id,
        grandchild: Id,
        source: Id,
        elsewhere: Id,
        inbox: Id,
        /// Held (after the setup move) **only** in `subject` — the regression's card.
        printing_a: Id,
        oracle_a: Id,
        /// Held in `subject` *and* in `elsewhere`.
        printing_b: Id,
        oracle_b: Id,
    }

    async fn tx(be: &HostedBackend) -> Transaction<'static, Postgres> {
        be.scoped_tx().await.expect("scoped tx")
    }

    /// `(collections, holdings, desires, moves)` for the caller — RLS scopes
    /// each count, and hidden collections are counted too (this is the
    /// "did you leave anything behind" measure, not a read model).
    async fn counts(be: &HostedBackend) -> (i64, i64, i64, i64) {
        let mut t = tx(be).await;
        let row: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM collections), (SELECT count(*) FROM holdings), \
                    (SELECT count(*) FROM desires), (SELECT count(*) FROM moves)",
        )
        .fetch_one(&mut *t)
        .await
        .expect("counts");
        t.commit().await.expect("commit");
        row
    }

    /// The `owned_by_card` view — the single definition of "owned per card", and
    /// therefore the thing the acceptance rule is stated against. No row = 0.
    async fn owned(be: &HostedBackend, oracle_id: Id) -> i32 {
        let mut t = tx(be).await;
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT owned FROM owned_by_card WHERE oracle_id = $1")
                .bind(oracle_id)
                .fetch_optional(&mut *t)
                .await
                .expect("owned_by_card");
        t.commit().await.expect("commit");
        row.map(|(n,)| n).unwrap_or(0)
    }

    /// Copies of a printing in a collection, hidden or not (raw `holdings`, not
    /// a read model — "where did the rows actually go").
    async fn present(be: &HostedBackend, collection_id: Id, printing_id: Id) -> i32 {
        let mut t = tx(be).await;
        let (n,): (i32,) = sqlx::query_as(
            "SELECT COALESCE(sum(quantity), 0)::int FROM holdings \
             WHERE collection_id = $1 AND printing_id = $2",
        )
        .bind(collection_id)
        .bind(printing_id)
        .fetch_one(&mut *t)
        .await
        .expect("present");
        t.commit().await.expect("commit");
        n
    }

    /// Desired quantity of `oracle_id` in a collection, hidden or not (raw
    /// `desires`, mirroring [`present`]'s reasoning for holdings — "where did
    /// the rows actually go", not a read model). 0 if none.
    async fn desired(be: &HostedBackend, collection_id: Id, oracle_id: Id) -> i32 {
        let mut t = tx(be).await;
        let (n,): (i32,) = sqlx::query_as(
            "SELECT COALESCE(sum(quantity), 0)::int FROM desires \
             WHERE collection_id = $1 AND oracle_id = $2",
        )
        .bind(collection_id)
        .bind(oracle_id)
        .fetch_one(&mut *t)
        .await
        .expect("desired");
        t.commit().await.expect("commit");
        n
    }

    /// Whether a ledger row has been reversed.
    async fn move_undone(be: &HostedBackend, move_id: Id) -> bool {
        let mut t = tx(be).await;
        let (undone,): (bool,) =
            sqlx::query_as("SELECT undone_at IS NOT NULL FROM moves WHERE id = $1")
                .bind(move_id)
                .fetch_one(&mut *t)
                .await
                .expect("move undone_at");
        t.commit().await.expect("commit");
        undone
    }

    /// `(parent_id, hidden)` straight off the row.
    async fn node(be: &HostedBackend, id: Id) -> (Option<Id>, bool) {
        let mut t = tx(be).await;
        let row: (Option<Uuid>, bool) = sqlx::query_as(
            "SELECT parent_id, deleted_at IS NOT NULL FROM collections WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&mut *t)
        .await
        .expect("node");
        t.commit().await.expect("commit");
        row
    }

    /// A ledger row's two ends. `NULL` means intake / removal, which is exactly
    /// what the old hard delete turned every historical move into.
    async fn move_ends(be: &HostedBackend, move_id: Id) -> (Option<Id>, Option<Id>) {
        let mut t = tx(be).await;
        let row: (Option<Uuid>, Option<Uuid>) =
            sqlx::query_as("SELECT from_collection_id, to_collection_id FROM moves WHERE id = $1")
                .bind(move_id)
                .fetch_one(&mut *t)
                .await
                .expect("move");
        t.commit().await.expect("commit");
        row
    }

    /// Remove every trace of a run — and derive *what* to remove rather than
    /// being told, so it also cleans up after a run that crashed half way
    /// (which is the state the next run's pre-state count would otherwise be
    /// measured against).
    ///
    /// The hard part is the copies that legitimately end up **outside** a
    /// scratch collection: the Inbox, where `ReturnToPrevious`'s no-history
    /// fallback and the undo redirect both put them. Every one of those got
    /// there through a `moves` row with a scratch collection at one end —
    /// delete writes real ledger moves, which is the property this whole task
    /// is about — so the ledger names the printings, and the holdings go first
    /// while it still does. Then the ledger, then the collections (their
    /// cascade takes the desires).
    ///
    /// Everything is RLS-scoped to the caller, so the `printing_id` sweep cannot
    /// reach another user's copies.
    async fn sweep(be: &HostedBackend) {
        let mut t = tx(be).await;
        let scratch = format!("{SCRATCH}%");
        sqlx::query(
            "DELETE FROM holdings WHERE printing_id IN ( \
               SELECT printing_id FROM moves \
                WHERE from_collection_id IN (SELECT id FROM collections WHERE name LIKE $1) \
                   OR to_collection_id IN (SELECT id FROM collections WHERE name LIKE $1) \
               UNION \
               SELECT printing_id FROM holdings \
                WHERE collection_id IN (SELECT id FROM collections WHERE name LIKE $1))",
        )
        .bind(&scratch)
        .execute(&mut *t)
        .await
        .expect("sweep holdings");
        sqlx::query(
            "DELETE FROM moves WHERE from_collection_id IN \
               (SELECT id FROM collections WHERE name LIKE $1) \
                OR to_collection_id IN (SELECT id FROM collections WHERE name LIKE $1)",
        )
        .bind(&scratch)
        .execute(&mut *t)
        .await
        .expect("sweep moves");
        sqlx::query("DELETE FROM collections WHERE name LIKE $1")
            .bind(&scratch)
            .execute(&mut *t)
            .await
            .expect("sweep collections");
        t.commit().await.expect("commit");
    }

    async fn binder(be: &HostedBackend, parent: Option<Id>, name: &str) -> Id {
        be.create_collection(NewCollection {
            parent_id: parent,
            kind: CollectionKind::Binder,
            name: format!("{SCRATCH} {name}"),
            format: None,
        })
        .await
        .expect("create")
        .id
    }

    fn have(printing_id: Id, quantity: i32) -> AddHave {
        AddHave {
            printing_id,
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: shared::default_language(),
            board: Board::Main,
            quantity,
        }
    }

    /// Build the subtree each phase deletes:
    ///
    /// ```text
    /// root ── subject (deck) ── child ── grandchild
    ///      ├─ source        (card A came from here)
    ///      └─ elsewhere     (holds card B too)
    /// ```
    ///
    /// Card **A** lands in `subject` by a real move *out of* `source`, which is
    /// both how it ends up held nowhere else and how `ReturnToPrevious` has a
    /// live previous location to find. Card **B** is added straight to
    /// `subject` (an intake, `from = NULL`), so it has no previous location and
    /// must fall back to the Inbox.
    async fn setup(be: HostedBackend, user_id: Uuid) -> Fixture {
        let inbox = be
            .list_collections()
            .await
            .expect("collections")
            .into_iter()
            .find(|c| c.is_inbox)
            .expect("inbox")
            .id;

        // Two printings of cards the user owns none of — that is what makes
        // "held only in the deleted collection" true, and what makes the sweep
        // safe to run against every collection including the Inbox.
        let mut t = tx(&be).await;
        let cards: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT DISTINCT ON (p.oracle_id) p.id, p.oracle_id FROM printings p \
             WHERE NOT EXISTS (SELECT 1 FROM owned_by_card o WHERE o.oracle_id = p.oracle_id) \
             ORDER BY p.oracle_id, p.id LIMIT 2",
        )
        .fetch_all(&mut *t)
        .await
        .expect("unowned printings");
        t.commit().await.expect("commit");
        assert_eq!(cards.len(), 2, "need two cards the user owns none of");

        let root = binder(&be, None, "root").await;
        let subject = be
            .create_collection(NewCollection {
                parent_id: Some(root),
                kind: CollectionKind::Deck,
                name: format!("{SCRATCH} subject"),
                format: None,
            })
            .await
            .expect("create deck")
            .id;
        let child = binder(&be, Some(subject), "child").await;
        let grandchild = binder(&be, Some(child), "grandchild").await;
        let source = binder(&be, Some(root), "source").await;
        let elsewhere = binder(&be, Some(root), "elsewhere").await;

        let (printing_a, oracle_a) = cards[0];
        let (printing_b, oracle_b) = cards[1];

        be.add_holding(source, have(printing_a, 2))
            .await
            .expect("A");
        be.move_cards(MoveRequest {
            from_collection_id: Some(source),
            to_collection_id: Some(subject),
            printing_id: printing_a,
            finish: Finish::Nonfoil,
            condition: Condition::Nm,
            language: shared::default_language(),
            from_board: Board::Main,
            to_board: Board::Main,
            quantity: 2,
        })
        .await
        .expect("A into the subject");
        be.add_holding(subject, have(printing_b, 1))
            .await
            .expect("B here");
        be.add_holding(elsewhere, have(printing_b, 4))
            .await
            .expect("B elsewhere");
        be.add_desire(
            subject,
            AddWant {
                oracle_id: oracle_a,
                printing_id: None,
                board: Board::Main,
                quantity: 3,
            },
        )
        .await
        .expect("want");

        assert_eq!(owned(&be, oracle_a).await, 2, "A is held, only here");
        assert_eq!(
            owned(&be, oracle_b).await,
            5,
            "B is held here and elsewhere"
        );

        Fixture {
            be,
            user_id,
            root,
            subject,
            child,
            grandchild,
            source,
            elsewhere,
            inbox,
            printing_a,
            oracle_a,
            printing_b,
            oracle_b,
        }
    }

    /// The assertions every disposition shares: exactly one node hidden, the
    /// children survive re-parented, and every ledger row the receipt names is a
    /// **real** move (two live ends), not an intake or a removal.
    async fn assert_shape(f: &Fixture, receipt: &shared::DeleteCollectionReceipt, dests: &[Id]) {
        assert_eq!(receipt.collection_id, f.subject);
        assert_eq!(receipt.reparented, vec![f.child], "one child, re-parented");

        assert!(node(&f.be, f.subject).await.1, "subject is hidden");
        for other in [f.root, f.child, f.grandchild, f.source, f.elsewhere] {
            assert!(!node(&f.be, other).await.1, "only one node is hidden");
        }
        assert_eq!(
            node(&f.be, f.child).await.0,
            Some(f.root),
            "the child re-points at the deleted node's parent"
        );
        assert_eq!(
            node(&f.be, f.grandchild).await.0,
            Some(f.child),
            "a grandchild keeps its own parent — delete removes exactly one node"
        );

        assert_eq!(receipt.move_ids.len(), dests.len());
        for (move_id, dest) in receipt.move_ids.iter().zip(dests) {
            assert_eq!(
                move_ends(&f.be, *move_id).await,
                (Some(f.subject), Some(*dest)),
                "a real move out of the deleted collection, not an intake/removal"
            );
        }
    }

    #[tokio::test]
    #[ignore = "writes to the live dev branch (DATABASE_URL + TR_DEV_USER_ID required)"]
    async fn delete_relocates_instead_of_destroying() {
        let user_id: Uuid = std::env::var("TR_DEV_USER_ID")
            .expect("TR_DEV_USER_ID (the dev user to act as)")
            .parse()
            .expect("uuid");
        let be = HostedBackend::for_user(user_id).await.expect("pool");

        // A previous crashed run would otherwise poison the counts.
        sweep(&be).await;
        let before = counts(&be).await;

        // --- ToParent (the default) ---------------------------------------
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        let receipt =
            f.be.delete_collection(DeleteCollectionReq::defaults(f.subject))
                .await
                .expect("delete");
        assert_shape(&f, &receipt, &[f.root, f.root]).await;
        assert_eq!(present(&f.be, f.root, f.printing_a).await, 2);
        assert_eq!(present(&f.be, f.subject, f.printing_a).await, 0);
        assert_eq!(
            owned(&f.be, f.oracle_a).await,
            2,
            "THE regression: a card held only in the deleted collection is \
             still owned, at the destination"
        );
        assert_eq!(owned(&f.be, f.oracle_b).await, 5);
        sweep(&f.be).await;

        // --- ReturnToPrevious ---------------------------------------------
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        let receipt =
            f.be.delete_collection(DeleteCollectionReq {
                collection_id: f.subject,
                haves: HaveDisposition::ReturnToPrevious,
                wants: WantDisposition::Discard,
            })
            .await
            .expect("delete");
        // A came from `source`; B was an intake, so it has no previous location
        // and falls back to the Inbox. Receipt order follows the snapshot's, so
        // match on the ends rather than on a guessed order.
        let ends: Vec<_> = {
            let mut v = Vec::new();
            for id in &receipt.move_ids {
                v.push(move_ends(&f.be, *id).await.1.unwrap());
            }
            v.sort();
            v
        };
        let mut want = vec![f.source, f.inbox];
        want.sort();
        assert_eq!(ends, want, "A back to its source, B to the Inbox");
        assert_eq!(present(&f.be, f.source, f.printing_a).await, 2);
        assert_eq!(present(&f.be, f.inbox, f.printing_b).await, 1);
        assert_eq!(owned(&f.be, f.oracle_a).await, 2, "still owned");
        sweep(&f.be).await;

        // --- To { elsewhere } ---------------------------------------------
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        let receipt =
            f.be.delete_collection(DeleteCollectionReq {
                collection_id: f.subject,
                haves: HaveDisposition::To {
                    collection_id: f.elsewhere,
                },
                wants: WantDisposition::To {
                    collection_id: f.elsewhere,
                },
            })
            .await
            .expect("delete");
        assert_shape(&f, &receipt, &[f.elsewhere, f.elsewhere]).await;
        assert_eq!(present(&f.be, f.elsewhere, f.printing_a).await, 2);
        assert_eq!(present(&f.be, f.elsewhere, f.printing_b).await, 5);
        assert_eq!(owned(&f.be, f.oracle_a).await, 2, "still owned");
        let (desires_here,): (i64,) = {
            let mut t = tx(&f.be).await;
            let row = sqlx::query_as("SELECT count(*) FROM desires WHERE collection_id = $1")
                .bind(f.elsewhere)
                .fetch_one(&mut *t)
                .await
                .expect("desires");
            t.commit().await.expect("commit");
            row
        };
        assert_eq!(desires_here, 1, "the wants moved with the cards");
        // The receipt grew a handle for this (maintainer ruling 2026-08-10):
        // `WantDisposition::To` has no ledger, so this is undo's *only* way
        // to find its way back.
        assert_eq!(receipt.desires.len(), 1, "one relocated desire row");
        assert_eq!(receipt.desires[0].to_collection_id, f.elsewhere);
        assert_eq!(receipt.desires[0].oracle_id, f.oracle_a);
        assert_eq!(receipt.desires[0].quantity, 3);
        sweep(&f.be).await;

        // --- Discard: writes nothing --------------------------------------
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        let receipt =
            f.be.delete_collection(DeleteCollectionReq {
                collection_id: f.subject,
                haves: HaveDisposition::Discard,
                wants: WantDisposition::Discard,
            })
            .await
            .expect("delete");
        assert_shape(&f, &receipt, &[]).await;
        assert_eq!(
            present(&f.be, f.subject, f.printing_a).await,
            2,
            "the copies are still there — attached to the hidden collection, \
             which is what makes them come back on undo"
        );
        assert_eq!(
            owned(&f.be, f.oracle_a).await,
            0,
            "…and not owned: hidden means it stops counting everywhere at once"
        );
        assert_eq!(
            owned(&f.be, f.oracle_b).await,
            4,
            "the copies elsewhere stay"
        );
        sweep(&f.be).await;

        // --- the undo redirect (maintainer ruling, 2026-08-09) -------------
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        // An *unrelated, older* move: copies out of `source` into `elsewhere`.
        // Then `source` is hidden by hand, standing in for "someone deleted it
        // in between". Undoing that move must not put the copies back into a
        // collection the user can no longer see.
        f.be.add_holding(f.source, have(f.printing_b, 3))
            .await
            .expect("stock the source");
        let m =
            f.be.move_cards(MoveRequest {
                from_collection_id: Some(f.source),
                to_collection_id: Some(f.elsewhere),
                printing_id: f.printing_b,
                finish: Finish::Nonfoil,
                condition: Condition::Nm,
                language: shared::default_language(),
                from_board: Board::Main,
                to_board: Board::Main,
                quantity: 3,
            })
            .await
            .expect("move")
            .move_id;
        {
            let mut t = tx(&f.be).await;
            sqlx::query("UPDATE collections SET deleted_at = now() WHERE id = $1")
                .bind(f.source)
                .execute(&mut *t)
                .await
                .expect("hand-hide");
            t.commit().await.expect("commit");
        }
        let inbox_before = present(&f.be, f.inbox, f.printing_b).await;
        f.be.undo_move(m).await.expect("undo");
        assert_eq!(
            present(&f.be, f.source, f.printing_b).await,
            0,
            "nothing written back into the hidden collection"
        );
        assert_eq!(
            present(&f.be, f.inbox, f.printing_b).await,
            inbox_before + 3,
            "the copies came back — redirected to the Inbox"
        );

        sweep(&f.be).await;
        let _ = f.user_id;

        // --- and nothing was left behind ----------------------------------
        assert_eq!(
            counts(&be).await,
            before,
            "collections / holdings / desires / moves are exactly as found"
        );
    }

    /// Step 5's own dev-branch evidence: undo reverses a delete **whole**
    /// (`ToParent` cards + an explicitly relocated want, so the receipt's new
    /// `desires` handle is actually exercised), byte-identical; then a
    /// second delete → restore cycle shows the *weaker* semantics — the
    /// collection comes back re-attached, but its cards and children stay
    /// exactly where the delete left them.
    #[tokio::test]
    #[ignore = "writes to the live dev branch (DATABASE_URL + TR_DEV_USER_ID required)"]
    async fn undo_and_restore_live() {
        let user_id: Uuid = std::env::var("TR_DEV_USER_ID")
            .expect("TR_DEV_USER_ID (the dev user to act as)")
            .parse()
            .expect("uuid");
        let be = HostedBackend::for_user(user_id).await.expect("pool");

        sweep(&be).await;
        let before = counts(&be).await;

        // --- Undo: ToParent + an explicit want relocation, reversed whole ---
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        let receipt =
            f.be.delete_collection(DeleteCollectionReq {
                collection_id: f.subject,
                haves: HaveDisposition::ToParent,
                wants: WantDisposition::To {
                    collection_id: f.elsewhere,
                },
            })
            .await
            .expect("delete");
        assert_shape(&f, &receipt, &[f.root, f.root]).await;
        assert_eq!(receipt.desires.len(), 1, "the want moved — one handle");
        assert_eq!(
            present(&f.be, f.root, f.printing_a).await,
            2,
            "card at the parent"
        );
        assert_eq!(
            desired(&f.be, f.elsewhere, f.oracle_a).await,
            3,
            "want at elsewhere"
        );
        assert_eq!(desired(&f.be, f.subject, f.oracle_a).await, 0);

        f.be.undo_delete(receipt.clone()).await.expect("undo");

        // Byte-identical restoration: live again, same parent, the child back
        // under it, the card and the want both back at their original
        // quantities and gone from wherever the delete had put them, and
        // every move the receipt named shows `undone_at`.
        assert!(!node(&f.be, f.subject).await.1, "subject is live again");
        assert_eq!(node(&f.be, f.subject).await.0, Some(f.root));
        assert_eq!(
            node(&f.be, f.child).await.0,
            Some(f.subject),
            "the child is back under subject, not still up at root"
        );
        assert_eq!(
            present(&f.be, f.subject, f.printing_a).await,
            2,
            "card A is back"
        );
        assert_eq!(
            present(&f.be, f.root, f.printing_a).await,
            0,
            "…and gone from root"
        );
        assert_eq!(
            desired(&f.be, f.subject, f.oracle_a).await,
            3,
            "want is back"
        );
        assert_eq!(
            desired(&f.be, f.elsewhere, f.oracle_a).await,
            0,
            "…and gone from elsewhere"
        );
        for move_id in &receipt.move_ids {
            assert!(move_undone(&f.be, *move_id).await, "{move_id} reversed");
        }
        assert_eq!(
            owned(&f.be, f.oracle_a).await,
            2,
            "still owned, same as before the delete"
        );
        sweep(&f.be).await;

        // --- Restore: the weaker path — cards/children stay put -----------
        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;
        let receipt =
            f.be.delete_collection(DeleteCollectionReq::defaults(f.subject))
                .await
                .expect("delete");
        assert_shape(&f, &receipt, &[f.root, f.root]).await;

        f.be.restore_collection(f.subject).await.expect("restore");

        assert!(!node(&f.be, f.subject).await.1, "restored");
        assert_eq!(
            node(&f.be, f.subject).await.0,
            Some(f.root),
            "reattached to its still-live original parent"
        );
        assert_eq!(
            node(&f.be, f.child).await.0,
            Some(f.root),
            "the child stays exactly where the delete left it — restore does \
             not reverse the re-parent, only undo does"
        );
        assert_eq!(
            present(&f.be, f.subject, f.printing_a).await,
            0,
            "no card came back with it"
        );
        assert_eq!(
            present(&f.be, f.root, f.printing_a).await,
            2,
            "…it stays at the destination the delete sent it to"
        );
        sweep(&f.be).await;

        assert_eq!(
            counts(&be).await,
            before,
            "collections / holdings / desires / moves are exactly as found"
        );
    }

    /// P6-113: `previous_location` must not let a delete's own, now-undone
    /// relocation decide a future "return to previous" destination.
    ///
    /// Undo (`undo_one`) stamps `undone_at` on the delete's relocating move
    /// but — unlike a fresh `move_cards` call — writes no new ledger row to
    /// record the reversal, so the only trace of "this collection received A
    /// once" is that one move, now marked undone. Without the
    /// `undone_at IS NULL` predicate, `previous_location` would still hand it
    /// back as if the relocation were live history; this pins that it does
    /// not, by calling `previous_location` directly (the same helper
    /// `delete_collection`'s planner and `teardown`'s `ReturnToPrevious` both
    /// go through) rather than re-deriving the assertion through a second
    /// full delete cycle.
    #[tokio::test]
    #[ignore = "writes to the live dev branch (DATABASE_URL + TR_DEV_USER_ID required)"]
    async fn previous_location_ignores_an_undone_move() {
        let user_id: Uuid = std::env::var("TR_DEV_USER_ID")
            .expect("TR_DEV_USER_ID (the dev user to act as)")
            .parse()
            .expect("uuid");
        let be = HostedBackend::for_user(user_id).await.expect("pool");

        sweep(&be).await;
        let before = counts(&be).await;

        let f = setup(HostedBackend::for_user(user_id).await.unwrap(), user_id).await;

        // Delete `subject` with ReturnToPrevious: A's only live history (the
        // setup's `source -> subject` move) sends it back to `source` — the
        // delete appends that as a real move, `subject -> source`.
        let receipt =
            f.be.delete_collection(DeleteCollectionReq {
                collection_id: f.subject,
                haves: HaveDisposition::ReturnToPrevious,
                wants: WantDisposition::Discard,
            })
            .await
            .expect("delete");
        assert_eq!(
            present(&f.be, f.source, f.printing_a).await,
            2,
            "A relocated to source"
        );

        // Undo the delete whole: `undo_one` stamps that `subject -> source`
        // move's `undone_at` and writes the copies straight back into
        // `subject` — no new ledger row records the reversal.
        f.be.undo_delete(receipt).await.expect("undo");
        assert_eq!(
            present(&f.be, f.subject, f.printing_a).await,
            2,
            "A is back in subject"
        );

        // The predicate under test. `source` was never a *live* previous
        // location for A on its own account — its only appearance as a move
        // destination is the delete's own relocation, now undone. Before this
        // fix, `previous_location(source, …)` would still resolve to
        // `subject` (the reversed move's `from`); after it, that move is
        // skipped exactly like a hidden source and there is no other
        // history, so the answer must be `None`.
        let grain = Grain {
            printing_id: f.printing_a,
            finish: Finish::Nonfoil.to_pg().to_string(),
            condition: Condition::Nm.to_pg().to_string(),
            language: shared::default_language(),
        };
        let mut t = tx(&f.be).await;
        let prev = previous_location(&mut t, f.source, &grain)
            .await
            .expect("previous_location");
        t.commit().await.expect("commit");
        assert_eq!(
            prev, None,
            "an undone move must not decide a return-to-previous destination"
        );

        sweep(&f.be).await;
        assert_eq!(
            counts(&be).await,
            before,
            "collections / holdings / desires / moves are exactly as found"
        );
    }
}

/// Regression guard for the "owned per card" collapse onto the `owned_by_card`
/// view (specs/collection-api.md Findings): `owned_by_oracle` (backing
/// `card_summary`/`search`) and `collection_view` already read the view
/// directly; `collection_tree`'s shopping-short badge, `all_cards`, and
/// `shopping_list` each had their own inline copy of `sum(h.quantity)` joined
/// through `printings` and grouped by oracle id — three inline copies,
/// collapsed onto the one `owned_by_card` SQL view
/// (migrations/0003_collections.sql). Structural, not live-DB — it greps
/// this file's own source, so it runs under plain `cargo test` with no
/// `DATABASE_URL`.
#[cfg(test)]
mod owned_definition_guard {
    #[test]
    fn owned_per_oracle_is_never_rederived_inline() {
        let src = include_str!("hosted.rs");

        // Collapse all whitespace — including the `\` line-continuations the
        // SQL string literals use to stay readable — to single spaces, so
        // reformatting (line wraps, indentation) can't dodge the check.
        let normalized = src
            .replace('\\', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        // The exact idiom every one of the three collapsed inline copies
        // shared: an *unfiltered* aggregate over `holdings` joined to
        // `printings`, grouped by oracle id alone. This is deliberately
        // narrower than "any holdings/printings join with sum(h.quantity)" —
        // `present_here`/`elsewhere`/per-location breakdowns elsewhere in
        // this file legitimately re-derive sums scoped by `collection_id` (a
        // `WHERE` sits between the join and the `GROUP BY` there), which is a
        // different computation, not a duplicate of `owned`.
        //
        // Assembled from two halves at runtime, not one literal: `include_str!`
        // pulls in this very test function, and a single contiguous literal
        // here would match itself.
        let holdings_join_printings = "FROM holdings h JOIN printings p ON p.id = h.printing_id";
        let grouped_by_oracle_only = "GROUP BY p.oracle_id";
        let forbidden = format!("{holdings_join_printings} {grouped_by_oracle_only}");
        assert!(
            !normalized.contains(&forbidden),
            "found an inline, unfiltered owned-per-oracle aggregate (`{forbidden}`) in \
             hosted.rs. \"Owned per card\" has exactly one source: the `owned_by_card` SQL \
             view (migrations/0003_collections.sql). Select from that view — e.g. \
             `SELECT oracle_id, owned FROM owned_by_card` — instead of re-deriving \
             sum(h.quantity) grouped by oracle_id, so the upcoming `deleted_at IS NULL` \
             filter (specs/collection-deletion.md) lands in exactly one place. See \
             specs/collection-api.md Findings for the collapse this test guards."
        );
    }
}

/// Guards for collection soft deletion (specs/collection-deletion.md, step 2).
/// Both are structural — they read this file's and the migration's own source —
/// so they run under plain `cargo test` with no `DATABASE_URL`, the same trick
/// `owned_definition_guard` above uses.
#[cfg(test)]
mod soft_delete_guard {
    /// Every ownership/existence lookup of a collection by id must exclude
    /// soft-deleted rows. This is the guard the spec singles out: a hidden
    /// collection has to fail `require_owned_collection` (and the create/reparent
    /// parent checks, which share the idiom) *exactly* as a non-existent one
    /// does, or it stays reachable as a move destination and a write target.
    ///
    /// **One documented exemption** (specs/collection-deletion.md → step 5):
    /// `require_deleted_collection` — undo and restore's own existence check —
    /// deliberately inverts the filter, because finding a **hidden** row is
    /// those two operations' entire job. Counted separately by its own exact
    /// literal, not folded into a looser needle that would also pass a *new*,
    /// undocumented unfiltered lookup elsewhere in the file — that is the
    /// difference between an allowlist and a weakened check.
    ///
    /// The needles are assembled at runtime, not written as one literal each:
    /// `include_str!` pulls in this test's own source, and a contiguous literal
    /// would match itself.
    #[test]
    fn collection_ownership_lookups_exclude_soft_deleted() {
        let src = include_str!("hosted.rs");
        let normalized = src
            .replace('\\', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        let lookup = format!("{} {}", "SELECT 1 FROM collections", "WHERE id = $1");
        let filtered = format!("{lookup} AND deleted_at IS NULL");
        // The one allowlisted inversion: `require_deleted_collection`'s own
        // idiom, assembled the same self-match-proof way.
        let exempt = format!("{lookup} AND deleted_at IS {} NULL", "NOT");
        let total = normalized.matches(&lookup).count();
        let guarded = normalized.matches(&filtered).count();
        let exempted = normalized.matches(&exempt).count();

        assert!(
            total > 0,
            "sanity: `{lookup}` is the ownership-check idiom and should still exist in hosted.rs"
        );
        assert_eq!(
            exempted, 1,
            "expected exactly one `require_deleted_collection`-shaped lookup \
             (`{exempt}`) — undo/restore's one documented exemption. Zero means the \
             helper was rewritten out from under this guard; more than one means a new \
             unfiltered-by-liveness lookup was added without being named here."
        );
        assert_eq!(
            total,
            guarded + exempted,
            "{} of {total} collection ownership lookups in hosted.rs are missing \
             `AND deleted_at IS NULL` and are not the one documented exemption above. \
             A soft-deleted collection must be as invisible to an ownership check as a \
             non-existent one (specs/collection-deletion.md → \"The read path\"), \
             otherwise it stays usable as a move destination or a write target.",
            total - guarded - exempted,
        );
    }

    /// The owned-per-oracle filter lives in exactly one place — the
    /// `owned_by_card` view — so that one place is worth pinning. Two ways to
    /// break it silently: drop the `WHERE c.deleted_at IS NULL` (hidden cards
    /// keep counting as owned everywhere at once), or restate the view without
    /// `security_invoker` (it then runs as its RLS-exempt owner and every user
    /// sees every user's counts).
    #[test]
    fn migration_0010_filters_and_invoker_scopes_the_owned_view() {
        let sql = include_str!("../../../migrations/0010_collection_soft_delete.sql");
        let normalized = sql.split_whitespace().collect::<Vec<_>>().join(" ");

        for needle in [
            "ALTER TABLE collections ADD COLUMN deleted_at timestamptz",
            "CREATE INDEX collections_user_parent_live_idx",
            "CREATE OR REPLACE VIEW owned_by_card WITH (security_invoker = true)",
            "WHERE c.deleted_at IS NULL",
        ] {
            assert!(
                normalized.contains(needle),
                "migrations/0010_collection_soft_delete.sql no longer contains `{needle}`. \
                 Soft deletion's column, its partial index, and the single filtered + \
                 invoker-scoped `owned_by_card` definition are what the whole read-path \
                 story rests on (specs/collection-deletion.md → Data model)."
            );
        }
    }
}
