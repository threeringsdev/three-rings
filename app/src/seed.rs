//! Dev seed data (specs/app-ui.md → Dev seed data): build the test user's
//! realistic collection tree **via the real `CollectionStore`/`CatalogStore`
//! methods** — never raw SQL — so the seed exercises exactly the code paths
//! the `/my/*` screens will read back. Invoked as `server --seed-dev <uuid>`
//! (see `scripts/seed-dev-data.sh`, which resolves the e2e user's uuid).
//!
//! Idempotent by sentinel: if a collection named [`SENTINEL`] exists, the tree
//! is assumed seeded and nothing is written. Re-seeding from scratch =
//! recreate the e2e user (`end2end/seed-e2e-user.sh` with a fresh `.env`).
//!
//! **Three independently-sentinelled blocks.** The tree ([`SENTINEL`]) is one;
//! the bulk box ([`BULK`]) and the depth box ([`DEPTH`]) were added later and
//! are gated on their own names, so an already-seeded user picks them up on a
//! plain re-run rather than needing the whole fixture rebuilt. New blocks
//! should follow that shape — and each exists because some assertion could not
//! be *falsified* against the fixture without it (see their own docs).

use shared::{
    AddHave, AddLine, AddWant, ApiError, Board, CollectionKind, Finish, Id, LineResult,
    MoveRequest, NewCollection, Page, SearchQuery, TagAssignment,
};
use uuid::Uuid;

use crate::backend::{CatalogStore, CollectionStore, HostedBackend};

/// The seed's presence marker; also the first collection it creates.
const SENTINEL: &str = "Trade Binder";

/// The bulk box — its own sentinel, its own block (see the module doc).
const BULK: &str = "Bulk Box";

/// The depth box — the third independently-sentinelled block (see the module
/// doc), added for the collection view. See [`build_depth`] for what it exists
/// to make observable.
const DEPTH: &str = "Depth Box";

/// How many distinct cards the bulk box holds.
///
/// Not decoration: `/my`'s page size is 50, so without a fixture larger than
/// that the everything-view can never render a second page and its "Next page"
/// control is unreachable by any browser test (found by the Codex review of the
/// All-cards task). The rest of the tree contributes ~19 distinct cards, so 60
/// here puts the fixture comfortably past one page with room for the tree to
/// change. `/my/collections/:id` will want the same headroom.
const BULK_CARDS: usize = 60;

/// Sums of what one run wrote, for the closing println.
#[derive(Debug, Default)]
pub struct Stats {
    pub collections: u32,
    pub holdings: u32,
    pub desires: u32,
    pub moves: u32,
}

pub async fn run(user_id: Uuid) -> Result<Stats, ApiError> {
    let be = HostedBackend::for_user(user_id).await?;

    // list_collections lazily provisions the Inbox on first authed load.
    let existing = be.list_collections().await?;
    let inbox = existing
        .iter()
        .find(|c| c.is_inbox)
        .ok_or_else(|| ApiError::Upstream("no Inbox after list_collections".into()))?
        .id;

    // The store methods each commit independently (deliberately — the seed
    // exercises the real per-request paths), so a mid-seed failure would
    // otherwise strand a partial tree behind the sentinel. On error, delete
    // the root collections this run created (cascades to children/holdings);
    // Inbox arrivals may linger, which a re-run tolerates (add_holding upserts).
    let mut stats = Stats::default();
    let mut roots: Vec<Id> = Vec::new();
    let result = async {
        if existing.iter().any(|c| c.name == SENTINEL) {
            println!("seed: '{SENTINEL}' already present — skipping the tree");
        } else {
            build(&be, inbox, &mut roots, &mut stats).await?;
        }
        if existing.iter().any(|c| c.name == BULK) {
            println!("seed: '{BULK}' already present — skipping the bulk box");
        } else {
            build_bulk(&be, &mut roots, &mut stats).await?;
        }
        if existing.iter().any(|c| c.name == DEPTH) {
            println!("seed: '{DEPTH}' already present — skipping the depth box");
        } else {
            build_depth(&be, &mut roots, &mut stats).await?;
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => Ok(stats),
        Err(e) => {
            for id in roots {
                let _ = be.delete_collection(id).await;
            }
            Err(e)
        }
    }
}

/// The depth box: the shapes `/my/collections/:id`'s own render rules describe
/// and nothing else in the fixture has.
///
/// Three of that page's rules were **unfalsifiable** against the tree above —
/// each of the mutations below survived the entire Playwright suite, because no
/// collection in the fixture had the shape the rule is about (adversarial review
/// of the collection-view task):
///
/// - *a folder row's count is rolled up, not own.* The only nested collection
///   was a leaf, so the two were equal for it. `Depth Shelf` has a child of its
///   own, so its row reads 4 while it holds 1.
/// - *a card row shows its rolled-up part, and OWNED collapses against the total
///   of the two.* No card was held in both a collection and a descendant, so
///   `present_rollup` was 0 on every row in the database and the `+n` marker
///   never rendered anywhere. The shared printing sits at all three depths.
/// - *WANTED prints once per (card, board).* `desired` is oracle-grained and
///   repeats across a card's printing rows, but no collection held two printings
///   of one card. `Depth Box` holds two of one, and wants three.
///
/// Its cards are picked from oracles nothing else in the fixture owns or wants,
/// so `owned` for them is exactly what this subtree holds — which is what makes
/// the OWNED-collapse rule observable rather than accidentally true.
///
/// (The still-open "wants one card from **two** collections" gap — which makes
/// WANTED-is-a-sum indistinguishable from WANTED-is-a-max on `/my` — is a
/// different shape and stays its own queued task; this block does not smuggle
/// it in.)
async fn build_depth(
    be: &HostedBackend,
    roots: &mut Vec<Id>,
    stats: &mut Stats,
) -> Result<(), ApiError> {
    let taken = owned_or_wanted(be).await?;
    let (shared, multi) = depth_picks(be, &taken).await?;

    let (box_id, shelf, drawer) = {
        let b = create(be, None, CollectionKind::Binder, DEPTH, None, stats).await?;
        roots.push(b);
        let shelf = create(
            be,
            Some(b),
            CollectionKind::Binder,
            "Depth Shelf",
            None,
            stats,
        )
        .await?;
        let drawer = create(
            be,
            Some(shelf),
            CollectionKind::Binder,
            "Depth Drawer",
            None,
            stats,
        )
        .await?;
        (b, shelf, drawer)
    };

    // One printing at all three depths: 2 here, 1 + 3 below it.
    add_have(be, box_id, shared.printing, 2, Board::Main, false, stats).await?;
    add_have(be, shelf, shared.printing, 1, Board::Main, false, stats).await?;
    add_have(be, drawer, shared.printing, 3, Board::Main, false, stats).await?;

    // Two printings of one card in one collection, plus a desire on the card —
    // the two rows that must show WANTED exactly once between them.
    add_have(be, box_id, multi.printings[0], 1, Board::Main, false, stats).await?;
    add_have(be, box_id, multi.printings[1], 1, Board::Main, false, stats).await?;
    add_want(be, box_id, multi.oracle, 3, stats).await?;
    Ok(())
}

/// Every oracle the fixture already owns or wants — the set this block's picks
/// must avoid so its `owned` totals stay self-contained. Walks the whole
/// everything-view rather than one page: it is already larger than a page.
async fn owned_or_wanted(be: &HostedBackend) -> Result<Vec<Id>, ApiError> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = be
            .all_cards(
                None,
                Page {
                    cursor,
                    limit: Some(200),
                },
            )
            .await?;
        out.extend(page.cards.iter().map(|r| r.card.oracle_id));
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return Ok(out),
        }
    }
}

/// A pick with every printing of its card, for the two-printings-of-one-card row.
struct MultiPick {
    oracle: Id,
    printings: Vec<Id>,
}

/// Find the two cards this block needs, from oracles `taken` does not contain:
/// any unused card, and an unused card with **two or more printings**. Walks the
/// catalog in pages and fails loudly rather than building a partial box (the
/// seed's convention).
async fn depth_picks(be: &HostedBackend, taken: &[Id]) -> Result<(Pick, MultiPick), ApiError> {
    let mut shared: Option<Pick> = None;
    let mut multi: Option<MultiPick> = None;
    let mut cursor = None;
    for _ in 0..6 {
        let page = be
            .search(
                SearchQuery { q: None },
                Page {
                    cursor,
                    limit: Some(200),
                },
            )
            .await?;
        for card in &page.cards {
            if taken.contains(&card.oracle_id) {
                continue;
            }
            let printings: Vec<Id> = be
                .card_detail(card.oracle_id)
                .await?
                .printings
                .iter()
                .map(|p| p.id)
                .collect();
            if printings.is_empty() {
                continue;
            }
            if shared.is_none() {
                shared = Some(Pick {
                    oracle: card.oracle_id,
                    printing: printings[0],
                });
                continue;
            }
            if multi.is_none() && printings.len() >= 2 {
                multi = Some(MultiPick {
                    oracle: card.oracle_id,
                    printings,
                });
            }
            // `take()` on both unconditionally would drop whichever is already
            // set when the other is not.
            if shared.is_some() && multi.is_some() {
                return Ok((shared.take().unwrap(), multi.take().unwrap()));
            }
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    Err(ApiError::Validation(
        "seed: the catalog has no unused card with two printings — is the POC \
         catalog ingested on this branch?"
            .into(),
    ))
}

/// The bulk box: one flat binder holding [`BULK_CARDS`] distinct cards, so the
/// fixture exceeds a page of any `/my/*` view (see [`BULK_CARDS`]). Deliberately
/// boring — one copy each, no wants, no nesting — because its whole job is
/// volume; the interesting shapes live in [`build`].
///
/// **It skips whatever is currently short.** The picks are the catalog's
/// alphabetically-first cards, which overlaps the deck's `t:creature` picks —
/// including the two the tree deliberately wants and holds *nowhere*. Filling
/// the bulk box blindly quietly owns them, and the "short → shopping list" leg
/// of the fixture (which `/my`, the needs view and `/my/shopping` all read)
/// evaporates. Found the first time this block ran.
async fn build_bulk(
    be: &HostedBackend,
    roots: &mut Vec<Id>,
    stats: &mut Stats,
) -> Result<(), ApiError> {
    let short: Vec<Id> = be
        .shopping_list()
        .await?
        .rows
        .into_iter()
        .map(|r| r.oracle_id)
        .collect();
    // Over-fetch by the number skipped so the box still reaches BULK_CARDS.
    let picks: Vec<Pick> = find(be, "", BULK_CARDS + short.len())
        .await?
        .into_iter()
        .filter(|p| !short.contains(&p.oracle))
        .take(BULK_CARDS)
        .collect();

    let bulk = create(be, None, CollectionKind::Binder, BULK, None, stats).await?;
    roots.push(bulk);
    let lines: Vec<AddLine> = picks
        .iter()
        .map(|c| {
            AddLine::Have(AddHave {
                printing_id: c.printing,
                quantity: 1,
                ..have_defaults()
            })
        })
        .collect();
    batch(be, bulk, lines, stats).await
}

async fn build(
    be: &HostedBackend,
    inbox: Id,
    roots: &mut Vec<Id>,
    stats: &mut Stats,
) -> Result<(), ApiError> {
    // -- catalog picks (POC subset; each query must return rows or the seed
    //    aborts with a clear error rather than building a half tree)
    let commanders = find(be, "t:legendary t:creature", 1).await?;
    let creatures = find(be, "t:creature", 12).await?;
    let instants = find(be, "t:instant", 4).await?;
    let lands = find(be, "t:land", 3).await?;

    // -- the tree (top-level ids recorded for cleanup-on-error; Rares
    //    cascades with Shoebox)
    let trade = create(be, None, CollectionKind::Binder, SENTINEL, None, stats).await?;
    roots.push(trade);
    let shoebox = create(be, None, CollectionKind::Binder, "Shoebox", None, stats).await?;
    roots.push(shoebox);
    let rares = create(
        be,
        Some(shoebox),
        CollectionKind::Binder,
        "Rares",
        None,
        stats,
    )
    .await?;
    let deck = create(
        be,
        None,
        CollectionKind::Deck,
        "Commander Deck",
        Some("commander"),
        stats,
    )
    .await?;
    roots.push(deck);

    // -- Inbox: a few unsorted arrivals
    for card in &creatures[0..4] {
        add_have(be, inbox, card.printing, 1, Board::Main, false, stats).await?;
    }

    // -- Trade Binder: mixed quantities (one foil playset for variety)
    let trade_lines: Vec<AddLine> = creatures[4..10]
        .iter()
        .enumerate()
        .map(|(i, c)| {
            AddLine::Have(AddHave {
                printing_id: c.printing,
                finish: if i == 0 {
                    Finish::Foil
                } else {
                    Finish::Nonfoil
                },
                quantity: if i == 0 { 4 } else { 1 + (i as i32 % 3) },
                ..have_defaults()
            })
        })
        .collect();
    batch(be, trade, trade_lines, stats).await?;

    // -- Rares nested under Shoebox
    for card in &lands[0..2] {
        add_have(be, rares, card.printing, 1, Board::Main, false, stats).await?;
    }

    // -- The deck: commander + mainboard, one sideboard card, and wants that
    //    populate the needs buckets: two owned-elsewhere (held in Trade
    //    Binder) and two short (never held anywhere → shopping list).
    let commander = &commanders[0];
    add_have(be, deck, commander.printing, 1, Board::Main, false, stats).await?;
    for card in creatures[0..3].iter().chain(&instants[0..3]) {
        add_have(be, deck, card.printing, 1, Board::Main, false, stats).await?;
    }
    add_have(be, deck, instants[3].printing, 1, Board::Side, false, stats).await?;
    for card in &creatures[4..6] {
        add_want(be, deck, card.oracle, 1, stats).await?; // owned in Trade Binder
    }
    for card in &creatures[10..12] {
        add_want(be, deck, card.oracle, 2, stats).await?; // short → shopping list
    }

    // -- commander tag (the built-in system tag, found by name in deck scope)
    let tags = be.list_tags(deck).await?;
    let commander_tag = tags
        .iter()
        .find(|t| t.name == "commander")
        .ok_or_else(|| ApiError::Upstream("built-in commander tag not found".into()))?;
    be.assign_tag(TagAssignment {
        collection_id: deck,
        oracle_id: commander.oracle,
        tag_id: commander_tag.id,
    })
    .await?;

    // -- one real move for undo/pull history: a copy Trade Binder → Shoebox
    be.move_cards(MoveRequest {
        from_collection_id: Some(trade),
        to_collection_id: Some(shoebox),
        printing_id: creatures[4].printing,
        finish: Finish::Foil,
        condition: Default::default(),
        language: shared::collection::default_language(),
        from_board: Default::default(),
        to_board: Default::default(),
        quantity: 1,
    })
    .await?;
    stats.moves += 1;

    Ok(())
}

/// A picked card: oracle + its first printing.
struct Pick {
    oracle: Id,
    printing: Id,
}

/// Search the catalog and resolve each hit's first printing. Errors if the
/// query can't fill `n` — a half-seeded tree is worse than a loud failure.
async fn find(be: &HostedBackend, q: &str, n: usize) -> Result<Vec<Pick>, ApiError> {
    let results = be
        .search(
            SearchQuery {
                q: Some(q.to_string()),
            },
            Page {
                cursor: None,
                limit: Some(n as u32 + 5),
            },
        )
        .await?;
    let mut picks = Vec::with_capacity(n);
    for card in results.cards.iter().take(n) {
        let detail = be.card_detail(card.oracle_id).await?;
        let printing = detail
            .printings
            .first()
            .ok_or_else(|| ApiError::Upstream(format!("no printings for {}", card.name)))?;
        picks.push(Pick {
            oracle: card.oracle_id,
            printing: printing.id,
        });
    }
    if picks.len() < n {
        return Err(ApiError::Validation(format!(
            "seed query '{q}' found {}/{n} cards — is the POC catalog ingested on this branch?",
            picks.len()
        )));
    }
    Ok(picks)
}

async fn create(
    be: &HostedBackend,
    parent_id: Option<Id>,
    kind: CollectionKind,
    name: &str,
    format: Option<&str>,
    stats: &mut Stats,
) -> Result<Id, ApiError> {
    let created = be
        .create_collection(NewCollection {
            parent_id,
            kind,
            name: name.to_string(),
            format: format.map(str::to_string),
        })
        .await?;
    stats.collections += 1;
    Ok(created.id)
}

fn have_defaults() -> AddHave {
    AddHave {
        printing_id: Uuid::nil(),
        finish: Finish::Nonfoil,
        condition: Default::default(),
        language: shared::collection::default_language(),
        board: Board::Main,
        quantity: 0,
    }
}

async fn add_have(
    be: &HostedBackend,
    collection: Id,
    printing: Id,
    quantity: i32,
    board: Board,
    foil: bool,
    stats: &mut Stats,
) -> Result<(), ApiError> {
    be.add_holding(
        collection,
        AddHave {
            printing_id: printing,
            finish: if foil { Finish::Foil } else { Finish::Nonfoil },
            board,
            quantity,
            ..have_defaults()
        },
    )
    .await?;
    stats.holdings += 1;
    Ok(())
}

async fn add_want(
    be: &HostedBackend,
    collection: Id,
    oracle: Id,
    quantity: i32,
    stats: &mut Stats,
) -> Result<(), ApiError> {
    be.add_desire(
        collection,
        AddWant {
            oracle_id: oracle,
            printing_id: None,
            board: Board::Main,
            quantity,
        },
    )
    .await?;
    stats.desires += 1;
    Ok(())
}

async fn batch(
    be: &HostedBackend,
    collection: Id,
    lines: Vec<AddLine>,
    stats: &mut Stats,
) -> Result<(), ApiError> {
    let n = lines.len() as u32;
    let results = be.batch_add(collection, lines).await?;
    if let Some(LineResult::Error { error }) = results
        .iter()
        .find(|r| matches!(r, LineResult::Error { .. }))
    {
        return Err(error.clone());
    }
    stats.holdings += n;
    Ok(())
}
