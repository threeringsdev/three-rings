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
// **These tests read; others write the same rows.** `destination-picker.spec.ts`
// fires real `+ Have` / `+ Want` against this same dev user in a parallel
// worker (its `+ Want` is not even undoable — specs/app-ui.md Findings), and
// `/my` aggregates *every* collection, so a concurrent add lands in this page
// between an API snapshot and the render it is compared against. Observed as a
// firefox-only failure the first time the full tier ran, for exactly that
// reason and not for anything browser-specific.
//
// So every API-cross-checked assertion runs inside [`agrees`], which re-reads
// the API *and* re-renders the page on each attempt: a real projection bug
// fails all attempts, while a mutation that lands mid-attempt is gone by the
// next one. Assertions that need no API snapshot (the empty states, the junk
// cursor) are left un-retried.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

/// Inner assertions get a short timeout: a failed attempt must surface fast
/// enough for `toPass` to retry inside the test's own budget. `expect.configure`
/// returns a *callable* expect — `quick(locator).toHaveText(…)`; there is no
/// `.expect` on it.
const quick = expect.configure({ timeout: 2000 });

/// Re-read `/my` from the API and from the page together, and assert they
/// agree — retrying the pair so a concurrent write cannot fail the run. See
/// the file header.
async function agrees(
  page: Page,
  request: APIRequestContext,
  body: (view: View) => Promise<void>,
  q = "",
): Promise<void> {
  await expect(async () => {
    const view = await allCards(request, q);
    await page.goto(q ? `/my?q=${encodeURIComponent(q)}` : "/my");
    await hydrated(page);
    await body(view);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
}

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

test("the table SSRs every row server-side @fast", async ({ request }) => {
  // Request-level: the rows must be in the raw response, before any JS. Under
  // the default out-of-order streaming this page would ship a skeleton with
  // the content parked in a <template>, so assert the skeleton's *absence*
  // too — that is the half that catches an accidental SsrMode change.
  await expect(async () => {
    const raw = await (await request.get("/my")).text();
    expect(raw).toContain('data-testid="all-cards-table"');
    expect(raw).not.toContain('aria-label="Loading your cards"');

    const view = await allCards(request);
    expect(view.cards.length).toBeGreaterThan(0);

    // EVERY row, not "at least one". Counting was the fix for a gap the Codex
    // mutation pass found: a `.take(1)` in `CardsTable` left the table, a row,
    // and the API's first card all present in the markup, so a `toContain`
    // pair could not tell one row from fifty.
    const ssrOracles = [...raw.matchAll(/data-oracle="([0-9a-f-]+)"/g)].map(
      (m) => m[1],
    );
    expect(ssrOracles).toEqual(view.cards.map((r) => r.card.oracle_id));
    expect(raw).toContain(view.cards[view.cards.length - 1].card.name);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

/// What a row should render, derived from the API row — the expectation half.
function expectedCells(row: Row) {
  const owned = row.locations.reduce((s, l) => s + l.quantity, 0);
  const where =
    row.locations.length === 0
      ? "—"
      : row.locations.length === 1
        ? `${row.locations[0].quantity} in ${row.locations[0].collection_name}`
        : `${owned} across ${row.locations.length} collections`;
  return {
    oracle: row.card.oracle_id,
    where,
    wanted: row.wanted > 0 ? String(row.wanted) : "—",
    owned: owned > 0 ? String(owned) : "—",
  };
}

/// Every rendered row's three columns, in document order — one round trip, so
/// this stays fast as the fixture grows past a page.
async function renderedCells(page: Page) {
  return page.$$eval('[data-testid="all-cards-row"]', (trs) =>
    trs.map((tr) => ({
      oracle: tr.getAttribute("data-oracle") ?? "",
      where:
        tr
          .querySelector('[data-testid="location-summary"]')
          ?.textContent?.trim() ?? "",
      wanted:
        tr.querySelector('[data-testid="wanted-count"]')?.textContent?.trim() ??
        "",
      owned:
        tr.querySelector('[data-testid="owned-count"]')?.textContent?.trim() ??
        "",
    })),
  );
}

test("three columns agree with the aggregate read @fast", async ({
  page,
  request,
}) => {
  await agrees(page, request, async (view) => {
    // OWNED is the sum of the locations — the invariant the DTO leans on by
    // deriving `owned()` instead of storing a second copy of it. Asserted on
    // the payload itself, since the page can only ever show one of the two.
    for (const row of view.cards) {
      const summed = row.locations.reduce((s, l) => s + l.quantity, 0);
      expect(row.card.owned ?? 0, `owned for ${row.card.name}`).toBe(summed);
    }
    expect(await renderedCells(page)).toEqual(view.cards.map(expectedCells));
  });
});

type ShoppingRow = {
  oracle_id: string;
  name: string;
  desired_total: number;
  owned: number;
};

/// Cards the caller wants more of than they hold, read from the shopping list.
///
/// The shopping list is the *independent witness* here: it computes the same
/// cross-collection desire total, and the same owned total, through its own
/// CTEs over `desires`/`holdings` rather than through the projection under
/// test. Note "on the shopping list" does NOT imply "held nowhere" — it means
/// `desired > owned` — so callers filter on `owned === 0` for that.
async function shoppingList(
  request: APIRequestContext,
): Promise<ShoppingRow[]> {
  const res = await request.get("/api/shopping-list");
  expect(res.status()).toBe(200);
  const { rows } = (await res.json()) as { rows: ShoppingRow[] };
  expect(
    rows.length,
    "dev seed should carry short wants (scripts/seed-dev-data.sh)",
  ).toBeGreaterThan(0);
  return rows;
}

test("a wanted-but-held-nowhere card is a row, and WANTED is the cross-collection total @fast", async ({
  page,
  request,
}) => {
  // Two contracts in one pass, because they want the same fixture rows:
  //
  // - a card held in no collection is still a row. Without the FULL OUTER JOIN
  //   in `all_cards` it does not exist at all, which is the failure asserted
  //   here — not merely a wrong number;
  // - WANTED is the cross-collection *sum*, cross-checked against the shopping
  //   list. The other tests take their expected WANTED from the very projection
  //   the page renders, so a regression that summed wrong (a max, or one
  //   collection's count) would agree with itself (Codex review, this task).
  //
  // Each card is reached through the quick search by name, so nothing depends
  // on where the page-size boundary happens to fall. The shopping list is
  // re-read inside the retry: `destination-picker.spec.ts` can add a Have to
  // one of these cards mid-run, which legitimately moves it off `owned === 0`.
  await expect(async () => {
    const shopping = await shoppingList(request);
    expect(
      shopping.filter((r) => r.owned === 0).length,
      "dev seed should want at least one card it holds nowhere",
    ).toBeGreaterThan(0);

    for (const short of shopping) {
      const view = await allCards(request, short.name);
      const row = view.cards.find((r) => r.card.oracle_id === short.oracle_id);
      expect(row, `${short.name} missing from /my`).toBeTruthy();
      expect(row!.wanted, `wanted total for ${short.name}`).toBe(
        short.desired_total,
      );

      await page.goto(`/my?q=${encodeURIComponent(short.name)}`);
      await hydrated(page);
      const tr = rowFor(page, short.oracle_id);
      await quick(tr).toBeVisible();
      await quick(tr.locator('[data-testid="wanted-count"]')).toHaveText(
        String(short.desired_total),
      );

      if (short.owned === 0) {
        expect(row!.locations, `${short.name} should be held nowhere`).toEqual(
          [],
        );
        await quick(tr.locator('[data-testid="owned-count"]')).toHaveText(
          "\u2014",
        );
        await quick(tr.locator('[data-testid="location-summary"]')).toHaveText(
          "\u2014",
        );
      }
    }
  }).toPass({ timeout: 45_000, intervals: [500, 1_000, 2_000] });
});

test("the location summary expands to the collections it names @fast", async ({
  page,
  request,
}) => {
  await agrees(page, request, async (view) => {
    const multi = view.cards.find((r) => r.locations.length > 1);
    expect(
      multi,
      "dev seed should hold at least one card in two collections",
    ).toBeTruthy();

    const tr = rowFor(page, multi!.card.oracle_id);
    const list = tr.locator('[data-testid="location-list"]');
    const content = page.locator(`#locations-${multi!.card.oracle_id}`);

    // A closed panel keeps its DOM (the grid animation needs it there), so
    // "collapsed" is data-state + `inert` — the same pair the tree asserts —
    // not absence. Height is asserted on the outer track, not the inner list:
    // the list keeps its intrinsic height and is clipped, and any padding put
    // on the content wrapper would leak into the closed height.
    const trigger = tr.locator("button[aria-expanded]");
    await quick(trigger).toHaveAttribute("aria-expanded", "false");
    await quick(content).toHaveAttribute("data-state", "closed");
    expect(await content.evaluate((el) => (el as HTMLElement).inert)).toBe(
      true,
    );
    expect(await content.boundingBox().then((b) => b?.height ?? 0)).toBe(0);

    await trigger.click();
    await quick(trigger).toHaveAttribute("aria-expanded", "true");
    await quick(content).toHaveAttribute("data-state", "open");
    expect(await content.evaluate((el) => (el as HTMLElement).inert)).toBe(
      false,
    );
    for (const loc of multi!.locations) {
      await quick(
        list.locator("li", {
          hasText: `${loc.quantity} · ${loc.collection_name}`,
        }),
      ).toHaveCount(1);
    }
    // Each entry links at the collection it names.
    await quick(
      list.locator(
        `a[href="/my/collections/${multi!.locations[0].collection_id}"]`,
      ),
    ).toHaveCount(1);
  });
});

test("a single-collection row links instead of disclosing @fast", async ({
  page,
  request,
}) => {
  await agrees(page, request, async (view) => {
    const single = view.cards.find((r) => r.locations.length === 1);
    expect(
      single,
      "dev seed should hold a card in exactly one collection",
    ).toBeTruthy();

    const tr = rowFor(page, single!.card.oracle_id);
    // No disclosure: the summary already names the one collection, so
    // expanding would only repeat it.
    await quick(tr.locator("button[aria-expanded]")).toHaveCount(0);
    await quick(tr.locator('[data-testid="location-summary"]')).toHaveAttribute(
      "href",
      `/my/collections/${single!.locations[0].collection_id}`,
    );
  });
});

test("quick search filters by name and rides the URL @fast", async ({
  page,
  request,
}) => {
  const view = await allCards(request);
  // A needle the first card matches and most others do not.
  const needle = view.cards[0].card.name.slice(0, 6);

  await expect(async () => {
    const expected = await allCards(request, needle);
    expect(expected.cards.length).toBeGreaterThan(0);

    await page.goto("/my");
    await hydrated(page);
    await page.locator("#my-query").fill(needle);

    // The URL is the query: the debounce moves it, and the rows follow the
    // URL. Assert on the decoded param rather than a literal string: the app's
    // own `encode_query_value` (app/src/catalog.rs) deliberately percent-encodes
    // more characters than JS's `encodeURIComponent` (e.g. `!`), a documented
    // choice, not a bug — a punctuation-heavy needle (the full catalog can
    // make card 0 be `"Ach! Hans, Run!"`) must not fail on encoder choice.
    await expect
      .poll(() => new URL(page.url()).searchParams.get("q"), { timeout: 2000 })
      .toBe(needle);
    expect(new URL(page.url()).pathname).toBe("/my");
    await quick(page.locator('[data-testid="all-cards-row"]')).toHaveCount(
      expected.cards.length,
    );
    await quick(rowFor(page, expected.cards[0].card.oracle_id)).toBeVisible();
    // The filter really narrowed: fewer rows than the unfiltered page.
    expect(expected.cards.length).toBeLessThan(view.cards.length);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });

  const expected = await allCards(request, needle);

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

test("Next page walks to the second page and back @fast", async ({
  page,
  request,
}) => {
  // The pager's own link, clicked. This is only reachable because the dev seed
  // carries a bulk box larger than the 50-row page (app/src/seed.rs `BULK_CARDS`)
  // — added for exactly this: the Codex review of this task pointed out that
  // with a sub-page fixture every paging assertion deep-linked a cursor from
  // the JSON route, so `Pager`'s href could have pointed anywhere.
  await expect(async () => {
    const first = await allCards(request);
    expect(
      first.next_cursor,
      "dev seed must exceed one page — re-run scripts/seed-dev-data.sh",
    ).toBeTruthy();
    const second = await allCards(request, "", first.next_cursor!);
    expect(second.cards.length).toBeGreaterThan(0);

    await page.goto("/my");
    await hydrated(page);
    await quick(page.locator('[data-testid="page-first"]')).toHaveCount(0);
    await page.locator('[data-testid="page-next"]').click();

    // The URL carries the cursor the API handed out, and the rows are the ones
    // that follow page one — not page one again.
    await quick(page).toHaveURL(
      `/my?cursor=${encodeURIComponent(first.next_cursor!)}`,
    );
    await quick(
      page.locator('[data-testid="all-cards-row"]').first(),
    ).toHaveAttribute("data-oracle", second.cards[0].card.oracle_id);
    await quick(rowFor(page, first.cards[0].card.oracle_id)).toHaveCount(0);

    // Off page one the pager offers a way home, and it lands on page one.
    await page.locator('[data-testid="page-first"]').click();
    await quick(page).toHaveURL("/my");
    await quick(page.locator('[data-testid="page-first"]')).toHaveCount(0);
    await quick(rowFor(page, first.cards[0].card.oracle_id)).toBeVisible();
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("?cursor= is honored on a cold load @fast", async ({ page, request }) => {
  // Deep-linked rather than clicked: a shared or reloaded URL must SSR the
  // same page. Uses a small-page cursor so the assertion does not depend on
  // where the 50-row boundary happens to fall.
  await expect(async () => {
    const first = await allCardsPaged(request, 3);
    expect(first.next_cursor, "a 3-row page must not be the last").toBeTruthy();
    const rest = await allCards(request, "", first.next_cursor!);
    expect(rest.cards.length).toBeGreaterThan(0);

    // Request-level: the cursor page must be in the raw response too.
    const url = `/my?cursor=${encodeURIComponent(first.next_cursor!)}`;
    const raw = await (await request.get(url)).text();
    expect(raw).toContain(`data-oracle="${rest.cards[0].card.oracle_id}"`);
    expect(raw).not.toContain(`data-oracle="${first.cards[0].card.oracle_id}"`);

    await page.goto(url);
    await hydrated(page);

    // The page starts *after* the cursor: none of the first three come back…
    for (const row of first.cards) {
      await quick(rowFor(page, row.card.oracle_id)).toHaveCount(0);
    }
    // …and the row that follows them leads.
    await quick(
      page.locator('[data-testid="all-cards-row"]').first(),
    ).toHaveAttribute("data-oracle", rest.cards[0].card.oracle_id);
    await quick(page.locator('[data-testid="page-first"]')).toHaveCount(1);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
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

test.describe("reached by a client-side navigation", () => {
  // A regression from #74 (the mobile `/my` root), shipping on `main` when this
  // was written: arriving at `/my` by an in-app **click** rendered
  // "You haven't added any cards yet." on an account with 100 cards, having made
  // **zero** requests.
  //
  // `leptos_server`'s `initial_value()` reads `__RESOLVED_RESOURCES[<next
  // monotonic id>]` for every `Resource::new` — it never checks the
  // `during_hydration()` flag leptos itself maintains — so a resource created
  // during a client-side navigation reads a slot belonging to the page just left.
  // `SsrMode::Async` serializes three times at disjoint id ranges and the client
  // consumes only the first, so the rest are unclaimed. The collection page
  // leaves quick-add's `{"Ok":{"cards":[],"next_cursor":null}}` at ids 8/12/16,
  // and that is byte-identical to an empty `AllCardsView`. #74 inserted
  // `MyRootNav`'s `<Suspense>` ahead of `AllCardsBody`, shifting this resource's
  // post-navigation id by +1 — off a hole (11) and onto a landmine (12).
  // Measured: dropping slot 12 fixes it, dropping 8 or 16 does not, and removing
  // `MyRootNav` fixes it. `AllCardsPayload`'s named field is the fix.
  //
  // **This must be a click, not a `goto`** — a cold load consumes the *first* id
  // range and is correct, which is why the bug hid for a whole task.
  test("shows the real rows, and actually fetches them @fast", async ({
    page,
  }) => {
    const calls: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("all_cards")) calls.push(r.url());
    });

    // Start somewhere whose leftover payload is the landmine. Trade Binder is
    // the measured case; any collection page serializes the same shape.
    const tree = await page.request.get("/api/collection_tree");
    expect(tree.ok()).toBeTruthy();
    const rows = (
      (await tree.json()) as { collections: { summary: { id: string; name: string } }[] }
    ).collections;
    const trade = rows.find((r) => r.summary.name === "Trade Binder")?.summary;
    expect(trade, "the dev seed must contain Trade Binder").toBeTruthy();
    await page.goto(`/my/collections/${trade!.id}`);
    await hydrated(page);

    // A real in-app navigation to /my specifically — not the sidebar's own
    // `All cards` row, which now targets /my/all everywhere (P6-154: it is
    // shared with the mobile drawer, where /my is the drill-down root list,
    // not the table). The regression this test guards was tied to `/my`'s own
    // component order (`MyRootNav`'s `<Suspense>` ahead of `AllCardsBody`
    // shifting the resource's serialized id) and `/my/all` mounts no
    // `MyRootNav` ahead of it, so only a click that actually lands on `/my`
    // still exercises that mechanism — the desktop mode switch does.
    await page.locator('nav[aria-label="Mode"] a[href="/my"]').click();
    await expect(page).toHaveURL(/\/my$/);

    // Rows are on screen…
    await expect(page.locator('[data-testid="all-cards-row"]').first()).toBeVisible();
    // …and the empty state is not. Both directions, because "some rows exist"
    // and "the empty state is gone" fail differently.
    await expect(page.locator('[data-testid="all-cards-empty"]')).toHaveCount(0);
    // …and they came from the server. Without this a correctly-guessed cached
    // payload would pass every assertion above — which is exactly how the bug
    // shipped, only with a wrong guess.
    expect(
      calls.length,
      "/my reached by a click must fetch, not read a leftover payload",
    ).toBeGreaterThan(0);
  });
});

// The anonymous `/my` bounce is asserted in smoke.spec.ts ("anonymous /my
// bounces to login with a return path") and is not repeated here. Note also
// that `browser.newContext()` inside a spec is NOT anonymous — Playwright
// applies the file's `test.use({ storageState })` to it — so an anonymous
// case belongs in a file that isn't signed in, which smoke.spec.ts is.

test.describe("mobile — a long collection name does not widen the table (P6-020)", () => {
  // `/my` below `md` is the drill-down list (app/src/my/root.rs); only
  // `/my/all` (`ALL_CARDS_PATH`) renders this table at phone width, which is
  // the whole point — the 390px case is exactly where P6-001 measured 0px
  // for the seeded fixture, a result that says nothing about a name longer
  // than anything in that fixture.
  test.use({ viewport: { width: 390, height: 844 } });

  test("@fast the WHERE cell truncates a long unbreakable name instead of overflowing", async ({
    page,
    request,
  }) => {
    // One 60-ish char token, no spaces or hyphens: nothing for the browser's
    // default line-breaking to grab onto. This is the exact shape the fix
    // targets — a card name or a fixed vocabulary word always has *some*
    // break opportunity, but a user-chosen collection name does not, and
    // `min-content` width under `table-layout: auto` is driven by the
    // longest unbreakable token in a column, not by any fixed budget.
    const token = Array.from(
      { length: 50 },
      () => "abcdefghijklmnopqrstuvwxyz0123456789"[Math.floor(Math.random() * 36)],
    ).join("");
    const longName = `zzE2eOverflow${token}W${test.info().workerIndex}`;

    // A card this account holds *nowhere* yet, so adding one `have` makes a
    // brand-new, single-location row — the `"{n} in {name}"` link shape this
    // test exists to measure, not the multi-collection disclosure. `q=z`
    // (not `n`, which other files already draw down to nothing — see
    // removal.spec.ts): a bulk `/api/all-cards?limit=…` snapshot cannot be
    // trusted to catch every owned oracle once the fixture exceeds the page
    // size, so each candidate is checked individually against `/api/all_cards`
    // (the page's own un-paged read) instead of one bulk pre-filter.
    const search = await request.get("/api/catalog/search?q=z&limit=100");
    expect(search.status(), "catalog search").toBe(200);
    const { cards } = (await search.json()) as {
      cards: { oracle_id: string; printing_id: string | null; name: string }[];
    };
    let card:
      | { oracle_id: string; printing_id: string | null; name: string }
      | undefined;
    for (const c of cards) {
      if (!c.printing_id) continue;
      const check = await request.get(`/api/all_cards?q=${encodeURIComponent(c.name)}`);
      expect(check.status(), `all_cards for ${c.name}`).toBe(200);
      const { cards: rows } = (await check.json()) as {
        cards: { card: { oracle_id: string }; locations: unknown[] }[];
      };
      const row = rows.find((r) => r.card.oracle_id === c.oracle_id);
      if (!row || row.locations.length === 0) {
        card = c;
        break;
      }
    }
    expect(
      card,
      "the fixture should have a catalog card this account owns nowhere",
    ).toBeTruthy();

    const created = await request.post("/api/collections", {
      data: { parent_id: null, kind: "binder", name: longName, format: null },
    });
    expect(created.status(), "create scratch collection").toBe(200);
    const collection = (await created.json()) as { id: string; name: string };

    try {
      const have = await request.post(`/api/collections/${collection.id}/have`, {
        data: { printing_id: card!.printing_id, quantity: 1 },
      });
      expect(have.status(), "add have").toBe(200);

      await page.goto(`/my/all?q=${encodeURIComponent(card!.name)}`);
      await hydrated(page);

      const row = rowFor(page, card!.oracle_id);
      await expect(row).toBeVisible();
      const cell = row.locator('[data-testid="location-summary"]');

      // The invariant this task lands, checked first: no viewport-width
      // dependence on user data length. Measure the scroll container, not
      // the document — `TableWrapper` is `overflow-auto`, so a too-wide
      // table is a wrapper-local scroll the document check alone misses (the
      // same trap collection-view.spec.ts's mobile no-scroll test
      // documents).
      const table = page.locator('[data-testid="all-cards-table"]');
      await expect(table).toHaveCount(1);
      const wrapper = await table.evaluate((el) => {
        const w = el.closest('[data-name="TableWrapper"]');
        if (!w) throw new Error("the table has no TableWrapper to scroll in");
        return { overflow: w.scrollWidth - w.clientWidth, client: w.clientWidth };
      });
      expect(wrapper.client, "table wrapper has no width").toBeGreaterThan(0);
      expect(
        wrapper.overflow,
        "a long collection name should not widen the table",
      ).toBeLessThanOrEqual(1);

      const doc = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(doc, "page overflows the document").toBeLessThanOrEqual(1);

      // Base case: the cell is genuinely clipping content — proof the
      // truncation CSS is actually engaged here, not merely that a short
      // name happened to fit inside whatever width the column landed on.
      const cellOverflow = await cell.evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(
        cellOverflow,
        "the WHERE cell should be clipping the long name (base: overflow > 0)",
      ).toBeGreaterThan(0);

      await expect(cell).toHaveAttribute("title", `1 in ${longName}`);
    } finally {
      // Discard: the default `ToParent` disposition on a top-level scratch
      // collection relocates its one holding to the Inbox rather than
      // destroying it, and soft-deletes the collection itself (P6-188).
      await request.post(`/api/collections/${collection.id}/delete`, { data: {} });
    }
  });
});
