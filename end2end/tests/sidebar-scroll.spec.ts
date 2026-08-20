import { expect, test, type Locator, type Page } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// The desktop sidebar rail as a scroll container (specs/app-ui.md → "The
// sidebar rail scrolls its own overflow"; `SidebarRail` in app/src/shell.rs).
//
// The bug these hold shut (WB-01M0AW176E, alpha feedback): the rail's inner
// panel is `md:sticky`, and nothing bounded its height. Once it pinned under
// the header, scrolling the page moved the main pane and left the rail exactly
// where it was — so everything past the fold (the last filter sections with
// every section expanded, or any tree taller than the window) sat frozen there.
// Measured precisely (see the spec's Findings table): at 1280x600 the last
// filter field came back only after scrolling a 3398 px catalog page to its
// absolute end, where the sticky box finally un-pins at the bottom of its
// containing block and the top of the rail leaves the window instead. The panel
// is now capped at the window below the header and scrolls itself.
//
// Below `md` the `<aside>` is a fixed drawer that has always scrolled; these
// are desktop-width tests and say nothing about that path.
//
// A note on what is NOT asserted here: the Set facet's picker is an *inline*
// `Command` list inside the rail (`CommandList class="max-h-56"`), not a
// popover — so it is a nested scroll container, not a top-layer element, and
// "is the picker clipped" is the wrong question for it. What is asserted is
// that its rows still work from inside a scrolled rail. The tree's `⋯` menu
// *is* a top-layer popover (`context_menu.rs`), and gets its own test.

const SCROLLER = "sidebar-rail-scroll";

type Box = {
  top: number;
  bottom: number;
  left: number;
  right: number;
  height: number;
};

type Metrics = {
  scrollHeight: number;
  clientHeight: number;
  scrollWidth: number;
  clientWidth: number;
  scrollTop: number;
  rect: Box;
  innerHeight: number;
  innerWidth: number;
  pageScrollTop: number;
};

function scroller(page: Page): Locator {
  return page.getByTestId(SCROLLER);
}

async function metrics(page: Page): Promise<Metrics> {
  return scroller(page).evaluate((el) => {
    const r = el.getBoundingClientRect();
    return {
      scrollHeight: el.scrollHeight,
      clientHeight: el.clientHeight,
      scrollWidth: el.scrollWidth,
      clientWidth: el.clientWidth,
      scrollTop: el.scrollTop,
      rect: {
        top: r.top,
        bottom: r.bottom,
        left: r.left,
        right: r.right,
        height: r.height,
      },
      innerHeight: window.innerHeight,
      innerWidth: window.innerWidth,
      pageScrollTop: document.scrollingElement?.scrollTop ?? 0,
    };
  });
}

/// The bottom edge of the sticky header — where the rail is supposed to start.
async function headerBottom(page: Page): Promise<number> {
  return page
    .locator("header")
    .first()
    .evaluate((el) => el.getBoundingClientRect().bottom);
}

/// How much window there is below the header for the rail to occupy.
///
/// Every "does it overflow?" question is asked against THIS, and against the
/// *header's* geometry rather than the rail's own. Two traps it avoids: an
/// unbounded rail's `clientHeight` simply grows to fit its content, so
/// `scrollHeight > clientHeight` is false on exactly the layout these tests
/// exist to catch; and an unbounded rail's `rect.top` goes negative once the
/// page is scrolled far enough for the sticky box to un-pin at the bottom of
/// its containing block, which would inflate the same figure past any content
/// height. A guard written either of those ways skips itself into uselessness.
function room(m: Metrics, header: number): number {
  return m.innerHeight - header;
}

/// Scroll the rail's own scroller to its end and report where it landed.
async function scrollRailToEnd(page: Page): Promise<number> {
  return scroller(page).evaluate((el) => {
    el.scrollTop = el.scrollHeight;
    return el.scrollTop;
  });
}

/// True iff the element occupying `locator`'s centre point IS that element (or
/// something inside it) — i.e. it is not clipped away, covered, or painted
/// somewhere its own box says it is not. `toBeVisible()` cannot tell you this:
/// an element scrolled out of an `overflow: auto` ancestor still has a box and
/// still reports visible.
async function hitTestsToItself(locator: Locator): Promise<boolean> {
  return locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const hit = document.elementFromPoint(
      r.left + r.width / 2,
      r.top + r.height / 2,
    );
    return !!hit && (hit === el || el.contains(hit) || hit.contains(el));
  });
}

/// Open every collapsed `<details>` section in the desktop rail — the reported
/// reproduction ("expanding all the sidebar filters puts some of them below the
/// fold").
async function expandEveryFilterSection(page: Page): Promise<void> {
  const summaries = page.locator("[data-testid=filter-rail] details > summary");
  const n = await summaries.count();
  expect(n, "the rail renders collapsible filter sections").toBeGreaterThan(0);
  for (let i = 0; i < n; i++) {
    const summary = summaries.nth(i);
    const open = await summary.evaluate((el) =>
      (el.parentElement as HTMLDetailsElement).hasAttribute("open"),
    );
    if (!open) await summary.click();
  }
}

test.describe("catalog filter rail", () => {
  test("expanding every filter section leaves the last one reachable @fast", async ({
    page,
  }) => {
    // Short enough that the expanded rail cannot fit — the user's own report,
    // reproduced. (On a laptop it takes a taller rail; the mechanism is the
    // same, and a fixed short viewport makes the test independent of how many
    // options each facet happens to carry.)
    await page.setViewportSize({ width: 1280, height: 600 });
    await page.goto("/catalog");
    await hydrated(page);
    await expandEveryFilterSection(page);
    // Clicking a summary low in the rail makes Playwright scroll the *page* to
    // reach it. Put the page back where the user left it, so what follows
    // measures the at-rest layout rather than a side effect of the fixture.
    await page.evaluate(() => document.scrollingElement?.scrollTo(0, 0));

    const header = await headerBottom(page);
    const before = await metrics(page);
    // Positive control: the expanded rail really is taller than the window has
    // room for. Without this the assertions below would pass on a rail that
    // simply fits.
    expect(
      before.scrollHeight,
      "the expanded rail is taller than the window below the header",
    ).toBeGreaterThan(room(before, header));
    // Bounded, and bounded to the *visible* region: the panel neither hides
    // under the sticky header nor runs off the bottom of the window. This is
    // the half `md:max-h-[calc(100dvh - header)]` buys, and it is the
    // assertion the unbounded layout fails.
    expect(before.rect.top).toBeCloseTo(header, 0);
    expect(before.rect.bottom).toBeLessThanOrEqual(before.innerHeight + 1);
    expect(before.scrollHeight).toBeGreaterThan(before.clientHeight);
    // No sideways scrollbar inside a 240 px rail (`overflow-y: auto` makes the
    // *x* axis `auto` too — this is the assertion that catches it).
    expect(before.scrollWidth).toBeLessThanOrEqual(before.clientWidth);

    // The last section's field starts below the fold — the bug, stated as a
    // measurement rather than assumed.
    const mv = page.locator("#filter-rail-mv");
    const outOfView = await mv.boundingBox();
    expect(outOfView, "the mana-value field is laid out").not.toBeNull();
    expect(outOfView!.y + outOfView!.height).toBeGreaterThan(
      before.innerHeight,
    );

    // ...and the RAIL is what brings it back. On the broken build `scrollTop`
    // stays 0 (an `overflow: visible` box has nothing to scroll) and the field
    // never moves, so this is the assertion that fails without the fix.
    const landed = await scrollRailToEnd(page);
    expect(landed, "the rail's own scroller moved").toBeGreaterThan(0);

    const after = await metrics(page);
    expect(
      after.pageScrollTop,
      "the sidebar scrolled, not the page",
    ).toBe(before.pageScrollTop);

    const inView = await mv.boundingBox();
    expect(inView!.y).toBeGreaterThanOrEqual(after.rect.top - 1);
    expect(inView!.y + inView!.height).toBeLessThanOrEqual(
      after.rect.bottom + 1,
    );
    expect(await hitTestsToItself(mv)).toBe(true);

    // Reachable is not the claim — *usable* is. Drive the section that was
    // stranded and watch it rewrite the canonical query text. (Scoped to the
    // desktop rail: the mobile `FilterSheet` mounts a SECOND `RailBody` that
    // stays in the DOM while closed, so an unscoped `aria-label` locator
    // resolves to two elements.)
    await page.selectOption(
      "[data-testid=filter-rail] [aria-label='Mana value comparison']",
      "<=",
    );
    await mv.fill("3");
    await mv.blur();
    // The URL is the canonical surface, and it is the one this test is
    // entitled to: how the query *bar* re-seeds from a rail edit is
    // `filter-rail.spec.ts`'s contract, not this file's.
    await page.waitForURL((url) => url.searchParams.get("q") === "mv<=3");
  });

  test("a rail that fits the window gets no scrollbar @fast", async ({
    page,
  }) => {
    // The other half of the cap: `max-h`, not `h`. A rail with room to spare
    // must not grow a scroll range (or a permanent gutter) just because it is
    // now a scroll container.
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto("/catalog");
    await hydrated(page);

    const header = await headerBottom(page);
    const m = await metrics(page);
    // Positive control: the cap is not what is keeping it short here — there is
    // genuine room left over, so "no overflow" is a real observation.
    expect(m.rect.height).toBeLessThan(room(m, header) - 1);
    expect(m.scrollHeight).toBeLessThanOrEqual(m.clientHeight);
    expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
  });

  test("the Set picker still picks from inside a scrolled rail @fast", async ({
    page,
  }) => {
    // The Set facet is an inline `Command` list — a scroll container nested in
    // the rail's new scroll container. Two nested scrollers is exactly the
    // shape that goes wrong quietly, so this drives a real pick from a rail
    // that has been scrolled away from the top.
    await page.setViewportSize({ width: 1280, height: 600 });
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    await expandEveryFilterSection(page);
    const landed = await scrollRailToEnd(page);
    expect(landed).toBeGreaterThan(0);

    const search = page.locator("#filter-rail-set");
    await search.scrollIntoViewIfNeeded();
    await search.fill("limited edition alpha");
    const option = page.locator(
      "[data-testid=filter-rail] [data-testid=set-option][data-code=lea]",
    );
    await expect(option).toHaveText("Limited Edition Alpha");
    // The picker's own `max-h-56` list is the INNER scroller; several rows come
    // back for this search, so the row has to be brought into that list's view
    // the same way a person's wheel would. What the test is proving is that
    // doing so works at all from inside the rail's new outer scroller — a
    // clipped-away row cannot be scrolled to.
    await option.scrollIntoViewIfNeeded();
    // Not `toBeVisible()`: a row scrolled out of either scroller still reports
    // visible. Hit-testing is what proves it is really on screen.
    expect(await hitTestsToItself(option)).toBe(true);
    await option.click();
    await page.waitForURL((url) =>
      (url.searchParams.get("q") ?? "").includes("s:lea"),
    );
    // The page never moved — the rail absorbed all of it.
    expect((await metrics(page)).pageScrollTop).toBe(0);
  });
});

test.describe("collection tree", () => {
  test.use({ storageState: AUTH_STATE });

  test("a tree taller than the window scrolls inside the rail @fast", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 600 });
    await page.goto("/my");
    await hydrated(page);
    await page.locator("[data-tree-row-head]").first().waitFor();

    const header = await headerBottom(page);
    const before = await metrics(page);
    // The tree's length is a property of the shared dev pool, not of this
    // test. Say so out loud rather than asserting on a row count: if the pool
    // is ever trimmed below one window's worth, this stops being a test of
    // anything and should skip, not pass.
    test.skip(
      before.scrollHeight <= room(before, header),
      "the seeded collection tree is shorter than the window — nothing to scroll",
    );
    expect(before.rect.bottom).toBeLessThanOrEqual(before.innerHeight + 1);
    expect(before.scrollHeight).toBeGreaterThan(before.clientHeight);

    const rows = page.locator("[data-tree-row-head]");
    const last = rows.nth((await rows.count()) - 1);
    const outOfView = await last.boundingBox();
    expect(outOfView!.y).toBeGreaterThan(before.innerHeight);

    const landed = await scrollRailToEnd(page);
    expect(landed).toBeGreaterThan(0);
    const after = await metrics(page);
    expect(after.pageScrollTop).toBe(before.pageScrollTop);
    expect(await hitTestsToItself(last)).toBe(true);
  });

  test("the row menu opens fully visible from a scrolled rail @fast", async ({
    page,
  }) => {
    // The regression this guards: the rail is now an `overflow: auto` ancestor,
    // and the tree's `⋯` menu is anchored to a row inside it. It survives
    // because `context_menu.rs` is a native top-layer popover — no ancestor's
    // overflow clips the top layer — but "should be fine" is not evidence, and
    // a row sitting at the very bottom of a scrolled rail is where a clipped
    // or mis-anchored panel would show up first.
    await page.setViewportSize({ width: 1280, height: 600 });
    await page.goto("/my");
    await hydrated(page);
    await page.locator("[data-tree-row-head]").first().waitFor();

    const header = await headerBottom(page);
    const before = await metrics(page);
    test.skip(
      before.scrollHeight <= room(before, header),
      "the seeded collection tree is shorter than the window — nothing to scroll",
    );
    expect(before.rect.bottom).toBeLessThanOrEqual(before.innerHeight + 1);
    await scrollRailToEnd(page);

    const rows = page.locator("[data-tree-row-head]");
    const last = rows.nth((await rows.count()) - 1);
    // `md:opacity-0 md:group-hover/row:opacity-100` — the trigger is a hover
    // affordance on desktop, so hover it the way a person would.
    await last.hover();
    const kebab = last.locator("[data-tree-row-actions]");
    expect(await hitTestsToItself(kebab)).toBe(true);
    await kebab.click();

    const menu = page.locator("#context-menu-tree");
    await expect
      .poll(() => menu.evaluate((el: HTMLElement) => el.matches(":popover-open")))
      .toBe(true);

    const panel = await menu.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return {
        top: r.top,
        bottom: r.bottom,
        left: r.left,
        right: r.right,
        height: r.height,
        innerHeight: window.innerHeight,
        innerWidth: window.innerWidth,
      };
    });
    expect(panel.height).toBeGreaterThan(0);
    expect(panel.top).toBeGreaterThanOrEqual(0);
    expect(panel.bottom).toBeLessThanOrEqual(panel.innerHeight);
    expect(panel.left).toBeGreaterThanOrEqual(0);
    expect(panel.right).toBeLessThanOrEqual(panel.innerWidth);
    // Escaping the rail's 240 px column is the top-layer proof: a panel clipped
    // by the new `overflow: auto` ancestor could not be wider than it.
    expect(panel.right).toBeGreaterThan((await metrics(page)).rect.right);

    // ...and it is operable where it landed, not merely drawn there.
    const rename = menu.getByRole("menuitem", { name: "Rename" });
    await expect(rename).toHaveCount(1);
    expect(await hitTestsToItself(rename)).toBe(true);
    await page.keyboard.press("Escape");
  });
});
