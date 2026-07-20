import { expect, test } from "@playwright/test";
import { hydrated } from "./helpers";

// Filter rail + query↔rail sync (specs/app-ui.md "/catalog",
// specs/catalog-search.md "One filter state, two views over it").
//
// The contract, in the order the tests assert it:
//   the rail is a view over the query text, never a second source of truth ·
//   rail edits rewrite only their own term and preserve everything else
//   verbatim · query-bar terms reflect back into the widgets · multi-selects
//   serialize to comma-OR · a query the grammar rejects makes the rail inert
//   rather than wrong · mobile gets the same widgets in a sheet with a badge.
//
// The desktop rail is `hidden md:block`, so these run at the default desktop
// viewport; the mobile block sets its own.

const RAIL = "[data-testid=filter-rail]";
const q = (page: { url(): string }) =>
  new URL(page.url()).searchParams.get("q");

test("the rail reflects the URL query without JS @fast", async ({
  request,
}) => {
  // Request-level: rail state present in the raw HTML is proof the widgets are
  // derived from the query server-side, not filled in after hydration.
  const res = await request.get("/catalog?q=t%3Ainstant%20c%3Aur%20cmc%3C%3D2");
  expect(res.status()).toBe(200);
  const html = await res.text();
  // The wireframe's own example query and its badge counts.
  expect(html).toContain('data-testid="filter-count-color"');
  expect(html).toContain('aria-checked="true" aria-label="Instant"');
  expect(html).toContain('aria-checked="true" aria-label="Blue"');
  expect(html).toContain('aria-checked="true" aria-label="Red"');
  // ...and the text fields carry `value`, or a shared link would render an
  // empty-looking rail until wasm landed.
  expect(html).toMatch(/id="filter-rail-mv"[^>]*value="2"/);
});

test("checking a color rewrites its term in the URL and the box @fast", async ({
  page,
}) => {
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  const rail = page.locator(RAIL);

  await rail.getByRole("checkbox", { name: "Red" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt c:r");

  // The query bar is the canonical surface — it has to show what the rail did.
  await expect(page.locator("#catalog-query")).toHaveValue("bolt c:r");
  await expect(page.getByTestId("results-grid")).toBeVisible();

  // A second color joins the same term (`c:` means "has all of these"), it
  // does not append a second one.
  await rail.getByRole("checkbox", { name: "Blue" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt c:ru");
  await expect(page.locator("#catalog-query")).toHaveValue("bolt c:ru");
});

test("unchecking the last value removes the term entirely @fast", async ({
  page,
}) => {
  // Not `c:` with no value — that is a parse error, so a naive implementation
  // breaks the whole query when you uncheck the last box.
  await page.goto("/catalog?q=bolt%20c%3Ar");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await expect(rail.getByRole("checkbox", { name: "Red" })).toHaveAttribute(
    "aria-checked",
    "true",
  );

  await rail.getByRole("checkbox", { name: "Red" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
  await expect(page.getByTestId("search-error")).toHaveCount(0);
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("a rail edit preserves terms the rail does not own, verbatim @fast", async ({
  page,
}) => {
  // The load-bearing promise of the two-surface design: `id:` and negations
  // have no widget, so an edit elsewhere must leave them byte-for-byte intact
  // — including the alias spelling and the quoting the user chose.
  const start = 'id:wu -t:land o:"draw a card"';
  await page.goto(`/catalog?q=${encodeURIComponent(start)}`);
  await hydrated(page);
  await page.locator(RAIL).getByRole("checkbox", { name: "Blue" }).click();
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("c:u"),
  );

  expect(q(page)).toBe('id:wu -t:land o:"draw a card" c:u');
  await expect(page.locator("#catalog-query")).toHaveValue(
    'id:wu -t:land o:"draw a card" c:u',
  );
});

test("a multi-select serializes to one comma-OR term @fast", async ({
  page,
}) => {
  // Comma-OR is the whole reason the rail's facets can be multi-select: flat
  // syntax has no other way to say "instant OR sorcery".
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await rail.getByRole("checkbox", { name: "Instant" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "t:instant");
  await rail.getByRole("checkbox", { name: "Sorcery" }).click();
  await page.waitForURL(
    (url) => url.searchParams.get("q") === "t:instant,sorcery",
  );

  // And it is a real search, not just a string: both types come back.
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(rail.getByTestId("filter-count-type")).toContainText("2");
});

test("typing in the query bar reflects back into the widgets @fast", async ({
  page,
}) => {
  // The other direction of the two-way binding: the rail is a *view*, so a
  // term typed by hand has to check the box.
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  // Rarity ships collapsed (wireframe), and a closed <details> keeps its
  // contents out of the accessibility tree — so open it before asserting on
  // the boxes inside.
  await rail.locator("summary").filter({ hasText: "Rarity" }).click();
  await expect(rail.getByRole("checkbox", { name: "Rare" })).toHaveAttribute(
    "aria-checked",
    "false",
  );

  await page.fill("#catalog-query", "r:rare t:creature");
  await page.waitForURL(
    (url) => url.searchParams.get("q") === "r:rare t:creature",
  );

  await expect(rail.getByRole("checkbox", { name: "Rare" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await expect(
    rail.getByRole("checkbox", { name: "Creature" }),
  ).toHaveAttribute("aria-checked", "true");
  await expect(rail.getByTestId("filter-count-rarity")).toContainText("1");
});

test("the name and text boxes edit the query they were read from @fast", async ({
  page,
}) => {
  await page.goto("/catalog?q=t%3Ainstant");
  await hydrated(page);
  const rail = page.locator(RAIL);

  await rail.locator("#filter-rail-name").fill("bolt");
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("bolt"),
  );
  expect(q(page)).toBe("t:instant bolt");

  await rail.locator("#filter-rail-text").fill("draw a card");
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("o:"),
  );
  // A value with spaces must come back quoted, or it would split into three
  // name terms on the next parse.
  expect(q(page)).toBe('t:instant bolt o:"draw a card"');
  await expect(page.getByTestId("search-error")).toHaveCount(0);
});

test("the name box cannot smuggle a keyed term into the query @fast", async ({
  page,
}) => {
  // Typing `t:instant` into the field labelled "Card name" means a name
  // containing that text — it must not silently become a type filter.
  await page.goto("/catalog");
  await hydrated(page);
  await page.locator(RAIL).locator("#filter-rail-name").fill("t:instant");
  await page.waitForURL((url) => (url.searchParams.get("q") ?? "") !== "");
  expect(q(page)).toBe('"t:instant"');
  await expect(
    page.locator(RAIL).getByRole("checkbox", { name: "Instant" }),
  ).toHaveAttribute("aria-checked", "false");
});

test("mana value pairs a comparison with a number @fast", async ({ page }) => {
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await rail.locator("summary").filter({ hasText: "Mana value" }).click();
  await rail.getByLabel("Mana value comparison").selectOption("<=");
  await rail.locator("#filter-rail-mv").fill("2");
  await rail.locator("#filter-rail-mv").blur();
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("mv"),
  );
  // Whole numbers keep no `.0` — the query text is user-facing.
  expect(q(page)).toBe("bolt mv<=2");

  // Clearing the box removes the filter rather than searching mv:0, or the
  // filter could never be turned off from the rail.
  await rail.locator("#filter-rail-mv").fill("");
  await rail.locator("#filter-rail-mv").blur();
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
});

test("Reset clears the rail's filters but not the rest of the query @fast", async ({
  page,
}) => {
  const start = "bolt c:ur t:instant -t:land id:wu";
  await page.goto(`/catalog?q=${encodeURIComponent(start)}`);
  await hydrated(page);
  await page.locator(RAIL).getByRole("button", { name: "Reset" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "-t:land id:wu");
  // The hand-typed terms survive; the widgets are empty.
  await expect(page.locator("#catalog-query")).toHaveValue("-t:land id:wu");
  await expect(
    page.locator(RAIL).getByRole("checkbox", { name: "Blue" }),
  ).toHaveAttribute("aria-checked", "false");
  await expect(
    page.locator(RAIL).getByRole("button", { name: "Reset" }),
  ).toHaveCount(0);
});

test("a rejected query makes the rail inert instead of wrong @fast", async ({
  page,
}) => {
  // There is no honest way to reflect an unparseable query into widgets, and
  // rewriting one term of it would mean guessing which term is broken — so the
  // rail says so rather than rendering empty-but-clickable boxes that would
  // eat the user's text on the next click.
  await page.goto("/catalog?q=pow%3E3");
  await hydrated(page);
  await expect(page.getByTestId("search-error")).toBeVisible();
  await expect(
    page.locator(RAIL).getByTestId("filter-rail-inert"),
  ).toBeVisible();
  await expect(
    page.locator(RAIL).getByRole("checkbox", { name: "Blue" }),
  ).toHaveCount(0);

  // Fixing the query brings the widgets back.
  await page.fill("#catalog-query", "c:u");
  await page.waitForURL((url) => url.searchParams.get("q") === "c:u");
  await expect(page.locator(RAIL).getByTestId("filter-rail-inert")).toHaveCount(
    0,
  );
  await expect(
    page.locator(RAIL).getByRole("checkbox", { name: "Blue" }),
  ).toHaveAttribute("aria-checked", "true");
});

test("rail edits replace history rather than piling it up @fast", async ({
  page,
}) => {
  // Dragging down a facet list must not bury the previous page under one
  // history entry per checkbox.
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  // The first filter on a bare /catalog pushes, so Back returns to browse-all
  // rather than walking off the site...
  await rail.getByRole("checkbox", { name: "Red" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "c:r");
  // ...and every refinement after it replaces, so the intermediate states do
  // not each become a history entry.
  await rail.getByRole("checkbox", { name: "Blue" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "c:ru");
  await rail.getByRole("checkbox", { name: "Green" }).click();
  await page.waitForURL((url) => url.searchParams.get("q") === "c:rug");

  await page.goBack();
  await page.waitForURL((url) => url.pathname === "/catalog" && !url.search);
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test.describe("mobile", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("the filter sheet carries an active-filter badge @fast", async ({
    page,
  }) => {
    await page.goto("/catalog?q=t%3Ainstant%20c%3Aur%20cmc%3C%3D2");
    await hydrated(page);
    // Desktop rail is gone at this width; the sheet trigger takes its place.
    await expect(page.locator(RAIL)).toBeHidden();
    // 1 type + 2 colors + 1 mana value — the wireframe's own badge count.
    await expect(page.getByTestId("filter-badge")).toContainText("4");

    await page.getByRole("button", { name: /Filters/ }).click();
    const sheet = page.locator("[data-testid=filter-sheet]");
    await expect(sheet).toBeVisible();
    await expect(
      sheet.getByRole("checkbox", { name: "Instant" }),
    ).toHaveAttribute("aria-checked", "true");

    // The sheet edits the same query text the rail does.
    await sheet.getByRole("checkbox", { name: "Sorcery" }).click();
    await page.waitForURL((url) =>
      (url.searchParams.get("q") ?? "").includes("t:instant,sorcery"),
    );
    await expect(page.getByTestId("filter-badge")).toContainText("5");
  });
});
