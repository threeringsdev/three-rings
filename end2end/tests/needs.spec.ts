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
// - **the board case behaves as decided.** `needs()` is board-**aware** since
//   P6-074 (see below and the `my::needs` module doc): a want on the sideboard
//   of a card held on the mainboard is a real need, and pulling it lands the
//   copies on the sideboard. This bullet used to pin the opposite decision;
//   it was re-taken deliberately, not drifted into.
// - **a partial pull stays honest (P6-119).** A pick-list line that finds
//   fewer copies at its source than it asked for — the pick list is a
//   snapshot, so the source can drain between generating it and ticking the
//   line — is not struck through as fully pulled. It stays in the walk
//   carrying the residual, and the toast names the shortfall.
//
// **On the board case.** A deck holding a card on `main` and wanting it on
// `side` renders two rows on the collection page — a mainboard row with copies
// and a sideboard row wanting one — and `/needs` now agrees with them: one need
// row, `board: "side"`, `present_here: 0`, plus a chip on the header. The deck
// genuinely wants a second copy; the one it holds is committed to the
// mainboard. Pulling it lands the copies on `side`, which is what makes the
// need closable at all (before P6-074 every pull hardcoded `to_board = main`).
// What is still *not* a need: relabelling a copy the deck already holds from
// one board to another — card-tagging's quantity-preserving op, which neither
// bucket here can offer.
//
// **And one copy elsewhere still only covers one board.** The elsewhere pool is
// per *card* (a binder copy fills any board's need), so both board rows see the
// same offer — but it is apportioned between them, mainboard first, not handed
// whole to each. The second test below pins that across all four surfaces that
// state it: the wire rows, the chip, the pick list, and `/my/shopping`.
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
  id: string;
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

/// Set one holding's absolute quantity — the raw REST route the count stepper
/// uses (`app/src/backend/routes.rs`), reached directly here to drain a source
/// *out of band* of the page under test: the pick list is a snapshot (module
/// doc in `app/src/my/needs.rs`), so a write through this route, not through
/// `page`, is what models "another tab" or "the stepper" changing the source
/// between generating the list and ticking one of its lines.
async function setHoldingQuantity(
  request: APIRequestContext,
  holdingId: string,
  quantity: number,
) {
  const res = await request.post(`/api/holdings/${holdingId}/quantity`, {
    data: { quantity },
  });
  expect(res.status(), "set holding quantity").toBe(200);
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
  board: string;
  desired: number;
  present_here: number;
  /// Per-row, and **apportioned**: a card's elsewhere copies are shared between
  /// its board rows, so these do not each get the whole pool (P6-074 review).
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
///
/// **Was `q=z&limit=60`, switched to `q=n&limit=200` (P6-119), file-scoped —
/// the same remedy P6-118 applied to `removal.spec.ts`.** (The old "`q=z`
/// rather than a vowel, since the seed's own name-ordered picks own the
/// alphabetically-first slice" rationale was specific to `z` and does not
/// carry over to explain `n` — dropped rather than left stale.) Measured
/// live: `q=z&limit=60` is now fully exhausted (0 free — a limit bump alone
/// cannot grow a query's own universe, which P6-118 already found tops out
/// at 132 total for `q=z`), while `q=n&limit=200` measured 112 free. This
/// file's own new partial-pull test (below) is what finally tripped it. This
/// now draws from the **same** `q=n&limit=200` pool `removal.spec.ts` already
/// uses (P6-118), so the two files are no longer isolated from each other's
/// draw-down either — the systemic fix (one query with real headroom shared
/// by every file, instead of each file drifting to its own patched term,
/// still including `batch-move.spec.ts` and `command-palette.spec.ts`, both
/// still on the exhausted `q=z`) is filed as follow-up, not done here.
async function unownedCards(
  request: APIRequestContext,
  n: number,
  skip = 0,
): Promise<Card[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const { cards: owned } = (await mine.json()) as { cards: { card: Card }[] };
  const taken = new Set(owned.map((r) => r.card.oracle_id));
  const res = await request.get("/api/catalog/search?q=n&limit=200");
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

// ------------------------------------------------- P6-119: partial pulls ---

test("@fast a line that finds less than it asked for stays in the walk with its residual", async ({
  page,
  request,
}) => {
  // The pick list is generated once and walked over several separate
  // requests (one `pull_needs` per tick), so a line's *displayed* ask can go
  // stale between generating it and ticking it — reachable in one session,
  // no second browser needed, because anything that touches the source in
  // between does it (another tab, the collection view's own count stepper).
  // Modeled here with a raw write through the holdings-quantity route rather
  // than a second page, for determinism: the point under test is what the
  // pick-list line does with the mismatch, not how the source came to drain.
  const [card] = await unownedCards(request, 1);
  const source = await createCollection(request, "binder", "partial-src");
  const deck = await createCollection(request, "deck", "partial-dst");
  try {
    await addHave(request, source.id, card.printing_id as string, 4);
    await addWant(request, deck.id, card.oracle_id, 4);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);
    await page.locator('[data-testid="pull-all"]').click();

    const group = page.locator(PICK_LIST).locator(PICK_GROUP, {
      hasText: source.name,
    });
    const row = group.locator(PICK_ROW);
    // The snapshot: the whole gap is fillable from this one source, so the
    // line asks for all 4.
    await expect(group.locator('[data-testid="pick-label"]')).toHaveText(
      `4 × ${card.name}`,
    );

    // Drain the source to 2, out of band — the mounted pick list does not
    // see this; it is a snapshot (module doc), not a re-derivation.
    const before = await holdingsOf(request, card.oracle_id);
    const sourceHolding = before.find((h) => h.collection_id === source.id);
    expect(sourceHolding, "the source must hold the card to drain it").toBeTruthy();
    await setHoldingQuantity(
      request,
      (sourceHolding as Holding).id,
      2,
    );

    await row.locator('[data-name="Checkbox"]').click();

    // The honest outcome, read off the toast: 2 of the 4 asked moved, not 4.
    // Cause-neutral wording ("still owed", not "not found at the source") —
    // this client cannot know *why* the residual remains, only how much.
    await expect(page.locator(TOAST)).toContainText(
      `Pulled 2 of 4 ${card.name} — 2 still owed`,
    );
    // Not struck through — this line is not done, it is owed 2 more.
    await expect(row).toHaveAttribute("data-state", "todo");
    await expect(row.locator('[data-testid="pick-label"]')).toHaveText(
      `2 × ${card.name}`,
    );
    // The checkbox itself must not read as checked either — a struck-through
    // label beside an unchecked box, or vice versa, would be its own lie.
    await expect(row.locator('[data-name="Checkbox"]')).toHaveAttribute(
      "data-state",
      "unchecked",
    );

    // Read back through the API: exactly 2 moved, none left behind unmoved
    // and unaccounted for at the drained source.
    await expect(async () => {
      expect(await copiesIn(request, card.oracle_id, source.id)).toBe(0);
      expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(2);
    }).toPass({ timeout: 5000 });

    // The line is still live: ticking it again asks the server fresh, and
    // the now-empty source has nothing left to give — refused by name, not
    // silently re-struck. Scoped to the refusal's own wording, not just
    // `card.name`: the *first* toast's "... still owed" message is still
    // visible for its own 5s window and also contains the card's name, so an
    // unscoped match could pass off a stale read of tick one rather than
    // proving tick two.
    await row.locator('[data-name="Checkbox"]').click();
    await expect(
      page.locator(TOAST, { hasText: "no longer missing here" }),
    ).toBeVisible();
    await expect(row).toHaveAttribute("data-state", "todo");
    expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(2);
  } finally {
    await deleteCollection(request, source.id);
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

test("@fast a want on the sideboard of a card held on the mainboard is a need, and pulling it lands on the sideboard", async ({
  page,
  request,
}) => {
  // **This test pins a decision, not just a behavior** (see the file header) —
  // and P6-074 re-took it. `needs()` groups this deck's desires and holdings by
  // `(oracle, board)`, so one copy on `main` plus one wanted on `side` is a
  // genuine gap of one on the sideboard: the copy the deck holds is committed
  // to the mainboard. The old shape (needs `[]`, no chip, sideboard row still
  // reading WANTED 1) was the contradiction this task closed.
  //
  // The second half is what made the first half shippable: a pull for a
  // sideboard need has to *land* on the sideboard, or the need survives every
  // pull aimed at it forever. Read back through `/api/cards/{id}/holdings`,
  // the one read that does not group the board away.
  const [card] = await unownedCards(request, 1);
  const deck = await createCollection(request, "deck", "board");
  const binder = await createCollection(request, "binder", "board-src");
  try {
    await addHave(request, deck.id, card.printing_id as string, 1, {
      board: "main",
    });
    await addWant(request, deck.id, card.oracle_id, 1, "side");
    // The copy the pull will draw from — in another collection, so it shows up
    // as owned-elsewhere rather than as something the deck already holds.
    await addHave(request, binder.id, card.printing_id as string, 1);

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    const sideRow = page.locator(
      `[data-testid="collection-row"][data-printing="${card.printing_id}"][data-board="side"]`,
    );
    await expect(sideRow.locator('[data-testid="wanted-count"]')).toHaveText(
      "1",
    );
    // The chip is the half that used to be absent. Header and needs page are
    // fed by two different queries at the same grain, which is why they are
    // asserted together — a half-fix in either one is the failure mode.
    await expect(page.locator('[data-testid="needs-chip"]')).toContainText(
      "1 missing",
    );

    // The wire says which board, not just that something is missing.
    const rows = await needsOf(request, deck.id);
    expect(rows).toHaveLength(1);
    expect(rows[0].board).toBe("side");
    expect(rows[0].oracle_id).toBe(card.oracle_id);
    expect(rows[0].desired).toBe(1);
    expect(rows[0].present_here).toBe(0);
    expect(rows[0].owned_elsewhere).toBe(1);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);
    const needRow = page.locator(
      `${NEED_ROW}[data-oracle="${card.oracle_id}"][data-board="side"]`,
    );
    await expect(needRow).toHaveCount(1);
    // The row says which board out loud — the mainboard says nothing, the same
    // convention the deck page's section headers use.
    await expect(needRow.locator('[data-testid="need-board"]')).toHaveText(
      "Sideboard",
    );

    await needRow.locator('[data-testid="pull-row"]').click();
    await expect(page.locator(TOAST).first()).toContainText("Pulled 1 copy");

    // Where the copy actually went: the deck's sideboard, not its mainboard.
    const inDeck = (await holdingsOf(request, card.oracle_id)).filter(
      (h) => h.collection_id === deck.id,
    );
    const onSide = inDeck.filter((h) => h.board === "side");
    expect(onSide.reduce((n, h) => n + h.quantity, 0)).toBe(1);
    expect(
      inDeck
        .filter((h) => h.board === "main")
        .reduce((n, h) => n + h.quantity, 0),
      "the mainboard copy is untouched — the pull added to the sideboard",
    ).toBe(1);
    // And the need is closed rather than re-offered forever, which is the whole
    // point of landing on the board that wanted it.
    expect(await needsOf(request, deck.id)).toEqual([]);
  } finally {
    await deleteCollection(request, binder.id);
    await deleteCollection(request, deck.id);
  }
});

test("@fast one copy elsewhere cannot cover two boards at once", async ({
  page,
  request,
}) => {
  // The P6-074 review's major, end to end. The elsewhere pool is per *card* —
  // a binder copy can fill any board's need — so a deck wanting one copy on
  // `main` and one on `side` sees the same single binder copy offered to both
  // rows. Applying that pool whole to each row let one physical copy "satisfy"
  // two gaps: both rows read `owned_elsewhere: 1 / short: 0`, the chip said
  // "2 missing — 2 owned elsewhere" with no to-buy clause and no Short bucket,
  // and the pick list offered two pullable lines against one copy — while
  // `/my/shopping`, which is per-oracle and was always right, said one to buy.
  // Two surfaces contradicting each other about one card.
  //
  // The pool is now apportioned in the read's own order (mainboard first), and
  // this test asserts every surface tells the same story.
  const [card] = await unownedCards(request, 1);
  const deck = await createCollection(request, "deck", "share");
  const binder = await createCollection(request, "binder", "share-src");
  try {
    await addWant(request, deck.id, card.oracle_id, 1, "main");
    await addWant(request, deck.id, card.oracle_id, 1, "side");
    await addHave(request, binder.id, card.printing_id as string, 1);

    // The wire: two rows, and the one copy is promised to exactly one of them.
    const rows = await needsOf(request, deck.id);
    expect(rows).toHaveLength(2);
    const main = rows.find((r) => r.board === "main");
    const side = rows.find((r) => r.board === "side");
    expect(main, "a mainboard row").toBeTruthy();
    expect(side, "a sideboard row").toBeTruthy();
    expect(main?.owned_elsewhere).toBe(1);
    expect(main?.short).toBe(0);
    expect(side?.owned_elsewhere, "the copy is already spoken for").toBe(0);
    expect(side?.short, "so the sideboard copy has to be bought").toBe(1);
    // The invariant, restated on live data: the two rows together cannot claim
    // more than the one copy that exists.
    expect(
      rows.reduce((n, r) => n + r.owned_elsewhere, 0),
    ).toBeLessThanOrEqual(1);

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    // The chip's to-buy clause is the half that vanished under the bug.
    await expect(page.locator('[data-testid="needs-chip"]')).toContainText(
      "2 missing — 1 owned elsewhere · 1 to buy",
    );

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);
    // One pullable row, one Short row — not two of either.
    await expect(page.locator(NEED_ROW)).toHaveCount(1);
    await expect(
      page.locator(`${NEED_ROW}[data-board="main"]`),
    ).toHaveCount(1);
    await expect(
      page.locator(`${SHORT_ROW}[data-board="side"]`),
    ).toHaveCount(1);

    // And the pick list offers one line, not two — a walk that told you to
    // fetch the same copy twice is the user-facing form of the same bug.
    await page.locator('[data-testid="pull-all"]').click();
    await expect(page.locator(PICK_LIST)).toBeVisible();
    await expect(page.locator(PICK_ROW)).toHaveCount(1);
    await expect(page.locator('[data-testid="pick-label"]')).toHaveText(
      `1 × ${card.name}`,
    );

    // `/my/shopping` is the surface that was right all along; it must now agree
    // rather than contradict. Containment only — the list is global.
    await page.goto("/my/shopping");
    await hydrated(page);
    await expect(
      page
        .locator(`[data-testid="shopping-row"][data-oracle="${card.oracle_id}"]`)
        .locator('[data-testid="shortfall"]'),
    ).toHaveText("1");
  } finally {
    await deleteCollection(request, binder.id);
    await deleteCollection(request, deck.id);
  }
});

// ---------------------------------------------- P6-140: per-row pending only ---

test("@fast one row's in-flight Pull does not disable another row's button", async ({
  page,
  request,
}) => {
  // The bug: `ElsewhereRow` used to receive its `pending` signal from its
  // parent `OwnedElsewhere`, one signal shared by every row in the table —
  // so clicking any row's Pull disabled every row's button, not just the one
  // that was actually in flight. Two independent cards from two independent
  // sources, so a correct row-B pull is unambiguous and does not lean on the
  // server's shared-stock reconciliation to make sense of the outcome.
  //
  // Holding `pull_needs` open is what lets this test stand *inside* the
  // in-flight window rather than racing it: A's request is genuinely
  // unresolved when B's button state is asserted.
  const [cardA, cardB] = await unownedCards(request, 2);
  const sourceA = await createCollection(request, "binder", "pendA-src");
  const sourceB = await createCollection(request, "binder", "pendB-src");
  const deck = await createCollection(request, "deck", "pend-dst");
  try {
    await addHave(request, sourceA.id, cardA.printing_id as string, 1);
    await addHave(request, sourceB.id, cardB.printing_id as string, 1);
    await addWant(request, deck.id, cardA.oracle_id, 1);
    await addWant(request, deck.id, cardB.oracle_id, 1);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);

    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    await page.route("**/api/pull_needs*", async (route) => {
      await gate;
      // The handler stays installed after the gate opens — see
      // `catalog.spec.ts`'s `holdSearches` for why the `.catch` is needed.
      await route.continue().catch(() => {});
    });

    const pullA = needRow(page, cardA.oracle_id).locator(
      '[data-testid="pull-row"]',
    );
    const pullB = needRow(page, cardB.oracle_id).locator(
      '[data-testid="pull-row"]',
    );

    await pullA.click();
    // A's own request is in flight, and its own button says so...
    await expect(pullA).toBeDisabled();
    // ...but B's does not — the base-bug assertion. A shared `pending` would
    // leave this `disabled` too.
    await expect(pullB).toBeEnabled();

    // The proof that matters is that B is actually clickable, not merely
    // missing the `disabled` attribute by coincidence: Playwright refuses to
    // click a genuinely disabled element, so this line alone fails against
    // the shared-signal bug.
    await pullB.click();
    await expect(pullB).toBeDisabled();

    // Let both held requests land, and both pulls actually complete — a
    // second in-flight Pull that never got enabled would be no better than
    // one that silently no-opped.
    release();
    await expect(async () => {
      expect(await copiesIn(request, cardA.oracle_id, deck.id)).toBe(1);
      expect(await copiesIn(request, cardB.oracle_id, deck.id)).toBe(1);
    }).toPass({ timeout: 5000 });
    expect(await needsOf(request, deck.id)).toEqual([]);
  } finally {
    await deleteCollection(request, sourceA.id);
    await deleteCollection(request, sourceB.id);
    await deleteCollection(request, deck.id);
  }
});

// ------------------------------------------------- P6-141: pull honesty ---

test("@fast a row-level Pull that closes a need drops the stale line from an open pick list", async ({
  page,
  request,
}) => {
  // The base bug: the pick list is a snapshot generated when "Pull all…" is
  // clicked (module doc in `app/src/my/needs.rs`), and it deliberately does
  // not refetch as the table above it does — so a row-level Pull, a *second*
  // control on the same page, can close the very need a checklist line still
  // names. Before the reconcile the line stayed on the walk looking exactly
  // as pullable as a live one; ticking it could only ever land the
  // `NoLongerNeeded` refusal. The fix drops it the moment the row-level Pull
  // proves the need is gone, not on the next tick.
  const [card] = await unownedCards(request, 1);
  const source = await createCollection(request, "binder", "reconcile-src");
  const deck = await createCollection(request, "deck", "reconcile-dst");
  try {
    await addHave(request, source.id, card.printing_id as string, 2);
    await addWant(request, deck.id, card.oracle_id, 2);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);

    // Open the pick list first — its snapshot now names this exact line.
    await page.locator('[data-testid="pull-all"]').click();
    const list = page.locator(PICK_LIST);
    await expect(list.locator(PICK_ROW)).toHaveCount(1);
    await expect(list.locator('[data-testid="pick-label"]')).toHaveText(
      `2 × ${card.name}`,
    );

    // Close the need through the *other* control — the row-level Pull in the
    // "Owned elsewhere" table — while the checklist stays open and untouched.
    await needRow(page, card.oracle_id)
      .locator('[data-testid="pull-row"]')
      .click();
    await expect(page.locator(TOAST)).toContainText("Pulled 2 copies");

    // The need is gone from the table it was pulled from...
    await expect(page.locator('[data-testid="needs-empty"]')).toBeVisible();
    // ...and the checklist's own line for the same need goes with it, rather
    // than lingering as a dead offer only a tick (and an error toast) away
    // from proving itself stale.
    await expect(list.locator(PICK_ROW)).toHaveCount(0);
    await expect(list.locator(PICK_GROUP)).toHaveCount(0);

    // Read back through the API: the deck really did receive the copies —
    // this is a real write, not just a UI state that happens to look closed.
    // (Not asserting the source is drained: `unownedCards`'s own doc records
    // that its "owned nowhere" guarantee is a first-200-rows check, not a
    // complete one, so another collection with more of this card can win the
    // allocation's quantity-desc order; which collection supplied the copies
    // is not what this test is about.)
    expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(2);
    expect(await needsOf(request, deck.id)).toEqual([]);

    // Undo must not be a one-way door on the checklist: the pull reverses,
    // which reopens this exact need, and the checklist that dropped the line
    // has to say so again — not silently disagree with the table row that
    // `report`'s own Undo already makes reappear.
    await page.locator(TOAST).getByRole("button", { name: "Undo" }).click();
    await expect(page.locator(TOAST)).toContainText("Put them back");

    // The table row is back...
    await expect(needRow(page, card.oracle_id)).toBeVisible();
    await expect(page.locator('[data-testid="needs-empty"]')).toHaveCount(0);
    // ...and so is the checklist's own line for it, in the same open session.
    await expect(list.locator(PICK_ROW)).toHaveCount(1);
    await expect(list.locator('[data-testid="pick-label"]')).toHaveText(
      `2 × ${card.name}`,
    );

    // Read back through the API: the copies really did move back.
    await expect(async () => {
      expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(0);
    }).toPass({ timeout: 5000 });
    expect(await needsOf(request, deck.id)).toHaveLength(1);
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, deck.id);
  }
});

test("@fast a pull whose items resolve to nothing is refused out loud, not silently ignored", async ({
  page,
  request,
}) => {
  // Both current callers always send at least one item — the row button
  // sends every source a need's elsewhere allocation names, the pick-list
  // tick sends its own single line — so `items: []` cannot be produced by
  // clicking anything today. The outgoing request is rewritten to prove the
  // client states this shape honestly regardless of whether the UI can reach
  // it yet, rather than leaving `report()`'s two branches to cover every case
  // by accident (`PullOutcome::is_empty`'s own doc, `app/src/my/needs.rs`;
  // pinned server-side by `plan_pull_needs`'s own empty-items test in
  // `app/src/backend/pull_plan.rs`).
  const [card] = await unownedCards(request, 1);
  const source = await createCollection(request, "binder", "empty-src");
  const deck = await createCollection(request, "deck", "empty-dst");
  try {
    await addHave(request, source.id, card.printing_id as string, 2);
    await addWant(request, deck.id, card.oracle_id, 2);

    await page.goto(`/my/collections/${deck.id}/needs`);
    await hydrated(page);

    await page.route("**/api/pull_needs*", async (route) => {
      const body = JSON.parse(route.request().postData() ?? "{}");
      await route.continue({
        postData: JSON.stringify({ ...body, items: [] }),
      });
    });

    await needRow(page, card.oracle_id)
      .locator('[data-testid="pull-row"]')
      .click();

    await expect(page.locator(TOAST)).toContainText("Nothing to pull");

    // Nothing actually moved — the rewritten request truthfully reported
    // nothing, and the toast must not claim otherwise.
    expect(await copiesIn(request, card.oracle_id, deck.id)).toBe(0);
    expect(await copiesIn(request, card.oracle_id, source.id)).toBe(2);
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, deck.id);
  }
});
