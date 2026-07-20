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
      const res = await page.request.get(
        `/api/collections/${inboxId}/view?limit=200`,
      );
      expect(res.status()).toBe(200);
      const view = await res.json();
      const row = view.cards.find((c: { name: string }) =>
        c.name.startsWith("Lightning Bolt"),
      );
      return row?.present ?? 0;
    };
    const before = await boltPresent();

    const have = page.getByTestId("quick-add-have").first();
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

    const want = page.getByTestId("quick-add-want").first();
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
});
