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
    CatalogCount, CollectionSummary, CollectionTree, CollectionView, DeckCommanders, DesireLine,
    HoldingLine, Id, LineResult, MoveReceipt, MoveRequest, NeedsView, NewCollection, NewTag, Page,
    Rename, RenameTag, Reorder, Reparent, SearchQuery, SearchResults, SetBoard, SetQuantity,
    ShoppingList, SuggestedDestination, Tag, TagAssignment, TaggedCard, Teardown, TeardownReceipt,
};

#[cfg(feature = "hosted")]
pub mod hosted;
#[cfg(feature = "native")]
pub mod native;
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
    /// GET = list the tree; POST = create.
    pub const COLLECTIONS: &str = "/api/collections";
    /// GET the sidebar-tree read model (rows + counts). Static, so it cannot
    /// collide with the `/api/collections/{id}/<op>` templates.
    pub const COLLECTION_TREE: &str = "/api/collections/tree";

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
        pub const REPARENT: &str = "reparent";
        pub const REORDER: &str = "reorder";
        pub const HAVE: &str = "have";
        pub const WANT: &str = "want";
        pub const BATCH: &str = "batch";
        pub const VIEW: &str = "view";
        pub const TEARDOWN: &str = "teardown";
        pub const NEEDS: &str = "needs";
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
    async fn search(&self, query: SearchQuery, page: Page) -> ApiResult<SearchResults>;
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

    /// Delete a collection (cascades to descendants + holdings/desires). The
    /// Inbox is undeletable (`Conflict`).
    async fn delete_collection(&self, id: Id) -> ApiResult<()>;

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

    /// One keyset page of a collection's card rows, with its metadata and
    /// immediate children. Counts (present / desired / owned / rolled-up) are
    /// computed for the visible page — the discipline that keeps a 100K-card
    /// view bounded (specs/collection-api.md → Read models). Sorted by
    /// (name, printing, board); the cursor is opaque.
    async fn collection_view(&self, id: Id, page: Page) -> ApiResult<CollectionView>;

    /// Move copies between collections in one transaction: decrement the source
    /// holding, upsert the destination, append a `moves` row. `from = None` is an
    /// intake, `to = None` a removal. Rejects insufficient source copies
    /// (`Conflict`). Returns the move id (for Undo).
    async fn move_cards(&self, req: MoveRequest) -> ApiResult<MoveReceipt>;

    /// Batch move (the selection tray): many items to one destination, all in a
    /// single transaction — all-or-nothing, so a bad item rolls the batch back.
    async fn move_batch(&self, req: BatchMove) -> ApiResult<Vec<MoveReceipt>>;

    /// Undo a move: reverse its holdings effect and stamp `undone_at`. Idempotent
    /// (undoing an already-undone move is a no-op).
    async fn undo_move(&self, move_id: Id) -> ApiResult<()>;

    /// Undo the caller's most recent not-yet-undone move (⌘K "undo last move").
    /// Returns the undone move id, or `None` if there is nothing to undo.
    async fn undo_last_move(&self) -> ApiResult<Option<MoveReceipt>>;

    /// Collections that desire a card more than they currently hold — the
    /// move/pull destination ranking, shortfall-first.
    async fn suggested_destinations(&self, oracle_id: Id) -> ApiResult<Vec<SuggestedDestination>>;

    /// Empty a collection — move every holding to a chosen destination, or back
    /// to each card's previous location (most-recent move *into* here, else
    /// Inbox). One transaction; returns how many move rows it wrote.
    async fn teardown(&self, collection_id: Id, mode: Teardown) -> ApiResult<TeardownReceipt>;

    /// The virtual everything-view: one keyset page of per-oracle rows
    /// aggregated across all the caller's collections (owned total + how many
    /// collections hold it). Sorted by (name, oracle).
    async fn all_cards(&self, page: Page) -> ApiResult<AllCardsView>;

    /// A collection's needs: cards it desires beyond what it holds, each split
    /// into owned-elsewhere (with locations) and short-to-buy.
    async fn needs(&self, collection_id: Id) -> ApiResult<NeedsView>;

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
