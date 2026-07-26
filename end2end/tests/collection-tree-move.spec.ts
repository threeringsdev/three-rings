import { expect, test, type Page } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// Tree "Move to…" — the mouse-free half of the drag layer (specs/app-ui.md
// "Collection tree"; design/information-architecture.md → "Tree management
// (create / rename / delete / move) happens in place via context menus").
//
// HTML5 drag fires on neither touch nor the keyboard, so before this the tree
// could not be reparented at all without a mouse. The contract asserted below:
//
//   a tree row is reachable by Tab and its `⋯` opens the shared menu · the menu
//   is keyboard-operable (focus lands in it, ↑↓ rove) · "Move to…" opens a
//   destination picker whose ⏎ commits a real reparent · the picker never
//   offers the moved node's own subtree (the cycle guard at the source) · "Top
//   level" un-nests · the picker's ↑↓ order is the order on screen (`command`'s
//   registration-order caveat) · and all of it works below `md`, where the rail
//   used to be `display:none` and the dialogs with it.
//
// These tests MUTATE the Neon dev branch; each creates uniquely-named scratch
// collections via the API and deletes them in a `finally` (delete cascades the
// subtree, so one delete per created root).

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

async function deleteCollection(page: Page, id: string): Promise<void> {
  await page.request.post(`/api/collections/${id}/delete`);
}

async function fetchTree(page: Page): Promise<TreeRow[]> {
  const resp = await page.request.get("/api/collection_tree");
  expect(resp.ok()).toBeTruthy();
  return ((await resp.json()) as { collections: TreeRow[] }).collections;
}

async function summaryOf(page: Page, id: string): Promise<Summary> {
  const row = (await fetchTree(page)).find((r) => r.summary.id === id);
  expect(row, `collection ${id} is gone`).toBeTruthy();
  return row!.summary;
}

// A row's own head — NOT `li > div`, which for a parent row is the
// `Collapsible` wrapper enclosing its descendants' heads too.
function rowHead(page: Page, id: string) {
  return page.locator(`[data-tree-row-head="${id}"]`);
}

const MENU = "#context-menu-tree";

function menuOpen(page: Page) {
  return page
    .locator(MENU)
    .evaluate((el: HTMLElement) => el.matches(":popover-open"));
}

// The move dialog. Paired with `[role=dialog]`: `DialogContent` renders a
// backdrop sibling that shares the panel's identifying attributes, so an
// id-only locator resolves to two elements.
function moveDialog(page: Page) {
  return page.locator('[role="dialog"]#tree-move');
}

/// Open/closed is `data-state`, never `toBeVisible()`. `DialogContent` closes
/// by fading (`data-[state=closed]:opacity-0`) and keeps its box, so a *closed*
/// dialog is "visible" to Playwright — `toBeVisible()` on it passes either way
/// and asserts nothing. (`toBeVisible()` still earns its keep for one thing:
/// whether an *ancestor* hid the whole subtree. See the mobile test.)
function expectMoveState(page: Page, state: "open" | "closed") {
  return expect(moveDialog(page)).toHaveAttribute("data-state", state);
}

// Only the rows the query left on screen. `CommandItem` hides a filtered-out
// row with `display: none` on the item while the test seam rides an inner
// span, so an unqualified locator counts every collection whatever is typed.
function moveOptions(page: Page) {
  return moveDialog(page).locator('[data-testid="destination-option"]:visible');
}

/** What currently has focus, described enough to assert on. */
function focusInfo(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    return {
      id: el?.id ?? "",
      role: el?.getAttribute("role") ?? "",
      text: (el?.textContent ?? "").trim(),
      rowActions: el?.getAttribute("data-tree-row-actions") ?? "",
    };
  });
}

/** Open a row's menu with the mouse — for the tests that are not about the route in. */
async function openRowMenu(page: Page, id: string) {
  await rowHead(page, id).click({ button: "right" });
  await expect.poll(() => menuOpen(page)).toBe(true);
  return page.locator(MENU);
}

async function openMovePicker(page: Page, id: string) {
  const menu = await openRowMenu(page, id);
  await menu.locator('[role="menuitem"]', { hasText: "Move to…" }).click();
  await expectMoveState(page, "open");
}

/** Strip the `📥`/`🗂` a destination row is labelled with. */
function bareName(label: string): string {
  return label.replace(/^[^\s]+\s+/, "").trim();
}

test.describe("keyboard", () => {
  test("a collection is moved without ever touching the mouse @fast", async ({
    page,
  }) => {
    const src = await createCollection(page, { name: scratchName("kb-src") });
    const dst = await createCollection(page, { name: scratchName("kb-dst") });
    try {
      await page.goto("/my");
      await hydrated(page);

      // Start where a keyboard user lands: the row's own link. Everything
      // after this is a real key press — clicking the menu item would prove
      // nothing about the path this task exists to create.
      await rowHead(page, src.id).locator("a").focus();

      // Tab reaches the row's actions button. Asserted, not assumed: if it
      // were not in the tab order the rest of this test would still "pass"
      // by opening the menu some other way.
      await page.keyboard.press("Tab");
      expect((await focusInfo(page)).rowActions).toBe(src.id);

      // ⏎ opens the shared menu *and* puts focus inside it.
      await page.keyboard.press("Enter");
      await expect.poll(() => menuOpen(page)).toBe(true);
      await expect
        .poll(async () => (await focusInfo(page)).role)
        .toBe("menuitem");
      expect((await focusInfo(page)).text).toBe("New binder inside…");

      // ↓↓ roves to "Move to…" — the third item.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      expect((await focusInfo(page)).text).toBe("Move to…");

      await page.keyboard.press("Enter");
      await expectMoveState(page, "open");
      // The dialog focuses its own field, or the keyboard path dead-ends here.
      await expect.poll(async () => (await focusInfo(page)).id).toBe(
        "tree-move-input",
      );

      // Type to narrow, ⏎ to commit.
      await page.keyboard.type(dst.name);
      await expect(moveOptions(page)).toHaveCount(1);
      await expect(moveOptions(page)).toHaveText(new RegExp(dst.name));
      await page.keyboard.press("Enter");

      // The server moved it — and to a *defined* spot: `plan_move` lands it
      // last among its new siblings, which in an empty destination is 1.0.
      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(dst.id);
      expect((await summaryOf(page, src.id)).position).toBe(1);
      // …and the dialog closed itself on success.
      await expectMoveState(page, "closed");
    } finally {
      await deleteCollection(page, src.id);
      await deleteCollection(page, dst.id);
    }
  });

  test("Escape leaves the menu and hands focus back to the row @fast", async ({
    page,
  }) => {
    const row = await createCollection(page, { name: scratchName("kb-esc") });
    try {
      await page.goto("/my");
      await hydrated(page);
      await rowHead(page, row.id).locator("a").focus();
      await page.keyboard.press("Tab");
      await page.keyboard.press("Enter");
      await expect.poll(() => menuOpen(page)).toBe(true);
      await expect
        .poll(async () => (await focusInfo(page)).role)
        .toBe("menuitem");

      await page.keyboard.press("Escape");
      await expect.poll(() => menuOpen(page)).toBe(false);
      // Back on the button it came from — not stranded on `<body>`, which is
      // where focus goes if nothing restores it.
      await expect
        .poll(async () => (await focusInfo(page)).rowActions)
        .toBe(row.id);
    } finally {
      await deleteCollection(page, row.id);
    }
  });
});

test.describe("the offered destinations", () => {
  test("never include the moved node's own subtree @fast", async ({ page }) => {
    const parent = await createCollection(page, { name: scratchName("cy-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("cy-kid"),
    });
    const other = await createCollection(page, { name: scratchName("cy-oth") });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, parent.id);

      const labels = await moveOptions(page).allInnerTexts();
      // The guard, at the source: no request the server would 409 can even be
      // asked for.
      expect(labels.some((l) => l.includes(parent.name))).toBe(false);
      expect(labels.some((l) => l.includes(child.name))).toBe(false);
      // The positive control — without it, an empty list passes the two
      // assertions above and says nothing.
      expect(labels.some((l) => l.includes(other.name))).toBe(true);
      expect(labels.some((l) => l.includes("Top level"))).toBe(true);
    } finally {
      await deleteCollection(page, parent.id);
      await deleteCollection(page, other.id);
    }
  });

  test("mark where the collection already lives, and picking it is a no-op @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("cu-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("cu-kid"),
    });
    const moves: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("reparent_collection")) moves.push(r.url());
    });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, child.id);

      // Exactly one row carries the ✓, and it is the current parent — the
      // count assertion is what stops "every row is ticked" from passing.
      const chosen = moveDialog(page).locator(
        '[data-testid="destination-option"][data-chosen="true"]',
      );
      await expect(chosen).toHaveCount(1);
      await expect(chosen).toHaveText(new RegExp(parent.name));

      // Picking it closes without writing anything.
      await chosen.click();
      await expectMoveState(page, "closed");
      expect((await summaryOf(page, child.id)).parent_id).toBe(parent.id);
      expect(moves, "a no-op move must not hit the API").toEqual([]);
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("the Inbox is offered as a destination but can never be moved @fast", async ({
    page,
  }) => {
    const inbox = (await fetchTree(page)).find((r) => r.summary.is_inbox)!;
    const src = await createCollection(page, { name: scratchName("inb-src") });
    try {
      await page.goto("/my");
      await hydrated(page);

      // The Inbox's own menu has no Move — the API refuses to reparent it, so
      // offering the action would only ever 409. (`New binder inside…` is the
      // control: the menu did open and does have items.)
      const menu = await openRowMenu(page, inbox.summary.id);
      await expect(
        menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }),
      ).toBeVisible();
      await expect(
        menu.locator('[role="menuitem"]', { hasText: "Move to…" }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect.poll(() => menuOpen(page)).toBe(false);

      // …but it *is* a legal target, exactly as it is for a drag.
      await openMovePicker(page, src.id);
      const labels = await moveOptions(page).allInnerTexts();
      expect(labels.some((l) => l.includes(inbox.summary.name))).toBe(true);
    } finally {
      await deleteCollection(page, src.id);
    }
  });
});

test.describe("committing", () => {
  test('"Top level" un-nests a nested collection @fast', async ({ page }) => {
    const parent = await createCollection(page, { name: scratchName("tl-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("tl-kid"),
    });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, child.id);
      await moveOptions(page)
        .filter({ hasText: "Top level" })
        .click();

      await expect
        .poll(async () => (await summaryOf(page, child.id)).parent_id)
        .toBe(null);
      // And the sidebar caught up (the resource refetched), so the move is
      // visible without a reload.
      await expect(
        page.locator(`li[data-tree-row="${child.id}"]`).first(),
      ).toBeVisible();
    } finally {
      await deleteCollection(page, parent.id);
      // The child is a root now, so the parent's cascade no longer takes it.
      await deleteCollection(page, child.id);
    }
  });

  test("the picker's ↓ order is the order on screen @fast", async ({ page }) => {
    // `command` builds its keyboard registry at view-*construction* time and
    // `visible_ids()` returns that order, so a consumer is only safe while
    // construction order equals document order. This picker sorts its data
    // before any row mounts and typing only hides rows — this test is what
    // pins that: whatever is drawn second must be what one ↓ selects.
    const stem = scratchName("ord");
    const src = await createCollection(page, { name: `${stem}-src` });
    const a = await createCollection(page, { name: `${stem}-a` });
    const b = await createCollection(page, { name: `${stem}-b` });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, src.id);
      // Narrow to this test's own collections (the moved node is excluded from
      // its own list, so `src` is not among them).
      await moveDialog(page).locator("#tree-move-input").fill(stem);

      const labels = await moveOptions(page).allInnerTexts();
      expect(
        labels.length,
        "need at least two rows for 'second' to mean anything",
      ).toBeGreaterThanOrEqual(2);
      const second = bareName(labels[1]);
      expect(second, "the two rows must be distinguishable").not.toBe(
        bareName(labels[0]),
      );

      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");

      const expected = (await fetchTree(page)).find(
        (r) => r.summary.name === second,
      )!;
      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(expected.summary.id);
    } finally {
      await deleteCollection(page, src.id);
      await deleteCollection(page, a.id);
      await deleteCollection(page, b.id);
    }
  });
});

test.describe("mobile", () => {
  // A real phone: `hasTouch` so taps are taps, and a width below `md` — the
  // width at which the rail used to be `display:none`, taking the tree, its
  // context menu and (mounted inside it) every management dialog with it.
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

  test("the tree, its menu and the move dialog are all reachable by touch @fast", async ({
    page,
  }) => {
    const src = await createCollection(page, { name: scratchName("mb-src") });
    const dst = await createCollection(page, { name: scratchName("mb-dst") });
    try {
      await page.goto("/my");
      await hydrated(page);

      // Closed at rest — the positive control for "visible after the tap".
      const rail = page.locator("#sidebar-rail");
      await expect(rail).toBeHidden();
      await page.getByTestId("rail-toggle").tap();
      await expect(rail).toBeVisible();

      // The row's actions button is a real tap target here (no hover to
      // reveal it on a phone).
      const actions = page.locator(`[data-tree-row-actions="${src.id}"]`);
      await expect(actions).toBeVisible();
      await actions.tap();
      await expect.poll(() => menuOpen(page)).toBe(true);

      await page.locator(MENU).locator('[role="menuitem"]', {
        hasText: "Move to…",
      }).tap();

      // The regression this pins, and the one place `toBeVisible()` on a
      // dialog says something: `data-state` would read "open" even with
      // `TreeDialogs` mounted inside the `display:none` rail, because the
      // signal flips either way. Only a zero-sized box catches the ancestor
      // having swallowed the whole subtree.
      await expectMoveState(page, "open");
      await expect(moveDialog(page)).toBeVisible();
      await moveDialog(page).locator("#tree-move-input").fill(dst.name);
      await expect(moveOptions(page)).toHaveCount(1);
      await moveOptions(page).tap();

      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(dst.id);
    } finally {
      await deleteCollection(page, src.id);
      await deleteCollection(page, dst.id);
    }
  });
});
