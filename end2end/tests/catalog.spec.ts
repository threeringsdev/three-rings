import { expect, test, type APIRequestContext } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// Catalog page (specs/app-ui.md "/catalog", specs/catalog-search.md).
//
// The load-bearing contract, in the order the tests assert it:
//   the query text is canonical and lives in the URL · the first page SSRs ·
//   typing debounces into one search · grammar errors render inline instead of
//   blanking the page · the view switch is a real radiogroup · anonymous
//   visitors get sign-in prompts carrying ?next · `?cursor=` is the keyset page
//   and every query edit drops it.
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
  await hydrated(page);
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(
    page.locator("[data-testid=results-grid] li").first(),
  ).toBeVisible();
  await expect(page.getByText(/cards in the catalog/)).toBeVisible();
});

test("typing debounces into a URL-canonical search @fast", async ({ page }) => {
  await page.goto("/catalog");
  await hydrated(page);
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
  await hydrated(page);
  await expect(page.locator("#catalog-query")).toHaveValue("bolt");
  await expect(page.getByText("Lightning Bolt").first()).toBeVisible();
});

test("back leaves the search session, not the site @fast", async ({ page }) => {
  // Refining replaces history; starting a search pushes. So one Back from a
  // refined query returns to browse-all rather than walking off the site.
  await page.goto("/catalog");
  await hydrated(page);
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
  await hydrated(page);
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
  await hydrated(page);
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
  await hydrated(page);
  await expect(page.getByTestId("results-grid")).toBeVisible();

  await page.fill("#catalog-query", "bolt pow>3");
  await expect(page.getByTestId("search-error")).toBeVisible();
  await expect(page.getByTestId("results-grid")).toBeVisible();
  // It must be the *previous* page that was kept, not any grid: assert the
  // actual cards survived, or "retained" could mean an empty marked container.
  await expect(page.getByTestId("results-grid")).toContainText(
    "Lightning Bolt",
  );
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
  await hydrated(page);
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
  await hydrated(page);
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
  await hydrated(page);
  await page.getByRole("radio", { name: "List view" }).click();
  await page.waitForURL((url) => url.searchParams.get("view") === "list");
  expect(new URL(page.url()).searchParams.get("q")).toBe("bolt");
  await expect(page.getByTestId("results-list")).toContainText(
    "Lightning Bolt",
  );
});

test("card tiles lazy-load images and link to the card @fast", async ({
  page,
}) => {
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  const tile = page.locator("[data-testid=results-grid] li").first();
  await expect(tile.locator("a").first()).toHaveAttribute(
    "href",
    /^\/cards\/[0-9a-f-]{36}$/,
  );
  // Not guarded by a count check: "bolt" returns image-bearing printings, so a
  // page that rendered no <img> at all is a failure, not a skip. (Transform
  // layouts used to have no image; the card-detail task's COALESCE fallback
  // fixed that at all six projection sites, so any query works here now.)
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
  await hydrated(page);
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
  await hydrated(page);
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

  test("a signed-in visitor gets no sign-in prompts @fast", async ({
    page,
  }) => {
    // The session is read opportunistically by the search adapter; when it is
    // present the quick actions stop being sign-in bait.
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
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

// ---------------------------------------------------------------------------
// Keyset paging (`?cursor=`) — specs/catalog-search.md "Result order and
// keyset". Two rules carry the feature: the cursor is URL state like `q` and
// `view` (shareable, restorable, SSR'd), and *any* edit to the query throws it
// away, because it names a row position in the result set the previous query
// produced.
// ---------------------------------------------------------------------------

type Results = {
  cards: { oracle_id: string; name: string }[];
  next_cursor: string | null;
};

/// The hosted JSON route behind the page's own adapter. `limit` is the reason
/// it is used here rather than `/api/search_catalog`: the page always asks for
/// 50, and a small page is the only way to put a *known* mid-set boundary under
/// an assertion instead of wherever the 50th card happens to fall.
async function search(
  request: APIRequestContext,
  opts: { q?: string; cursor?: string; limit?: number } = {},
): Promise<Results> {
  const params = new URLSearchParams({ q: opts.q ?? "" });
  if (opts.cursor !== undefined) params.set("cursor", opts.cursor);
  if (opts.limit !== undefined) params.set("limit", String(opts.limit));
  const url = `/api/catalog/search?${params}`;
  const res = await request.get(url);
  expect(res.status(), `GET ${url}`).toBe(200);
  return (await res.json()) as Results;
}

/// A card's tile, by the oracle id its link carries — the grid has no per-tile
/// id attribute and the link is what the page promises anyway.
const tileFor = (page: import("@playwright/test").Page, oracleId: string) =>
  page.locator(`[data-testid=results-grid] a[href="/cards/${oracleId}"]`);

/// A syntactically valid cursor positioned past the end of the catalog
/// (`{name, oracle_id}`, base64url, no padding — `hosted::encode_cursor`). Built
/// rather than fetched because there is no cursor *for* the last page: the
/// endpoint stops emitting one there, which is precisely the case under test.
const PAST_THE_END = Buffer.from(
  JSON.stringify({
    name: "zzzzzzzz",
    oracle_id: "ffffffff-ffff-ffff-ffff-ffffffffffff",
  }),
).toString("base64url");

test("the pager walks to a second page of different cards @fast", async ({
  page,
  request,
}) => {
  // The pager's own link, clicked — deep-linking a cursor from the JSON route
  // would leave `Pager`'s href free to point anywhere (the lesson `/my`'s
  // paging review left behind).
  const first = await search(request);
  expect(
    first.next_cursor,
    "browse-all must exceed one page for this to mean anything",
  ).toBeTruthy();
  const second = await search(request, { cursor: first.next_cursor! });
  expect(second.cards.length).toBeGreaterThan(0);

  await page.goto("/catalog");
  await hydrated(page);
  await expect(page.getByTestId("page-first")).toHaveCount(0);
  await page.getByTestId("page-next").click();

  // The URL carries the cursor the endpoint handed out — a base64url cursor is
  // unreserved throughout, so it is its own encoding.
  await page.waitForURL(`/catalog?cursor=${first.next_cursor}`);
  // ...and the rows are the ones that follow page one, not page one again.
  await expect(tileFor(page, second.cards[0].oracle_id)).toBeVisible();
  await expect(tileFor(page, first.cards[0].oracle_id)).toHaveCount(0);
  await expect(page.getByTestId("page-first")).toBeVisible();
});

test("a shared ?cursor= URL restores that page, SSR included @fast", async ({
  page,
  request,
}) => {
  const first = await search(request, { limit: 3 });
  expect(first.next_cursor, "a 3-row page cannot be the last").toBeTruthy();
  const rest = await search(request, { cursor: first.next_cursor! });
  expect(rest.cards.length).toBeGreaterThan(0);

  // Request-level: no JS has run, so page-two markup in the raw HTML is proof
  // the cursor reached the server render rather than being applied afterwards.
  const url = `/catalog?cursor=${first.next_cursor}`;
  const html = await (await request.get(url)).text();
  expect(html).toContain(`/cards/${rest.cards[0].oracle_id}`);
  expect(html).not.toContain(`/cards/${first.cards[0].oracle_id}`);
  expect(html).toContain('data-testid="page-first"');

  await page.goto(url);
  await hydrated(page);
  for (const card of first.cards) {
    await expect(tileFor(page, card.oracle_id)).toHaveCount(0);
  }
  await expect(tileFor(page, rest.cards[0].oracle_id)).toBeVisible();
});

test("the last page offers no next, and back to the start returns @fast", async ({
  page,
  request,
}) => {
  // A one-card first page of a small result set, so the page *after* it is the
  // last one whatever the catalog holds.
  const one = await search(request, { q: "bolt", limit: 1 });
  expect(one.next_cursor).toBeTruthy();
  const last = await search(request, { q: "bolt", cursor: one.next_cursor! });
  expect(last.next_cursor, "the rest of `bolt` must fit one page").toBeNull();
  expect(last.cards.length).toBeGreaterThan(0);

  await page.goto(`/catalog?q=bolt&cursor=${one.next_cursor}`);
  await hydrated(page);
  await expect(tileFor(page, last.cards[0].oracle_id)).toBeVisible();
  // The card the cursor was taken *after* is gone. Without this the whole test
  // passes on a page that ignores the cursor entirely: `bolt` fits one page, so
  // the unpaged render also contains `last.cards[0]` and also has no next
  // (found by mutating the resource to pass `None` — it survived).
  await expect(tileFor(page, one.cards[0].oracle_id)).toHaveCount(0);
  // No next cursor, no Next control — offering one would page into nothing.
  await expect(page.getByTestId("page-next")).toHaveCount(0);

  await page.getByTestId("page-first").click();
  // Home means page one *of the same search*: a cursor is the only thing the
  // link drops. Losing `q` would throw away the search to fix the page.
  await page.waitForURL("/catalog?q=bolt");
  await expect(tileFor(page, one.cards[0].oracle_id)).toBeVisible();
  await expect(page.getByTestId("page-first")).toHaveCount(0);
});

test("typing drops the page cursor @fast", async ({ page, request }) => {
  const first = await search(request, { limit: 3 });
  await page.goto(`/catalog?cursor=${first.next_cursor}`);
  await hydrated(page);

  // A new query has no page two yet; carrying the cursor forward would page
  // into rows that result set need not contain.
  await page.fill("#catalog-query", "bolt");
  await page.waitForURL("/catalog?q=bolt");
});

test("a rail edit and Reset drop the page cursor @fast", async ({
  page,
  request,
}) => {
  // The rail is the *other* writer of the query text, so it needs its own
  // assertion: it navigates through its own committer, not the query bar's.
  const first = await search(request, { q: "bolt", limit: 1 });
  await page.goto(`/catalog?q=bolt&cursor=${first.next_cursor}`);
  await hydrated(page);

  const rail = page.locator("[data-testid=filter-rail]");
  await rail.getByRole("checkbox", { name: "Red" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt c:r");
  expect(new URL(page.url()).searchParams.get("cursor")).toBeNull();

  // Reset is the third path into the same string and does not go through a
  // facet — it clears the rail-owned terms wholesale.
  await page.goto(`/catalog?q=bolt%20c%3Ar&cursor=${first.next_cursor}`);
  await hydrated(page);
  await page.locator("[data-testid=filter-rail]").getByText("Reset").click();
  await page.waitForURL("/catalog");
});

test("the view switch keeps your place in the results @fast", async ({
  page,
  request,
}) => {
  // Relayouting is not a query edit: the reader stays on the page they are on.
  const first = await search(request, { limit: 3 });
  await page.goto(`/catalog?cursor=${first.next_cursor}`);
  await hydrated(page);

  await page.getByRole("radio", { name: "List view" }).click();
  await page.waitForURL(`/catalog?view=list&cursor=${first.next_cursor}`);
  await expect(page.getByTestId("results-list")).toBeVisible();
  // And the pager keeps the layout, or Next would drop a list reader into the
  // grid (the hrefs are reactive in `view` for exactly this).
  await expect(page.getByTestId("page-next")).toHaveAttribute(
    "href",
    /view=list/,
  );
});

test("a page past the end says so and offers a way home @fast", async ({
  page,
}) => {
  // An empty *cursored* page is not "no cards match": the search is fine, the
  // reader has walked off the end.
  await page.goto(`/catalog?cursor=${PAST_THE_END}`);
  await hydrated(page);
  const empty = page.getByTestId("no-results");
  await expect(empty).toContainText("Nothing on this page");
  await expect(empty).not.toContainText("No cards match");
  await page.getByTestId("page-first").click();
  await page.waitForURL("/catalog");
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("a corrupt cursor renders an error with a way back @fast", async ({
  page,
}) => {
  // A shared link can rot. `/my` leaves this a dead end (its error arm has no
  // pager); here the query survives and only the cursor is dropped.
  await page.goto("/catalog?q=bolt&cursor=not-a-real-cursor");
  await hydrated(page);
  await expect(page.getByTestId("search-error")).toContainText(
    "invalid cursor",
  );
  await page.getByTestId("page-first").click();
  await page.waitForURL("/catalog?q=bolt");
  await expect(page.getByTestId("results-grid")).toContainText(
    "Lightning Bolt",
  );
});
