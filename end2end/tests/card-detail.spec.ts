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
//   thing from "signed in and owning nothing", and its total agrees with the
//   `owned` the catalog badge is drawn from;
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
/// An adventure: two oracle faces, ONE image — the layout class that must NOT
/// get a flip control. Flippability is keyed off `layout`, not face count
/// (shared::has_back_face), and this is the card that tells the two apart.
const ADVENTURE_QUERY = "Brazen Borrower";

type FaceSummary = {
  name: string;
  mana_cost: string | null;
  type_line: string | null;
  image_uri: string | null;
};

type Summary = {
  oracle_id: string;
  name: string;
  image_uri: string | null;
  owned: number | null;
  faces: FaceSummary[];
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
  // The load-bearing half. Under Leptos's default out-of-order streaming the
  // body still *contains* all of the above — inside a <template> the client
  // hoists — while the in-place markup is the skeleton. Asserting the skeleton
  // is gone is what distinguishes real SSR from "it's in there somewhere"
  // (this assertion failed before the route took SsrMode::Async).
  expect(html, "page streamed a skeleton instead of SSR-ing").not.toContain(
    'aria-label="Loading card"',
  );
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
  }) => {
    // The dev seed puts holdings on the first hits of `t:creature`
    // (app/src/seed.rs), and an authed search now *says which* — `owned` is
    // filled on every search hit (the owned-badge task; it was `None` on all of
    // them before, which is why this used to walk four candidate cards hoping
    // one had holdings). `page.request` carries the page context's session.
    const cards = await search(page.request, "t:creature");
    const card = cards.find((c) => (c.owned ?? 0) > 0);
    expect(
      card,
      "no seeded holdings on the creatures — run scripts/seed-dev-data.sh",
    ).toBeTruthy();

    await page.goto(`/cards/${card!.oracle_id}`);
    await hydrated(page);
    const section = page.getByTestId("your-copies");
    await expect(section).toBeVisible();
    // The header total is summed from this page's own per-collection ownership
    // rows — a different query from the `owned_by_card` read behind
    // `CardSummary::owned` — so this doubles as the check that the catalog's
    // badge and this page cannot disagree.
    await expect(section).toContainText(`Your copies · ${card!.owned}`);
    // Every copy is somewhere: the collections are named and linked...
    const rows = section.locator("li");
    await expect(rows.first().locator("a[href^='/my/collections/']")).toBeVisible();
    // ...and each row carries its own quantity. Asserting only the header
    // total would pass with every per-location count missing or wrong
    // (Codex review, low) — so check the rows sum to the header.
    const counts = await rows.evaluateAll((els) =>
      els.map((el) => Number(el.lastElementChild?.textContent?.trim())),
    );
    expect(counts.length).toBeGreaterThan(0);
    expect(counts.every((n) => Number.isInteger(n) && n > 0)).toBe(true);
    expect(counts.reduce((a, b) => a + b, 0)).toBe(card!.owned);
  });
});

test.describe("DFC flip", () => {
  test("the detail page flips both the art and the oracle block @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, DFC_QUERY);
    // The projection contract rides along: a DFC summary carries both faces,
    // each with its own art (front/back of one printing).
    expect(card.faces, "search projection lost the flip faces").toHaveLength(2);
    const [front, back] = card.faces;
    expect(front.image_uri).not.toBe(back.image_uri);

    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);

    // The heading is the *face* name; the combined "Front // Back" identity
    // stays on the page as a subtitle (and is what SSR/search matched on).
    const h1 = page.getByTestId("card-name");
    await expect(h1).toHaveText(front.name);
    await expect(page.getByTestId("card-combined-name")).toHaveText(card.name);

    const art = page.locator("img").first();
    const frontSrc = await art.getAttribute("src");
    // Scryfall URLs encode the face position — /front/ vs /back/ — which is
    // what proves the art is paired with the *right* face, not merely
    // different. (The detail hero may be a different printing than the search
    // summary's representative one, so exact equality with the API faces
    // would over-constrain; the path segment can't lie either way. A swapped
    // face→art index survives a mere "src changed" assertion — Codex mutation
    // pass.)
    expect(frontSrc).toMatch(/\/front\//);
    const frontOracle = await page.getByTestId("card-oracle-text").textContent();

    await page.getByTestId("card-flip").click();

    // Name, art, and oracle text all swapped — the whole block, not just the
    // image (the task's acceptance criteria).
    await expect(h1).toHaveText(back.name);
    const backSrc = await art.getAttribute("src");
    expect(backSrc).not.toBe(frontSrc);
    expect(backSrc).toMatch(/^https:\/\/cards\.scryfall\.io\//);
    expect(backSrc).toMatch(/\/back\//);
    const backOracle = await page.getByTestId("card-oracle-text").textContent();
    expect(backOracle).not.toBe(frontOracle);
    // A flip is not navigation.
    expect(new URL(page.url()).pathname).toBe(`/cards/${card.oracle_id}`);

    // ...and it cycles back to the front.
    await page.getByTestId("card-flip").click();
    await expect(h1).toHaveText(front.name);
    await expect(art).toHaveAttribute("src", frontSrc!);
  });

  test("an adventure gets no flip control — one image, keyed off layout @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, ADVENTURE_QUERY);
    expect(card.name).toContain("//"); // two oracle faces...
    expect(card.faces).toHaveLength(0); // ...but the projection says no flip
    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);
    await expect(page.getByTestId("card-flip")).toHaveCount(0);
    // Single heading, combined name — exactly the pre-task rendering.
    await expect(page.getByTestId("card-name")).toHaveText(card.name);
    await expect(page.getByTestId("card-combined-name")).toHaveCount(0);
  });

  test("a single-face card gets no flip control @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, SINGLE_FACE_QUERY);
    expect(card.faces).toHaveLength(0);
    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);
    await expect(page.getByTestId("card-flip")).toHaveCount(0);
  });

  test("the hover preview flips without closing or navigating @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, DFC_QUERY);
    await page.goto(`/catalog?q=${encodeURIComponent(DFC_QUERY)}&view=list`);
    await hydrated(page);

    await page.getByTestId("card-preview-trigger").first().hover();
    const hover = page.locator("[data-testid=card-preview-hover]").first();
    await expect(hover).toBeVisible();
    await expect(hover).toContainText(card.faces[0].name);

    await hover.getByTestId("card-flip").click();
    await expect(hover).toContainText(card.faces[1].name);
    // The preview art comes from the same representative printing as the
    // API summary, so exact equality is safe here — and it is what catches a
    // body that swaps the name but keeps the front art (Codex mutation pass).
    await expect(hover.locator("img")).toHaveAttribute(
      "src",
      card.faces[1].image_uri!,
    );
    // The click stayed inside the preview: still open, still on the catalog.
    await expect(hover).toBeVisible();
    expect(new URL(page.url()).pathname).toBe("/catalog");
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
  await expect(hoverBody).toBeHidden();
  // Lazily mounted — and this is the assertion that actually says so: the
  // popover element itself is always in the DOM, so only the absence of a
  // *second* copy of the card name proves the body hasn't rendered. Eager
  // bodies made getByText(name).first() resolve to a hidden node.
  await expect(page.getByText(SINGLE_FACE_QUERY, { exact: true })).toHaveCount(1);

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

test("a touch tap on a fine-pointer device still opens the sheet @fast", async ({
  page,
}) => {
  // Hybrid devices (touchscreen laptops) report `(pointer: coarse) === false`
  // while still taking finger taps, and Playwright cannot emulate that combo —
  // `hasTouch` flips the media query in all three engines. So drive the code
  // path directly: a real pointerdown carrying pointerType "touch" in an
  // otherwise fine-pointer context. Before the fix this followed the link
  // (Codex review, medium).
  await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`);
  await hydrated(page);
  expect(await page.evaluate(() => matchMedia("(pointer: coarse)").matches)).toBe(
    false,
  );

  // Both events synthetically: Playwright's own .click() emits a real *mouse*
  // pointerdown first, which would reset the touch intent before the click
  // lands. A synthetic click on an anchor still navigates if nothing calls
  // preventDefault, so the URL assertion below stays meaningful.
  const trigger = page.getByTestId("card-preview-trigger").first();
  await trigger.evaluate((el) => {
    el.dispatchEvent(
      new PointerEvent("pointerdown", { pointerType: "touch", bubbles: true }),
    );
    (el.querySelector("a") ?? el).dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true }),
    );
  });

  await expect(
    page.locator("[data-testid=card-preview-sheet][role=dialog]").first(),
  ).toHaveAttribute("data-state", "open");
  expect(new URL(page.url()).pathname).toBe("/catalog");
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
    // `data-state`, not toBeVisible: the sheet slides in via a transform and
    // stays in the layout when closed, so a closed sheet is "visible" to
    // Playwright too (found by mutation — app-ui Findings).
    await expect(sheet).toHaveAttribute("data-state", "open");
    await expect(sheet).toContainText(SINGLE_FACE_QUERY);
    // The tap was intercepted: still on the catalog, not the detail page.
    expect(new URL(page.url()).pathname).toBe("/catalog");

    // ...and the sheet is how you get to the page from here.
    await sheet.getByTestId("card-preview-full-details").click();
    await page.waitForURL((url) => url.pathname.startsWith("/cards/"));
    await expect(page.getByTestId("card-name")).toContainText(SINGLE_FACE_QUERY);
  });

  test("the sheet flips a DFC without closing or navigating @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, DFC_QUERY);
    await page.goto(`/catalog?q=${encodeURIComponent(DFC_QUERY)}`);
    await hydrated(page);

    await page.getByTestId("card-preview-trigger").first().click();
    const sheet = page.locator("[data-testid=card-preview-sheet][role=dialog]");
    await expect(sheet).toHaveAttribute("data-state", "open");
    await expect(sheet).toContainText(card.faces[0].name);

    await sheet.getByTestId("card-flip").click();
    await expect(sheet).toContainText(card.faces[1].name);
    // Exact art equality — same reasoning as the hover test: the sheet body
    // renders the representative printing's per-face art.
    await expect(sheet.locator("img")).toHaveAttribute(
      "src",
      card.faces[1].image_uri!,
    );
    // The flip tap must neither close the sheet nor follow the tile link.
    await expect(sheet).toHaveAttribute("data-state", "open");
    expect(new URL(page.url()).pathname).toBe("/catalog");
  });

  test("a coarse pointer over a row never raises a hover card @fast", async ({
    page,
  }) => {
    // Touch browsers fire a synthetic mouseenter, so a finger that merely
    // travels over a row — scrolling the list, say — would raise a hover card
    // that nothing then dismisses (there is no mouseleave until you touch
    // something else). That is what the hover_card `disabled` prop guards.
    //
    // Deliberately NO click here. Tapping happens to mask the bug: the sheet's
    // backdrop steals the pointer, whose mouseleave cancels the pending open —
    // so a tap-based assertion passes even with `disabled` removed (verified by
    // mutation, app-ui Findings).
    await page.goto(
      `/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`,
    );
    await hydrated(page);

    await page.getByTestId("card-preview-trigger").first().hover();
    await page.waitForTimeout(400); // well past the 150 ms hover intent
    await expect(
      page.locator("[data-testid=card-preview-hover]").first(),
    ).toBeHidden();
    // ...and the sheet did not open either — a hover is not a tap.
    await expect(
      page.locator("[data-testid=card-preview-sheet][role=dialog]"),
    ).toHaveAttribute("data-state", "closed");
  });
});
