import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// Undo and Restore: two deliberately different recovery paths for a soft
// delete (specs/collection-deletion.md → step 5, P6-190).
//
//   Undo (the delete toast) reverses a delete *whole*, from its own receipt:
//   the collection un-hides, its cards move back, its wants move back, and
//   its children re-parent back — including a want that was explicitly
//   relocated (`WantDisposition::To`), which the receipt only grew a handle
//   for in this task (maintainer ruling 2026-08-10).
//
//   Restore (the "Recently deleted" list, `/my/recently-deleted`) is the
//   weaker, later path: the collection comes back, re-attached to its
//   original parent if that parent is still live (otherwise top level), but
//   its cards and children are left wherever they now are.
//
// These tests mutate the Neon dev branch. Per this file's own Findings
// discipline (collection-tree-manage.spec.ts), cleanup here goes further than
// deleting scratch collections: the Undo test's fixture card is explicitly
// *removed* (not just relocated) before cleanup, so a repeated local run does
// not permanently drain the shared `catalog/search?q=z` unowned-card pool the
// way a plain relocating delete would (this file's own scratch collections
// are deleted afterward regardless, which is the accepted debris pattern
// every file in this suite already leaves).

test.use({ storageState: AUTH_STATE });

type Summary = {
  id: string;
  parent_id: string | null;
  name: string;
  is_inbox: boolean;
  position: number;
};
type TreeRow = { summary: Summary; present: number };
type Card = { oracle_id: string; printing_id: string | null; name: string };
type Holding = {
  id: string;
  collection_id: string;
  printing_id: string;
  quantity: number;
};

let scratchSeq = 0;
function scratchName(tag: string): string {
  scratchSeq += 1;
  const w = process.env.TEST_WORKER_INDEX ?? "0";
  return `zz-e2e-undo-${tag}-w${w}-${scratchSeq}`;
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

async function fetchTree(page: Page): Promise<TreeRow[]> {
  const resp = await page.request.get("/api/collection_tree");
  expect(resp.ok()).toBeTruthy();
  return ((await resp.json()) as { collections: TreeRow[] }).collections;
}

/// Cleanup: remove a collection and everything under it. Delete no longer
/// cascades (specs/collection-deletion.md), so this walks the *live* subtree
/// deepest-first — a row already soft-deleted (e.g. by the test itself, as
/// part of what's under test) is simply absent from this read and skipped,
/// which is correct: deleting it again would 404.
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
    if (!rows.some((r) => r.summary.id === node) && node !== id) continue;
    await page.request.post(`/api/collections/${node}/delete`, { data: {} });
  }
}

/// Catalog cards the signed-in user owns nowhere, with a real printing. Same
/// shape as `collection-tree-manage.spec.ts`'s own helper (not shared — each
/// e2e file in this suite is self-contained), `limit=200` for the same
/// already-documented reason (repeated local runs drain the front of a
/// narrower slice).
async function unownedCards(
  request: APIRequestContext,
  n: number,
  skip = 0,
): Promise<Card[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const { cards: owned } = (await mine.json()) as { cards: { card: Card }[] };
  const taken = new Set(owned.map((r) => r.card.oracle_id));
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

/// Fully release a card's copies back to "owned nowhere" — a real removal
/// (`to_collection_id: null`), not a relocating delete. This is the fixture
/// hygiene this file's own Findings ask for: the Undo test's card ends the
/// test back where it started (attached to a scratch collection about to be
/// deleted), so without this it would permanently join the Inbox and shrink
/// the shared unowned-card pool on every local run.
async function releaseHolding(
  request: APIRequestContext,
  oracleId: string,
  collectionId: string,
) {
  const res = await request.get(`/api/cards/${oracleId}/holdings`);
  expect(res.status()).toBe(200);
  const rows = (await res.json()) as Holding[];
  for (const h of rows.filter((h) => h.collection_id === collectionId)) {
    const move = await request.post(`/api/holdings/${h.id}/move`, {
      data: { to_collection_id: null },
    });
    expect(move.status(), "release holding").toBe(200);
  }
}

async function needsRows(
  request: APIRequestContext,
  collectionId: string,
): Promise<{ oracle_id: string; desired: number }[]> {
  const res = await request.get(`/api/collections/${collectionId}/needs`);
  expect(res.status()).toBe(200);
  return ((await res.json()) as { rows: { oracle_id: string; desired: number }[] })
    .rows;
}

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

test.describe("Undo (the delete toast)", () => {
  test("puts the collection, its child, its cards and its explicitly-relocated want back @fast", async ({
    page,
    request,
  }) => {
    const [card] = await unownedCards(request, 1);
    const elsewhere = await createCollection(page, {
      name: scratchName("undo-else"),
    });
    const parent = await createCollection(page, { name: scratchName("undo-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("undo-kid"),
    });
    let released = false;
    try {
      await addHave(request, parent.id, card.printing_id as string, 2);
      await addWant(request, parent.id, card.oracle_id, 3);

      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, parent.id);
      await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();

      const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
      // Wants: explicit pick, not the "Remove from Collection" default — this
      // is the path the receipt didn't used to carry a handle for at all
      // (P6-188's own Findings: "step 5's undo cannot reverse
      // WantDisposition::To"), so it's the one this task's Undo has to prove.
      await dialog.getByTestId("delete-wants-trigger").click();
      const wantsList = page.locator("#popover-tree-delete-wants");
      await wantsList
        .getByTestId("destination-option")
        .filter({ hasText: elsewhere.name })
        .click();
      await expect(dialog.getByTestId("delete-wants-trigger")).toContainText(
        elsewhere.name,
      );
      await dialog.locator("#tree-delete-confirm").click();

      // The delete landed: parent gone, child up a level, card in the Inbox,
      // want attached to `elsewhere`.
      await expect(page.locator(`li[data-tree-row="${parent.id}"]`)).toHaveCount(0);
      const inboxId = (await fetchTree(page)).find((r) => r.summary.is_inbox)!
        .summary.id;
      await expect
        .poll(async () => await copiesIn(request, card.oracle_id, inboxId))
        .toBe(2);
      await expect
        .poll(async () => {
          const rows = await needsRows(request, elsewhere.id);
          return rows.find((r) => r.oracle_id === card.oracle_id)?.desired;
        })
        .toBe(3);

      // The undo toast, with an Undo action.
      const toast = page.locator('[data-name="Toast"]', {
        hasText: `Deleted ${parent.name}`,
      });
      await expect(toast).toBeVisible();
      await toast.getByRole("button", { name: "Undo" }).click();

      // Everything is back: the collection, its child under it, the card at
      // its original quantity, the want back on `parent` and gone from
      // `elsewhere`.
      await expect
        .poll(async () =>
          (await fetchTree(page)).some((r) => r.summary.id === parent.id),
        )
        .toBe(true);
      await expect
        .poll(async () =>
          (await fetchTree(page)).find((r) => r.summary.id === child.id)?.summary
            .parent_id,
        )
        .toBe(parent.id);
      await expect
        .poll(async () => await copiesIn(request, card.oracle_id, parent.id))
        .toBe(2);
      await expect
        .poll(async () => await copiesIn(request, card.oracle_id, inboxId))
        .toBe(0);
      await expect
        .poll(async () => {
          const rows = await needsRows(request, parent.id);
          return rows.find((r) => r.oracle_id === card.oracle_id)?.desired;
        })
        .toBe(3);
      const elsewhereNeeds = await needsRows(request, elsewhere.id);
      expect(elsewhereNeeds.some((r) => r.oracle_id === card.oracle_id)).toBe(false);

      // Fixture hygiene: give the card back rather than letting a relocating
      // cleanup delete strand it "owned" in the Inbox permanently.
      await releaseHolding(request, card.oracle_id, parent.id);
      released = true;
    } finally {
      if (!released) await releaseHolding(request, card.oracle_id, parent.id);
      await deleteCollection(page, child.id);
      await deleteCollection(page, parent.id);
      await deleteCollection(page, elsewhere.id);
    }
  });
});

test.describe("Restore (the Recently deleted list)", () => {
  test("lists the row and reattaches to a live parent, leaving cards and children where they now are @fast", async ({
    page,
    request,
  }) => {
    const [card] = await unownedCards(request, 1, 1);
    const grandparent = await createCollection(page, {
      name: scratchName("res-gp"),
    });
    const subject = await createCollection(page, {
      parent_id: grandparent.id,
      name: scratchName("res-subj"),
    });
    const kid = await createCollection(page, {
      parent_id: subject.id,
      name: scratchName("res-kid"),
    });
    try {
      await addHave(request, subject.id, card.printing_id as string, 1);

      await page.goto("/my");
      await hydrated(page);
      const menu = await openRowMenu(page, subject.id);
      await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();
      const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
      await dialog.locator("#tree-delete-confirm").click(); // defaults: ToParent, Discard
      await expect(page.locator(`li[data-tree-row="${subject.id}"]`)).toHaveCount(0);

      // `subject`'s own child re-parented up to `grandparent`, and its card
      // relocated to `grandparent` (`ToParent`'s default) — this is the state
      // Restore must *not* undo.
      await expect
        .poll(async () =>
          (await fetchTree(page)).find((r) => r.summary.id === kid.id)?.summary
            .parent_id,
        )
        .toBe(grandparent.id);
      await expect
        .poll(async () => await copiesIn(request, card.oracle_id, grandparent.id))
        .toBe(1);

      // The "Recently deleted" list shows it — name, kind, deleted-when.
      // Located by id, not by name: this page has no purge, so a soft-deleted
      // row from an earlier local run of this same file (this suite's own
      // deterministic, non-wall-clock `scratchName`) can share this run's
      // exact name — id is the only thing that's actually unique.
      await page.goto("/my/recently-deleted");
      await hydrated(page);
      const row = page.locator(
        `[data-testid="recently-deleted-row"][data-collection-id="${subject.id}"]`,
      );
      await expect(row).toBeVisible();
      await expect(row).toContainText(subject.name);
      await expect(row).toContainText("Binder");
      await expect(row.getByTestId("recently-deleted-when")).not.toHaveText("");

      await row.getByTestId("recently-deleted-restore").click();
      await expect(row).toHaveCount(0); // the list drops it once restored

      // Restored, re-attached to its still-live parent…
      await expect
        .poll(async () =>
          (await fetchTree(page)).find((r) => r.summary.id === subject.id)?.summary
            .parent_id,
        )
        .toBe(grandparent.id);
      // …but its child stays exactly where the delete left it — Restore does
      // not reverse the re-parent, only undo does.
      const after = await fetchTree(page);
      expect(after.find((r) => r.summary.id === kid.id)?.summary.parent_id).toBe(
        grandparent.id,
      );
      // …and its card stays at the destination the delete sent it to.
      expect(await copiesIn(request, card.oracle_id, grandparent.id)).toBe(1);
      expect(await copiesIn(request, card.oracle_id, subject.id)).toBe(0);
    } finally {
      await deleteCollection(page, kid.id);
      await deleteCollection(page, subject.id);
      await deleteCollection(page, grandparent.id);
    }
  });

  test("falls back to top level when the original parent is no longer live @fast", async ({
    page,
  }) => {
    const grandparent = await createCollection(page, {
      name: scratchName("res-deadgp"),
    });
    const subject = await createCollection(page, {
      parent_id: grandparent.id,
      name: scratchName("res-deadsubj"),
    });
    try {
      // Hide `subject` first, then its own parent — so when `subject` is
      // restored, `grandparent` (read fresh, not from a stale snapshot) is
      // itself hidden.
      await page.request.post(`/api/collections/${subject.id}/delete`, {
        data: {},
      });
      await page.request.post(`/api/collections/${grandparent.id}/delete`, {
        data: {},
      });

      await page.goto("/my/recently-deleted");
      await hydrated(page);
      // Located by id — see the sibling test's comment above for why.
      const row = page.locator(
        `[data-testid="recently-deleted-row"][data-collection-id="${subject.id}"]`,
      );
      await expect(row).toBeVisible();
      await row.getByTestId("recently-deleted-restore").click();
      await expect(row).toHaveCount(0);

      await expect
        .poll(async () =>
          (await fetchTree(page)).find((r) => r.summary.id === subject.id)?.summary
            .parent_id,
        )
        .toBe(null);
    } finally {
      // `subject` is live again (restored); `grandparent` is still hidden and
      // is left that way — the accepted debris pattern this suite already
      // follows for every other file's soft-deleted scratch rows.
      await page.request.post(`/api/collections/${subject.id}/delete`, {
        data: {},
      });
    }
  });
});
