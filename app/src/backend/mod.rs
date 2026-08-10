//! The data-access trait seam (specs/data-access-backends.md).
//!
//! Server functions and the hosted JSON routes program against the per-domain
//! [`CatalogStore`] / [`CollectionStore`] traits, never against sqlx or HTTP
//! directly. Two structs implement every trait — one per deployment target:
//!
//! - [`HostedBackend`] (feature `hosted`): in-process sqlx against Neon. It is
//!   the authorization terminus — it holds the `DATABASE_URL` pool and runs
//!   every session-scoped query inside a per-request transaction that
//!   `SET LOCAL app.user_id`, so data-model's RLS policies apply as a backstop.
//! - [`NativeBackend`] (feature `native`): an HTTPS client of the hosted JSON
//!   routes, forwarding the caller's Better Auth JWT as `Authorization: Bearer`.
//!   The Tauri binary contains no sqlx path at all.
//!
//! **Exactly one backend feature must be enabled** alongside `ssr` — enforced by
//! the compile_error below. Callers select the configured backend through the
//! per-request constructors on each struct; the choice is a compile-time cfg,
//! not a runtime branch, so the wrong backend can never be linked.
//!
//! This is the seam-proving slice: `card_count` (anonymous catalog probe) and
//! `list_collections` (session-scoped, exercises the GUC transaction).
//! collection-api extends these traits with the full method surface.

use shared::{
    AddHave, AddLine, AddWant, AllCardsView, ApiResult, BatchMove, CardDetail, CardSummary,
    CatalogCount, CollectionSummary, CollectionTree, CollectionView, DeckCommanders,
    DeleteCollectionReceipt, DeleteCollectionReq, DeletedCollectionRow, DesireLine, HoldingLine,
    HoldingMove, Id, LineResult, MoveReceipt, MoveRequest, NeedsView, NewCollection, NewTag, Page,
    Rename, RenameTag, Reorder, Reparent, SearchQuery, SearchResults, SetBoard, SetQuantity,
    SetQuery, SetSummary, ShoppingList, SuggestedDestination, Tag, TagAssignment, TaggedCard,
    Teardown, TeardownReceipt, UndoReceipt,
};

/// The pull request/response DTOs live beside the UI that renders them
/// ([`crate::my::needs`], unconditionally compiled — see that module's own
/// doc), not in `shared`, because they already crossed the wire as a Leptos
/// server-fn's argument/return types before this trait method existed —
/// [`crate::my::move_selection::Skipped`] is placed the same way, for the
/// same reason, for the batch-move adapter. That precedent is for the
/// **Leptos server-fn wire** only, though: every DTO the *native* `/api`
/// routes carried before this task lived in `shared`, so `PullItem`/
/// `PullOutcome` are the first `app::my` types to also cross that second,
/// native-client wire (`native.rs`/`routes.rs`, added by this task) — a new
/// precedent, not an extension of an existing one for that surface. The
/// hosted route and native client import them from here rather than
/// duplicating or relocating them.
use crate::my::needs::{PullItem, PullOutcome};

#[cfg(feature = "hosted")]
pub mod delete_plan;
#[cfg(feature = "hosted")]
pub mod hosted;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "hosted")]
pub mod pull_plan;
#[cfg(feature = "hosted")]
pub mod routes;

#[cfg(feature = "hosted")]
pub use hosted::HostedBackend;
#[cfg(feature = "native")]
pub use native::NativeBackend;

// A server build needs a concrete backend. `ssr` alone is the substrate (router
// + auth core); without `hosted` or `native` there is nothing to answer a data
// query, so fail loud at compile time rather than link a backend-less server.
#[cfg(all(feature = "ssr", not(any(feature = "hosted", feature = "native"))))]
compile_error!(
    "enable exactly one data-access backend alongside `ssr`: \
     `hosted` (web server, sqlx) or `native` (Tauri shell, HTTPS client). \
     See specs/data-access-backends.md."
);

/// The hosted route paths — the single source of truth the hosted router
/// (`routes.rs`) mounts and the native client calls, so the two cannot drift on
/// the URL. Operation-named / RPC-ish per specs/collection-api.md.
#[cfg(feature = "ssr")]
pub mod paths {
    use shared::Id;

    pub const CATALOG_COUNT: &str = "/api/catalog/count";
    pub const CATALOG_SEARCH: &str = "/api/catalog/search";
    /// GET one window of the set list — the rail's Set facet picker.
    pub const CATALOG_SETS: &str = "/api/catalog/sets";
    /// GET = list the tree; POST = create.
    pub const COLLECTIONS: &str = "/api/collections";
    /// GET the sidebar-tree read model (rows + counts). Static, so it cannot
    /// collide with the `/api/collections/{id}/<op>` templates.
    pub const COLLECTION_TREE: &str = "/api/collections/tree";
    /// GET the caller's soft-deleted collections (specs/collection-deletion.md
    /// → step 5, "Recently deleted"). Static, same reason as
    /// [`COLLECTION_TREE`]: it cannot collide with `/api/collections/{id}/<op>`.
    pub const RECENTLY_DELETED: &str = "/api/collections/recently-deleted";

    /// Card detail / summary (by oracle id).
    pub const CARD_DETAIL_ROUTE: &str = "/api/cards/{id}";
    pub const CARD_SUMMARY_ROUTE: &str = "/api/cards/{id}/summary";
    pub fn card_detail(oracle_id: Id) -> String {
        format!("/api/cards/{oracle_id}")
    }
    pub fn card_summary(oracle_id: Id) -> String {
        format!("/api/cards/{oracle_id}/summary")
    }

    /// Per-collection operation names — the shared vocabulary the router mounts
    /// (as `/api/collections/{id}/<op>`) and the client fills, so they can't drift.
    pub mod op {
        pub const RENAME: &str = "rename";
        pub const DELETE: &str = "delete";
        /// Undo a delete whole, from its own receipt (specs/collection-deletion.md
        /// → step 5, "Undo").
        pub const UNDO_DELETE: &str = "undo-delete";
        /// Restore a soft-deleted collection from the "Recently deleted" list —
        /// the weaker recovery path, deliberately (specs/collection-deletion.md
        /// → step 5, "Restore").
        pub const RESTORE: &str = "restore";
        pub const REPARENT: &str = "reparent";
        pub const REORDER: &str = "reorder";
        pub const HAVE: &str = "have";
        pub const WANT: &str = "want";
        pub const BATCH: &str = "batch";
        pub const VIEW: &str = "view";
        pub const TEARDOWN: &str = "teardown";
        pub const NEEDS: &str = "needs";
        /// Pull this collection's needs from the caller's other collections —
        /// one transaction, plan and write together (specs/collection-api.md
        /// → "Pull / Pull-all"; P6-120).
        pub const PULL: &str = "pull";
        /// GET the tags in scope for a collection (system + account + this deck's).
        pub const TAGS: &str = "tags";
        /// GET a deck's commanders + derived color identity.
        pub const COMMANDERS: &str = "commanders";
    }

    /// Tags & boards (specs/card-tagging.md, surface in collection-api §Tags &
    /// boards). Tag CRUD is a top-level resource; assignment carries all three
    /// ids in the body; per-card and per-tag reads and the board re-label hang
    /// off their anchor.
    pub const TAGS: &str = "/api/tags";
    pub const TAGS_ASSIGN: &str = "/api/tags/assign";
    pub const TAGS_UNASSIGN: &str = "/api/tags/unassign";

    /// Tag-by-id op (`rename`/`delete`) — the router mounts `tag_op_route(op)`,
    /// the client fills `tag_op(id, op)`, mirroring the per-collection ops.
    pub fn tag_op_route(op: &str) -> String {
        format!("/api/tags/{{id}}/{op}")
    }
    pub fn tag_op(id: Id, op: &str) -> String {
        format!("/api/tags/{id}/{op}")
    }

    /// A card's tags within a collection (by collection + oracle id).
    pub const CARD_TAGS_ROUTE: &str = "/api/collections/{id}/cards/{oracle}/tags";
    pub fn card_tags(collection_id: Id, oracle_id: Id) -> String {
        format!("/api/collections/{collection_id}/cards/{oracle_id}/tags")
    }

    /// A deck's cards carrying a given tag (by collection + tag id).
    pub const TAG_CARDS_ROUTE: &str = "/api/collections/{id}/tags/{tag}/cards";
    pub fn tag_cards(collection_id: Id, tag_id: Id) -> String {
        format!("/api/collections/{collection_id}/tags/{tag_id}/cards")
    }

    /// Re-label a holding / desire stack onto another board. Route template
    /// (`{id}` = holding / desire id) / client path.
    pub const HOLDING_BOARD_ROUTE: &str = "/api/holdings/{id}/board";
    pub fn holding_board(holding_id: Id) -> String {
        format!("/api/holdings/{holding_id}/board")
    }
    pub const DESIRE_BOARD_ROUTE: &str = "/api/desires/{id}/board";
    pub fn desire_board(desire_id: Id) -> String {
        format!("/api/desires/{desire_id}/board")
    }

    /// Global read models.
    pub const ALL_CARDS: &str = "/api/all-cards";
    pub const SHOPPING_LIST: &str = "/api/shopping-list";

    /// Move endpoints (not per-collection: a move spans two collections).
    pub const MOVES: &str = "/api/moves";
    pub const MOVES_BATCH: &str = "/api/moves/batch";
    /// Undo N moves in one transaction — the batch counterpart of
    /// `{id}/undo`, and the selection tray's single Undo.
    pub const MOVES_UNDO_BATCH: &str = "/api/moves/undo-batch";
    pub const MOVES_UNDO_LAST: &str = "/api/moves/undo-last";
    pub const MOVE_UNDO_ROUTE: &str = "/api/moves/{id}/undo";
    pub fn move_undo(move_id: Id) -> String {
        format!("/api/moves/{move_id}/undo")
    }

    /// Suggested destinations for a card (by oracle id).
    pub const CARD_DESTINATIONS_ROUTE: &str = "/api/cards/{id}/destinations";
    pub fn card_destinations(oracle_id: Id) -> String {
        format!("/api/cards/{oracle_id}/destinations")
    }

    /// The caller's holdings of a card, ungrouped (by oracle id).
    pub const CARD_HOLDINGS_ROUTE: &str = "/api/cards/{id}/holdings";
    pub fn card_holdings(oracle_id: Id) -> String {
        format!("/api/cards/{oracle_id}/holdings")
    }

    /// The axum route template for a per-collection operation (`{id}` param).
    pub fn collection_op_route(op: &str) -> String {
        format!("/api/collections/{{id}}/{op}")
    }

    /// The client-side path for an operation on a specific collection.
    pub fn collection_op(id: Id, op: &str) -> String {
        format!("/api/collections/{id}/{op}")
    }

    /// Set a holding's quantity. Route template (`{id}` = holding id) / client path.
    pub const HOLDING_QUANTITY_ROUTE: &str = "/api/holdings/{id}/quantity";
    pub fn holding_quantity(holding_id: Id) -> String {
        format!("/api/holdings/{holding_id}/quantity")
    }

    /// Move (or remove) a named holding stack — the grain-addressed write.
    pub const HOLDING_MOVE_ROUTE: &str = "/api/holdings/{id}/move";
    pub fn holding_move(holding_id: Id) -> String {
        format!("/api/holdings/{holding_id}/move")
    }
}

/// Catalog reads — anonymous-safe (the public IA routes). No session credential;
/// the backend struct is constructed without one.
#[cfg(feature = "ssr")]
#[allow(async_fn_in_trait)] // internal trait, always awaited on a concrete type
pub trait CatalogStore {
    /// Number of distinct oracle cards in the catalog (0 until ingestion runs).
    async fn card_count(&self) -> ApiResult<CatalogCount>;

    /// Full card page: oracle data + printings + rulings + related parts, and —
    /// when the backend carries a session — the caller's copies & locations.
    async fn card_detail(&self, oracle_id: Id) -> ApiResult<CardDetail>;

    /// The hover / quick-preview subset for a card; `owned` filled when authed.
    async fn card_summary(&self, oracle_id: Id) -> ApiResult<CardSummary>;

    /// One keyset page of catalog search results. This is the endpoint *shell*
    /// (specs/collection-api.md): the query→SQL translation is
    /// [catalog-search](../../specs/catalog-search.md)'s — until it lands, `q`
    /// does a fuzzy name match. Empty until catalog-ingestion populates the rows.
    ///
    /// Each row's `owned` follows the same authed-only rule as
    /// [`card_summary`](Self::card_summary) — the caller's global count when the
    /// backend carries a session, `None` (unknown, *not* zero) when anonymous.
    /// Ownership never affects *which* cards a search returns or how it pages.
    async fn search(&self, query: SearchQuery, page: Page) -> ApiResult<SearchResults>;

    /// One window of the set list, newest first — the vocabulary behind the
    /// filter rail's Set facet, so a user picks *Modern Horizons 3* instead of
    /// remembering `mh3`.
    ///
    /// Bounded by [`SetQuery::limit`] and filtered by [`SetQuery::term`] rather
    /// than returning all ~1050 sets; the reason is the picker's, and it is
    /// recorded on `SetQuery`. Public/anonymous like the rest of this trait —
    /// sets carry no ownership, so the answer never depends on the session.
    async fn list_sets(&self, query: SetQuery) -> ApiResult<Vec<SetSummary>>;
}

/// Collection reads/writes — session-scoped. The backend carries the caller's
/// identity (hosted: the verified `user_id`; native: the forwarded JWT), so
/// these methods take no credential argument. A backend built without a session
/// answers with [`shared::ApiError::Unauthorized`].
#[cfg(feature = "ssr")]
#[allow(async_fn_in_trait)]
pub trait CollectionStore {
    /// The caller's collections, flat (the client rebuilds the tree from
    /// `parent_id`). Runs inside the `SET LOCAL app.user_id` transaction on the
    /// hosted side, and **lazily provisions the Inbox** on first authed load
    /// (idempotent via the `collections_one_inbox` unique index).
    async fn list_collections(&self) -> ApiResult<Vec<CollectionSummary>>;

    /// The My-cards sidebar in one round-trip: every collection with its own
    /// present count plus the shopping-short badge count
    /// (specs/app-ui.md → Collection tree). Same flat shape and lazy Inbox
    /// provisioning as [`Self::list_collections`] — this read *is* a
    /// "first `/my` request".
    async fn collection_tree(&self) -> ApiResult<CollectionTree>;

    /// Create a binder or deck; returns the new node. Rejects a `format` on a
    /// binder (`Validation`) and a non-existent / not-owned `parent_id`
    /// (`NotFound`/`Forbidden`).
    async fn create_collection(&self, req: NewCollection) -> ApiResult<CollectionSummary>;

    /// Rename a collection; returns the updated node. The Inbox is unrenamable
    /// (`Conflict`).
    async fn rename_collection(&self, id: Id, req: Rename) -> ApiResult<CollectionSummary>;

    /// Delete a collection — which **relocates rather than destroys**
    /// (specs/collection-deletion.md). In one transaction: the live children
    /// re-point at the deleted node's parent (delete removes exactly one node),
    /// the holdings move out as real ledger moves per
    /// [`HaveDisposition`](shared::HaveDisposition), the desires follow
    /// [`WantDisposition`](shared::WantDisposition), and only then is
    /// `deleted_at` stamped. `Discard` on either side writes **nothing**: those
    /// rows stay attached to the hidden collection and return intact on undo.
    ///
    /// The Inbox is undeletable (`Conflict`); a soft-deleted collection is as
    /// absent as a non-existent one (`NotFound`). Returns the handles the undo
    /// toast needs — never a count.
    async fn delete_collection(
        &self,
        req: DeleteCollectionReq,
    ) -> ApiResult<DeleteCollectionReceipt>;

    /// **Undo** a delete whole, from its own receipt — the misclick path, on
    /// the delete toast (specs/collection-deletion.md → step 5). Clears
    /// `deleted_at` **first**, then reverses every `move_id` (the same
    /// [`Self::undo_moves`]), re-parents every id in `reparented` back, and
    /// re-inserts every relocated desire. A stale receipt (the collection
    /// already restored, or its parent deleted in the meantime) is a real
    /// error, not a silent no-op — see [`Self::restore_collection`] for the
    /// weaker, later path this is deliberately not.
    async fn undo_delete(&self, receipt: DeleteCollectionReceipt) -> ApiResult<()>;

    /// **Restore** a soft-deleted collection from the "Recently deleted" list,
    /// potentially days later (specs/collection-deletion.md → step 5). Clears
    /// `deleted_at`, re-attaches to the original parent if it is still live —
    /// otherwise top level — and leaves cards and children exactly where they
    /// now are. Not a time machine: unlike [`Self::undo_delete`] it does not
    /// reverse anything that happened to the collection's contents.
    async fn restore_collection(&self, id: Id) -> ApiResult<()>;

    /// The caller's soft-deleted collections, newest first — the "Recently
    /// deleted" list's read model (specs/collection-deletion.md → step 5).
    /// Deliberately thin: name, kind, when. No counts, no purge, no permanent
    /// delete.
    async fn recently_deleted(&self) -> ApiResult<Vec<DeletedCollectionRow>>;

    /// Move a collection under a new parent (or to top level). Rejects a cycle —
    /// the target being the node itself or one of its descendants (`Conflict`).
    async fn reparent_collection(&self, id: Id, req: Reparent) -> ApiResult<()>;

    /// Set a collection's fractional sort position among its siblings.
    async fn reorder_collection(&self, id: Id, req: Reorder) -> ApiResult<()>;

    /// `+ Have` — add present copies to a collection (upsert the holding,
    /// increment quantity, append an intake `moves` row). Returns the resulting
    /// holding. Rejects a non-owned collection (`NotFound`) and quantity ≤ 0
    /// (`Validation`).
    async fn add_holding(&self, collection_id: Id, req: AddHave) -> ApiResult<HoldingLine>;

    /// `+ Want` — add a desired count for a card in a collection (upsert the
    /// desire, increment quantity). Returns the resulting desire.
    async fn add_desire(&self, collection_id: Id, req: AddWant) -> ApiResult<DesireLine>;

    /// Set a holding's absolute quantity (the stepper). `0` deletes the row and
    /// returns `None`; otherwise the updated holding.
    async fn set_holding_quantity(
        &self,
        holding_id: Id,
        req: SetQuantity,
    ) -> ApiResult<Option<HoldingLine>>;

    /// Batch add (the enter-50-cards path): each line runs independently in its
    /// own transaction, so one bad line doesn't sink the batch — the result
    /// vector is positional (`results[i]` is `lines[i]`'s outcome).
    async fn batch_add(&self, collection_id: Id, lines: Vec<AddLine>)
        -> ApiResult<Vec<LineResult>>;

    /// One keyset page of a collection's card rows, with its metadata,
    /// immediate children, whole-collection totals and (for a deck) its
    /// commanders. Per-row counts (present / desired / owned / rolled-up) are
    /// computed for the visible page — the discipline that keeps a 100K-card
    /// view bounded (specs/collection-api.md → Read models); the header
    /// `totals` deliberately are not, since a header that changed as you paged
    /// would be describing the page rather than the collection. Rows include
    /// cards this collection only *wants* (see the hosted impl). Sorted by
    /// (name, printing, board); the cursor is opaque.
    ///
    /// `q` is the in-collection quick search — a plain card-name substring, not
    /// the catalog grammar (design/information-architecture.md → "Two search
    /// surfaces"), same as [`Self::all_cards`]'s.
    async fn collection_view(
        &self,
        id: Id,
        q: Option<String>,
        page: Page,
    ) -> ApiResult<CollectionView>;

    /// Move copies between collections in one transaction: decrement the source
    /// holding, upsert the destination, append a `moves` row. `from = None` is an
    /// intake, `to = None` a removal. Rejects insufficient source copies
    /// (`Conflict`). Returns the move id (for Undo).
    ///
    /// Addressed at the full grain **and** at a board on each end
    /// (`MoveRequest::from_board` / `to_board`): a deck's sideboard stack is a
    /// different stack of the same printing, and a write that assumed `main`
    /// would take copies the caller never pointed at.
    async fn move_cards(&self, req: MoveRequest) -> ApiResult<MoveReceipt>;

    /// Move copies **out of one named `holdings` row** — the same write as
    /// [`Self::move_cards`], addressed by the id of a stack instead of by a
    /// grain the caller has to re-state. `to = None` removes them (undoably).
    ///
    /// Two reasons it exists rather than being folded into `move_cards`:
    ///
    /// - **The caller cannot state the grain.** A rendered collection row is
    ///   `(printing, board)` with finish/condition/language summed away
    ///   (`collection_view`'s `present` CTE), so it can name the stack it is
    ///   showing but not the grain a move is addressed at. `CardRow::holding_id`
    ///   is exactly that name, and is `Some` precisely when one row backs the
    ///   cell.
    /// - **The check belongs inside the write transaction.** Resolving a grain
    ///   with a read and then writing it is a check-then-act across two
    ///   transactions; here the grain, the board, the owning collection and the
    ///   quantity are all read in the transaction that then performs the move,
    ///   so a concurrent change cannot land between them.
    ///
    /// `quantity = None` moves the whole stack. `NotFound` if the holding is not
    /// the caller's (RLS) or is already gone.
    async fn move_holding(&self, holding_id: Id, req: HoldingMove) -> ApiResult<MoveReceipt>;

    /// Batch move (the selection tray): many items to one destination, all in a
    /// single transaction — all-or-nothing, so a bad item rolls the batch back.
    async fn move_batch(&self, req: BatchMove) -> ApiResult<Vec<MoveReceipt>>;

    /// Undo a move: reverse its holdings effect and stamp `undone_at`. Idempotent
    /// (undoing an already-undone move is a no-op — the receipt's
    /// `restored_holding_id` is `None` on the second call, since nothing writes
    /// the second time).
    ///
    /// Returns [`UndoReceipt`], not `()`: a caller addressing a *specific*
    /// holding by id (the collection-view stepper) needs to know the id the
    /// reversal actually wrote to, since undoing a removal re-inserts under a
    /// **new** id rather than reviving the old one. `restored_holding_id` is
    /// `None` whenever there is no holding at the move's own origin to name —
    /// which also covers a soft-deleted origin: a hosted backend redirects
    /// those copies to the Inbox rather than losing them, and a caller still
    /// addressing the original collection must not be handed that unrelated
    /// holding (see [`UndoReceipt`]'s own doc).
    async fn undo_move(&self, move_id: Id) -> ApiResult<UndoReceipt>;

    /// Undo several moves **in one transaction** — the batch counterpart of
    /// [`Self::undo_move`], and the symmetric partner of [`Self::move_batch`].
    ///
    /// It exists because a batch move writes one ledger row per item, so the
    /// tray's single Undo has N rows to reverse: looping `undo_move` would be N
    /// transactions, and a failure partway would leave the batch half-reverted
    /// behind a toast that already said it undid the whole thing. Here the batch
    /// reverts wholly or not at all, exactly as it was applied. Idempotent per
    /// move, like `undo_move`.
    async fn undo_moves(&self, move_ids: Vec<Id>) -> ApiResult<()>;

    /// Undo the caller's most recent not-yet-undone move (⌘K "undo last move").
    /// Returns the undone move id, or `None` if there is nothing to undo.
    async fn undo_last_move(&self) -> ApiResult<Option<MoveReceipt>>;

    /// Every holding of a card the caller owns, **at full grain** — printing,
    /// finish, condition, language, board, quantity — across all their
    /// collections.
    ///
    /// The read models deliberately collapse that detail: `collection_view`
    /// groups by `(printing, board)` and `CardDetail::ownership` by
    /// `(collection, printing)`, so a row reading `present = 3` says nothing
    /// about whether those three are foil, played, Japanese, or sideboarded.
    /// A *move* is addressed at the full grain (`holding_take`), so anything
    /// deciding whether a move can be made needs the ungrouped rows — otherwise
    /// it can only find out by attempting the write and reading a `Conflict`
    /// back, which in a batch means killing the batch. Small by construction:
    /// one card's holdings, session-scoped.
    async fn holdings_of_oracle(&self, oracle_id: Id) -> ApiResult<Vec<HoldingLine>>;

    /// Collections that desire a card more than they currently hold — the
    /// move/pull destination ranking, shortfall-first.
    async fn suggested_destinations(&self, oracle_id: Id) -> ApiResult<Vec<SuggestedDestination>>;

    /// Empty a collection — move every holding to a chosen destination, or back
    /// to each card's previous location (most-recent move *into* here, else
    /// Inbox). One transaction; returns **the ids of the move rows it wrote**,
    /// so the caller can reverse the whole teardown through [`Self::undo_moves`]
    /// (⌘K's `Undo last move`) instead of guessing which move was last.
    async fn teardown(&self, collection_id: Id, mode: Teardown) -> ApiResult<TeardownReceipt>;

    /// The virtual everything-view: one keyset page of per-oracle rows
    /// aggregated across all the caller's collections — owned total, wanted
    /// total, and every collection holding a copy. Sorted by (name, oracle).
    ///
    /// `q` is the page's **quick search**, and it is deliberately *not* the
    /// catalog grammar (specs/catalog-search.md): a plain case-insensitive
    /// substring of the card name, because this box filters a list you already
    /// own rather than querying the catalog. An empty/whitespace `q` is the
    /// same as `None` — browse everything.
    async fn all_cards(&self, q: Option<String>, page: Page) -> ApiResult<AllCardsView>;

    /// A collection's needs: cards it desires beyond what it holds, each split
    /// into owned-elsewhere (with locations) and short-to-buy.
    async fn needs(&self, collection_id: Id) -> ApiResult<NeedsView>;

    /// **Pull** — fill `to_collection_id`'s needs from the caller's other
    /// collections (specs/app-ui.md → `/my/collections/:id/needs`; P6-120).
    ///
    /// One transaction: re-derives the destination's gap from a fresh
    /// `needs`-shaped read, locks every source stack the plan might draw
    /// from (`FOR UPDATE`, in a canonical order this method controls rather
    /// than the caller's — see [`crate::backend::pull_plan::oracle_ids_of`]),
    /// plans against exactly what is locked, then writes. Before this method
    /// existed, the same composition ran as three independently-committed
    /// calls (`needs` → `holdings_of_oracle` → `move_batch`) from the
    /// `pull_needs` server fn, leaving a window where the write could act on
    /// a plan the database had already moved past — the same check-then-act
    /// shape [`Self::move_holding`] closed for the single-row path, now
    /// closed here for the batch one.
    ///
    /// `items` carries **no quantity** — same rule as
    /// [`crate::my::needs`]'s own doc: a pull's count is the gap, a fact
    /// about the database at write time, never the caller's. Each item that
    /// cannot be honored comes back in [`crate::my::needs::PullOutcome::skipped`]
    /// with why, never silently dropped; a partial pull reports the copies it
    /// actually moved rather than the copies it was asked for.
    async fn pull_needs(
        &self,
        to_collection_id: Id,
        items: Vec<PullItem>,
    ) -> ApiResult<PullOutcome>;

    /// The global shopping list: cards short across the whole collection
    /// (total desired − owned > 0), with which collections want them.
    async fn shopping_list(&self) -> ApiResult<ShoppingList>;

    // --- Tags & boards (specs/card-tagging.md) ------------------------------

    /// Create an **account**- or **deck**-scoped tag (`req.collection_id`
    /// distinguishes). System tags are seeded, never created here. A duplicate
    /// name in the same scope is `Conflict`.
    async fn create_tag(&self, req: NewTag) -> ApiResult<Tag>;

    /// Rename one of the caller's tags. A built-in / not-owned tag is `NotFound`
    /// (RLS hides system tags from writes); a name clash is `Conflict`.
    async fn rename_tag(&self, tag_id: Id, req: RenameTag) -> ApiResult<Tag>;

    /// Delete one of the caller's tags — cascades its `card_tags` assignments.
    /// A built-in / not-owned tag is `NotFound`.
    async fn delete_tag(&self, tag_id: Id) -> ApiResult<()>;

    /// The tags in scope for a collection: the system built-ins + the caller's
    /// account tags + that collection's deck tags.
    async fn list_tags(&self, collection_id: Id) -> ApiResult<Vec<Tag>>;

    /// Assign a tag to a card in a collection (anchored at `(collection,
    /// oracle)`). Enforces: the card is in the deck (a holding or desire exists),
    /// a deck-scoped tag is applied only within its own collection, and the
    /// built-in caps — `commander` ≤ 2, `companion` ≤ 1 per deck. Idempotent.
    async fn assign_tag(&self, req: TagAssignment) -> ApiResult<()>;

    /// Remove a tag from a card in a collection. Idempotent (removing an absent
    /// assignment is a no-op).
    async fn unassign_tag(&self, req: TagAssignment) -> ApiResult<()>;

    /// A card's tags within a collection.
    async fn card_tags(&self, collection_id: Id, oracle_id: Id) -> ApiResult<Vec<Tag>>;

    /// A collection's cards carrying a given tag (built-in or user) — the
    /// "group a deck by a tag" read.
    async fn cards_with_tag(&self, collection_id: Id, tag_id: Id) -> ApiResult<Vec<TaggedCard>>;

    /// A deck's commanders (`commander` built-in tag) and the color identity
    /// derived from them (the WUBRG union of their `color_identity`, computed on
    /// read — never stored, so always current).
    async fn deck_commanders(&self, collection_id: Id) -> ApiResult<DeckCommanders>;

    /// Re-label part or all of a **holding** stack onto another board — a
    /// quantity-preserving in-place update, splitting the row when only part
    /// changes board and merging into the destination board's row if present.
    /// Not a `moves` entry. Boards apply to decks only (`Validation` on a binder).
    async fn set_holding_board(&self, holding_id: Id, req: SetBoard) -> ApiResult<()>;

    /// Re-label part or all of a **desire** stack onto another board (as
    /// [`set_holding_board`](Self::set_holding_board), for desired copies).
    async fn set_desire_board(&self, desire_id: Id, req: SetBoard) -> ApiResult<()>;
}
