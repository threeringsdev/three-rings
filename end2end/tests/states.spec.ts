// The state arms — every surface's zero-data, failed-fetch and in-flight
// behavior (specs/app-ui.md → "Empty / error / loading states").
//
// The one rule these tests exist to hold: **an arm must not claim more than the
// app knows.** So each test below induces a *real* failure and then asserts two
// things about it — that the page said which failure it was, and that it left a
// way forward that does not depend on the read that just failed.
//
// How the failures are induced, and why it matters that they are:
//
// - **A junk `?cursor=`** is a real server-side `validation:` error reachable on
//   a cold load, which makes it the only failure here that is also visible to
//   `curl` — so the paging arms are asserted in the SSR'd HTML as well as in the
//   browser. It is also exactly the case the way home exists for: a shared or
//   bookmarked cursor link goes stale with nothing for the reader to fix.
// - **A collection id that isn't there** is a real `not found:` error, same
//   properties, and it is how a link to a deleted collection behaves.
// - **A 500 fulfilled by `page.route`** covers the reads that fail over the wire.
//   `page.route` alone is not enough: `/my` and `/catalog` are `SsrMode::Async`,
//   so a `goto` resolves their resources in-process and makes **zero** browser
//   requests (measured — specs/app-ui.md, the mobile `/my` root). The working
//   mechanism is an **SPA navigation** out of `/dev/components`, which is outside
//   `AppShell` and therefore has none of the shell's resources yet. Every such
//   test asserts the interception actually happened, so it cannot pass by
//   inducing nothing.
//
// Every negative assertion has a positive control on the same page or in the
// same test — "no retry button" and "no dishonest empty line" both pass on a
// blank page.

import { expect, test, type Page } from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const JUNK_CURSOR = "not-a-real-cursor";
// A well-formed id the server will not find. Well-formed matters: a malformed
// one is rejected client-side by `Id::parse_str` on some routes and would test
// the parser instead of the arm.
const ABSENT_COLLECTION = "11111111-2222-3333-4444-555555555555";

/// SPA-navigate out of the bench and into the shell, which is the only way a
/// shell resource becomes an HTTP request a `page.route` can fail. The bench's
/// my-root section carries a real `/my/all` link for exactly this.
async function intoShell(page: Page) {
  await page.goto("/dev/components");
  await hydrated(page);
  await page.locator('a[href="/my/all"]').first().click();
  await page.waitForURL("/my/all");
}

test.describe("a stale cursor", () => {
  test("@fast /my names the failure and offers the way home, with no useless retry", async ({
    page,
    request,
  }) => {
    // Positive control first, on the same route: page one is healthy, carries no
    // error and offers no way home (there is nowhere to go home *from*). Without
    // this, every assertion below could be passing on a page that is simply
    // broken in some other way.
    await page.goto("/my");
    await hydrated(page);
    await expect(page.getByTestId("all-cards-error")).toHaveCount(0);
    await expect(page.getByTestId("page-first")).toHaveCount(0);
    await expect(page.getByTestId("all-cards-table")).toBeVisible();

    await page.goto(`/my?cursor=${JUNK_CURSOR}`);
    await hydrated(page);

    const err = page.getByTestId("all-cards-error");
    await expect(err).toBeVisible();
    await expect(err).toContainText("Couldn't load your cards");
    // The classification is asserted, not just the copy: the affordances below
    // are chosen from it, so a mis-classified error would offer the wrong ones
    // while still reading plausibly.
    await expect(err).toHaveAttribute("data-failure", "request");
    // A retry re-sends the same bad cursor. Offering one would be theatre — and
    // the transport test below is this assertion's positive control, since it
    // proves the button exists at all when it can help.
    await expect(err.getByTestId("state-retry")).toHaveCount(0);

    // The defect this test exists for: before the fix this page was the message
    // and nothing else.
    const home = err.getByTestId("page-first");
    await expect(home).toBeVisible();
    await home.click();
    await expect(page).toHaveURL("/my");
    await expect(page.getByTestId("all-cards-table")).toBeVisible();

    // And it works with no JS at all, which is what makes it a real escape for a
    // shared link: the way out is in the SSR'd HTML, not added by hydration.
    const raw = await (await request.get(`/my?cursor=${JUNK_CURSOR}`)).text();
    expect(raw).toContain('data-failure="request"');
    expect(raw).toContain('data-testid="page-first"');
    expect(raw).not.toContain('data-testid="state-retry"');
  });

  test("@fast the way home keeps the search the cursor was paging", async ({
    page,
  }) => {
    // `/catalog` recorded the rule — "a bad cursor must not cost the user the
    // search they typed" — and `/my/all` was dropping the query along with the
    // cursor, on both this arm and its paged-empty one.
    const q = "bolt";
    await page.goto(`/my/all?q=${q}&cursor=${JUNK_CURSOR}`);
    await hydrated(page);
    const home = page.getByTestId("all-cards-error").getByTestId("page-first");
    await expect(home).toBeVisible();
    // Asserted on the href rather than by clicking, so the failure message names
    // the actual defect (a link that drops `q`) instead of "wrong URL after
    // click".
    await expect(home).toHaveAttribute("href", `/my/all?q=${q}`);
    await home.click();
    await expect(page).toHaveURL(`/my/all?q=${q}`);
    // The search survived the trip, which is the whole point.
    await expect(page.locator("#my-query")).toHaveValue(q);
  });

  test("@fast a collection's own paging arm offers both ways out", async ({
    page,
    request,
  }) => {
    const tree = await (await request.get("/api/collection_tree")).json();
    const id = tree.collections[0].summary.id;
    await page.goto(`/my/collections/${id}?cursor=${JUNK_CURSOR}`);
    await hydrated(page);

    const err = page.getByTestId("collection-error");
    await expect(err).toBeVisible();
    await expect(err).toHaveAttribute("data-failure", "request");
    // Two, because this arm replaces the *header* — which is where the
    // breadcrumb and the mobile back link live — so losing the cursor and
    // leaving the collection entirely are both things the reader may need.
    await expect(err.getByTestId("page-first")).toBeVisible();
    await expect(err.getByTestId("collection-error-home")).toBeVisible();
    await err.getByTestId("page-first").click();
    await expect(page).toHaveURL(`/my/collections/${id}`);
    await expect(page.getByTestId("collection-error")).toHaveCount(0);
  });
});

test("@fast a link to a collection that isn't there says so and leads out", async ({
  page,
  request,
}) => {
  await page.goto(`/my/collections/${ABSENT_COLLECTION}`);
  await hydrated(page);

  const err = page.getByTestId("collection-error");
  await expect(err).toBeVisible();
  await expect(err).toHaveAttribute("data-failure", "missing");
  // The bare noun the API carries (`not found: collection`) is dropped rather
  // than concatenated — "Couldn't load this collection: collection" is what
  // appending it produced.
  await expect(err).toContainText("may have been deleted");
  await expect(err).not.toContainText(": collection");
  await expect(err.getByTestId("state-retry")).toHaveCount(0);
  // Not paged, so no page-one link — the way out is the one that is always true.
  await expect(err.getByTestId("page-first")).toHaveCount(0);
  await err.getByTestId("collection-error-home").click();
  await expect(page).toHaveURL("/my");

  // SSR too: a dead link is usually followed from outside the app.
  const raw = await (
    await request.get(`/my/collections/${ABSENT_COLLECTION}`)
  ).text();
  expect(raw).toContain('data-failure="missing"');
  expect(raw).toContain('data-testid="collection-error-home"');
});

test("@fast an unreachable backend gets a retry, and the retry recovers", async ({
  page,
}) => {
  // The other half of the honesty claim: the request-level arms above withhold
  // the retry, so something has to prove the button exists when it *can* help —
  // and that pressing it actually refetches rather than just re-rendering.
  let treeReads = 0;
  let failing = true;
  await page.route("**/api/collection_tree*", async (route) => {
    treeReads++;
    if (failing) {
      await route.fulfill({
        status: 500,
        contentType: "text/plain",
        body: "induced: collection tree unavailable",
      });
    } else {
      await route.fallback();
    }
  });

  await intoShell(page);
  expect(
    treeReads,
    "the tree read was never intercepted — the failure was not induced",
  ).toBeGreaterThan(0);

  const err = page.getByTestId("tree-error");
  await expect(err).toBeVisible();
  await expect(err).toContainText("Couldn't load collections");
  // Announced. A silent nav that simply isn't there is invisible to the reader
  // least able to notice its absence.
  await expect(err.locator('[role="alert"]')).toBeVisible();
  // `warning`, not `destructive`: the page beside the rail is fine.
  await expect(err.locator("[data-tone]")).toHaveAttribute(
    "data-tone",
    "partial",
  );

  // Let the next read through, then press the button. A retry that re-rendered
  // the same failure would leave the error on screen.
  const before = treeReads;
  failing = false;
  await err.getByTestId("tree-retry").click();
  await expect(page.getByTestId("tree-error")).toHaveCount(0);
  // The tree is really back — the positive control for "the error went away".
  await expect(page.locator('a[href="/my/shopping"]').first()).toBeVisible();
  expect(treeReads, "the retry issued no request").toBeGreaterThan(before);
});

test("@fast the destination picker blames the read, not the user's collections", async ({
  page,
}) => {
  // The dishonest state this task existed to find: the picker collapsed a failed
  // `list_collections` into an empty list, and `CommandEmpty` then asserted "No
  // collection matches." — a failed fetch claiming the account has no
  // collections. On the native backend an offline phone is the ordinary case for
  // this read.
  let reads = 0;
  await page.route("**/api/list_collections*", async (route) => {
    reads++;
    await route.fulfill({
      status: 500,
      contentType: "text/plain",
      body: "induced: collections unavailable",
    });
  });

  await intoShell(page);
  // Mode switch, still an SPA navigation: the picker mounts on `/catalog` and
  // creates its resource client-side, where the 500 lands.
  await page.locator('nav[aria-label="Mode"] a[href="/catalog"]').click();
  await page.waitForURL("/catalog");
  await page.getByTestId("destination-label").click();

  const err = page.getByTestId("destination-error");
  await expect(err).toBeVisible();
  await expect(err).toContainText("Couldn't load your collections");
  expect(
    reads,
    "the collections read was never intercepted — the failure was not induced",
  ).toBeGreaterThan(0);

  // The claim that was false, gone. Asserted as text on the page rather than on
  // a testid, because the line came from `CommandEmpty` and had none.
  await expect(page.getByText("No collection matches.")).toHaveCount(0);
  await expect(page.getByTestId("destination-option")).toHaveCount(0);
  // It goes through the shared classifier now, not a hand-rolled banner: the
  // first cut printed the raw wire detail and offered an unconditional retry, so
  // an `unauthorized:` failure got a button that 401s forever.
  await expect(err).toHaveAttribute("data-failure", "transport");
  await expect(err.getByTestId("state-retry")).toBeVisible();
});

test("@fast the move dialog says the tree is missing instead of offering only root", async ({
  page,
  request,
}) => {
  // The third `DestinationList` consumer, and the one where the collapse wrote.
  // `move_rows` renders `⬆ Top level` unconditionally, so a failed tree read left
  // the dialog listing that one row with `CommandEmpty` silent (non-empty
  // registry) and no error anywhere — asserting root was the only place this
  // collection could go, and offering a reparent as the only move.
  let treeReads = 0;
  await page.route("**/api/collection_tree*", async (route) => {
    treeReads++;
    await route.fulfill({
      status: 500,
      contentType: "text/plain",
      body: "induced: collection tree unavailable",
    });
  });

  await intoShell(page);
  expect(
    treeReads,
    "the tree read was never intercepted — the failure was not induced",
  ).toBeGreaterThan(0);
  // Control: the rail knows the tree is gone. This is the state in which the
  // dialog below used to lie.
  await expect(page.getByTestId("tree-error")).toBeVisible();

  // A collection link on `/my/all` comes from `all_cards`, not from the tree —
  // the one SPA route into a collection page that survives this failure, and the
  // reason the scenario is reachable at all. Specifically the **single-location**
  // shape, which renders a bare `<a>`; the multi-location shape hides its links
  // inside a closed `Collapsible` whose trigger swallows the click.
  //
  // Not the Inbox, though: its own kebab withholds Move to…/Rename…/Delete…
  // (the API's `AND NOT is_inbox` guard, app/src/my/collection.rs), so a link
  // that lands there could never open the dialog under test. Other e2e
  // suites' quick-adds default to the Inbox, and it has accumulated so much
  // fixture history that it now dominates every *page one* of `/my/all`'s
  // single-location rows — so this resolves the target card from the API
  // (walking `all_cards` pages if needed, cheap next to rendering) and then
  // searches for it by name, rather than trusting whatever DOM order happens
  // to land on page one.
  const collectionsRes = await request.get("/api/collections");
  expect(collectionsRes.status()).toBe(200);
  const inboxId = (
    (await collectionsRes.json()) as { id: string; is_inbox: boolean }[]
  ).find((c) => c.is_inbox)?.id;
  expect(inboxId, "the authed user must have an Inbox").toBeTruthy();

  type AllCardsRow = {
    card: { name: string; oracle_id: string };
    locations: { collection_id: string }[];
  };
  let cursor: string | undefined;
  let target: AllCardsRow | undefined;
  for (let i = 0; i < 20 && !target; i++) {
    const url =
      "/api/all_cards?q=" + (cursor ? `&cursor=${encodeURIComponent(cursor)}` : "");
    const res = await request.get(url);
    expect(res.status()).toBe(200);
    const view = await res.json();
    target = (view.cards as AllCardsRow[]).find(
      (r) => r.locations.length === 1 && r.locations[0].collection_id !== inboxId,
    );
    if (!view.next_cursor) break;
    cursor = view.next_cursor;
  }
  expect(
    target,
    "fixture has no non-Inbox single-location card — no SPA route into a collection survives the failed tree",
  ).toBeTruthy();

  // Narrow the table to that one card so its row renders regardless of where
  // it falls in the full (debris-heavy) list — and then scope the link to the
  // row that card's own oracle id names, not `.first()`: the search text can
  // still substring-match other cards' names (e.g. a plain "Bolt" needle also
  // matches "Blastfire Bolt"), so `.first()` post-search is exactly the same
  // trap the catalog-size bulk load exposed elsewhere in this suite.
  await page.locator("#my-query").fill(target!.card.name);
  await page.waitForURL((u) => u.searchParams.get("q") === target!.card.name);
  const link = page
    .locator(`[data-testid="all-cards-row"][data-oracle="${target!.card.oracle_id}"]`)
    .locator('a[data-testid="location-summary"]');
  await expect(link).toBeVisible();
  await link.click();
  await page.waitForURL(/\/my\/collections\//);
  // The page itself is fine (it reads `collection_view`), which is exactly why
  // the kebab is live and the dialog reachable.
  await expect(page.getByTestId("collection-page")).toBeVisible();
  await expect(page.getByTestId("collection-error")).toHaveCount(0);

  await page.locator('[data-testid="collection-actions"]').click();
  const menu = page.locator("#context-menu-collection-header");
  await expect.poll(() => menu.evaluate((el) => el.matches(":popover-open"))).toBe(
    true,
  );
  await menu.locator('[role="menuitem"]').filter({ hasText: "Move to…" }).click();

  const dialog = page.locator('[role="dialog"]#tree-move');
  await expect(dialog).toBeVisible();
  // The fix: the failure is named…
  await expect(dialog.getByTestId("destination-error")).toBeVisible();
  // …the line that would have spoken for it is gone…
  await expect(dialog.getByText("No collection to move into.")).toHaveCount(0);
  // …and `⬆ Top level` is still offered, because reparenting to root is the one
  // destination that never needed the tree — the `fallback_rows` discipline.
  // What it must not be is alone and unexplained, which the assertion above
  // covers and this one's presence completes.
  await expect(
    dialog.locator('[data-testid="destination-option"]', {
      hasText: "Top level",
    }),
  ).toBeVisible();
});

test("@fast the picker's healthy list is the positive control", async ({
  page,
}) => {
  // Without this, the test above passes on a picker that is broken outright.
  await page.goto("/catalog");
  await hydrated(page);
  await page.getByTestId("destination-label").click();
  await expect(
    page.getByTestId("destination-option").first(),
  ).toBeVisible();
  await expect(page.getByTestId("destination-error")).toHaveCount(0);
});

test.describe("empty states say which kind of nothing they are", () => {
  test("@fast an empty needs list is the good kind, and says what it did not count", async ({
    page,
    request,
  }) => {
    // Reached by URL on purpose: `/my/collections/:id/needs` is linked *only*
    // from the needs chip, which is absent when nothing is missing — so this
    // arm is unreachable by navigation and was untested by construction
    // (specs/app-ui.md recorded the same trap).
    const tree = await (await request.get("/api/collection_tree")).json();
    // The fixture has to distinguish the two arms or this test is vacuous, so
    // both are looked up from the API rather than hard-coded: this suite creates
    // and deletes collections in parallel, and a hard-coded id (or a scan that
    // trusts every row of a snapshot to survive the scan) is how that shows up as
    // a mystery failure. A row that has gone by the time it is read is skipped,
    // not fatal.
    let withNeeds: string | undefined;
    let withoutNeeds: string | undefined;
    for (const row of tree.collections) {
      if (withNeeds && withoutNeeds) break;
      const id = row.summary.id;
      const res = await request.get(`/api/collections/${id}/needs`);
      if (!res.ok()) continue;
      const needs = await res.json();
      if (needs.rows.length > 0) withNeeds ??= id;
      else withoutNeeds ??= id;
    }
    expect(
      withoutNeeds,
      "fixture has no collection with an empty needs list — this test would be vacuous",
    ).toBeTruthy();
    expect(
      withNeeds,
      "fixture has no collection with needs — the positive control would be vacuous",
    ).toBeTruthy();

    await page.goto(`/my/collections/${withoutNeeds}/needs`);
    await hydrated(page);
    const empty = page.getByTestId("needs-empty");
    await expect(empty).toBeVisible();
    // The qualifier is the load-bearing half: this page's arithmetic is
    // board-blind, so "nothing missing" unqualified would tell a deck owner
    // their sideboard is filled.
    await expect(empty).toContainText("board slots");
    // `success`, because this nothing is an achievement — the opposite claim
    // from `/my/all`'s "you haven't added any cards yet".
    await expect(empty.locator("[data-tone]")).toHaveAttribute(
      "data-tone",
      "resolved",
    );

    // Positive control: a collection that *is* missing cards renders the summary
    // and not this arm. Both directions, since "the empty state is gone" and
    // "rows exist" fail differently.
    await page.goto(`/my/collections/${withNeeds}/needs`);
    await hydrated(page);
    await expect(page.getByTestId("needs-summary")).toBeVisible();
    await expect(page.getByTestId("needs-empty")).toHaveCount(0);
  });
});

test("@fast a rejected query labels the results it kept as not current", async ({
  page,
}) => {
  // The catalog keeps the last good page under a grammar error, dimmed and
  // inert. Unlabeled, that is results sitting under an error with no way to tell
  // they answer the *previous* query — and the block is `aria-hidden`, so a
  // screen reader was told nothing at all.
  await page.goto("/catalog?q=bolt");
  await hydrated(page);
  await expect(page.getByTestId("results-grid")).toBeVisible();
  await expect(page.locator('[data-tone="stale"]')).toHaveCount(0);

  await page.fill("#catalog-query", "bolt pow>3");
  await expect(page.getByTestId("search-error")).toBeVisible();
  const stale = page.locator('[data-tone="stale"]');
  await expect(stale).toBeVisible();
  await expect(stale).toHaveText("Previous results");
  // It labels a block that really is there — otherwise the badge is captioning
  // nothing.
  await expect(page.locator("[data-stale=true]")).toBeVisible();

  // Fixing the query clears the label with the error.
  await page.fill("#catalog-query", "bolt");
  await expect(page.getByTestId("search-error")).toHaveCount(0);
  await expect(page.locator('[data-tone="stale"]')).toHaveCount(0);
});

test("@fast the bench shows all four failure classes with their own affordances", async ({
  page,
}) => {
  // The four classes side by side, and the only place several of these arms can
  // be *looked* at — inducing an expired session on a `/my/*` page is
  // impossible, because the auth guard bounces the load before the page renders.
  await page.goto("/dev/components");
  await hydrated(page);

  const missing = page.getByTestId("bench-error-missing");
  await expect(missing).toHaveAttribute("data-failure", "missing");
  await expect(missing).toContainText("may have been deleted");
  await expect(missing.getByTestId("state-retry")).toHaveCount(0);
  await expect(missing.getByTestId("bench-error-away")).toBeVisible();

  const request = page.getByTestId("bench-error-request");
  await expect(request).toHaveAttribute("data-failure", "request");
  await expect(request).toContainText("invalid cursor");
  await expect(request.getByTestId("state-retry")).toHaveCount(0);
  await expect(request.getByTestId("bench-error-home")).toBeVisible();

  const transport = page.getByTestId("bench-error-transport");
  await expect(transport).toHaveAttribute("data-failure", "transport");
  await expect(transport.getByTestId("state-retry")).toBeVisible();

  const session = page.getByTestId("bench-error-session");
  await expect(session).toHaveAttribute("data-failure", "session");
  // Not the page's failure: the raw `unauthorized: invalid token` this used to
  // print reads as a page bug, and was mistaken for one.
  await expect(session).toContainText("Your session has expired");
  await expect(session).not.toContainText("invalid token");
  await expect(session.getByTestId("state-retry")).toHaveCount(0);
  const signin = session.getByTestId("state-signin");
  await expect(signin).toBeVisible();
  // It comes back here, which is what makes it a fix rather than a detour.
  await expect(signin).toHaveAttribute(
    "href",
    "/login?next=%2Fdev%2Fcomponents",
  );

  // The retry is wired, not drawn: without this the two banners above are a
  // screenshot.
  await expect(page.getByTestId("bench-retries")).toHaveText("0");
  await transport.getByTestId("state-retry").click();
  await expect(page.getByTestId("bench-retries")).toHaveText("1");

  // All three tones render on the page (the badge families the token work was
  // for), each exactly once here.
  for (const tone of ["resolved", "partial", "stale"]) {
    await expect(
      page.locator(`#states [data-tone="${tone}"]`),
    ).toHaveCount(1);
  }
});
