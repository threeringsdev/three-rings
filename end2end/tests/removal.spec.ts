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
// `finally`. Delete no longer cascades (specs/collection-deletion.md, stale
// since P6-188): it soft-deletes the collection and relocates its holdings to
// the Inbox (the default `ToParent` disposition — these scratch collections
// are top-level) rather than destroying them. Printings come from catalog
// cards the fixture owns nowhere, so a count read back is this test's writes
// and nothing else.

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
  // `limit=200`, not 60: the shared `q=z` unowned-card pool this suite reuses
  // across files has been drained far enough by repeated runs that 60 no
  // longer clears the front (measured 0 free at 60, 51 free at 200 — the same
  // reason `collection-undo-restore.spec.ts` and
  // `collection-tree-manage.spec.ts` already made this switch).
  //
  // P6-118: `q=z` itself is now exhausted, not just under-limited — `z` only
  // ever matches 132 cards total in this POC catalog (a `limit` bump cannot
  // grow a query's own universe), and by the time this task ran, this file's
  // *other* tests alone had driven free-at-200 to 0. `q=n` was measured with
  // real headroom (152 free of 200) and is local to this file, so it does not
  // draw down the same shared pool `batch-move.spec.ts`,
  // `command-palette.spec.ts` and `needs.spec.ts` still lean on — those are
  // unchanged, same as P6-117 left them, still out of this task's scope.
  // The `q=n` switch resets the pool but not a sibling derivation flaw: `mine`
  // below is *also* capped at `limit=200`, so once this account holds more
  // than 200 cards, `taken` silently drops the rest and a genuinely-owned
  // card past the 200th would misread as free — needs a dedicated
  // fixture-hardening follow-up, not fixed here.
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

// -------------------------------------------- same-device undo-flow defects ---
//
// P6-117: removal deliberately does *not* dispose the row after it commits
// (that is what keeps the *removal's own* Undo toast reachable — see the
// module comment above `commitZero`'s callers). Two defects fall out of that
// choice, both reachable on one device with no timing trick beyond "act
// before a 5s toast auto-dismisses":
//
//   1. A count-change toast raised *before* the removal keeps its own Undo
//      button live after the row is gone, and firing it used to write the
//      reversed count straight to the holding `remove_holding` had just
//      deleted — a bogus "Couldn't save: not found: holding" toast.
//   2. Undo-of-removal optimistically restores the row before the server's
//      response lands, but the stepper it re-shows kept the *dead*
//      pre-removal holding id until an unrelated view refetch remounted the
//      row — a real window where a +/- failed the same way.

test("@fast a stale count-change toast's Undo does nothing once the row has been removed", async ({
  page,
  request,
}) => {
  // Two commit-and-toast cycles plus three DB read-backs (batch-move.spec.ts's
  // longest test carries the same note): fits the default 30s alone, tight
  // under a loaded shared dev branch — slow, not flaky.
  test.slow();
  const [card] = await unownedCards(request, 1, 10);
  const binder = await createCollection(request, "binder", "stale-toast");
  try {
    await addHave(request, binder.id, card.printing_id as string, 3);
    const where = { [binder.id]: "binder" };

    await page.goto(`/my/collections/${binder.id}`);
    await hydrated(page);
    const row = rowFor(page, card.printing_id as string);
    await expect(row.locator(STEPPER_VALUE)).toHaveText("3");

    // Change the count first. Its own commit toast carries an Undo that would
    // normally re-commit `3 → 1`'s reversal — and it is still on screen (its
    // 5s auto-dismiss has not fired) when the row is removed a moment later.
    await row.locator(STEPPER_VALUE).click();
    await row.locator(STEPPER_INPUT).fill("1");
    await row.locator(STEPPER_INPUT).press("Enter");
    const staleToast = page.locator(TOAST, { hasText: `${card.name}: 3 → 1` });
    await expect(staleToast).toBeVisible();

    // Remove the row outright. This raises a *second*, later toast — the
    // stale one from the count change is still sitting underneath it, because
    // removal deliberately does not dispose the row (that would take the
    // removal's own Undo down with it).
    await commitZero(row);
    const removalToast = page.locator(TOAST, { hasText: "Removed" });
    await expect(removalToast).toContainText(`Removed ${card.name} (1 copy)`);

    // Fire the *stale* toast's Undo promptly — no DB round-trip in between.
    // Its 5s auto-dismiss started when it first appeared (well before this
    // point), not when the row was removed, so stacking a `toPass` poll here
    // risks it dismissing before this click lands on a loaded shared dev
    // branch. The read-backs move after the click instead.
    //
    // Watched from here, not from the top of the test: the *legitimate*
    // `3 → 1` edit above also calls `set_holding_quantity`, and the point is
    // to catch only a write this click itself provokes.
    const badWrites: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("/api/set_holding_quantity")) badWrites.push(r.url());
    });
    await staleToast.getByRole("button", { name: "Undo" }).click();

    // Bounded wait, not an instant check: the fix returns before firing
    // anything, so there is no request to await — a wrongly-issued write
    // needs real time to round-trip and render its error toast, and a single
    // check right after the click would pass vacuously whether or not the
    // guard actually ran.
    await page.waitForTimeout(1000);
    expect(
      badWrites,
      "the stale toast's Undo must not write to the dead holding",
    ).toEqual([]);
    await expect(
      page.locator(TOAST, { hasText: "Couldn't save" }),
    ).toHaveCount(0);
    // The DB read-back that used to sit before the click: it still proves
    // both that the removal actually landed *and* that the stale Undo put
    // nothing back, in one read, now that the click is no longer waiting on
    // it.
    expect(await grainsIn(request, card.oracle_id, where)).toEqual([]);

    // The regression this fix must not cause: the *removal's* own Undo is
    // still fully live and still works.
    await removalToast.getByRole("button", { name: "Undo" }).click();
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "binder: nonfoil/nm/en/main x1",
      ]);
    }).toPass({ timeout: 5000 });
  } finally {
    await deleteCollection(request, binder.id);
  }
});

// Defect 1 (the stale-id race) is deliberately **not** given an e2e test.
// `undo_removal` rewires the row's captured `holding_id` straight from the
// server's `UndoReceipt` (collection.rs), so a real race would need a +/-
// landing in the gap between that rewire (synchronous, in the same success
// branch) and nothing — the window it used to depend on (an unrelated
// `collection_view` refetch) is no longer load-bearing for correctness at
// all. The one way to *construct* the old window deterministically —
// `page.route` holding `collection_view` open across the Undo click — was
// tried and dropped: it reproduces a genuine but **pre-existing and
// unrelated** wasm panic in `count_stepper.rs`'s blur-commit machinery
// (`container_ref`, defined at count_stepper.rs:99, accessed after disposal
// at count_stepper.rs:416, inside the deferred `set_timeout` a stray
// `focusout` schedules), confirmed to reproduce identically against
// unmodified `main` — i.e. it is `commitZero`'s own Enter-key commit path
// racing a fresh remount, not anything this task touches. Filed as a
// follow-up rather than fixed here (see this file's — and app-ui.md's —
// Findings). Defect 1's fix is covered by the existing "the page followed
// the database rather than waiting for a reload" assertions above (the
// *eventual* consistency of the rewired id) and by the unit-level type
// threading in `shared::UndoReceipt` / `hosted::undo_one`.

// --------------------------- P6-118: section header + selection track it ---
//
// Two more defects `here_delta` alone did not cover. `section_slots` (a deck
// section header's own count) summed the *static* `row.present` at payload
// load, so it kept the removed copies until an unrelated refetch — a number
// contradicting both the row's own "—" and the page header on the same
// screen. And `selectable` was computed once from pre-removal data, so a
// removed row's checkbox stayed interactive and ticking it earned a
// `NoCopies` refusal from the tray ("has no copies left to move — reload the
// page"). Both are now reactive on the same `removed` signal `HereCount`
// already flips for its own "—" fallback, so this test is deterministic — no
// `toPass()` polling for either assertion, because a refetch is not what
// fixes them.
//
// The checkbox assertion is on *visibility*, not DOM presence: the fix is a
// `style:display` toggle (`contents` ⇄ `none`) on a wrapper around an
// always-mounted `SelectionCheckbox`, not a mount/unmount — see
// `CardTableRow`'s doc for why a second structural mount/unmount was
// rejected (it landed in the same reactive flush as `HereCount`'s own
// `<Show>`, which made the pre-existing `count_stepper.rs` disposal race
// this file's Findings already documents (P6-117) fire far more often in
// practice). `toBeHidden()`/`toBeVisible()` correctly see through
// `display:none` on an ancestor, unlike the `Sheet` transform trap the
// e2e-suite skill warns about.

test("@fast removing a deck row drops the section count and its own checkbox immediately, and Undo restores both", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 11);
  const deck = await createCollection(request, "deck", "section");
  try {
    await addHave(request, deck.id, card.printing_id as string, 2);

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    const row = rowFor(page, card.printing_id as string);
    await expect(row.locator(STEPPER_VALUE)).toHaveText("2");

    // One card in a fresh scratch deck is one section, so its header is
    // unambiguously this row's own count.
    const section = page.locator('[data-testid="deck-section"]');
    await expect(section).toHaveCount(1);
    await expect(section).toHaveText(/· 2$/);

    const checkbox = row.locator('[data-testid="row-select"]');
    await expect(checkbox).toBeVisible();
    // Select before removing: an in-flight selection must survive the
    // removal rather than vanish silently. The tray already has a name and a
    // toast for a selection that outlived its copies — `SkipReason::NoCopies`
    // in `move_selection.rs` names "the stepper" explicitly as one of the
    // causes — so this is that mechanism being reached, not new UX.
    await checkbox.click();
    const trayCount = page.locator('[data-testid="tray-count"]');
    await expect(trayCount).toHaveText("1 card");

    await commitZero(row);

    // The section header agrees the moment the row does — no refetch, no
    // reload, and `slots` reactively summed here would have caught nothing
    // since the delta is what moves it.
    await expect(section).toHaveText(/· 0$/);
    // The checkbox withdraws with the stepper: ticking a removed row only
    // ever earned a refusal. `toBeHidden()` alone would pass vacuously if the
    // node detached instead of just going invisible (a style toggle is the
    // load-bearing claim here, not a mount/unmount — see `CardTableRow`'s
    // doc), so pin the node is still there too.
    await expect(checkbox).toBeHidden();
    await expect(checkbox).toHaveCount(1);
    // The pre-existing selection is left alone, not silently dropped.
    await expect(trayCount).toHaveText("1 card");

    const toast = page.locator(TOAST, { hasText: "Removed" });
    await toast.getByRole("button", { name: "Undo" }).click();

    // Undo restores both together — the same `removed` signal that hid the
    // checkbox is what the count stepper's own restore flips back.
    await expect(section).toHaveText(/· 2$/);
    await expect(checkbox).toBeVisible();
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

// P6-031: the teardown's own success toast gains an Undo, reusing ⌘K's
// `undo_selection_move` reversal (`app/src/my/collection.rs`'s
// `undo_teardown`). Two things pin the whole point of the task, not just that
// clicking the button writes something: the deck's contents actually come
// back (read from the database, not the toast), and the palette's own
// `Undo last move` — which used to be the *only* way to reverse a teardown —
// must stop offering to reverse this one a second time, per `forget_last_move`.
test("@fast Empty deck's own toast Undo restores the deck, and ⌘K stops offering the reversal", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 12);
  const deck = await createCollection(request, "deck", "tdtoastundo");
  const dest = await createCollection(request, "binder", "tdtoastdest");
  try {
    await addHave(request, deck.id, card.printing_id as string, 2);
    const where = { [deck.id]: "deck", [dest.id]: "dest" };

    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    await page.locator('[data-testid="teardown-open"]').click();
    await page
      .locator('[data-testid="teardown-destination"]')
      .selectOption({ label: dest.name });
    await page.locator('[data-testid="teardown-confirm"]').click();

    const toast = page.locator(TOAST, { hasText: "Emptied" });
    await expect(toast).toContainText("1 card moved");
    // A single unretried read, not a `toPass` poll: the server write already
    // committed before the toast rendered (`teardown_collection` is awaited
    // before `toast.show` runs), so there is nothing to wait out here — and
    // this toast auto-dismisses at 5000ms, the same window a `toPass({
    // timeout: 5000 })` could itself burn through before Undo is ever
    // clicked (P6-119's Findings: the same shape of race, elsewhere). Click
    // promptly after the toast's own text confirms the teardown; every other
    // read-back happens after.
    expect(await grainsIn(request, card.oracle_id, where)).toEqual([
      "dest: nonfoil/nm/en/main x2",
    ]);

    // The toast the phone has no other way to reach it from — the button must
    // actually be there, and clicking it is the whole verification.
    await toast.getByRole("button", { name: "Undo" }).click();

    // Read back, not off the toast: the deck's copies are actually restored.
    await expect(async () => {
      expect(await grainsIn(request, card.oracle_id, where)).toEqual([
        "deck: nonfoil/nm/en/main x2",
      ]);
    }).toPass({ timeout: 5000 });
    expect(await viewRows(request, deck.id)).toHaveLength(1);
    expect(await viewRows(request, dest.id)).toHaveLength(0);

    // ⌘K must not offer to reverse an already-reversed teardown — the same
    // "Nothing to undo yet" arm `command-palette.spec.ts` pins for a session
    // with no recorded move at all.
    await page.keyboard.press("ControlOrMeta+k");
    await expect(page.locator("#command-palette[role=dialog]")).toHaveAttribute(
      "data-state",
      "open",
    );
    await page.keyboard.type("undo");
    await expect(
      page.locator("#command-palette [data-name=CommandItem]").first(),
    ).toContainText("Undo last move");
    await page.keyboard.press("Enter");
    await expect(page.getByText(/Nothing to undo yet/)).toBeVisible();
    await expect(page.getByText("Undid the last move")).toHaveCount(0);
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, dest.id);
  }
});
