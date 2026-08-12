import { expect, test, type APIRequestContext } from "@playwright/test";
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
  // ...and the fields carry `value`, or a shared link would render an
  // empty-looking rail until wasm landed. `bind:value` alone does NOT emit the
  // attribute — the `Input` primitive seeds it from `bind_value` (see its
  // bind_value arm). Both component shapes are asserted because they are two
  // separate render paths into that primitive:
  expect(html).toMatch(/id="filter-rail-mv"[^>]*value="2"/); // Input, number
});

test("a shared link SSRs its text into every bound field @fast", async ({
  request,
}) => {
  // The `bind:value`-renders-no-attribute trap, at the primitive. It is
  // invisible in the browser (the field just looks empty for a beat) and every
  // SSR'd form re-inherits it, so it is asserted request-level on both shapes.
  const html = await (await request.get("/catalog?q=bolt")).text();
  expect(html).toMatch(/id="catalog-query"[^>]*value="bolt"/); // InputGroupInput
  expect(html).toMatch(/id="filter-rail-name"[^>]*value="bolt"/); // Input, text
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
  // The rail is a view over the query, so it has to follow its own edit —
  // asserting only the URL would pass on a rail whose boxes never re-check.
  await expect(rail.getByRole("checkbox", { name: "Red" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await expect(rail.getByRole("checkbox", { name: "Blue" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await expect(rail.getByTestId("filter-count-color")).toContainText("2");
});

test("a facet click survives the query bar's debounce window @fast", async ({
  page,
}) => {
  // P6-086. The two surfaces that write this one `?q=` disagree for the whole
  // 250 ms after a keystroke: the query bar's timer holds the box text it
  // captured, the URL still holds the pre-typing query. A facet clicked in that
  // window committed against the URL's stale copy and was then overwritten when
  // the timer fired — the click silently undone a quarter-second later.
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  const rail = page.locator(RAIL);

  await page.locator("#catalog-query").fill("bolt mv<=2");
  // We really are inside the window: the URL has not moved, so the newest query
  // text exists *only* in the bar's armed timer. (Without this the test would
  // still pass on a build where the debounce had already fired, proving
  // nothing.)
  expect(q(page)).toBe("bolt");

  await rail.getByRole("checkbox", { name: "Red" }).click();

  // One navigation carrying both intents: what you typed plus what you clicked.
  await page.waitForURL(
    (url) => url.searchParams.get("q") === "bolt mv<=2 c:r",
  );

  // ...and the armed debounce does not undo it. Kill-verify: on base the timer
  // fires here with the pre-click text and the URL reverts to "bolt mv<=2" —
  // the facet edit gone — while the box is left reading "bolt c:r", which the
  // URL no longer says either.
  await page.waitForTimeout(600);
  expect(q(page)).toBe("bolt mv<=2 c:r");
  await expect(page.locator("#catalog-query")).toHaveValue("bolt mv<=2 c:r");
  await expect(rail.getByRole("checkbox", { name: "Red" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await expect(rail.getByTestId("filter-count-color")).toContainText("1");
  await expect(rail.getByTestId("filter-count-mana-value")).toContainText("1");
  await expect(page.getByTestId("search-error")).toHaveCount(0);
  await expect(page.getByTestId("results-grid")).toBeVisible();
});

test("Reset mid-debounce clears the filters instead of restoring them @fast", async ({
  page,
}) => {
  // Same race, the other rail writer: Reset drops the rail-owned terms, so a
  // debounce left armed would put every one of them straight back ~250 ms
  // later. The hand-typed `-t:land` survives, exactly as it does outside the
  // window.
  await page.goto("/catalog?q=bolt%20c%3Ar%20-t%3Aland");
  await hydrated(page);
  const rail = page.locator(RAIL);

  await page.locator("#catalog-query").fill("bolt c:r -t:land o:flying");
  expect(q(page)).toBe("bolt c:r -t:land");

  await rail.getByRole("button", { name: "Reset" }).click();
  // Reset saw the typed `o:flying` (it is the newest query text) and cleared it
  // along with the rest of the rail's terms.
  await page.waitForURL((url) => url.searchParams.get("q") === "-t:land");

  await page.waitForTimeout(600);
  expect(q(page)).toBe("-t:land");
  await expect(page.locator("#catalog-query")).toHaveValue("-t:land");
  await expect(rail.getByRole("checkbox", { name: "Red" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
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
  // ...and the box unchecks with it, rather than staying lit over a query that
  // no longer carries the filter.
  await expect(rail.getByRole("checkbox", { name: "Red" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  await expect(rail.getByTestId("filter-count-color")).toHaveCount(0);
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

test("a repeated facet key keeps the term the rail did not show @fast", async ({
  page,
}) => {
  // The rail shows the FIRST `c:` only, so the second is hand-written text by
  // its own rule and must survive an edit — read() and rewrite() disagreeing
  // here is silent data loss (Codex review, high).
  await page.goto("/catalog?q=c%3Au%20c%3Ar");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await expect(rail.getByRole("checkbox", { name: "Blue" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await expect(rail.getByTestId("filter-count-color")).toContainText("1");

  await rail.getByRole("checkbox", { name: "White" }).click();
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("c:uw"),
  );
  expect(q(page)).toBe("c:uw c:r");
});

test("a multi-select serializes to one comma-OR term @fast", async ({
  page,
  request,
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

  await expect(rail.getByTestId("filter-count-type")).toContainText("2");

  // And it is a real OR against the catalog, not just a string the UI echoes.
  // Name-narrowed so the numbers stay well under the 50-row first page (paging
  // is a later task). Counts are read from the API rather than pinned: the
  // full-catalog bulk load put many more "bolt"-named instants/sorceries in
  // play (19 instants, 15 sorceries as of writing, not the POC catalog's
  // single "Lightning Bolt") — the durable invariant isn't a literal count,
  // it's that instant/sorcery are mutually exclusive types, so a comma-OR's
  // union must equal the sum of the two singles at any catalog size.
  const countFor = async (query: string) => {
    await page.goto(`/catalog?q=${encodeURIComponent(query)}`);
    await hydrated(page);
    await expect(page.getByTestId("results-grid")).toBeVisible();
    return await page.locator("[data-testid=results-grid] li").count();
  };
  const apiCountFor = async (query: string) => {
    const res = await request.get(
      `/api/catalog/search?q=${encodeURIComponent(query)}&limit=200`,
    );
    expect(res.status(), `GET ${query}`).toBe(200);
    const body = (await res.json()) as { cards: unknown[]; next_cursor: string | null };
    expect(body.next_cursor, `"${query}" should fit on one page`).toBeNull();
    return body.cards.length;
  };

  const instantCount = await apiCountFor("bolt t:instant");
  const sorceryCount = await apiCountFor("bolt t:sorcery");
  expect(instantCount).toBeGreaterThan(0);
  expect(sorceryCount).toBeGreaterThan(0);

  expect(await countFor("bolt t:instant")).toBe(instantCount);
  expect(await countFor("bolt t:sorcery")).toBe(sorceryCount);
  expect(await countFor("bolt t:instant,sorcery")).toBe(
    instantCount + sorceryCount,
  );
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
  // Inert means the whole widget set is gone — not just one checkbox. A rail
  // still showing Reset or the text fields would happily eat the broken query
  // on the next click.
  await expect(
    page.locator(RAIL).getByRole("checkbox", { name: "Blue" }),
  ).toHaveCount(0);
  await expect(
    page.locator(RAIL).getByRole("button", { name: "Reset" }),
  ).toHaveCount(0);
  await expect(page.locator("#filter-rail-name")).toHaveCount(0);
  await expect(page.locator("#filter-rail-mv")).toHaveCount(0);

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

// ---------------------------------------------------------------------------
// The Set facet as a real picker (specs/app-ui.md → Filter rail;
// specs/catalog-search.md → the `s:` term).
//
// The design these tests hold in place: **the selection is the query text, and
// the fetched list is only a way to discover codes.** Chips and ✓ marks read the
// parsed `s:` term and never intersect it with the rows, which is what makes a
// code the list has never heard of impossible to silently drop.
//
// Fixture facts, taken from the Neon dev catalog itself rather than from the
// code under test (`select code, name from sets …`): `mh3` is "Modern Horizons
// 3", `lea` is "Limited Edition Alpha", and the name "Modern Horizons 3" is a
// prefix of six *other* set names (Commander, Tokens, Promos, …) — so a search
// for it returns several rows, which is what lets a per-row assertion about
// selection mean anything. No set's code or name contains "xyz".
// ---------------------------------------------------------------------------

const SET_SEARCH = "#filter-rail-set";
const chip = (page: import("@playwright/test").Page, code: string) =>
  page.locator(`${RAIL} [data-testid=set-chip][data-code=${code}]`);
const option = (page: import("@playwright/test").Page, code: string) =>
  page.locator(`${RAIL} [data-testid=set-option][data-code=${code}]`);

test("a shared set link SSRs its selection, and a bare one fetches nothing @fast", async ({
  request,
}) => {
  // Request-level: chips in the raw HTML are proof the selection is derived from
  // the query text server-side — it cannot have come from the list, which is
  // fetched asynchronously.
  const html = await (
    await request.get("/catalog?q=id%3Awu%20s%3Amh3%2Clea")
  ).text();
  expect(html).toContain('data-testid="set-chip" data-code="mh3"');
  expect(html).toContain('data-testid="set-chip" data-code="lea"');
  // ...and the section is expanded because it carries filters, so the
  // not-yet-engaged hint is there too (P6-137 review) — this is the positive
  // control for the negative assertions below: it proves the markup exists to
  // be found when it should be.
  expect(html).toContain('data-testid="set-unengaged"');
  // The load-bearing negative: `s:mh3,lea` used to auto-open the section AND
  // eagerly fetch+SSR the *entire* set list (~1,047 rows) behind it, twice
  // (once for the desktop rail, once for the mobile sheet, which mounts
  // off-screen rather than unmounting while closed) — every shared/refreshed
  // set-filtered link paid for that unconditionally. The chip alone is what a
  // shared link needs to *look* right; the row list is deferred until someone
  // actually engages the picker (focus, hover, or a real disclosure toggle —
  // see `SetPicker`'s doc), so it must not be here.
  expect(html).not.toContain('data-testid="set-option"');

  // On a bare /catalog the facet still renders, but nothing is selected and the
  // list has not been fetched — the section is collapsed, and `/catalog` is the
  // app's most-loaded route.
  const bare = await (await request.get("/catalog")).text();
  expect(bare).toContain(">Set<"); // positive control: the facet is on the page
  expect(bare).not.toContain('data-testid="set-chip"');
  expect(bare).not.toContain('data-testid="set-option"');
});

test("a set term reflects into the picker's rows @fast", async ({ page }) => {
  await page.goto("/catalog?q=s%3Amh3");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await expect(rail.getByTestId("filter-count-set")).toContainText("1");
  await expect(chip(page, "mh3")).toBeVisible();

  // Search by *name* — the point of the picker. Several rows come back, and the
  // fixture is what makes the assertion sharp: exactly the selected one carries
  // the marker while its same-named siblings do not, so a widget that marked
  // everything (or nothing) fails here.
  await rail.locator(SET_SEARCH).fill("modern horizons 3");
  await expect(option(page, "mh3")).toHaveAttribute("data-selected", "true");
  await expect(option(page, "mh3")).toHaveText("Modern Horizons 3");
  await expect(option(page, "m3c")).toHaveText("Modern Horizons 3 Commander");
  await expect(option(page, "m3c")).not.toHaveAttribute(
    "data-selected",
    "true",
  );
});

test("picking a set rewrites only its term @fast", async ({ page }) => {
  // The load-bearing promise: `id:`, a negation and someone's chosen quoting all
  // survive byte for byte, and the `s:` term is rewritten in place rather than
  // appended.
  const start = 'id:wu -t:land o:"draw a card" s:mh3 bolt';
  await page.goto(`/catalog?q=${encodeURIComponent(start)}`);
  await hydrated(page);
  const rail = page.locator(RAIL);

  await rail.locator(SET_SEARCH).fill("limited edition alpha");
  await expect(option(page, "lea")).toHaveText("Limited Edition Alpha");
  await option(page, "lea").click();
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("s:mh3,lea"),
  );

  expect(q(page)).toBe('id:wu -t:land o:"draw a card" s:mh3,lea bolt');
  // The query bar is the canonical surface, so it has to show what the rail did.
  await expect(page.locator("#catalog-query")).toHaveValue(
    'id:wu -t:land o:"draw a card" s:mh3,lea bolt',
  );
  // The picker follows its own edit — asserting only the URL would pass on a
  // picker whose chips and ✓ never update.
  await expect(chip(page, "lea")).toBeVisible();
  await expect(chip(page, "mh3")).toBeVisible();
  await expect(option(page, "lea")).toHaveAttribute("data-selected", "true");
  await expect(rail.getByTestId("filter-count-set")).toContainText("2");
  await expect(page.getByTestId("search-error")).toHaveCount(0);
});

test("unpicking the last set removes the term entirely @fast", async ({
  page,
}) => {
  // Not `s:` with no value — that is a parse error, so a naive implementation
  // breaks the whole query when the last chip goes.
  await page.goto("/catalog?q=bolt%20s%3Amh3");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await expect(rail.getByTestId("filter-count-set")).toContainText("1");

  await chip(page, "mh3").click();
  await page.waitForURL((url) => url.searchParams.get("q") === "bolt");
  await expect(page.getByTestId("search-error")).toHaveCount(0);
  await expect(page.getByTestId("results-grid")).toBeVisible();
  // The section is still open and the search box still there — so the chips and
  // the badge being gone is a real absence, not a missing subtree.
  await expect(rail.locator(SET_SEARCH)).toBeVisible();
  await expect(rail.getByTestId("set-chips")).toHaveCount(0);
  await expect(rail.getByTestId("filter-count-set")).toHaveCount(0);
});

test("a hand-typed duplicate code renders exactly one chip @fast", async ({
  page,
}) => {
  // P6-139: `s:mh3,mh3` used to parse to two identical Set values, and the
  // rail rendered one chip per value — two elements sharing one
  // data-testid/data-code, a Playwright strict-mode landmine (`chip(...)`
  // below would refuse to resolve to a single element). The parser now
  // dedupes the comma-OR list, so only one chip — and one count — comes out
  // the other side.
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);

  await page.fill("#catalog-query", "s:mh3,mh3");
  await page.waitForURL((url) => url.searchParams.get("q") === "s:mh3,mh3");

  await expect(chip(page, "mh3")).toHaveCount(1);
  await expect(rail.getByTestId("filter-count-set")).toContainText("1");
});

test("a code the picker never lists is still the user's selection @fast", async ({
  page,
}) => {
  // The failure guarded here: validating the selection against the fetched list
  // would drop a pasted `s:xyz` on the next unrelated rail click. No set's code
  // or name contains "xyz", so the list genuinely cannot recognize it.
  await page.goto("/catalog?q=s%3Axyz%2Cmh3");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await expect(chip(page, "xyz")).toBeVisible();
  await expect(chip(page, "mh3")).toBeVisible();
  await expect(rail.getByTestId("filter-count-set")).toContainText("2");

  // Searching for it finds nothing — with the chip still standing beside the
  // empty list, which is the whole point.
  await rail.locator(SET_SEARCH).fill("xyz");
  await expect(rail.getByTestId("set-empty")).toBeVisible();
  await expect(chip(page, "xyz")).toBeVisible();

  // An edit elsewhere in the rail leaves it byte for byte alone...
  await rail.getByRole("checkbox", { name: "Blue" }).click();
  await page.waitForURL((url) =>
    (url.searchParams.get("q") ?? "").includes("c:u"),
  );
  expect(q(page)).toBe("s:xyz,mh3 c:u");
  // ...and it is removable like any other chip, which a code the list owned
  // could never be.
  await chip(page, "xyz").click();
  await page.waitForURL((url) => url.searchParams.get("q") === "s:mh3 c:u");
  await expect(chip(page, "xyz")).toHaveCount(0);
  await expect(chip(page, "mh3")).toBeVisible(); // positive control
});

test("a typed % is literal, not a LIKE wildcard @fast", async ({ page }) => {
  // Regression guard (P6-136): `list_sets`' ILIKE bound the typed term
  // unescaped, so a bare "%" acted as a wildcard and matched every set
  // instead of none. No set's code or name contains a literal "%".
  await page.goto("/catalog?q=s%3Amh3");
  await hydrated(page);
  const rail = page.locator(RAIL);

  // Positive control: the box genuinely searches and returns rows (mh3 need
  // not be among the newest-25 default browse, so search for it by name).
  await rail.locator(SET_SEARCH).fill("modern horizons 3");
  await expect(option(page, "mh3")).toBeVisible();

  await rail.locator(SET_SEARCH).fill("%");
  await expect(rail.getByTestId("set-empty")).toBeVisible();
  await expect(rail.locator("[data-testid=set-option]")).toHaveCount(0);
  await expect(chip(page, "mh3")).toBeVisible(); // selection survives an empty list
});

test("a set pick drops the page cursor @fast", async ({ page, request }) => {
  // The picker reaches the URL through its own signal→Effect hop (a row is built
  // inside a `Suspend`, so it cannot capture the non-Send navigate closure), so
  // "every rail edit starts at page one" needs asserting on *this* writer and
  // not only on a checkbox.
  const first = await firstPage(request, "bolt");
  await page.goto(`/catalog?q=bolt%20s%3Amh3&cursor=${first}`);
  await hydrated(page);
  const rail = page.locator(RAIL);

  await chip(page, "mh3").click();
  await page.waitForURL("/catalog?q=bolt");
  expect(new URL(page.url()).searchParams.get("cursor")).toBeNull();
  await expect(rail.locator(SET_SEARCH)).toBeVisible(); // positive control
});

// The picker's list has three non-answers — not fetched, failed, and genuinely
// empty — and only the last is a claim about the catalog. These two tests hold
// them apart, because a `Vec` that flattened them rendered "No set matches." over
// a catalog of ~1050 sets both when the fetch had not happened yet and when it
// had failed (adversarial review, major).

test("a failed set list says the fetch failed, and retries @fast", async ({
  page,
}) => {
  // The cheapest way to force an `Err` out of the adapter: fail its request. This
  // is the *normal* case on the native backend, where an offline phone gets
  // `ApiError::Upstream` — which the client maps distinctly on purpose.
  let broken = true;
  await page.route("**/api/list_sets*", async (route) => {
    if (broken) {
      await route.fulfill({
        status: 500,
        contentType: "text/plain",
        body: "set list unavailable",
      });
    } else {
      await route.continue();
    }
  });

  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  // Collapsed by default, so opening it is the first fetch — and the one that
  // fails.
  await rail.locator("summary").filter({ hasText: "Set" }).click();

  await expect(rail.getByTestId("set-error")).toBeVisible();
  // The whole point: a failure must not masquerade as a verdict on the catalog.
  await expect(rail.getByTestId("set-empty")).toHaveCount(0);
  // Positive control for that count — the picker itself rendered, so the absence
  // above is a real absence and not a missing subtree.
  await expect(rail.locator(SET_SEARCH)).toBeVisible();

  // And the way out works: nothing in the search box is wrong to fix, and
  // nothing retries on its own.
  broken = false;
  await rail.getByTestId("set-retry").click();
  await expect(rail.locator("[data-testid=set-option]").first()).toBeVisible();
  await expect(rail.getByTestId("set-error")).toHaveCount(0);
});

test("the set list never flashes an empty verdict before it loads @fast", async ({
  page,
  request,
}) => {
  // Request-level pin: on a bare /catalog the section is collapsed and the list
  // has not been asked for, so the SSR'd markup must not already contain an
  // answer. It did — `set-empty` shipped in the HTML with no loading state at
  // all, which made "No set matches." the first thing anyone ever saw here.
  //
  // `set-unengaged`, not `set-loading` — P6-137 review: the picker no longer
  // fetches merely because the section is open (auto-open on a URL carrying
  // sets was doing exactly that, unconditionally, twice per page — see the
  // SSR test above), so a bare, never-touched section has never asked the
  // server anything and must not claim it is loading either.
  const html = await (await request.get("/catalog")).text();
  expect(html).toContain(">Set<"); // positive control: the facet is on the page
  expect(html).toContain('data-testid="set-unengaged"');
  expect(html).not.toContain('data-testid="set-loading"');
  expect(html).not.toContain('data-testid="set-empty"');

  // ...and in the browser. The response is held open so the window under
  // assertion is deterministic rather than a race with a ~95 ms local round trip
  // (a cold Neon hop is materially worse, which is the case that bites).
  await page.route("**/api/list_sets*", async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 1500));
    await route.continue();
  });
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  // Not constructed at all until engaged, so this holds regardless — see the
  // SSR assertion above for why `toBeHidden()` (which Playwright also passes
  // on outright absence) is the right shape here rather than `toHaveCount(0)`.
  await expect(rail.getByTestId("set-loading")).toBeHidden();

  await rail.locator("summary").filter({ hasText: "Set" }).click();
  await expect(rail.getByTestId("set-loading")).toBeVisible();
  await expect(rail.getByTestId("set-empty")).toHaveCount(0);

  // The rows arriving are the positive control that this was a delay and not a
  // failure — otherwise the two assertions above would also pass on a picker
  // that never loaded anything at all.
  await expect(rail.locator("[data-testid=set-option]").first()).toBeVisible({
    timeout: 10_000,
  });
  await expect(rail.getByTestId("set-loading")).toHaveCount(0);
  await expect(rail.getByTestId("set-error")).toHaveCount(0);
});

test("a broad term returns every match, not a 25-row cap @fast", async ({
  page,
  request,
}) => {
  // P6-137, maintainer ruling: the old 25-row default silently truncated —
  // "commander" alone matches well over a hundred sets by name or code. This
  // is kill-verifiable: capped at the old default, the list stops at exactly
  // 25 rows regardless of how many actually match.
  const res = await request.get("/api/list_sets?q=commander");
  expect(res.status()).toBe(200);
  const rows = (await res.json()) as Array<{ code: string }>;
  expect(rows.length).toBeGreaterThan(25);

  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await rail.locator("summary").filter({ hasText: "Set" }).click();
  await rail.locator(SET_SEARCH).fill("commander");
  // `>=`, not an exact match against `rows.length`: that count came from a
  // *separate* request, moments earlier, against a catalog nothing here holds
  // still — the point is "not capped at 25", not "byte-identical to a request
  // this test itself raced." A longer timeout too: a loaded CI runner
  // rendering ~100+ rows can outrun the default 5s poll.
  await expect
    .poll(() => rail.locator("[data-testid=set-option]").count(), {
      timeout: 10_000,
    })
    .toBeGreaterThanOrEqual(rows.length);
});

test("browsing with a blank term reaches the whole catalog, not just the newest window @fast", async ({
  page,
  request,
}) => {
  // Blank `q` browses the newest sets first (SetQuery::term), so the picker's
  // *own* list — not a search — is the sharpest test of the cap's removal: an
  // old set like Limited Edition Alpha sits nowhere near the top of a
  // newest-first ordering, so it is reachable in the DOM only because the
  // list is no longer truncated to a small window.
  // `q` is a required param on the server fn — blank, not omitted, is what
  // "browse" means here (SetQuery::term treats both as the same "browse the
  // newest sets" case, but an *omitted* q is a 500, not a blank one).
  const res = await request.get("/api/list_sets?q=");
  expect(res.status()).toBe(200);
  const rows = (await res.json()) as Array<{ code: string }>;
  // Sanity floor on the fixture itself — the catalog is ~1050 sets today.
  expect(rows.length).toBeGreaterThan(1000);

  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await rail.locator("summary").filter({ hasText: "Set" }).click();
  // `>=`, not exact — see the sibling test above for why. 10s: rendering
  // ~1,047 rows on a loaded runner can outrun the default 5s poll.
  await expect
    .poll(() => rail.locator("[data-testid=set-option]").count(), {
      timeout: 10_000,
    })
    .toBeGreaterThanOrEqual(rows.length);
  // Reachable only by scrolling past everything newer — the DOM holds it
  // even though nothing scrolled it into view.
  await expect(option(page, "lea")).toHaveCount(1);
});

test("Enter in the set picker acts on what you typed, not the stale list @fast", async ({
  page,
}) => {
  // P6-138: `CommandInput` writes the live box text synchronously per
  // keystroke, but the picker's rows — and the keyboard-nav registry Enter
  // reads — are re-keyed by a 250ms-debounced server fetch. Mid-window, Enter
  // used to activate whatever the *stale* list highlighted.
  //
  // Limited Edition Alpha ("lea") and Beta ("leb") are a real, adjacent pair
  // where each is an exact-code-match top hit for its own search term
  // (P6-136's ranking tiers), so "the stale list's top row" and "what the box
  // now asks for" are two different, unambiguous sets — not a ranking
  // coincidence this test would need to hope for.
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await rail.locator("summary").filter({ hasText: "Set" }).click();

  // Establish the stale list for real: search "leb" and let it land.
  await rail.locator(SET_SEARCH).fill("leb");
  await expect(option(page, "leb")).toHaveText("Limited Edition Beta");

  // Hold the *next* fetch open so the race window is deterministic instead of
  // competing with a real, fast local round trip.
  let release: (() => void) | null = null;
  await page.route("**/api/list_sets*", async (route) => {
    const term = new URL(route.request().url()).searchParams.get("q") ?? "";
    if (term === "lea") {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    }
    await route.continue();
  });

  // Type the new term. The debounce fires (250ms) and re-keys the fetch, but
  // that fetch is the one being held — the "leb" rows stay mounted.
  await rail.locator(SET_SEARCH).fill("lea");
  await page.waitForTimeout(400); // past the 250ms debounce, still inside the hold
  await expect(option(page, "leb")).toBeVisible(); // still the stale list on screen

  // The race itself: Enter, while the box already reads "lea" but the rows
  // still answer "leb".
  await rail.locator(SET_SEARCH).press("Enter");
  // Kill-verify: unpatched `command.rs` selects the highlighted *stale* row
  // right here — `chip(page, "leb")` would appear immediately. The fix must
  // not add a chip at all yet; it has nothing fresh to act on.
  await expect(chip(page, "leb")).toHaveCount(0);
  await expect(chip(page, "lea")).toHaveCount(0);

  // Release the held fetch — the deferred Enter now has its answer.
  expect(release, "the route handler must have captured a releaser").not.toBeNull();
  release!();
  await expect(option(page, "lea")).toBeVisible();
  // The chip that lands matches what was typed, never the stale row.
  await expect(chip(page, "lea")).toBeVisible();
  await expect(chip(page, "leb")).toHaveCount(0);
});

test("Enter in the set picker still activates the highlighted row once the debounce has settled @fast", async ({
  page,
}) => {
  // Positive control for the fix above: when the rows on screen already
  // answer the box (the ordinary, non-racing case — by far the common one),
  // Enter must still work exactly as before.
  await page.goto("/catalog");
  await hydrated(page);
  const rail = page.locator(RAIL);
  await rail.locator("summary").filter({ hasText: "Set" }).click();

  await rail.locator(SET_SEARCH).fill("limited edition alpha");
  await expect(option(page, "lea")).toBeVisible();
  await page.waitForTimeout(400); // past the debounce — rows are fresh

  await rail.locator(SET_SEARCH).press("Enter");
  await expect(chip(page, "lea")).toBeVisible();
});

test("a set-filtered link auto-opens the section without fetching the row list @fast", async ({
  request,
}) => {
  // The regression this guards (P6-137 review, major): `section_seeded_open`
  // auto-opens the Set section whenever the URL already carries a set, and
  // this component renders *twice* per page (the desktop rail and the mobile
  // `FilterSheet`, which mounts its children off-screen rather than
  // unmounting while closed — see `SheetContent`). The old "fetch when open"
  // rule fired on page load with no interaction at all, so every such link
  // paid for SSR-rendering ~2,094 rows of markup (all ~1,047 sets, twice)
  // before anyone touched the picker. `filter-rail.spec.ts`'s SSR test above
  // pins the desktop half of this; this one pins the *mobile* half, since the
  // two are separate `SetPicker` instances that could regress independently.
  const html = await (await request.get("/catalog?q=s%3Amh3")).text();
  // The chip is the auto-open contract — a shared link must still visibly
  // reflect its selected sets, in both renders.
  const chipCount = (
    html.match(/data-testid="set-chip" data-code="mh3"/g) ?? []
  ).length;
  expect(chipCount).toBe(2);
  // Neither render fetched the row list.
  expect(html).not.toContain('data-testid="set-option"');
  const unengagedCount = (
    html.match(/data-testid="set-unengaged"/g) ?? []
  ).length;
  expect(unengagedCount).toBe(2);
});

/// A real first-page cursor for `q`, from the hosted JSON route at `limit=1` —
/// the page always asks for 50, so a small page is the only way to get a cursor
/// out of a small result set.
async function firstPage(request: APIRequestContext, q: string) {
  const res = await request.get(
    `/api/catalog/search?q=${encodeURIComponent(q)}&limit=1`,
  );
  expect(res.status()).toBe(200);
  const body = (await res.json()) as { next_cursor: string | null };
  expect(
    body.next_cursor,
    "the fixture must exceed a one-row page",
  ).toBeTruthy();
  return body.next_cursor!;
}

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
    // Two locators on purpose: `panel` is the SheetContent (which carries the
    // open/closed state), `sheet` is the rail body nested inside it.
    // `data-state`, not toBeVisible — SheetContent slides in via a transform
    // and keeps its box when closed, so a closed sheet and everything in it is
    // "visible" to Playwright. Proved by mutation on the card-detail task
    // (app-ui Findings).
    const panel = page.locator(
      '[data-name=SheetContent][aria-label="Filters"]',
    );
    await expect(panel).toHaveAttribute("data-state", "open");
    const sheet = page.locator("[data-testid=filter-sheet]");
    await expect(
      sheet.getByRole("checkbox", { name: "Instant" }),
    ).toHaveAttribute("aria-checked", "true");

    // The footer count comes from an Effect-written signal, not a resource read
    // in render. Reading the resource in render made SSR emit "Show results"
    // and hydration claim that text node without rewriting it, so the label
    // stayed countless until the next query change (app-ui Findings).
    // On the panel, not a button locator: SheetContent renders its own X close
    // button with the same aria-label as the footer's SheetClose.
    await expect(panel).toContainText(/Show \d+ results/);

    // The sheet edits the same query text the rail does.
    await sheet.getByRole("checkbox", { name: "Sorcery" }).click();
    await page.waitForURL((url) =>
      (url.searchParams.get("q") ?? "").includes("t:instant,sorcery"),
    );
    await expect(page.getByTestId("filter-badge")).toContainText("5");
  });

  test("the set picker works inside the filter sheet @fast", async ({
    page,
    request,
  }) => {
    // The rail becomes a slide-over on mobile, and the picker is the one rail
    // widget that fetches — so it needs its own assertion that the sheet's copy
    // is live rather than a second, inert render.
    await page.goto("/catalog?q=s%3Amh3");
    await hydrated(page);
    await expect(page.getByTestId("filter-badge")).toContainText("1");

    await page.getByRole("button", { name: /Filters/ }).click();
    const panel = page.locator(
      '[data-name=SheetContent][aria-label="Filters"]',
    );
    // `data-state`, not toBeVisible — a closed SheetContent keeps its box.
    await expect(panel).toHaveAttribute("data-state", "open");
    const sheet = page.locator("[data-testid=filter-sheet]");
    await expect(
      sheet.locator("[data-testid=set-chip][data-code=mh3]"),
    ).toBeVisible();

    // Lazy-mount, mobile half (P6-137 review): the sheet auto-opened on the
    // `s:mh3` URL same as the desktop rail, but engaging it is what's supposed
    // to bring the row list in — until then it shows the chip and this hint,
    // not rows.
    await expect(sheet.getByTestId("set-unengaged")).toBeVisible();
    await expect(sheet.locator("[data-testid=set-option]")).toHaveCount(0);

    // Its own list, fetched through its own resource, engaged by focusing the
    // search box (the same interaction `.fill()` performs first).
    await sheet.locator("#filter-sheet-set").fill("limited edition alpha");
    await expect(sheet.getByTestId("set-unengaged")).toHaveCount(0);
    const lea = sheet.locator("[data-testid=set-option][data-code=lea]");
    await expect(lea).toHaveText("Limited Edition Alpha");
    await lea.click();
    await page.waitForURL((url) => url.searchParams.get("q") === "s:mh3,lea");
    await expect(page.getByTestId("filter-badge")).toContainText("2");
    // The sheet stays open across a pick: this facet is multi-select, so closing
    // it would make every second set cost a reopen.
    await expect(panel).toHaveAttribute("data-state", "open");

    // The uncap itself, in the sheet: clearing the search reaches the same
    // full, unbounded browse-all list the desktop rail does — not a mobile-
    // specific window. `>=`, longer timeout — see the desktop equivalents
    // above for why.
    const res = await request.get("/api/list_sets?q=");
    const rows = (await res.json()) as Array<{ code: string }>;
    expect(rows.length).toBeGreaterThan(1000);
    await sheet.locator("#filter-sheet-set").fill("");
    await expect
      .poll(() => sheet.locator("[data-testid=set-option]").count(), {
        timeout: 10_000,
      })
      .toBeGreaterThanOrEqual(rows.length);
  });

  test("colorless counts even though it has no checkbox @fast", async ({
    page,
  }) => {
    // `c:colorless` is a supported Color filter the five-checkbox facet cannot
    // draw. If it counted 0 the badge would vanish on a filtered query and
    // Reset would be unreachable (Codex review, medium).
    await page.goto("/catalog?q=c%3Acolorless");
    await hydrated(page);
    await expect(page.getByTestId("filter-badge")).toContainText("1");

    await page.getByRole("button", { name: /Filters/ }).click();
    const sheet = page.locator("[data-testid=filter-sheet]");
    await expect(sheet.getByRole("button", { name: "Reset" })).toBeVisible();
    await sheet.getByRole("button", { name: "Reset" }).click();
    await page.waitForURL((url) => url.pathname === "/catalog" && !url.search);
  });
});
