// Batch move — the selection tray's "Move to…" (specs/app-ui.md → Selection
// tray; specs/collection-api.md → "Move (batch)").
//
// The contract, in assertion order:
//
// - the picker is the *catalog's* destination picker, ranked for the selection:
//   the same `destination-option` rows and search box, with the collections that
//   want the selected cards first, hinting their summed shortfall;
// - a batch of two rows the user chose quantities for moves both, in one write,
//   and the page it was moved out of follows the database rather than waiting
//   for a reload;
// - the single Undo reverts the **whole** batch — every card *and* every copy
//   of it, not the last card of it;
// - a `/my` row whose copies sit in exactly one place, one copy of it, resolves
//   to that place and moves without asking anything;
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

/// HERE-cell-scoped stepper locators. Every collection row carries **two**
/// steppers since WANTED became a still-needed count you can edit
/// (specs/app-ui.md, maintainer ruling 2026-08-19), so a row-scoped
/// `count-stepper-*` locator resolves to two elements and fails Playwright's
/// strict mode. Say which column you mean.
const HERE_VALUE =
  '[data-testid="here-cell"] [data-testid="count-stepper-value"]';
const HERE_INC = '[data-testid="here-cell"] [data-testid="count-stepper-inc"]';
const HERE_DEC = '[data-testid="here-cell"] [data-testid="count-stepper-dec"]';

const TRAY = '[data-testid="selection-tray"]';
const COUNT = '[data-testid="tray-count"]';
const MOVE = '[data-testid="tray-move"]';
const PICKER = "#popover-tray-destination";
const OPTION = '[data-testid="destination-option"]';
const HINT = '[data-testid="destination-hint"]';
const TOAST = '[data-name="Toast"]';
// The which-copies step (P6-151, and the quantity-and-version picker it became
// in P6-150). The *rows* are the load-bearing seam: a closed dialog keeps its
// box (and its footer buttons) in the DOM, so a visibility assertion on the
// panel would pass whether or not the step ever opened — the rows mount only
// while it is open, so counting them is the assertion that cannot lie.
const STEP_CARD = '[data-testid="which-copies-card"]';
const STEP_ROW = '[data-testid="which-copies-row"]';
const STEP_CONFIRM = '[data-testid="which-copies-confirm"]';
const STEP_CANCEL = '[data-testid="which-copies-cancel"]';
const PICK_VALUE = '[data-testid="pick-value"]';
const PICK_INC = '[data-testid="pick-inc"]';
const PICK_DEC = '[data-testid="pick-dec"]';
const PICK_LABEL = '[data-testid="pick-label"]';
const PANEL = '[data-testid="which-copies"]';

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
/// "Owned nowhere" is asked of the holdings endpoint per candidate, with `/my`
/// as a cheap prefilter, because
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

  // Every candidate is **verified** held-nowhere, one holdings read each
  // (P6-150). The `/my` filter above is only "not in the first 200 rows", and
  // today's bulk-loaded seed parks a large slice of the catalog in the dev
  // user's Inbox — so a candidate routinely came back "free" while sitting in
  // the Inbox at 51 copies, which turns every single-place assertion in this
  // file into a which-copies dialog. `GET /api/cards/{id}/holdings` is the one
  // read that cannot be wrong about it, and it is cheap enough to ask per
  // candidate because the scan stops at `n`.
  const takeFree = async (from: Card[], want: number) => {
    const out: Card[] = [];
    for (const card of from) {
      const held = await request.get(`/api/cards/${card.oracle_id}/holdings`);
      expect(held.status(), "holdings of oracle").toBe(200);
      if (((await held.json()) as { quantity: number }[]).some((h) => h.quantity > 0)) {
        continue;
      }
      out.push(card);
      if (out.length === want) break;
    }
    return out;
  };

  // **The scan is windowed, and that is what keeps concurrent tests apart.**
  // `skip` used to slice a fixed `n` cards, so two tests were disjoint by
  // construction; verifying each candidate broke that, because a scan that
  // walks past its own block converges on the same first free card every other
  // block eventually reaches. Each call searches its own `STRIDE`-wide block
  // first — the spacing this file's own `skip` values use — and only falls
  // back to the rest of the list when its block is fully owned.
  const STRIDE = 5;

  // **Several search terms, tried in order, because one letter's luck is not a
  // fixture.** This helper searched `q=z` alone, chosen once because the seed
  // picked its cards from name-ordered searches and the alphabetically-first
  // slice was the owned one. Measured again for P6-150: `z` now yields **zero**
  // held-nowhere cards in its first 40 hits (the Inbox has swallowed that
  // slice too) while `q`, `x`, `vi` and `un` yield 27-32 — and the erosion
  // happened between two runs of this same suite, so pinning a second letter
  // would only postpone this. Trying them in order costs one extra search per
  // exhausted term and nothing at all in the common case.
  for (const term of ["q", "x", "vi", "un", "z"]) {
    const res = await request.get(`/api/catalog/search?q=${term}&limit=120`);
    expect(res.status(), "catalog search").toBe(200);
    const { cards } = (await res.json()) as { cards: Card[] };
    // Single-faced only. A double-faced card's catalog name is
    // `Front // Back` while `/cards/:id` heads with the front face alone, so a
    // fixture that picked one made `expect(card-name).toContainText(card.name)`
    // fail on a page that was showing exactly the right card (seen the moment
    // the search term above changed). Nothing in this file needs a DFC.
    const candidates = cards.filter(
      (c) => c.printing_id && !c.name.includes(" // ") && !taken.has(c.oracle_id),
    );
    const free = await takeFree(candidates.slice(skip, skip + STRIDE), n);
    if (free.length < n) {
      free.push(...(await takeFree(candidates.slice(skip + STRIDE), n - free.length)));
    }
    if (free.length === n) return free;
  }
  throw new Error(
    `no search term yielded ${n} catalog cards the dev user owns nowhere past offset ${skip} — ` +
      "the seed's owned slice has grown; measure a fresh term and add it above",
  );
}

/// How many distinct **full-grain** stacks of a card the caller holds —
/// `(collection, printing, board, finish, condition, language)`, the same
/// grouping the which-copies step lists as rows since P6-150, read from the one
/// endpoint that does not group any of it away.
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
    finish: string;
    condition: string;
    language: string;
    quantity: number;
  }[];
  const stacks = new Set(
    rows
      .filter((h) => h.quantity > 0 && h.collection_id !== exclude)
      .map(
        (h) =>
          `${h.collection_id}/${h.printing_id}/${h.board}/${h.finish}/${h.condition}/${h.language}`,
      ),
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

// ------------------------------------------- the quantity picker's controls ---

/// Drive one which-copies row's stepper to `want`, one press per copy.
///
/// Pressed rather than typed on purpose: `− n +` is the whole control on a
/// phone (the collection view's `CountStepper` hides its buttons below `sm`
/// and expects a tap-to-type instead, which is exactly why this picker does
/// not reuse it), so the buttons are the path a real user takes and the one
/// worth asserting. The value is read back after each press, so a swallowed
/// click fails here rather than as a wrong quantity three assertions later.
async function setQuantity(
  row: ReturnType<Page["locator"]>,
  want: number,
) {
  const value = row.locator(PICK_VALUE);
  for (let guard = 0; guard < 20; guard++) {
    const now = Number(await value.innerText());
    if (now === want) return;
    await row.locator(now < want ? PICK_INC : PICK_DEC).click();
    await expect(value).toHaveText(String(now < want ? now + 1 : now - 1));
  }
  throw new Error(`stepper never reached ${want}`);
}

/// Zero every row of the open picker except the ones held in `keep`.
///
/// Every row opens at **one copy** (P6-150's default), so a card whose copies
/// sit in several places would move one from each on a bare confirm. Tests
/// that care about exactly one stack say so.
///
/// Matched on the label's **first segment**, which is the collection name, and
/// compared exactly: a substring test over the whole row would keep
/// `zz-…-src-w0-1` when asked for `zz-…-src-w0-1x`, and would also match a
/// name that happened to appear in the printing chip or the grain.
async function onlyFrom(page: Page, keep: string) {
  const rows = page.locator(STEP_ROW);
  for (let i = 0; i < (await rows.count()); i++) {
    const row = rows.nth(i);
    const place = (await row.locator(PICK_LABEL).innerText()).split(" · ")[0];
    if (place.trim() !== keep) await setQuantity(row, 0);
  }
}

// ---------------------------------------------- a batch, and one undo of it ---

test("@fast a batch of two rows moves the copies picked for each, and one Undo reverts all of it", async ({
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

    // P6-150: each row holds two copies, so neither moves on a guess — the
    // picker asks how many, one section per selected card, each opening at one
    // copy out of the two that are there.
    await expect(page.locator(STEP_ROW)).toHaveCount(2);
    // One section per selected card, in the order they were selected — asserted
    // rather than assumed, because the two rows below are told apart by it.
    const sections = page.locator(STEP_CARD);
    await expect(sections).toHaveCount(2);
    await expect(sections.nth(0)).toHaveAttribute("data-card", first.name);
    await expect(sections.nth(1)).toHaveAttribute("data-card", second.name);
    const rowOfA = sections.nth(0).locator(STEP_ROW);
    const rowOfB = sections.nth(1).locator(STEP_ROW);
    await expect(rowOfA).toContainText(`${source.name} · 2 copies`);
    await expect(rowOfA.locator(PICK_VALUE)).toHaveText("1");
    // The whole stack is offered, and no more than the whole stack.
    await setQuantity(rowOfA, 2);
    await expect(rowOfA.locator(PICK_INC)).toBeDisabled();
    await setQuantity(rowOfB, 1);
    const confirm = page.locator(STEP_CONFIRM);
    await expect(confirm).toHaveText("Move 3 copies");
    await confirm.click();

    // Copies, not cards — the count the old "(1 copy each)" wording could not
    // state at all, and the point of the whole story.
    const toast = page.locator(TOAST, { hasText: /Moved \d+ cop/ });
    await expect(toast).toContainText(`Moved 3 copies of 2 cards → 🗂 ${dest.name}`);
    // Both entries were answered, so both leave the tray and the pill goes away.
    await expect(page.locator(TRAY)).toHaveCount(0);

    // The page followed the write without a reload — the half-emptied row
    // counts down on its own.
    await expect(rowB.locator(HERE_VALUE)).toHaveText(
      "1",
    );
    // …and the database actually moved, which the toast alone cannot show.
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([0, 1]);
      expect(await present(request, dest.id, printings)).toEqual([2, 1]);
    }).toPass({ timeout: 5000 });

    // One Undo, every card *and* every copy back — the failures this asserts
    // against are an undo that reverts only the last item of the batch, and one
    // that reverses a quantity of 1 for a move of 2.
    await toast.getByRole("button", { name: "Undo" }).click();
    await expect(page.locator(TOAST, { hasText: "Put them back" })).toBeVisible();
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([2, 2]);
      expect(await present(request, dest.id, printings)).toEqual([0, 0]);
    }).toPass({ timeout: 5000 });
    await expect(rowB.locator(HERE_VALUE)).toHaveText(
      "2",
    );
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// ------------------------------------------- resolving a `/my` (oracle) row ---

test("@fast a /my row holding one copy in one place resolves to it and moves, unasked", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 5);
  const source = await createCollection(request, "binder", "mysrc");
  const dest = await createCollection(request, "binder", "mydest");
  try {
    // One copy, in one place: the direct path P6-150 deliberately kept, because
    // a stack of one has no quantity question in it to ask.
    await addHave(request, source.id, card.printing_id as string, 1);

    // `/my` names neither the collection nor the held printing — the row is
    // per-oracle and its printing is only the representative one. Both are
    // resolved server-side from what the caller actually holds.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    const row = myRow(page, card.oracle_id);
    await select(row).click();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    await moveTo(page, dest.name);

    // No dialog on the way: the picker did not open, so the copy moved on the
    // direct path.
    await expect(page.locator(STEP_ROW)).toHaveCount(0);
    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 1 copy of 1 card → 🗂 ${dest.name}`,
    );
    const printings = [card.printing_id as string];
    await expect(async () => {
      expect(await present(request, source.id, printings)).toEqual([0]);
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
    await addHave(request, source.id, plain.printing_id as string, 1);
    // Foil, and *only* foil: the row renders `present = 1` with a checkbox,
    // because the view sums across finishes and says nothing about which.
    await addHave(request, source.id, foil.printing_id as string, 1, {
      finish: "foil",
    });

    await page.goto(`/my/collections/${source.id}`);
    await hydrated(page);
    const foilRow = collectionRow(page, foil.printing_id as string);
    await expect(
      foilRow.locator(HERE_VALUE),
      "the fixture must render the foil stack as an ordinary selectable row",
    ).toHaveText("1");

    await select(collectionRow(page, plain.printing_id as string)).click();
    await select(foilRow).click();
    await expect(page.locator(COUNT)).toHaveText("2 cards");

    await moveTo(page, dest.name);

    // Both moved, in one write — and nothing was refused or asked about.
    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 2 copies of 2 cards → 🗂 ${dest.name}`,
    );
    await expect(page.locator(TOAST, { hasText: "wasn't moved" })).toHaveCount(0);
    await expect(page.locator(STEP_ROW)).toHaveCount(0);
    await expect(page.locator(COUNT)).toHaveCount(0);

    await expect(async () => {
      expect(
        await present(request, source.id, [
          plain.printing_id as string,
          foil.printing_id as string,
        ]),
      ).toEqual([0, 0]);
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

// ------------------------- the grain refusal became a pair of picker rows ---
//
// P6-150's headline case, and the one refusal P6-151 had to leave standing: a
// stack holding several finish/condition/language grains and no default one —
// 2 foil + 1 etched — was `SkipReason::Grain`, a toast telling the user their
// row was un-moveable, because the step's rows were `(collection, printing,
// board)` and could not tell those copies apart. The rows are full grain now,
// so the same row is simply **two rows with two steppers**, and the answer is
// asked for instead of refused.
//
// The assertion that matters is the API read at the end: the foils moved and
// the etched copy did not. A picker that moved "two copies of that card" would
// satisfy every toast here and fail that.

test("@fast a stack of several grains opens the picker as one row each, and moves the grain picked", async ({
  page,
  request,
}) => {
  test.slow();
  const [card] = await unownedCards(request, 1, 45);
  const printing = card.printing_id as string;
  const source = await createCollection(request, "binder", "grainsplit");
  const dest = await createCollection(request, "binder", "graindest2");
  try {
    await addHave(request, source.id, printing, 2, { finish: "foil" });
    await addHave(request, source.id, printing, 1, { finish: "etched" });

    await page.goto(`/my/collections/${source.id}`);
    await hydrated(page);
    const row = collectionRow(page, printing);
    // One row, summing both grains — which is exactly why the page cannot ask
    // this question and the picker must. It does not even carry a stepper: a
    // HERE cell backed by more than one `holdings` row renders as plain text
    // (`collection.rs`), because there is no single row for it to commit to.
    await expect(row.locator('[data-testid="here-cell"]')).toHaveText("3");
    await expect(row.locator(HERE_VALUE)).toHaveCount(0);
    await select(row).click();

    await moveTo(page, dest.name);

    // Two rows, and each says which copies it is: the label is the only thing
    // standing between the user and two indistinguishable steppers.
    const rows = page.locator(STEP_ROW);
    await expect(rows).toHaveCount(2);
    const foils = rows.filter({ hasText: "foil" });
    const etched = rows.filter({ hasText: "etched" });
    await expect(foils).toContainText(`${source.name} · foil · 2 copies`);
    await expect(etched).toContainText(`${source.name} · etched · 1 copy`);
    // Both open at one copy — the default that makes the common single-stack
    // case one press of the button.
    await expect(foils.locator(PICK_VALUE)).toHaveText("1");
    await expect(etched.locator(PICK_VALUE)).toHaveText("1");

    // Take both foils and leave the etched copy where it is.
    await setQuantity(foils, 2);
    await setQuantity(etched, 0);
    const confirm = page.locator(STEP_CONFIRM);
    await expect(confirm).toHaveText("Move 2 copies");
    await confirm.click();

    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 2 copies of 1 card → 🗂 ${dest.name}`,
    );
    await expect(page.locator(TRAY)).toHaveCount(0);

    // What actually moved, by grain — the whole feature in one read.
    await expect(async () => {
      const res = await request.get(`/api/cards/${card.oracle_id}/holdings`);
      const holdings = (await res.json()) as {
        collection_id: string;
        finish: string;
        quantity: number;
      }[];
      expect(
        holdings
          .filter((h) => h.collection_id === source.id || h.collection_id === dest.id)
          .map(
            (h) =>
              `${h.collection_id === dest.id ? "dest" : "src"}/${h.finish} x${h.quantity}`,
          )
          .sort(),
      ).toEqual(["dest/foil x2", "src/etched x1"]);
    }).toPass({ timeout: 5000 });
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
    await addHave(request, deck.id, card.printing_id as string, 1, {
      board: "side",
    });

    // `/my` aggregates across boards, so the row reads OWNED 1 and offers a
    // checkbox — the board is invisible here. It used to be a refusal for that
    // reason; the ungrouped read supplies it to the write instead.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    await select(myRow(page, card.oracle_id)).click();

    await moveTo(page, dest.name);

    await expect(page.locator(TOAST).first()).toContainText("Moved 1 copy");
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
          .filter((h) => h.collection_id === deck.id || h.collection_id === dest.id)
          .map((h) => `${h.collection_id === deck.id ? "deck" : "dest"}/${h.board} x${h.quantity}`)
          .sort(),
      ).toEqual(["dest/main x1"]);
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
    // row per **grain** of every place the copies actually sit in, which is the
    // question the oracle-grained key could not answer on its own. Exact, and
    // derived from the database rather than from `places`: since P6-150 a place
    // holding two finishes is two rows, so the row count is the full-grain
    // stack count (`stackCount`), while `places` is still what the refusal
    // sentence below counts.
    await expect(page.locator(STEP_ROW)).toHaveCount(
      await stackCount(request, scattered.card.oracle_id, dest.id),
    );
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

test("@fast an ambiguous /my row asks which copies, and moves exactly the copies picked from each stack", async ({
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
    // each naming its collection and how many copies are in it.
    await expect(page.locator(STEP_CARD)).toHaveAttribute("data-card", card.name);
    const rows = page.locator(STEP_ROW);
    await expect(rows).toHaveCount(stacks);
    // Each row names its stack's own size; the stepper beside it says what is
    // being taken, and opens at one copy.
    const rowHere = rows.filter({ hasText: here.name });
    const rowThere = rows.filter({ hasText: there.name });
    await expect(rowHere).toContainText(`${here.name} · 2 copies`);
    await expect(rowThere).toContainText(`${there.name} · 3 copies`);
    await expect(rowHere.locator(PICK_VALUE)).toHaveText("1");
    // The destination is not offered as a place to take copies *from*.
    await expect(rows.filter({ hasText: dest.name })).toHaveCount(0);

    // Take *different numbers* from both stacks of the same card — the step's
    // headline flow, and the one the tray's own "N cards, 1 copy each" phrasing
    // could not describe: this is one card and three copies, and a toast saying
    // "3 cards" is a false statement about the user's collection.
    await onlyFrom(page, here.name);
    const confirm = page.locator(STEP_CONFIRM);
    await expect(confirm).toHaveText("Move 1 copy");
    await setQuantity(rowHere, 2);
    await setQuantity(rowThere, 1);
    await expect(confirm).toHaveText("Move 3 copies");
    await confirm.click();

    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 3 copies of 1 card → 🗂 ${dest.name}`,
    );
    // The question was answered, so the tray stops asking it — the entry the
    // step was opened for is a `card:` token, and the move that answered it
    // reported grain-suffixed `held:` ones.
    await expect(page.locator(TRAY)).toHaveCount(0);

    // The assertion the whole feature stands on: each stack gave up exactly
    // what its own stepper said — the whole of one, one copy of the other —
    // and the destination got the sum.
    await expect(async () => {
      expect(await present(request, here.id, [printing])).toEqual([0]);
      expect(await present(request, there.id, [printing])).toEqual([2]);
      expect(await present(request, dest.id, [printing])).toEqual([3]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, here.id);
    await deleteCollection(request, there.id);
    await deleteCollection(request, dest.id);
  }
});

// ------------------- the picker never claims the copies are gone (P6-150) ---
//
// "This card has no copies left" and "the read has not come back yet" look
// alike and mean opposite things, and the dialog resolves its rows in an
// `Effect` rather than awaiting them — so a payload belonging to a *previous*
// question (leptos hands the last resolved value back during a refetch, and the
// first one this resource ever resolves is empty) rendered the
// nothing-left-to-move sentence for the whole selection until the real read
// landed, with the confirm disabled.
//
// Auto-retrying assertions cannot catch that: they poll until it self-corrects.
// So the read is **stalled deliberately** and the in-flight state is asserted
// while it cannot resolve — the only way this is a test rather than a race.

test("@fast while its read is in flight the picker says so, and never that there is nothing to move", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 55);
  const printing = card.printing_id as string;
  const source = await createCollection(request, "binder", "flightsrc");
  const dest = await createCollection(request, "binder", "flightdest");
  try {
    await addHave(request, source.id, printing, 3);

    await page.goto(`/my/collections/${source.id}`);
    await hydrated(page);
    await select(collectionRow(page, printing)).click();

    let release = () => {};
    const stalled = new Promise<void>((resolve) => (release = resolve));
    await page.route("**/api/selection_stacks", async (route) => {
      await stalled;
      await route.continue();
    });

    await moveTo(page, dest.name);

    // The dialog is open (the move refused, three copies being a question),
    // and its row list is honest about not knowing yet.
    const panel = page.locator(PANEL);
    await expect(panel).toHaveAttribute("data-state", "loading");
    await expect(panel).toContainText("Finding your copies…");
    // The assertion this test exists for — and it holds *while the read cannot
    // return*, not merely eventually.
    await expect(panel).not.toContainText("No copies left to move");
    await expect(page.locator(STEP_CARD)).toHaveCount(0);
    await expect(page.locator(STEP_CONFIRM)).toBeDisabled();

    release();

    await expect(panel).toHaveAttribute("data-state", "ready");
    await expect(page.locator(STEP_ROW)).toHaveCount(1);
    await expect(page.locator(STEP_CONFIRM)).toHaveText("Move 1 copy");
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// ------------------- one card, selected twice, is one question (P6-150) ---
//
// The ruling's question 3. `/my` and a collection page are two views of one
// shelf, so selecting a card on each makes two tray entries over the *same
// copies*. Two sections would apportion one pile across two identical lists,
// and — since both address the same `(collection, printing, board)` — would
// submit two wire items the server cannot tell apart in its own outcome.
//
// The second navigation is a **click**, not a `goto`: the tray is in-memory by
// design, so a document load starts it empty and the duplicate could not exist.

test("@fast the same card selected on /my and in its collection is one question, and answering it clears both", async ({
  page,
  request,
}) => {
  test.slow();
  const [card] = await unownedCards(request, 1, 60);
  const printing = card.printing_id as string;
  const source = await createCollection(request, "binder", "dupsrc");
  const dest = await createCollection(request, "binder", "dupdest");
  try {
    await addHave(request, source.id, printing, 2);

    // The `/my` row first — an oracle-grained entry naming no place…
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);
    await select(myRow(page, card.oracle_id)).click();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    // …then into the collection through the sidebar (a real SPA navigation, so
    // the selection rides along) and the row for the very same copies.
    await page
      .getByRole("navigation", { name: "Collections" })
      .getByRole("link", { name: new RegExp(source.name) })
      .click();
    await page.waitForURL((url) => url.pathname === `/my/collections/${source.id}`);
    await expect(page.locator(COUNT)).toHaveText("1 card");
    await select(collectionRow(page, printing)).click();
    await expect(page.locator(COUNT), "two entries, one shelf").toHaveText("2 cards");

    await moveTo(page, dest.name);

    // One section, one row — not two lists over the same pile.
    await expect(page.locator(STEP_CARD)).toHaveCount(1);
    const rows = page.locator(STEP_ROW);
    await expect(rows).toHaveCount(1);
    await expect(rows).toContainText(`${source.name} · 2 copies`);
    const confirm = page.locator(STEP_CONFIRM);
    await expect(confirm).toHaveText("Move 1 copy");
    await confirm.click();

    // One card, one copy — the count the un-merged version got wrong twice
    // over (two sections defaulting to one copy each, reported as two cards).
    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 1 copy of 1 card → 🗂 ${dest.name}`,
    );
    // Both tray entries retire, so the pill goes away: leaving the duplicate
    // checked would have the tray still asking a question just answered.
    await expect(page.locator(TRAY)).toHaveCount(0);

    await expect(async () => {
      expect(await present(request, source.id, [printing])).toEqual([1]);
      expect(await present(request, dest.id, [printing])).toEqual([1]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, source.id);
    await deleteCollection(request, dest.id);
  }
});

// --------- two rows of one card are two entries, however they are shown ---
//
// The other half of the collapse, and the case that makes it a *display* merge
// rather than an identity one: a deck's mainboard and sideboard rows are one
// card and two tray entries over copies that are not each other's. They are one
// section (one card, one list to choose from), but each row still answers only
// for the entry whose copies it is — so taking the sideboard copy must leave the
// mainboard entry checked *and* say why it did not move. Retiring it silently
// is the exact failure the whole reporting path exists to prevent.

test("@fast two board rows of one card are one section, and answering one leaves the other checked and named", async ({
  page,
  request,
}) => {
  test.slow();
  const [card] = await unownedCards(request, 1, 65);
  const printing = card.printing_id as string;
  const deck = await createCollection(request, "deck", "boardsrc");
  const dest = await createCollection(request, "binder", "boarddest");
  try {
    await addHave(request, deck.id, printing, 2);
    await addHave(request, deck.id, printing, 2, { board: "side" });

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    const boardRow = (board: string) =>
      page.locator(
        `[data-testid="collection-row"][data-printing="${printing}"][data-board="${board}"]`,
      );
    await select(boardRow("main")).click();
    await select(boardRow("side")).click();
    await expect(page.locator(COUNT)).toHaveText("2 cards");

    await moveTo(page, dest.name);

    // One card, one section — and both stacks offered, because two entries
    // asked about them.
    await expect(page.locator(STEP_CARD)).toHaveCount(1);
    const rows = page.locator(STEP_ROW);
    await expect(rows).toHaveCount(2);
    const sideRow = rows.filter({ hasText: "sideboard" });
    const mainRow = rows.filter({ hasNotText: "sideboard" });
    await expect(sideRow).toHaveCount(1);
    await expect(mainRow).toHaveCount(1);

    // Take the sideboard copy, leave the mainboard stack alone.
    await setQuantity(mainRow, 0);
    await expect(page.locator(STEP_CONFIRM)).toHaveText("Move 1 copy");
    await page.locator(STEP_CONFIRM).click();

    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 1 copy of 1 card → 🗂 ${dest.name}`,
    );
    // The mainboard entry moved nothing — so it is still checked, and it is
    // told why rather than disappearing.
    await expect(page.locator(COUNT)).toHaveText("1 card");
    await expect(page.locator(TOAST, { hasText: "wasn't moved" })).toContainText(
      `${card.name} has 2 copies`,
    );

    // And the database agrees about which copies left.
    await expect(async () => {
      const res = await request.get(`/api/cards/${card.oracle_id}/holdings`);
      const held = (await res.json()) as {
        collection_id: string;
        board: string;
        quantity: number;
      }[];
      expect(
        held
          .filter((h) => h.collection_id === deck.id || h.collection_id === dest.id)
          .map((h) => `${h.collection_id === deck.id ? "deck" : "dest"}/${h.board} x${h.quantity}`)
          .sort(),
      ).toEqual(["deck/main x2", "deck/side x1", "dest/main x1"]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, dest.id);
  }
});

// ------------------------------------------------- the picker on a phone ---
//
// P6-150 is a mobile-relevant story: the tray docks above the bottom tab bar,
// so the dialog it opens is the one surface where a quantity gets chosen on a
// phone. Two things have to hold there and nowhere else can prove them — the
// panel fits a narrow viewport (a stepper pushed off the right edge is not a
// control), and the ± buttons are real 44 px targets rather than the dense
// desktop size. This is why the picker does not reuse `CountStepper`, whose own
// ± are `hidden sm:inline-flex`.

test.describe("mobile", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("@fast the quantity picker fits a phone, with tappable steppers", async ({
    page,
    request,
  }) => {
    const [card] = await unownedCards(request, 1, 50);
    const source = await createCollection(request, "binder", "phonesrc");
    const dest = await createCollection(request, "binder", "phonedest");
    try {
      await addHave(request, source.id, card.printing_id as string, 3);

      await page.goto(`/my/collections/${source.id}`);
      await hydrated(page);
      await select(collectionRow(page, card.printing_id as string)).click();
      await moveTo(page, dest.name);

      const row = page.locator(STEP_ROW);
      await expect(row).toHaveCount(1);
      // The buttons are the whole control here, so they are the touch target.
      for (const control of [PICK_DEC, PICK_INC]) {
        const box = await row.locator(control).boundingBox();
        expect(box?.height ?? 0, `${control} height`).toBeGreaterThanOrEqual(44);
        expect(box?.width ?? 0, `${control} width`).toBeGreaterThanOrEqual(44);
      }
      // …and the panel stays inside the viewport: nothing to scroll sideways
      // to reach, on the page or in the dialog's own scroller.
      const overflow = await page.evaluate(() => ({
        page: document.documentElement.scrollWidth - window.innerWidth,
        panel: (() => {
          const p = document.querySelector('[data-testid="which-copies"]');
          return p ? p.scrollWidth - p.clientWidth : 0;
        })(),
      }));
      expect(overflow.page, "the page scrolls sideways").toBeLessThanOrEqual(0);
      expect(overflow.panel, "the row list scrolls sideways").toBeLessThanOrEqual(0);

      await setQuantity(row, 2);
      await page.locator(STEP_CONFIRM).click();
      await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
        `Moved 2 copies of 1 card → 🗂 ${dest.name}`,
      );
      await expect(async () => {
        expect(await present(request, dest.id, [card.printing_id as string])).toEqual([2]);
      }).toPass({ timeout: 5000 });
    } finally {
      await deleteCollection(request, source.id);
      await deleteCollection(request, dest.id);
    }
  });
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
    await expect(page.locator(TOAST, { hasText: /Moved \d+ cop/ })).toContainText(
      `Moved 2 copies of 2 cards → 🗂 ${dest.name}`,
    );
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
    const movedToast = page.locator(TOAST, { hasText: /Moved \d+ cop/ });
    await expect(async () => {
      expect((await stepRows.count()) > 0 || (await movedToast.count()) > 0).toBe(
        true,
      );
    }).toPass({ timeout: 5000 });
    if (await stepRows.count()) {
      // Every row opens at one copy, so the ones this test does not want are
      // stepped back to zero rather than left to move something it will then
      // assert against.
      await onlyFrom(page, source.name);
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
