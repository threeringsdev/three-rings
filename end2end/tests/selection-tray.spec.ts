// Selection tray — custom gap component №3 (specs/app-ui.md "Custom gap
// components"; design/wireframes.pen → "Tray Wrap" / "Selection Tray").
//
// This task is **read-only**: the tray collects a selection and renders it, and
// "Move to…" is inert. So the contract splits cleanly in two, and so does this
// file:
//
// - the *component* — nothing at zero selection, a stack that stops at three, a
//   count that reads `1 card` / `n cards`, clear, and an inert "Move to…" — is
//   exercised on the bench, which is public and needs no Neon read (the
//   count-stepper precedent). The bench mounts its own `SelectionState`, so it
//   drives the same component the pages host without touching their selection;
// - the *cross-view state* — the reason this thing is installed in the shell
//   rather than on a page — can only be shown on the real pages: a pick has to
//   survive a Catalog ⇄ My-cards mode switch, accumulate across `/my` and a
//   collection, and outlive `/my/collections/:id` detaching its whole DOM
//   subtree after a `?q=` navigation.
//
// Every navigation in the authed half is a **click**, never `page.goto`: the
// selection is in-memory, so a document load legitimately starts empty and a
// `goto`-based test would be asserting the wrong thing.
//
// Read-only throughout — nothing here writes to the dev user, so it needs none
// of the `zz-e2e-…` scratch-collection isolation the writing specs use.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, clickUntil, hydrated, textEquals } from "./helpers";

const TRAY = '[data-testid="selection-tray"]';
const COUNT = '[data-testid="tray-count"]';
const THUMB = '[data-testid="tray-thumb"]';
const CLEAR = '[data-testid="tray-clear"]';
const MOVE = '[data-testid="tray-move"]';
const BENCH_SELECT = '#bench-tray-rows [data-testid="row-select"]';
const MY_ROW_SELECT = '[data-testid="all-cards-row"] [data-testid="row-select"]';
const COL_ROW_SELECT =
  '[data-testid="collection-row"] [data-testid="row-select"]';

// -------------------------------------------------------- the component ---

test.describe("bench", () => {
  async function open(page: Page) {
    await page.goto("/dev/components");
    await hydrated(page);
    await page.locator(BENCH_SELECT).first().scrollIntoViewIfNeeded();
  }

  test("@fast nothing renders at zero selection, and the count reads per pick", async ({
    page,
  }) => {
    await open(page);
    // Absent, not hidden — `Show` renders nothing at all, so a `toBeHidden()`
    // here would pass on a merely transparent tray.
    await expect(page.locator(TRAY)).toHaveCount(0);

    const first = page.locator(BENCH_SELECT).first();
    await expect(first).toHaveAttribute("aria-checked", "false");
    await first.click();
    await expect(page.locator(TRAY)).toHaveCount(1);
    await expect(page.locator(COUNT)).toHaveText("1 card");
    await expect(first).toHaveAttribute("aria-checked", "true");

    await page.locator(BENCH_SELECT).nth(1).click();
    await expect(page.locator(COUNT)).toHaveText("2 cards");
    // Rows 0 and 1 are the same card at two grains (a `/my` oracle row and a
    // collection's printing row). Two entries, deliberately: they address
    // different things, and merging them would make one checkbox lie about the
    // other.
    await expect(
      page.locator(`${BENCH_SELECT}[data-selection-key^="card:"]`),
    ).toHaveAttribute("aria-checked", "true");
  });

  test("@fast the thumbnail stack stops at three", async ({ page }) => {
    await open(page);
    for (let i = 0; i < 4; i++) await page.locator(BENCH_SELECT).nth(i).click();
    await expect(page.locator(COUNT)).toHaveText("4 cards");
    // The wireframe draws three cards behind a count, not one per pick.
    await expect(page.locator(THUMB)).toHaveCount(3);
  });

  test("@fast clear empties the tray and un-checks the rows", async ({
    page,
  }) => {
    await open(page);
    for (let i = 0; i < 3; i++) await page.locator(BENCH_SELECT).nth(i).click();
    await expect(page.locator(COUNT)).toHaveText("3 cards");

    await page.locator(CLEAR).click();
    await expect(page.locator(TRAY)).toHaveCount(0);
    for (let i = 0; i < 3; i++) {
      await expect(page.locator(BENCH_SELECT).nth(i)).toHaveAttribute(
        "aria-checked",
        "false",
      );
    }
  });

  test('@fast "Move to…" opens the destination picker', async ({ page }) => {
    await open(page);
    await page.locator(BENCH_SELECT).first().click();
    const move = page.locator(MOVE);
    await expect(move).toHaveText("Move to…");

    // The picker is the *catalog's* control (`DestinationList`), so it brings
    // that control's search box with it. On the bench the collection list is a
    // session read the anonymous page cannot make, so what it renders is the
    // **failed** arm — asserted on the content, never on a visibility flag (a
    // closed popover is in the DOM either way; e2e-suite skill).
    //
    // This assertion used to read `No collection to move to.`, and the comment
    // above it used to call that "the honest rendering". It was not: the read
    // 401s, and an empty list is a claim about the account rather than about the
    // read. `DestinationList` now separates the two (the state-arms task) and the
    // empty line is reserved for what it can actually speak about — a *filter*
    // that matched nothing.
    await move.click();
    const picker = page.locator("#popover-tray-destination");
    await expect(
      picker.locator('[data-name="CommandInput"]'),
    ).toHaveAttribute("placeholder", "Search collections…");
    await expect(picker).toContainText("Couldn't load your collections.");
    await expect(picker).not.toContainText("No collection to move to.");
    // Nothing was moved and nothing was deselected by merely opening it.
    await expect(page.locator(COUNT)).toHaveText("1 card");
  });
});

// -------------------------------------------------- the cross-view state ---

test.describe("cross-view", () => {
  test.use({ storageState: AUTH_STATE });

  type TreeRow = {
    summary: { id: string; name: string; is_inbox: boolean };
    present: number;
  };

  /// A collection that actually holds copies of its own — the only kind with a
  /// selectable card row (a desire-only row has nothing to move, so it renders
  /// no checkbox). Chosen from the API rather than hardcoded: ids are
  /// per-database and the seed's shape is allowed to change.
  async function collectionWithCards(
    request: APIRequestContext,
  ): Promise<TreeRow> {
    const res = await request.get("/api/collections/tree");
    expect(res.status()).toBe(200);
    const rows = ((await res.json()) as { collections: TreeRow[] }).collections;
    const hit = rows.find((r) => r.present > 0);
    expect(
      hit,
      "dev seed should carry a collection holding cards (scripts/seed-dev-data.sh)",
    ).toBeTruthy();
    return hit!;
  }

  /// The All-cards table. `path` exists for the mobile block: below `md` `/my`
  /// is the drill-down root list (app/src/my/root.rs) and this table is one
  /// route down, at `/my/all` — the same rows, reachable on touch.
  async function openMy(page: Page, path = "/my") {
    await page.goto(path);
    await hydrated(page);
    await page.locator(MY_ROW_SELECT).first().waitFor();
  }

  test("@fast the selection survives a Catalog ⇄ My cards mode switch", async ({
    page,
  }) => {
    await openMy(page);
    // Zero selection on a real page renders no tray either.
    await expect(page.locator(TRAY)).toHaveCount(0);

    // clickUntil: the all-cards row checkbox lives inside the `<Transition>`
    // `AllCardsBody` resolves into (app/src/my/all_cards.rs) — same confirmed
    // flake class as the mode-switch clicks below (e2e-suite skill "Tiers").
    await clickUntil(page.locator(MY_ROW_SELECT).nth(0), () =>
      textEquals(page.locator(COUNT), "1 card"),
    );
    await clickUntil(page.locator(MY_ROW_SELECT).nth(1), () =>
      textEquals(page.locator(COUNT), "2 cards"),
    );
    const key = await page
      .locator(MY_ROW_SELECT)
      .first()
      .getAttribute("data-selection-key");
    expect(key).toMatch(/^card:[0-9a-f-]+$/);

    // The desktop mode switch — real SPA navigation (the router intercepts the
    // anchor), which is what "survives a mode switch" means. clickUntil: the
    // same Mode nav confirmed flaky in smoke.spec.ts.
    const modes = page.locator('nav[aria-label="Mode"]');
    await clickUntil(
      modes.getByRole("link", { name: "Catalog" }),
      async () => new URL(page.url()).pathname === "/catalog",
    );
    // Still up, still counting the same picks, on a page with no rows of its
    // own to select.
    await expect(page.locator(COUNT)).toHaveText("2 cards");

    await clickUntil(
      modes.getByRole("link", { name: "My cards" }),
      async () => new URL(page.url()).pathname === "/my",
    );
    await expect(page.locator(COUNT)).toHaveText("2 cards");
    // …and the rows it came from come back checked, which is the half a
    // count-only assertion would miss.
    await expect(
      page.locator(`${MY_ROW_SELECT}[data-selection-key="${key}"]`),
    ).toHaveAttribute("aria-checked", "true");
  });

  test("@fast picks on /my and inside a collection accumulate into one selection", async ({
    page,
    request,
  }) => {
    const collection = await collectionWithCards(request);
    await openMy(page);
    // clickUntil: same all-cards `<Transition>` flake as the mode-switch
    // test above.
    await clickUntil(page.locator(MY_ROW_SELECT).first(), () =>
      textEquals(page.locator(COUNT), "1 card"),
    );

    // Sidebar link → SPA navigation into the collection view. Same
    // `CollectionTreeNav` `<Suspense>` as collection-tree.spec.ts.
    await clickUntil(
      page.locator(
        `aside[aria-label="Sidebar"] li[data-tree-row="${collection.summary.id}"] a[href="/my/collections/${collection.summary.id}"]`,
      ),
      async () =>
        new URL(page.url()).pathname === `/my/collections/${collection.summary.id}`,
    );
    await page.locator(COL_ROW_SELECT).first().waitFor();
    await expect(page.locator(COUNT)).toHaveText("1 card");

    // A collection row is the grain-complete shape: its key names the
    // collection, the printing and the board. Same flake class — the
    // collection view's own rows resolve inside their own async boundary.
    const here = page.locator(COL_ROW_SELECT).first();
    await clickUntil(here, () => textEquals(page.locator(COUNT), "2 cards"));
    expect(await here.getAttribute("data-selection-key")).toMatch(
      new RegExp(`^held:${collection.summary.id}:[0-9a-f-]+:(main|side|maybe)$`),
    );

    // The churn case, and the reason the state lives in the shell: a `?q=`
    // navigation on this route detaches and re-attaches the page's whole DOM
    // subtree ~800 ms later. Type a search, let the rebuild happen, and the
    // tray must be untouched by it.
    await page.locator("#collection-query").fill("zzz-no-such-card");
    await expect(page).toHaveURL(/\?q=zzz-no-such-card/);
    await page.waitForTimeout(1200);
    await expect(page.locator(TRAY)).toHaveCount(1);
    await expect(page.locator(COUNT)).toHaveText("2 cards");
  });

  // The tray's own trigger docks at the very bottom of the window (the
  // `mobile` describe below, and the desktop dock above the same tab-bar-less
  // chrome) — exactly the position a popover's default placement or its
  // no-anchor-positioning JS fallback can get wrong and park off-screen at
  // the document's bottom edge (fixed: `components/ui/popover.rs`). Asserted
  // at both a desktop and a narrow viewport per that fix's own two
  // contributing bugs — a leftover `relative` class (any anchor-positioning
  // engine, any viewport) and a stale pre-content-load flip decision (the
  // JS fallback, not currently exercisable by this suite's chromium-only
  // tier since Chromium supports CSS anchor positioning natively — see the
  // primitive's module doc for both).
  test("@fast \"Move to…\" opens fully inside the viewport, adjacent to its trigger", async ({
    page,
  }) => {
    await openMy(page, "/my/all");
    await clickUntil(page.locator(MY_ROW_SELECT).first(), () =>
      textEquals(page.locator(COUNT), "1 card"),
    );
    await page.locator(MOVE).click();

    const panel = page.locator("#popover-tray-destination");
    // Content, not just a visibility flag — the e2e-suite skill's own
    // "closed popover is in the DOM" caveat, though moot here: a *closed*
    // native popover is `display:none`, so `boundingBox()` below already
    // returns `null` rather than a lying zero-size box.
    await expect(
      panel.locator('[data-name="CommandInput"]'),
    ).toHaveAttribute("placeholder", "Search collections…");

    const box = await panel.boundingBox();
    const viewport = page.viewportSize();
    expect(box, "picker panel should be rendered and open").toBeTruthy();
    expect(viewport, "viewport size should be known").toBeTruthy();
    expect(box!.y, "panel top should not be above the viewport").toBeGreaterThanOrEqual(0);
    expect(
      box!.y + box!.height,
      "panel bottom should not run past the viewport — the reported bug",
    ).toBeLessThanOrEqual(viewport!.height);
    expect(box!.x, "panel left should not be off the left edge").toBeGreaterThanOrEqual(0);
    expect(
      box!.x + box!.width,
      "panel right should not run past the viewport",
    ).toBeLessThanOrEqual(viewport!.width);
  });

  test.describe("mobile", () => {
    test.use({ viewport: { width: 390, height: 844 } });

    test("@fast the tray docks above the bottom tab bar", async ({ page }) => {
      await openMy(page, "/my/all");
      // clickUntil: the confirmed flaky site (e2e-suite skill "Tiers") — the
      // all-cards row checkbox lives inside the `<Transition>` `AllCardsBody`
      // resolves into.
      await clickUntil(page.locator(MY_ROW_SELECT).first(), () =>
        textEquals(page.locator(COUNT), "1 card"),
      );

      const tray = await page.locator(TRAY).boundingBox();
      const tabs = await page.locator('nav[aria-label="Primary"]').boundingBox();
      expect(tray, "tray should be on screen").toBeTruthy();
      expect(tabs, "bottom tabs should be on screen at 390px").toBeTruthy();
      // Above, not over: the wireframe stacks Tray Wrap directly on the Tab Bar.
      expect(tray!.y + tray!.height).toBeLessThanOrEqual(tabs!.y);
      // And the tabs are still the thing at the bottom of the viewport.
      expect(tabs!.y + tabs!.height).toBeGreaterThanOrEqual(844 - 1);
    });

    test("@fast \"Move to…\" still opens fully inside a narrow viewport", async ({ page }) => {
      await openMy(page, "/my/all");
      await clickUntil(page.locator(MY_ROW_SELECT).first(), () =>
        textEquals(page.locator(COUNT), "1 card"),
      );
      await page.locator(MOVE).click();

      const panel = page.locator("#popover-tray-destination");
      await expect(
        panel.locator('[data-name="CommandInput"]'),
      ).toHaveAttribute("placeholder", "Search collections…");

      const box = await panel.boundingBox();
      expect(box, "picker panel should be rendered and open").toBeTruthy();
      expect(box!.y).toBeGreaterThanOrEqual(0);
      expect(box!.y + box!.height, "panel bottom should not run past 844px").toBeLessThanOrEqual(
        844,
      );
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width, "panel right should not run past 390px").toBeLessThanOrEqual(
        390,
      );
    });
  });
});
