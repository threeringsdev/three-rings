import { expect, test, type Page } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// Read-only collection tree (specs/app-ui.md "Collection tree", read-only
// half; design/information-architecture.md → My cards mode).
//
// The contract, in the order asserted below:
//   /my SSRs the tree server-side · All cards and Shopping list are pinned
//   Item links with badges · Inbox is the first tree row · badges roll up
//   (parent = own + descendants; All cards = every collection's own count;
//   Shopping list = the server's distinct-cards-short count) · a chevron
//   collapses its subtree (data-state + inert — links become unreachable) ·
//   the row matching the URL carries aria-current=page · rows navigate ·
//   the mobile My-cards tab badge mirrors the Inbox rollup.
//
// Counts are computed from GET /api/collection_tree (the page's own read
// adapter) rather than hardcoded — sibling specs mutate holdings on the dev
// branch, so absolute numbers drift between runs.

type TreeRow = {
  summary: {
    id: string;
    parent_id: string | null;
    name: string;
    is_inbox: boolean;
  };
  present: number;
};
type TreeDto = { collections: TreeRow[]; shopping_short: number };

async function fetchTree(page: Page): Promise<TreeDto> {
  const resp = await page.request.get("/api/collection_tree");
  expect(resp.ok()).toBeTruthy();
  return (await resp.json()) as TreeDto;
}

/// Own present + every descendant's — the badge the UI must show for a node.
function rollup(dto: TreeDto, id: string): number {
  const own = dto.collections.find((r) => r.summary.id === id)?.present ?? 0;
  return dto.collections
    .filter((r) => r.summary.parent_id === id)
    .reduce((sum, r) => sum + rollup(dto, r.summary.id), own);
}

/// A row's *own* badge — `.first()` because a parent's `<li>` also contains
/// its children's badges further down.
function rowBadge(page: Page, id: string) {
  return page
    .locator(`li[data-tree-row="${id}"] [data-name="Badge"]`)
    .first();
}

test.describe("signed in", () => {
  test.use({ storageState: AUTH_STATE });

  test("/my SSRs the tree server-side @fast", async ({ page }) => {
    // Request-level: the markup must be in the raw response, before any JS.
    const raw = await (await page.request.get("/my")).text();
    expect(raw).toContain('aria-label="Collections"');
    expect(raw).toContain("All cards");
    expect(raw).toContain("Shopping list");
    expect(raw).toContain("data-tree-row");
  });

  test("pinned rows and rolled-up badges @fast", async ({ page }) => {
    const dto = await fetchTree(page);
    await page.goto("/my");
    await hydrated(page);
    const nav = page.locator('nav[aria-label="Collections"]');

    // All cards: pinned link to /my carrying the everything-total.
    const total = dto.collections.reduce((s, r) => s + r.present, 0);
    const allCards = nav.locator("a", { hasText: "All cards" });
    await expect(allCards).toHaveAttribute("href", "/my");
    await expect(allCards.locator('[data-name="Badge"]')).toHaveText(
      String(total),
    );

    // Inbox: a real collection, pinned first among the tree rows.
    const inbox = dto.collections.find((r) => r.summary.is_inbox);
    expect(inbox).toBeTruthy();
    await expect(nav.locator("li[data-tree-row]").first()).toHaveAttribute(
      "data-tree-row",
      inbox!.summary.id,
    );

    // Every row's badge is its rollup — the parent rows are the ones that
    // can get this wrong, so assert all of them. Skip the `zz-e2e-` scratch
    // collections the management spec creates/deletes in parallel against the
    // same dev user: `fetchTree` (a live DB read) picks them up, but they may
    // vanish mid-assertion. They carry zero cards, so they never alter a seed
    // row's rollup — ignoring them keeps this test isolated from that churn.
    for (const row of dto.collections) {
      if (row.summary.name.startsWith("zz-e2e-")) continue;
      await expect(rowBadge(page, row.summary.id)).toHaveText(
        String(rollup(dto, row.summary.id)),
      );
    }

    // Shopping list: pinned link to /my/shopping with the short-card badge.
    const shopping = nav.locator("a", { hasText: "Shopping list" });
    await expect(shopping).toHaveAttribute("href", "/my/shopping");
    await expect(shopping.locator('[data-name="Badge"]')).toHaveText(
      String(dto.shopping_short),
    );
  });

  test("chevron collapses the subtree @fast", async ({ page }) => {
    const dto = await fetchTree(page);
    // Prefer a *seed* nested pair, never a `zz-e2e-` scratch one the
    // management spec may delete mid-test (see the rollup test's note).
    const parent = dto.collections.find(
      (r) =>
        !r.summary.name.startsWith("zz-e2e-") &&
        dto.collections.some(
          (c) =>
            c.summary.parent_id === r.summary.id &&
            !c.summary.name.startsWith("zz-e2e-"),
        ),
    );
    expect(parent, "seed data must include a nested collection").toBeTruthy();
    const parentId = parent!.summary.id;
    const child = dto.collections.find(
      (c) =>
        c.summary.parent_id === parentId &&
        !c.summary.name.startsWith("zz-e2e-"),
    )!;

    await page.goto("/my");
    await hydrated(page);

    const content = page.locator(`#tree-children-${parentId}`);
    const trigger = page.locator(
      `button[aria-controls="tree-children-${parentId}"]`,
    );
    const childLink = page.locator(
      `li[data-tree-row="${child.summary.id}"] a`,
    );

    // Nesting: the child renders inside the parent's collapsible panel.
    await expect(content.locator(`li[data-tree-row="${child.summary.id}"]`))
      .toHaveCount(1);
    await expect(content).toHaveAttribute("data-state", "open");
    await expect(trigger).toHaveAttribute("aria-expanded", "true");

    // Collapse: state flips and the subtree's links become unreachable.
    // (A closed panel keeps its DOM — assert data-state + inert, not
    // visibility; see the e2e-suite skill's "assertions that lie".)
    await trigger.click();
    await expect(content).toHaveAttribute("data-state", "closed");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(await content.evaluate((el) => el.inert)).toBe(true);

    // Re-open: the child is clickable again.
    await trigger.click();
    await expect(content).toHaveAttribute("data-state", "open");
    expect(await content.evaluate((el) => el.inert)).toBe(false);
    await expect(childLink).toHaveAttribute(
      "href",
      `/my/collections/${child.summary.id}`,
    );
  });

  test("the row matching the URL is selected @fast", async ({ page }) => {
    const dto = await fetchTree(page);
    const inbox = dto.collections.find((r) => r.summary.is_inbox)!;

    await page.goto(`/my/collections/${inbox.summary.id}`);
    await hydrated(page);
    const nav = page.locator('nav[aria-label="Collections"]');
    // Exactly one selected row in the tree, and it is the URL's.
    await expect(nav.locator('a[aria-current="page"]')).toHaveCount(1);
    await expect(
      nav.locator(
        `li[data-tree-row="${inbox.summary.id}"] a[aria-current="page"]`,
      ),
    ).toHaveCount(1);

    // A collection's own subpage keeps its row selected — you are still
    // operating in that collection (Codex review, this task).
    await page.goto(`/my/collections/${inbox.summary.id}/needs`);
    await hydrated(page);
    await expect(
      nav.locator(
        `li[data-tree-row="${inbox.summary.id}"] a[aria-current="page"]`,
      ),
    ).toHaveCount(1);

    // On /my the pinned All-cards row is the selected one instead.
    await page.goto("/my");
    await hydrated(page);
    const selected = nav.locator('a[aria-current="page"]');
    await expect(selected).toHaveCount(1);
    await expect(selected).toHaveText(/All cards/);
  });

  test("clicking a tree row navigates to its collection @fast", async ({
    page,
  }) => {
    const dto = await fetchTree(page);
    const inbox = dto.collections.find((r) => r.summary.is_inbox)!;
    await page.goto("/my");
    await hydrated(page);
    await page
      .locator(`li[data-tree-row="${inbox.summary.id}"] a`)
      .first()
      .click();
    await page.waitForURL(`/my/collections/${inbox.summary.id}`);
  });

  test.describe("mobile", () => {
    test.use({ viewport: { width: 390, height: 844 } });

    test("the My-cards tab badge mirrors the Inbox rollup @fast", async ({
      page,
    }) => {
      const dto = await fetchTree(page);
      const inbox = dto.collections.find((r) => r.summary.is_inbox)!;
      const count = rollup(dto, inbox.summary.id);
      // A zero Inbox would make the badge (correctly) absent and this test
      // vacuous — fail loudly so the seed drift is fixed instead.
      expect(count, "seeded Inbox must hold cards").toBeGreaterThan(0);

      // The badge shows in *both* modes — assert from Catalog mode.
      await page.goto("/catalog");
      await hydrated(page);
      await expect(
        page.locator(
          'nav[aria-label="Primary"] a[href="/my"] [data-name="Badge"]',
        ),
      ).toHaveText(String(count));
    });
  });
});

test.describe("anonymous", () => {
  test("no tree fetch, catalog rail unchanged @fast", async ({ page }) => {
    const treeRequests: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("collection_tree")) treeRequests.push(r.url());
    });
    await page.goto("/catalog");
    await hydrated(page);
    // The anonymous shell must skip the session read entirely (no 401 noise)…
    expect(treeRequests).toEqual([]);
    // …and the Catalog-mode rail still carries the filter rail.
    await expect(
      page.locator('aside[aria-label="Sidebar"]'),
    ).toBeVisible();
    await expect(
      page.locator('nav[aria-label="Collections"]'),
    ).toHaveCount(0);
  });
});
