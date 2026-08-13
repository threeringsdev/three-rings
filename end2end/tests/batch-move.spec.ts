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
//   system, so a guess would conjure them);
// - a stale tray entry (P6-122: the stepper drove its row to zero after it was
//   selected, before the move ran) does not fail the batch and does not write
//   wrong — the live rows still move, the dead one is refused by name, and
//   *unlike* the still-actionable refusals above, it does not stay checked:
//   the server just proved it gone, so the tray drops it too.
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
// The which-copies step (P6-151). The *rows* are the load-bearing seam: a
// closed dialog keeps its box (and its footer buttons) in the DOM, so a
// visibility assertion on the panel would pass whether or not the step ever
// opened — the rows mount only while it is open, so counting them is the
// assertion that cannot lie.
const STEP_CARD = '[data-testid="which-copies-card"]';
const STEP_ROW = '[data-testid="which-copies-row"]';
const STEP_CONFIRM = '[data-testid="which-copies-confirm"]';
const STEP_CANCEL = '[data-testid="which-copies-cancel"]';

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

/// How many distinct `(collection, printing, board)` stacks of a card the
/// caller holds — the same grouping the which-copies step lists as rows, read
/// from the one endpoint that does not group any of it away.
///
/// Derived rather than assumed, because "how many places is this card in" is
/// exactly what the step renders and exactly what `unownedCards` cannot
/// promise: its "owned nowhere" is really "not in the first 200 rows of `/my`",
/// which on a fixture of thousands lets an owned card through (observed — a
/// seeded 2-stack card rendered 3 rows, correctly).
async function stackCount(
  request: APIRequestContext,
  oracleId: string,
  exclude: string,
): Promise<number> {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status(), "holdings of oracle").toBe(200);
  const rows = (await res.json()) as {
    collection_id: string;
    printing_id: string;
    board: string;
    quantity: number;
  }[];
  const stacks = new Set(
    rows
      .filter((h) => h.quantity > 0 && h.collection_id !== exclude)
      .map((h) => `${h.collection_id}/${h.printing_id}/${h.board}`),
  );
  return stacks.size;
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

/// The holding row `(collectionId, printingId)` resolves to — the id
/// `POST /api/holdings/{id}/quantity` takes, i.e. what the count stepper's own
/// commit addresses. Used to simulate exactly that commit from outside the
/// page, driving a *selected* row's stack to zero between selection and move.
async function holdingId(
  request: APIRequestContext,
  oracleId: string,
  collectionId: string,
  printingId: string,
): Promise<string> {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status(), "holdings of oracle").toBe(200);
  const rows = (await res.json()) as {
    id: string;
    collection_id: string;
    printing_id: string;
  }[];
  const row = rows.find(
    (h) => h.collection_id === collectionId && h.printing_id === printingId,
  );
  expect(row, "the seeded holding must still be there to find").toBeTruthy();
  return (row as { id: string }).id;
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
// resolution read they reached `holding_take`, which matched the full grain and
// `board = 'main'`, returned `Conflict("no copies to move")`, and — because the
// batch is one transaction — killed every *other* card in the selection with an
// error naming none of them. That was fixed first by refusing them, and now by
// **moving** them: `MoveItem` carries the grain and the board of the stack the
// resolution actually found. These two tests are the ones that catch a
// regression to either behavior — an assertion that the copies moved is also an
// assertion that they were addressed correctly, since a default-grain write
// finds nothing to take.

test("@fast a foil-only row moves as foil, alongside a plain one", async ({
  page,
  request,
}) => {
  test.slow();
  const [plain, foil] = await unownedCards(request, 2, 10);
  const source = await createCollection(request, "binder", "grainsrc");
  const dest = await createCollection(request, "binder", "graindest");
  try {
    await addHave(request, source.id, plain.printing_id as string, 2);
    // Foil, and *only* foil: the row renders `present = 2` with a checkbox,
    // because the view sums across finishes and says nothing about which.
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

    // Both moved, in one write — and nothing was refused.
    await expect(page.locator(TOAST, { hasText: "Moved 2 cards" })).toContainText(
      `Moved 2 cards (1 copy each) → 🗂 ${dest.name}`,
    );
    await expect(page.locator(TOAST, { hasText: "wasn't moved" })).toHaveCount(0);
    await expect(page.locator(COUNT)).toHaveCount(0);

    await expect(async () => {
      expect(
        await present(request, source.id, [
          plain.printing_id as string,
          foil.printing_id as string,
        ]),
      ).toEqual([1, 1]);
      expect(
        await present(request, dest.id, [
          plain.printing_id as string,
          foil.printing_id as string,
        ]),
      ).toEqual([1, 1]);
    }).toPass({ timeout: 5000 });

    // The copy that landed is still foil — the assertion a default-grain write
    // would fail even while the counts above looked right.
    const landed = await request.get(`/api/cards/${foil.oracle_id}/holdings`);
    expect(landed.status()).toBe(200);
    const rows = (await landed.json()) as {
      collection_id: string;
      finish: string;
    }[];
    expect(
      rows.filter((h) => h.collection_id === dest.id).map((h) => h.finish),
    ).toEqual(["foil"]);
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

test("@fast a /my row whose copies are all sideboarded moves off the sideboard", async ({
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
    // checkbox — the board is invisible here. It used to be a refusal for that
    // reason; the ungrouped read supplies it to the write instead.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    await select(myRow(page, card.oracle_id)).click();

    await moveTo(page, dest.name);

    await expect(page.locator(TOAST).first()).toContainText("Moved 1 card");
    await expect(async () => {
      // Taken off the *sideboard* — a `board = 'main'` write would have found
      // nothing and rolled back — and landed in the binder as an ordinary copy.
      const res = await request.get(`/api/cards/${card.oracle_id}/holdings`);
      const rows = (await res.json()) as {
        collection_id: string;
        board: string;
        quantity: number;
      }[];
      expect(
        rows
          .map((h) => `${h.collection_id === deck.id ? "deck" : "dest"}/${h.board} x${h.quantity}`)
          .sort(),
      ).toEqual(["deck/side x1", "dest/main x1"]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, dest.id);
  }
});

test("@fast a /my row scattered over collections asks which copies, and cancelling refuses it by name", async ({
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

    // P6-151: the batch no longer dead-ends here — it asks. The step lists one
    // row per place the copies actually sit in, which is the question the
    // oracle-grained key could not answer on its own.
    await expect(page.locator(STEP_ROW)).toHaveCount(places);
    // Declining is still a refusal, named and reasoned — not a silent drop.
    await page.locator(STEP_CANCEL).click();
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

// ------------------------------------------- which copies? (P6-151) ---
//
// The refusal above is only half the contract now. A `/my` row whose copies are
// spread over several collections opens the **which-copies step**: the concrete
// stacks behind the card, one row per (collection, printing, board) with its
// count, and a submit that goes back through the *same* batch move as
// `SelectionKey::Held` items — which is what makes the picked copies precise
// without a new write path.
//
// The assertion that matters is the last one, and it is API-read: exactly the
// picked stack moved and the other one is untouched. A step that moved "a copy
// of that card" from wherever it felt like would satisfy every toast in this
// test and fail that.

test("@fast an ambiguous /my row asks which copies, and moves exactly the stack picked", async ({
  page,
  request,
}) => {
  test.slow();
  const [card] = await unownedCards(request, 1, 25);
  const printing = card.printing_id as string;
  const here = await createCollection(request, "binder", "amb-here");
  const there = await createCollection(request, "binder", "amb-there");
  const dest = await createCollection(request, "binder", "amb-dest");
  try {
    // Two collections, deliberately different counts: the step's rows have to
    // say *how many* copies are where, and equal counts could not tell a row
    // that reads its own stack from one that reads the other's.
    await addHave(request, here.id, printing, 2);
    await addHave(request, there.id, printing, 3);
    // What the step should list, read from the database rather than assumed to
    // be the two seeded here — see `stackCount`.
    const stacks = await stackCount(request, card.oracle_id, dest.id);
    expect(stacks, "the seeded card must be in at least two places").toBeGreaterThan(1);

    // The `/my` row aggregates both — it reads OWNED 5 and names neither
    // place, which is exactly why the batch cannot resolve it alone.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    await select(myRow(page, card.oracle_id)).click();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    await moveTo(page, dest.name);

    // The step, not a dead end: one section for the card, one row per stack,
    // each naming its collection and its count.
    await expect(page.locator(STEP_CARD)).toHaveAttribute("data-card", card.name);
    const rows = page.locator(STEP_ROW);
    await expect(rows).toHaveCount(stacks);
    // Each row states what *ticking it* does — one copy, out of the stack's own
    // count — not the size of the stack it would take.
    await expect(rows.filter({ hasText: here.name })).toHaveText(
      `${here.name} · 1 of 2 copies`,
    );
    await expect(rows.filter({ hasText: there.name })).toHaveText(
      `${there.name} · 1 of 3 copies`,
    );
    // The destination is not offered as a place to take copies *from*.
    await expect(rows.filter({ hasText: dest.name })).toHaveCount(0);

    // Tick *both* stacks of the same card — the step's headline flow, and the
    // one the tray's own "N cards, 1 copy each" phrasing cannot describe: this
    // is one card and two copies, and a toast saying "2 cards" is a false
    // statement about the user's collection.
    await rows.filter({ hasText: here.name }).click();
    const confirm = page.locator(STEP_CONFIRM);
    await expect(confirm).toHaveText("Move 1 copy");
    await rows.filter({ hasText: there.name }).click();
    await expect(confirm).toHaveText("Move 2 copies");
    await confirm.click();

    await expect(page.locator(TOAST, { hasText: "Moved" })).toContainText(
      `Moved 2 copies of 1 card → 🗂 ${dest.name}`,
    );
    // The question was answered, so the tray stops asking it — the entry the
    // step was opened for is a `card:` token, and the move that answered it
    // reported `held:` ones.
    await expect(page.locator(TRAY)).toHaveCount(0);

    // The assertion the whole feature stands on: each picked stack gave up
    // exactly one copy — not one copy total, and not the whole stack — and the
    // destination got both.
    await expect(async () => {
      expect(await present(request, here.id, [printing])).toEqual([1]);
      expect(await present(request, there.id, [printing])).toEqual([2]);
      expect(await present(request, dest.id, [printing])).toEqual([2]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, here.id);
    await deleteCollection(request, there.id);
    await deleteCollection(request, dest.id);
  }
});

// --------------------------------------- a stale entry does not sink a batch ---
//
// P6-122: a tray key can outlive what it names — a stepper commit, a teardown,
// a collection deletion. This drives exactly the stepper case from outside the
// page (the same `POST /api/holdings/{id}/quantity` the count stepper's own
// commit calls), between selecting the row and running the move, and checks
// the whole contract the staleness policy promises: the live rows still move
// (no whole-batch abort), the write is honest (nothing lands for the dead
// entry — no wrong-write), and the tray stops counting it once the server has
// said so (unlike the still-actionable refusal above, which stays checked).

test("@fast a row driven to zero after selection is refused by name, the rest of the batch still moves, and the tray drops it", async ({
  page,
  request,
}) => {
  test.slow();
  const [a, b, zeroed] = await unownedCards(request, 3, 20);
  const printings = [a, b, zeroed].map((c) => c.printing_id as string);
  const source = await createCollection(request, "binder", "stalesrc");
  const dest = await createCollection(request, "binder", "staledest");
  try {
    await addHave(request, source.id, a.printing_id as string, 1);
    await addHave(request, source.id, b.printing_id as string, 1);
    await addHave(request, source.id, zeroed.printing_id as string, 1);

    await page.goto(`/my/collections/${source.id}`);
    await hydrated(page);

    // Select all three while every row is still live.
    await select(collectionRow(page, a.printing_id as string)).click();
    await select(collectionRow(page, b.printing_id as string)).click();
    await select(collectionRow(page, zeroed.printing_id as string)).click();
    await expect(page.locator(COUNT)).toHaveText("3 cards");

    // The stepper's own commit, from outside this page — the row is *already
    // selected* when its stack goes to zero, which is the case the tray's
    // staleness policy exists for (SelectionKey's doc comment).
    const zeroId = await holdingId(
      request,
      zeroed.oracle_id,
      source.id,
      zeroed.printing_id as string,
    );
    const zeroRes = await request.post(`/api/holdings/${zeroId}/quantity`, {
      data: { quantity: 0 },
    });
    expect(zeroRes.status(), "zero the stepper-driven row").toBe(200);

    await moveTo(page, dest.name);

    // The two live rows moved in the same write — one stale entry did not
    // fail the batch wholesale.
    await expect(
      page.locator(TOAST, { hasText: "Moved 2 cards" }),
    ).toContainText(`Moved 2 cards (1 copy each) → 🗂 ${dest.name}`);
    // The dead one is named honestly, not silently swallowed.
    await expect(page.locator(TOAST, { hasText: "wasn't moved" })).toContainText(
      `${zeroed.name} has no copies left to move — reload the page`,
    );
    // …and, unlike a refusal the user can still act on, the tray does not
    // keep counting something the server just proved gone: every entry
    // cleared, moved or dropped, so the pill itself is gone.
    await expect(page.locator(TRAY)).toHaveCount(0);

    // The database agrees: the live pair actually moved, and the dead entry
    // was never touched again — it stayed at zero, not conjured back or
    // written into the destination.
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([0, 0, 0]);
      expect(await present(request, dest.id, printings)).toEqual([1, 1, 0]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// --------------------------------- a move made ON the card-detail page ---
//
// P6-152: `HoldingsRevision` was a source only on `/my` and
// `/my/collections/:id` — `/cards/:id`'s own "Your copies" block had no way to
// hear a move happen, so a batch move performed while parked on that page left
// it naming the pre-move collection until an unrelated reload. The tray is
// shell-level and a selection survives navigation (P6-122), so the real repro
// stays entirely client-side: select the row on `/my`, follow its own link
// into `/cards/:id` (a real SPA navigation — `page.goto` would legitimately
// start the selection empty), move through the tray *parked on the detail
// page*, and assert the ownership block updates with no navigation between
// the write and the read. Base: the block keeps naming `source` until reload.
//
// `unownedCards`'s own "owned nowhere" is only "not in the first 200 rows of
// `/my`" (its own doc) — on today's bulk-loaded seed the dev user's Inbox
// already holds nearly the whole catalog at real quantities, so a candidate
// from it is routinely owned elsewhere too (base-parity triage: the same
// contamination sinks two of this file's own *unmodified* tests, see the
// suite header). Adding a copy to a fresh `source` collection therefore
// cannot assume single-place resolution either — this drives whichever path
// the tray actually takes (a direct resolve, or the which-copies step,
// P6-151) rather than assuming one, and its own assertions do not depend on
// which one fires.

test("@fast a move made from the card-detail page updates its own ownership block without a reload", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 40);
  const source = await createCollection(request, "binder", "detailsrc");
  const dest = await createCollection(request, "binder", "detaildest");
  try {
    await addHave(request, source.id, card.printing_id as string, 1);

    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    const row = myRow(page, card.oracle_id);
    await select(row).click();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    // The row's own link into the detail page (all_cards.rs) — clicked, not
    // `goto`'d, so the in-memory tray selection rides along.
    await row.locator(`a[href="/cards/${card.oracle_id}"]`).click();
    await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
    await expect(page.getByTestId("card-name")).toContainText(card.name);

    // The tray survived the navigation (P6-122) — still parked, still one
    // card — and its own picker is what this test drives from here.
    await expect(page.locator(TRAY)).toBeVisible();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    const ownership = page.getByTestId("your-copies");
    await expect(ownership).toBeVisible();
    await expect(ownership).toContainText(source.name);

    await moveTo(page, dest.name);

    // Either the batch resolved directly, or (the likelier case against
    // today's seed) it opened the which-copies step — wait for whichever the
    // tray actually did instead of assuming one, then answer it if it asked:
    // the `source` stack is the only one this test wants moved.
    const stepRows = page.locator(STEP_ROW);
    const movedToast = page.locator(TOAST, { hasText: "Moved" });
    await expect(async () => {
      expect((await stepRows.count()) > 0 || (await movedToast.count()) > 0).toBe(
        true,
      );
    }).toPass({ timeout: 5000 });
    if (await stepRows.count()) {
      await stepRows.filter({ hasText: source.name }).click();
      await page.locator(STEP_CONFIRM).click();
    }
    await expect(movedToast).toContainText(dest.name);
    await expect(page.locator(TRAY)).toHaveCount(0);

    // The assertion the whole fix stands on: no navigation happened between
    // the write above and this read, so a block still naming `source` here
    // means the page's own resource never took the revision as a source.
    await expect(ownership).toContainText(dest.name);
    await expect(ownership).not.toContainText(source.name);

    await expect(async () => {
      expect(
        await present(request, source.id, [card.printing_id as string]),
      ).toEqual([0]);
      expect(
        await present(request, dest.id, [card.printing_id as string]),
      ).toEqual([1]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// ------------------------------- the tray picker's empty line tells the truth ---
//
// P6-152, the picker's other gap: `empty="No collection to move to."` on the
// tray's `DestinationList` fired for a *filtered* zero too, not only a failed
// or genuinely empty read — so typing a search term that matched nothing
// claimed the user had nowhere to move copies, which was false (their
// collections are right there, unfiltered). `DestinationList`'s own default
// ("No collection matches.", the catalog picker's wording) is the true
// sentence for a filter, and the tray now uses it instead of overriding it.
//
// The genuinely-empty case this override used to (over-)cover cannot happen
// here: `collection_list()` provisions the caller's undeletable Inbox row as
// a side effect of the very read backing this list (`ensure_inbox`,
// collection-api.md → "Inbox provisioning"), so the tray's registry is never
// really empty — only ever filtered down to nothing.

test("@fast a no-match search in the tray picker says so, not that there's nowhere to move to", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 35);
  const source = await createCollection(request, "binder", "pickersrc");
  try {
    await addHave(request, source.id, card.printing_id as string, 1);

    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    await select(myRow(page, card.oracle_id)).click();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    const picker = await openPicker(page);
    // A term no scratch collection name (nor the seeded Inbox) can match.
    await picker
      .locator('[data-name="CommandInput"]')
      .fill("zzz-no-such-collection-at-all");
    await expect(picker).toContainText("No collection matches.");
    await expect(picker).not.toContainText("No collection to move to.");

    // Still selected, and nothing written — opening and filtering the picker
    // is not itself an action.
    await expect(page.locator(COUNT)).toHaveText("1 card");
  } finally {
    await deleteCollection(request, source.id);
  }
});
