// `/cards/:id` — the card detail page and the two preview affordances
// (specs/app-ui.md → "`/cards/:id`").
//
// The load-bearing contracts, in assertion order:
//
// - the full page SSRs (printings and rulings in the raw HTML, not fetched in);
// - multi-face cards carry an image, on the detail page *and* wherever else a
//   card image is projected — this is the COALESCE fallback, and it is the one
//   assertion here that fails against the old SQL;
// - "your copies" is present iff the caller is signed in, which is a different
//   thing from "signed in and owning nothing";
// - a malformed id is a rendered not-found, not a crash;
// - desktop hovers a preview; touch taps a sheet *instead of navigating*.
//
// Card ids are resolved at runtime through the search API rather than
// hardcoded: the POC catalog is re-ingestable, and a pinned UUID would rot.

import { expect, test, type APIRequestContext } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

/// A double-faced card. `image_uris` is NULL on every DFC printing (migration
/// 0002 puts the art under `faces`), so this is the card that renders imageless
/// without the projection fallback.
const DFC_QUERY = "Agadeem's Awakening";
const SINGLE_FACE_QUERY = "Lightning Bolt";

type Summary = {
  oracle_id: string;
  name: string;
  image_uri: string | null;
  owned: number | null;
};

async function search(
  request: APIRequestContext,
  q: string,
): Promise<Summary[]> {
  const res = await request.get(
    `/api/search_catalog?q=${encodeURIComponent(q)}`,
  );
  expect(res.status()).toBe(200);
  return (await res.json()).cards;
}

async function firstCard(
  request: APIRequestContext,
  q: string,
): Promise<Summary> {
  const cards = await search(request, q);
  expect(
    cards.length,
    `no catalog hit for "${q}" — is the POC catalog ingested on this branch?`,
  ).toBeGreaterThan(0);
  return cards[0];
}

test("card detail SSRs the card, its printings and its rulings @fast", async ({
  request,
}) => {
  const card = await firstCard(request, DFC_QUERY);
  // Request-level: no JS runs, so this markup is proof of SSR rather than of a
  // client-side fetch into an empty shell.
  const res = await request.get(`/cards/${card.oracle_id}`);
  expect(res.status()).toBe(200);
  const html = await res.text();

  expect(html).toContain('data-testid="card-detail"');
  expect(html).toContain(card.name);
  expect(html).toContain('data-testid="card-printings"');
  // This card has WotC rulings in the POC set; their absence would mean the
  // rulings query silently returned nothing.
  expect(html).toContain('data-testid="card-rulings"');
});

test("a multi-face card renders an image everywhere it is projected @fast", async ({
  request,
}) => {
  // The regression this locks: `image_uris->>'normal'` is NULL for every
  // double-faced printing, so before the COALESCE fallback both of these were
  // null/absent and DFCs showed a bare skeleton.
  const card = await firstCard(request, DFC_QUERY);
  expect(card.name).toContain("//"); // sanity: this really is a multi-face card
  expect(card.image_uri, "search projection lost the multi-face image").toMatch(
    /^https:\/\/cards\.scryfall\.io\//,
  );

  const html = await (await request.get(`/cards/${card.oracle_id}`)).text();
  expect(html, "detail projection lost the multi-face image").toContain(
    "https://cards.scryfall.io/",
  );
});

test("a malformed card id renders not-found rather than failing @fast", async ({
  page,
}) => {
  await page.goto("/cards/not-a-uuid");
  await hydrated(page);
  await expect(page.getByTestId("card-detail-missing")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Card not found" })).toBeVisible();
});

test("an anonymous visitor gets no your-copies section @fast", async ({
  page,
  request,
}) => {
  const card = await firstCard(request, SINGLE_FACE_QUERY);
  await page.goto(`/cards/${card.oracle_id}`);
  await hydrated(page);
  await expect(page.getByTestId("card-name")).toContainText(card.name);
  // `ownership` is None for anonymous callers — the section is absent, not empty.
  await expect(page.getByTestId("your-copies")).toHaveCount(0);
});

test.describe("authed", () => {
  test.use({ storageState: AUTH_STATE });

  test("a signed-in visitor gets the your-copies section @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, SINGLE_FACE_QUERY);
    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);
    // Present even at zero copies: `Some(vec![])` and `None` are different
    // answers, and only the anonymous one hides the section.
    await expect(page.getByTestId("your-copies")).toBeVisible();
  });

  test("owned copies show their collections and quantities @fast", async ({
    page,
    request,
  }) => {
    // The dev seed puts holdings on the first hits of `t:creature`
    // (app/src/seed.rs), and search orders by (name, oracle_id) — so the same
    // query here resolves to the same cards. Note it deliberately does NOT use
    // `CardSummary::owned`: the search projection never fills that column
    // (see app-ui Findings), so filtering on it would silently skip.
    const cards = await search(request, "t:creature");
    let found = false;

    for (const card of cards.slice(0, 4)) {
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);
      const section = page.getByTestId("your-copies");
      await expect(section).toBeVisible();
      const text = (await section.textContent()) ?? "";
      const match = /Your copies · (\d+)/.exec(text);
      expect(match, "your-copies rendered without a total").not.toBeNull();
      if (Number(match![1]) > 0) {
        // Every copy is somewhere: the collections are named and linked.
        await expect(
          section.locator("a[href^='/my/collections/']").first(),
        ).toBeVisible();
        found = true;
        break;
      }
    }

    expect(
      found,
      "no seeded holdings on the first creatures — run scripts/seed-dev-data.sh",
    ).toBe(true);
  });
});

test("hovering a list row opens a preview without changing the URL @fast", async ({
  page,
}) => {
  await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`);
  await hydrated(page);

  const hoverBody = page
    .locator("[data-testid=card-preview-hover]")
    .first();
  // Lazily mounted: nothing in the DOM until the pointer arrives.
  await expect(hoverBody).toBeHidden();

  await page.getByTestId("card-preview-trigger").first().hover();
  await expect(hoverBody).toBeVisible(); // 150 ms hover intent
  await expect(hoverBody).toContainText(SINGLE_FACE_QUERY);
  // A preview is not navigation.
  expect(new URL(page.url()).pathname).toBe("/catalog");
});

test("a grid tile offers no hover preview — it is already the art @fast", async ({
  page,
}) => {
  await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}`);
  await hydrated(page);
  await page.getByTestId("card-preview-trigger").first().hover();
  await page.waitForTimeout(400); // well past the 150 ms intent delay
  await expect(page.locator("[data-testid=card-preview-hover]")).toHaveCount(0);
});

test.describe("touch", () => {
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

  test("tapping a tile opens the sheet instead of navigating @fast", async ({
    page,
  }) => {
    await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}`);
    await hydrated(page);

    await page.getByTestId("card-preview-trigger").first().click();

    // The spread puts the testid on the backdrop as well as the panel; the
    // dialog is the one with the content in it.
    const sheet = page.locator("[data-testid=card-preview-sheet][role=dialog]");
    await expect(sheet).toBeVisible();
    await expect(sheet).toContainText(SINGLE_FACE_QUERY);
    // The tap was intercepted: still on the catalog, not the detail page.
    expect(new URL(page.url()).pathname).toBe("/catalog");

    // ...and the sheet is how you get to the page from here.
    await sheet.getByTestId("card-preview-full-details").click();
    await page.waitForURL((url) => url.pathname.startsWith("/cards/"));
    await expect(page.getByTestId("card-name")).toContainText(SINGLE_FACE_QUERY);
  });

  test("touch gets no hover card on top of the sheet @fast", async ({
    page,
  }) => {
    // Touch browsers fire a synthetic mouseenter on tap, so without the
    // hover card's `disabled` prop a tap would open both at once.
    await page.goto(
      `/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`,
    );
    await hydrated(page);

    await page.getByTestId("card-preview-trigger").first().click();
    await expect(
      page.locator("[data-testid=card-preview-sheet][role=dialog]"),
    ).toBeVisible();
    await page.waitForTimeout(400); // past the hover intent delay
    await expect(
      page.locator("[data-testid=card-preview-hover]"),
    ).toBeHidden();
  });
});
