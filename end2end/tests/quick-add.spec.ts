// The quick-add panel — the keyboard-first intake surface on
// `/my/collections/:id` (specs/app-ui.md → "Quick-add panel";
// design/wireframes.pen → the `Proto — Add flow` storyboards).
//
// The contract, in assertion order:
//
// - the panel is client-only: absent from the SSR'd HTML, absent until the field
//   is focused, and it is the *content* that is asserted, never a visibility flag
//   (a hidden container proves nothing about lazy mounting);
// - the keystroke ledger in the footer, and its Want-led flip in a deck;
// - `↑↓` walks the catalog candidates and moves the `⏎` chip with the highlight;
// - `⏎` adds one copy *to this collection* and the toast undoes it — asserted
//   against the collection read, so it proves the database moved rather than that
//   a toast said so;
// - `⌥⏎` commits the *other* kind (Want in a binder, Have in a deck);
// - `⇧⏎` + digits + `⏎` adds that many copies, and the digits never reach the
//   search box;
// - `Escape` abandons a pending count first and the panel second;
// - the loop point: after an add the field is empty and still focused, so the
//   next card starts with no extra keystroke;
// - `IN THIS COLLECTION` rows are context, not add targets: they carry the HERE
//   count and are not part of the `↑↓` walk;
// - `IN THIS COLLECTION` rows wait for a query — none at rest, some once the
//   query matches, none again once an add clears the field back to rest.
//
// **Isolation.** Everything that writes does so inside a `zz-e2e-…` binder or
// deck it creates via the API and deletes in a `finally` — the convention
// `collection-view.spec.ts` established. Delete cascades holdings and desires, so
// a scratch collection leaves nothing behind and the seeded tree that other specs
// assert on is never touched. That matters more here than elsewhere: a Want has
// no undo operation (specs/app-ui.md Findings), so the only way to keep `+ Want`
// from accumulating desire rows on the shared fixture is to add it somewhere
// disposable.
//
// "lightning" is a stable POC-catalog probe — several cards match it, which is
// what makes the `↑↓` disambiguation walk assertable at all.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const quick = expect.configure({ timeout: 2000 });

type Summary = { id: string; name: string };
type Row = {
  oracle_id: string;
  printing_id: string;
  name: string;
  present: number;
  desired: number;
  present_rollup: number;
};
type View = { cards: Row[] };

let scratchSeq = 0;
/// Worker index plus a per-file counter — no wall clock, so parallel workers
/// cannot collide.
const scratchName = () =>
  `zz-e2e-quickadd-w${test.info().workerIndex}-${++scratchSeq}`;

async function createCollection(
  request: APIRequestContext,
  kind: "binder" | "deck",
  format: string | null = null,
): Promise<string> {
  const name = scratchName();
  const res = await request.post("/api/collections", {
    data: { parent_id: null, kind, name, format },
  });
  expect(res.status(), `create ${name}`).toBe(200);
  return ((await res.json()) as Summary).id;
}

const deleteCollection = (request: APIRequestContext, id: string) =>
  request.post(`/api/collections/${id}/delete`, { data: {} });

async function viewOf(request: APIRequestContext, id: string): Promise<View> {
  const res = await request.get(`/api/collections/${id}/view?limit=200`);
  expect(res.status(), `view ${id}`).toBe(200);
  return (await res.json()) as View;
}

/// The row for `name`, or `undefined` — used to assert a card arrived *and* to
/// assert it left again after Undo.
async function rowNamed(
  request: APIRequestContext,
  id: string,
  name: string,
): Promise<Row | undefined> {
  const view = await viewOf(request, id);
  return view.cards.find((c) => c.name === name);
}

/// Open the panel the way a user does — focus the field, then type. Typing is
/// what the panel is for, so nothing here uses `fill`: the debounce, the
/// committed-`?q=` search and the highlight all key off real keystrokes.
async function openPanel(page: Page, id: string, query: string) {
  await page.goto(`/my/collections/${id}`);
  await hydrated(page);
  const box = page.locator("#collection-query");
  await box.click();
  await expect(page.getByTestId("quick-add-panel")).toBeAttached();
  await box.pressSequentially(query, { delay: 30 });
  // The candidates are fetched for the *committed* query, so the panel fills in
  // one debounce plus one round trip.
  await expect(
    page.getByTestId("quick-add-candidate").first(),
  ).toBeAttached({ timeout: 15000 });
  return box;
}

/// Open the panel and leave the field untouched — "at rest", no `?q=` at all.
/// Distinct from [`openPanel`] (which always types a query) so a test can
/// assert what the panel shows *before* the first keystroke.
async function openPanelAtRest(page: Page, id: string) {
  await page.goto(`/my/collections/${id}`);
  await hydrated(page);
  const box = page.locator("#collection-query");
  await box.click();
  await expect(page.getByTestId("quick-add-panel")).toBeAttached();
  return box;
}

const candidates = (page: Page) => page.getByTestId("quick-add-candidate");
const highlighted = (page: Page) =>
  page.locator('[data-testid="quick-add-candidate"][data-highlighted="true"]');
const chip = (page: Page) => page.getByTestId("quick-add-chip");
const toastFor = (page: Page, name: string) =>
  page.locator("[data-name=Toast]").filter({ hasText: name });

/// The name of the row `⏎` would add right now.
const highlightedName = async (page: Page) =>
  (await highlighted(page).textContent())?.trim() ?? "";

// -------------------------------------------------------------- structure ---

test("the panel is client-only and opens on focus @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    // Server-side there is no panel at all — not a hidden one. Asserting on the
    // markup, not on visibility, is what makes this fail if it ever starts
    // rendering server-side (where its resource read would disagree with
    // hydration on an `SsrMode::Async` route).
    const res = await page.request.get(`/my/collections/${binder}`);
    expect(res.status()).toBe(200);
    const html = await res.text();
    expect(html).toContain('id="collection-query"');
    expect(html).not.toContain("quick-add-panel");

    await page.goto(`/my/collections/${binder}`);
    await hydrated(page);
    await expect(page.getByTestId("quick-add-panel")).toHaveCount(0);
    await page.locator("#collection-query").click();
    await expect(page.getByTestId("quick-add-panel")).toHaveCount(1);
  } finally {
    await deleteCollection(request, binder);
  }
});

test("the footer states the keystroke ledger, and a deck flips it @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  const deck = await createCollection(request, "deck", "commander");
  try {
    // specs/app-ui.md: `↑↓ navigate · ⏎ add 1 here · ⇧⏎ set count · ⌥⏎ want
    // instead`, with the deck variant flipping only the default.
    await page.goto(`/my/collections/${binder}`);
    await hydrated(page);
    await page.locator("#collection-query").click();
    const footer = page.getByTestId("quick-add-footer");
    await expect(footer).toContainText("↑↓");
    await expect(footer).toContainText("navigate");
    await expect(footer).toContainText("add 1 here");
    await expect(footer).toContainText("set count");
    await expect(footer).toContainText("want instead");

    await page.goto(`/my/collections/${deck}`);
    await hydrated(page);
    await page.locator("#collection-query").click();
    // Want-led: ⏎ wants, ⌥⏎ has. The binder text must be *gone*, not merely
    // joined by the deck text.
    await expect(footer).toContainText("want 1");
    await expect(footer).toContainText("have instead");
    await expect(footer).not.toContainText("add 1 here");
    await expect(footer).not.toContainText("want instead");
  } finally {
    await deleteCollection(request, binder);
    await deleteCollection(request, deck);
  }
});

// ------------------------------------------------------------ keystrokes ---

test("↑↓ walks the candidates and carries the ⏎ chip @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    const box = await openPanel(page, binder, "lightning");
    const rows = candidates(page);
    const count = await rows.count();
    test.skip(count < 2, "needs two catalog matches to have anything to walk");

    // Exactly one row is highlighted, and it is the first — "best catalog match
    // pre-selected" is what makes the steady-state cost ~5–7 keystrokes.
    await expect(highlighted(page)).toHaveCount(1);
    const first = await rows.nth(0).textContent();
    expect(await highlightedName(page)).toBe(first?.trim());
    // The chip rides the highlight, so it is on that row and nowhere else.
    await expect(chip(page)).toHaveCount(1);

    await box.press("ArrowDown");
    const second = await rows.nth(1).textContent();
    await quick.poll(() => highlightedName(page)).toBe(second?.trim());
    await expect(chip(page)).toHaveCount(1);

    await box.press("ArrowUp");
    await quick.poll(() => highlightedName(page)).toBe(first?.trim());

    // ↑ at the top clamps rather than wrapping — a wrap would send a fast typist
    // to the bottom of the list mid-disambiguation.
    await box.press("ArrowUp");
    await quick.poll(() => highlightedName(page)).toBe(first?.trim());
  } finally {
    await deleteCollection(request, binder);
  }
});

test("⏎ adds one copy here, and the toast undoes it @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    const box = await openPanel(page, binder, "lightning");
    const name = await highlightedName(page);
    expect(name, "something must be highlighted to add").toBeTruthy();
    expect(await rowNamed(request, binder, name)).toBeUndefined();

    const add = page.waitForResponse(
      (r) => r.url().includes("/api/quick_add") && r.status() === 200,
    );
    await box.press("Enter");
    await add;

    // The database moved — one copy, in *this* collection. Reading the
    // collection back is the half a toast assertion cannot give you.
    await expect
      .poll(async () => (await rowNamed(request, binder, name))?.present ?? 0)
      .toBe(1);

    // The toast names the count, the card and where it went: with ⇧⏎ able to add
    // a playset, "Added <card>" would not say whether the digits landed.
    const toast = toastFor(page, name);
    await expect(toast).toContainText("Added 1");

    // The loop point (storyboard M2): field empty and still focused, so the next
    // card costs only its own characters.
    await expect(box).toHaveValue("");
    await expect(box).toBeFocused();

    const undo = page.waitForResponse(
      (r) => r.url().includes("/api/undo_quick_add") && r.status() === 200,
    );
    await toast.getByRole("button", { name: "Undo" }).click();
    await undo;
    await expect
      .poll(async () => (await rowNamed(request, binder, name))?.present ?? 0)
      .toBe(0);
  } finally {
    await deleteCollection(request, binder);
  }
});

test("⌥⏎ commits the other kind — Want in a binder @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    const box = await openPanel(page, binder, "lightning");
    const name = await highlightedName(page);

    const add = page.waitForResponse(
      (r) => r.url().includes("/api/quick_add") && r.status() === 200,
    );
    await box.press("Alt+Enter");
    await add;

    // A Want, not a Have: `desired` moves and `present` does not. Asserting both
    // is the point — the flip is only correct if it changed *which* kind ran.
    await expect
      .poll(async () => {
        const row = await rowNamed(request, binder, name);
        return row ? `${row.present}/${row.desired}` : "missing";
      })
      .toBe("0/1");
    // Deliberately no Undo for a Want: desires are outside the move ledger and
    // there is no compensating operation (specs/app-ui.md Findings).
    const toast = toastFor(page, name);
    await expect(toast).toContainText("Wanted 1");
    await expect(toast.getByRole("button", { name: "Undo" })).toHaveCount(0);
  } finally {
    await deleteCollection(request, binder);
  }
});

test("⇧⏎ then digits then ⏎ adds that many copies @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    const box = await openPanel(page, binder, "lightning");
    const name = await highlightedName(page);
    const typed = await box.inputValue();

    await box.press("Shift+Enter");
    // Count entry shows on the highlighted row, empty and waiting for digits.
    await expect(page.getByTestId("quick-add-count")).toContainText("__");
    await box.press("4");
    await expect(page.getByTestId("quick-add-count")).toContainText("4");
    // The digit belongs to the count, not to the search: if it reached the box
    // the query would change and the candidate under the highlight with it.
    await expect(box).toHaveValue(typed);

    const add = page.waitForResponse(
      (r) => r.url().includes("/api/quick_add") && r.status() === 200,
    );
    await box.press("Enter");
    await add;

    // A playset in one detour — this is the whole reason `quick_add` takes a
    // quantity instead of the surface firing four adds (undo targets one move).
    await expect
      .poll(async () => (await rowNamed(request, binder, name))?.present ?? 0)
      .toBe(4);
    await expect(toastFor(page, name)).toContainText("Added 4");
  } finally {
    await deleteCollection(request, binder);
  }
});

test("Escape abandons the count first, then the panel @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    const box = await openPanel(page, binder, "lightning");
    await box.press("Shift+Enter");
    await expect(page.getByTestId("quick-add-count")).toBeAttached();

    // One Escape per thing to abandon, so a mistyped playset does not also cost
    // you the panel and the query.
    await box.press("Escape");
    await expect(page.getByTestId("quick-add-count")).toHaveCount(0);
    await expect(page.getByTestId("quick-add-panel")).toHaveCount(1);

    await box.press("Escape");
    await expect(page.getByTestId("quick-add-panel")).toHaveCount(0);
  } finally {
    await deleteCollection(request, binder);
  }
});

// -------------------------------------------------- in this collection ---

test("IN THIS COLLECTION rows carry HERE and are not ↑↓ targets @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    // Seed two copies of one card, then search for it: the panel must say "you
    // already have 2 here" *and* still pre-highlight a catalog candidate, which
    // is the storyboard's S2 (present row unhighlighted, best match selected).
    const trade = await page.request.get("/api/collections/tree");
    expect(trade.status()).toBe(200);
    const rows = (
      (await trade.json()) as { collections: { summary: { id: string; name: string } }[] }
    ).collections;
    const source = rows.find((r) => r.summary.name === "Trade Binder")!;
    const seed = (await viewOf(request, source.summary.id)).cards[0];
    const have = await request.post(`/api/collections/${binder}/have`, {
      data: { printing_id: seed.printing_id, quantity: 2 },
    });
    expect(have.status()).toBe(200);

    // Search by a prefix of the seeded card's name so both sections populate.
    const probe = seed.name.slice(0, 6);
    await openPanel(page, binder, probe);

    const present = page.getByTestId("quick-add-present");
    await expect(present).toHaveCount(1);
    await expect(
      present.getByTestId("quick-add-present-count"),
    ).toHaveText("2");
    // A present row is context, not a target: the highlight (and so ⏎) is on a
    // catalog candidate, never on this row.
    await expect(present).not.toHaveAttribute("data-highlighted", "true");
    await expect(highlighted(page)).toHaveCount(1);
    await expect(candidates(page).first()).toBeAttached();
  } finally {
    await deleteCollection(request, binder);
  }
});

test("present rows wait for a query — none at rest, some on a match, none again once an add clears the field @fast", async ({
  page,
  request,
}) => {
  const binder = await createCollection(request, "binder");
  try {
    // Seed two copies of a real card into the scratch binder, the same pattern
    // the test above uses — a non-empty collection is exactly the shape that
    // used to leak its whole first page into an at-rest panel (P6-147).
    const trade = await page.request.get("/api/collections/tree");
    expect(trade.status()).toBe(200);
    const rows = (
      (await trade.json()) as { collections: { summary: { id: string; name: string } }[] }
    ).collections;
    const source = rows.find((r) => r.summary.name === "Trade Binder")!;
    const seed = (await viewOf(request, source.summary.id)).cards[0];
    const have = await request.post(`/api/collections/${binder}/have`, {
      data: { printing_id: seed.printing_id, quantity: 2 },
    });
    expect(have.status()).toBe(200);

    const present = page.getByTestId("quick-add-present");

    // At rest — no `?q=` typed yet — the panel must show nothing here, even
    // though the binder already holds a card the empty-query read would return
    // as its unfiltered first page.
    const box = await openPanelAtRest(page, binder);
    await expect(present).toHaveCount(0);

    // Typing a query matching the seeded card brings the row back.
    const probe = seed.name.slice(0, 6);
    await box.pressSequentially(probe, { delay: 30 });
    await expect(present).toHaveCount(1, { timeout: 15000 });
    await expect(
      present.getByTestId("quick-add-present-count"),
    ).toHaveText("2");
    // A catalog candidate must also be mounted, or ⏎ below has nothing to add.
    await expect(candidates(page).first()).toBeAttached();

    // Completing an add clears the field (the loop point another test already
    // covers) — present rows must go with it, not linger from the retained
    // facts that P6-068 keeps flowing across the refetch.
    const add = page.waitForResponse(
      (r) => r.url().includes("/api/quick_add") && r.status() === 200,
    );
    await box.press("Enter");
    await add;
    await expect(box).toHaveValue("");
    await expect(present).toHaveCount(0);
  } finally {
    await deleteCollection(request, binder);
  }
});
