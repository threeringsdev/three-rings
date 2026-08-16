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
  // Exact endpoint match, not a substring: `/api/search_catalog_count`
  // (round 2's independent count request) also contains "/api/search_catalog"
  // as a substring and would otherwise double-count here.
  const requests: string[] = [];
  page.on("request", (r) => {
    if (new URL(r.url()).pathname === "/api/search_catalog") {
      requests.push(r.url());
    }
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

// A modified arrow is never a roving-focus move (adversarial review, round 2
// of the back-button task): `ViewSwitch`'s keydown handler used to match bare
// `ev.key()` and call `prevent_default()` without `stop_propagation()`, so a
// focused view switch on non-mac turned one `Alt+←` into both "flip to grid"
// *and* — because the event kept bubbling — "navigate back"
// (`components::back_nav::install_back_shortcut`'s window-level listener).
// Fixed at both ends (`view_switch.rs` ignores any arrow carrying a modifier;
// `back_nav.rs` also now defers to any keydown a component upstream already
// claimed, for every future case like this one, not just this bug).
//
// A "does it navigate" assertion cannot tell fixed from broken here on its
// own — this took a couple of wrong turns to land on, kept as reasoning
// rather than dropped silently:
//
// - A cold `page.goto` load makes `back_nav::has_history()` false, so *even
//   once `ViewSwitch` is fixed* the chord still reaches `back_nav`'s
//   listener (that is the point of the fix, not a leak) and it still
//   navigates — to its own fallback, `/catalog`. Under the old bug the same
//   keypress drove two navigations (`ViewSwitch`'s own flip, then
//   `back_nav`'s fallback overwriting it), landing on the exact same URL.
//   Final state cannot distinguish the two; verified by running a
//   URL-pathname-only version of this test against a deliberately-reverted
//   `ViewSwitch` guard and watching it pass anyway.
// - Giving the page real prior history so `back_nav` lands somewhere else on
//   a real back doesn't fully resolve it either, for a subtler reason: the
//   view switch only ever toggles between exactly two states (grid ⇄ list),
//   so *any* setup built by toggling the switch itself makes "the state
//   `ViewSwitch`'s own flip would produce" and "the state one real history
//   entry back actually contains" the same URL by construction — confirmed
//   the same way, with the same false-pass.
//
// What actually distinguishes them: a **third**, unrelated dimension folded
// into the same entry — the search text. Query-bar behavior is itself
// load-bearing history state (`components/query_bar.rs`, `catalog/rail.rs`:
// "History granularity is per search session" — the *first* filter on a bare
// page pushes; refining an existing one replaces), so the very first `bolt`
// search is a real, separate history entry from the bare page before it, and
// `ViewSwitch`'s flip can never reproduce or erase it. If the flip fires (the
// bug), it lands on `/catalog?q=bolt` — the *pushed* query state, minus the
// view param it just cleared. If only `back_nav`'s real history walk fires
// (the fix), it lands on the exact same `/catalog?q=bolt` — because that
// really is the one entry behind the current one. Both link back to `bolt`;
// what only the bug produces is *staying on* `?q=bolt&view=list` — the
// current entry, unmoved, because the flip pushed a new entry that
// `back_nav`'s now-blocked call would otherwise have popped straight back
// off. That is the actual assertion below.
test("Alt+ArrowLeft on a focused view switch does not also flip the view @fast", async ({
  page,
}) => {
  // This suite's own runner is real macOS Chromium, where `back_nav`'s chord
  // is `⌘[`, not `Alt+←` — spoof `navigator.platform` so `Alt+←` is live as
  // the desktop chord (same technique `card-detail.spec.ts`'s own
  // `Alt+ArrowLeft` shortcut test uses).
  await page.addInitScript(() => {
    Object.defineProperty(window.navigator, "platform", {
      get: () => "Linux x86_64",
    });
  });

  await page.goto("/catalog");
  await hydrated(page);

  // Two real, separate pushes: the first search (bare → `?q=bolt`, a genuine
  // push per the app's own "first filter on a bare page" rule), then the
  // view toggle (`?q=bolt` → `?q=bolt&view=list`, also a push — clicking, not
  // the keyboard, so this is unaffected by either fix under test).
  await page.locator("#catalog-query").fill("bolt");
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
  await page.getByRole("radio", { name: "List view" }).click();
  await page.waitForURL((url) => url.searchParams.get("view") === "list");

  await page.getByRole("radio", { name: "List view" }).focus();
  await page.keyboard.press("Alt+ArrowLeft");
  await page.waitForTimeout(300);

  // The fixed behavior (a real `history.back()`, landing one entry behind)
  // and the broken behavior (`ViewSwitch`'s own flip, immediately popped back
  // off by an unguarded `back_nav`) both resolve `q=bolt` — but only the
  // fixed path actually *moves*. Landing back on the exact URL just left
  // (`view=list` still present) is the bug's signature.
  const url = new URL(page.url());
  expect(url.searchParams.get("q")).toBe("bolt");
  expect(
    url.searchParams.get("view"),
    "still on ?view=list — Alt+ArrowLeft round-tripped instead of moving, ViewSwitch's own flip claimed the key",
  ).not.toBe("list");
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
  cards: { oracle_id: string; name: string; owned: number | null }[];
  next_cursor: string | null;
};

/// The hosted JSON route behind the page's own adapter. `limit` is the reason
/// it is used here rather than `/api/search_catalog`: the page always asks for
/// `CATALOG_PAGE_SIZE` (60, WB-01M033AFA0VSCGB8Z3HTYPFZVD), and a small page
/// is the only way to put a *known* mid-set boundary under an assertion
/// instead of wherever the 60th card happens to fall.
///
/// `page` is round 2's explicit-jump parameter (maintainer ruling,
/// 2026-08-15) — an offset under the same sort `cursor` walks; either names a
/// page, never both from one call here (matching `Pager`, which never
/// generates both together either).
async function search(
  request: APIRequestContext,
  opts: { q?: string; cursor?: string; limit?: number; page?: number } = {},
): Promise<Results> {
  const params = new URLSearchParams({ q: opts.q ?? "" });
  if (opts.cursor !== undefined) params.set("cursor", opts.cursor);
  if (opts.limit !== undefined) params.set("limit", String(opts.limit));
  if (opts.page !== undefined) params.set("page", String(opts.page));
  const url = `/api/catalog/search?${params}`;
  const res = await request.get(url);
  expect(res.status(), `GET ${url}`).toBe(200);
  return (await res.json()) as Results;
}

/// The row count for a query (round 2's `search_count`) — `/api/catalog/search/count`.
async function searchCount(
  request: APIRequestContext,
  q?: string,
): Promise<number> {
  const params = new URLSearchParams({ q: q ?? "" });
  const url = `/api/catalog/search/count?${params}`;
  const res = await request.get(url);
  expect(res.status(), `GET ${url}`).toBe(200);
  const body = (await res.json()) as { cards: number };
  return body.cards;
}

/// A card's tile, by the oracle id its link carries — the grid has no per-tile
/// id attribute and the link is what the page promises anyway.
const tileFor = (page: import("@playwright/test").Page, oracleId: string) =>
  page.locator(`[data-testid=results-grid] a[href="/cards/${oracleId}"]`);

/// A syntactically valid cursor positioned past the real end of the catalog
/// (`{name, oracle_id}`, base64url, no padding — `hosted::encode_cursor`).
/// Keyset paging orders `ORDER BY c.name, c.oracle_id` under this DB's default
/// (byte-order) collation, so a cursor built from the *actual last card's own*
/// identity always yields zero rows next — nothing sorts strictly after it.
///
/// A hardcoded `"zzzzzzzz"` sentinel drifted the moment the full-catalog bulk
/// load landed: Scryfall names carrying accented Latin letters (e.g. "Éomer
/// of the Riddermark", "Óin the Brave") encode to UTF-8 bytes ≥ 0xC3, which
/// sort *after* ASCII `z` (0x7A) — so `zzzzzzzz` stopped being past the end
/// and the "page past the end" case silently became a real, non-empty page.
/// `"zzzzzzzz"` is kept only as a cheap jump-off point (skip the large
/// all-ASCII bulk of the catalog in one query) — the loop below then walks
/// whatever non-ASCII tail exists today to the API's own real last row,
/// so this stays correct at any catalog size or alphabet mix.
async function pastTheEndCursor(request: APIRequestContext): Promise<string> {
  let cursor = Buffer.from(
    JSON.stringify({
      name: "zzzzzzzz",
      oracle_id: "ffffffff-ffff-ffff-ffff-ffffffffffff",
    }),
  ).toString("base64url");
  let last: { name: string; oracle_id: string } | null = null;
  for (;;) {
    const page = await search(request, { cursor, limit: 200 });
    if (page.cards.length === 0) break;
    const lastCard = page.cards[page.cards.length - 1];
    last = { name: lastCard.name, oracle_id: lastCard.oracle_id };
    if (!page.next_cursor) break;
    cursor = page.next_cursor;
  }
  expect(
    last,
    "expected at least one card past the zzzzzzzz jump-off point",
  ).toBeTruthy();
  return Buffer.from(JSON.stringify(last)).toString("base64url");
}

test("the pager walks to a second page of different cards @fast", async ({
  page,
  request,
}) => {
  // The pager's own link, clicked — deep-linking a cursor from the JSON route
  // would leave `Pager`'s href free to point anywhere (the lesson `/my`'s
  // paging review left behind). Round 2 (maintainer ruling, 2026-08-15):
  // every pager link is now an explicit page-N jump (`?page=`, no cursor at
  // all) — `search` at the JSON route with `page=2` is the equivalent
  // request, cross-checked against the true row order.
  const first = await search(request);
  expect(
    first.next_cursor,
    "browse-all must exceed one page for this to mean anything",
  ).toBeTruthy();
  const second = await search(request, { page: 2 });
  expect(second.cards.length).toBeGreaterThan(0);

  await page.goto("/catalog");
  await hydrated(page);
  // Prev always renders now (WB-01M032Q6BX8BM7NPK8H3AQKGWF) — disabled on
  // page one, not absent.
  await expect(page.getByTestId("pager-prev")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await page.getByTestId("pager-next").click();

  // The URL carries only the page number — no cursor at all (round 2: every
  // page is directly offset-addressable server-side).
  await page.waitForURL("/catalog?page=2");
  // ...and the rows are the ones that follow page one, not page one again.
  await expect(tileFor(page, second.cards[0].oracle_id)).toBeVisible();
  await expect(tileFor(page, first.cards[0].oracle_id)).toHaveCount(0);
  // Prev is now a real link back to page one, and the strip labels the
  // current page.
  await expect(page.getByTestId("pager-prev")).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.getByTestId("pager-current")).toHaveText("2");
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
  // No `&page=` on this link on purpose: an old-style or hand-built shared
  // link carries only `?cursor=`, and the page label is cosmetic (`PAGE_PARAM`)
  // — the rows it restores must still be exactly right regardless.
  const url = `/catalog?cursor=${first.next_cursor}`;
  const html = await (await request.get(url)).text();
  expect(html).toContain(`/cards/${rest.cards[0].oracle_id}`);
  expect(html).not.toContain(`/cards/${first.cards[0].oracle_id}`);
  expect(html).toContain('data-testid="pager-prev"');

  await page.goto(url);
  await hydrated(page);
  for (const card of first.cards) {
    await expect(tileFor(page, card.oracle_id)).toHaveCount(0);
  }
  await expect(tileFor(page, rest.cards[0].oracle_id)).toBeVisible();
});

test("the last page disables Next, and Prev returns to page one @fast", async ({
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

  await page.goto(`/catalog?q=bolt&cursor=${one.next_cursor}&page=2`);
  await hydrated(page);
  await expect(tileFor(page, last.cards[0].oracle_id)).toBeVisible();
  // The card the cursor was taken *after* is gone. Without this the whole test
  // passes on a page that ignores the cursor entirely: `bolt` fits one page, so
  // the unpaged render also contains `last.cards[0]` and also has no next
  // (found by mutating the resource to pass `None` — it survived).
  await expect(tileFor(page, one.cards[0].oracle_id)).toHaveCount(0);
  // Next always renders now (WB-01M032Q6BX8BM7NPK8H3AQKGWF) — disabled on
  // the true last page, not absent. `last = Some(page)` (this *is* the last
  // page, per `page_strip`'s doc comment) is knowable with no `COUNT`, so the
  // current page's own number renders too, plain and un-linked.
  await expect(page.getByTestId("pager-next")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.getByTestId("pager-current")).toHaveText("2");

  await page.getByTestId("pager-prev").click();
  // Home means page one *of the same search*: a cursor is the only thing the
  // link drops. Losing `q` would throw away the search to fix the page.
  await page.waitForURL("/catalog?q=bolt");
  await expect(tileFor(page, one.cards[0].oracle_id)).toBeVisible();
  // `bolt` fits one page start to finish, so page one has nowhere to page
  // *to* either — no pager at all, the same landmark-free case
  // "a single-page result set renders no pagination landmark" pins.
  await expect(page.getByTestId("pager-prev")).toHaveCount(0);
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
  await expect(page.getByTestId("pager-next")).toHaveAttribute(
    "href",
    /view=list/,
  );
});

test("a page past the end says so and offers a way home @fast", async ({
  page,
  request,
}) => {
  // An empty *cursored* page is not "no cards match": the search is fine, the
  // reader has walked off the end.
  const pastTheEnd = await pastTheEndCursor(request);
  await page.goto(`/catalog?cursor=${pastTheEnd}`);
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
  //
  // P6-043: a corrupt `?cursor=` now carries its own `ApiError::BadCursor`
  // wire variant, so the banner blames the page reference rather than
  // echoing "invalid cursor" as if `bolt` itself were rejected — the old
  // assertion here (`toContainText("invalid cursor")`) was pinning exactly
  // that mislabeling.
  await page.goto("/catalog?q=bolt&cursor=not-a-real-cursor");
  await hydrated(page);
  const err = page.getByTestId("search-error");
  await expect(err).toContainText("page link");
  await expect(err).not.toContainText("invalid cursor");
  await page.getByTestId("page-first").click();
  await page.waitForURL("/catalog?q=bolt");
  await expect(page.getByTestId("results-grid")).toContainText(
    "Lightning Bolt",
  );
});

// ---------------------------------------------------------------------------
// Paging honesty (P6-130…133). One theme: `<Transition>` keeps the previous
// result set on screen while a newer search runs, so the URL and the rendered
// page routinely disagree. Everything that describes the page on screen — which
// page it is, how many rows it holds, whether its pager can be clicked — has to
// follow the payload that produced it, and the pager has to stop being
// actionable while the two disagree.
// ---------------------------------------------------------------------------

/// Hold every catalog search until the returned `release` is called. This is
/// the only way to *stand inside* the in-flight window these tests are about:
/// the results on screen are the previous query's, the URL is already the new
/// one, and nothing has resolved.
async function holdSearches(page: import("@playwright/test").Page) {
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  await page.route("**/api/search_catalog*", async (route) => {
    await gate;
    // The handler stays installed after the gate opens — `unroute` while a
    // handler is mid-`continue` fulfils the route itself and the continue then
    // throws "Route is already handled". Once the gate is open every later
    // request passes straight through anyway.
    await route.continue().catch(() => {});
  });
  return () => release();
}

test("a stale pager cannot revert what you just typed @fast", async ({
  page,
  request,
}) => {
  // P6-130. The pager rendered with the *old* results carries the old
  // `(q, cursor)`. An anchor click goes around `QueryBar::commit` straight to
  // that pair, and the query bar's re-seed effect — seeing the URL move without
  // it — rewrites the box, undoing what the user just typed. A cursor is only
  // valid for its own query, so the control is inert until the results under it
  // catch up.
  const browse = await search(request);
  expect(browse.next_cursor, "browse-all must have a page two").toBeTruthy();

  await page.goto("/catalog");
  await hydrated(page);
  const next = page.getByTestId("pager-next");
  await expect(next).toBeVisible();

  // A query with a page two of its own, so the pager is still there afterwards
  // and "it came back to life" is assertable rather than "it disappeared".
  const typed = "t:creature";
  const release = await holdSearches(page);
  await page.fill("#catalog-query", typed);
  await page.waitForURL((url) => url.searchParams.get("q") === typed);

  // No strobing: the previous results are still on screen. That is *why* the
  // stale pager is reachable at all, so it has to stay true here.
  await expect(tileFor(page, browse.cards[0].oracle_id)).toBeVisible();
  // Inert, and it says so — not removed, which would move the tab stop out from
  // under a keyboard user mid-navigation.
  await expect(next).toHaveAttribute("aria-disabled", "true");

  // `dispatchEvent`, not `click()`: this is exactly what a keyboard Enter on
  // the focused link produces, and it bypasses the CSS that already stops the
  // mouse. If the handler does not refuse it, the browser navigates.
  await next.dispatchEvent("click");
  await page.waitForTimeout(300);
  expect(new URL(page.url()).searchParams.get("q")).toBe(typed);
  await expect(page.locator("#catalog-query")).toHaveValue(typed);

  // And it comes back to life once the results answer the box again — inert
  // during the search, not broken.
  release();
  await expect(page.getByTestId("result-count")).toHaveText(/^\d+\+ results$/);
  await expect(page.getByTestId("pager-next")).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.locator("#catalog-query")).toHaveValue(typed);
});

test("a grammar error after paging does not resurrect the old page @fast", async ({
  page,
  request,
}) => {
  // P6-131. `last_good` kept whatever came back OK last, including a *cursored*
  // page, and the error arm rendered it dimmed under the next error — so a
  // half-typed term put rows 2..n of a search you had left on screen, labelled
  // "Previous results". Only page one is retained now, and paging away forgets
  // it. (The refinement case this feature exists for — an error while editing
  // page one — is pinned by "a mid-typing grammar error keeps the last good
  // results" above.)
  const one = await search(request, { q: "bolt", limit: 1 });
  expect(one.next_cursor).toBeTruthy();
  const rest = await search(request, { q: "bolt", cursor: one.next_cursor! });
  expect(rest.cards.length).toBeGreaterThan(0);

  await page.goto(`/catalog?q=bolt&cursor=${one.next_cursor}`);
  await hydrated(page);
  const pageTwoCard = rest.cards[0].oracle_id;
  await expect(tileFor(page, pageTwoCard)).toBeVisible();

  await page.fill("#catalog-query", "bolt pow>3");
  await expect(page.getByTestId("search-error")).toBeVisible();
  // The rows themselves, not just the marker: "no dimmed block" would also pass
  // on a page that rendered the old cards undimmed.
  await expect(tileFor(page, pageTwoCard)).toHaveCount(0);
  await expect(page.locator("[data-stale=true]")).toHaveCount(0);
});

test("the count says which page it is counting @fast", async ({
  page,
  request,
}) => {
  // P6-132. Keyset paging has no offset and the endpoint runs no COUNT, so a
  // page past the first can only speak for itself. Unqualified, the last page
  // of a 73-row search read "23 results".
  const one = await search(request, { q: "bolt", limit: 1 });
  const rest = await search(request, { q: "bolt", cursor: one.next_cursor! });
  expect(rest.next_cursor, "the rest of `bolt` must fit one page").toBeNull();

  await page.goto(`/catalog?q=bolt&cursor=${one.next_cursor}`);
  await hydrated(page);
  // The exact number as well as the qualifier: a page that qualified the claim
  // but counted the wrong rows would still be lying.
  await expect(page.getByTestId("result-count")).toHaveText(
    `${rest.cards.length} results on this page`,
  );

  // Page one is allowed to speak for the whole set, because it is the whole
  // set — `bolt` fits one page, so there is no "+" and no qualifier.
  const all = await search(request, { q: "bolt" });
  expect(all.next_cursor).toBeNull();
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  await expect(page.getByTestId("result-count")).toHaveText(
    `${all.cards.length} results`,
  );

  // ...and the start of a longer set says "at least".
  await page.goto("/catalog");
  await hydrated(page);
  await expect(page.getByTestId("result-count")).toHaveText(/^\d+\+ results$/);
});

test("Prev does not enable prematurely while the next page loads @fast", async ({
  page,
  request,
}) => {
  // P6-133a, restated for the numbered pager: "Am I past the start?" used to
  // be read off the URL, which moves the instant Next is clicked — so page
  // one, still on screen through the load, grew a "Back to the start"
  // pointing at itself. Prev always renders now (WB-01M032Q6BX8BM7NPK8H3AQKGWF)
  // instead of appearing/disappearing, so the same bug would show up as Prev
  // enabling early rather than appearing early — same regression, new shape.
  const browse = await search(request);
  expect(browse.next_cursor).toBeTruthy();

  await page.goto("/catalog");
  await hydrated(page);
  const prev = page.getByTestId("pager-prev");
  await expect(prev).toHaveAttribute("aria-disabled", "true");

  const release = await holdSearches(page);
  await page.getByTestId("pager-next").click();
  // Round 2: Next is an explicit page-N jump too — the URL carries only the
  // page number, no cursor at all.
  await page.waitForURL("/catalog?page=2");
  // The URL is page two; the DOM is still page one. The pager belongs to what
  // is rendered.
  await expect(tileFor(page, browse.cards[0].oracle_id)).toBeVisible();
  await expect(prev).toHaveAttribute("aria-disabled", "true");

  // Positive control: it enables the moment page two actually renders, so
  // this test cannot pass on a build that never enables it at all.
  release();
  await expect(prev).not.toHaveAttribute("aria-disabled", "true");
});

test("a single-page result set renders no pagination landmark @fast", async ({
  page,
  request,
}) => {
  // P6-133b. `<nav aria-label="Pagination">` wrapped an empty `<span>` when
  // there was nothing to page — a named landmark a screen reader announces as
  // navigation and which then contains nothing.
  const all = await search(request, { q: "bolt" });
  expect(all.next_cursor, "`bolt` must fit one page for this to mean anything")
    .toBeNull();

  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Pagination" })).toHaveCount(
    0,
  );

  // Positive control: browse-all does have somewhere to go, and gets the
  // landmark. Otherwise "no nav" passes on a page with no pager at all.
  await page.goto("/catalog");
  await hydrated(page);
  await expect(
    page.getByRole("navigation", { name: "Pagination" }),
  ).toBeVisible();
});

// ---------------------------------------------------------------------------
// The numbered page strip (WB-01M032Q6BX8BM7NPK8H3AQKGWF, `page_strip` in
// app/src/catalog.rs). Real dev data (38k+ cards) stands in for the task's
// abstract "28 pages" examples — same shapes, real numbers, asserted against
// the real DOM rather than the pure function alone (that has its own unit
// tests).
//
// **Round 2 (maintainer ruling, 2026-08-15): every rendered number is a real
// link, no exceptions.** An explicit page-N jump is a server-side `OFFSET`
// under the same sort the keyset cursor uses — it needs no cursor this
// browser happens to have already fetched, so round 1's "some numbers render
// inert, present but unreachable" compromise no longer applies to anything
// except staleness (an in-flight newer search) and the Prev/Next boundary
// (page one / the true last page).
// ---------------------------------------------------------------------------

test("page one shows every numbered link as real, none inert @fast", async ({
  page,
  request,
}) => {
  const first = await search(request);
  const pageSize = first.cards.length;
  expect(first.next_cursor, "browse-all must exceed one page").toBeTruthy();
  const count = await ((await request.get("/api/catalog/count")).json() as Promise<{
    cards: number;
  }>);
  const lastPage = Math.ceil(count.cards / pageSize);
  expect(lastPage, "the fixture must be large enough for a real ellipsis")
    .toBeGreaterThan(6);

  await page.goto("/catalog");
  await hydrated(page);

  await expect(page.getByTestId("pager-current")).toHaveText("1");
  await expect(page.getByTestId("pager-prev")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  // Every numbered slot the strip shows — the near band *and* the true last
  // page — is a real, working link now, not just the ones adjacent to page
  // one or already fetched.
  for (const n of [2, 3, 4, 5, lastPage]) {
    await expect(page.getByTestId(`pager-page-${n}`)).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
  }
  await expect(page.getByTestId("pager-next")).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("a distant page number jumps there directly, no walking required @fast", async ({
  page,
  request,
}) => {
  // The whole point of round 2's offset path: page one links straight to the
  // true last page (or any other far-off number `page_strip` shows), and
  // clicking it is a single navigation — never N clicks through the pages
  // between, and no trail needs to have been built up first.
  const count = await ((await request.get("/api/catalog/count")).json() as Promise<{
    cards: number;
  }>);
  const first = await search(request);
  const lastPage = Math.ceil(count.cards / first.cards.length);
  const lastPageRows = await search(request, { page: lastPage });
  expect(lastPageRows.cards.length).toBeGreaterThan(0);

  await page.goto("/catalog");
  await hydrated(page);
  await page.getByTestId(`pager-page-${lastPage}`).click();

  await page.waitForURL(`/catalog?page=${lastPage}`);
  await expect(page.getByTestId("pager-current")).toHaveText(String(lastPage));
  await expect(tileFor(page, lastPageRows.cards[0].oracle_id)).toBeVisible();
  await expect(tileFor(page, first.cards[0].oracle_id)).toHaveCount(0);
  // Landed on (or past) the true last page: Next has nowhere further to go.
  await expect(page.getByTestId("pager-next")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("a fresh ?page=N URL lands on the real page N, no prior visit @fast", async ({
  page,
  request,
}) => {
  // "Shareable URLs must be honest" (maintainer ruling, 2026-08-15): a
  // `?page=9` link pasted into a brand-new session — no cursor, no cookies,
  // no prior navigation in this browser at all — must resolve to the actual
  // ninth page, cross-checked against the JSON route's own `page=9` fetch.
  const nine = await search(request, { q: "t:instant", page: 9 });
  expect(nine.cards.length, "t:instant must reach a real page 9").toBeGreaterThan(0);
  const one = await search(request, { q: "t:instant" });

  // Request-level: no JS runs, so page-nine markup in the raw HTML is proof
  // the jump reached the server render, not a client-side patch afterward.
  const url = `/catalog?q=t%3Ainstant&page=9`;
  const html = await (await request.get(url)).text();
  expect(html).toContain(`/cards/${nine.cards[0].oracle_id}`);
  expect(html).not.toContain(`/cards/${one.cards[0].oracle_id}`);

  await page.goto(url);
  await hydrated(page);
  await expect(page.getByTestId("pager-current")).toHaveText("9");
  await expect(tileFor(page, nine.cards[0].oracle_id)).toBeVisible();
  // Prev is real too — this browser never "walked" here, but page 8 is just
  // as directly addressable as page 9 was.
  await expect(page.getByTestId("pager-prev")).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("an out-of-range page number renders no results, not a crash @fast", async ({
  page,
}) => {
  // Adversarial-review blocker (WB-01M032Q6BX8BM7NPK8H3AQKGWF round 2):
  // `GET /catalog?page=18446744073709551615` (usize::MAX on a 64-bit build)
  // reached unguarded `page + 1` arithmetic and panicked an anonymous SSR
  // request in debug builds. `parse_page` now clamps to a bounded ceiling —
  // still far past the real last page, so the honest answer is an empty
  // page (reusing the existing "walked off the end" empty state), never a
  // panic or a 500.
  const res = await page.goto("/catalog?page=18446744073709551615");
  expect(res?.status()).toBe(200);
  await hydrated(page);
  await expect(page.getByTestId("no-results")).toContainText(
    "Nothing on this page",
  );
  await expect(page.getByTestId("results-grid")).toHaveCount(0);
});

test("the strip shows a short form immediately, then upgrades once the count resolves @fast", async ({
  page,
  request,
}) => {
  // "the strip may briefly show the short form while the count resolves"
  // (maintainer ruling, 2026-08-15): `search_count` is a second, independent
  // request — `results` (and the near band it draws from) must not wait on
  // it. Delay the count route specifically and watch the true last-page
  // number arrive after the results already have.
  //
  // Must be a *client-side* query change, not a cold `page.goto`: SSR
  // resolves both `results` and `search_count` server-side and embeds them
  // for hydration, so a route interception armed before a cold load never
  // gets a chance to intercept anything — the race this test is about only
  // exists once the app is already running in the browser.
  const total = await searchCount(request, "t:instant");
  const pageSizeProbe = await search(request, { q: "t:instant" });
  const truePage = Math.ceil(total / pageSizeProbe.cards.length);
  expect(truePage, "fixture must have a real last page beyond the near band")
    .toBeGreaterThan(6);

  await page.goto("/catalog");
  await hydrated(page);

  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  await page.route("**/api/search_catalog_count*", async (route) => {
    await gate;
    await route.continue();
  });

  await page.fill("#catalog-query", "t:instant");
  await page.waitForURL((url) => url.searchParams.get("q") === "t:instant");
  // Results render without waiting on the held count request.
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(page.getByTestId(`pager-page-${truePage}`)).toHaveCount(0);

  release();
  await expect(page.getByTestId(`pager-page-${truePage}`)).toBeVisible();
  await expect(page.getByTestId(`pager-page-${truePage}`)).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("the true last page of a filtered query disables Next @fast", async ({
  page,
  request,
}) => {
  // `t:planeswalker c:g` is a two-page fixture at today's catalog size (95
  // rows / 60-row pages) — small enough to walk to its real last page inside
  // one test. If catalog growth changes that shape, the `expect` below is the
  // signal to pick a new query rather than the test silently testing page one.
  const q = "t:planeswalker c:g";
  const first = await search(request, { q });
  expect(first.next_cursor, `${q} must run past one page`).toBeTruthy();
  const rest = await search(request, { q, cursor: first.next_cursor! });
  expect(rest.next_cursor, `${q} must fit exactly two pages`).toBeNull();

  await page.goto(`/catalog?q=${encodeURIComponent(q)}`);
  await hydrated(page);
  await page.getByTestId("pager-next").click();
  await expect(page.getByTestId("pager-current")).toHaveText("2");

  // `next_cursor` is empty — this *is* the last page, knowable with no
  // `COUNT` at all (`page_strip`'s `last = Some(page)` case).
  await expect(page.getByTestId("pager-next")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  // Only two pages total: `page_strip(2, Some(2))` is the `last <= 6` shortcut
  // — every number shown, no ellipsis, nothing past page 2.
  await expect(page.getByTestId("pager-page-1")).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.getByTestId("pager-page-3")).toHaveCount(0);
});

// ---------------------------------------------------------------------------
// The header's filtered count (WB-01M0324HQ12B590CZ0YXJPB5T6) — the
// browse-all "N cards in the catalog." line's counterpart once a query
// applies. Reuses `search_count` (round 2's own resource, above), so these
// tests are mostly about *rendering* honesty: matches the true total, stays
// off the critical path, and doesn't repeat the empty state's own verdict.
// ---------------------------------------------------------------------------

test("a filtered query's header count matches the true total and survives a reload @fast", async ({
  page,
  request,
}) => {
  const total = await searchCount(request, "bolt");
  expect(total, "fixture must have real hits for this probe query").toBeGreaterThan(0);

  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  const count = page.getByTestId("catalog-count");
  await expect(count).toHaveText(`${total} cards match.`);
  // The unfiltered sentence is a different element and must not also render.
  await expect(page.getByText(/cards in the catalog/)).toHaveCount(0);

  // The number survives a fresh SSR too, not just the client-side upgrade.
  await page.reload();
  await expect(page.getByTestId("catalog-count")).toHaveText(
    `${total} cards match.`,
  );
});

test("the header count reserves nothing before it resolves, then upgrades in place @fast", async ({
  page,
}) => {
  // Mirrors "the strip shows a short form immediately..." above:
  // `search_count` is a second, independent request, and the header's own
  // `<Transition>` is a separate boundary from `Results`' — a held count must
  // not hold up the cards, and must not show a stale/wrong number meanwhile
  // (here: no number at all, since this is the query's first-ever resolve).
  await page.goto("/catalog");
  await hydrated(page);

  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  await page.route("**/api/search_catalog_count*", async (route) => {
    await gate;
    await route.continue();
  });

  await page.fill("#catalog-query", "bolt");
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
  await expect(page.getByTestId("results-grid")).toBeVisible();
  // No reserved space and nothing rendered while the count is still in flight.
  await expect(page.getByTestId("catalog-count")).toHaveCount(0);

  release();
  await expect(page.getByTestId("catalog-count")).toHaveText(/cards match\./);
});

test("a zero-hit query suppresses the header count; the empty state carries the message @fast", async ({
  page,
}) => {
  // `NoResults` already says "No cards match that search." in the body; the
  // header repeating the same verdict a second time above the grid would be
  // one fact stated twice, not two facts.
  const q = "zzznonexistentcardnamequery12345";
  await page.goto(`/catalog?q=${q}`);
  await hydrated(page);
  await expect(page.getByTestId("no-results")).toContainText(
    "No cards match that search.",
  );
  await expect(page.getByTestId("catalog-count")).toHaveCount(0);
});

test("a stale count dims instead of asserting itself over a newer empty result (WB-01M0324HQ12B590CZ0YXJPB5T6 round 2) @fast", async ({
  page,
}) => {
  // `results` and `search_count` are two independent, similar-latency round
  // trips — either can resolve first. Hold only the count route (mirroring
  // "the strip shows a short form immediately..." above) so `results` wins
  // the race: the body settles into the zero-hit empty state while the
  // header is still carrying the *previous* query's number. That old number
  // must not keep asserting itself as if it described what's on screen.
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  const count = page.getByTestId("catalog-count");
  await expect(count).toHaveText(/cards match\./);
  await expect(count).not.toHaveAttribute("data-stale");
  const staleText = await count.textContent();

  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  await page.route("**/api/search_catalog_count*", async (route) => {
    await gate;
    await route.continue();
  });

  await page.fill("#catalog-query", "zzznonexistentcardnamequery12345");
  await page.waitForURL(
    (url) => url.searchParams.get("q") === "zzznonexistentcardnamequery12345",
  );
  // `search_catalog` was never held: the empty state is already up.
  await expect(page.getByTestId("no-results")).toContainText(
    "No cards match that search.",
  );
  // The header is still showing "bolt"'s count, unchanged — and now marked
  // stale, not presented as an authoritative fact about the empty page
  // directly below it.
  await expect(count).toHaveText(staleText!);
  await expect(count).toHaveAttribute("data-stale", "true");

  release();
  // The fresh count is zero, which is silent (the empty state above already
  // owns that message) — so the line clears rather than un-dimming to
  // "0 cards match."
  await expect(count).toHaveCount(0);
});

test.describe("authed", () => {
  test.use({ storageState: AUTH_STATE });

  test("a signed-in visitor sees the same catalog-wide filtered count @fast", async ({
    page,
    request,
  }) => {
    // `search_catalog_count` answers from `*Backend::anonymous()` regardless
    // of session — catalog-wide, not ownership-scoped — so the number must
    // match what an anonymous request gets for the same query.
    const total = await searchCount(request, "bolt");
    await page.goto("/catalog?q=bolt");
    await hydrated(page);
    await expect(page.getByTestId("catalog-count")).toHaveText(
      `${total} cards match.`,
    );
  });
});

test.describe("mobile", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("the filter sheet's footer count is qualified too @fast", async ({
    page,
    request,
  }) => {
    // The sheet's "Show N results" is the same claim as the toolbar count and
    // was built from the same unqualified row count (P6-132).
    const one = await search(request, { q: "bolt", limit: 1 });
    const rest = await search(request, { q: "bolt", cursor: one.next_cursor! });

    await page.goto(`/catalog?q=bolt&cursor=${one.next_cursor}`);
    await hydrated(page);
    await page.getByRole("button", { name: /Filters/ }).click();
    // The SheetContent carries the open state; a closed sheet is still
    // "visible" to Playwright (it slides via a transform).
    const panel = page.locator('[data-name=SheetContent][aria-label="Filters"]');
    await expect(panel).toHaveAttribute("data-state", "open");
    await expect(panel).toContainText(
      `Show ${rest.cards.length} results on this page`,
    );
  });
});

// ---------------------------------------------------------------------------
// Wide-viewport grid & page size (WB-01M033AFA0VSCGB8Z3HTYPFZVD, maintainer
// report from a 2560x1440 monitor). Two paired complaints, one task:
//
// - The grid used to freeze at `max-w-7xl` (1280px — exactly half of
//   2560px) the moment `xl:grid-cols-6` took over, so a wide monitor's
//   catalog page visibly wasted the right half of the screen. The cap is
//   gone; `GRID_CLASS` now adds a `3xl:grid-cols-10` tier (custom 2200px
//   breakpoint, `style/input.css`) instead, so six columns stop growing
//   before they would look comically oversized.
// - The page size moved from 50 to 60 for the same "wastes space unevenly"
//   reason: 50 divides evenly by only 2 and 5 of the grid's column counts;
//   60 divides evenly by 2, 3, 4, 5, 6 *and* 10 — every full page tiles into
//   whole rows, at every breakpoint, with nothing left over.
//
// Every number below is read from the live API/DOM, never hardcoded — a
// future page-size or catalog-size change must not need an edit here.
// ---------------------------------------------------------------------------

/// Distinct `grid-template-columns` track count at whatever viewport is
/// current — the ground truth for which `GRID_CLASS` tier is active, read
/// off the resolved computed style rather than re-deriving it from a
/// Tailwind breakpoint name (which would just be testing the test).
async function gridColumnCount(
  page: import("@playwright/test").Page,
): Promise<number> {
  const grid = page.getByTestId("results-grid");
  await expect(grid).toBeVisible();
  return grid.evaluate(
    (el) =>
      getComputedStyle(el).gridTemplateColumns.trim().split(/\s+/).length,
  );
}

test("a full page holds 60 results, not 50 @fast", async ({ page }) => {
  // Browse-all (`/catalog`, no `q`) is far larger than one page at today's
  // catalog size, so this reads the real full-page tile count straight off
  // the DOM — the UI's own promise, not just the API's.
  await page.goto("/catalog");
  await hydrated(page);
  await expect(page.locator("[data-testid=results-grid] li")).toHaveCount(
    60,
  );
});

test("the grid fills a 2560px viewport instead of freezing near 1280px, and jumps to 10 columns @fast", async ({
  page,
}) => {
  await page.setViewportSize({ width: 2560, height: 1440 });
  await page.goto("/catalog");
  await hydrated(page);
  const grid = page.getByTestId("results-grid");
  const box = await grid.boundingBox();
  // The retired `max-w-7xl` cap froze this near 1280px regardless of
  // viewport; a grid still reading near that here is the literal bug
  // report, not a fluke of the sidebar/padding.
  expect(
    box?.width,
    "grid must not still be capped near the old 1280px max-w-7xl",
  ).toBeGreaterThan(2000);
  expect(await gridColumnCount(page)).toBe(10);
});

test("6 columns hold at an ordinary wide viewport (~1500px) @fast", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1500, height: 900 });
  await page.goto("/catalog");
  await hydrated(page);
  expect(await gridColumnCount(page)).toBe(6);
});

test("no horizontal overflow at phone width (390px) @fast", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/catalog");
  await hydrated(page);
  expect(await gridColumnCount(page)).toBe(2);
  const overflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
  );
  expect(overflow, "the page scrolls sideways at 390px").toBeLessThanOrEqual(
    0,
  );
});

test("the numbered pager's derived last page satisfies last*pageSize >= total @fast", async ({
  page,
  request,
}) => {
  // Browse-all again: guaranteed enough rows for a real multi-page
  // last-page computation. This is #151's "page 773 of 38623 at 50/page"
  // scenario, generalized — the exact page/total numbers move with catalog
  // growth and the page-size bump, so nothing here is pinned to either.
  const q = "";
  const total = await searchCount(request, q);
  const first = await search(request, { q });
  const pageSize = first.cards.length;
  expect(
    pageSize,
    "browse-all must return a full page to derive a page size from",
  ).toBeGreaterThan(0);
  const lastPage = Math.ceil(total / pageSize);
  expect(lastPage * pageSize).toBeGreaterThanOrEqual(total);
  expect((lastPage - 1) * pageSize).toBeLessThan(total);

  // The server agrees: the derived last page is truly last (no further
  // `next_cursor`), and one page past it is empty.
  const lastPageResults = await search(request, { q, page: lastPage });
  expect(
    lastPageResults.next_cursor,
    "the derived last page must be the true last page",
  ).toBeNull();
  const oneBeyond = await search(request, { q, page: lastPage + 1 });
  expect(
    oneBeyond.cards.length,
    "one page past the derived last must be empty",
  ).toBe(0);

  // And the live pager agrees too: jumping straight to it disables Next.
  await page.goto(`/catalog?page=${lastPage}`);
  await hydrated(page);
  await expect(page.getByTestId("pager-next")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

// ---------------------------------------------------------------------------
// The "N owned" badge — authed only. `CardSummary::owned` is data-model's
// global per-user, per-oracle count (specs/collection-api.md read models), and
// `null` on the wire means *unknown*, which is a different claim from 0.
//
// The regression this locks is a silent one: catalog search never filled the
// column, so the badge was unreachable and a signed-in catalog rendered
// identically to an anonymous one. An authed-only assertion would not have
// caught that — both halves are asserted here, against the same cards.
//
// `t:creature` is the seed's own picking query (app/src/seed.rs), so its first
// page is where the fixture's holdings are; counts are read from the API rather
// than hardcoded, because re-seeding moves them.
// ---------------------------------------------------------------------------

const OWNED_QUERY = "t:creature";

/// The owned badge on one card's tile, located through the oracle id its link
/// carries (the grid has no per-tile id of its own).
const ownedBadgeFor = (
  page: import("@playwright/test").Page,
  oracleId: string,
) =>
  page
    .locator("[data-testid=results-grid] li")
    .filter({ has: page.locator(`a[href="/cards/${oracleId}"]`) })
    .getByTestId("owned-badge");

test("an anonymous catalog reports owned as unknown and badges nothing @fast", async ({
  page,
  request,
}) => {
  const anon = await search(request, { q: OWNED_QUERY, limit: 15 });
  expect(anon.cards.length).toBeGreaterThan(0);
  // Strictly `null`, not falsy: a projection that defaulted the column to 0
  // would render the same badge-free page as this one, so only the wire value
  // distinguishes "unknown" from "you hold none of these".
  for (const c of anon.cards) {
    expect(
      c.owned,
      `owned for ${c.name} must be null when anonymous`,
    ).toBeNull();
  }

  await page.goto(`/catalog?q=${encodeURIComponent(OWNED_QUERY)}`);
  await hydrated(page);
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(page.getByTestId("owned-badge")).toHaveCount(0);
});

test.describe("authed", () => {
  test.use({ storageState: AUTH_STATE });

  test("the tile badges your own copies, and agrees with the card page @fast", async ({
    page,
  }) => {
    // `page.request` shares the page context's cookies, so this is the same
    // session the render below uses — not a second, anonymous one.
    const mine = await search(page.request, { q: OWNED_QUERY, limit: 15 });
    for (const c of mine.cards) {
      expect(
        c.owned,
        `owned for ${c.name} must be filled when authed`,
      ).not.toBeNull();
    }

    // Fixture check, not a behavior check: if the seed gave every card the same
    // count, "the badge shows the right number" would be unfalsifiable.
    const distinct = new Set(
      mine.cards.map((c) => c.owned).filter((n) => n! > 0),
    );
    expect(
      distinct.size,
      "seeded owned counts are all identical — re-run scripts/seed-dev-data.sh",
    ).toBeGreaterThan(1);

    const top = mine.cards.reduce((a, b) => (b.owned! > a.owned! ? b : a));
    expect(top.owned!).toBeGreaterThan(0);
    const none = mine.cards.find((c) => c.owned === 0);
    expect(
      none,
      "no authed-but-unowned card here — the 0-vs-null distinction goes untested",
    ).toBeTruthy();

    // SSR, before any JS: the number is server-rendered, not filled in after
    // hydration by a second request.
    const url = `/catalog?q=${encodeURIComponent(OWNED_QUERY)}`;
    const html = await (await page.request.get(url)).text();
    expect(html).toContain('data-testid="owned-badge"');
    expect(html).toContain(`${top.owned} owned`);

    await page.goto(url);
    await hydrated(page);
    await expect(ownedBadgeFor(page, top.oracle_id)).toHaveText(
      `${top.owned} owned`,
    );
    // `Some(0)` is a real answer and still no badge — "0 owned" would be noise
    // on every card you don't have.
    await expect(ownedBadgeFor(page, none!.oracle_id)).toHaveCount(0);

    // The list view projects the same column and must not have been forgotten.
    await page.goto(`${url}&view=list`);
    await hydrated(page);
    await expect(
      page.getByTestId("results-list").getByTestId("owned-badge").first(),
    ).toBeVisible();

    // The number itself, pinned to an independently-computed one: the detail
    // page's total is summed from the per-collection *ownership* rows (a
    // different query from the `owned_by_card` read behind the badge), so a
    // wrong-but-plausible count in either place fails here.
    await page.goto(`/cards/${top.oracle_id}`);
    await hydrated(page);
    await expect(page.getByTestId("your-copies")).toContainText(
      `Your copies · ${top.owned}`,
    );
  });
});
