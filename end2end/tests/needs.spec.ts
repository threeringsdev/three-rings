// Needs view + pick list + `/my/shopping` (specs/app-ui.md →
// `/my/collections/:id/needs`, `/my/shopping`).
//
// What this file exists to prove, in assertion order:
//
// - **the split is right.** Every copy of a card's gap is either fillable from
//   another of your collections or has to be bought, and the two buckets have to
//   add up to the headline — a card whose gap is *partly* fillable belongs in
//   both, and dropping it from either would lose copies;
// - **Pull writes.** The row button and the pick-list tick both perform a real
//   `move_cards`, read back through `/api/cards/{id}/holdings` (the one read
//   that does not group the grain away) rather than off a toast;
// - **Pull all is grouped by where you walk**, and its per-group counts are the
//   allocation the server then performs — not a hopeful client number;
// - **the export is pasteable text**, one `N Card Name` line per short card;
// - **the board case behaves as decided.** `needs()` is board-blind by design
//   (see below and the `my::needs` module doc). The test pins today's decision
//   *on purpose*: if someone makes needs board-aware, this test must fail and
//   the decision must be re-taken, rather than the page quietly changing shape.
//
// **On the board case.** A deck holding a card on `main` and wanting it on
// `side` renders two rows on the collection page — a mainboard row with copies
// and a sideboard row wanting one — while `/needs` shows nothing and the header
// carries no chip. That is deliberate: both needs buckets are defined by an
// operation (pull copies in, or buy them), and neither can fix a mis-boarded
// copy — the deck already *has* it. The outstanding action is a board relabel,
// which is card-tagging's quantity-preserving op and not a move.
//
// Isolation: scratch `zz-e2e-…` collections created via the API and deleted in a
// `finally`; printings come from catalog cards the dev fixture owns nowhere, so
// every count read back is this test's own writes. `/my/shopping` is global and
// therefore shared — its assertions are containment, never equality.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const TOAST = '[data-name="Toast"]';
const SUMMARY = '[data-testid="needs-summary"]';
const NEED_ROW = '[data-testid="needs-row"]';
const SHORT_ROW = '[data-testid="short-row"]';
const PICK_LIST = '[data-testid="pick-list"]';
const PICK_GROUP = '[data-testid="pick-group"]';
const PICK_ROW = '[data-testid="pick-row"]';

type Summary = { id: string; name: string };
type Card = { oracle_id: string; printing_id: string | null; name: string };
type Holding = {
  collection_id: string;
  finish: string;
  board: string;
  quantity: number;
};

let scratchSeq = 0;
/// Worker index, a file counter **and** a random tail: this file locates pick
/// groups by collection *name*, and a test that dies before its `finally` (a
/// timeout under a loaded suite) leaves a collection the next run at the same
/// worker index would name identically — two same-named groups make the locator
/// ambiguous, which reads as a UI bug and is not one.
const scratchName = (what: string) =>
  `zz-e2e-needs-${what}-w${test.info().workerIndex}-${++scratchSeq}-` +
  Math.random().toString(36).slice(2, 7);

async function createCollection(
  request: APIRequestContext,
  kind: "binder" | "deck",
  what: string,
): Promise<Summary> {
  const name = scratchName(what);
  const res = await request.post("/api/collections", {
    data: { parent_id: null, kind, name, format: null },
  });
  expect(res.status(), `create ${name}`).toBe(200);
  return (await res.json()) as Summary;
}

const deleteCollection = (request: APIRequestContext, id: string) =>
  request.post(`/api/collections/${id}/delete`, { data: {} });

/// Put copies into a collection at an explicit grain. `grain` reaches the fields
/// `AddHave` defaults — finish, condition, language, board — which is the only
/// reason a test here can tell a grain-aware pull from one that assumes the
/// default.
async function addHave(
  request: APIRequestContext,
  id: string,
  printingId: string,
  quantity = 1,
  grain: { finish?: string; condition?: string; board?: string } = {},
) {
  const res = await request.post(`/api/collections/${id}/have`, {
    data: { printing_id: printingId, quantity, ...grain },
  });
  expect(res.status(), "add have").toBe(200);
}

/// Want copies of a card. `board` matters for exactly one test — the deck that
/// wants on the sideboard what it holds on the mainboard.
async function addWant(
  request: APIRequestContext,
  id: string,
  oracleId: string,
  quantity = 1,
  board?: string,
) {
  const res = await request.post(`/api/collections/${id}/want`, {
    data: { oracle_id: oracleId, quantity, ...(board ? { board } : {}) },
  });
  expect(res.status(), "add want").toBe(200);
}

/// Every copy of a card the signed-in user holds, ungrouped.
async function holdingsOf(
  request: APIRequestContext,
  oracleId: string,
): Promise<Holding[]> {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status(), "holdings").toBe(200);
  return (await res.json()) as Holding[];
}

/// Copies of a card in one collection, summed across grains — `0` where the
/// holding is gone entirely, which is what pulling the last copy does.
async function copiesIn(
  request: APIRequestContext,
  oracleId: string,
  collectionId: string,
): Promise<number> {
  const rows = await holdingsOf(request, oracleId);
  return rows
    .filter((h) => h.collection_id === collectionId)
    .reduce((n, h) => n + h.quantity, 0);
}

type NeedRow = {
  oracle_id: string;
  desired: number;
  present_here: number;
  owned_elsewhere: number;
  short: number;
};

async function needsOf(
  request: APIRequestContext,
  id: string,
): Promise<NeedRow[]> {
  const res = await request.get(`/api/collections/${id}/needs`);
  expect(res.status(), `needs ${id}`).toBe(200);
  return ((await res.json()) as { rows: NeedRow[] }).rows;
}

/// Catalog cards the signed-in user owns nowhere, with a real printing.
/// `q=z` rather than a vowel: the seed picked its own cards from name-ordered
/// searches, so the alphabetically-first slice of the catalog is exactly the
/// slice the dev user already owns.
async function unownedCards(
  request: APIRequestContext,
  n: number,
  skip = 0,
): Promise<Card[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const { cards: owned } = (await mine.json()) as { cards: { card: Card }[] };
  const taken = new Set(owned.map((r) => r.card.oracle_id));
  const res = await request.get("/api/catalog/search?q=z&limit=60");
  expect(res.status(), "catalog search").toBe(200);
  const { cards } = (await res.json()) as { cards: Card[] };
  const free = cards
    .filter((c) => c.printing_id && !taken.has(c.oracle_id))
    .slice(skip, skip + n);
  expect(
    free.length,
    `the fixture has fewer than ${n + skip} catalog cards the dev user owns nowhere`,
  ).toBe(n);
  return free;
}

const needRow = (page: Page, oracleId: string) =>
  page.locator(`${NEED_ROW}[data-oracle="${oracleId}"]`);
const shortRow = (page: Page, oracleId: string) =>
  page.locator(`${SHORT_ROW}[data-oracle="${oracleId}"]`);

// ------------------------------------------------------------- the split ---

test("@fast the two buckets split the gap by what you already own", async ({
  page,
  request,
}) => {
  // One card owned elsewhere but not enough of it, one owned nowhere at all.
  // The first is the interesting one: 4 wanted, 3 findable, so it is *both* an
  // Owned-elsewhere row and a Short row, and the headline is the sum of the two
  // buckets rather than of the rows.
  const [partly, none] = await unownedCards(request, 2);
  const source = await createCollection(request, "binder", "src");
  const deck = await createCollection(request, "deck", "dst");
  try {
    await addHave(request, source.id, partly.printing_id as string, 3);
    await addWant(request, deck.id, partly.oracle_id, 4);
    await addWant(request, deck.id, none.oracle_id, 2);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);

    // 4 + 2 missing, of which 3 are findable — the same sentence, from the same
    // formatter, as the chip on the collection header.
    await expect(page.locator(SUMMARY)).toHaveText(
      "6 missing — 3 owned elsewhere · 3 to buy",
    );

    // Owned elsewhere: only the card someone holds, showing where and how many
    // of the gap it closes.
    await expect(page.locator(NEED_ROW)).toHaveCount(1);
    const row = needRow(page, partly.oracle_id);
    await expect(row.locator('[data-testid="need-gap"]')).toHaveText("4");
    await expect(row.locator('[data-testid="need-fillable"]')).toHaveText("3");
    await expect(row.locator('[data-testid="need-locations"]')).toContainText(
      `3 in ${source.name}`,
    );

    // Short: both cards, because three of the first card's four are findable and
    // the fourth is not. A row that appeared in only one bucket would lose
    // copies out of the other's total.
    await expect(page.locator(SHORT_ROW)).toHaveCount(2);
    await expect(
      shortRow(page, partly.oracle_id).locator('[data-testid="short-count"]'),
    ).toHaveText("1");
    await expect(
      shortRow(page, none.oracle_id).locator('[data-testid="short-count"]'),
    ).toHaveText("2");
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, deck.id);
  }
});

// ---------------------------------------------------------- one-tap Pull ---

test("@fast Pull moves the copies it names, at the grain they are held", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1);
  const source = await createCollection(request, "binder", "pull-src");
  const deck = await createCollection(request, "deck", "pull-dst");
  try {
    // A foil-only stack: a pull that restated the default grain would aim at
    // copies that do not exist and move nothing, while the toast said otherwise.
    await addHave(request, source.id, card.printing_id as string, 3, {
      finish: "foil",
    });
    await addWant(request, deck.id, card.oracle_id, 2);
    expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(0);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);
    await needRow(page, card.oracle_id)
      .locator('[data-testid="pull-row"]')
      .click();
    await expect(page.locator(TOAST)).toContainText("Pulled 2 copies");

    // Read back through the API, not off the toast.
    await expect(async () => {
      expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(2);
      expect(await copiesIn(request, card.oracle_id, source.id)).toBe(1);
    }).toPass({ timeout: 5000 });
    const landed = (await holdingsOf(request, card.oracle_id)).filter(
      (h) => h.collection_id === deck.id,
    );
    expect(landed.map((h) => h.finish)).toEqual(["foil"]);

    // The need is closed, so the page it was pulled from says so.
    await expect(page.locator('[data-testid="needs-empty"]')).toBeVisible();
    expect(await needsOf(request, deck.id)).toEqual([]);

    // ...and Undo puts them back where they came from.
    await page.locator(TOAST).getByRole("button", { name: "Undo" }).click();
    await expect(async () => {
      expect(await copiesIn(request, card.oracle_id, source.id)).toBe(3);
      expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(0);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, deck.id);
  }
});

// ------------------------------------------------------------- pick list ---

test("@fast Pull all groups the walk by source, and each tick records its move", async ({
  page,
  request,
}) => {
  // Two sources for one card, a gap smaller than their sum: the allocation has
  // to spend the bigger pile first and stop when the gap closes, which is what
  // makes the two group counts different numbers and the test meaningful.
  test.slow();
  const [card] = await unownedCards(request, 1);
  const big = await createCollection(request, "binder", "big");
  const small = await createCollection(request, "binder", "small");
  const deck = await createCollection(request, "deck", "pick-dst");
  try {
    await addHave(request, big.id, card.printing_id as string, 3);
    await addHave(request, small.id, card.printing_id as string, 2);
    await addWant(request, deck.id, card.oracle_id, 4);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);
    await page.locator('[data-testid="pull-all"]').click();

    const list = page.locator(PICK_LIST);
    await expect(list.locator(PICK_GROUP)).toHaveCount(2);
    const bigGroup = list.locator(PICK_GROUP, { hasText: big.name });
    const smallGroup = list.locator(PICK_GROUP, { hasText: small.name });
    // 3 from the bigger pile, then 1 of the 2 in the smaller — the gap is 4,
    // not 5, and the checklist must not tell you to fetch a card you do not
    // need.
    await expect(bigGroup.locator('[data-testid="pick-label"]')).toHaveText(
      `3 × ${card.name}`,
    );
    await expect(smallGroup.locator('[data-testid="pick-label"]')).toHaveText(
      `1 × ${card.name}`,
    );

    // Tick the smaller pile: exactly one copy moves, out of that collection.
    await smallGroup.locator('[data-name="Checkbox"]').click();
    await expect(smallGroup.locator(PICK_ROW)).toHaveAttribute(
      "data-state",
      "pulled",
    );
    await expect(async () => {
      expect(await copiesIn(request, card.oracle_id, small.id)).toBe(1);
      expect(await copiesIn(request, card.oracle_id, big.id)).toBe(3);
      expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(1);
    }).toPass({ timeout: 5000 });

    // The list is a snapshot of a physical walk: ticking one line must not
    // rebuild the other under the hand holding it. (The table above it *does*
    // refetch — a tick bumps the holdings revision — which is exactly why the
    // checklist has to live outside that payload. When it did not, the last
    // tick emptied the needs rows and took the whole list, and its Done button,
    // off the page.)
    await expect(bigGroup.locator('[data-testid="pick-label"]')).toHaveText(
      `3 × ${card.name}`,
    );

    await bigGroup.locator('[data-name="Checkbox"]').click();
    await expect(async () => {
      expect(await copiesIn(request, card.oracle_id, big.id)).toBe(0);
      expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(4);
    }).toPass({ timeout: 5000 });

    // The gap is closed — the table said so before the walk ended — and the
    // list survives to be dismissed by hand.
    await expect(page.locator('[data-testid="needs-empty"]')).toBeVisible();
    await expect(list).toBeVisible();
    await page.locator('[data-testid="pick-list-close"]').click();
    await expect(page.locator(PICK_LIST)).toHaveCount(0);
    expect(await needsOf(request, deck.id)).toEqual([]);
  } finally {
    await deleteCollection(request, big.id);
    await deleteCollection(request, small.id);
    await deleteCollection(request, deck.id);
  }
});

// -------------------------------------------------------- shopping + export ---

test("@fast the shopping list states the shortfall and exports it as text", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1);
  const deck = await createCollection(request, "deck", "shop");
  try {
    // 3 wanted, 1 owned — so the shortfall is 2, and the export line has to be
    // the *shortfall*, not the want. A list that told you to buy three when you
    // own one is the single worst thing this page can do.
    const binder = await createCollection(request, "binder", "shop-have");
    try {
      await addHave(request, binder.id, card.printing_id as string, 1);
      await addWant(request, deck.id, card.oracle_id, 3);

      await page.goto("/my/shopping");
      await hydrated(page);

      const row = page.locator(
        `[data-testid="shopping-row"][data-oracle="${card.oracle_id}"]`,
      );
      await expect(row.locator('[data-testid="shortfall"]')).toHaveText("2");
      await expect(row.locator('[data-testid="wanted-by"]')).toHaveText(
        deck.name,
      );

      // The export is the page's deliverable: `N Card Name`, pasteable as is.
      // Containment, not equality — the list is global and other collections
      // are on it.
      const text = await page
        .locator('[data-testid="shopping-export"]')
        .inputValue();
      expect(text).toContain(`2 ${card.name}`);
      expect(text).not.toContain(`3 ${card.name}`);
      // One line per card, no header and no annotation: every line must parse.
      for (const line of text.split("\n")) {
        expect(line).toMatch(/^\d+ \S/);
      }
    } finally {
      await deleteCollection(request, binder.id);
    }
  } finally {
    await deleteCollection(request, deck.id);
  }
});

// ------------------------------------------------------------ the board case ---

test("@fast a want on the sideboard of a card held on the mainboard is not a need", async ({
  page,
  request,
}) => {
  // **This test pins a decision, not just a behavior** (see the file header).
  // `needs()` groups desires and holdings by oracle alone, so this deck — one
  // copy on `main`, one wanted on `side` — has nothing missing: it already owns
  // the card. The sideboard row on the collection page still shows an unfilled
  // slot, and that is a *different* statement, fixable only by a board relabel
  // (card-tagging), which is not a move and not something either needs bucket
  // can offer. If needs is ever made board-aware, this test must fail loudly.
  const [card] = await unownedCards(request, 1);
  const deck = await createCollection(request, "deck", "board");
  try {
    await addHave(request, deck.id, card.printing_id as string, 1, {
      board: "main",
    });
    await addWant(request, deck.id, card.oracle_id, 1, "side");

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    const sideRow = page.locator(
      `[data-testid="collection-row"][data-printing="${card.printing_id}"][data-board="side"]`,
    );
    await expect(sideRow.locator('[data-testid="wanted-count"]')).toHaveText(
      "1",
    );
    // No chip: the header agrees with the needs page it links to, which is the
    // whole reason the arithmetic was not half-fixed in one of them.
    await expect(page.locator('[data-testid="needs-chip"]')).toHaveCount(0);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);
    // The empty state is the *only* thing this user reads, so it must not claim
    // more than the page can see. "Nothing missing" would tell this deck it is
    // complete; it has an unfilled sideboard slot the arithmetic never looked
    // for. The text has to be about copies, and it has to say so.
    const empty = page.locator('[data-testid="needs-empty"]');
    await expect(empty).toContainText("holds every copy it wants");
    await expect(empty).toContainText("Unfilled board slots aren't counted here");
    expect(await needsOf(request, deck.id)).toEqual([]);
  } finally {
    await deleteCollection(request, deck.id);
  }
});
