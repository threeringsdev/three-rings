// Batch move — the selection tray's "Move to…" (specs/app-ui.md → Selection
// tray; specs/collection-api.md → "Move (batch)").
//
// The contract, in assertion order:
//
// - the picker is the *catalog's* destination picker, ranked for the selection:
//   the same `destination-option` rows and search box, with the collections that
//   want the selected cards first, hinting their summed shortfall;
// - a batch of two rows moves both, in one write, and the page it was moved out
//   of follows the database rather than waiting for a reload;
// - the single Undo reverts the **whole** batch, not the last card of it;
// - a `/my` row whose copies sit in exactly one place resolves to that place and
//   moves;
// - a `/my` row whose copies sit in several places is **refused by name** and
//   stays checked — the one thing this task must never do is guess a source
//   (`MoveItem { from_collection_id: None }` means copies from *outside* the
//   system, so a guess would conjure them).
//
// Every database assertion is read back through the API, never off the toast: a
// toast is evidence that a message was raised, not that rows moved.
//
// **Isolation.** Everything that writes does so inside `zz-e2e-…` collections
// created via the API and deleted in a `finally` (the `collection-view.spec.ts`
// convention; delete cascades holdings and desires). The one test that touches
// the seeded fixture is the refusal test — and it is a refusal, so it writes
// nothing by construction. Source printings come from catalog cards the fixture
// owns *nowhere*, which is also what makes the `/my` resolution assertions mean
// what they say.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const TRAY = '[data-testid="selection-tray"]';
const COUNT = '[data-testid="tray-count"]';
const MOVE = '[data-testid="tray-move"]';
const PICKER = "#popover-tray-destination";
const OPTION = '[data-testid="destination-option"]';
const HINT = '[data-testid="destination-hint"]';
const TOAST = '[data-name="Toast"]';

type Summary = { id: string; name: string };
type Row = {
  oracle_id: string;
  printing_id: string;
  name: string;
  present: number;
};
type View = { cards: Row[] };
type Card = {
  oracle_id: string;
  printing_id: string | null;
  name: string;
  owned: number | null;
};
type Location = { collection_id: string; collection_name: string };
type AllCardsRow = { card: Card; locations: Location[] };

let scratchSeq = 0;
/// Worker index, a per-file counter, **and** a random tail.
///
/// The first two are the suite's convention; the tail is because this file
/// picks its destination out of a list *by name*, and a test that dies before
/// its `finally` (a timeout under a loaded suite — seen) leaves a collection
/// behind that the next run at the same worker index would name identically.
/// Two same-named options make the picker locator ambiguous, which reads as a
/// UI bug and is not one.
const scratchName = (what: string) =>
  `zz-e2e-move-${what}-w${test.info().workerIndex}-${++scratchSeq}-` +
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

/// Put copies into a collection. `grain` reaches the fields `AddHave` defaults
/// — `finish`, `condition`, `language`, `board` — which is the whole point:
/// a fixture that only ever writes `nonfoil/nm/en/main` cannot distinguish a
/// move that checks the grain from one that assumes it, and every holding in
/// this suite was written that way until the foil tests below.
async function addHave(
  request: APIRequestContext,
  id: string,
  printingId: string,
  quantity = 1,
  grain: { finish?: string; board?: string } = {},
) {
  const res = await request.post(`/api/collections/${id}/have`, {
    data: { printing_id: printingId, quantity, ...grain },
  });
  expect(res.status(), "add have").toBe(200);
}

async function addWant(
  request: APIRequestContext,
  id: string,
  oracleId: string,
  quantity = 1,
) {
  const res = await request.post(`/api/collections/${id}/want`, {
    data: { oracle_id: oracleId, quantity },
  });
  expect(res.status(), "add want").toBe(200);
}

async function viewOf(
  request: APIRequestContext,
  id: string,
): Promise<View> {
  const res = await request.get(`/api/collections/${id}/view?limit=200`);
  expect(res.status(), `view ${id}`).toBe(200);
  return (await res.json()) as View;
}

/// Copies of each printing in a collection, straight from the read model — `0`
/// where the row is gone entirely, which is what moving the last copy does.
/// One view read per collection, not one per printing: this runs inside a
/// polling `toPass` and the suite is parallel.
async function present(
  request: APIRequestContext,
  id: string,
  printingIds: string[],
): Promise<number[]> {
  const view = await viewOf(request, id);
  return printingIds.map(
    (p) => view.cards.find((c) => c.printing_id === p)?.present ?? 0,
  );
}

/// Catalog cards the signed-in user owns **nowhere**, with a real printing.
///
/// The resolution assertions hinge on how many places a card sits in, so the
/// fixture is pinned rather than assumed: a card owned nowhere is in exactly one
/// place after one scratch add.
///
/// "Owned nowhere" is asked of `/my` rather than of the search rows, because
/// `/api/catalog/search` returns `owned: null` even for a signed-in caller
/// (`into_summary(None)` in the hosted impl) — filtering on it would silently
/// call every card unowned. `/my` *is* the set of cards the caller owns or
/// wants, so its complement is the answer, in one request instead of one
/// card-detail read per candidate (which timed out the test).
///
/// `skip` keeps concurrent tests off each other's cards: the suite is
/// `fullyParallel`, so two tests reading the same candidate list would otherwise
/// race — one adding a card to its scratch binder while the other was counting
/// on that same card being held nowhere.
async function unownedCards(
  request: APIRequestContext,
  n: number,
  skip = 0,
): Promise<Card[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const { cards: owned } = (await mine.json()) as { cards: AllCardsRow[] };
  const taken = new Set(owned.map((r) => r.card.oracle_id));

  // `z`, not a vowel: the seed itself picked its cards from name-ordered
  // searches, so the alphabetically-first slice of the catalog is exactly the
  // slice the dev user already owns — `q=a` yields zero free cards.
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

/// A `/my` row whose copies are spread over several collections — the shape the
/// oracle-grained selection key cannot resolve on its own. Seeded by
/// `build_depth` (one printing at three depths), but found rather than named so
/// the test tracks the fixture instead of a line number in it.
async function scatteredCard(
  request: APIRequestContext,
): Promise<AllCardsRow> {
  const res = await request.get("/api/all-cards?limit=200");
  expect(res.status(), "all cards").toBe(200);
  const { cards } = (await res.json()) as { cards: AllCardsRow[] };
  const row = cards.find((c) => c.locations.length > 1);
  expect(
    row,
    "dev seed must hold one card in more than one collection (build_depth)",
  ).toBeTruthy();
  return row as AllCardsRow;
}

const collectionRow = (page: Page, printingId: string) =>
  page.locator(`[data-testid="collection-row"][data-printing="${printingId}"]`);
const myRow = (page: Page, oracleId: string) =>
  page.locator(`[data-testid="all-cards-row"][data-oracle="${oracleId}"]`);
const select = (row: ReturnType<typeof collectionRow>) =>
  row.locator('[data-testid="row-select"]');

/// Open the tray's picker.
async function openPicker(page: Page) {
  await page.locator(MOVE).click();
  const picker = page.locator(PICKER);
  // Content, not visibility: a closed popover is in the DOM, so `toBeHidden`
  // would pass whether or not the list ever mounted (e2e-suite skill).
  await expect(picker.locator(OPTION).first()).toBeVisible();
  return picker;
}

/// Pick `name` out of an open picker, driving the shared search box on the way
/// — the picker is the catalog's `DestinationList`, filtering included.
async function pick(picker: ReturnType<Page["locator"]>, name: string) {
  await picker.locator('[data-name="CommandInput"]').fill(name);
  const option = picker.locator(OPTION, { hasText: name });
  await expect(option).toHaveCount(1);
  await option.click();
}

/// Open and pick in one go, for the tests that are not asserting the ranking.
async function moveTo(page: Page, name: string) {
  await pick(await openPicker(page), name);
}

// ---------------------------------------------- a batch, and one undo of it ---

test("@fast a batch of two rows moves in one write, and one Undo reverts all of it", async ({
  page,
  request,
}) => {
  // The longest test in the file — two API setups, a page, the picker, a write,
  // an undo, and a read-back of each. It fits the default 30 s alone and does
  // not under a full parallel suite, which is a slow test, not a flaky one.
  test.slow();
  const [first, second] = await unownedCards(request, 2);
  const printings = [first.printing_id as string, second.printing_id as string];
  const source = await createCollection(request, "binder", "src");
  const dest = await createCollection(request, "deck", "dest");
  try {
    await addHave(request, source.id, printings[0], 2);
    await addHave(request, source.id, printings[1], 2);
    // The destination *wants* both cards, so it is a suggested destination —
    // and its hint is the shortfall summed over the selection, which is the
    // whole reason the ranking is computed per batch rather than per card.
    await addWant(request, dest.id, first.oracle_id, 1);
    await addWant(request, dest.id, second.oracle_id, 1);

    await page.goto(`/my/collections/${source.id}`);
    await hydrated(page);

    const rowA = collectionRow(page, first.printing_id as string);
    const rowB = collectionRow(page, second.printing_id as string);
    await select(rowA).click();
    await select(rowB).click();
    await expect(page.locator(COUNT)).toHaveText("2 cards");

    // The picker: the catalog's control, ranked for this selection. The wanted
    // destination leads, hinting `wants 2` — one shortfall from each card, which
    // is the ranking being computed for the batch rather than per card.
    const picker = await openPicker(page);
    await expect(picker.locator(OPTION).first()).toContainText(dest.name);
    await expect(picker.locator(HINT).first()).toHaveText("wants 2");

    await pick(picker, dest.name);

    const toast = page.locator(TOAST, { hasText: "Moved 2 cards" });
    await expect(toast).toContainText(`Moved 2 cards (1 copy each) → 🗂 ${dest.name}`);
    // Both entries moved, so both leave the tray and the pill goes away.
    await expect(page.locator(TRAY)).toHaveCount(0);

    // The page followed the write without a reload…
    await expect(rowA.locator('[data-testid="count-stepper-value"]')).toHaveText(
      "1",
    );
    // …and the database actually moved, which the toast alone cannot show.
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([1, 1]);
      expect(await present(request, dest.id, printings)).toEqual([1, 1]);
    }).toPass({ timeout: 5000 });

    // One Undo, both cards back — the failure this asserts against is an undo
    // that reverts only the last item of the batch.
    await toast.getByRole("button", { name: "Undo" }).click();
    await expect(page.locator(TOAST, { hasText: "Put them back" })).toBeVisible();
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([2, 2]);
      expect(await present(request, dest.id, printings)).toEqual([0, 0]);
    }).toPass({ timeout: 5000 });
    await expect(rowB.locator('[data-testid="count-stepper-value"]')).toHaveText(
      "2",
    );
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// ------------------------------------------- resolving a `/my` (oracle) row ---

test("@fast a /my row held in one place resolves to that place and moves", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 5);
  const source = await createCollection(request, "binder", "mysrc");
  const dest = await createCollection(request, "binder", "mydest");
  try {
    await addHave(request, source.id, card.printing_id as string, 2);

    // `/my` names neither the collection nor the held printing — the row is
    // per-oracle and its printing is only the representative one. Both are
    // resolved server-side from what the caller actually holds.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    const row = myRow(page, card.oracle_id);
    await select(row).click();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    await moveTo(page, dest.name);

    await expect(
      page.locator(TOAST, { hasText: "Moved 1 card" }),
    ).toContainText(`Moved 1 card (1 copy) → 🗂 ${dest.name}`);
    const printings = [card.printing_id as string];
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([1]);
      expect(await present(request, dest.id, printings)).toEqual([1]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// ----------------------------------- grains and boards a row cannot show ---
//
// Every read model the UI renders collapses the grain a move is addressed at:
// `collection_view` groups by `(printing, board)` and `CardDetail::ownership`
// by `(collection, printing)`. So a foil-only stack and a sideboard-only card
// are selectable rows that look exactly like movable ones. Before the
// resolution read they reached `holding_take`, which matches the full grain and
// `board = 'main'`, returned `Conflict("no copies to move")`, and — because the
// batch is one transaction — killed every *other* card in the selection with an
// error naming none of them. These two tests are the ones that catch that.

test("@fast a foil-only row is refused by name while the rest of the batch moves", async ({
  page,
  request,
}) => {
  test.slow();
  const [plain, foil] = await unownedCards(request, 2, 10);
  const source = await createCollection(request, "binder", "grainsrc");
  const dest = await createCollection(request, "binder", "graindest");
  try {
    await addHave(request, source.id, plain.printing_id as string, 2);
    // Foil, and *only* foil: the row still renders `present = 2` with a
    // checkbox, because the view sums across finishes.
    await addHave(request, source.id, foil.printing_id as string, 2, {
      finish: "foil",
    });

    await page.goto(`/my/collections/${source.id}`);
    await hydrated(page);
    const foilRow = collectionRow(page, foil.printing_id as string);
    await expect(
      foilRow.locator('[data-testid="count-stepper-value"]'),
      "the fixture must render the foil stack as an ordinary selectable row",
    ).toHaveText("2");

    await select(collectionRow(page, plain.printing_id as string)).click();
    await select(foilRow).click();
    await expect(page.locator(COUNT)).toHaveText("2 cards");

    await moveTo(page, dest.name);

    // The movable one moved…
    await expect(page.locator(TOAST, { hasText: "Moved 1 card" })).toContainText(
      `Moved 1 card (1 copy) → 🗂 ${dest.name}`,
    );
    // …and the other was named, with a reason, rather than taking the batch
    // down with an error naming no card at all.
    await expect(
      page.locator(TOAST, { hasText: "wasn't moved" }),
    ).toContainText(`${foil.name} has only foil, non-NM or non-English copies`);
    // It is still checked: the refusal is work left to do, not a silent drop.
    await expect(page.locator(COUNT)).toHaveText("1 card");

    await expect(async () => {
      expect(
        await present(request, source.id, [
          plain.printing_id as string,
          foil.printing_id as string,
        ]),
      ).toEqual([1, 2]);
      expect(
        await present(request, dest.id, [
          plain.printing_id as string,
          foil.printing_id as string,
        ]),
      ).toEqual([1, 0]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

test("@fast a /my row whose copies are all sideboarded is refused by board", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 15);
  const deck = await createCollection(request, "deck", "sidedeck");
  const dest = await createCollection(request, "binder", "sidedest");
  try {
    await addHave(request, deck.id, card.printing_id as string, 2, {
      board: "side",
    });

    // `/my` aggregates across boards, so the row reads OWNED 2 and offers a
    // checkbox — the board is invisible here, which is exactly the trap.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    await select(myRow(page, card.oracle_id)).click();

    await moveTo(page, dest.name);

    await expect(page.locator(TOAST).first()).toContainText(
      `${card.name} sits on the side board`,
    );
    await expect(page.locator(COUNT)).toHaveText("1 card");
    // Nothing was taken from the mainboard it does not have, and nothing
    // landed in the destination.
    const landed = await viewOf(request, dest.id);
    expect(landed.cards).toHaveLength(0);
    expect(
      await present(request, deck.id, [card.printing_id as string]),
    ).toEqual([2]);
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, dest.id);
  }
});

test("@fast a /my row scattered over collections is refused by name, and nothing moves", async ({
  page,
  request,
}) => {
  const scattered = await scatteredCard(request);
  const places = scattered.locations.length;
  const dest = await createCollection(request, "binder", "refuse");
  try {
    await page.goto(`/my?q=${encodeURIComponent(scattered.card.name)}`);
    await hydrated(page);
    const row = myRow(page, scattered.card.oracle_id);
    await select(row).click();

    await moveTo(page, dest.name);

    // Named and reasoned, not silently dropped from the batch.
    await expect(page.locator(TOAST).first()).toContainText(
      `${scattered.card.name} is in ${places} collections`,
    );
    // …and it is still checked, because it is still work to do.
    await expect(page.locator(COUNT)).toHaveText("1 card");
    // Nothing was written: no source was guessed, so no copies were conjured
    // and none were taken from any of the places it really sits.
    const landed = await viewOf(request, dest.id);
    expect(landed.cards).toHaveLength(0);
  } finally {
    await deleteCollection(request, dest.id);
  }
});
