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
/// A transform DFC whose card-level `keywords` (Scryfall's union across both
/// faces) is non-empty — `Agadeem's Awakening` has none, so it can't tell the
/// keywords row's own face-gating apart from "never renders". Front face
/// (`Kruin Outlaw`) prints no keyworded abilities of its own; the back
/// (`Terror of Kruin Pass`) has First strike + Double strike, and both sit
/// beside `Transform` in the unioned list every face shares.
const DFC_KEYWORDS_QUERY = "Kruin Outlaw";
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
  printing_id: string | null;
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

/// Resolve a card by its **exact** name rather than "first substring hit".
/// The full-catalog bulk load put several distinct cards' full names on a
/// shared substring — `SINGLE_FACE_QUERY` ("Lightning Bolt") also matches
/// "Emeritus of Conflict // Lightning Bolt" and "Lightning Bolt // Lightning
/// Bolt", which `firstCard` would silently hand back instead (alphabetically
/// first) — every "Lightning Bolt"-named tile on /catalog mounts its own
/// sheet, so a query, trigger or sheet locator scoped only by that substring
/// now resolves to more than one element. Callers that need the specific
/// single-face card use this instead.
async function exactCard(
  request: APIRequestContext,
  name: string,
): Promise<Summary> {
  const cards = await search(request, name);
  const card = cards.find((c) => c.name === name);
  expect(card, `no exact catalog hit for "${name}"`).toBeTruthy();
  return card!;
}

/// A tile's own preview trigger, scoped by the oracle id its tile links to —
/// the same disambiguation `tileFor` uses in catalog.spec.ts, needed here
/// because a substring query can mount several same-substring tiles at once.
const triggerFor = (page: import("@playwright/test").Page, oracleId: string) =>
  page.locator('[data-testid="card-preview-trigger"]').filter({
    has: page.locator(`a[href="/cards/${oracleId}"]`),
  });

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

test("an anonymous visitor gets no your-copies section, no wants section, and no steppers @fast", async ({
  page,
  request,
}) => {
  const card = await firstCard(request, SINGLE_FACE_QUERY);
  await page.goto(`/cards/${card.oracle_id}`);
  await hydrated(page);
  await expect(page.getByTestId("card-name")).toContainText(card.name);
  // `ownership`/`wants` are both None for anonymous callers — the sections
  // are absent, not empty (an authed reader who owns/wants nothing still gets
  // the section, with its own empty-state sentence).
  await expect(page.getByTestId("your-copies")).toHaveCount(0);
  await expect(page.getByTestId("your-wants")).toHaveCount(0);
  // Not just the sections: no stepper of either shape renders anywhere on
  // the page for an anonymous reader — inert, not merely hidden.
  await expect(page.locator('[data-testid="count-stepper"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="ownership-row"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="want-row"]')).toHaveCount(0);
});

// -------------------------------------------------------- the back control ---
//
// components::back_nav (app/src/components/back_nav.rs): real in-app history
// when there is any, a fixed fallback otherwise — `/catalog` for anonymous
// readers, `/my` once signed in. Shared by the on-page control
// (`data-testid=card-detail-back`) and the app-wide `⌘[` / `Alt+←` desktop
// shortcut, so both are exercised here against the same fixture.

test.describe("the back control", () => {
  test("returns to the prior in-app page, query string included @fast", async ({
    page,
    request,
  }) => {
    const card = await exactCard(request, SINGLE_FACE_QUERY);
    const catalogUrl = `/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`;
    await page.goto(catalogUrl);
    await hydrated(page);

    // A real client-side navigation (the router's click-delegate intercepts
    // the tile's own `<a href>`), not `page.goto` — this is what has to leave
    // a `history.back()` target behind for the shell's navigation counter to
    // see (back_nav's module doc: a cold load intentionally does not).
    await page.locator(`a[href="/cards/${card.oracle_id}"]`).first().click();
    await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
    await hydrated(page);

    const back = page.getByTestId("card-detail-back");
    await expect(back).toBeVisible();
    await back.click();

    // The exact prior URL, query string and all — a fixed fallback could only
    // ever guess `/catalog`, never the `q=`/`view=` this test started from.
    await page.waitForURL(
      (url) => url.pathname === "/catalog" && url.search === new URL(catalogUrl, page.url()).search,
    );
  });

  test("a cold direct load offers the anonymous fallback, and it works @fast", async ({
    page,
    request,
  }) => {
    const card = await firstCard(request, SINGLE_FACE_QUERY);
    // `page.goto`, not a click-through: this is the cold-deep-link case the
    // fallback exists for — a fresh load with nothing for `history.back()` to
    // return to.
    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);

    const back = page.getByTestId("card-detail-back");
    await expect(back).toHaveAttribute("href", "/catalog");
    await back.click();
    await page.waitForURL("/catalog");
  });

  test.describe("authed", () => {
    test.use({ storageState: AUTH_STATE });

    test("a cold direct load offers the signed-in fallback, and it works @fast", async ({
      page,
      request,
    }) => {
      const card = await firstCard(request, SINGLE_FACE_QUERY);
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const back = page.getByTestId("card-detail-back");
      await expect(back).toHaveAttribute("href", "/my");
      await back.click();
      await page.waitForURL("/my");
    });
  });

  // The blocker adversarial review found (round 2): a shell-lifetime
  // navigation *counter* re-arms on the very navigation `browser_back()`
  // itself causes, since a popstate is a navigation too — so
  // `/catalog → card → back` left the landed-on `/catalog` entry reading
  // "has history" again, and a **second** back press called
  // `history.back()` with nothing real behind it, which on desktop (no
  // address bar) can walk the reader out of the app entirely.
  // `back_nav::has_history` no longer counts navigations at all — it reads a
  // marker stamped onto the actual history entry (see that module's doc for
  // the full mechanism) — so this pins the fixed behavior directly: pressing
  // Back twice in a row from a fresh two-hop session must land on the
  // fallback the second time, not attempt a second `history.back()`.
  test("a second Back press from the landed-on entry falls back instead of leaving the app @fast", async ({
    page,
    request,
  }) => {
    const card = await exactCard(request, SINGLE_FACE_QUERY);
    const catalogUrl = `/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}`;
    await page.goto(catalogUrl);
    await hydrated(page);

    await page.locator(`a[href="/cards/${card.oracle_id}"]`).first().click();
    await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
    await hydrated(page);

    // First press: real history, lands back on the search it came from.
    await page.getByTestId("card-detail-back").click();
    await page.waitForURL((url) => url.pathname === "/catalog");

    // Second press: nothing real behind this entry. Only the keyboard
    // shortcut can even attempt it here — the catalog page carries no Back
    // control of its own, which is exactly why this is the shortcut's
    // regression to own: `/catalog` is the fallback for both mechanisms, so
    // this is the same "must not be `browser_back()`'s to attempt" claim
    // whichever surface presses it.
    await page.keyboard.press("Meta+BracketLeft");
    await page.waitForTimeout(400);

    // Still inside the app, on the fallback — not one hop further into
    // whatever a real browser's history had *before* this tab's session
    // (an external referrer, `about:blank`, or — on desktop, with no address
    // bar — nowhere recoverable at all).
    expect(new URL(page.url()).pathname).toBe("/catalog");
  });

  test.describe("the ⌘[ / Alt+← shortcut", () => {
    // `page.keyboard.press` genuinely reaches the page here — measured, not
    // assumed: a throwaway instrumented probe against this same dev server
    // confirmed the keydown arrives at `window` with `defaultPrevented` false
    // beforehand and true after the app's own handler runs, so this is a real
    // exercise of `back_nav::install_back_shortcut`, not a driver no-op.
    // `back_nav`'s own module doc has the fuller, corrected version of this
    // finding: the working theory was that real desktop browsers never
    // deliver this keydown to page JS at all (which would make the point
    // moot), and that is unconfirmed for an interactive browser window but
    // measurably false for headless Chromium under Playwright specifically —
    // which is exactly what lets this suite exercise the real handler rather
    // than a driver no-op. What this suite confirms: the chord is
    // recognized, it walks real history the same way the button does, and it
    // stays out of the way of a focused field.

    test("Cmd+[ walks back through real in-app history @fast", async ({
      page,
      request,
    }) => {
      const card = await exactCard(request, SINGLE_FACE_QUERY);
      const catalogUrl = `/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`;
      await page.goto(catalogUrl);
      await hydrated(page);

      await page.locator(`a[href="/cards/${card.oracle_id}"]`).first().click();
      await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
      await hydrated(page);

      await page.keyboard.press("Meta+BracketLeft");
      await page.waitForURL(
        (url) => url.pathname === "/catalog" && url.search === new URL(catalogUrl, page.url()).search,
      );
    });

    test("Alt+ArrowLeft (the non-mac spelling) does the same @fast", async ({
      page,
      request,
    }) => {
      // `is_back_chord`'s platform split reads `navigator.platform`
      // (`palette::is_mac`), and this suite's own runner is a real macOS
      // Chromium — so without this override the mac branch is what is live
      // and Alt+ArrowLeft is correctly ignored (that failure mode was
      // caught running this test the first time). Spoofing the platform is
      // what makes this a genuine exercise of the *other* branch end to end
      // (chord parsing through to `history.back()`) rather than leaving it
      // covered only by `is_back_chord`'s own Rust unit tests.
      await page.addInitScript(() => {
        Object.defineProperty(window.navigator, "platform", {
          get: () => "Linux x86_64",
        });
      });

      const card = await exactCard(request, SINGLE_FACE_QUERY);
      const catalogUrl = `/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`;
      await page.goto(catalogUrl);
      await hydrated(page);

      await page.locator(`a[href="/cards/${card.oracle_id}"]`).first().click();
      await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
      await hydrated(page);

      await page.keyboard.press("Alt+ArrowLeft");
      await page.waitForURL(
        (url) => url.pathname === "/catalog" && url.search === new URL(catalogUrl, page.url()).search,
      );
    });

    test("stays out of the way of a focused field @fast", async ({
      page,
      request,
    }) => {
      // The card-detail page itself carries no text field, so the guard is
      // exercised on the catalog's own query bar — landing there is exactly
      // what a stray Cmd+[ over a typed-but-not-yet-submitted query must NOT
      // discard.
      const card = await exactCard(request, SINGLE_FACE_QUERY);
      await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}`);
      await hydrated(page);

      await page.locator(`a[href="/cards/${card.oracle_id}"]`).first().click();
      await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
      await hydrated(page);

      // Back onto the catalog is the shortcut's own effect, so first prove it
      // fires at all from a neutral focus (`body`)...
      await page.locator("body").click();
      await page.keyboard.press("Meta+BracketLeft");
      await page.waitForURL((url) => url.pathname === "/catalog");

      // ...then prove the identical chord is swallowed while a field owns the
      // keyboard: focus the query bar, retype the card's own navigation
      // (which the trigger no longer offers from here) is unnecessary — the
      // input merely needs focus and a harmless edit for the guard to have
      // something to protect.
      const query = page.locator("#catalog-query");
      await query.click();
      await query.fill("a stray edit the shortcut must not discard");
      await page.keyboard.press("Meta+BracketLeft");
      // Still on the catalog (no back-out-of-the-app), still carrying the
      // typed text — the shortcut did nothing at all rather than acting on
      // the wrong target.
      await expect(page.locator("#catalog-query")).toHaveValue(
        "a stray edit the shortcut must not discard",
      );
      expect(new URL(page.url()).pathname).toBe("/catalog");
    });

    // MAJOR (round 2 adversarial review): the chord used to fire straight
    // through an open overlay — a Dialog's own text fields aren't
    // `focus_is_editable`'s concern (focus is usually on a button inside it,
    // not a field), so nothing stopped `⌘[` from navigating the page out from
    // under a confirm dialog. Fixed the same way `⌘K` already handles this
    // (`palette::palette_chord_target`'s "swallow the chord, change nothing"
    // arm, gated on `components::ui::overlay_stack::is_empty()`): claim the
    // keystroke, but do nothing while any Dialog/Sheet/Popover is open — not
    // close it (that's Escape's job) and not navigate underneath it.
    //
    // Route-independent, per the reviewer's own framing: any shell-level
    // Dialog does, and the tree's delete confirm is the cheapest one
    // reachable without touching a card at all.
    test.describe("authed", () => {
      test.use({ storageState: AUTH_STATE });

      test("does nothing while an overlay is open — leaves the dialog alone, does not navigate @fast", async ({
        page,
        request,
      }) => {
        const id = await createCollection(request, scratchName("overlay-gate"));
        try {
          // Real prior history, not a cold `/my` load: the authed fallback
          // *is* `/my`, so a cold load's "would-be target" and "already
          // here" coincide and a broken gate would be invisible by URL
          // alone. Landing on `/my` via an in-app link from `/catalog`
          // leaves a real `history.back()` target one hop behind it, so an
          // unguarded chord has somewhere observably different to go.
          await page.goto("/catalog");
          await hydrated(page);
          await page.getByRole("link", { name: "My cards" }).click();
          await page.waitForURL("/my");
          await hydrated(page);

          const row = page.locator(`[data-tree-row-head="${id}"]`);
          await row.click({ button: "right" });
          const menu = page.locator("#context-menu-tree");
          const deleteItem = menu.locator('[role="menuitem"]', {
            hasText: "Delete…",
          });
          await expect(deleteItem).toBeVisible();
          await deleteItem.click();

          const dialog = page.locator('[role="dialog"]', { hasText: "Delete" });
          await expect(dialog).toBeVisible();

          await page.keyboard.press("Meta+BracketLeft");
          await page.waitForTimeout(300);

          // Neither effect the bug could produce happened: no navigation
          // back to /catalog, and the dialog is exactly as it was.
          expect(new URL(page.url()).pathname).toBe("/my");
          await expect(dialog).toBeVisible();
        } finally {
          await deleteCollection(request, id);
        }
      });
    });
  });
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

// ----------------------------------------------------------------- writes ---
//
// The have/want steppers (P6-054's semantics, lifted to
// `app/src/components/holding_stepper.rs` and reused here from the
// `/my/collections/:id` HERE-cell precedent). Isolation follows
// `collection-view.spec.ts`'s own convention: every write happens inside a
// `zz-e2e-…` scratch collection created via the API and deleted in a
// `finally`, so these tests never touch the shared dev-seed tree other specs
// (and other parallel workers) are reading at the same time.

let scratchSeq = 0;
function scratchName(prefix: string): string {
  return `zz-e2e-cd-${prefix}-w${test.info().workerIndex}-${++scratchSeq}`;
}

async function createCollection(
  request: APIRequestContext,
  name: string,
): Promise<string> {
  const res = await request.post("/api/collections", {
    data: { parent_id: null, kind: "binder", name, format: null },
  });
  expect(res.status(), `create ${name}`).toBe(200);
  return ((await res.json()) as { id: string }).id;
}

async function deleteCollection(request: APIRequestContext, id: string) {
  await request.post(`/api/collections/${id}/delete`, { data: {} });
}

async function addHave(
  request: APIRequestContext,
  collectionId: string,
  printingId: string,
  quantity: number,
  finish?: "nonfoil" | "foil" | "etched",
) {
  const res = await request.post(`/api/collections/${collectionId}/have`, {
    data: { printing_id: printingId, quantity, finish },
  });
  expect(res.status(), "add have").toBe(200);
}

async function addWant(
  request: APIRequestContext,
  collectionId: string,
  oracleId: string,
  quantity: number,
) {
  const res = await request.post(`/api/collections/${collectionId}/want`, {
    data: { oracle_id: oracleId, quantity },
  });
  expect(res.status(), "add want").toBe(200);
}

type OwnershipEntry = {
  collection_id: string;
  collection_name: string;
  printing_id: string;
  quantity: number;
  holding_id: string | null;
};
type WantEntry = {
  collection_id: string;
  collection_name: string;
  quantity: number;
  desire_id: string | null;
};
type Detail = { ownership: OwnershipEntry[] | null; wants: WantEntry[] | null };

async function cardDetail(
  request: APIRequestContext,
  oracleId: string,
): Promise<Detail> {
  const res = await request.get(`/api/cards/${oracleId}`);
  expect(res.status()).toBe(200);
  return (await res.json()) as Detail;
}

/// The header total sums *every* collection holding/wanting this card, not
/// just the scratch collection a test creates — the dev-seed tree and other
/// specs' own scratch collections (this suite's fixture-pool contention,
/// e2e-suite skill's own "KNOWN SUITE STATE" note) can already hold real
/// copies/wants of a well-known card like Lightning Bolt. So the header
/// assertions below compare against a **baseline read taken immediately
/// before this test's own write**, not a hardcoded absolute number — the
/// same reasoning `all-cards.spec.ts` applies to its own cross-checks.
function ownedSum(d: Detail): number {
  return (d.ownership ?? []).reduce((s, o) => s + o.quantity, 0);
}
function wantedSum(d: Detail): number {
  return (d.wants ?? []).reduce((s, w) => s + w.quantity, 0);
}

const ownershipRow = (page: import("@playwright/test").Page, collectionId: string) =>
  page.locator(
    `[data-testid="ownership-row"][data-collection-id="${collectionId}"]`,
  );
const wantRow = (page: import("@playwright/test").Page, collectionId: string) =>
  page.locator(`[data-testid="want-row"][data-collection-id="${collectionId}"]`);

test.describe("the ownership + wants steppers", () => {
  test.use({ storageState: AUTH_STATE });

  test("the have stepper edits Your copies in place, and a reload agrees @fast", async ({
    page,
    request,
  }) => {
    const card = await exactCard(request, SINGLE_FACE_QUERY);
    const baseline = ownedSum(await cardDetail(request, card.oracle_id));
    const scratch = await createCollection(request, scratchName("have"));
    try {
      await addHave(request, scratch, card.printing_id!, 2);
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const section = page.getByTestId("your-copies");
      await expect(section).toContainText(`Your copies · ${baseline + 2}`);
      const row = ownershipRow(page, scratch);
      await expect(row).toBeVisible();
      await expect(row.getByTestId("count-stepper-value")).toHaveText("2");

      await row.getByTestId("count-stepper-inc").click();
      await page.locator('[data-testid="card-name"]').click();

      await expect(row.getByTestId("count-stepper-value")).toHaveText("3");
      // The header total follows the same commit — a local delta, not a
      // refetch (a refetch here would remount every row mid-toast).
      await expect(section).toContainText(`Your copies · ${baseline + 3}`);
      await expect(async () => {
        const after = await cardDetail(request, card.oracle_id);
        const line = after.ownership!.find((o) => o.collection_id === scratch);
        expect(line?.quantity).toBe(3);
      }).toPass({ timeout: 10_000 });

      // Persisted, not just optimistic.
      await page.reload();
      await hydrated(page);
      await expect(page.getByTestId("your-copies")).toContainText(
        `Your copies · ${baseline + 3}`,
      );
      await expect(
        ownershipRow(page, scratch).getByTestId("count-stepper-value"),
      ).toHaveText("3");
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  test("committing a have to zero removes it, and Undo restores it @fast", async ({
    page,
    request,
  }) => {
    // A card none of this file's *total*-asserting tests touch (see
    // `ownedSum`'s doc) — this test's own assertions are all row/API-scoped
    // by `scratch`, so sharing a card with `multigrain` below is fine, but
    // it must not be `SINGLE_FACE_QUERY`/`ADVENTURE_QUERY`, which the
    // have/want-adjust tests read a header *total* off of concurrently.
    const card = await firstCard(request, DFC_KEYWORDS_QUERY);
    const scratch = await createCollection(request, scratchName("zero"));
    try {
      await addHave(request, scratch, card.printing_id!, 1);
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const row = ownershipRow(page, scratch);
      await row.getByTestId("count-stepper-dec").click();
      await page.locator('[data-testid="card-name"]').click();

      // The stepper is withdrawn — a plain dash, not a live 0 a further +/-
      // could post against a row `remove_holding` already deleted.
      await expect(row.getByTestId("here-count")).toHaveText("—");
      await expect(row.locator('[data-testid="count-stepper"]')).toHaveCount(0);

      const toast = page.locator('[data-name="Toast"]', { hasText: "Removed" });
      await expect(toast).toContainText(`Removed ${card.name} (1 copy)`);
      await expect(async () => {
        const after = await cardDetail(request, card.oracle_id);
        expect(after.ownership!.find((o) => o.collection_id === scratch)).toBeUndefined();
      }).toPass({ timeout: 10_000 });

      // Undo reverses it through the same move ledger the collection view's
      // HERE cell uses — a real ledger undo, not a client-side re-add.
      await toast.getByRole("button", { name: "Undo" }).click();
      await expect(row.getByTestId("count-stepper-value")).toHaveText("1");
      await expect(async () => {
        const after = await cardDetail(request, card.oracle_id);
        const line = after.ownership!.find((o) => o.collection_id === scratch);
        expect(line?.quantity).toBe(1);
      }).toPass({ timeout: 10_000 });
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  test("the want stepper edits Your wants in place, and a reload agrees @fast", async ({
    page,
    request,
  }) => {
    // Exclusive to this test within the file (see `ownedSum`'s doc) — the
    // header total this test reads must not be perturbed by another test's
    // concurrent write to the same card.
    const card = await firstCard(request, ADVENTURE_QUERY);
    const baseline = wantedSum(await cardDetail(request, card.oracle_id));
    const scratch = await createCollection(request, scratchName("want"));
    try {
      await addWant(request, scratch, card.oracle_id, 2);
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const section = page.getByTestId("your-wants");
      await expect(section).toContainText(`Your wants · ${baseline + 2}`);
      const row = wantRow(page, scratch);
      await expect(row.getByTestId("count-stepper-value")).toHaveText("2");

      await row.getByTestId("count-stepper-inc").click();
      await page.locator('[data-testid="card-name"]').click();

      await expect(row.getByTestId("count-stepper-value")).toHaveText("3");
      await expect(section).toContainText(`Your wants · ${baseline + 3}`);
      await expect(async () => {
        const after = await cardDetail(request, card.oracle_id);
        const line = after.wants!.find((w) => w.collection_id === scratch);
        expect(line?.quantity).toBe(3);
      }).toPass({ timeout: 10_000 });

      await page.reload();
      await hydrated(page);
      await expect(page.getByTestId("your-wants")).toContainText(
        `Your wants · ${baseline + 3}`,
      );
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  test("committing a want to zero removes it politely, with no Undo offered @fast", async ({
    page,
    request,
  }) => {
    // Desires carry no ledger (`shared::QuickAddReceipt`'s own doc: a `+
    // Want` is confirmed but never undoable) — a committed zero here is a
    // direct delete, unlike the have stepper's reversible move.
    //
    // `DFC_QUERY`, not `SINGLE_FACE_QUERY`/`ADVENTURE_QUERY`: this test's own
    // assertions are row/API-scoped, but a shared card must still avoid the
    // two header-*total*-asserting tests above (see `ownedSum`'s doc).
    const card = await firstCard(request, DFC_QUERY);
    const scratch = await createCollection(request, scratchName("wantzero"));
    try {
      await addWant(request, scratch, card.oracle_id, 1);
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const row = wantRow(page, scratch);
      await row.getByTestId("count-stepper-dec").click();
      await page.locator('[data-testid="card-name"]').click();

      await expect(row.getByTestId("here-count")).toHaveText("—");
      const toast = page.locator('[data-name="Toast"]', {
        hasText: `Removed ${card.name} from wants`,
      });
      await expect(toast).toBeVisible();
      // No Undo action — the whole point of "politely": a confirmation, not
      // a promise this operation cannot keep.
      await expect(toast.getByRole("button", { name: "Undo" })).toHaveCount(0);

      await expect(async () => {
        const after = await cardDetail(request, card.oracle_id);
        expect(after.wants!.find((w) => w.collection_id === scratch)).toBeUndefined();
      }).toPass({ timeout: 10_000 });
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  test("a have cell spanning more than one finish refuses the stepper, with the standard message @fast", async ({
    page,
    request,
  }) => {
    // This test's own assertions are row/API-scoped (see `ownedSum`'s doc);
    // shares `DFC_KEYWORDS_QUERY` with `zero` above rather than a header-
    // total-asserting test's card.
    const card = await firstCard(request, DFC_KEYWORDS_QUERY);
    const scratch = await createCollection(request, scratchName("multigrain"));
    try {
      // Two `holdings` rows behind one (collection, printing) cell — same
      // printing, different finish — so `holding_id` comes back `None` and
      // the cell cannot say which grain a typed number would mean.
      await addHave(request, scratch, card.printing_id!, 2, "nonfoil");
      await addHave(request, scratch, card.printing_id!, 1, "foil");

      const before = await cardDetail(request, card.oracle_id);
      const line = before.ownership!.find((o) => o.collection_id === scratch);
      expect(line?.holding_id, "sanity: the fixture is genuinely multi-grain").toBeNull();
      expect(line?.quantity).toBe(3);

      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const row = ownershipRow(page, scratch);
      await expect(row.locator('[data-testid="count-stepper"]')).toHaveCount(0);
      const cell = row.getByTestId("here-count");
      await expect(cell).toHaveText("3");
      await expect(cell).toHaveAttribute(
        "title",
        "several finishes or conditions here — edit them individually",
      );
    } finally {
      await deleteCollection(request, scratch);
    }
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

  test("the keywords row follows the swap instead of showing the front's union beside the back's text @fast", async ({
    page,
    request,
  }) => {
    // `keywords` is card-level (Scryfall's union of both faces' ability
    // words) and the raw `card_faces` jsonb this page's flip control reads
    // never carried a per-face equivalent (app/src/ingest/extract.rs,
    // `ORACLE_FACE_KEYS` excludes it) — so pairing the unioned row with a
    // flipped-to back face used to show the front's keywords beside the
    // back's oracle text. The honest-minimal fix pairs the row with the
    // front face only, and hides it once flipped.
    const card = await firstCard(request, DFC_KEYWORDS_QUERY);
    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);

    const keywords = page.getByTestId("card-keywords");
    await expect(keywords).toBeVisible();
    await expect(keywords).toContainText("Transform");
    await expect(keywords).toContainText("First strike");

    await page.getByTestId("card-flip").click();
    await expect(page.getByTestId("card-name")).not.toHaveText(card.faces[0].name);
    // The row is gone entirely on the back face — not relabeled, not still
    // showing the front's list. Absence, not a stale copy.
    await expect(keywords).toHaveCount(0);

    // ...and it's back once flipped to the front again.
    await page.getByTestId("card-flip").click();
    await expect(page.getByTestId("card-keywords")).toBeVisible();
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

  test("the hover preview resets to the front on a close and reopen @fast", async ({
    page,
    request,
  }) => {
    // `PreviewBody` mounts once and stays mounted (the lazy-mount latch, see
    // its own doc comment) behind `hovered` — so its `face` signal is a
    // single long-lived piece of state, not one created fresh per open. The
    // module doc for `PreviewBody` says "each starting at the front"; before
    // this task's fix the mounted-once instance carried a flip across a
    // close/reopen instead, contradicting it.
    const card = await firstCard(request, DFC_QUERY);
    await page.goto(`/catalog?q=${encodeURIComponent(DFC_QUERY)}&view=list`);
    await hydrated(page);

    const trigger = page.getByTestId("card-preview-trigger").first();
    await trigger.hover();
    const hover = page.locator("[data-testid=card-preview-hover]").first();
    await expect(hover).toBeVisible();
    await expect(hover).toContainText(card.faces[0].name);

    await hover.getByTestId("card-flip").click();
    await expect(hover).toContainText(card.faces[1].name);

    // Close: move well away from both the trigger and the popover, and
    // outlast the 150 ms close-intent timer.
    await page.mouse.move(0, 0);
    await expect(hover).toBeHidden();

    // Reopen the same trigger. The body never unmounted (it never does), but
    // the face must not have carried the flip through the close.
    await trigger.hover();
    await expect(hover).toBeVisible();
    await expect(hover).toContainText(card.faces[0].name);
  });
});

// -------------------------------------------------------- the printings table ---
//
// cards.rs's `Printings`: every row reuses `CardPreview` (hover-card on
// desktop, same as the catalog list view's rows), the table caps at ~20 rows
// and scrolls inside that cap, and a row is a real link back to this same
// page with `?printing=<id>` set — which `CardDetailBody` reads to pick that
// printing's own art and float it to the top of this very list.

/// Heavily reprinted — well over the ~20-row cap, so the cap/scroll
/// assertions have real overflow to measure. Any card with 2+ distinct
/// printings works for the hover/click tests; this one also fixes the
/// "long list" fixture so one card serves every test in this section.
const LONG_PRINTINGS_QUERY = "Sol Ring";

type Printing = {
  id: string;
  set_code: string | null;
  set_name: string | null;
  image_uri: string | null;
};

async function cardPrintings(
  request: APIRequestContext,
  oracleId: string,
): Promise<Printing[]> {
  const res = await request.get(`/api/cards/${oracleId}`);
  expect(res.status()).toBe(200);
  const body = (await res.json()) as { printings: Printing[] };
  return body.printings;
}

/// The table's own scroll container — `TableWrapper`, the immediate parent
/// of the `card-printings` table — not the document (e2e-suite skill: an
/// `overflow-auto` wrapper absorbs the overflow, so `document.documentElement`
/// never moves and would make a document-level assertion vacuous).
const printingsWrapper = (page: import("@playwright/test").Page) =>
  page.locator('[data-testid="card-printings"]').locator("xpath=..");

const printingRow = (page: import("@playwright/test").Page, printingId: string) =>
  page.locator(`[data-testid="printing-row"][data-printing-id="${printingId}"]`);

test.describe("the printings table", () => {
  test("hovering a row opens a preview with that printing's own art @fast", async ({
    page,
    request,
  }) => {
    const card = await exactCard(request, LONG_PRINTINGS_QUERY);
    const printings = await cardPrintings(request, card.oracle_id);
    // Not index 0 — the page's default hero is *already* that printing's
    // art before anything is hovered, so a passing assertion there couldn't
    // tell "the row's own art" apart from "whatever the page shows anyway".
    const target = printings.find((p, i) => i > 0 && p.image_uri);
    expect(
      target,
      "no second printing with art in the fixture — need a different query",
    ).toBeTruthy();

    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);

    const row = printingRow(page, target!.id);
    await expect(row).toBeVisible();
    // Scoped by the hover card's own id (now the *printing* id, not the
    // shared oracle_id — see `CardPreview`'s `id` prop doc comment): every
    // row on this page shares one oracle_id, so without that override every
    // row's hover card would collide on the same DOM id.
    const hoverBody = page.locator(
      `[data-testid=card-preview-hover]#hc-content-card-preview-${target!.id}`,
    );
    await expect(hoverBody).toBeHidden();
    await expect(page.getByText(card.name, { exact: true })).toHaveCount(1);

    // Force the row near the top of the viewport before hovering: the
    // hover_card's own CSS (`position-try-order: most-height`) opens the
    // panel on whichever side — above or below the row — has more room, so
    // a row sitting anywhere past the viewport's vertical midpoint would
    // legitimately (and correctly) open *above* it. Pinning the row here
    // guarantees "below" wins, which is what the geometry assertion below
    // actually needs to be meaningful rather than flaky.
    const absoluteTop = await row.evaluate(
      (el) => el.getBoundingClientRect().top + window.scrollY,
    );
    await page.evaluate((y) => window.scrollTo(0, Math.max(0, y - 60)), absoluteTop);

    // The row's own link (cell 1, "Set"), not the row's bounding-box center:
    // the row is whole-width clickable via a real anchor in *every* cell now
    // (WKWebView doesn't honor `position` on `<tr>`, so the row can no
    // longer be one stretched overlay — see cards.rs's `Printings` module
    // doc), but only cell 1 is wired to `CardPreview`'s hover affordance.
    // Cells 2–4 are plain duplicate navigation links.
    await row.getByTestId("printing-row-link").hover();
    await expect(hoverBody).toBeVisible(); // 150 ms hover intent
    await expect(hoverBody).toContainText(card.name);
    await expect(hoverBody.locator("img")).toHaveAttribute("src", target!.image_uri!);
    // A preview is not navigation.
    expect(new URL(page.url()).pathname).toBe(`/cards/${card.oracle_id}`);

    // The geometry regression this whole test exists to pin (round-2
    // adversarial review): `CardPreview`'s trigger used to be given only
    // the invisible full-row overlay anchor as children, which — being
    // `position: absolute` — contributes nothing to the trigger's auto
    // height, so the anchor CSS positioned the panel "below" a *zero-height*
    // box sitting at the row's top edge, opening the panel over the hovered
    // row and the next several. The row's real (visible, in-flow) content
    // now lives inside the trigger too, giving it the row's actual height —
    // asserted here as "the panel's top is at or below the row's bottom
    // edge", not merely "the panel is visible" (which the old, broken
    // geometry would also have satisfied). `toPass`: the CSS anchor
    // positioning engine's chosen fallback can lag a frame behind the
    // panel becoming `:popover-open`, so the very first bounding-box read
    // is occasionally still mid-settle.
    const EPSILON = 2;
    await expect(async () => {
      const rowBox = await row.boundingBox();
      const panelBox = await hoverBody.boundingBox();
      expect(rowBox).toBeTruthy();
      expect(panelBox).toBeTruthy();
      expect(panelBox!.y).toBeGreaterThanOrEqual(rowBox!.y + rowBox!.height - EPSILON);
    }).toPass({ timeout: 2000 });
  });

  test("a long printings list is capped and scrolls; a short one is not @fast", async ({
    page,
    request,
  }) => {
    const long = await exactCard(request, LONG_PRINTINGS_QUERY);
    await page.goto(`/cards/${long.oracle_id}`);
    await hydrated(page);

    const longDims = await printingsWrapper(page).evaluate((el) => ({
      scrollHeight: el.scrollHeight,
      clientHeight: el.clientHeight,
    }));
    // Bounded (the ~20-row cap, not "however tall 137 rows happen to be")
    // *and* actually scrollable inside that bound.
    expect(longDims.scrollHeight).toBeGreaterThan(longDims.clientHeight);

    // `DFC_KEYWORDS_QUERY` ("Kruin Outlaw") has 3 printings — comfortably
    // under the cap, so its table should show every row uncapped.
    const short = await firstCard(request, DFC_KEYWORDS_QUERY);
    await page.goto(`/cards/${short.oracle_id}`);
    await hydrated(page);

    const shortDims = await printingsWrapper(page).evaluate((el) => ({
      scrollHeight: el.scrollHeight,
      clientHeight: el.clientHeight,
    }));
    expect(shortDims.scrollHeight).toBeLessThanOrEqual(shortDims.clientHeight);
  });

  test("clicking a row navigates to that printing and lists it first @fast", async ({
    page,
    request,
  }) => {
    const card = await exactCard(request, LONG_PRINTINGS_QUERY);
    const printings = await cardPrintings(request, card.oracle_id);
    const target = printings.find((p, i) => i > 0 && p.image_uri);
    expect(
      target,
      "no second printing with art in the fixture — need a different query",
    ).toBeTruthy();

    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);
    // The hero art before the click — proof the assertion below is a real
    // change, not the page having shown this printing's art all along.
    const heroBefore = await page.locator("img").first().getAttribute("src");
    expect(heroBefore).not.toBe(target!.image_uri);

    const row = printingRow(page, target!.id);
    await row.scrollIntoViewIfNeeded();
    await row.click();

    await page.waitForURL(
      (url) =>
        url.pathname === `/cards/${card.oracle_id}` &&
        url.searchParams.get("printing") === target!.id,
    );
    // Same route, no remount (query-only navigation) — the hero art and the
    // table order both re-derive from the new `?printing=` without a second
    // page load.
    await expect(page.locator("img").first()).toHaveAttribute(
      "src",
      target!.image_uri!,
    );
    await expect(page.getByTestId("printing-row").first()).toHaveAttribute(
      "data-printing-id",
      target!.id,
    );
  });

  // -------------------------------------------- printing flips and history ---
  //
  // The maintainer report this pins: #158 made each row click a *pushed*
  // history entry, and #156's Back pops history one entry at a time — so a
  // visit that flips through several printings needed one Back per printing
  // before it ever left the page. Fix (cards.rs `CardPreview`'s
  // `replace_history` prop, wired true only here): a printing switch within
  // one card visit replaces the current entry instead of pushing a new one —
  // "history granularity is per session", the same rule `components::query_bar`
  // and `catalog::rail` already apply to `?q=` edits. `components::back_nav`'s
  // wrapped `history.replaceState` carries the *current* entry's own
  // `has_history` marker forward untouched, so the entry a reader landed on
  // by clicking into this card from the catalog keeps naming the catalog as
  // its own back-target no matter how many printings get flipped afterward.

  test("flipping between printings then one Back returns to the catalog — not to a printing @fast", async ({
    page,
    request,
  }) => {
    const card = await exactCard(request, LONG_PRINTINGS_QUERY);
    const printings = await cardPrintings(request, card.oracle_id);
    const withArt = printings.filter((p) => p.image_uri);
    expect(
      withArt.length,
      "need at least two printings with art to flip between",
    ).toBeGreaterThanOrEqual(2);
    const [first, second] = withArt;

    const catalogUrl = `/catalog?q=${encodeURIComponent(LONG_PRINTINGS_QUERY)}&view=list`;
    await page.goto(catalogUrl);
    await hydrated(page);

    // A real client-side navigation into the card — the one push that must
    // remain this visit's only back-target.
    await page.locator(`a[href="/cards/${card.oracle_id}"]`).first().click();
    await page.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
    await hydrated(page);

    const rowFirst = printingRow(page, first.id);
    await rowFirst.scrollIntoViewIfNeeded();
    await rowFirst.click();
    await page.waitForURL((url) => url.searchParams.get("printing") === first.id);

    const rowSecond = printingRow(page, second.id);
    await rowSecond.scrollIntoViewIfNeeded();
    await rowSecond.click();
    await page.waitForURL((url) => url.searchParams.get("printing") === second.id);

    // One Back — not three — leaves the card entirely and lands on the exact
    // search this visit started from. Against the unfixed base this instead
    // lands back on `?printing=<first.id>` (a second Back needed to reach
    // `?printing` unset, a third to finally reach `/catalog`), which is
    // exactly the bug report: Back walked the printings viewed instead of
    // leaving the page.
    await page.getByTestId("card-detail-back").click();
    await page.waitForURL(
      (url) =>
        url.pathname === "/catalog" &&
        url.search === new URL(catalogUrl, page.url()).search,
    );
  });

  test("a modifier-click on a printing row opens it in a new tab instead of navigating in place @fast", async ({
    page,
    context,
    request,
  }) => {
    // The replace-navigation the test above pins is a client-side intercept
    // (`ev.prevent_default()` + `navigate(..., { replace: true })`), and that
    // intercept must stay out of the way of a modified click — the same
    // guard `CardPreview::on_click` already applies for "open in a new tab".
    const card = await exactCard(request, LONG_PRINTINGS_QUERY);
    const printings = await cardPrintings(request, card.oracle_id);
    const target = printings.find((p, i) => i > 0 && p.image_uri);
    expect(
      target,
      "no second printing with art in the fixture — need a different query",
    ).toBeTruthy();

    await page.goto(`/cards/${card.oracle_id}`);
    await hydrated(page);

    const row = printingRow(page, target!.id);
    await row.scrollIntoViewIfNeeded();

    const [popup] = await Promise.all([
      context.waitForEvent("page"),
      row.click({ modifiers: ["Meta"] }),
    ]);
    // The popup starts at "about:blank" — wait for the real navigation to
    // land rather than `waitForLoadState()`, which can resolve against that
    // initial blank document.
    await popup.waitForURL((url) => url.pathname === `/cards/${card.oracle_id}`);
    const popupUrl = new URL(popup.url());
    expect(popupUrl.pathname).toBe(`/cards/${card.oracle_id}`);
    expect(popupUrl.searchParams.get("printing")).toBe(target!.id);
    await popup.close();

    // The original tab never navigated at all — the modifier click was left
    // to the browser's native "open in a new tab" default, not swallowed by
    // the same-page replace intercept.
    const originalUrl = new URL(page.url());
    expect(originalUrl.pathname).toBe(`/cards/${card.oracle_id}`);
    expect(originalUrl.searchParams.has("printing")).toBe(false);
  });

  test.describe("touch", () => {
    test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

    test("the table scrolls inside its own cap without moving the page @fast", async ({
      page,
      request,
    }) => {
      const card = await exactCard(request, LONG_PRINTINGS_QUERY);
      await page.goto(`/cards/${card.oracle_id}`);
      await hydrated(page);

      const wrapper = printingsWrapper(page);
      await expect(wrapper).toBeVisible();
      const before = await wrapper.evaluate((el) => el.scrollTop);
      await wrapper.evaluate((el) => {
        el.scrollTop = 400;
      });
      const after = await wrapper.evaluate((el) => el.scrollTop);
      // The container itself absorbed the scroll...
      expect(after).toBeGreaterThan(before);
      // ...and the page underneath did not move — a scrollable region inside
      // the page, not a hijack of the page's own scroll.
      expect(await page.evaluate(() => window.scrollY)).toBe(0);
    });
  });
});

test("hovering a list row opens a preview without changing the URL @fast", async ({
  page,
  request,
}) => {
  // Exact card, not `.first()`: the substring "Lightning Bolt" now also
  // matches "Emeritus of Conflict // Lightning Bolt" (alphabetically first),
  // whose hover body would still satisfy a `toContainText` check on the
  // substring and mask that the wrong row was ever hovered.
  const card = await exactCard(request, SINGLE_FACE_QUERY);
  await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}&view=list`);
  await hydrated(page);

  // Scoped by the HoverCard's own id (`hc-content-card-preview-{oracle_id}`,
  // components/ui/hover_card.rs) — `.first()` would resolve to whichever of
  // the three "Lightning Bolt"-substring tiles sorts first, not necessarily
  // the one this test just hovered.
  const hoverBody = page.locator(
    `[data-testid=card-preview-hover]#hc-content-card-preview-${card.oracle_id}`,
  );
  await expect(hoverBody).toBeHidden();
  // Lazily mounted — and this is the assertion that actually says so: the
  // popover element itself is always in the DOM, so only the absence of a
  // *second* copy of the card name proves the body hasn't rendered. Eager
  // bodies made getByText(name).first() resolve to a hidden node.
  await expect(page.getByText(card.name, { exact: true })).toHaveCount(1);

  await triggerFor(page, card.oracle_id).hover();
  await expect(hoverBody).toBeVisible(); // 150 ms hover intent
  await expect(hoverBody).toContainText(card.name);
  // A preview is not navigation.
  expect(new URL(page.url()).pathname).toBe("/catalog");
});

test("a grid tile offers no hover preview — it is already the art @fast", async ({
  page,
}) => {
  // Any tile does: this assertion has no card-identity dependency (grid
  // tiles never get a hover card, `hover=false` at the call site), so
  // `.first()` over the substring match is fine as-is.
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
    request,
  }) => {
    // Exact card + oracle-id-scoped locators: the substring query now also
    // matches "Emeritus of Conflict // Lightning Bolt" and "Lightning Bolt //
    // Lightning Bolt" (the full-catalog bulk load), each mounting its own
    // sheet — an unscoped `[data-testid=card-preview-sheet][role=dialog]`
    // locator resolves to all three (Playwright strict-mode violation).
    const card = await exactCard(request, SINGLE_FACE_QUERY);
    await page.goto(`/catalog?q=${encodeURIComponent(SINGLE_FACE_QUERY)}`);
    await hydrated(page);

    await triggerFor(page, card.oracle_id).click();

    // The spread puts the testid on the backdrop as well as the panel; the
    // sheet's own id (`card-sheet-{oracle_id}`, app/src/cards.rs) is what
    // picks the one dialog out of however many the substring mounted.
    const sheet = page.locator(`#card-sheet-${card.oracle_id}[role=dialog]`);
    // `data-state`, not toBeVisible: the sheet slides in via a transform and
    // stays in the layout when closed, so a closed sheet is "visible" to
    // Playwright too (found by mutation — app-ui Findings).
    await expect(sheet).toHaveAttribute("data-state", "open");
    await expect(sheet).toContainText(card.name);
    // The tap was intercepted: still on the catalog, not the detail page.
    expect(new URL(page.url()).pathname).toBe("/catalog");

    // ...and the sheet is how you get to the page from here.
    await sheet.getByTestId("card-preview-full-details").click();
    await page.waitForURL((url) => url.pathname.startsWith("/cards/"));
    await expect(page.getByTestId("card-name")).toContainText(card.name);
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

  test("the sheet resets to the front on a close and reopen @fast", async ({
    page,
    request,
  }) => {
    // Same defect, the sheet's own affordance: `PreviewBody` stays mounted
    // across a close (the sheet slides out over 300 ms rather than unmount),
    // so without an explicit reset the flip state would ride along to the
    // next open too.
    const card = await firstCard(request, DFC_QUERY);
    await page.goto(`/catalog?q=${encodeURIComponent(DFC_QUERY)}`);
    await hydrated(page);

    const trigger = page.getByTestId("card-preview-trigger").first();
    await trigger.click();
    const sheet = page.locator("[data-testid=card-preview-sheet][role=dialog]");
    await expect(sheet).toHaveAttribute("data-state", "open");
    await expect(sheet).toContainText(card.faces[0].name);

    await sheet.getByTestId("card-flip").click();
    await expect(sheet).toContainText(card.faces[1].name);

    // Close via Escape (the sheet's own dismissal, same signal a backdrop tap
    // or the close button would flip) and reopen the same trigger.
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveAttribute("data-state", "closed");

    await trigger.click();
    await expect(sheet).toHaveAttribute("data-state", "open");
    await expect(sheet).toContainText(card.faces[0].name);
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
    // ...and no sheet opened either — a hover is not a tap. Asserted as "zero
    // open sheets" rather than one specific locator's `data-state`: the
    // substring query mounts one sheet per matching tile (the full-catalog
    // bulk load put three "Lightning Bolt"-named cards on this query), so
    // "none opened" has to hold across all of them, not just whichever one
    // an unscoped locator happens to resolve to.
    await expect(
      page.locator('[data-testid=card-preview-sheet][data-state="open"]'),
    ).toHaveCount(0);
  });
});
