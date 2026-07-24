// `/my` — the All-cards everything-view (specs/app-ui.md → "`/my`").
//
// The load-bearing contracts, in assertion order:
//
// - the table SSRs (rows in the raw HTML, not fetched in) — `/my` is
//   `SsrMode::Async` precisely so this holds;
// - the three columns say what the aggregate read says: the location summary
//   is the per-collection breakdown, WANTED is the desire total, OWNED is the
//   sum of the locations. Every number is cross-checked against the API rather
//   than hardcoded, because the dev seed is re-runnable;
// - a card you *want* but hold nowhere is still a row (the FULL OUTER JOIN);
// - the location summary expands to the collections it names, and only the
//   multi-collection form gets a disclosure;
// - quick search is URL-canonical, debounced, and drops the page cursor;
// - `?cursor=` is honored, "Back to the start" returns, and a junk cursor is a
//   rendered error rather than a crash.
//
// These tests only READ the dev user's data, so they are safe to run in
// parallel with the mutating specs — with one caveat honored below: a
// concurrent spec may add or delete a scratch collection between an API read
// and a page render, so anything cross-checked against the API is read from
// the same request the assertion uses, and per-row numbers are asserted
// row-by-row rather than as a page total.

import { expect, test, type APIRequestContext } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

type Location = {
  collection_id: string;
  collection_name: string;
  quantity: number;
};
type Row = {
  card: { oracle_id: string; name: string; owned: number | null };
  wanted: number;
  locations: Location[];
};
type View = { cards: Row[]; next_cursor: string | null };

/// The page's own adapter — the exact call `AllCardsPage` makes.
async function allCards(
  request: APIRequestContext,
  q = "",
  cursor?: string,
): Promise<View> {
  const url =
    `/api/all_cards?q=${encodeURIComponent(q)}` +
    (cursor === undefined ? "" : `&cursor=${encodeURIComponent(cursor)}`);
  const res = await request.get(url);
  expect(res.status(), `GET ${url}`).toBe(200);
  return (await res.json()) as View;
}

/// The hosted JSON route, which (unlike the server fn) takes a page size —
/// the only way to get a real mid-set cursor out of a 19-card fixture.
async function allCardsPaged(
  request: APIRequestContext,
  limit: number,
  cursor?: string,
): Promise<View> {
  const url =
    `/api/all-cards?limit=${limit}` +
    (cursor === undefined ? "" : `&cursor=${encodeURIComponent(cursor)}`);
  const res = await request.get(url);
  expect(res.status(), `GET ${url}`).toBe(200);
  return (await res.json()) as View;
}

const rowFor = (page: import("@playwright/test").Page, oracleId: string) =>
  page.locator(`[data-testid="all-cards-row"][data-oracle="${oracleId}"]`);

test("the table SSRs its rows server-side @fast", async ({ request }) => {
  // Request-level: the rows must be in the raw response, before any JS. Under
  // the default out-of-order streaming this page would ship a skeleton with
  // the content parked in a <template>, so assert the skeleton's *absence*
  // too — that is the half that catches an accidental SsrMode change.
  const raw = await (await request.get("/my")).text();
  expect(raw).toContain('data-testid="all-cards-table"');
  expect(raw).toContain('data-testid="all-cards-row"');
  expect(raw).not.toContain('aria-label="Loading your cards"');

  const view = await allCards(request);
  expect(view.cards.length).toBeGreaterThan(0);
  expect(raw).toContain(view.cards[0].card.name);
});

test("three columns agree with the aggregate read @fast", async ({
  page,
  request,
}) => {
  const view = await allCards(request);
  await page.goto("/my");
  await hydrated(page);

  await expect(page.locator('[data-testid="all-cards-row"]')).toHaveCount(
    view.cards.length,
  );

  for (const row of view.cards) {
    const tr = rowFor(page, row.card.oracle_id);
    const owned = row.locations.reduce((s, l) => s + l.quantity, 0);
    // OWNED is the sum of the locations — the invariant the DTO leans on by
    // deriving `owned()` instead of storing a second copy of it.
    expect(row.card.owned ?? 0, `owned for ${row.card.name}`).toBe(owned);

    await expect(tr.locator('[data-testid="owned-count"]')).toHaveText(
      owned > 0 ? String(owned) : "—",
    );
    await expect(tr.locator('[data-testid="wanted-count"]')).toHaveText(
      row.wanted > 0 ? String(row.wanted) : "—",
    );

    const summary = tr.locator('[data-testid="location-summary"]');
    if (row.locations.length === 0) {
      await expect(summary).toHaveText("—");
    } else if (row.locations.length === 1) {
      await expect(summary).toHaveText(
        `${row.locations[0].quantity} in ${row.locations[0].collection_name}`,
      );
    } else {
      await expect(summary).toHaveText(
        `${owned} across ${row.locations.length} collections`,
      );
    }
  }
});

test("a card wanted but held nowhere is still a row @fast", async ({
  page,
  request,
}) => {
  // The seed's two "short" wants: desired in the deck, held in no collection.
  // Without the FULL OUTER JOIN in `all_cards` these rows do not exist at all,
  // which is the failure this asserts — not merely a wrong number.
  const view = await allCards(request);
  const wantOnly = view.cards.filter(
    (r) => r.locations.length === 0 && r.wanted > 0,
  );
  expect(
    wantOnly.length,
    "dev seed should carry short wants (scripts/seed-dev-data.sh)",
  ).toBeGreaterThan(0);

  await page.goto("/my");
  await hydrated(page);
  for (const row of wantOnly) {
    const tr = rowFor(page, row.card.oracle_id);
    await expect(tr).toBeVisible();
    await expect(tr.locator('[data-testid="owned-count"]')).toHaveText("—");
    await expect(tr.locator('[data-testid="location-summary"]')).toHaveText(
      "—",
    );
    await expect(tr.locator('[data-testid="wanted-count"]')).toHaveText(
      String(row.wanted),
    );
  }
});

test("the location summary expands to the collections it names @fast", async ({
  page,
  request,
}) => {
  const view = await allCards(request);
  const multi = view.cards.find((r) => r.locations.length > 1);
  expect(
    multi,
    "dev seed should hold at least one card in two collections",
  ).toBeTruthy();

  await page.goto("/my");
  await hydrated(page);
  const tr = rowFor(page, multi!.card.oracle_id);
  const list = tr.locator('[data-testid="location-list"]');
  const content = page.locator(`#locations-${multi!.card.oracle_id}`);

  // A closed panel keeps its DOM (the grid animation needs it there), so
  // "collapsed" is data-state + `inert` — the same pair the tree asserts —
  // not absence, and not the inner list's height (that stays intrinsic; it is
  // the outer track that collapses).
  const trigger = tr.locator("button[aria-expanded]");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await expect(content).toHaveAttribute("data-state", "closed");
  expect(await content.evaluate((el) => (el as HTMLElement).inert)).toBe(true);
  expect(await content.boundingBox().then((b) => b?.height ?? 0)).toBe(0);

  await trigger.click();
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  await expect(content).toHaveAttribute("data-state", "open");
  expect(await content.evaluate((el) => (el as HTMLElement).inert)).toBe(false);
  for (const loc of multi!.locations) {
    await expect(
      list.locator("li", { hasText: `${loc.quantity} · ${loc.collection_name}` }),
    ).toHaveCount(1);
  }
  // Each entry links at the collection it names.
  await expect(
    list.locator(`a[href="/my/collections/${multi!.locations[0].collection_id}"]`),
  ).toHaveCount(1);
});

test("a single-collection row links instead of disclosing @fast", async ({
  page,
  request,
}) => {
  const view = await allCards(request);
  const single = view.cards.find((r) => r.locations.length === 1);
  expect(single, "dev seed should hold a card in exactly one collection")
    .toBeTruthy();

  await page.goto("/my");
  await hydrated(page);
  const tr = rowFor(page, single!.card.oracle_id);
  // No disclosure: the summary already names the one collection, so expanding
  // would only repeat it.
  await expect(tr.locator("button[aria-expanded]")).toHaveCount(0);
  await expect(tr.locator('[data-testid="location-summary"]')).toHaveAttribute(
    "href",
    `/my/collections/${single!.locations[0].collection_id}`,
  );
});

test("quick search filters by name and rides the URL @fast", async ({
  page,
  request,
}) => {
  const view = await allCards(request);
  const target = view.cards[0].card.name;
  // A needle that this card matches and (almost certainly) most others do not.
  const needle = target.slice(0, 6);
  const expected = await allCards(request, needle);
  expect(expected.cards.length).toBeGreaterThan(0);
  expect(expected.cards.length).toBeLessThan(view.cards.length);

  await page.goto("/my");
  await hydrated(page);
  await page.locator("#my-query").fill(needle);

  // The URL is the query: the debounce moves it, and the rows follow the URL.
  await expect(page).toHaveURL(`/my?q=${encodeURIComponent(needle)}`);
  await expect(page.locator('[data-testid="all-cards-row"]')).toHaveCount(
    expected.cards.length,
  );
  await expect(rowFor(page, expected.cards[0].card.oracle_id)).toBeVisible();

  // And a cold load of that URL SSRs the same filtered page.
  const raw = await (
    await request.get(`/my?q=${encodeURIComponent(needle)}`)
  ).text();
  expect(raw).toContain(expected.cards[0].card.name);
});

test("a search that matches nothing says so @fast", async ({ page }) => {
  await page.goto("/my?q=zzzzz-no-such-card");
  await hydrated(page);
  await expect(page.locator('[data-testid="all-cards-empty"]')).toContainText(
    "No cards of yours match that search",
  );
  await expect(page.locator('[data-testid="all-cards-table"]')).toHaveCount(0);
});

test("a wildcard typed into the search is literal @fast", async ({ page }) => {
  // `%` is a LIKE wildcard; if it reached the SQL unescaped this would match
  // every card instead of none. Same escaping helper as /catalog's bare terms.
  await page.goto("/my?q=%25");
  await hydrated(page);
  await expect(page.locator('[data-testid="all-cards-empty"]')).toBeVisible();
});

test("?cursor= renders the next keyset page @fast", async ({
  page,
  request,
}) => {
  const first = await allCardsPaged(request, 3);
  expect(first.next_cursor, "a 3-row page must not be the last").toBeTruthy();
  const rest = await allCards(request, "", first.next_cursor!);
  expect(rest.cards.length).toBeGreaterThan(0);

  await page.goto(`/my?cursor=${encodeURIComponent(first.next_cursor!)}`);
  await hydrated(page);

  // The page starts *after* the cursor: none of the first three come back…
  for (const row of first.cards) {
    await expect(rowFor(page, row.card.oracle_id)).toHaveCount(0);
  }
  // …and the row that follows them leads.
  await expect(page.locator('[data-testid="all-cards-row"]').first()).toHaveAttribute(
    "data-oracle",
    rest.cards[0].card.oracle_id,
  );

  // Off page one, the pager offers a way home; page one does not show it.
  await page.locator('[data-testid="page-first"]').click();
  await expect(page).toHaveURL("/my");
  await expect(page.locator('[data-testid="page-first"]')).toHaveCount(0);
  await expect(rowFor(page, first.cards[0].card.oracle_id)).toBeVisible();
});

test("editing the search drops the page cursor @fast", async ({
  page,
  request,
}) => {
  const first = await allCardsPaged(request, 3);
  expect(first.next_cursor).toBeTruthy();
  await page.goto(`/my?cursor=${encodeURIComponent(first.next_cursor!)}`);
  await hydrated(page);

  // A new filter has no page two yet — carrying the old cursor forward would
  // page into a result set that no longer exists.
  await page.locator("#my-query").fill("a");
  await expect(page).toHaveURL("/my?q=a");
});

test("a junk cursor is a rendered error, not a crash @fast", async ({
  page,
}) => {
  await page.goto("/my?cursor=not-a-real-cursor");
  await hydrated(page);
  await expect(page.locator('[data-testid="all-cards-error"]')).toContainText(
    "Couldn't load your cards",
  );
});

// The anonymous `/my` bounce is asserted in smoke.spec.ts ("anonymous /my
// bounces to login with a return path") and is not repeated here. Note also
// that `browser.newContext()` inside a spec is NOT anonymous — Playwright
// applies the file's `test.use({ storageState })` to it — so an anonymous
// case belongs in a file that isn't signed in, which smoke.spec.ts is.
