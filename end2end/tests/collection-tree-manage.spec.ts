import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, clickUntil, hydrated } from "./helpers";

// Collection-tree management (specs/app-ui.md "Collection tree", management
// half; design/information-architecture.md → "Tree management … happens in
// place via context menus"). The contract, in the order asserted below:
//
//   right-click a row opens the shared context menu · the Inbox row offers
//   create only (no rename/delete — it is protected) · "New binder/deck
//   inside…" and the background "New … " open a create dialog that adds a
//   child/root · Rename edits a name in place · Delete hides one node and moves
//   its children up a level · drag drops reparent (into a row) and reorder (onto
//   a row's edge band) · a drop onto the node's own descendant is refused (the
//   client cycle pre-check), and the server is the backstop (409).
//
// These tests MUTATE the Neon dev branch, so every one creates its own
// uniquely-named scratch collections via the API and deletes them in a
// `finally`. Delete no longer cascades (specs/collection-deletion.md — children
// survive), so `deleteCollection` walks the subtree deepest-first.

test.use({ storageState: AUTH_STATE });

type Summary = {
  id: string;
  parent_id: string | null;
  name: string;
  is_inbox: boolean;
  position: number;
};
type TreeRow = { summary: Summary; present: number };

let scratchSeq = 0;
function scratchName(tag: string): string {
  // Unique per call across parallel workers/browsers: worker index + a
  // per-file counter. No wall-clock (deterministic, avoids collisions).
  scratchSeq += 1;
  const w = process.env.TEST_WORKER_INDEX ?? "0";
  return `zz-e2e-${tag}-w${w}-${scratchSeq}`;
}

async function createCollection(
  page: Page,
  body: { parent_id?: string | null; kind?: "binder" | "deck"; name: string },
): Promise<Summary> {
  const resp = await page.request.post("/api/collections", {
    data: { parent_id: null, kind: "binder", format: null, ...body },
  });
  expect(resp.ok(), `create ${body.name}: ${resp.status()}`).toBeTruthy();
  return (await resp.json()) as Summary;
}

/// Cleanup helper: remove a collection **and everything under it**.
///
/// Delete no longer cascades — it hides one node and moves its children up a
/// level (specs/collection-deletion.md) — so a cleanup that deleted only the
/// root would strand every descendant on the dev branch as a new top-level
/// collection. Deepest-first, reading the tree once before anything is hidden.
async function deleteCollection(page: Page, id: string): Promise<void> {
  const rows = await fetchTree(page);
  const children = new Map<string, string[]>();
  for (const r of rows) {
    const parent = r.summary.parent_id;
    if (parent) children.set(parent, [...(children.get(parent) ?? []), r.summary.id]);
  }
  const order: string[] = [];
  const walk = (node: string) => {
    for (const kid of children.get(node) ?? []) walk(kid);
    order.push(node);
  };
  walk(id);
  for (const node of order) {
    await page.request.post(`/api/collections/${node}/delete`, { data: {} });
  }
}

async function fetchTree(page: Page): Promise<TreeRow[]> {
  const resp = await page.request.get("/api/collection_tree");
  expect(resp.ok()).toBeTruthy();
  return ((await resp.json()) as { collections: TreeRow[] }).collections;
}

// ------------------------------------------- cards, for the honest-count --
// ---------------------------------------------- and picker tests (P6-189) --
//
// Same shape as `needs.spec.ts`'s own helpers (not shared — each e2e file is
// self-contained by this suite's convention).

type Card = { oracle_id: string; printing_id: string | null; name: string };
type Holding = { collection_id: string; quantity: number };

/// Catalog cards the signed-in user owns nowhere, with a real printing.
/// `q=z` rather than a vowel: the seed picked its own cards from name-ordered
/// searches, so the alphabetically-first slice of the catalog is exactly the
/// slice the dev user already owns. `skip` lets two calls in one test draw
/// disjoint cards.
async function unownedCards(
  request: APIRequestContext,
  n: number,
  skip = 0,
): Promise<Card[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const { cards: owned } = (await mine.json()) as { cards: { card: Card }[] };
  const taken = new Set(owned.map((r) => r.card.oracle_id));
  // `limit=200`, wider than `needs.spec.ts`'s 60: this file's delete tests
  // leave their card **owned** (relocated to the Inbox by the default
  // `ToParent`, per specs/collection-deletion.md — a delete relocates, it
  // does not return the card to "unowned"), so repeated local runs drain the
  // front of this pool permanently. A wider net is cheap insurance against
  // that, not a fix for it — see this task's Findings for the real one.
  const res = await request.get("/api/catalog/search?q=z&limit=200");
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

async function addHave(
  request: APIRequestContext,
  id: string,
  printingId: string,
  quantity = 1,
) {
  const res = await request.post(`/api/collections/${id}/have`, {
    data: { printing_id: printingId, quantity },
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

/// Copies of a card in one collection, summed across grains — `0` where the
/// holding is gone entirely (discarded, or never relocated there).
async function copiesIn(
  request: APIRequestContext,
  oracleId: string,
  collectionId: string,
): Promise<number> {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status(), "holdings").toBe(200);
  const rows = (await res.json()) as Holding[];
  return rows
    .filter((h) => h.collection_id === collectionId)
    .reduce((n, h) => n + h.quantity, 0);
}

/// Copies of a card across every collection — the total `unownedCards`'
/// `/api/all-cards?limit=200` check *means* to guarantee is zero, but cannot:
/// that route hard-caps at 200 rows, and this dev user's owned-card count has
/// grown past it, so a card past the cap can already be held without showing
/// up as "taken". Used to re-verify a candidate directly against the
/// unpaginated per-card holdings read before trusting it as a clean single-
/// location fixture.
async function totalCopies(
  request: APIRequestContext,
  oracleId: string,
): Promise<number> {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status(), "holdings").toBe(200);
  const rows = (await res.json()) as Holding[];
  return rows.reduce((n, h) => n + h.quantity, 0);
}

/// A card `unownedCards` believes is free, re-checked against the
/// unpaginated holdings read (see `totalCopies`) and skipped forward past the
/// pool's already-drained front until one genuinely has zero copies anywhere.
async function genuinelyUnownedCard(
  request: APIRequestContext,
  startSkip = 0,
): Promise<Card> {
  for (let skip = startSkip; skip < startSkip + 40; skip += 2) {
    const [candidate] = await unownedCards(request, 1, skip);
    if ((await totalCopies(request, candidate.oracle_id)) === 0) return candidate;
  }
  throw new Error(
    "no genuinely unowned card found in the pool — every candidate up to " +
      `skip ${startSkip + 40} is already held somewhere`,
  );
}

// A row's own clickable/draggable head — NOT `li > div`, which for a parent
// row is the `Collapsible` wrapper enclosing its descendants' heads too.
function rowHead(page: Page, id: string) {
  return page.locator(`[data-tree-row-head="${id}"]`);
}

async function openRowMenu(page: Page, id: string) {
  await rowHead(page, id).click({ button: "right" });
  const menu = page.locator("#context-menu-tree");
  await expect
    .poll(() => menu.evaluate((el: HTMLElement) => el.matches(":popover-open")))
    .toBe(true);
  return menu;
}

const treeMenu = (page: Page) => page.locator("#context-menu-tree");

const menuOpen = (page: Page) =>
  treeMenu(page).evaluate((el: HTMLElement) => el.matches(":popover-open"));

async function closeTreeMenu(page: Page) {
  await page.keyboard.press("Escape");
  await expect.poll(() => menuOpen(page)).toBe(false);
}

// The pinned **All cards** row and its own `⋯` (`app/src/my/tree.rs` —
// `PinnedRow menu=true` / `RowMenuButton`, `PINNED_MENU_KEY`).
const allCardsRow = (page: Page) => page.locator('[data-tree-pinned="all-cards"]');
const allCardsKebab = (page: Page) =>
  page.locator('[data-tree-row-actions="all-cards"]');

// Dispatch a full HTML5 drag sequence with one shared DataTransfer, dropping
// at a fractional Y in the target row (top band = before, middle = into,
// bottom = after — matches RowShell's `drop_intent`). Playwright's own
// `dragTo` is unreliable for HTML5 DnD across engines; manual dispatch is
// deterministic and works identically in chromium/firefox/webkit (verified).
async function dragRow(page: Page, srcId: string, dstId: string, yFrac: number) {
  await page.evaluate(
    ({ srcId, dstId, yFrac }) => {
      const src = document.querySelector(
        `[data-tree-row-head="${srcId}"]`,
      ) as HTMLElement;
      const dst = document.querySelector(
        `[data-tree-row-head="${dstId}"]`,
      ) as HTMLElement;
      const dt = new DataTransfer();
      const rect = dst.getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height * yFrac;
      const fire = (el: Element, type: string, x: number, y: number) =>
        el.dispatchEvent(
          new DragEvent(type, {
            bubbles: true,
            cancelable: true,
            clientX: x,
            clientY: y,
            dataTransfer: dt,
          }),
        );
      const s = src.getBoundingClientRect();
      fire(src, "dragstart", s.left + 5, s.top + 5);
      fire(dst, "dragover", cx, cy);
      fire(dst, "drop", cx, cy);
      fire(src, "dragend", cx, cy);
    },
    { srcId, dstId, yFrac },
  );
}

test.describe("context menu", () => {
  test("right-click a row opens the menu with management actions @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("menu") });
    try {
      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, parent.id);
      // The four row actions are present.
      for (const label of [
        "New binder inside…",
        "New deck inside…",
        "Rename…",
        "Delete…",
      ]) {
        await expect(menu.locator('[role="menuitem"]', { hasText: label })).toBeVisible();
      }
      // ESC closes it.
      await page.keyboard.press("Escape");
      await expect
        .poll(() => menu.evaluate((el: HTMLElement) => el.matches(":popover-open")))
        .toBe(false);
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("the Inbox row is protected — create only, no rename/delete @fast", async ({
    page,
  }) => {
    const inbox = (await fetchTree(page)).find((r) => r.summary.is_inbox)!;
    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, inbox.summary.id);
    await expect(
      menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }),
    ).toBeVisible();
    await expect(menu.locator('[role="menuitem"]', { hasText: "Rename…" })).toHaveCount(0);
    await expect(menu.locator('[role="menuitem"]', { hasText: "Delete…" })).toHaveCount(0);
  });
});

// The pinned **All cards** row now carries the same three ways into the shared
// menu that every collection row has (`app/src/my/tree.rs`). The capability was
// never missing — a right-click on blank rail has always opened
// `MenuTarget::Background` — but the only *discoverable* trigger for it was a
// right-click on nothing, which is why the alpha report said "a user must right
// click to discover this option". So these tests are about the affordance and
// about the menu being aimed at the top level, not about a new server path.
test.describe("All cards row menu", () => {
  test("the ⋯ is revealed on hover and on focus, transparent at rest @fast", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    const kebab = allCardsKebab(page);
    const opacity = () => kebab.evaluate((el) => getComputedStyle(el).opacity);

    // `md:opacity-0`, deliberately not `hidden` — both read as "visible" to
    // Playwright, so computed opacity is the only thing that can tell the
    // at-rest state from the revealed one (the same trap `responsive.spec.ts`
    // records for the tree rows' own ⋯).
    await expect.poll(opacity).toBe("0");
    expect((await kebab.boundingBox())!.width).toBeGreaterThan(0);

    await allCardsRow(page).hover();
    await expect.poll(opacity).toBe("1");

    // Off the row again: back to transparent, so the hover reveal is really
    // the hover's doing and not a button that was always shown.
    await page.mouse.move(0, 0);
    await expect.poll(opacity).toBe("0");

    await kebab.focus();
    await expect.poll(opacity).toBe("1");
  });

  test("the ⋯ opens the top-level create menu, and only that @fast", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    const menu = treeMenu(page);
    await clickUntil(allCardsKebab(page), () => menuOpen(page));

    // Exactly the two create items — the Background arm. The count is the
    // load-bearing half: it is what fails if this row ever starts aiming at a
    // `MenuTarget::Row` and offers Move/Rename/Delete for a virtual view that
    // has no collection behind it to move, rename or delete.
    await expect(menu.locator('[role="menuitem"]')).toHaveCount(2);
    await expect(menu.locator('[role="menuitem"]').nth(0)).toHaveText("New binder…");
    await expect(menu.locator('[role="menuitem"]').nth(1)).toHaveText("New deck…");

    await closeTreeMenu(page);
  });

  test("right-click and Shift+F10 on the row open the same menu @fast", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    const menu = treeMenu(page);
    const items = menu.locator('[role="menuitem"]');

    await allCardsRow(page).click({ button: "right" });
    await expect.poll(() => menuOpen(page)).toBe(true);
    await expect(items).toHaveCount(2);
    await expect(items.nth(0)).toHaveText("New binder…");
    await closeTreeMenu(page);

    // The platform keyboard chord, from the row's own link — the row head is a
    // `<div>`, so the keydown has to reach it by bubbling from a focused child.
    await allCardsRow(page).locator("a").focus();
    await page.keyboard.press("Shift+F10");
    await expect.poll(() => menuOpen(page)).toBe(true);
    await expect(items).toHaveCount(2);
    await expect(items.nth(0)).toHaveText("New binder…");
    await closeTreeMenu(page);
  });

  test("New binder… from the All cards menu creates a top-level binder @fast", async ({
    page,
  }) => {
    const name = scratchName("all-cr");
    let createdId: string | undefined;
    try {
      await page.goto("/my");
      await hydrated(page);
      await clickUntil(allCardsKebab(page), () => menuOpen(page));
      await treeMenu(page)
        .locator('[role="menuitem"]', { hasText: "New binder…" })
        .click();

      const dialog = page.locator('[role="dialog"]', { hasText: "New binder" });
      await expect(dialog).toContainText("top level");
      await dialog.locator("#tree-create-name").fill(name);
      await dialog.locator("#tree-create-confirm").click();

      // Same shape as the background-right-click test above: poll for the row
      // rather than for its `parent_id`, which is `null` at the top level — the
      // very value under test, so it cannot double as a "found?" sentinel.
      await expect
        .poll(async () => {
          const row = (await fetchTree(page)).find((r) => r.summary.name === name);
          createdId = row?.summary.id;
          return row ? "found" : "missing";
        })
        .toBe("found");
      const row = (await fetchTree(page)).find((r) => r.summary.name === name)!;
      expect(row.summary.parent_id).toBeNull();
      // …and it rendered into the tree, not just into the database.
      await expect(
        page.locator("nav[aria-label='Collections']", { hasText: name }),
      ).toBeVisible();
    } finally {
      if (createdId) await deleteCollection(page, createdId);
    }
  });

  test("a collection row's own ⋯ still opens its row menu @fast", async ({
    page,
  }) => {
    // The regression check on the refactor: both rows now render the *same*
    // `RowMenuButton`, so the thing that could break is a row's button aiming
    // at the background target (create-only) instead of at itself.
    const c = await createCollection(page, { name: scratchName("row-kebab") });
    try {
      await page.goto("/my");
      await hydrated(page);
      const kebab = page.locator(`[data-tree-row-actions="${c.id}"]`);
      await rowHead(page, c.id).hover();
      await clickUntil(kebab, () => menuOpen(page));

      const menu = treeMenu(page);
      for (const label of [
        "New binder inside…",
        "New deck inside…",
        "Move to…",
        "Rename…",
        "Delete…",
      ]) {
        await expect(menu.locator('[role="menuitem"]', { hasText: label })).toBeVisible();
      }
      // The row arm names its subject; the background arm names nothing.
      await expect(menu).toContainText(c.name);
      await closeTreeMenu(page);
    } finally {
      await deleteCollection(page, c.id);
    }
  });
});

test.describe("create", () => {
  test("New binder inside a collection adds a child @fast", async ({ page }) => {
    const parent = await createCollection(page, { name: scratchName("cr-par") });
    const childName = scratchName("cr-kid");
    try {
      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, parent.id);
      await menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }).click();

      const dialog = page.locator('[role="dialog"]', { hasText: "New binder" });
      await expect(dialog).toBeVisible();
      await expect(dialog).toContainText(parent.name); // "Inside <parent>."
      await dialog.locator("#tree-create-name").fill(childName);
      await dialog.locator("#tree-create-confirm").click();

      // The child appears under the parent, server-side confirmed.
      await expect
        .poll(async () => {
          const rows = await fetchTree(page);
          return rows.find((r) => r.summary.name === childName)?.summary.parent_id;
        })
        .toBe(parent.id);
      // …and rendered as a tree row (the resource refetched).
      await expect(page.locator("nav[aria-label='Collections']", { hasText: childName }))
        .toBeVisible();
    } finally {
      await deleteCollection(page, parent.id); // subtree-aware helper
    }
  });

  test("background right-click creates a top-level collection @fast", async ({
    page,
  }) => {
    const name = scratchName("cr-root");
    let createdId: string | undefined;
    try {
      await page.goto("/my");
      await hydrated(page);
      // Right-click the rail background (the tree container, below the rows).
      const root = page.locator("[data-tree-root]");
      await root.click({ button: "right", position: { x: 5, y: 5 } });
      const menu = page.locator("#context-menu-tree");
      await expect
        .poll(() => menu.evaluate((el: HTMLElement) => el.matches(":popover-open")))
        .toBe(true);
      await menu.locator('[role="menuitem"]', { hasText: "New binder…" }).click();

      const dialog = page.locator('[role="dialog"]', { hasText: "New binder" });
      await expect(dialog).toContainText("top level");
      await dialog.locator("#tree-create-name").fill(name);
      await dialog.locator("#tree-create-confirm").click();

      // Poll for the row to appear (not its parent_id — that is `null` at the
      // top level, the very value under test, so it can't double as a
      // "found?" sentinel).
      await expect
        .poll(async () => {
          const row = (await fetchTree(page)).find((r) => r.summary.name === name);
          createdId = row?.summary.id;
          return row ? "found" : "missing";
        })
        .toBe("found");
      // Created at the top level: no parent.
      const row = (await fetchTree(page)).find((r) => r.summary.name === name)!;
      expect(row.summary.parent_id).toBeNull();
    } finally {
      if (createdId) await deleteCollection(page, createdId);
    }
  });
});

// Dialog's Tab/Shift+Tab focus trap (P6-125, dialog.rs), exercised through
// the create dialog — one of the four `DialogContent` consumers in this
// file, and the one with no autofocus effect of its own (unlike the move
// picker below), so the starting focus is set explicitly rather than relied
// on. The trap is one fix at the `DialogContent` level (specs/app-ui.md); this
// is a regression check on this consumer, not a re-test of the mechanism
// itself (covered per-boundary in the `dialog` bench section /
// `end2end/bench-check.mjs`).
test.describe("Tab focus trap (P6-125)", () => {
  test("Tab cycles through the create dialog and wraps at the end @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("trap-cyc") });
    try {
      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, parent.id);
      await menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }).click();

      const dialog = page.locator("#tree-create");
      await expect(dialog).toBeVisible();

      // Deterministic starting point — this dialog installs no autofocus of
      // its own, so nothing puts the caret in the Name field on open.
      const nameInput = dialog.locator("#tree-create-name");
      await nameInput.focus();
      await expect(nameInput).toBeFocused();

      // DOM order inside DialogContent: close button, Name field, Cancel,
      // Create — the close button precedes the children `DialogContent`
      // wraps, so it (not the Name field) is the first tabbable stop.
      const closeBtn = page.locator('#tree-create > button[aria-label="Close dialog"]');
      const cancelBtn = dialog.locator("footer button", { hasText: "Cancel" });
      const createBtn = dialog.locator("#tree-create-confirm");

      await page.keyboard.press("Tab");
      await expect(cancelBtn).toBeFocused();
      await page.keyboard.press("Tab");
      await expect(createBtn).toBeFocused();
      // Create is the last tabbable — Tab from here wraps to the first
      // (the close button), never escaping to whatever is behind the scrim.
      await page.keyboard.press("Tab");
      await expect(closeBtn).toBeFocused();

      await page.keyboard.press("Escape");
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("Shift+Tab from the first tabbable wraps to the last @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("trap-shift") });
    try {
      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, parent.id);
      await menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }).click();

      const dialog = page.locator("#tree-create");
      await expect(dialog).toBeVisible();

      const closeBtn = page.locator('#tree-create > button[aria-label="Close dialog"]');
      const createBtn = dialog.locator("#tree-create-confirm");

      await closeBtn.focus();
      await expect(closeBtn).toBeFocused();
      // The close button is the first tabbable — Shift+Tab from here wraps
      // to the last (Create), not out to the page behind the scrim.
      await page.keyboard.press("Shift+Tab");
      await expect(createBtn).toBeFocused();

      await page.keyboard.press("Escape");
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("clicking non-interactive dialog chrome does not leak focus past the scrim @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("trap-chr") });
    try {
      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, parent.id);
      await menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }).click();

      const dialog = page.locator("#tree-create");
      await expect(dialog).toBeVisible();

      const closeBtn = page.locator('#tree-create > button[aria-label="Close dialog"]');
      const createBtn = dialog.locator("#tree-create-confirm");
      const description = dialog.locator('[data-name="DialogDescription"]');

      // Click non-interactive chrome (the description line, "At the top
      // level."). It has no focusable target of its own, so the browser's
      // focus algorithm walks up to the nearest focusable ancestor —
      // DialogContent's own `tabindex="-1"` (added for the zero-tabbables
      // fallback) — and lands there instead. That state is routine (any
      // click on a title/description/padding reaches it), not a rare edge
      // case: the container itself does not appear in the tabbable set, so
      // without the fix Shift+Tab here fell through to the browser's own
      // native Tab and walked backward out of the dialog entirely.
      await description.click();
      await expect(dialog).toBeFocused();
      await page.keyboard.press("Shift+Tab");
      await expect(createBtn).toBeFocused(); // not a control behind the scrim

      // Forward Tab from the same state redirects the other direction.
      await description.click();
      await expect(dialog).toBeFocused();
      await page.keyboard.press("Tab");
      await expect(closeBtn).toBeFocused();

      await page.keyboard.press("Escape");
    } finally {
      await deleteCollection(page, parent.id);
    }
  });
});

test("Rename edits the name in place @fast", async ({ page }) => {
  const before = scratchName("rn-before");
  const after = scratchName("rn-after");
  const c = await createCollection(page, { name: before });
  try {
    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, c.id);
    await menu.locator('[role="menuitem"]', { hasText: "Rename…" }).click();

    const dialog = page.locator('[role="dialog"]', { hasText: "Rename" });
    const field = dialog.locator("#tree-rename-name");
    await expect(field).toHaveValue(before); // pre-filled with the current name
    await field.fill(after);
    await dialog.locator("#tree-rename-confirm").click();

    await expect(rowHead(page, c.id)).toContainText(after);
    const server = (await fetchTree(page)).find((r) => r.summary.id === c.id);
    expect(server?.summary.name).toBe(after);
  } finally {
    await deleteCollection(page, c.id);
  }
});

// P6-126: `/my`'s WHERE column names each row's collection(s), and the
// all-cards resource's own sources used to be `(url_q, url_cursor,
// holdings_revision)` — never `TreeManage::revision`, the counter a sidebar
// rename bumps. So a rename left the column naming the old collection until
// an unrelated search or page turn happened to refetch it. Fixed by taking
// `TreeManage::revision` as a fourth resource source, same trick
// `collection.rs` already uses (`app/src/my/all_cards.rs`). This asserts the
// cell updates on its own — no reload, no re-search — right after the rename
// commits.
test("a sidebar rename updates the Where column on /my without navigating @fast", async ({
  page,
  request,
}) => {
  const before = scratchName("where-before");
  const after = scratchName("where-after");
  const c = await createCollection(page, { name: before });
  try {
    // `genuinelyUnownedCard`, not `unownedCards` directly: this test needs
    // the row to render the *single*-location form (`N in <name>`), which
    // only holds while the card is held nowhere else — see its own doc for
    // why the plain helper cannot promise that by itself.
    const card = await genuinelyUnownedCard(request);
    await addHave(request, c.id, card.printing_id as string, 1);

    // Search by name rather than loading the bare page: the dev seed owns far
    // more than one keyset page of cards, and the new holding's row can land
    // anywhere in it. The search narrows to just this card, so the row is
    // guaranteed on page one regardless of total count — and the query itself
    // never changes for the rest of the test, so nothing but the rename could
    // be responsible for the cell updating.
    await page.goto(`/my?q=${encodeURIComponent(card.name)}`);
    await hydrated(page);

    const location = page
      .locator(`[data-testid="all-cards-row"][data-oracle="${card.oracle_id}"]`)
      .locator('[data-testid="location-summary"]');
    await expect(location).toHaveText(`1 in ${before}`);

    const menu = await openRowMenu(page, c.id);
    await menu.locator('[role="menuitem"]', { hasText: "Rename…" }).click();
    const dialog = page.locator('[role="dialog"]', { hasText: "Rename" });
    await dialog.locator("#tree-rename-name").fill(after);
    await dialog.locator("#tree-rename-confirm").click();

    // No reload, no re-search: the cell must pick up the rename from the
    // tree revision alone, the same read this page has been showing all
    // along.
    await expect(location).toHaveText(`1 in ${after}`);
    await expect(location).not.toContainText(before);
  } finally {
    // Discard, not the generic `deleteCollection`'s `ToParent` default: a
    // relocate-on-delete would land this card in the Inbox and leave it
    // permanently "owned" for every future run drawing from the same
    // `unownedCards` pool — the exact drain `genuinelyUnownedCard` exists to
    // route around. Discard writes nothing (specs/collection-deletion.md),
    // so the card reads as held nowhere again once the collection is hidden.
    await page.request.post(`/api/collections/${c.id}/delete`, {
      data: { haves: { mode: "discard" } },
    });
  }
});

// P6-127, the other half of P6-126's fix. `/my/collections/:id` took
// `TreeManage::revision` — the counter EVERY tree mutation bumps — as a source
// of the resource that carries its **card table**, so a rename refetched the
// whole payload and rebuilt every row. That re-seeds each `CountStepper` from
// a new `value` signal and disposes the one the count's own undo toast is
// still pointing at, so `Undo` returned early and did nothing: the count stuck
// at the new number and the write never happened. (The same defect
// `collection.rs`'s module doc already records against awaiting the *tree*,
// reached from the other side.)
//
// Fixed by narrowing the source to `TreeManage::content_revision` — only the
// tree mutations that can move copies (delete, its undo, reparent) — and
// getting the rename's own freshness from the tree instead. So this test pins
// both halves at once: the `<h1>` must still follow the rename, and the undo
// must still write.
//
// The API read is the load-bearing assertion. The stepper showing "3" again
// could in principle be an optimistic write with no request behind it; the
// server's own count cannot be.
test("a rename mid-toast leaves the stepper's Undo working @fast", async ({
  page,
  request,
}) => {
  const before = scratchName("undo-before");
  const after = scratchName("undo-after");
  const c = await createCollection(page, { name: before });
  try {
    // A card held nowhere else, so this collection's row is the only holding
    // of it and `totalCopies` below reads exactly this row's quantity.
    const card = await genuinelyUnownedCard(request);
    await addHave(request, c.id, card.printing_id as string, 3);

    await page.goto(`/my/collections/${c.id}`);
    await hydrated(page);

    const row = page.locator(
      `[data-testid="collection-row"][data-oracle="${card.oracle_id}"]`,
    );
    // HERE-cell scoped: the WANTED cell carries a stepper of its own now.
    const count = row.locator(
      '[data-testid="here-cell"] [data-testid="count-stepper-value"]',
    );
    await expect(count).toHaveText("3");

    // Commit 3 → 5 through the click-to-type path (no hover dependency on the
    // opacity-revealed ± buttons).
    await count.click();
    await row
      .locator('[data-testid="here-cell"] [data-testid="count-stepper-input"]')
      .waitFor();
    await page.keyboard.type("5");
    await page.keyboard.press("Enter");
    await expect(count).toHaveText("5");

    const toast = page.locator('[data-name="Toast"]', {
      hasText: `${card.name}: 3 → 5`,
    });
    await expect(toast).toBeVisible();

    // …now rename this very collection from its own header kebab, while that
    // toast is still up. Toasts live 5s (`sonner.rs`), so everything between
    // here and the Undo click stays on the critical path deliberately — the
    // `toBeVisible` guard below fails loudly if it ever stops fitting rather
    // than letting the test pass without exercising the undo.
    await page.locator('[data-testid="collection-actions"]').click();
    await page
      .locator('[role="menuitem"]', { hasText: "Rename…" })
      .first()
      .click();
    const dialog = page.locator('[role="dialog"]', { hasText: "Rename" });
    await dialog.locator("#tree-rename-name").fill(after);
    await dialog.locator("#tree-rename-confirm").click();

    // The rename must still reach this page's own title with no navigation —
    // the requirement `TreeManage::revision` was made a source for. It is also
    // the synchronisation point: on the pre-fix code this is the moment the
    // payload refetch has landed and the table has been rebuilt.
    await expect(page.locator('[data-testid="collection-title"]')).toHaveText(
      after,
    );

    await expect(toast).toBeVisible();
    await toast.getByRole("button", { name: "Undo" }).click();

    await expect(count).toHaveText("3");
    // The write itself. `expect.poll` because the undo is a fire-and-forget
    // `spawn_local` — the optimistic UI is ahead of the request.
    await expect
      .poll(() => totalCopies(request, card.oracle_id), { timeout: 10_000 })
      .toBe(3);
  } finally {
    // Discard rather than relocate, for the reason the Where-column test
    // above documents: a relocate would leave this card permanently "owned"
    // and drain the `unownedCards` pool every future run draws from.
    await page.request.post(`/api/collections/${c.id}/delete`, {
      data: { haves: { mode: "discard" } },
    });
  }
});

// Delete relocates rather than destroys (specs/collection-deletion.md): the
// node is hidden, and its child survives by moving up a level. This test used
// to assert the opposite — "1 nested collection" and "cannot be undone" in the
// copy, and the whole subtree gone from the server — which was the accurate
// description of a real data-loss bug.
//
// `P6-189`'s honest counts land here too: an empty collection is the positive
// control for "the count is never silently wrong or omitted" — 0 cards, 0
// wants, and the child-collections line names an exact count (1) and an exact
// destination ("the top level", not "Inbox" — a re-parented child can *become*
// top-level, unlike a have, which needs somewhere real to land).
test("Delete hides the collection and moves its child up a level @fast", async ({
  page,
}) => {
  const parent = await createCollection(page, { name: scratchName("del-par") });
  const child = await createCollection(page, {
    parent_id: parent.id,
    name: scratchName("del-kid"),
  });
  try {
    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, parent.id);
    await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();

    const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
    // The copy says where things go, and no longer promises destruction.
    await expect(dialog.getByTestId("delete-cards-count")).toHaveText("0 cards");
    await expect(dialog.getByTestId("delete-wants-count")).toHaveText("0 wants");
    await expect(dialog.getByTestId("delete-children-line")).toHaveText(
      "1 collection moves up to the top level.",
    );
    await expect(dialog).not.toContainText("cannot be undone");
    await dialog.locator("#tree-delete-confirm").click();

    // The row is gone from the tree and the server…
    await expect(page.locator(`li[data-tree-row="${parent.id}"]`)).toHaveCount(0);
    await expect
      .poll(async () =>
        (await fetchTree(page)).some((r) => r.summary.id === parent.id),
      )
      .toBe(false);
    // …while the child is still there, now at the top level.
    await expect
      .poll(async () =>
        (await fetchTree(page)).find((r) => r.summary.id === child.id)?.summary
          .parent_id,
      )
      .toBeNull();
  } finally {
    await deleteCollection(page, child.id);
    await deleteCollection(page, parent.id);
  }
});

// The card count is this node's *own* present copies, never the rolled-up
// subtree total (`P6-111`, absorbed by `P6-189`) — a child's cards survive
// with it and must not inflate what this delete claims to relocate. The wants
// count is stated at all, which it never was before this task.
test("Delete's card count is this collection's own, not the rolled-up subtree @fast", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1);
  const parent = await createCollection(page, { name: scratchName("own-par") });
  const child = await createCollection(page, {
    parent_id: parent.id,
    name: scratchName("own-kid"),
  });
  try {
    // One card + one want directly in `parent`, and a *different* card in
    // `child` — if the dialog rolled the subtree up, `parent`'s count would
    // read 2, not 1.
    await addHave(request, parent.id, card.printing_id as string, 1);
    await addWant(request, parent.id, card.oracle_id, 1);
    const [childCard] = await unownedCards(request, 1, 1);
    await addHave(request, child.id, childCard.printing_id as string, 1);

    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, parent.id);
    await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();

    const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
    await expect(dialog.getByTestId("delete-cards-count")).toHaveText("1 card");
    await expect(dialog.getByTestId("delete-wants-count")).toHaveText("1 want");
    await expect(dialog.getByTestId("delete-children-line")).toHaveText(
      "1 collection moves up to the top level.",
    );

    // Defaults: haves → parent (Inbox, since `parent` is top-level), wants →
    // Remove from Collection. Confirmed by the pickers' own trigger labels.
    await expect(dialog.getByTestId("delete-haves-label")).toContainText(
      "Inbox (parent)",
    );
    await expect(dialog.getByTestId("delete-wants-label")).toHaveText(
      "Remove from Collection",
    );
    await dialog.locator("#tree-delete-confirm").click();
    await expect(page.locator(`li[data-tree-row="${parent.id}"]`)).toHaveCount(0);

    // The card moved to the Inbox (the default `ToParent`, top-level → Inbox)…
    const inboxId = (await fetchTree(page)).find((r) => r.summary.is_inbox)!
      .summary.id;
    await expect
      .poll(async () => await copiesIn(request, card.oracle_id, inboxId))
      .toBe(1);
    // …the want went hidden with the collection — not moved anywhere, not
    // still counted (`WantDisposition::Discard` writes nothing).
    const needs = await request.get(`/api/collections/${inboxId}/needs`);
    expect(needs.status()).toBe(200);
    const needRows = ((await needs.json()) as { rows: { oracle_id: string }[] })
      .rows;
    expect(needRows.some((r) => r.oracle_id === card.oracle_id)).toBe(false);
    // …and the child's own card is untouched — it survived, re-parented to
    // the top level, still holding what it always held.
    const childNow = (await fetchTree(page)).find((r) => r.summary.id === child.id);
    expect(childNow?.summary.parent_id).toBeNull();
    await expect
      .poll(async () => await copiesIn(request, childCard.oracle_id, child.id))
      .toBe(1);
  } finally {
    await deleteCollection(page, child.id);
    await deleteCollection(page, parent.id);
  }
});

// The two pickers actually wire through to the write: an explicit haves pick
// relocates to that collection (not the default parent), and "Remove from
// Collection" on the wants side is exercised as the default throughout the
// test above — this is the picker *interaction* the other test's defaults
// don't exercise.
test("The haves picker relocates cards to an explicit pick @fast", async ({
  page,
  request,
}) => {
  const [card] = await unownedCards(request, 1, 2);
  const subject = await createCollection(page, { name: scratchName("pick-subj") });
  const elsewhere = await createCollection(page, {
    name: scratchName("pick-dest"),
  });
  try {
    await addHave(request, subject.id, card.printing_id as string, 1);

    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, subject.id);
    await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();

    const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
    await dialog.getByTestId("delete-haves-trigger").click();
    const havesList = page.locator("#popover-tree-delete-haves");
    await havesList.getByTestId("destination-option").filter({
      hasText: elsewhere.name,
    }).click();
    await expect(dialog.getByTestId("delete-haves-trigger")).toContainText(
      elsewhere.name,
    );

    await dialog.locator("#tree-delete-confirm").click();
    await expect(page.locator(`li[data-tree-row="${subject.id}"]`)).toHaveCount(0);

    await expect
      .poll(async () => await copiesIn(request, card.oracle_id, elsewhere.id))
      .toBe(1);
  } finally {
    await deleteCollection(page, subject.id);
    await deleteCollection(page, elsewhere.id);
  }
});

// Escape must close only the topmost overlay — a picker `Popover` opened
// *inside* the delete `Dialog` never registered with the app's own overlay
// stack, so `Dialog`'s own Escape listener still believed itself topmost and
// closed the whole confirm out from under an open picker on the same
// keypress (Adversarial review, this task — verified live before the fix:
// one Escape closed both). `Popover` now registers and owns its own gated
// Escape listener, mirroring `Dialog`'s.
test("Escape closes only the open picker, not the delete dialog behind it @fast", async ({
  page,
}) => {
  const subject = await createCollection(page, { name: scratchName("esc") });
  try {
    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, subject.id);
    await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();

    const dialog = page.locator('[role="dialog"]#tree-delete');
    await expect(dialog).toHaveAttribute("data-state", "open");
    await dialog.getByTestId("delete-haves-trigger").click();
    const popover = page.locator("#popover-tree-delete-haves");
    await expect
      .poll(() => popover.evaluate((el) => el.matches(":popover-open")))
      .toBe(true);

    await page.keyboard.press("Escape");
    await expect
      .poll(() => popover.evaluate((el) => el.matches(":popover-open")))
      .toBe(false);
    // The positive control: without the fix this already reads "closed".
    await expect(dialog).toHaveAttribute("data-state", "open");

    // A second Escape, with no picker open, closes the dialog as normal.
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveAttribute("data-state", "closed");
  } finally {
    await deleteCollection(page, subject.id);
  }
});

test("Delete targets the row it was opened for, not a later right-click @fast", async ({
  page,
}) => {
  // The confirm snapshots its subject when it opens; the shared `menu_target`
  // keeps moving as the user right-clicks around (Codex review, this task). A
  // real right-click can't reach a row behind the modal backdrop, so we
  // dispatch `contextmenu` directly to move `menu_target` while the dialog is
  // open — then confirm must still delete the *original* row.
  const victim = await createCollection(page, { name: scratchName("snap-victim") });
  const bystander = await createCollection(page, { name: scratchName("snap-bystander") });
  let victimGone = false;
  try {
    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, victim.id);
    await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();
    const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
    await expect(dialog).toContainText(victim.name);

    // Move menu_target to the bystander behind the backdrop.
    await rowHead(page, bystander.id).dispatchEvent("contextmenu", {
      clientX: 20,
      clientY: 20,
      bubbles: true,
    });
    // The confirm still names — and deletes — the victim.
    await expect(dialog).toContainText(victim.name);
    await dialog.locator("#tree-delete-confirm").click();

    await expect
      .poll(async () => {
        const rows = await fetchTree(page);
        return {
          victim: rows.some((r) => r.summary.id === victim.id),
          bystander: rows.some((r) => r.summary.id === bystander.id),
        };
      })
      .toEqual({ victim: false, bystander: true });
    victimGone = true;
  } finally {
    if (!victimGone) await deleteCollection(page, victim.id);
    await deleteCollection(page, bystander.id);
  }
});

test.describe("drag", () => {
  test("drop into a row reparents @fast", async ({ page }) => {
    const a = await createCollection(page, { name: scratchName("dnd-a") });
    const b = await createCollection(page, { name: scratchName("dnd-b") });
    try {
      await page.goto("/my");
      await hydrated(page);
      await dragRow(page, a.id, b.id, 0.5); // middle band = into

      await expect
        .poll(async () =>
          (await fetchTree(page)).find((r) => r.summary.id === a.id)?.summary.parent_id,
        )
        .toBe(b.id);
      // Rendered nested: A's row sits inside B's collapsible panel.
      await expect(
        page.locator(`#tree-children-${b.id} li[data-tree-row="${a.id}"]`),
      ).toHaveCount(1);
    } finally {
      await deleteCollection(page, a.id);
      await deleteCollection(page, b.id);
    }
  });

  test("drop on a row's lower edge reorders among siblings", async ({ page }) => {
    // Two roots created back-to-back get positions p and p+1 (append). Drag
    // the earlier one onto the later one's bottom band → it takes a position
    // just past it, so server order flips.
    const first = await createCollection(page, { name: scratchName("ord-1") });
    const second = await createCollection(page, { name: scratchName("ord-2") });
    try {
      await page.goto("/my");
      await hydrated(page);
      // Precondition: first sorts before second.
      const pre = await fetchTree(page);
      const posFirst = pre.find((r) => r.summary.id === first.id)!.summary.position;
      const posSecond = pre.find((r) => r.summary.id === second.id)!.summary.position;
      expect(posFirst).toBeLessThan(posSecond);

      await dragRow(page, first.id, second.id, 0.9); // bottom band = after

      await expect
        .poll(async () => {
          const rows = await fetchTree(page);
          const pf = rows.find((r) => r.summary.id === first.id)!.summary.position;
          const ps = rows.find((r) => r.summary.id === second.id)!.summary.position;
          return pf > ps; // first now sorts after second
        })
        .toBe(true);
      // Same parent (a reorder, not a reparent).
      const after = await fetchTree(page);
      expect(after.find((r) => r.summary.id === first.id)!.summary.parent_id).toBeNull();
    } finally {
      await deleteCollection(page, first.id);
      await deleteCollection(page, second.id);
    }
  });

  test("dropping a node onto its own descendant is refused (cycle guard) @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("cyc-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("cyc-kid"),
    });
    try {
      await page.goto("/my");
      await hydrated(page);
      // Watch for a reparent write leaving the browser. The point of the
      // client forbidden-set is that dropping onto a descendant sends
      // *nothing* — asserting only the end-state can't tell "client refused"
      // from "client sent, server rejected 409", since both leave the tree
      // unchanged (Codex mutation-pass, this task).
      const reparentPosts: string[] = [];
      page.on("request", (r) => {
        if (r.method() === "POST" && /reparent_collection|\/reparent$/.test(r.url()))
          reparentPosts.push(r.url());
      });

      // Drag the parent INTO its own child — the client forbidden-set must
      // refuse it, so parent stays at the top level and no request is sent.
      await dragRow(page, parent.id, child.id, 0.5);
      await page.waitForTimeout(500);
      expect(reparentPosts, "the client must not send a cycle-creating reparent").toEqual([]);
      const rows = await fetchTree(page);
      expect(rows.find((r) => r.summary.id === parent.id)!.summary.parent_id).toBeNull();
      expect(rows.find((r) => r.summary.id === child.id)!.summary.parent_id).toBe(parent.id);

      // Backstop: even bypassing the client, the server rejects the cycle 409.
      const resp = await page.request.post(`/api/collections/${parent.id}/reparent`, {
        data: { new_parent_id: child.id },
      });
      expect(resp.status()).toBe(409);
    } finally {
      await deleteCollection(page, parent.id); // subtree-aware helper
    }
  });
});
