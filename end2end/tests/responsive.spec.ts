// Responsive audit (specs/TODO.md Phase 5 stage 3; design/wireframes.pen's nine
// frames at their own widths). What this file pins is the class of defect the
// audit found: chrome that is `fixed` to the viewport and therefore invisible to
// every layout the document can express, and touch targets that measure fine in
// the abstract and are 16 px on the device.
//
// Three rules this file follows, each because the alternative shipped once:
//
//   * **Geometry, never `toBeVisible()`.** A `Sheet` keeps its box when closed,
//     the rail drawer is `invisible`, and the tree row's `⋯` is `opacity-0` —
//     all three read as *visible* to Playwright (e2e-suite skill).
//   * **Measure the control, not its cell.** The whole point of the checkbox
//     finding is that the cell was 32 px while the thing you could hit was 16.
//   * **Every negative assertion carries a positive control.** "The toast does
//     not cover the tray" passes trivially if the tray never rendered.
//
// Read-only: nothing here writes to the dev user.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

const DESKTOP = { width: 1440, height: 900 };
const PHONE = { width: 390, height: 844 };

/// The sidebar rail's width at `md`+ (`w-60`). The tray dock's `md:left-60`
/// offset is this number, and the content column starts here.
const RAIL = 240;
/// The touch-target floor the repo builds to (`size-11`).
const TAP = 44;

const TRAY = '[data-testid="selection-tray"]';
const TOASTER = '[data-name="Toaster"]';
const TABS = 'nav[aria-label="Primary"]';
const SELECT_TARGET = '[data-testid="row-select-target"]';
const SELECT_BOX = '[data-testid="row-select"]';

type TreeRow = {
  summary: { id: string; name: string; kind: string; parent_id: string | null };
  present: number;
};

async function tree(request: APIRequestContext): Promise<TreeRow[]> {
  const res = await request.get("/api/collections/tree");
  expect(res.status()).toBe(200);
  return ((await res.json()) as { collections: TreeRow[] }).collections;
}

/// Collections that actually hold copies of their own — the only ones with a
/// selectable row (a desire-only row renders no checkbox at all).
async function holders(request: APIRequestContext): Promise<TreeRow[]> {
  const rows = (await tree(request)).filter((r) => r.present > 0);
  expect(rows.length, "dev seed should carry collections holding cards").toBeGreaterThan(0);
  return rows;
}

/// `getBoundingClientRect` for one element, as plain numbers.
async function rect(page: Page, selector: string, nth = 0) {
  const box = await page.locator(selector).nth(nth).evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height, top: r.top, bottom: r.bottom, left: r.left, right: r.right };
  });
  return box;
}

// ------------------------------------------------------------ tap targets ---

test.describe("tap targets at phone width", () => {
  test.use({ storageState: AUTH_STATE, viewport: PHONE, hasTouch: true });

  for (const surface of ["collection", "all-cards"] as const) {
    test(`@fast the ${surface} row's select target is ${TAP} px and its box stays 16 px`, async ({
      page,
      request,
    }) => {
      const url =
        surface === "collection"
          ? `/my/collections/${(await holders(request))[0].summary.id}`
          : "/my/all";
      await page.goto(url);
      await hydrated(page);
      await page.locator(SELECT_TARGET).first().waitFor();

      // The *control's own box*, not the cell's: the cell was already 32 px
      // wide when the thing you could hit was 16.
      const target = await rect(page, SELECT_TARGET);
      expect(target.w, "select tap target width").toBeGreaterThanOrEqual(TAP);
      expect(target.h, "select tap target height").toBeGreaterThanOrEqual(TAP);

      // …and the visual checkbox is still the wireframe's small box. Without
      // this, "make it 44 px" is satisfied by a 44 px checkbox, which is a
      // different (and wrong) change.
      const box = await rect(page, SELECT_BOX);
      expect(box.w, "the drawn checkbox should stay small").toBeLessThanOrEqual(20);
      expect(box.h, "the drawn checkbox should stay small").toBeLessThanOrEqual(20);

      // The hazard that made 16 px worse than merely small: the next cell is
      // the card-detail link, so a miss navigated away. The target must not
      // reach it.
      const link = await rect(
        page,
        `${surface === "collection" ? '[data-testid="collection-row"]' : '[data-testid="all-cards-row"]'} a[href^="/cards/"]`,
      );
      expect(
        link.left,
        "the select target must not overlap the card-detail link",
      ).toBeGreaterThanOrEqual(target.right);
    });
  }

  test("@fast a tap on the target's outer ring selects, and does not navigate", async ({
    page,
    request,
  }) => {
    const held = (await holders(request))[0];
    await page.goto(`/my/collections/${held.summary.id}`);
    await hydrated(page);
    await page.locator(SELECT_TARGET).first().waitFor();
    await expect(page.locator(TRAY)).toHaveCount(0);

    // 3 px inside the target's top-left corner — inside the padded hit area,
    // well outside the 16 px box. This is the assertion that makes the size
    // check mean something: a 44 px box that only responds in the middle would
    // pass the measurement and fail here.
    const target = await rect(page, SELECT_TARGET);
    const box = await rect(page, SELECT_BOX);
    expect(
      box.left - target.left,
      "the corner must be outside the drawn box",
    ).toBeGreaterThan(3);
    await page.touchscreen.tap(target.left + 3, target.top + 3);

    await expect(page.locator(TRAY)).toHaveCount(1);
    // Still here. Before the fix the same coordinate was dead space, and a
    // near-miss the other way landed on the link.
    expect(new URL(page.url()).pathname).toBe(`/my/collections/${held.summary.id}`);

    // Positive control for that last claim: the neighbouring cell really does
    // respond to a tap, so "did not navigate" is a property of *where* the tap
    // landed rather than of taps in general.
    //
    // What it responds *with* is worth knowing, and it is not what the filed
    // defect assumed. On a coarse pointer `CardPreview` intercepts the tap into
    // the bottom sheet rather than following the link, so a real finger's
    // mis-tap opens a dismissible sheet — annoying, not destructive. It is the
    // *fine*-pointer 390 px case (a narrowed desktop window) that actually
    // navigates away. Both are worth avoiding; only one loses your place.
    await page
      .locator('[data-testid="collection-row"] [data-testid="card-preview-trigger"]')
      .first()
      .click();
    await expect(
      page.locator("[data-testid=card-preview-sheet][role=dialog]").first(),
    ).toHaveAttribute("data-state", "open");
    expect(new URL(page.url()).pathname).toBe(`/my/collections/${held.summary.id}`);
  });

  test("@fast the rail toggle — the only touch route into tree management — is a 44 px target", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    // A real long-press raises no `contextmenu` on the Android webview (app-ui
    // Findings), so this button is how a phone reaches create/rename/move/
    // delete at all. It was 27.8 × 26.
    const toggle = await rect(page, '[data-testid="rail-toggle"]');
    expect(toggle.w).toBeGreaterThanOrEqual(TAP);
    expect(toggle.h).toBeGreaterThanOrEqual(TAP);
  });
});

// ------------------------------------------------- fixed bottom chrome ---

test.describe("nothing docked at the bottom paints over anything else", () => {
  test.use({ storageState: AUTH_STATE });

  /// Raise the tray by selecting one row, and hand back the collection id.
  async function selectOneRow(page: Page, request: APIRequestContext) {
    const held = (await holders(request))[0];
    await page.goto(`/my/collections/${held.summary.id}`);
    await hydrated(page);
    await page.locator(SELECT_TARGET).first().waitFor();
    await page.locator(SELECT_BOX).first().click();
    await expect(page.locator(TRAY)).toHaveCount(1);
    return held;
  }

  test("@fast a toast cannot reach the tray's clear × at 1440", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(DESKTOP);
    await selectOneRow(page, request);

    const toaster = await rect(page, TOASTER);
    const tray = await rect(page, TRAY);
    const clear = await rect(page, '[data-testid="tray-clear"]');

    // Positive controls first: both boxes are real, and they overlap
    // horizontally — which is *why* the vertical relationship matters. If the
    // toaster moved to the left edge this test would stop being about anything.
    expect(tray.w, "tray pill has width").toBeGreaterThan(0);
    expect(toaster.w, "toaster has width").toBeGreaterThan(0);
    expect(
      Math.min(toaster.right, tray.right) - Math.max(toaster.left, tray.left),
      "toaster and tray share horizontal space, so height is the only separator",
    ).toBeGreaterThan(0);
    expect(clear.left, "the clear × is in the overlapping strip").toBeGreaterThan(
      toaster.left,
    );

    // Toasts stack upward from the container's bottom edge (pinned on the bench
    // below), so the container's bottom is every toast's bottom.
    expect(
      toaster.bottom,
      "a toast would paint over the tray pill",
    ).toBeLessThanOrEqual(tray.top);
  });

  test("@fast a toast clears the bottom tab bar at 390, tray or no tray", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(PHONE);
    await page.goto("/my/all");
    await hydrated(page);
    await page.locator(SELECT_TARGET).first().waitFor();

    // Resting: the tab bar is the only bottom chrome, and it is always there
    // below `md`. This is the arm that was broken *without* any selection.
    const tabs = await rect(page, TABS);
    expect(tabs.h, "bottom tabs are on screen at 390").toBeGreaterThan(0);
    expect(
      (await rect(page, TOASTER)).bottom,
      "a toast would paint over the bottom tabs",
    ).toBeLessThanOrEqual(tabs.top);

    // …and with the tray up, above the tray too.
    await page.locator(SELECT_BOX).first().click();
    await expect(page.locator(TRAY)).toHaveCount(1);
    const tray = await rect(page, TRAY);
    expect(tray.top).toBeLessThan(tabs.top); // the tray really is between them
    expect(
      (await rect(page, TOASTER)).bottom,
      "a toast would paint over the tray pill",
    ).toBeLessThanOrEqual(tray.top);
  });

  test("@fast a real toast occupies its container's bottom edge", async ({
    page,
  }) => {
    // The bridge that makes the two assertions above non-vacuous. They measure
    // the *container* (an empty `<ol>`, so height 0) rather than a toast,
    // because inducing a real toast on an authed page means performing a write.
    // What licenses that substitution is this: a toast's bottom edge is the
    // container's bottom edge, so clearing the container clears every toast
    // however tall the stack gets. Driven on the bench, which raises real
    // toasts through the real handle and needs no session.
    await page.setViewportSize(DESKTOP);
    await page.goto("/dev/components");
    await hydrated(page);

    const section = page.locator("#sonner");
    await section.scrollIntoViewIfNeeded();
    const before = await section
      .locator(TOASTER)
      .evaluate((el) => el.getBoundingClientRect().bottom);

    await section.getByRole("button", { name: "With undo action" }).click();
    const toast = section.locator('[data-name="Toast"]').first();
    await expect(toast).toHaveCount(1);

    const [liBottom, olBottom, liHeight] = await section.evaluate((root) => {
      const li = root.querySelector('[data-name="Toast"]')!.getBoundingClientRect();
      const ol = root.querySelector('[data-name="Toaster"]')!.getBoundingClientRect();
      return [li.bottom, ol.bottom, li.height];
    });
    expect(liHeight, "a real toast has real height").toBeGreaterThan(20);
    expect(Math.abs(liBottom - olBottom)).toBeLessThanOrEqual(1);
    // The empty container sat at the same edge, which is the substitution.
    expect(Math.abs(before - olBottom)).toBeLessThanOrEqual(1);
  });

  test("@fast the tray centres on the content column, not on the window", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(DESKTOP);
    await selectOneRow(page, request);

    const main = await rect(page, "main");
    const tray = await rect(page, TRAY);

    // Positive control: there *is* an offset content column to be wrong about.
    // With no rail the two centres coincide and this test proves nothing.
    expect(main.left, "the content column starts past the rail").toBe(RAIL);

    const trayCentre = tray.left + tray.w / 2;
    const columnCentre = main.left + main.w / 2;
    expect(
      Math.abs(trayCentre - columnCentre),
      `tray centred at ${trayCentre}, content column at ${columnCentre}`,
    ).toBeLessThanOrEqual(1);
    // Named explicitly so the failure message says which mistake was made: the
    // old behaviour centred on the viewport, half a rail to the left.
    expect(Math.abs(trayCentre - DESKTOP.width / 2)).toBeGreaterThan(RAIL / 2 - 1);
  });

  test("@fast at phone width the tray spans the viewport, because the column does", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(PHONE);
    await selectOneRow(page, request);
    const dock = await rect(page, '[data-testid="selection-tray-dock"]');
    // The `md:left-60` override must not leak below `md` — the rail is an
    // overlay drawer there and the content column is the whole viewport.
    expect(dock.left).toBe(0);
    expect(dock.w).toBe(PHONE.width);
  });
});

// --------------------------------------------- affordances that hide ---

test.describe("affordances asserted by computed style, not visibility", () => {
  test.use({ storageState: AUTH_STATE });

  test("@fast the tree row's ⋯ is transparent at rest and revealed on focus", async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await page.goto("/my");
    await hydrated(page);

    const actions = page.locator("[data-tree-row-actions]").first();
    // `opacity-0`, deliberately not `hidden`, so it stays tab-reachable. Both
    // states read as "visible" to Playwright, so nothing but computed opacity
    // can tell them apart — and nothing pinned this before.
    await expect
      .poll(() => actions.evaluate((el) => getComputedStyle(el).opacity))
      .toBe("0");
    // Still in layout while transparent: that is what makes it reachable at all.
    expect((await rect(page, "[data-tree-row-actions]")).w).toBeGreaterThan(0);

    await actions.focus();
    await expect
      .poll(() => actions.evaluate((el) => getComputedStyle(el).opacity))
      .toBe("1");
  });

  test("@fast the closed rail drawer is invisible and off screen at phone width", async ({
    page,
  }) => {
    await page.setViewportSize(PHONE);
    await page.goto("/my");
    await hydrated(page);

    const rail = page.locator('aside[aria-label="Sidebar"]');
    // `invisible`, not merely off-screen: off-screen alone leaves every tree
    // link Tab-reachable behind the page.
    const closed = await rail.evaluate((el) => {
      const s = getComputedStyle(el);
      return { visibility: s.visibility, right: el.getBoundingClientRect().right };
    });
    expect(closed.visibility).toBe("hidden");
    expect(closed.right, "the closed drawer is off the left edge").toBeLessThanOrEqual(0);

    await page.locator('[data-testid="rail-toggle"]').click();
    // Polled, not read once: the drawer slides on `transition-[left]
    // duration-200`, so a single read lands mid-interpolation and sees a
    // fractional `left` — which is how this assertion first failed at -240.
    await expect
      .poll(() => rail.evaluate((el) => getComputedStyle(el).visibility))
      .toBe("visible");
    await expect
      .poll(() => rail.evaluate((el) => Math.round(el.getBoundingClientRect().left)))
      .toBe(0);
  });
});

// ------------------------------------------- SPA click-through content ---

test.describe("reaching a screen by clicking, not by goto", () => {
  test.use({ storageState: AUTH_STATE });

  // Why this block exists: `/my` once rendered an *empty* All-cards table when
  // reached by clicking the sidebar's "All cards" row, while `goto('/my')` was
  // correct — a serialized-resource id collision that the entire e2e tier
  // passed, because every test in it loads pages directly. The fix is on main;
  // the verification gap was not, until here.
  //
  // It was collection-dependent (the client id counter sits somewhere different
  // after each page builds its own resources), so a single spot check would
  // have cleared a live bug. Hence the loop.
  //
  // The audit that added this block found a *second* live instance on the
  // adjacent navigation — collection → Catalog — which is the last test here.

  /// Small collections only. This is a fixture requirement, not a convenience:
  /// the bug rendered the *collection's* cards as catalog results, so the two
  /// outcomes are only distinguishable where the collection holds fewer cards
  /// than a catalog page. Bulk Box holds a full 50 and would pass either way.
  async function smallHolders(request: APIRequestContext) {
    const rows = (await tree(request)).filter((r) => r.present > 0);
    const small: { row: TreeRow; cards: number }[] = [];
    for (const row of rows) {
      const res = await request.get(`/api/collections/${row.summary.id}/view`);
      if (res.status() !== 200) continue;
      const view = (await res.json()) as { cards: unknown[] };
      if (view.cards.length > 0 && view.cards.length < 40) {
        small.push({ row, cards: view.cards.length });
      }
    }
    expect(
      small.length,
      "the fixture needs collections holding fewer cards than one catalog page, or this test cannot tell the two renderings apart",
    ).toBeGreaterThan(1);
    return small;
  }

  test("@fast Catalog reached by clicking from a collection shows the catalog, not the collection", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(DESKTOP);
    // `shared::SearchResults` and `shared::CollectionView` cross-decoded: serde
    // ignored the four extra keys and `CardRow` is a structural superset of
    // `CardSummary`, so the catalog's resource read the collection page's
    // serialized `collection_view` slot, believed it, and never fetched. The
    // page then reported "11 results" over a Commander Deck's eleven cards.
    // Closed by the named-field `SearchPayload` (app/src/catalog.rs).
    for (const { row, cards } of (await smallHolders(request)).slice(0, 4)) {
      await page.goto(`/my/collections/${row.summary.id}`);
      await hydrated(page);
      await expect(page.locator('[data-testid="collection-title"]')).toHaveText(
        row.summary.name,
      );

      const searches: string[] = [];
      const onReq = (r: { url(): string }) => {
        if (r.url().includes("/api/search_catalog")) searches.push(r.url());
      };
      page.on("request", onReq);
      await page.locator('nav[aria-label="Mode"] a[href="/catalog"]').click();
      await page.waitForURL(/\/catalog$/);

      // The resource must have actually gone to the server. This is the half
      // that fails loudest under the bug: the old behaviour rendered a
      // plausible page having made *zero* requests, so a content-only
      // assertion could in principle be satisfied by a lucky payload.
      await expect
        .poll(() => searches.length, { message: "catalog never fetched results" })
        .toBeGreaterThan(0);
      page.off("request", onReq);

      // …and the content is a catalog page, not this collection. Strictly more
      // tiles than the collection holds, so it cannot be the collection's rows
      // however they were ordered.
      await expect
        .poll(
          () => page.locator('[data-testid="card-preview-trigger"]').count(),
          {
            message: `/catalog after ${row.summary.name} (${cards} cards) rendered the collection instead`,
          },
        )
        .toBeGreaterThan(cards);
      await expect(page.getByTestId("result-count")).not.toHaveText(
        `${cards} results`,
      );
    }
  });

  test("@fast /my carries rows when reached by clicking All cards from each collection", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(DESKTOP);
    const held = await holders(request);
    const allCards = 'aside[aria-label="Sidebar"] a[href="/my"]';

    for (const c of held.slice(0, 4)) {
      await page.goto(`/my/collections/${c.summary.id}`);
      await hydrated(page);
      await expect(page.locator('[data-testid="collection-title"]')).toHaveText(
        c.summary.name,
      );

      await page.locator(allCards).first().click();
      await page.waitForURL(/\/my$/);
      // Content, not the URL. The bug changed nothing about the URL.
      await expect(
        page.locator('[data-testid="all-cards-row"]').first(),
        `/my via a click from ${c.summary.name} rendered no rows`,
      ).toBeVisible();
      await expect(page.locator("main")).not.toContainText(
        "haven't added any cards",
      );
    }
  });

  test("@fast /my/all carries rows when reached by tapping the root list row", async ({
    page,
    request,
  }) => {
    await page.setViewportSize(PHONE);
    // Below `md` the aggregate table lives at `/my/all`, reached from the
    // drill-down root list — a different link, a different resource order.
    const held = await holders(request);
    for (const c of held.slice(0, 3)) {
      await page.goto(`/my/collections/${c.summary.id}`);
      await hydrated(page);
      await page.locator('[data-testid="collection-back"]').click();
      await page.waitForURL(/\/my(\/collections\/.*)?$/);
      await hydrated(page);

      const row = page.locator('[data-testid="my-root-list"] a[href="/my/all"]');
      if ((await row.count()) === 0) continue; // back landed on a parent, not the root
      await row.first().click();
      await page.waitForURL(/\/my\/all$/);
      await expect(
        page.locator('[data-testid="all-cards-row"]').first(),
        `/my/all via a tap after ${c.summary.name} rendered no rows`,
      ).toBeVisible();
    }
  });
});
