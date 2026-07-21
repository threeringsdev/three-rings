import { expect, test, type Page } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// Collection-tree management (specs/app-ui.md "Collection tree", management
// half; design/information-architecture.md → "Tree management … happens in
// place via context menus"). The contract, in the order asserted below:
//
//   right-click a row opens the shared context menu · the Inbox row offers
//   create only (no rename/delete — it is protected) · "New binder/deck
//   inside…" and the background "New … " open a create dialog that adds a
//   child/root · Rename edits a name in place · Delete confirms with the
//   cascade's counts, then removes the subtree · drag drops reparent (into a
//   row) and reorder (onto a row's edge band) · a drop onto the node's own
//   descendant is refused (the client cycle pre-check), and the server is the
//   backstop (409).
//
// These tests MUTATE the Neon dev branch, so every one creates its own
// uniquely-named scratch collections via the API and deletes them in a
// `finally`. Deleting a parent cascades its subtree (FK ON DELETE CASCADE),
// so cleanup is one delete per root created.

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

async function deleteCollection(page: Page, id: string): Promise<void> {
  await page.request.post(`/api/collections/${id}/delete`);
}

async function fetchTree(page: Page): Promise<TreeRow[]> {
  const resp = await page.request.get("/api/collection_tree");
  expect(resp.ok()).toBeTruthy();
  return ((await resp.json()) as { collections: TreeRow[] }).collections;
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
      await deleteCollection(page, parent.id); // cascades the child
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

test("Delete confirms with the cascade counts, then removes the subtree @fast", async ({
  page,
}) => {
  const parent = await createCollection(page, { name: scratchName("del-par") });
  await createCollection(page, { parent_id: parent.id, name: scratchName("del-kid") });
  let deleted = false;
  try {
    await page.goto("/my");
    await hydrated(page);
    const menu = await openRowMenu(page, parent.id);
    await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();

    const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
    // The confirm names the cascade: the one nested collection.
    await expect(dialog).toContainText("1 nested collection");
    await expect(dialog).toContainText("cannot be undone");
    await dialog.locator("#tree-delete-confirm").click();

    // The row is gone from the tree and the server.
    await expect(page.locator(`li[data-tree-row="${parent.id}"]`)).toHaveCount(0);
    await expect
      .poll(async () =>
        (await fetchTree(page)).some((r) => r.summary.id === parent.id),
      )
      .toBe(false);
    deleted = true;
  } finally {
    if (!deleted) await deleteCollection(page, parent.id);
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
      await deleteCollection(page, parent.id); // cascades child
    }
  });
});
