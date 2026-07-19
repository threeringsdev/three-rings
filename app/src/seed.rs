//! Dev seed data (specs/app-ui.md → Dev seed data): build the test user's
//! realistic collection tree **via the real `CollectionStore`/`CatalogStore`
//! methods** — never raw SQL — so the seed exercises exactly the code paths
//! the `/my/*` screens will read back. Invoked as `server --seed-dev <uuid>`
//! (see `scripts/seed-dev-data.sh`, which resolves the e2e user's uuid).
//!
//! Idempotent by sentinel: if a collection named [`SENTINEL`] exists, the tree
//! is assumed seeded and nothing is written. Re-seeding from scratch =
//! recreate the e2e user (`end2end/seed-e2e-user.sh` with a fresh `.env`).

use shared::{
    AddHave, AddLine, AddWant, ApiError, Board, CollectionKind, Finish, Id, LineResult,
    MoveRequest, NewCollection, Page, SearchQuery, TagAssignment,
};
use uuid::Uuid;

use crate::backend::{CatalogStore, CollectionStore, HostedBackend};

/// The seed's presence marker; also the first collection it creates.
const SENTINEL: &str = "Trade Binder";

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
    if existing.iter().any(|c| c.name == SENTINEL) {
        println!("seed: '{SENTINEL}' already present — nothing to do");
        return Ok(Stats::default());
    }
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
    let mut roots: Vec<Id> = Vec::new();
    match build(&be, inbox, &mut roots).await {
        Ok(stats) => Ok(stats),
        Err(e) => {
            for id in roots {
                let _ = be.delete_collection(id).await;
            }
            Err(e)
        }
    }
}

async fn build(be: &HostedBackend, inbox: Id, roots: &mut Vec<Id>) -> Result<Stats, ApiError> {
    let mut stats = Stats::default();

    // -- catalog picks (POC subset; each query must return rows or the seed
    //    aborts with a clear error rather than building a half tree)
    let commanders = find(be, "t:legendary t:creature", 1).await?;
    let creatures = find(be, "t:creature", 12).await?;
    let instants = find(be, "t:instant", 4).await?;
    let lands = find(be, "t:land", 3).await?;

    // -- the tree (top-level ids recorded for cleanup-on-error; Rares
    //    cascades with Shoebox)
    let trade = create(be, None, CollectionKind::Binder, SENTINEL, None, &mut stats).await?;
    roots.push(trade);
    let shoebox = create(
        be,
        None,
        CollectionKind::Binder,
        "Shoebox",
        None,
        &mut stats,
    )
    .await?;
    roots.push(shoebox);
    let rares = create(
        be,
        Some(shoebox),
        CollectionKind::Binder,
        "Rares",
        None,
        &mut stats,
    )
    .await?;
    let deck = create(
        be,
        None,
        CollectionKind::Deck,
        "Commander Deck",
        Some("commander"),
        &mut stats,
    )
    .await?;
    roots.push(deck);

    // -- Inbox: a few unsorted arrivals
    for card in &creatures[0..4] {
        add_have(be, inbox, card.printing, 1, Board::Main, false, &mut stats).await?;
    }

    // -- Trade Binder: the bulk box (one foil playset for variety)
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
    batch(be, trade, trade_lines, &mut stats).await?;

    // -- Rares nested under Shoebox
    for card in &lands[0..2] {
        add_have(be, rares, card.printing, 1, Board::Main, false, &mut stats).await?;
    }

    // -- The deck: commander + mainboard, one sideboard card, and wants that
    //    populate the needs buckets: two owned-elsewhere (held in Trade
    //    Binder) and two short (never held anywhere → shopping list).
    let commander = &commanders[0];
    add_have(
        be,
        deck,
        commander.printing,
        1,
        Board::Main,
        false,
        &mut stats,
    )
    .await?;
    for card in creatures[0..3].iter().chain(&instants[0..3]) {
        add_have(be, deck, card.printing, 1, Board::Main, false, &mut stats).await?;
    }
    add_have(
        be,
        deck,
        instants[3].printing,
        1,
        Board::Side,
        false,
        &mut stats,
    )
    .await?;
    for card in &creatures[4..6] {
        add_want(be, deck, card.oracle, 1, &mut stats).await?; // owned in Trade Binder
    }
    for card in &creatures[10..12] {
        add_want(be, deck, card.oracle, 2, &mut stats).await?; // short → shopping list
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
        quantity: 1,
    })
    .await?;
    stats.moves += 1;

    Ok(stats)
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
