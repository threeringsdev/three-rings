import { expect, test, type Page } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// The collection header's `⋯` (design/wireframes.pen → `Header Kebab` on
// *Desktop — Collection view*, `M Header Kebab` on *Mobile — Collection view*;
// design/information-architecture.md → "Tree management (create / rename /
// delete / move) happens in place via context menus", and "navigation collapses,
// features don't").
//
// It is the *second* home for the tree's management menu, and on a phone the
// natural one — the tree is behind a drawer there. The contract asserted below:
//
//   the kebab is on screen at rest at both widths (it is NOT hover-revealed like
//   the tree row's `⋯`) · it opens the same panel with the same five actions the
//   tree row offers for the same collection, and offers no sixth · the keyboard
//   reaches all of it with real key presses (⏎ opens and focus lands inside, ↑↓
//   rove, ESC closes and hands focus back) · a rename or a create done from the
//   header is *visible on the page it was done from* · deleting the collection
//   you are looking at walks up to its parent instead of leaving a dead id on
//   screen · `Move to…` resolves the subject's subtree off the route, so it
//   cannot offer a descendant · the Inbox offers create only · and all of it
//   works by touch at 390 px.
//
// These tests MUTATE the Neon dev branch; each creates uniquely-named scratch
// collections via the API and deletes them in a `finally` (delete cascades the
// subtree, so one delete per created root).

test.use({ storageState: AUTH_STATE });

type Summary = {
  id: string;
  parent_id: string | null;
  kind: "binder" | "deck";
  name: string;
  is_inbox: boolean;
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

// The header's panel — a *second* `context_menu` instance, so it has its own
// popover id. Scoping to it matters: at `md` and up the sidebar's own
// `#context-menu-tree` panel is in the same document rendering the same
// `TreeMenu` off the same `menu_target`, and an unscoped `[role=menuitem]`
// locator would resolve to both panels' copies.
const MENU = "#context-menu-collection-header";
const KEBAB = '[data-testid="collection-actions"]';

function menuOpen(page: Page) {
  return page
    .locator(MENU)
    .evaluate((el: HTMLElement) => el.matches(":popover-open"));
}

function items(page: Page) {
  return page.locator(`${MENU} [role="menuitem"]`);
}

async function openKebab(page: Page) {
  await page.locator(KEBAB).click();
  await expect.poll(() => menuOpen(page)).toBe(true);
}

/** What currently has focus, described enough to assert on. */
function focusInfo(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    return {
      id: el?.id ?? "",
      role: el?.getAttribute("role") ?? "",
      text: (el?.textContent ?? "").trim(),
      testid: el?.getAttribute("data-testid") ?? "",
    };
  });
}

/// `toBeVisible()` cannot see an `opacity: 0` element — it reads as visible. The
/// tree row's own `⋯` *is* `opacity-0` until hover/focus at `md` and up, so
/// "visible" alone would pass for a kebab that had accidentally inherited that
/// treatment. The frame draws this one at rest, so opacity is part of the claim.
async function expectOnScreenAtRest(page: Page) {
  const kebab = page.locator(KEBAB);
  await expect(kebab).toBeVisible();
  const opacity = await kebab.evaluate(
    (el) => getComputedStyle(el).opacity,
  );
  expect(opacity, "the header kebab is not hover-revealed").toBe("1");
}

const MENU_LABELS = [
  "New binder inside…",
  "New deck inside…",
  "Move to…",
  "Rename…",
  "Delete…",
];

test.describe("desktop", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("the kebab opens the tree's menu, with the tree row's own actions @fast", async ({
    page,
  }) => {
    const col = await createCollection(page, { name: scratchName("set") });
    try {
      await page.goto(`/my/collections/${col.id}`);
      await hydrated(page);
      await expectOnScreenAtRest(page);

      await openKebab(page);
      // The whole set, in order — a count-free "has Rename…" assertion would
      // not notice a sixth item appearing or a fifth going missing.
      await expect(items(page)).toHaveText(MENU_LABELS);
      // The argued exclusion: teardown is a content action and stays a visible
      // button on the page (asserted for a deck below), not a menu row.
      await expect(
        items(page).filter({ hasText: "Empty deck" }),
      ).toHaveCount(0);

      // …and it is the *same* set the sidebar row for this very collection
      // offers. This is the assertion that makes "the two surfaces do not
      // drift" mean something rather than being a claim in a comment.
      await page.keyboard.press("Escape");
      await expect.poll(() => menuOpen(page)).toBe(false);
      await page.locator(`[data-tree-row-head="${col.id}"]`).click({
        button: "right",
      });
      const treeMenu = page.locator("#context-menu-tree");
      await expect
        .poll(() =>
          treeMenu.evaluate((el: HTMLElement) => el.matches(":popover-open")),
        )
        .toBe(true);
      await expect(treeMenu.locator('[role="menuitem"]')).toHaveText(
        MENU_LABELS,
      );
    } finally {
      await deleteCollection(page, col.id);
    }
  });

  test("a deck keeps Empty deck… as a button beside the kebab @fast", async ({
    page,
  }) => {
    // The positive control for the exclusion above: the action was not dropped,
    // it simply did not move into the menu.
    const deck = await createCollection(page, {
      kind: "deck",
      name: scratchName("deck"),
    });
    try {
      await page.goto(`/my/collections/${deck.id}`);
      await hydrated(page);
      await expect(page.getByTestId("teardown-open")).toBeVisible();
      await expectOnScreenAtRest(page);
    } finally {
      await deleteCollection(page, deck.id);
    }
  });

  test("the whole menu is reachable with real key presses @fast", async ({
    page,
  }) => {
    const col = await createCollection(page, { name: scratchName("kb") });
    try {
      await page.goto(`/my/collections/${col.id}`);
      await hydrated(page);

      // Focus the button and press — never click it. A click would prove
      // nothing about the keyboard path, which is the half a mouse hides.
      await page.locator(KEBAB).focus();
      expect((await focusInfo(page)).testid).toBe("collection-actions");

      await page.keyboard.press("Enter");
      await expect.poll(() => menuOpen(page)).toBe(true);
      // ⏎ must also put focus *inside* the panel, or the rest of the menu is
      // unreachable: Tab from the opener walks the document, not the popover.
      await expect
        .poll(async () => (await focusInfo(page)).role)
        .toBe("menuitem");
      expect((await focusInfo(page)).text).toBe("New binder inside…");

      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      expect((await focusInfo(page)).text).toBe("Move to…");
      // Wraps at the end, like a menu is expected to.
      await page.keyboard.press("ArrowUp");
      await page.keyboard.press("ArrowUp");
      await page.keyboard.press("ArrowUp");
      expect((await focusInfo(page)).text).toBe("Delete…");

      await page.keyboard.press("Escape");
      await expect.poll(() => menuOpen(page)).toBe(false);
      // Back on the kebab — not stranded on `<body>`, which is where focus goes
      // if nothing restores it.
      await expect
        .poll(async () => (await focusInfo(page)).testid)
        .toBe("collection-actions");
    } finally {
      await deleteCollection(page, col.id);
    }
  });

  test("Rename… from the header renames it and the title follows @fast", async ({
    page,
  }) => {
    // The page's title, counts and folder rows come from `collection_view`,
    // which no `tree.refetch()` can update — before `TreeManage::revision` this
    // left the `<h1>` on the old name beside a breadcrumb showing the new one.
    const col = await createCollection(page, { name: scratchName("ren") });
    const renamed = scratchName("ren-after");
    try {
      await page.goto(`/my/collections/${col.id}`);
      await hydrated(page);
      await expect(page.getByTestId("collection-title")).toHaveText(col.name);

      await openKebab(page);
      await items(page).filter({ hasText: "Rename…" }).click();
      await expect(page.locator('[role="dialog"]#tree-rename')).toHaveAttribute(
        "data-state",
        "open",
      );
      await page.locator("#tree-rename-name").fill(renamed);
      await page.locator("#tree-rename-confirm").click();

      // The server took it…
      await expect
        .poll(async () => (await summaryOf(page, col.id)).name)
        .toBe(renamed);
      // …and so did the page, without a reload.
      await expect(page.getByTestId("collection-title")).toHaveText(renamed);
    } finally {
      await deleteCollection(page, col.id);
    }
  });

  test("New binder inside… adds a folder row to the page it was run from @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("mk") });
    const child = scratchName("mk-kid");
    try {
      await page.goto(`/my/collections/${parent.id}`);
      await hydrated(page);
      // No children yet — the positive control for the row appearing.
      await expect(page.getByTestId("folder-row")).toHaveCount(0);

      await openKebab(page);
      await items(page).filter({ hasText: "New binder inside…" }).click();
      await expect(page.locator('[role="dialog"]#tree-create')).toHaveAttribute(
        "data-state",
        "open",
      );
      await page.locator("#tree-create-name").fill(child);
      await page.locator("#tree-create-confirm").click();

      await expect
        .poll(async () =>
          (await fetchTree(page)).some((r) => r.summary.name === child),
        )
        .toBe(true);
      const created = (await fetchTree(page)).find(
        (r) => r.summary.name === child,
      )!.summary;
      expect(created.parent_id).toBe(parent.id);
      // The row the create should have produced *on this page*.
      await expect(
        page.locator(
          `[data-testid="folder-row"][data-collection="${created.id}"]`,
        ),
      ).toBeVisible();
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("Move to… resolves the subtree off the route, so it never offers a descendant @fast", async ({
    page,
  }) => {
    // The header's subject comes from the URL, not from a tree row, so its
    // `forbidden` set is looked up in the tree by id (`MenuTarget::for_collection`).
    // This is the assertion that the lookup found the *subtree* and not just
    // the node.
    const subject = await createCollection(page, { name: scratchName("mv") });
    const kid = await createCollection(page, {
      parent_id: subject.id,
      name: scratchName("mv-kid"),
    });
    const target = await createCollection(page, { name: scratchName("mv-dst") });
    try {
      await page.goto(`/my/collections/${subject.id}`);
      await hydrated(page);
      await openKebab(page);
      await items(page).filter({ hasText: "Move to…" }).click();

      const dialog = page.locator('[role="dialog"]#tree-move');
      await expect(dialog).toHaveAttribute("data-state", "open");
      const labels = await dialog
        .locator('[data-testid="destination-option"]:visible')
        .allInnerTexts();
      expect(labels.some((l) => l.includes(subject.name))).toBe(false);
      expect(labels.some((l) => l.includes(kid.name))).toBe(false);
      // The positive control — without it an empty list passes both.
      expect(labels.some((l) => l.includes(target.name))).toBe(true);

      await dialog.locator("#tree-move-input").fill(target.name);
      await dialog
        .locator('[data-testid="destination-option"]:visible')
        .first()
        .click();
      await expect
        .poll(async () => (await summaryOf(page, subject.id)).parent_id)
        .toBe(target.id);
    } finally {
      await deleteCollection(page, subject.id);
      await deleteCollection(page, target.id);
    }
  });

  test("the Inbox header offers create only @fast", async ({ page }) => {
    // Rename, delete *and* reparent are all refused by the API for the Inbox
    // with the same `AND NOT is_inbox`, so offering any of them would only ever
    // produce an error. Same withholding as the tree row's menu.
    const inbox = (await fetchTree(page)).find((r) => r.summary.is_inbox)!;
    await page.goto(`/my/collections/${inbox.summary.id}`);
    await hydrated(page);
    await openKebab(page);
    // The positive control first: the menu did open and does have items.
    await expect(items(page)).toHaveText([
      "New binder inside…",
      "New deck inside…",
    ]);
    for (const gone of ["Move to…", "Rename…", "Delete…"]) {
      await expect(items(page).filter({ hasText: gone })).toHaveCount(0);
    }
  });
});

test.describe("deleting what you are looking at", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("walks up to the parent instead of leaving a dead id on screen @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("del-p") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("del-k"),
    });
    try {
      await page.goto(`/my/collections/${child.id}`);
      await hydrated(page);
      await openKebab(page);
      await items(page).filter({ hasText: "Delete…" }).click();
      await expect(page.locator('[role="dialog"]#tree-delete')).toHaveAttribute(
        "data-state",
        "open",
      );
      await page.locator("#tree-delete-confirm").click();

      await expect(page).toHaveURL(
        new RegExp(`/my/collections/${parent.id}$`),
      );
      // Landed on a real page, not an error one — the check that says the
      // navigation went somewhere useful rather than merely somewhere else.
      await expect(page.getByTestId("collection-title")).toHaveText(
        parent.name,
      );
      await expect(page.getByTestId("collection-error")).toHaveCount(0);
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("falls back to /my when it was top level @fast", async ({ page }) => {
    const col = await createCollection(page, { name: scratchName("del-top") });
    let deleted = false;
    try {
      await page.goto(`/my/collections/${col.id}`);
      await hydrated(page);
      await openKebab(page);
      await items(page).filter({ hasText: "Delete…" }).click();
      await page.locator("#tree-delete-confirm").click();

      await expect(page).toHaveURL(/\/my$/);
      deleted = true;
      // We are off the collection view entirely — not still standing on it with
      // a changed URL, which is what "the page kept rendering a dead id" would
      // look like.
      await expect(page.getByTestId("collection-page")).toHaveCount(0);
      // …and on the My-cards landing. Asserted via the heading rather than the
      // All-cards *table*: an SPA navigation into `/my` renders the table's
      // empty state and issues **zero** requests — a pre-existing defect that
      // reproduces identically through the sidebar's own `All cards` row and the
      // collection breadcrumb's root link (measured; nothing in this task
      // touches `/my` or `all_cards.rs`). Filed rather than papered over here.
      await expect(
        page.locator("h1", { hasText: "All cards" }).first(),
      ).toBeVisible();
    } finally {
      if (!deleted) await deleteCollection(page, col.id);
    }
  });
});

test.describe("deleting something else you can see", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("takes its folder row and its rollup off the page with it @fast", async ({
    page,
  }) => {
    // The other half of the delete story, and the half the first cut got wrong:
    // when `route_after_delete` returns `None` there is no navigation, so nothing
    // refetched `collection_view` and the deleted child stayed on its parent's
    // page as a folder row linking to an id the database no longer had — with its
    // copies still counted in the header. Exactly the defect class `revision` was
    // added for.
    //
    // Driven from the **sidebar row**, not the kebab: the parent is the page most
    // likely to be open when you right-click a child in the tree, and the fix
    // belongs to the shared dialog rather than to either trigger.
    const parent = await createCollection(page, { name: scratchName("sib-p") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("sib-k"),
    });
    try {
      await page.goto(`/my/collections/${parent.id}`);
      await hydrated(page);

      const folder = page.locator(
        `[data-testid="folder-row"][data-collection="${child.id}"]`,
      );
      // The positive control: the row is there to begin with, so its later
      // absence means something.
      await expect(folder).toBeVisible();
      await expect(folder.locator("a")).toHaveAttribute(
        "href",
        `/my/collections/${child.id}`,
      );

      await page.locator(`[data-tree-row-head="${child.id}"]`).click({
        button: "right",
      });
      const treeMenu = page.locator("#context-menu-tree");
      await expect
        .poll(() =>
          treeMenu.evaluate((el: HTMLElement) => el.matches(":popover-open")),
        )
        .toBe(true);
      await treeMenu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();
      await page.locator("#tree-delete-confirm").click();

      // Gone from the server…
      await expect
        .poll(async () =>
          (await fetchTree(page)).some((r) => r.summary.id === child.id),
        )
        .toBe(false);
      // …and gone from this page, which never navigated.
      await expect(page).toHaveURL(
        new RegExp(`/my/collections/${parent.id}$`),
      );
      await expect(folder).toHaveCount(0);
      // The header stopped counting its copies too. `0 here` is the whole
      // sentence for an empty binder with nothing rolled up (`counts_summary`),
      // so this also proves the rollup clause went away rather than just
      // shrinking.
      await expect(page.getByTestId("collection-counts")).toHaveText("0 here");
    } finally {
      await deleteCollection(page, parent.id);
    }
  });
});

test.describe("mobile", () => {
  // A real phone: `hasTouch` so taps are taps, and a width below `md` — where
  // the collection tree is behind the rail drawer and this kebab is the natural
  // home for management.
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

  test("the kebab is a 44 px tap target that opens the menu @fast", async ({
    page,
  }) => {
    const col = await createCollection(page, { name: scratchName("mb") });
    const dst = await createCollection(page, { name: scratchName("mb-dst") });
    try {
      await page.goto(`/my/collections/${col.id}`);
      await hydrated(page);

      // The rail drawer is shut at this width — the control that says this path
      // does not secretly depend on the tree being on screen.
      await expect(page.locator("#sidebar-rail")).toBeHidden();
      await expectOnScreenAtRest(page);
      const box = (await page.locator(KEBAB).boundingBox())!;
      expect(box.width).toBeGreaterThanOrEqual(44);
      expect(box.height).toBeGreaterThanOrEqual(44);

      await page.locator(KEBAB).tap();
      await expect.poll(() => menuOpen(page)).toBe(true);
      await expect(items(page)).toHaveText(MENU_LABELS);

      await items(page).filter({ hasText: "Move to…" }).tap();
      const dialog = page.locator('[role="dialog"]#tree-move');
      await expect(dialog).toHaveAttribute("data-state", "open");
      // `data-state` flips whether or not an ancestor swallowed the subtree;
      // only a non-zero box catches a dialog rendered inside `display: none`.
      await expect(dialog).toBeVisible();

      await dialog.locator("#tree-move-input").fill(dst.name);
      const options = dialog.locator(
        '[data-testid="destination-option"]:visible',
      );
      await expect(options).toHaveCount(1);
      await options.tap();
      await expect
        .poll(async () => (await summaryOf(page, col.id)).parent_id)
        .toBe(dst.id);
    } finally {
      await deleteCollection(page, col.id);
      await deleteCollection(page, dst.id);
    }
  });

  test("no sideways scroll with the kebab in the header @fast", async ({
    page,
  }) => {
    // Both kinds: a **deck** is the crowded case, because `Empty deck…` shares
    // the `Header Actions` cluster with the kebab. A binder alone would not
    // exercise the width the two of them need.
    const binder = await createCollection(page, { name: scratchName("mb-ovf") });
    const deck = await createCollection(page, {
      kind: "deck",
      name: scratchName("mb-ovf-deck"),
    });
    try {
      for (const col of [binder, deck]) {
        await page.goto(`/my/collections/${col.id}`);
        await hydrated(page);
        // The control: the header actions really are on screen here, so a pass
        // is not "nothing rendered, nothing overflowed".
        await expect(page.locator(KEBAB)).toBeVisible();
        if (col === deck) {
          await expect(page.getByTestId("teardown-open")).toBeVisible();
        }
        const doc = await page.evaluate(() => ({
          scroll: document.documentElement.scrollWidth,
          client: document.documentElement.clientWidth,
        }));
        expect(
          doc.scroll,
          `${col.name} overflows the document: ${JSON.stringify(doc)}`,
        ).toBe(doc.client);
      }
    } finally {
      await deleteCollection(page, binder.id);
      await deleteCollection(page, deck.id);
    }
  });
});
