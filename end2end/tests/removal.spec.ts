// Undoable removal + deck teardown (specs/app-ui.md → `/my/collections/:id`;
// specs/collection-api.md → Move `to = None`, Undo, Teardown).
//
// The thing this file exists to prove: **a card can be removed from a binder,
// and the undo gives back the copies that were actually there.** For two tasks
// the stepper's floor was `min = 1`, because a committed 0 ran
// `DELETE FROM holdings` while the undo the stepper offered re-POSTed the dead
// id — a success toast over vanished copies. The floor made that unreachable and
// made a binder card impossible to remove at all.
//
// The high-risk half is not "does it come back" but "does it come back **as what
// it was**". A rendered row is `(printing, board)` with finish, condition and
// language summed away, so an undo that restored the *default* grain would look
// identical on screen and be a silent data change. Two tests pin that:
//
//   - a foil / lightly-played / Japanese stack must return foil, LP and Japanese;
//   - a sideboard stack must return to the sideboard, leaving the mainboard
//     stack of the same printing untouched throughout.
//
// Every assertion about the database is read back through `/api/cards/{id}/holdings`
// — the one read that does *not* group the grain away. A toast is evidence that a
// message was raised, not that rows moved. (`collection_view`'s `present` would
// have passed all of these at the wrong grain.)
//
// Isolation: scratch `zz-e2e-…` collections created via the API and deleted in a
// `finally` (delete cascades holdings, desires and nothing else's). Printings
// come from catalog cards the fixture owns nowhere, so a count read back is this
// test's writes and nothing else.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const TOAST = '[data-name="Toast"]';
const STEPPER_VALUE = '[data-testid="count-stepper-value"]';
const STEPPER_INPUT = '[data-testid="count-stepper-input"]';
const HERE = '[data-testid="here-count"]';

type Summary = { id: string; name: string };
type Card = { oracle_id: string; printing_id: string | null; name: string };
type Holding = {
  collection_id: string;
  printing_id: string;
  finish: string;
  condition: string;
  language: string;
  board: string;
  quantity: number;
};
type Row = { printing_id: string; present: number; board: string };

let scratchSeq = 0;
const scratchName = (what: string) =>
  `zz-e2e-rm-${what}-w${test.info().workerIndex}-${++scratchSeq}-` +
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

/// Put copies into a collection at an explicit grain. `grain` reaches the
/// fields `AddHave` defaults — finish, condition, language, board — which is the
/// only reason this file can tell a grain-aware write from one that assumes the
/// default.
async function addHave(
  request: APIRequestContext,
  id: string,
  printingId: string,
  quantity = 1,
  grain: {
    finish?: string;
    condition?: string;
    language?: string;
    board?: string;
  } = {},
) {
  const res = await request.post(`/api/collections/${id}/have`, {
    data: { printing_id: printingId, quantity, ...grain },
  });
  expect(res.status(), "add have").toBe(200);
}

/// Every copy of a card the signed-in user holds, **ungrouped** — the read the
/// rendered views deliberately do not give you.
async function holdingsOf(
  request: APIRequestContext,
  oracleId: string,
): Promise<Holding[]> {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status(), "holdings").toBe(200);
  return (await res.json()) as Holding[];
}

/// A comparable, order-independent description of a card's copies: the grain
/// string plus the collection it sits in. This is what an undo has to reproduce.
async function grainsIn(
  request: APIRequestContext,
  oracleId: string,
  where: Record<string, string>,
): Promise<string[]> {
  const rows = await holdingsOf(request, oracleId);
  return rows
    .map((h) => {
      const place = where[h.collection_id] ?? h.collection_id.slice(0, 8);
      return `${place}: ${h.finish}/${h.condition}/${h.language}/${h.board} x${h.quantity}`;
    })
    .sort();
}

async function viewRows(
  request: APIRequestContext,
  id: string,
): Promise<Row[]> {
  const res = await request.get(`/api/collections/${id}/view?limit=200`);
  expect(res.status(), `view ${id}`).toBe(200);
  return ((await res.json()) as { cards: Row[] }).cards;
}

/// Catalog cards the signed-in user owns nowhere, with a real printing — so a
/// holdings read for one of them is exactly what this test wrote.
async function unownedCards(
  request: APIRequestContext,
  n: number,
  skip = 0,
): Promise<Card[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const { cards: owned } = (await mine.json()) as {
    cards: { card: Card }[];
  };
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

const rowFor = (page: Page, printingId: string, board = "main") =>
  page.locator(
    `[data-testid="collection-row"][data-printing="${printingId}"][data-board="${board}"]`,
  );

/// Drive the stepper to zero the way a user does: click the count to type in
/// it, replace the number, commit with ⏎. (Stepping down with `−` is the same
/// commit; typing is one action instead of N and does not depend on the count.)
async function commitZero(row: ReturnType<typeof rowFor>) {
  await row.locator(STEPPER_VALUE).click();
  await row.locator(STEPPER_INPUT).fill("0");
  await row.locator(STEPPER_INPUT).press("Enter");
}

// ------------------------------------------- a binder card can be removed ---

test("@fast a card can be removed from a binder, and Undo brings the copies back", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1);
  const binder = await createCollection(request, "binder", "bind");
  try {
    await addHave(request, binder.id, card.printing_id as string, 3);
    const where = { [binder.id]: "binder" };

    await page.goto(`/my/collections/${binder.id}`);
    await hydrated(page);
    const row = rowFor(page, card.printing_id as string);
    await expect(row.locator(STEPPER_VALUE)).toHaveText("3");

    await commitZero(row);

    // The row stops offering a stepper: the holding it wrote to is gone, so a
    // control that kept accepting numbers would be addressing a dead id.
    await expect(row.locator(HERE)).toHaveText("—");
    // The header follows the same delta, or the two disagree on screen — and it
    // is the *removal* that has to move it, since the view is not refetched.
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "0 here",
    );
    const toast = page.locator(TOAST, { hasText: "Removed" });
    await expect(toast).toContainText(`Removed ${card.name} (3 copies)`);

    // Read back, not off the toast: the copies are actually gone.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([]);
    }).toPass({ timeout: 5000 });
    expect(await viewRows(request, binder.id)).toHaveLength(0);

    await toast.getByRole("button", { name: "Undo" }).click();

    // …and they come back, in the binder, as the three copies they were.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "binder: nonfoil/nm/en/main x3",
      ]);
    }).toPass({ timeout: 5000 });
    // The page followed the database rather than waiting for a reload — the
    // undo refetches the view, so the row is back with a live holding id.
    await expect(row.locator(STEPPER_VALUE)).toHaveText("3");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "3 here",
    );
  } finally {
    await deleteCollection(request, binder.id);
  }
});

// -------------------------------------------- the grain the row cannot show ---

test("@fast an undone removal returns the exact grain, not the default one", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 3);
  const binder = await createCollection(request, "binder", "grain");
  try {
    // Foil, lightly played, Japanese — three axes the row renders none of. The
    // view sums across all of them, so `present = 2` is all the page knows.
    await addHave(request, binder.id, card.printing_id as string, 2, {
      finish: "foil",
      condition: "lp",
      language: "ja",
    });
    const where = { [binder.id]: "binder" };
    expect(
      await grainsIn(request, card.oracle_id, where),
      "the fixture must actually hold a non-default grain",
    ).toEqual(["binder: foil/lp/ja/main x2"]);

    await page.goto(`/my/collections/${binder.id}`);
    await hydrated(page);
    const row = rowFor(page, card.printing_id as string);
    await expect(row.locator(STEPPER_VALUE)).toHaveText("2");

    await commitZero(row);
    const toast = page.locator(TOAST, { hasText: "Removed" });
    await expect(toast).toContainText(`Removed ${card.name} (2 copies)`);
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([]);
    }).toPass({ timeout: 5000 });

    await toast.getByRole("button", { name: "Undo" }).click();

    // The assertion that matters: foil/lp/ja, not nonfoil/nm/en. An undo that
    // restored the default grain would render identically and be a silent data
    // change.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "binder: foil/lp/ja/main x2",
      ]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, binder.id);
  }
});

// ------------------------------------------------- the board, same problem ---

test("@fast removing a sideboard stack leaves the mainboard alone and undoes to the sideboard", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 5);
  const deck = await createCollection(request, "deck", "board");
  try {
    // Same printing on two boards: two rows on screen, two stacks in the
    // database. A board-blind write would take from the wrong one.
    await addHave(request, deck.id, card.printing_id as string, 2, {
      board: "side",
    });
    await addHave(request, deck.id, card.printing_id as string, 1);
    const where = { [deck.id]: "deck" };

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    const side = rowFor(page, card.printing_id as string, "side");
    await expect(side.locator(STEPPER_VALUE)).toHaveText("2");

    await commitZero(side);
    await expect(side.locator(HERE)).toHaveText("—");
    const toast = page.locator(TOAST, { hasText: "Removed" });
    await expect(toast).toBeVisible();

    // The mainboard copy is untouched — this is the assertion a
    // `board = 'main'`-pinned write fails.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "deck: nonfoil/nm/en/main x1",
      ]);
    }).toPass({ timeout: 5000 });

    await toast.getByRole("button", { name: "Undo" }).click();

    // Back on the *sideboard*, not merged into the mainboard stack.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "deck: nonfoil/nm/en/main x1",
        "deck: nonfoil/nm/en/side x2",
      ]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, deck.id);
  }
});

// --------------------------------------------------------------- teardown ---

test("@fast Empty deck… sends every board to the chosen destination", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 7);
  const deck = await createCollection(request, "deck", "tddeck");
  const dest = await createCollection(request, "binder", "tddest");
  try {
    await addHave(request, deck.id, card.printing_id as string, 1);
    await addHave(request, deck.id, card.printing_id as string, 2, {
      board: "side",
    });
    const where = { [deck.id]: "deck", [dest.id]: "dest" };

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    await page.locator('[data-testid="teardown-open"]').click();
    await page
      .locator('[data-testid="teardown-destination"]')
      .selectOption({ label: dest.name });
    await page.locator('[data-testid="teardown-confirm"]').click();

    await expect(page.locator(TOAST, { hasText: "Emptied" })).toContainText(
      "2 cards moved",
    );
    // All three copies left, both boards, and landed as ordinary copies in the
    // binder — a board is a deck's label, not a property that travels.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "dest: nonfoil/nm/en/main x3",
      ]);
    }).toPass({ timeout: 5000 });
    expect(await viewRows(request, deck.id)).toHaveLength(0);
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, dest.id);
  }
});

test("@fast Return to previous locations sends each card back where it came from", async ({
  page,
  request,
}) => {
  const [fromBinder, noHistory] = await unownedCards(request, 2, 8);
  const binder = await createCollection(request, "binder", "prevsrc");
  const deck = await createCollection(request, "deck", "prevdeck");
  try {
    // One card with a history — it was moved into the deck from the binder —
    // and one with none, which the fallback must route to the Inbox.
    await addHave(request, binder.id, fromBinder.printing_id as string, 2);
    const staged = (await viewRows(request, binder.id)) as (Row & {
      holding_id: string;
    })[];
    const holdingId = staged.find(
      (r) => r.printing_id === fromBinder.printing_id,
    )!.holding_id;
    const moved = await request.post(`/api/holdings/${holdingId}/move`, {
      data: { to_collection_id: deck.id },
    });
    expect(moved.status(), "stage the move into the deck").toBe(200);
    await addHave(request, deck.id, noHistory.printing_id as string, 1);

    const collections = await request.get("/api/collections");
    const inbox = ((await collections.json()) as (Summary & {
      is_inbox: boolean;
    })[]).find((c) => c.is_inbox)!;
    const where = {
      [binder.id]: "binder",
      [deck.id]: "deck",
      [inbox.id]: "inbox",
    };

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    await page.locator('[data-testid="teardown-open"]').click();
    // "" is the previous-locations option — the default, so this asserts the
    // dialog opens on it rather than selecting it.
    await expect(page.locator('[data-testid="teardown-destination"]')).toHaveValue(
      "",
    );
    await page.locator('[data-testid="teardown-confirm"]').click();

    await expect(page.locator(TOAST, { hasText: "Emptied" })).toBeVisible();
    await expect(async () => {
      expect(await grainsIn(request, fromBinder.oracle_id, where)).toEqual([
        "binder: nonfoil/nm/en/main x2",
      ]);
      expect(await grainsIn(request, noHistory.oracle_id, where)).toEqual([
        "inbox: nonfoil/nm/en/main x1",
      ]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, binder.id);
    // The Inbox is undeletable; take back only what this test put in it.
    const left = await holdingsOf(request, noHistory.oracle_id);
    for (const h of left) {
      const rows = (await viewRows(request, h.collection_id)) as (Row & {
        holding_id: string;
      })[];
      const id = rows.find((r) => r.printing_id === h.printing_id)?.holding_id;
      if (id) {
        await request.post(`/api/holdings/${id}/move`, {
          data: { to_collection_id: null },
        });
      }
    }
  }
});
