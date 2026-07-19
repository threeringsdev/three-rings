import { expect, test } from "@playwright/test";
import { AUTH_STATE } from "./helpers";

// Catalog page (specs/app-ui.md "/catalog", specs/catalog-search.md).
//
// The load-bearing contract, in the order the tests assert it:
//   the query text is canonical and lives in the URL · the first page SSRs ·
//   typing debounces into one search · grammar errors render inline instead of
//   blanking the page · the view switch is a real radiogroup · anonymous
//   visitors get sign-in prompts carrying ?next.
//
// "bolt" is a stable POC-catalog probe (Lightning Bolt); assertions stay off
// exact result counts, which move with the catalog.

test("catalog SSRs the first page when the URL carries q @fast", async ({
  request,
}) => {
  // Request-level: no JS runs, so rendered result markup in the raw HTML is
  // proof of SSR rather than a client-side fetch into an empty shell.
  const res = await request.get("/catalog?q=bolt");
  expect(res.status()).toBe(200);
  const html = await res.text();
  expect(html).toMatch(/<h1[^>]*>Catalog<\/h1>/);
  expect(html).toContain('data-testid="results-grid"');
  expect(html).toContain("Lightning Bolt");
});

test("browse-all renders without a query @fast", async ({ page }) => {
  // Empty query is a valid search (specs/catalog-search.md), not an empty
  // state: /catalog with no ?q must still list cards.
  await page.goto("/catalog");
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(
    page.locator("[data-testid=results-grid] li").first(),
  ).toBeVisible();
  await expect(page.getByText(/cards in the catalog/)).toBeVisible();
});

test("typing debounces into a URL-canonical search @fast", async ({ page }) => {
  await page.goto("/catalog");
  const requests: string[] = [];
  page.on("request", (r) => {
    if (r.url().includes("/api/search_catalog")) requests.push(r.url());
  });

  // fill() sets the value in one input event — the debounce collapses it to a
  // single navigation, and the query lands in the URL, not in component state.
  await page.fill("#catalog-query", "bolt");
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
  await expect(page.getByTestId("result-count")).toContainText("results");
  await expect(page.getByText("Lightning Bolt").first()).toBeVisible();

  // One search per settled query — not one per keystroke.
  await page.waitForTimeout(600);
  expect(requests.length).toBe(1);
});

test("a shared search URL restores the field and the results @fast", async ({
  page,
}) => {
  // The URL is the whole state: landing cold on one must repopulate the box.
  await page.goto("/catalog?q=bolt");
  await expect(page.locator("#catalog-query")).toHaveValue("bolt");
  await expect(page.getByText("Lightning Bolt").first()).toBeVisible();
});

test("back leaves the search session, not the site @fast", async ({ page }) => {
  // Refining replaces history; starting a search pushes. So one Back from a
  // refined query returns to browse-all rather than walking off the site.
  await page.goto("/catalog");
  await page.fill("#catalog-query", "bolt");
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
  await page.fill("#catalog-query", "counter");
  await page.waitForURL((url) => url.searchParams.get("q") === "counter");

  await page.goBack();
  await page.waitForURL((url) => url.pathname === "/catalog" && !url.search);
  // The field follows the URL back, or the two sources of truth have split.
  await expect(page.locator("#catalog-query")).toHaveValue("");
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("clearing the query returns to browse-all @fast", async ({ page }) => {
  await page.goto("/catalog?q=bolt");
  await page.getByLabel("Clear search").click();
  await page.waitForURL((url) => url.pathname === "/catalog" && !url.search);
  await expect(page.locator("#catalog-query")).toHaveValue("");
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("a grammar error renders inline and keeps the page @fast", async ({
  page,
}) => {
  // `pow>3` is a real unknown term: the parser rejects it with a 422 naming
  // the term (specs/catalog-search.md). Half-typed queries hit this constantly,
  // so it must read as a message about the query, not as a page failure.
  await page.goto("/catalog?q=pow%3E3");
  const err = page.getByTestId("search-error");
  await expect(err).toBeVisible();
  await expect(err).toContainText("pow>3");
  await expect(err).not.toContainText("Search failed");
  // The chrome survives — the query is still editable, so the user can fix it.
  await expect(page.locator("#catalog-query")).toHaveValue("pow>3");
});

test("a mid-typing grammar error keeps the last good results @fast", async ({
  page,
}) => {
  // Regression (Codex review): the error arm used to replace the result set, so
  // typing one more term strobed the whole page away and back. The rejected
  // query is a message about the query — the last page that did parse stays,
  // dimmed and inert, underneath it.
  await page.goto("/catalog?q=bolt");
  await expect(page.getByTestId("results-grid")).toBeVisible();

  await page.fill("#catalog-query", "bolt pow>3");
  await expect(page.getByTestId("search-error")).toBeVisible();
  await expect(page.getByTestId("results-grid")).toBeVisible();
  // It must be the *previous* page that was kept, not any grid: assert the
  // actual cards survived, or "retained" could mean an empty marked container.
  await expect(page.getByTestId("results-grid")).toContainText("Lightning Bolt");
  await expect(page.locator("[data-stale=true]")).toBeVisible();

  // Fixing the query clears both the error and the stale marking.
  await page.fill("#catalog-query", "bolt");
  await expect(page.getByTestId("search-error")).toHaveCount(0);
  await expect(page.locator("[data-stale=true]")).toHaveCount(0);
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("punctuation-heavy queries round-trip through the URL @fast", async ({
  page,
}) => {
  // The grammar is punctuation-dense (`:`, `<=`, quotes, `-` negation). Typing
  // it is the path that exercises our own encoder — the URL has to come back
  // out as the exact string that went in, or the user searched something else
  // than what they typed. A query with real hits, so a silently-broken encode
  // shows up as "no results" rather than passing.
  const q = 't:instant c:r mv<=2 -o:"draw a card"';
  await page.goto("/catalog");
  await page.fill("#catalog-query", q);
  await page.waitForURL((url) => url.searchParams.get("q") === q);
  await expect(page.locator("#catalog-query")).toHaveValue(q);
  await expect(page.getByTestId("search-error")).toHaveCount(0);
  await expect(page.getByTestId("results-grid")).toBeVisible();

  // And a reload of that generated URL lands on the same state.
  await page.reload();
  await expect(page.locator("#catalog-query")).toHaveValue(q);
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("the view switch is a radiogroup with roving focus @fast", async ({
  page,
}) => {
  await page.goto("/catalog?q=bolt");
  const group = page.getByRole("radiogroup", { name: "Result layout" });
  const grid = group.getByRole("radio", { name: "Grid view" });
  const list = group.getByRole("radio", { name: "List view" });

  // Roving focus: exactly one tab stop, and it is the selected item.
  await expect(grid).toHaveAttribute("aria-checked", "true");
  await expect(grid).toHaveAttribute("tabindex", "0");
  await expect(list).toHaveAttribute("tabindex", "-1");

  await grid.focus();
  await page.keyboard.press("ArrowRight");

  // Arrow selects, moves the tab stop, AND carries focus with it.
  await page.waitForURL((url) => url.searchParams.get("view") === "list");
  await expect(page.getByTestId("results-list")).toBeVisible();
  await expect(page.getByTestId("results-grid")).toHaveCount(0);
  await expect(list).toHaveAttribute("aria-checked", "true");
  await expect(list).toHaveAttribute("tabindex", "0");
  await expect(list).toBeFocused();

  // And the layout choice is in the URL, so it survives a reload.
  await page.reload();
  await expect(page.getByTestId("results-list")).toBeVisible();
});

test("switching view keeps the query @fast", async ({ page }) => {
  await page.goto("/catalog?q=bolt");
  await page.getByRole("radio", { name: "List view" }).click();
  await page.waitForURL((url) => url.searchParams.get("view") === "list");
  expect(new URL(page.url()).searchParams.get("q")).toBe("bolt");
  await expect(page.getByTestId("results-list")).toContainText("Lightning Bolt");
});

test("card tiles lazy-load images and link to the card @fast", async ({
  page,
}) => {
  await page.goto("/catalog?q=bolt");
  const tile = page.locator("[data-testid=results-grid] li").first();
  await expect(tile.locator("a").first()).toHaveAttribute(
    "href",
    /^\/cards\/[0-9a-f-]{36}$/,
  );
  // Not guarded by a count check: "bolt" returns image-bearing printings, so a
  // page that rendered no <img> at all is a failure, not a skip. (Transform
  // layouts legitimately have no image until the card-detail COALESCE fix —
  // hence the specific query.)
  await expect(tile.locator("img")).toHaveAttribute("loading", "lazy");
  await expect(tile.locator("img")).toHaveAttribute("decoding", "async");
});

test("a query keeps URL-structural characters intact @fast", async ({
  page,
}) => {
  // `&` and `+` are the characters a naive encoder gets wrong: unencoded, `&`
  // splits the query into a second parameter and `+` decodes back as a space.
  // Either way the user silently searched something other than what they typed.
  const q = "bolt &foo +bar";
  await page.goto("/catalog");
  await page.fill("#catalog-query", q);
  await page.waitForURL((url) => url.searchParams.get("q") === q);
  expect(new URL(page.url()).searchParams.get("q")).toBe(q);
  await page.reload();
  await expect(page.locator("#catalog-query")).toHaveValue(q);
});

test("anonymous quick actions prompt sign-in with a return path @fast", async ({
  page,
}) => {
  await page.goto("/catalog?q=bolt");
  const prompt = page
    .locator("[data-testid=results-grid] li")
    .first()
    .getByTestId("signin-prompt")
    .first();
  await expect(prompt).toBeVisible();

  await prompt.click();
  await page.waitForURL(
    (url) =>
      url.pathname === "/login" &&
      url.searchParams.get("next") === "/catalog?q=bolt",
  );
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
});

test.describe("authed", () => {
  test.use({ storageState: AUTH_STATE });

  test("a signed-in visitor gets no sign-in prompts @fast", async ({ page }) => {
    // The session is read opportunistically by the search adapter; when it is
    // present the quick actions stop being sign-in bait.
    await page.goto("/catalog?q=bolt");
    await expect(page.getByTestId("results-grid")).toBeVisible();
    await expect(page.getByTestId("signin-prompt")).toHaveCount(0);
    // Assert the positive too — "no prompts" is also true of a page with no
    // quick actions at all, which would make this test pass by deleting the
    // feature. The adds themselves land with the destination picker, so the
    // buttons are present and inert until then.
    const tile = page.locator("[data-testid=results-grid] li").first();
    await expect(
      tile.getByRole("button", { name: /Add .* to Want/ }),
    ).toBeVisible();
    await expect(
      tile.getByRole("button", { name: /Add .* to Have/ }),
    ).toBeVisible();
  });
});
