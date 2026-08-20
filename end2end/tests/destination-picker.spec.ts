import { expect, test } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// Destination picker + Want/Have quick actions + undo toasts
// (specs/app-ui.md "/catalog", specs/collection-api.md → Undo).
//
// The contract, in the order asserted below:
//   anonymous quick actions stay sign-in links (and work without JS) ·
//   the picker only exists for a signed-in caller · it lists collections with
//   the Inbox pinned and marks the current choice · choosing sticks across a
//   search and a reload (the tr_dest cookie) · `+ Have` adds one copy and its
//   toast undoes it · `+ Want` confirms but deliberately offers no undo.
//
// **These tests write to the Neon dev branch.** Every `+ Have` is undone by
// the test that made it, so holdings return to their prior state. `+ Want` has
// no undo operation to call (specs/app-ui.md Findings), so its desire row's
// quantity grows by one per run against a single upserted row — bounded rows,
// growing count, on a throwaway test user.
//
// "bolt" is a stable POC-catalog probe (Lightning Bolt).

test.describe("anonymous", () => {
  test("quick actions are sign-in links carrying ?next @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const prompt = page.getByTestId("signin-prompt").first();
    await expect(prompt).toBeVisible();
    // An <a>, not a button: the sign-in path must survive with JS disabled.
    expect(await prompt.evaluate((el) => el.tagName)).toBe("A");
    await expect(prompt).toHaveAttribute(
      "href",
      /\/login\?next=%2Fcatalog%3Fq%3Dbolt/,
    );
  });

  test("no destination picker without a session @fast", async ({ page }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    await expect(page.getByTestId("results-grid")).toBeVisible();
    // Anonymous visitors have no collections, so the picker must not render at
    // all — not render disabled, not render empty.
    await expect(page.getByTestId("destination-label")).toHaveCount(0);
  });
});

test.describe("signed in", () => {
  test.use({ storageState: AUTH_STATE });

  test("the picker defaults to the Inbox and lists collections @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);

    const label = page.getByTestId("destination-label");
    // Lazy Inbox provisioning happens on the first authed list_collections.
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });

    await label.click();
    const options = page.getByTestId("destination-option");
    await expect(options.first()).toBeVisible();
    // Inbox pins to the top regardless of name ordering.
    await expect(options.first()).toHaveText(/Inbox/);
    // The current choice is marked — via data-chosen, not the primitive's
    // aria-selected (that means keyboard-highlighted, a different thing).
    await expect(options.first()).toHaveAttribute("data-chosen", "true");
    // ...and the mark has to *track* the choice, not be painted on row 0. With
    // only this row asserted, hard-coding `data-chosen="true"` would pass.
    if ((await options.count()) > 1) {
      await expect(options.nth(1)).not.toHaveAttribute("data-chosen", "true");
    }
  });

  test("the chosen destination survives a search and a reload @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });

    // Need a second collection to prove stickiness means anything. Skip
    // rather than silently assert nothing if the test user has only an Inbox.
    await label.click();
    const options = page.getByTestId("destination-option");
    await expect(options.first()).toBeVisible();
    const count = await options.count();
    test.skip(
      count < 2,
      "test user has only the Inbox — nothing to switch to",
    );

    const otherName = (await options.nth(1).textContent())?.trim() ?? "";
    await options.nth(1).click();
    await expect(label).toHaveText(otherName);
    // Choosing closes the popover — a pick shouldn't need a second dismiss.
    await expect(options.first()).toBeHidden();

    // Sticky across a search (the picker unmounts and remounts with results).
    await page.fill("#catalog-query", "island");
    await page.waitForURL((url) => url.searchParams.get("q") === "island");
    await expect(label).toHaveText(otherName);

    // Sticky across a reload — this is the tr_dest cookie, and it must resolve
    // back to the same collection by id.
    await page.reload();
    await hydrated(page);
    await expect(label).toHaveText(otherName, { timeout: 10000 });

    // Put the fixture back so test order can't matter.
    await label.click();
    await options.first().click();
    await expect(label).toHaveText(/Inbox/);
  });

  test("+ Have adds one copy and the toast undoes it @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });
    const destination = (await label.textContent())?.trim() ?? "";

    // Read the destination's real contents around the add, so this test proves
    // the *database* moved rather than that a toast said so. Mutation analysis
    // caught the earlier version: it passed with `undo_quick_add` stubbed to
    // return Ok(()) without calling `undo_move`, because the 200 and the
    // "Removed" toast were both still produced.
    // The machine REST route, not the `list_collections` server fn: same data,
    // plain GET + JSON, and `page.request` shares the context's session cookies.
    const listRes = await page.request.get("/api/collections");
    expect(listRes.status()).toBe(200);
    const collections = await listRes.json();
    const inboxId = (
      collections.find((c: { is_inbox: boolean }) => c.is_inbox) ?? collections[0]
    )?.id;
    expect(inboxId, "the authed user must have a collection to add to").toBeTruthy();
    const boltPresent = async () => {
      // `q=`, not a bare `?limit=200` page one: this dev user's Inbox has
      // accumulated far more than 200 distinct card rows across the
      // project's e2e history (fixture-pool growth, WB-01KZMVA2Y1's class —
      // a `limit=200` first page no longer reliably reaches "Lightning
      // Bolt" alphabetically). `collection_view`'s own `q` is the in-
      // collection search (routes.rs `collection_view`, same substring
      // search `/my` uses) — read through it instead of paging blind.
      const res = await page.request.get(
        `/api/collections/${inboxId}/view?q=${encodeURIComponent("Lightning Bolt")}&limit=10`,
      );
      expect(res.status()).toBe(200);
      const view = await res.json();
      // Exact, not `startsWith`: the full-catalog bulk load also seeded
      // "Lightning Bolt // Lightning Bolt", which `startsWith` would match
      // too.
      const row = view.cards.find(
        (c: { name: string }) => c.name === "Lightning Bolt",
      );
      return row?.present ?? 0;
    };
    const before = await boltPresent();

    // A "bolt" browse now returns many cards ("Beacon Bolt", "Blastfire
    // Bolt", … — the full-catalog bulk load, not the POC's single
    // "Lightning Bolt"), so `.first()` no longer reliably lands on the
    // probe card. Scope by the button's own aria-label (`QuickAddButton`,
    // app/src/catalog.rs: `"Add {name} to {noun}"`), exact, instead.
    const have = page.getByRole("button", {
      name: "Add Lightning Bolt to Have",
      exact: true,
    });
    // Disabled until the destination resolves — an add with no destination
    // would have to guess where it goes.
    await expect(have).toBeEnabled({ timeout: 10000 });

    const add = page.waitForResponse(
      (r) => r.url().includes("/api/quick_add") && r.status() === 200,
    );
    await have.click();
    await add;

    // The toast names the card AND where it went — "added" alone doesn't tell
    // the user whether the sticky picker was pointing where they thought.
    const toast = page.locator("[data-name=Toast]").filter({ hasText: "Lightning Bolt" });
    await expect(toast).toContainText("Added");
    await expect(toast).toContainText(destination);
    // Exactly one copy — the adapter builds the AddLine, so quantity can't be
    // widened by the caller, and this is what pins that down.
    expect(await boltPresent()).toBe(before + 1);

    // Undo is offered for a Have (it wrote a move row) and actually reverses.
    const undo = page.waitForResponse(
      (r) => r.url().includes("/api/undo_quick_add") && r.status() === 200,
    );
    await toast.getByRole("button", { name: "Undo" }).click();
    await undo;
    await expect(
      page.locator("[data-name=Toast]").filter({ hasText: /Removed/ }),
    ).toBeVisible();
    // The whole point of Undo: the copy is gone again, not just the toast.
    expect(await boltPresent()).toBe(before);
  });

  test("+ Want confirms but offers no undo @fast", async ({ page }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });

    // Scoped by aria-label, exact — see the `+ Have` test above: `.first()`
    // on a "bolt" browse no longer reliably lands on "Lightning Bolt" now
    // that the full catalog returns many "*Bolt*" cards.
    const want = page.getByRole("button", {
      name: "Add Lightning Bolt to Want",
      exact: true,
    });
    await expect(want).toBeEnabled({ timeout: 10000 });

    const add = page.waitForResponse(
      (r) => r.url().includes("/api/quick_add") && r.status() === 200,
    );
    await want.click();
    await add;

    const toast = page.locator("[data-name=Toast]").filter({ hasText: "Lightning Bolt" });
    await expect(toast).toContainText("Wanted");
    // Deliberately no Undo: desires are outside the move ledger and there is
    // no compensating operation, so offering the button would be a lie.
    // Asserting count 0 on the toast itself (not the page) is what makes this
    // fail if the action is ever wired up unconditionally.
    await expect(toast.getByRole("button", { name: "Undo" })).toHaveCount(0);
  });

  test("the picker filters collections by typing @fast", async ({ page }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });
    await label.click();

    const options = page.getByTestId("destination-option");
    await expect(options.first()).toBeVisible();
    // Filtering to one row proves nothing if there was only one row. Mutation
    // analysis flagged this as conditionally vacuous — require something to
    // filter *out* rather than passing trivially on an Inbox-only fixture.
    test.skip(
      (await options.count()) < 2,
      "test user has only the Inbox — nothing to filter out",
    );

    await page.getByPlaceholder("Search collections…").fill("inbox");
    // Filtering hides non-matches rather than unmounting them, so assert on
    // what is *visible*, and that the match survived.
    await expect(options.filter({ hasText: /Inbox/ })).toBeVisible();
    const visible = await options.evaluateAll(
      (els) => els.filter((el) => el.offsetParent !== null).length,
    );
    expect(visible).toBe(1);

    await page.getByPlaceholder("Search collections…").fill("zzz-no-such");
    await expect(page.getByText("No collection matches.")).toBeVisible();
  });

  // Regression guard for an alpha bug report (WB-01M0DT0J4R): the picker's
  // search box had no horizontal padding, so what you typed sat flush against
  // the panel edge. `CommandInput` carries no `px-*` of its own by design —
  // every consumer supplies it on the wrapper — and `DestinationList` was the
  // one consumer that shipped without a wrapper. Measured in CSS pixels off
  // the *panel* rather than read off a class name, so a future restyle that
  // keeps the padding by other means still passes and one that drops it fails.
  //
  // `DestinationList` is shared, so this covers the selection tray's and the
  // tree's "Move to…" boxes too; the catalog's is the reachable one to measure
  // (the other two need a selection / a tree dialog first).
  test("the search box's text is inset from the panel edge @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });

    await label.click();
    const panel = page.locator("#popover-destination-picker");
    await expect(panel).toBeVisible();
    const search = page.getByPlaceholder("Search collections…");
    await expect(search).toBeVisible();
    await expect(page.getByTestId("destination-option").first()).toBeVisible();
    // The native popover's 150ms open transition still has the panel at
    // `scale(0.95)` — measuring mid-transition scales every gap below.
    await page.waitForTimeout(250);

    // Type first: the placeholder and the caret share the content box, and the
    // report is about typed text specifically.
    await search.fill("in");
    // The Inbox always survives this filter and is always the first row, so
    // the row measured below is a real, visible one.
    await expect(
      page.getByTestId("destination-option").filter({ hasText: /Inbox/ }),
    ).toBeVisible();

    const m = await search.evaluate((el) => {
      const input = el as HTMLInputElement;
      const panelEl = document.getElementById("popover-destination-picker")!;
      const p = panelEl.getBoundingClientRect();
      const ics = getComputedStyle(input);
      const ir = input.getBoundingClientRect();
      // Where the glyphs actually start/end: the input's content box, which is
      // its border box minus its own border and padding. The padding that
      // fixes this bug lives on the wrapper, so reading the input's own
      // `padding-left` alone would report 0 both before and after.
      const textLeft =
        ir.left + parseFloat(ics.borderLeftWidth) + parseFloat(ics.paddingLeft);
      const textRight =
        ir.right -
        parseFloat(ics.borderRightWidth) -
        parseFloat(ics.paddingRight);

      // The rows below it, for alignment: `CommandItem`'s own content box.
      // The first *visible* one — filtering hides non-matches rather than
      // unmounting them, and a hidden element measures 0×0 at 0,0.
      const row = Array.from(
        panelEl.querySelectorAll("[data-testid=destination-option]"),
      ).find((el) => (el as HTMLElement).offsetParent !== null) as HTMLElement;
      const rcs = getComputedStyle(row);
      const rr = row.getBoundingClientRect();
      const rowTextLeft =
        rr.left + parseFloat(rcs.borderLeftWidth) + parseFloat(rcs.paddingLeft);

      return {
        leftGap: textLeft - p.left,
        rightGap: p.right - textRight,
        rowLeftGap: rowTextLeft - p.left,
      };
    });

    // The bug: 0 (the panel is `p-0`, so the input's content box started at
    // the panel's own border). 8px is the smallest inset that reads as
    // deliberate; the shipped value is 12.
    expect(m.leftGap).toBeGreaterThanOrEqual(8);
    // Both sides — a `pl-` only fix would leave the caret welded to the right
    // edge once the text is long enough to scroll.
    expect(m.rightGap).toBeGreaterThanOrEqual(8);
    // ...and it lines up with the rows it filters, rather than being merely
    // nonzero. 1px of slack for subpixel rounding.
    expect(Math.abs(m.leftGap - m.rowLeftGap)).toBeLessThanOrEqual(1);
  });

  // Regression guard for a maintainer bug report filed against pre-#148 code:
  // (a) the picker appeared to render outside the view frame — only "a slim
  // gray bar above the picker" was visible — and (b) clicking away flashed
  // the card grid down a frame before it snapped back. #148 removed a
  // leftover `relative` class on `PopoverContent` that broke top-layer
  // `position: fixed`, corrupting every `anchor()` offset against document
  // height instead of viewport height (app/src/components/ui/popover.rs).
  // Verified by hand against this branch: both symptoms are already gone.
  // These tests guard the user-visible properties (panel in-viewport, grid
  // undisturbed), not the historical mutation itself: reintroducing the
  // exact `relative` class does not visibly reproduce on this page's
  // `PopoverAlign::Center` in current Chromium, while a gross positioning
  // break (verified with a translate-y mutation) fails the first test loudly.
  test("the picker panel renders fully inside the viewport @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });

    await label.click();
    const content = page.locator("#popover-destination-picker");
    await expect(content).toBeVisible();
    // Let the native popover's open transition (150ms, popover.rs) settle
    // before measuring — mid-transition the panel is still animating in from
    // `scale(0.95) translateY(-2px)`.
    await page.waitForTimeout(250);

    const viewport = page.viewportSize();
    const box = await content.boundingBox();
    expect(box, "panel has no bounding box — not actually visible").not.toBeNull();
    // The bug report's symptom: the panel positioned itself against document
    // height, not viewport height, so most of it fell below the fold. A
    // correctly-positioned panel's box sits entirely within [0, viewport].
    expect(box!.x).toBeGreaterThanOrEqual(-1);
    expect(box!.y).toBeGreaterThanOrEqual(-1);
    expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width + 1);
    expect(box!.y + box!.height).toBeLessThanOrEqual(viewport!.height + 1);
  });

  test("dismissing the picker does not flash the results grid @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    const label = page.getByTestId("destination-label");
    await expect(label).toHaveText(/Inbox/, { timeout: 10000 });

    await label.click();
    await expect(page.locator("#popover-destination-picker")).toBeVisible();

    const grid = page.getByTestId("results-grid");
    await expect(grid).toBeVisible();

    // Per-frame bounding-box sampling of the grid container — the measurable
    // signature a layout flash leaves — run in-page via requestAnimationFrame
    // so it isn't limited by CDP round-trip latency between frames. Armed
    // before the dismissal click, so the window covers the tail of the open
    // transition as well as the whole close — a flash on either edge fails.
    // Wall-clock-bounded like all-cards.spec.ts's row-height guard, rather
    // than frame-counted, so a throttled frame rate can't end the read early.
    const samplesPromise = grid.evaluate(async (el) => {
      const samples: { top: number; height: number }[] = [];
      const start = performance.now();
      await new Promise<void>((resolve) => {
        function loop() {
          const r = el.getBoundingClientRect();
          samples.push({ top: r.top, height: r.height });
          if (performance.now() - start < 800) {
            requestAnimationFrame(loop);
          } else {
            resolve();
          }
        }
        requestAnimationFrame(loop);
      });
      return samples;
    });

    // Click away on plain page background — the result-count text, a `<p>`
    // with no click handler of its own — to trigger the popover's native
    // light-dismiss without navigating anywhere.
    await page.getByTestId("result-count").click();

    const samples = await samplesPromise;
    expect(samples.length).toBeGreaterThan(10);
    const tops = samples.map((s) => s.top);
    const heights = samples.map((s) => s.height);
    // The bug report's symptom: the grid visibly pushed down for a frame
    // before snapping back. 1px of slack absorbs subpixel rounding without
    // hiding a real flash, which moves tens to hundreds of pixels.
    expect(Math.max(...tops) - Math.min(...tops)).toBeLessThanOrEqual(1);
    expect(Math.max(...heights) - Math.min(...heights)).toBeLessThanOrEqual(1);
  });
});
