// `/my` on a phone — the My-cards root drill-down list
// (design/wireframes.pen → "Mobile — My cards root", 390×844).
//
// The load-bearing contracts, in assertion order:
//
// - at 390 px `/my` is the *list* and not the All-cards table, and at desktop
//   width it is still the table — the switch is CSS over one SSR'd markup, so
//   every assertion here reads computed visibility rather than DOM presence;
// - the list is the sidebar's top level: same rows, same order (Inbox pinned
//   first), same rolled-up counts, cross-checked against the tree API — and
//   nested collections are absent, because you reach those by drilling in;
// - the aggregate table is still reachable on touch (`/my/all`) and its up-link
//   walks back to the root screen — "navigation collapses, features don't";
// - a real tap on a row drills in, and the collection view's back link walks
//   back up to the root list;
// - the rail drawer still opens on touch and still carries the tree's row
//   actions, because it is the only touch path to create / rename / move /
//   delete (see the shell's own comment). It must stay *closed* until asked —
//   asserted on `left`, not on visibility, since a slid-out panel is "visible"
//   to Playwright;
// - nothing on the screen scrolls sideways at 390 px, measured on the scroll
//   containers rather than the document (specs/app-ui.md:1198).
//
// **Every negative assertion here has a positive control on the same page.**
// "The table is not on mobile" passes on a blank page, so each absence is paired
// with the presence of the thing that replaced it.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const PHONE = { width: 390, height: 844 };

const LIST = '[data-testid="my-root-list"]';
const ROW = '[data-testid="my-root-row"]';
const TABLE = '[data-testid="all-cards-table"]';

type Summary = {
  id: string;
  parent_id: string | null;
  name: string;
  is_inbox: boolean;
};
type TreeRow = { summary: Summary; present: number };
type Tree = { collections: TreeRow[]; shopping_short: number };

async function fetchTree(request: APIRequestContext): Promise<Tree> {
  const res = await request.get("/api/collection_tree");
  expect(res.ok(), `tree read failed: ${res.status()}`).toBeTruthy();
  return res.json();
}

/// The rolled-up present count a row's badge shows: own copies plus every
/// descendant's. Computed here from the flat read so the expectation does not
/// come from the same projection the page renders.
function rollup(tree: Tree, id: string): number {
  const kids = tree.collections.filter((r) => r.summary.parent_id === id);
  return (
    tree.collections.find((r) => r.summary.id === id)!.present +
    kids.reduce((n, k) => n + rollup(tree, k.summary.id), 0)
  );
}

/// The top-level rows the list must show, in the order it must show them:
/// server order with the Inbox lifted to the front (app/src/my/tree.rs →
/// `assemble`).
function expectedRoots(tree: Tree): Summary[] {
  const roots = tree.collections
    .filter((r) => r.summary.parent_id === null)
    .map((r) => r.summary);
  const i = roots.findIndex((s) => s.is_inbox);
  if (i > 0) roots.unshift(roots.splice(i, 1)[0]);
  return roots;
}

/// Rows as `label` strings, in DOM order.
async function labels(page: Page): Promise<string[]> {
  return page.locator(`${ROW} span:nth-child(2)`).allInnerTexts();
}

test.describe("mobile", () => {
  test.use({ viewport: PHONE, hasTouch: true });

  test("@fast /my is the collection list, not the card table", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);

    // The positive control comes first: the list is what a phone gets.
    await expect(page.locator(LIST)).toBeVisible();
    await expect(page.locator('[data-testid="my-root"] h1')).toHaveText(
      "My cards",
    );
    // Only then is the absence of the table meaningful. It is in the DOM (one
    // markup at every width, CSS picks) — `toBeHidden` reads the computed
    // `display`, which is the thing under test.
    await expect(page.locator(TABLE)).toHaveCount(1);
    await expect(page.locator(TABLE)).toBeHidden();
    await expect(page.locator('#my-query')).toBeHidden();
  });

  test("@fast the list is the sidebar's top level — rows, order and counts", async ({
    page,
    request,
  }) => {
    // Read the API and the page together and retry the pair: other specs write
    // to this same dev user in parallel workers (see all-cards.spec.ts).
    await expect(async () => {
      const tree = await fetchTree(request);
      const roots = expectedRoots(tree);
      expect(roots.length, "seed must have top-level collections").toBeGreaterThan(1);
      expect(roots[0].is_inbox, "the Inbox is pinned first").toBeTruthy();

      await page.goto("/my");
      await hydrated(page);

      expect(await labels(page)).toEqual([
        "All cards",
        ...roots.map((s) => s.name),
        "Shopping list",
      ]);

      // Counts agree with the tree read, per row and for the aggregate.
      const total = tree.collections.reduce((n, r) => n + r.present, 0);
      expect(total, "seeded collections must hold cards").toBeGreaterThan(0);
      const counts = await page
        .locator('[data-testid="my-root-count"]')
        .allInnerTexts();
      expect(counts).toEqual([
        String(total),
        ...roots.map((s) => String(rollup(tree, s.id))),
        String(tree.shopping_short),
      ]);
    }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
  });

  test("@fast nested collections are reached by drilling in, not listed", async ({
    page,
    request,
  }) => {
    const tree = await fetchTree(request);
    const nested = tree.collections.filter((r) => r.summary.parent_id !== null);
    // Without a nested collection in the fixture this test asserts nothing.
    expect(nested.length, "seed must nest something").toBeGreaterThan(0);

    await page.goto("/my");
    await hydrated(page);
    const shown = await labels(page);
    for (const r of nested) {
      expect(
        shown,
        `${r.summary.name} is nested and belongs one level down`,
      ).not.toContain(r.summary.name);
    }
    // Positive control: a top-level *parent* of one of them is on the list, so
    // the absences above are about depth and not about an empty page. (The seed
    // nests three deep — `Depth Box → Depth Shelf → Depth Drawer` — so the
    // parent has to be picked, not assumed to be a root.)
    const byId = new Map(tree.collections.map((r) => [r.summary.id, r.summary]));
    const parents = nested
      .map((r) => byId.get(r.summary.parent_id!)!)
      .filter((s) => s.parent_id === null);
    expect(parents.length, "seed must nest under a top-level collection").toBeGreaterThan(0);
    expect(shown).toContain(parents[0].name);
  });

  test("@fast tapping a collection drills in and back walks up to the root", async ({
    page,
    request,
  }) => {
    const tree = await fetchTree(request);
    const inbox = tree.collections.find((r) => r.summary.is_inbox)!;

    await page.goto("/my");
    await hydrated(page);
    await page
      .locator(`${ROW}[data-collection="${inbox.summary.id}"]`)
      .tap();
    await page.waitForURL(`/my/collections/${inbox.summary.id}`);
    await hydrated(page);
    await expect(page.locator('[data-testid="collection-title"]')).toHaveText(
      inbox.summary.name,
    );

    // The collection view's back link is the other half of the drill-down: at
    // the top level it names the screen it returns to, which is this list.
    const back = page.locator('[data-testid="collection-back"]');
    await expect(back).toBeVisible();
    await expect(back).toContainText("My cards");
    await back.tap();
    await page.waitForURL("/my");
    await hydrated(page);
    await expect(page.locator(LIST)).toBeVisible();
  });

  test("@fast the All-cards table is still reachable on touch", async ({
    page,
  }) => {
    // "Navigation collapses, features don't": the aggregate view is a feature,
    // so a phone must be able to get to it — one tap from the root.
    await page.goto("/my");
    await hydrated(page);
    const all = page.locator(`${ROW}`).first();
    await expect(all).toHaveAttribute("href", "/my/all");
    await all.tap();
    await page.waitForURL("/my/all");
    await hydrated(page);

    // Here the table *is* what a phone gets, and its query bar with it.
    await expect(page.locator(TABLE)).toBeVisible();
    await expect(page.locator('#my-query')).toBeVisible();
    await expect(page.locator(LIST)).toHaveCount(0);

    // And its own up-link walks back to the root screen.
    const back = page.locator('[data-testid="all-cards-back"]');
    await expect(back).toBeVisible();
    await back.tap();
    await page.waitForURL("/my");
    await expect(page.locator(LIST)).toBeVisible();
  });

  test("@fast a search deep-linked at /my is one tap from its results", async ({
    page,
  }) => {
    // `?q=` on a phone lands on the list (the table is a route down), so the
    // aggregate row carries the query rather than dropping it.
    await page.goto("/my?q=bolt");
    await hydrated(page);
    const all = page.locator(ROW).first();
    await expect(all).toHaveAttribute("href", "/my/all?q=bolt");
    await all.tap();
    await page.waitForURL("/my/all?q=bolt");
    await hydrated(page);
    await expect(page.locator('#my-query')).toHaveValue("bolt");
  });

  test("@fast the rail drawer stays shut and still opens the tree", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);

    const rail = page.locator("#sidebar-rail");
    // NOT `toBeVisible`: the drawer is a `fixed` panel slid off screen by
    // `left`, and an off-screen element reads as visible to Playwright. The
    // computed `left` is the state.
    const closedLeft = await rail.evaluate(
      (el) => getComputedStyle(el).left,
    );
    expect(closedLeft, "the drawer must start off screen").toBe("-240px");
    expect(await rail.evaluate((el) => el.getAttribute("data-open"))).toBeNull();

    // It is still the only touch path to the tree's create/rename/move/delete
    // menu, so it must still open — the list replaced its navigation job, not
    // its management job.
    await page.locator('[data-testid="rail-toggle"]').tap();
    await expect(rail).toHaveAttribute("data-open", "true");
    await expect(async () => {
      expect(await rail.evaluate((el) => getComputedStyle(el).left)).toBe("0px");
    }).toPass({ timeout: 3000 });
    await expect(
      page.locator("[data-tree-row-actions]").first(),
    ).toBeVisible();

    // A navigation is a dismissal (shell.rs), and the list is still underneath.
    // Tap the scrim to the *right* of the 240 px panel: the scrim spans the
    // viewport and its own centre is underneath the open drawer, which is
    // `z-50` and swallows the tap (Playwright reports the interception).
    await page
      .locator('[data-testid="rail-scrim"]')
      .tap({ position: { x: 330, y: 300 } });
    await expect(rail).not.toHaveAttribute("data-open", "true");
    await expect(page.locator(LIST)).toBeVisible();
  });

  test("@fast nothing scrolls sideways at 390 px", async ({ page }) => {
    for (const url of ["/my", "/my/all"]) {
      await page.goto(url);
      await hydrated(page);

      // Measure the scroll containers, not just the document: an
      // `overflow-auto` wrapper absorbs its own overflow and the document
      // never moves (specs/app-ui.md:1198 — that mistake hid 92–128 px).
      const worst = await page.evaluate(() => {
        const out: { where: string; overflow: number; client: number }[] = [];
        const els = [
          document.documentElement,
          ...document.querySelectorAll<HTMLElement>(
            '[data-testid="my-root-list"], [data-name="TableWrapper"], main',
          ),
        ];
        for (const el of els) {
          if (!el.clientWidth) continue;
          out.push({
            where: el.tagName + (el.getAttribute("data-testid") ?? ""),
            overflow: el.scrollWidth - el.clientWidth,
            client: el.clientWidth,
          });
        }
        return out;
      });
      expect(worst.length, `${url} had no measurable container`).toBeGreaterThan(1);
      for (const m of worst) {
        expect(
          m.overflow,
          `${url}: ${m.where} scrolls sideways (${m.overflow}px)`,
        ).toBeLessThanOrEqual(1);
      }
    }
  });

  test("@fast rows are full-width 44 px touch targets", async ({ page }) => {
    await page.goto("/my");
    await hydrated(page);
    const rows = page.locator(ROW);
    const n = await rows.count();
    expect(n).toBeGreaterThan(2);
    for (let i = 0; i < n; i++) {
      const box = (await rows.nth(i).boundingBox())!;
      expect(box, `row ${i} has no box`).toBeTruthy();
      expect(box.height, `row ${i} is under the 44px target`).toBeGreaterThanOrEqual(
        44,
      );
      // The frame's rows fill the list's width (padding 8 px each side).
      expect(box.width).toBeGreaterThan(PHONE.width - 40);
    }
  });
});

test.describe("desktop", () => {
  test("@fast /my is still the All-cards table, and the list is not shown", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    // Positive control first: the shipped desktop landing is unchanged.
    await expect(page.locator(TABLE)).toBeVisible();
    await expect(page.locator('#my-query')).toBeVisible();
    await expect(page.locator("h1:visible")).toHaveText("All cards");
    // The mobile list is in this document too, and hidden.
    await expect(page.locator(LIST)).toHaveCount(1);
    await expect(page.locator(LIST)).toBeHidden();
    // The sidebar tree is the desktop navigation, and still is.
    await expect(page.locator('nav[aria-label="Collections"]')).toBeVisible();
  });

  test("@fast /my keeps building its own URLs; /my/all keeps its own", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    await expect(page.locator('[data-testid="page-next"]')).toHaveAttribute(
      "href",
      /^\/my\?cursor=/,
    );

    await page.goto("/my/all");
    await hydrated(page);
    await expect(page.locator('[data-testid="page-next"]')).toHaveAttribute(
      "href",
      /^\/my\/all\?cursor=/,
    );
    // The up-link exists but is a phone affordance only.
    await expect(page.locator('[data-testid="all-cards-back"]')).toBeHidden();
    // …and the rail still marks All cards as where you are: the table has two
    // routes now, and the sidebar is on screen at both.
    await expect(
      page.locator('nav[aria-label="Collections"] a[href="/my"]'),
    ).toHaveAttribute("aria-current", "page");
  });
});
